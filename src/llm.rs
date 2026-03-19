//! LLM client that proxies through MCP server.
//!
//! Replaces direct Anthropic API calls with MCP-proxied requests,
//! enabling multi-provider support via `LiteLLM` on the server.

use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::focus::{Focus, format_focus_context};
use crate::mcp_client::{ChatContent, ChatMessage, ChatRequest, ChatResponse, McpClient, ToolCall};
use crate::memory::Memory;
use crate::scheduler::{Scheduler, format_schedule_context};
use crate::setup::SETUP_COMPLETE_MARKER;
use crate::tools::{Tool, execute_tool_local, get_schedule_tool_definitions, is_schedule_tool};

/// Result of a setup-aware chat session.
pub struct SetupChatResult {
    /// The response text from the LLM.
    pub response: String,
    /// Whether `complete_setup` was called during the conversation.
    pub setup_completed: bool,
}

/// Tool execution backend.
#[derive(Clone)]
pub enum ToolBackend {
    /// Local filesystem access (Mac or standalone Pi with local vault)
    Local { vault_path: std::path::PathBuf },
    /// Remote MCP server (Pi thin client connecting to Mac)
    Mcp { client: McpClient },
}

/// LLM client with tool execution support.
pub struct Llm {
    mcp_client: McpClient,
    model: String,
    tool_backend: ToolBackend,
    memory: Option<Arc<Memory>>,
    focus: Option<Arc<Focus>>,
    scheduler: Option<Arc<Scheduler>>,
}

impl Clone for Llm {
    fn clone(&self) -> Self {
        Self {
            mcp_client: self.mcp_client.clone(),
            model: self.model.clone(),
            tool_backend: self.tool_backend.clone(),
            memory: self.memory.clone(),
            focus: self.focus.clone(),
            scheduler: self.scheduler.clone(),
        }
    }
}

impl Llm {
    /// Create an LLM client from config with optional memory.
    ///
    /// # Errors
    ///
    /// Returns an error if MCP configuration is not present in config.
    #[allow(dead_code)] // Kept for backward compatibility
    pub fn from_config_with_memory(config: &Config, memory: Option<Arc<Memory>>) -> Result<Self> {
        Self::from_config_with_context(config, memory, None)
    }

    /// Create an LLM client from config with optional memory and focus.
    ///
    /// # Errors
    ///
    /// Returns an error if MCP configuration is not present in config.
    pub fn from_config_with_context(
        config: &Config,
        memory: Option<Arc<Memory>>,
        focus: Option<Arc<Focus>>,
    ) -> Result<Self> {
        Self::from_config_full(config, memory, focus, None)
    }

    /// Create an LLM client from config with all context options.
    ///
    /// # Errors
    ///
    /// Returns an error if MCP configuration is not present in config.
    pub fn from_config_full(
        config: &Config,
        memory: Option<Arc<Memory>>,
        focus: Option<Arc<Focus>>,
        scheduler: Option<Arc<Scheduler>>,
    ) -> Result<Self> {
        let mcp_config = config
            .mcp
            .as_ref()
            .context("MCP configuration required for LLM proxy")?;

        let mcp_client = McpClient::from_config(mcp_config);

        let tool_backend = config.vault.as_ref().map_or_else(
            || ToolBackend::Mcp {
                client: mcp_client.clone(),
            },
            |vault| ToolBackend::Local {
                vault_path: vault.path.clone(),
            },
        );

        // Get model from [llm] section, fall back to [claude] for backward compatibility
        let model = config
            .llm
            .as_ref()
            .map_or_else(|| config.claude.model.clone(), |l| l.model.clone());

        Ok(Self {
            mcp_client,
            model,
            tool_backend,
            memory,
            focus,
            scheduler,
        })
    }

    /// Get the vault path description for the system prompt.
    fn vault_description(&self) -> String {
        match &self.tool_backend {
            ToolBackend::Local { vault_path } => vault_path.display().to_string(),
            ToolBackend::Mcp { .. } => "your Mac (via MCP)".to_string(),
        }
    }

