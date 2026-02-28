//! LLM client that proxies through MCP server.
//!
//! Replaces direct Anthropic API calls with MCP-proxied requests,
//! enabling multi-provider support via `LiteLLM` on the server.

use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::{Map, Value};

use crate::config::Config;
use crate::mcp_client::{ChatContent, ChatMessage, ChatRequest, McpClient, ToolCall};
use crate::memory::Memory;
use crate::setup::SETUP_COMPLETE_MARKER;
use crate::tools::{Tool, execute_tool_local};

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
}

impl Clone for Llm {
    fn clone(&self) -> Self {
        Self {
            mcp_client: self.mcp_client.clone(),
            model: self.model.clone(),
            tool_backend: self.tool_backend.clone(),
            memory: self.memory.clone(),
        }
    }
}

impl Llm {
    /// Create an LLM client from config with optional memory.
    ///
    /// # Errors
    ///
    /// Returns an error if MCP configuration is not present in config.
    pub fn from_config_with_memory(config: &Config, memory: Option<Arc<Memory>>) -> Result<Self> {
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
    async fn execute_tool(&self, name: &str, input: &Value) -> String {
        match &self.tool_backend {
            ToolBackend::Local { vault_path } => execute_tool_local(name, input, vault_path).await,
            ToolBackend::Mcp { client } => client
                .call_tool(name, input)
                .await
                .unwrap_or_else(|e| format!("Error: {e}")),
        }
    }

    /// Get tool definitions from the configured backend.
    async fn get_tools(&self) -> Result<Vec<Tool>> {
        match &self.tool_backend {
            ToolBackend::Local { .. } => Ok(crate::tools::get_tool_definitions()),
            ToolBackend::Mcp { client } => client.get_tool_definitions().await,
        }
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

    /// Try to load Lu.md content from the vault.
    async fn load_lu_context(&self) -> Option<String> {
        let result = self
            .execute_tool("read_file", &serde_json::json!({"path": "Lu.md"}))
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
    async fn process_tool_calls(&self, tool_calls: &[ToolCall]) -> Vec<Value> {
        let mut results = Vec::new();

        for tc in tool_calls {
            let input: Value = serde_json::from_str(&tc.function.arguments)
                .unwrap_or_else(|_| Value::Object(Map::default()));

            tracing::debug!("Executing tool: {}", tc.function.name);
            tracing::trace!("Tool input: {:?}", input);

            let result = self.execute_tool(&tc.function.name, &input).await;
            tracing::trace!("Tool {} returned {} bytes", tc.function.name, result.len());

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
        let system = self.build_system_prompt().await;

        let mut messages = self.load_conversation_history(user_id);

        // Add system message at start
        messages.insert(
            0,
            ChatMessage {
                role: "system".to_string(),
                content: ChatContent::Text(system),
            },
        );

        // Add user message
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: ChatContent::Text(user_message.to_string()),
        });

        // Store user message in memory
        self.store_message(user_id, "user", user_message);

        tracing::debug!(
            "Starting chat with {} messages, {} tools",
            messages.len(),
            tools.len()
        );

        // Tool loop
        loop {
            let request = ChatRequest {
                model: self.model.clone(),
                messages: messages.clone(),
                tools: Some(Self::tools_to_json(&tools)),
            };

            tracing::debug!("Calling MCP chat endpoint");
            let response = self.mcp_client.chat(&request).await?;
            tracing::debug!("LLM usage: {:?}", response.usage);

            if let Some(tool_calls) = &response.tool_calls {
                if !tool_calls.is_empty() {
                    tracing::debug!("Received {} tool calls, continuing loop", tool_calls.len());

                    // Add assistant message with tool calls
                    messages.push(ChatMessage {
                        role: "assistant".to_string(),
                        content: ChatContent::Blocks(vec![serde_json::json!({
                            "type": "tool_use",
                            "tool_calls": tool_calls,
                        })]),
                    });

                    // Execute tools and add results
                    let results = self.process_tool_calls(tool_calls).await;
                    messages.push(ChatMessage {
                        role: "user".to_string(),
                        content: ChatContent::Blocks(results),
                    });

                    continue;
                }
            }

            // No tool calls, return content
            let content = response.content.unwrap_or_default();
            tracing::debug!("Chat complete, returning {} chars", content.len());

            self.store_message(user_id, "assistant", &content);
            return Ok(content);
        }
    }

    /// Chat with streaming response support.
    ///
    /// Similar to `chat()` but calls the provided callback with accumulated text
    /// as the response streams in. For now, falls back to non-streaming.
    ///
    /// # Errors
    ///
    /// Returns an error if the MCP server is unreachable or returns an error.
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

                        let result = self.execute_tool(&tc.function.name, &input).await;

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
