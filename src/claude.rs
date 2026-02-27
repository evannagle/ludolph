//! Claude API client with tool execution loop.
//!
//! Uses the anthropic-sdk-rust crate for API interaction.

use std::sync::Arc;

use anthropic_sdk::{
    client::Anthropic,
    types::{
        ContentBlockDelta, MessageStreamEvent, Tool, ToolInputSchema,
        messages::{
            ContentBlock, ContentBlockParam, MessageContent, MessageCreateBuilder, MessageParam,
            Role,
        },
    },
};
use anyhow::{Context, Result};
use futures::StreamExt;
use serde_json::{Map, Value};

use crate::config::Config;
use crate::mcp_client::McpClient;
use crate::memory::Memory;
use crate::setup::SETUP_COMPLETE_MARKER;
use crate::tools::execute_tool_local;

/// Result of a setup-aware chat session.
pub struct SetupChatResult {
    /// The response text from Claude.
    pub response: String,
    /// Whether `complete_setup` was called during the conversation.
    pub setup_completed: bool,
}

/// Tool execution backend.
#[derive(Clone)]
enum ToolBackend {
    /// Local filesystem access (Mac or standalone Pi with local vault)
    Local { vault_path: std::path::PathBuf },
    /// Remote MCP server (Pi thin client connecting to Mac)
    Mcp { client: McpClient },
}

/// Pending tool call being accumulated during streaming.
struct PendingToolCall {
    id: String,
    name: String,
    input_json: String,
}

/// Claude API client with tool execution support.
pub struct Claude {
    client: Arc<Anthropic>,
    model: String,
    tool_backend: ToolBackend,
    memory: Option<Arc<Memory>>,
}

impl Clone for Claude {
    fn clone(&self) -> Self {
        Self {
            client: Arc::clone(&self.client),
            model: self.model.clone(),
            tool_backend: self.tool_backend.clone(),
            memory: self.memory.clone(),
        }
    }
}

impl Claude {
    /// Create a Claude client with optional memory.
    #[must_use]
    pub fn from_config_with_memory(config: &Config, memory: Option<Arc<Memory>>) -> Self {
        let tool_backend = config
            .mcp
            .as_ref()
            .map(|mcp_config| ToolBackend::Mcp {
                client: McpClient::from_config(mcp_config),
            })
            .or_else(|| {
                config.vault.as_ref().map(|vault| ToolBackend::Local {
                    vault_path: vault.path.clone(),
                })
            })
            .expect("Neither MCP nor vault configured");

        let client =
            Anthropic::new(&config.claude.api_key).expect("Failed to create Anthropic client");

        Self {
            client: Arc::new(client),
            model: config.claude.model.clone(),
            tool_backend,
            memory,
        }
    }

    /// Get the vault path description for the system prompt.
    fn vault_description(&self) -> String {
        match &self.tool_backend {
            ToolBackend::Local { vault_path } => vault_path.display().to_string(),
            ToolBackend::Mcp { .. } => "your Mac (via MCP)".to_string(),
        }
    }

    /// Execute a tool using the configured backend.
    async fn execute_tool(&self, name: &str, input: &Value) -> String {
        match &self.tool_backend {
            ToolBackend::Local { vault_path } => execute_tool_local(name, input, vault_path).await,
            ToolBackend::Mcp { client } => client
                .call_tool(name, input)
                .await
                .unwrap_or_else(|e| format!("Error: {e}")),
        }
    }