    /// Execute a tool using the configured backend.
    async fn execute_tool(&self, name: &str, input: &Value, user_id: Option<i64>) -> String {
        // Handle schedule tools specially
        if is_schedule_tool(name) {
            if let Some(scheduler) = &self.scheduler {
                let uid = user_id.unwrap_or(0);
                let result = crate::tools::schedule::execute(name, input, scheduler, uid);

                // Handle immediate schedule execution
                if let Some(schedule_id) = result.strip_prefix("SCHEDULE_RUN_NOW:") {
                    return self.execute_schedule_now(scheduler, schedule_id, uid).await;
                }

                return result;
            }
            return "Error: Scheduler not configured".to_string();
        }

        match &self.tool_backend {
            ToolBackend::Local { vault_path } => execute_tool_local(name, input, vault_path).await,
            ToolBackend::Mcp { client } => client
                .call_tool(name, input)
                .await
                .unwrap_or_else(|e| format!("Error: {e}")),
        }
    }

    /// Execute a schedule immediately and record the run.
    #[allow(clippy::cognitive_complexity)]
    async fn execute_schedule_now(
        &self,
        scheduler: &Arc<Scheduler>,
        schedule_info: &str,
        user_id: i64,
    ) -> String {
        use crate::scheduler::RunStatus;

        // Parse "schedule_id:schedule_name"
        let parts: Vec<&str> = schedule_info.splitn(2, ':').collect();
        let schedule_id = parts.first().unwrap_or(&"");

        // Get the schedule
        let schedule = match scheduler.get(schedule_id) {
            Ok(Some(s)) => s,
            Ok(None) => return format!("Error: Schedule '{schedule_id}' not found"),
            Err(e) => return format!("Error getting schedule: {e}"),
        };

        let schedule_name = &schedule.name;

        // Record run start
        let run_id = match scheduler.record_run_start(schedule_id, user_id) {
            Ok(id) => id,
            Err(e) => return format!("Error recording run start: {e}"),
        };

        // Execute the schedule's prompt
        // We use a simple single-turn chat to avoid recursion issues
        let prompt = &schedule.prompt;
        tracing::info!("Executing schedule '{schedule_name}' immediately");

        let result = self.execute_schedule_prompt(prompt, user_id).await;

        // Record run completion
        match &result {
            Ok(response) => {
                let summary = if response.len() > 500 {
                    format!("{}...", &response[..497])
                } else {
                    response.clone()
                };

                if let Err(e) = scheduler.record_run_complete(
                    run_id,
                    schedule_id,
                    RunStatus::Success,
                    Some(&summary),
                    None,
                ) {
                    tracing::error!("Failed to record run completion: {e}");
                }

                format!("Executed schedule '{schedule_name}' successfully.\n\nResult:\n{response}")
            }
            Err(e) => {
                let error_msg = format!("{e}");
                if let Err(record_err) = scheduler.record_run_complete(
                    run_id,
                    schedule_id,
                    RunStatus::Error,
                    None,
                    Some(&error_msg),
                ) {
                    tracing::error!("Failed to record run error: {record_err}");
                }

                format!("Schedule '{schedule_name}' failed: {error_msg}")
            }
        }
    }

