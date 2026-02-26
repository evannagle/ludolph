//! Claude API client with tool execution loop.
//!
//! Uses the anthropic-sdk-rust crate for API interaction.

use std::sync::Arc;

use anthropic_sdk::{
    client::Anthropic,
    types::{
        Tool, ToolInputSchema,
        messages::{
            ContentBlock, ContentBlockParam, MessageContent, MessageCreateBuilder, MessageParam,
            Role,
        },
    },
};
use anyhow::{Context, Result};
use serde_json::{Map, Value};

use crate::config::Config;
use crate::mcp_client::McpClient;
use crate::memory::Memory;
use crate::tools::execute_tool_local;

/// Tool execution backend.
#[derive(Clone)]
enum ToolBackend {
    /// Local filesystem access (Mac or standalone Pi with local vault)
    Local { vault_path: std::path::PathBuf },
    /// Remote MCP server (Pi thin client connecting to Mac)
    Mcp { client: McpClient },
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

    /// Chat with optional user context for memory.
    ///
    /// If `user_id` is provided and memory is configured, conversation history
    /// will be included in context and the exchange will be stored.
    pub async fn chat(&self, user_message: &str, user_id: Option<i64>) -> Result<String> {
        let tools = self.get_tools().await?;

        // Build system prompt with memory awareness
        let memory_context = if self.memory.is_some() {
            "\n\nYou have access to conversation history with this user. \
             Recent messages are included below. For older conversations, \
             search in .lu/conversations/ within the vault."
        } else {
            ""
        };

        let system = format!(
            "You are Ludolph, a helpful assistant with access to the user's Obsidian vault at {}. \
             You can read files and search the vault to answer questions about their notes. \
             Be concise and helpful.{}",
            self.vault_description(),
            memory_context
        );

        // Load conversation context from memory
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

        // Tool execution loop
        loop {
            let params = MessageCreateBuilder::new(&self.model, 4096)
                .system(&system)
                .tools(tools.clone())
                .build();

            // Manually set messages since builder doesn't support pre-built Vec<MessageParam>
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
                        tool_results.push(ContentBlockParam::ToolResult {
                            tool_use_id: id.clone(),
                            content: Some(result),
                            is_error: None,
                        });
                    }
                    _ => {}
                }
            }

            if tool_results.is_empty() {
                // Store assistant response in memory
                if let (Some(memory), Some(uid)) = (&self.memory, user_id) {
                    let _ = memory.add_message(uid, "assistant", &final_text);

                    // Check if we should persist to long-term storage
                    if memory.should_persist(uid).unwrap_or(false) {
                        self.persist_memory(uid).await;
                    }
                }
                return Ok(final_text);
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
