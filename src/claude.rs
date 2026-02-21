use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::tools::execute_tool;

#[derive(Clone)]
pub struct Claude {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    max_tokens: u32,
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
    pub fn new() -> Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .or_else(|_| Self::load_from_config())
            .map_err(|_| anyhow::anyhow!("ANTHROPIC_API_KEY not set"))?;

        Ok(Self {
            client: reqwest::Client::new(),
            api_key,
            model: "claude-sonnet-4-20250514".to_string(),
        })
    }

    fn load_from_config() -> Result<String> {
        let config_path = directories::BaseDirs::new()
            .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?
            .home_dir()
            .join("ludolph/config.toml");

        let config_str = std::fs::read_to_string(config_path)?;
        let config: toml::Value = toml::from_str(&config_str)?;

        config["claude"]["api_key"]
            .as_str()
            .map(ToString::to_string)
            .ok_or_else(|| anyhow::anyhow!("api_key not found in config"))
    }

    pub async fn chat(&self, user_message: &str) -> Result<String> {
        let tools = crate::tools::get_tool_definitions();
        let mut messages = vec![Message {
            role: "user".to_string(),
            content: serde_json::Value::String(user_message.to_string()),
        }];

        loop {
            let request = ChatRequest {
                model: self.model.clone(),
                max_tokens: 4096,
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

            let response: ChatResponse = self
                .client
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&request)
                .send()
                .await?
                .json()
                .await?;

            // Process response
            let mut assistant_content = Vec::new();
            let mut tool_results = Vec::new();
            let mut final_text = String::new();

            for block in response.content {
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

                        let result = execute_tool(&name, &input).await;
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