    /// Execute a schedule's prompt without full conversation context.
    ///
    /// This is a simplified execution path to avoid recursion when
    /// running schedules immediately from within a tool call.
    /// Uses a minimal system prompt to avoid the recursive call chain.
    async fn execute_schedule_prompt(&self, prompt: &str, _user_id: i64) -> Result<String> {
        // Get vault tools only (no schedule tools to avoid recursion)
        let tools = match &self.tool_backend {
            ToolBackend::Local { .. } => crate::tools::get_tool_definitions(),
            ToolBackend::Mcp { client } => client.get_tool_definitions().await?,
        };

        // Use a minimal system prompt to avoid recursion through build_system_prompt
        let system = format!(
            "You are Ludolph, a helpful assistant with access to the user's Obsidian vault at {}. \
             Execute the following scheduled task. Be concise in your response.",
            self.vault_description()
        );

        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: ChatContent::Text(system),
            },
            ChatMessage {
                role: "user".to_string(),
                content: ChatContent::Text(prompt.to_string()),
            },
        ];

        // Simple single-turn execution (no tool loop to avoid recursion)
        let request = ChatRequest {
            model: self.model.clone(),
            messages,
            tools: Some(Self::tools_to_json(&tools)),
        };

        let response = self.mcp_client.chat(&request).await?;
        Ok(response.content.unwrap_or_default())
    }

    /// Get tool definitions from the configured backend.
    async fn get_tools(&self) -> Result<Vec<Tool>> {
        let mut tools = match &self.tool_backend {
            ToolBackend::Local { .. } => crate::tools::get_tool_definitions(),
            ToolBackend::Mcp { client } => client.get_tool_definitions().await?,
        };

        // Add schedule tools if scheduler is available
        if self.scheduler.is_some() {
            tools.extend(get_schedule_tool_definitions());
        }

        Ok(tools)
    }

    /// Build system prompt with memory, focus, schedule, and vault context.
    async fn build_system_prompt(&self, user_id: Option<i64>) -> String {
        let memory_context = if self.memory.is_some() {
            "\n\nYou have access to conversation history with this user. \
             Recent messages are included below. For older conversations, \
             search in .lu/conversations/ within the vault."
        } else {
            ""
        };

        let focus_context = self.get_focus_context(user_id);
        let schedule_context = self.get_schedule_context(user_id);

        let lu_context = self
            .load_lu_context()
            .await
            .map_or_else(String::new, |content| {
                format!("\n\n## Vault Context (from Lu.md)\n\n{content}")
            });

        format!(
            "You are Ludolph, a helpful assistant with access to the user's Obsidian vault at {}. \
             You can read files and search the vault to answer questions about their notes.\n\n\
             FORMATTING: Your responses go to Telegram. Keep them clean and readable:\n\
             - Plain text only. No markdown syntax (no **, no `, no #).\n\
             - Short paragraphs. Break up walls of text.\n\
             - Simple lists when helpful. Use bullet points sparingly.\n\
             - No emojis unless the user uses them first.\n\
             - Be concise. Get to the point.\n\
             - If you have multiple questions, ask one at a time.{}{}{}{}",
            self.vault_description(),
            memory_context,
            focus_context,
            schedule_context,
            lu_context
        )
    }

    /// Get focus context for system prompt.
    fn get_focus_context(&self, user_id: Option<i64>) -> String {
        if let (Some(focus), Some(uid)) = (&self.focus, user_id) {
            match focus.get_focus(uid) {
                Ok(files) if !files.is_empty() => format_focus_context(&files),
                _ => String::new(),
            }
        } else {
            String::new()
        }
    }

    /// Get schedule context for system prompt.
    fn get_schedule_context(&self, user_id: Option<i64>) -> String {
        if let (Some(scheduler), Some(uid)) = (&self.scheduler, user_id) {
            match scheduler.get_active_schedules(uid) {
                Ok(schedules) if !schedules.is_empty() => format_schedule_context(&schedules),
                _ => String::new(),
            }
        } else {
            String::new()
        }
    }

    /// Track file access after a `read_file` tool call.
    fn track_file_access(
        &self,
        user_id: Option<i64>,
        tool_name: &str,
        input: &Value,
        result: &str,
    ) {
        // Only track read_file calls that succeeded
        if tool_name != "read_file" || result.starts_with("Error:") {
            return;
        }

        let Some(focus) = &self.focus else { return };
        let Some(uid) = user_id else { return };
        let Some(path) = input.get("path").and_then(|v| v.as_str()) else {
            return;
        };

        if let Err(e) = focus.touch(uid, path, result) {
            tracing::warn!("Failed to track file focus: {e}");
        }
    }

    /// Load conversation history from memory.
    fn load_conversation_history(&self, user_id: Option<i64>) -> Vec<ChatMessage> {
        let mut messages = Vec::new();

        if let (Some(memory), Some(uid)) = (&self.memory, user_id) {
            let context = memory.get_context(uid).unwrap_or_default();
            for msg in context {
                messages.push(ChatMessage {
                    role: msg.role.clone(),
                    content: ChatContent::Text(msg.content.clone()),
                });
            }
        }

        messages
    }

    /// Store message in memory.
    fn store_message(&self, user_id: Option<i64>, role: &str, content: &str) {
        if let (Some(memory), Some(uid)) = (&self.memory, user_id) {
            let _ = memory.add_message(uid, role, content);
        }
    }

    /// Clear all messages for a user.
    ///
    /// Used to start fresh conversations (e.g., `/setup`).
    pub fn clear_user_memory(&self, user_id: i64) {
        if let Some(memory) = &self.memory {
            let _ = memory.clear_user(user_id);
        }
    }

    /// Try to load Lu.md content from the vault.
    async fn load_lu_context(&self) -> Option<String> {
        let result = self
            .execute_tool("read_file", &serde_json::json!({"path": "Lu.md"}), None)
            .await;

        if result.contains("Error:") || result.contains("not found") || result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    /// Convert tools to JSON format for API.
    fn tools_to_json(tools: &[Tool]) -> Vec<Value> {
        tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    }
                })
            })
            .collect()
    }

    /// Process tool calls and return results.
    async fn process_tool_calls(
        &self,
        tool_calls: &[ToolCall],
        user_id: Option<i64>,
    ) -> Vec<Value> {
        let mut results = Vec::new();

        for tc in tool_calls {
            let input: Value = serde_json::from_str(&tc.function.arguments)
                .unwrap_or_else(|_| Value::Object(Map::default()));

            tracing::debug!("Executing tool: {}", tc.function.name);
            tracing::trace!("Tool input: {:?}", input);

            let result = self.execute_tool(&tc.function.name, &input, user_id).await;
            tracing::trace!("Tool {} returned {} bytes", tc.function.name, result.len());

            // Track file access for focus layer
            self.track_file_access(user_id, &tc.function.name, &input, &result);

            results.push(serde_json::json!({
                "type": "tool_result",
                "tool_use_id": tc.id,
                "content": result,
            }));
        }

        results
    }

    /// Chat with the LLM, handling tool calls.
    ///
    /// If `user_id` is provided and memory is configured, conversation history
    /// will be included in context and the exchange will be stored.
    ///
    /// # Errors
    ///
    /// Returns an error if the MCP server is unreachable or returns an error.
    pub async fn chat(&self, user_message: &str, user_id: Option<i64>) -> Result<String> {
        let tools = self.get_tools().await?;
        let mut messages = self.prepare_messages(user_message, user_id).await;

        tracing::debug!(
            "Starting chat with {} messages, {} tools",
            messages.len(),
            tools.len()
        );

        loop {
            let response = self.call_llm(&messages, &tools).await?;

            if self
                .handle_tool_calls(&response, &mut messages, user_id)
                .await
            {
                continue;
            }

            let content = response.content.unwrap_or_default();
            tracing::debug!("Chat complete, returning {} chars", content.len());

            self.store_message(user_id, "assistant", &content);
            return Ok(content);
        }
    }

    /// Prepare messages for a chat request.
    async fn prepare_messages(&self, user_message: &str, user_id: Option<i64>) -> Vec<ChatMessage> {
        let system = self.build_system_prompt(user_id).await;
        let mut messages = self.load_conversation_history(user_id);

        messages.insert(
            0,
            ChatMessage {
                role: "system".to_string(),
                content: ChatContent::Text(system),
            },
        );

        messages.push(ChatMessage {
            role: "user".to_string(),
            content: ChatContent::Text(user_message.to_string()),
        });

        self.store_message(user_id, "user", user_message);
        messages
    }

    /// Call the LLM with the current messages and tools.
    async fn call_llm(&self, messages: &[ChatMessage], tools: &[Tool]) -> Result<ChatResponse> {
        let request = ChatRequest {
            model: self.model.clone(),
            messages: messages.to_vec(),
            tools: Some(Self::tools_to_json(tools)),
        };

        tracing::debug!("Calling MCP chat endpoint");
        let response = self.mcp_client.chat(&request).await?;
        tracing::debug!("LLM usage: {:?}", response.usage);
        Ok(response)
    }

    /// Handle tool calls from a response. Returns true if tool calls were processed.
    async fn handle_tool_calls(
        &self,
        response: &ChatResponse,
        messages: &mut Vec<ChatMessage>,
        user_id: Option<i64>,
    ) -> bool {
        let Some(tool_calls) = &response.tool_calls else {
            return false;
        };

        if tool_calls.is_empty() {
            return false;
        }

        tracing::debug!("Received {} tool calls, continuing loop", tool_calls.len());

        messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: ChatContent::Blocks(vec![serde_json::json!({
                "type": "tool_use",
                "tool_calls": tool_calls,
            })]),
        });

        let results = self.process_tool_calls(tool_calls, user_id).await;
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: ChatContent::Blocks(results),
        });

        true
    }

    /// Chat with streaming response support.
    ///
    /// Similar to `chat()` but calls the provided callback with accumulated text
    /// as the response streams in. For now, falls back to non-streaming.
    ///
    /// # Errors
    ///
    /// Returns an error if the MCP server is unreachable or returns an error.
    #[allow(dead_code)] // Kept for backward compatibility
    pub async fn chat_streaming<F>(
        &self,
        user_message: &str,
        user_id: Option<i64>,
        on_text: F,
    ) -> Result<String>
    where
        F: Fn(&str) + Send + Sync,
    {
        // For now, fall back to non-streaming
        // TODO: Implement SSE client for streaming
        let result = self.chat(user_message, user_id).await?;
        on_text(&result);
        Ok(result)
    }

    /// Chat with cancellation and new-message detection support.
    ///
    /// Cancellation is checked at yield points:
    /// - Before each LLM call
    /// - Between tool executions (after each tool completes)
    ///
    /// Note: During a single HTTP request to the MCP proxy, cancellation cannot
    /// interrupt mid-flight. Multi-tool conversations provide more frequent
    /// cancellation opportunities.
    ///
    /// # Returns
    /// - `Ok(Some(response))` - completed successfully
    /// - `Ok(None)` - cancelled by token OR new messages detected
    /// - `Err(e)` - error occurred
    ///
    /// # Errors
    ///
    /// Returns an error if the MCP server is unreachable or returns an error.
    #[allow(clippy::cognitive_complexity)]
    pub async fn chat_cancellable<F, C>(
        &self,
        user_message: &str,
        user_id: Option<i64>,
        cancel_token: CancellationToken,
        check_new_messages: C,
        on_text: F,
    ) -> Result<Option<String>>
    where
        F: Fn(&str) + Send + Sync,
        C: Fn() -> bool + Send,
    {
        let tools = self.get_tools().await?;
        let mut messages = self.prepare_messages(user_message, user_id).await;

        tracing::debug!(
            "Starting cancellable chat with {} messages, {} tools",
            messages.len(),
            tools.len()
        );

        loop {
            // Check for cancellation before starting LLM request
            if cancel_token.is_cancelled() {
                tracing::debug!("Chat cancelled before LLM call");
                return Ok(None);
            }

            // Check for new messages before starting LLM request
            if check_new_messages() {
                tracing::debug!("New messages detected before LLM call");
                return Ok(None);
            }

            // Make the LLM call
            // Note: This HTTP request cannot be interrupted mid-flight
            let response = self.call_llm(&messages, &tools).await?;

            // Handle tool calls with cancellation checks between each tool
            if let Some(tool_calls) = &response.tool_calls {
                if !tool_calls.is_empty() {
                    tracing::debug!("Received {} tool calls", tool_calls.len());

                    // Add assistant message with tool calls
                    messages.push(ChatMessage {
                        role: "assistant".to_string(),
                        content: ChatContent::Blocks(vec![serde_json::json!({
                            "type": "tool_use",
                            "tool_calls": tool_calls,
                        })]),
                    });

                    // Execute tools with cancellation checks between each
                    let mut results = Vec::new();
                    for tc in tool_calls {
                        // Check for cancellation between tools
                        if cancel_token.is_cancelled() {
                            tracing::debug!("Chat cancelled between tool executions");
                            return Ok(None);
                        }
                        if check_new_messages() {
                            tracing::debug!("New messages detected between tool executions");
                            return Ok(None);
                        }

                        let input: Value = serde_json::from_str(&tc.function.arguments)
                            .unwrap_or_else(|_| Value::Object(Map::default()));

                        tracing::debug!("Executing tool: {}", tc.function.name);
                        let result = self.execute_tool(&tc.function.name, &input, user_id).await;

                        // Track file access for focus layer
                        self.track_file_access(user_id, &tc.function.name, &input, &result);

                        results.push(serde_json::json!({
                            "type": "tool_result",
                            "tool_use_id": tc.id,
                            "content": result,
                        }));
                    }

                    messages.push(ChatMessage {
                        role: "user".to_string(),
                        content: ChatContent::Blocks(results),
                    });

                    continue;
                }
            }

            let content = response.content.unwrap_or_default();
            tracing::debug!("Chat complete, returning {} chars", content.len());

            // Call the callback with final result
            on_text(&content);

            self.store_message(user_id, "assistant", &content);
            return Ok(Some(content));
        }
    }

    /// Chat with a custom system prompt, tracking if `complete_setup` is called.
    ///
    /// Used for setup mode where we need a different system prompt and
    /// need to know when setup completes.
    ///
    /// Uses memory to maintain conversation context across setup messages.
    ///
    /// # Errors
    ///
    /// Returns an error if the MCP server is unreachable or returns an error.
    pub async fn chat_with_system(
        &self,
        user_message: &str,
        system_prompt: &str,
        user_id: Option<i64>,
    ) -> Result<SetupChatResult> {
        let tools = self.get_tools().await?;

        let mut messages = self.load_conversation_history(user_id);

        // Add system message at start
        messages.insert(
            0,
            ChatMessage {
                role: "system".to_string(),
                content: ChatContent::Text(system_prompt.to_string()),
            },
        );

        // Add user message
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: ChatContent::Text(user_message.to_string()),
        });

        self.store_message(user_id, "user", user_message);

        let mut setup_completed = false;

        loop {
            let request = ChatRequest {
                model: self.model.clone(),
                messages: messages.clone(),
                tools: Some(Self::tools_to_json(&tools)),
            };

            let response = self.mcp_client.chat(&request).await?;
            tracing::debug!("LLM usage: {:?}", response.usage);

            if let Some(tool_calls) = &response.tool_calls {
                if !tool_calls.is_empty() {
                    messages.push(ChatMessage {
                        role: "assistant".to_string(),
                        content: ChatContent::Blocks(vec![serde_json::json!({
                            "type": "tool_use",
                            "tool_calls": tool_calls,
                        })]),
                    });

                    let mut results = Vec::new();
                    for tc in tool_calls {
                        let input: Value = serde_json::from_str(&tc.function.arguments)
                            .unwrap_or_else(|_| Value::Object(Map::default()));

                        let result = self.execute_tool(&tc.function.name, &input, user_id).await;

                        // Track file access for focus layer
                        self.track_file_access(user_id, &tc.function.name, &input, &result);

                        // Check if this is the complete_setup tool
                        if tc.function.name == "complete_setup"
                            && result.contains(SETUP_COMPLETE_MARKER)
                        {
                            setup_completed = true;
                        }

                        results.push(serde_json::json!({
                            "type": "tool_result",
                            "tool_use_id": tc.id,
                            "content": result,
                        }));
                    }

                    messages.push(ChatMessage {
                        role: "user".to_string(),
                        content: ChatContent::Blocks(results),
                    });

                    continue;
                }
            }

            let content = response.content.unwrap_or_default();
            self.store_message(user_id, "assistant", &content);

            return Ok(SetupChatResult {
                response: content,
                setup_completed,
            });
        }
    }
}