    /// Persist old messages from short-term memory to long-term vault storage.
    ///
    /// Called automatically when persist threshold is reached.
    async fn persist_memory(&self, user_id: i64) {
        let Some(memory) = &self.memory else {
            return;
        };

        // Get messages that need to be persisted
        let messages = match memory.get_messages_to_persist(user_id) {
            Ok(msgs) if !msgs.is_empty() => msgs,
            _ => return,
        };

        // Format messages for the MCP tool
        let formatted: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": m.role,
                    "content": m.content,
                    "timestamp": m.timestamp.to_rfc3339()
                })
            })
            .collect();

        let input = serde_json::json!({
            "messages": formatted,
            "user_id": user_id
        });

        // Call MCP tool to save
        let result = self.execute_tool("save_conversation", &input).await;
        tracing::debug!("Persist memory result: {}", result);

        // Mark as persisted and cleanup if successful
        if !result.contains("error") {
            if let Some(last) = messages.last() {
                let _ = memory.mark_persisted(user_id, &last.timestamp);
                let _ = memory.cleanup(user_id);
            }
        }
    }

    /// Get tool definitions from the configured backend, converted to SDK format.
    async fn get_tools(&self) -> Result<Vec<Tool>> {
        let tools = match &self.tool_backend {
            ToolBackend::Local { .. } => crate::tools::get_tool_definitions(),
            ToolBackend::Mcp { client } => client.get_tool_definitions().await?,
        };

        // Convert to SDK Tool format
        Ok(tools.into_iter().map(convert_tool).collect())
    }

    /// Build system prompt with memory and vault context.
    async fn build_system_prompt(&self) -> String {
        let memory_context = if self.memory.is_some() {
            "\n\nYou have access to conversation history with this user. \
             Recent messages are included below. For older conversations, \
             search in .lu/conversations/ within the vault."
        } else {
            ""
        };

        let lu_context = self
            .load_lu_context()
            .await
            .map_or_else(String::new, |content| {
                format!("\n\n## Vault Context (from Lu.md)\n\n{content}")
            });

        format!(
            "You are Ludolph, a helpful assistant with access to the user's Obsidian vault at {}. \
             You can read files and search the vault to answer questions about their notes. \
             Be concise and helpful.{}{}",
            self.vault_description(),
            memory_context,
            lu_context
        )
    }

    /// Load conversation history from memory.
    fn load_conversation_history(&self, user_id: Option<i64>) -> Vec<MessageParam> {
        let mut messages = Vec::new();

        if let (Some(memory), Some(uid)) = (&self.memory, user_id) {
            let context = memory.get_context(uid).unwrap_or_default();
            for msg in context {
                let role = if msg.role == "user" {
                    Role::User
                } else {
                    Role::Assistant
                };
                messages.push(MessageParam {
                    role,
                    content: MessageContent::Text(msg.content),
                });
            }
        }

        messages
    }

    /// Process Claude response blocks into assistant content and tool results.
    async fn process_response_blocks(
        &self,
        blocks: &[ContentBlock],
    ) -> (Vec<ContentBlockParam>, Vec<ContentBlockParam>, String) {
        let mut assistant_content = Vec::new();
        let mut tool_results = Vec::new();
        let mut final_text = String::new();

        for block in blocks {
            match block {
                ContentBlock::Text { text } => {
                    final_text.clone_from(text);
                    assistant_content.push(ContentBlockParam::Text { text: text.clone() });
                }
                ContentBlock::ToolUse { id, name, input } => {
                    tracing::debug!("Executing tool: {}", name);
                    tracing::trace!("Tool input: {:?}", input);

                    assistant_content.push(ContentBlockParam::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    });

                    let result = self.execute_tool(name, input).await;
                    tracing::trace!("Tool {} returned {} bytes", name, result.len());

                    tool_results.push(ContentBlockParam::ToolResult {
                        tool_use_id: id.clone(),
                        content: Some(result),
                        is_error: Some(false),
                    });
                }
                _ => {}
            }
        }

        (assistant_content, tool_results, final_text)
    }

    /// Store assistant message in memory and persist if needed.
    async fn store_assistant_message(&self, user_id: Option<i64>, text: &str) {
        if let (Some(memory), Some(uid)) = (&self.memory, user_id) {
            let _ = memory.add_message(uid, "assistant", text);

            if memory.should_persist(uid).unwrap_or(false) {
                self.persist_memory(uid).await;
            }
        }
    }

    /// Chat with optional user context for memory.
    ///
    /// If `user_id` is provided and memory is configured, conversation history
    /// will be included in context and the exchange will be stored.
    #[allow(clippy::too_many_lines)]
    /// Execute a single conversation turn (API call + response processing).
    async fn execute_turn(
        &self,
        system: &str,
        tools: &[Tool],
        messages: &[MessageParam],
    ) -> Result<(Vec<ContentBlockParam>, Vec<ContentBlockParam>, String)> {
        tracing::debug!(
            "Calling Claude API (turn {}, {} messages)",
            messages.len() / 2 + 1,
            messages.len()
        );

        let params = MessageCreateBuilder::new(&self.model, 4096)
            .system(system)
            .tools(tools.to_vec())
            .build();

        let mut params = params;
        params.messages = messages.to_vec();

        let response = self
            .client
            .messages()
            .create(params)
            .await
            .context("Failed to call Claude API")?;

        tracing::debug!(
            "Received Claude response with {} content blocks",
            response.content.len()
        );

        Ok(self.process_response_blocks(&response.content).await)
    }

    /// Execute the tool loop until no more tools need to be called.
    async fn execute_tool_loop(
        &self,
        system: &str,
        tools: &[Tool],
        messages: &mut Vec<MessageParam>,
    ) -> Result<String> {
        loop {
            let (assistant_content, tool_results, final_text) =
                self.execute_turn(system, tools, messages).await?;

            if tool_results.is_empty() {
                tracing::debug!(
                    "Conversation complete, returning {} chars",
                    final_text.len()
                );
                return Ok(final_text);
            }

            tracing::debug!(
                "Tool execution loop continuing with {} results",
                tool_results.len()
            );

            messages.push(MessageParam {
                role: Role::Assistant,
                content: MessageContent::Blocks(assistant_content),
            });
            messages.push(MessageParam {
                role: Role::User,
                content: MessageContent::Blocks(tool_results),
            });
        }
    }

    /// Chat with optional user context for memory.
    ///
    /// If `user_id` is provided and memory is configured, conversation history
    /// will be included in context and the exchange will be stored.
    pub async fn chat(&self, user_message: &str, user_id: Option<i64>) -> Result<String> {
        let tools = self.get_tools().await?;
        let system = self.build_system_prompt().await;

        // Load conversation history and add current message
        let mut messages = self.load_conversation_history(user_id);
        messages.push(MessageParam {
            role: Role::User,
            content: MessageContent::Text(user_message.to_string()),
        });

        // Store user message in memory
        if let (Some(memory), Some(uid)) = (&self.memory, user_id) {
            let _ = memory.add_message(uid, "user", user_message);
        }

        // Execute tool loop and get final response
        let final_text = self
            .execute_tool_loop(&system, &tools, &mut messages)
            .await?;

        // Store assistant response
        self.store_assistant_message(user_id, &final_text).await;

        Ok(final_text)
    }

    /// Chat with streaming response support.
    ///
    /// Similar to `chat()` but calls the provided callback with accumulated text
    /// as the response streams in. The callback receives the full text so far.
    #[allow(clippy::too_many_lines)]
    pub async fn chat_streaming<F>(
        &self,
        user_message: &str,
        user_id: Option<i64>,
        on_text: F,
    ) -> Result<String>
    where
        F: Fn(&str) + Send + Sync,
    {
        let tools = self.get_tools().await?;
        let system = self.build_system_prompt().await;

        // Load conversation history and add current message
        let mut messages = self.load_conversation_history(user_id);
        messages.push(MessageParam {
            role: Role::User,
            content: MessageContent::Text(user_message.to_string()),
        });

        // Store user message in memory
        if let (Some(memory), Some(uid)) = (&self.memory, user_id) {
            let _ = memory.add_message(uid, "user", user_message);
        }

        // Execute streaming tool loop
        let final_text = self
            .execute_streaming_tool_loop(&system, &tools, &mut messages, &on_text)
            .await?;

        // Store assistant response
        self.store_assistant_message(user_id, &final_text).await;

        Ok(final_text)
    }

    /// Execute the tool loop with streaming support.
    ///
    /// Streams text content to the callback as it arrives. When tool calls are
    /// detected, they are executed and the loop continues.
    #[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
    async fn execute_streaming_tool_loop<F>(
        &self,
        system: &str,
        tools: &[Tool],
        messages: &mut Vec<MessageParam>,
        on_text: &F,
    ) -> Result<String>
    where
        F: Fn(&str) + Send + Sync,
    {
        loop {
            let params = MessageCreateBuilder::new(&self.model, 4096)
                .system(system)
                .tools(tools.to_vec())
                .stream(true)
                .build();

            let mut params = params;
            params.messages.clone_from(messages);

            // Start streaming
            let mut stream = self
                .client
                .messages()
                .create_stream(params)
                .await
                .context("Failed to create streaming request")?;

            let mut accumulated_text = String::new();
            let mut assistant_content: Vec<ContentBlockParam> = Vec::new();
            let mut tool_results: Vec<ContentBlockParam> = Vec::new();
            let mut current_tool: Option<PendingToolCall> = None;

            // Process stream events
            while let Some(event_result) = stream.next().await {
                let event = event_result.context("Stream error")?;

                match event {
                    MessageStreamEvent::ContentBlockStart {
                        content_block: ContentBlock::ToolUse { id, name, input },
                        ..
                    } => {
                        // Initialize tool call tracking when a ToolUse block starts
                        current_tool = Some(PendingToolCall {
                            id: id.clone(),
                            name: name.clone(),
                            input_json: input.to_string(),
                        });
                    }
                    MessageStreamEvent::ContentBlockDelta { delta, .. } => {
                        match delta {
                            ContentBlockDelta::TextDelta { text } => {
                                accumulated_text.push_str(&text);
                                on_text(&accumulated_text);
                            }
                            ContentBlockDelta::InputJsonDelta { partial_json } => {
                                // Accumulate tool input JSON
                                if let Some(ref mut tool) = current_tool {
                                    tool.input_json.push_str(&partial_json);
                                }
                            }
                            _ => {}
                        }
                    }
                    MessageStreamEvent::ContentBlockStop { .. } => {
                        // Check if we have a completed tool call
                        if let Some(tool) = current_tool.take() {
                            tracing::debug!("Executing tool: {}", tool.name);

                            // Parse the accumulated JSON
                            let input: serde_json::Value =
                                serde_json::from_str(&tool.input_json).unwrap_or_default();

                            assistant_content.push(ContentBlockParam::ToolUse {
                                id: tool.id.clone(),
                                name: tool.name.clone(),
                                input: input.clone(),
                            });

                            let result = self.execute_tool(&tool.name, &input).await;
                            tracing::trace!("Tool {} returned {} bytes", tool.name, result.len());

                            tool_results.push(ContentBlockParam::ToolResult {
                                tool_use_id: tool.id,
                                content: Some(result),
                                is_error: Some(false),
                            });
                        }
                    }
                    MessageStreamEvent::MessageStop => {
                        break;
                    }
                    _ => {}
                }
            }

            // Add accumulated text to assistant content if any
            if !accumulated_text.is_empty() {
                assistant_content.insert(
                    0,
                    ContentBlockParam::Text {
                        text: accumulated_text.clone(),
                    },
                );
            }

            // If no tool calls, we're done
            if tool_results.is_empty() {
                tracing::debug!(
                    "Streaming complete, returning {} chars",
                    accumulated_text.len()
                );
                return Ok(accumulated_text);
            }

            // Continue tool loop
            tracing::debug!(
                "Streaming tool loop continuing with {} results",
                tool_results.len()
            );

            messages.push(MessageParam {
                role: Role::Assistant,
                content: MessageContent::Blocks(assistant_content),
            });
            messages.push(MessageParam {
                role: Role::User,
                content: MessageContent::Blocks(tool_results),
            });
        }
    }

    /// Chat with a custom system prompt, tracking if `complete_setup` is called.
    ///
    /// Used for setup mode where we need a different system prompt and
    /// need to know when setup completes.
    ///
    /// Uses memory to maintain conversation context across setup messages.
    pub async fn chat_with_system(
        &self,
        user_message: &str,
        system_prompt: &str,
        user_id: Option<i64>,
    ) -> Result<SetupChatResult> {
        let tools = self.get_tools().await?;

        // Load conversation context from memory (same as regular chat)
        let mut messages: Vec<MessageParam> = Vec::new();

        if let (Some(memory), Some(uid)) = (&self.memory, user_id) {
            let context = memory.get_context(uid).unwrap_or_default();
            for msg in context {
                let role = if msg.role == "user" {
                    Role::User
                } else {
                    Role::Assistant
                };
                messages.push(MessageParam {
                    role,
                    content: MessageContent::Text(msg.content),
                });
            }
        }

        // Add current user message
        messages.push(MessageParam {
            role: Role::User,
            content: MessageContent::Text(user_message.to_string()),
        });

        // Store user message in memory
        if let (Some(memory), Some(uid)) = (&self.memory, user_id) {
            let _ = memory.add_message(uid, "user", user_message);
        }

        let mut setup_completed = false;

        // Tool execution loop
        loop {
            let params = MessageCreateBuilder::new(&self.model, 4096)
                .system(system_prompt)
                .tools(tools.clone())
                .build();

            let mut params = params;
            params.messages.clone_from(&messages);

            let response = self
                .client
                .messages()
                .create(params)
                .await
                .context("Failed to call Claude API")?;

            // Process response
            let mut assistant_content: Vec<ContentBlockParam> = Vec::new();
            let mut tool_results: Vec<ContentBlockParam> = Vec::new();
            let mut final_text = String::new();

            for block in &response.content {
                match block {
                    ContentBlock::Text { text } => {
                        final_text.clone_from(text);
                        assistant_content.push(ContentBlockParam::Text { text: text.clone() });
                    }
                    ContentBlock::ToolUse { id, name, input } => {
                        assistant_content.push(ContentBlockParam::ToolUse {
                            id: id.clone(),
                            name: name.clone(),
                            input: input.clone(),
                        });

                        let result = self.execute_tool(name, input).await;

                        // Check if this is the complete_setup tool
                        if name == "complete_setup" && result.contains(SETUP_COMPLETE_MARKER) {
                            setup_completed = true;
                        }

                        tool_results.push(ContentBlockParam::ToolResult {
                            tool_use_id: id.clone(),
                            content: Some(result),
                            is_error: Some(false),
                        });
                    }
                    _ => {}
                }
            }

            if tool_results.is_empty() {
                // Store assistant response in memory
                if let (Some(memory), Some(uid)) = (&self.memory, user_id) {
                    let _ = memory.add_message(uid, "assistant", &final_text);
                }

                return Ok(SetupChatResult {
                    response: final_text,
                    setup_completed,
                });
            }

            // Add assistant message and tool results, continue loop
            messages.push(MessageParam {
                role: Role::Assistant,
                content: MessageContent::Blocks(assistant_content),
            });
            messages.push(MessageParam {
                role: Role::User,
                content: MessageContent::Blocks(tool_results),
            });
        }
    }

    /// Try to load Lu.md content from the vault for the system prompt.
    async fn load_lu_context(&self) -> Option<String> {
        let result = self
            .execute_tool("read_file", &serde_json::json!({"path": "Lu.md"}))
            .await;

        // Check if the result looks like an error
        if result.contains("Error:") || result.contains("not found") || result.is_empty() {
            None
        } else {
            Some(result)
        }
    }
}

/// Convert our tool definition to SDK Tool format.
fn convert_tool(tool: crate::tools::Tool) -> Tool {
    // Extract properties and required from the JSON schema
    let (properties, required) = extract_schema_parts(&tool.input_schema);

    Tool {
        name: tool.name,
        description: tool.description,
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties,
            required,
            additional: Map::new(),
        },
    }
}

/// Extract properties and required arrays from a JSON schema Value.
fn extract_schema_parts(schema: &Value) -> (Map<String, Value>, Vec<String>) {
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    (properties, required)
}
