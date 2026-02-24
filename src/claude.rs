//! Claude API client with tool execution loop.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::mcp_client::McpClient;
use crate::tools::execute_tool_local;

/// Tool execution backend.
#[derive(Clone)]
enum ToolBackend {
    /// Local filesystem access (Mac or standalone Pi with local vault)
    Local { vault_path: std::path::PathBuf },
    /// Remote MCP server (Pi thin client connecting to Mac)
    Mcp { client: McpClient },
}

#[derive(Clone)]
pub struct Claude {
    client: reqwest::Client,
    api_key: String,
    model: String,
    tool_backend: ToolBackend,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<Message>,
    tools: Vec<ToolDefinition>,
}

#[derive(Serialize, Deserialize, Clone)]
struct Message {
    role: String,
    content: serde_json::Value,
}

#[derive(Serialize)]
struct ToolDefinition {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

#[derive(Deserialize)]
struct ChatResponse {
    content: Vec<ContentBlock>,
    #[allow(dead_code)]
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct ApiError {
    error: ApiErrorDetail,
}

#[derive(Deserialize)]
struct ApiErrorDetail {
    message: String,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

impl Claude {
    /// Create a Claude client from config.
    ///
    /// If MCP config is present, uses remote MCP server for tool execution.
    /// Otherwise, uses local filesystem access.
    ///
    /// # Panics
    /// Panics if neither MCP nor vault is configured.
    #[must_use]
    pub fn from_config(config: &Config) -> Self {
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

        Self {
            client: reqwest::Client::new(),
            api_key: config.claude.api_key.clone(),
            model: config.claude.model.clone(),
            tool_backend,
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
    async fn execute_tool(&self, name: &str, input: &serde_json::Value) -> String {
        match &self.tool_backend {
            ToolBackend::Local { vault_path } => execute_tool_local(name, input, vault_path).await,
            ToolBackend::Mcp { client } => client
                .call_tool(name, input)
                .await
                .unwrap_or_else(|e| format!("Error: {e}")),
        }
    }

    /// Get tool definitions from the configured backend.
    async fn get_tools(&self) -> Result<Vec<crate::tools::Tool>> {
        match &self.tool_backend {
            ToolBackend::Local { .. } => Ok(crate::tools::get_tool_definitions()),
            ToolBackend::Mcp { client } => client.get_tool_definitions().await,
        }
    }

    pub async fn chat(&self, user_message: &str) -> Result<String> {
        let tools = self.get_tools().await?;

        let system = format!(
            "You are Ludolph, a helpful assistant with access to the user's Obsidian vault at {}. \
             You can read files and search the vault to answer questions about their notes. \
             Be concise and helpful.",
            self.vault_description()
        );

        let mut messages = vec![Message {
            role: "user".to_string(),
            content: serde_json::Value::String(user_message.to_string()),
        }];

        loop {
            let request = ChatRequest {
                model: self.model.clone(),
                max_tokens: 4096,
                system: system.clone(),
                messages: messages.clone(),
                tools: tools
                    .iter()
                    .map(|t| ToolDefinition {
                        name: t.name.clone(),
                        description: t.description.clone(),
                        input_schema: t.input_schema.clone(),
                    })
                    .collect(),
            };

            let response = self
                .client
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&request)
                .send()
                .await
                .context("Failed to connect to Claude API")?;

            let status = response.status();
            let body = response
                .text()
                .await
                .context("Failed to read response body")?;

            if !status.is_success() {
                // Try to parse error message
                if let Ok(api_error) = serde_json::from_str::<ApiError>(&body) {
                    anyhow::bail!("Claude API error: {}", api_error.error.message);
                }
                anyhow::bail!("Claude API error ({status}): {body}");
            }

            let chat_response: ChatResponse =
                serde_json::from_str(&body).context("Failed to parse Claude response")?;

            // Process response
            let mut assistant_content = Vec::new();
            let mut tool_results = Vec::new();
            let mut final_text = String::new();

            for block in chat_response.content {
                match block {
                    ContentBlock::Text { text } => {
                        final_text.clone_from(&text);
                        assistant_content.push(serde_json::json!({
                            "type": "text",
                            "text": text
                        }));
                    }
                    ContentBlock::ToolUse { id, name, input } => {
                        assistant_content.push(serde_json::json!({
                            "type": "tool_use",
                            "id": id,
                            "name": name,
                            "input": input
                        }));

                        let result = self.execute_tool(&name, &input).await;
                        tool_results.push(serde_json::json!({
                            "type": "tool_result",
                            "tool_use_id": id,
                            "content": result
                        }));
                    }
                }
            }

            if tool_results.is_empty() {
                return Ok(final_text);
            }

            // Add assistant message and tool results, continue loop
            messages.push(Message {
                role: "assistant".to_string(),
                content: serde_json::Value::Array(assistant_content),
            });
            messages.push(Message {
                role: "user".to_string(),
                content: serde_json::Value::Array(tool_results),
            });
        }
    }
}
