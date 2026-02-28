//! MCP client for connecting to Mac's MCP server.
//!
//! This module provides HTTP client functionality for the Pi thin client
//! to communicate with the Mac's MCP server, including Wake-on-LAN support.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::process::Command;
use std::time::Duration;

use crate::config::McpConfig;
use crate::tools::Tool;

/// MCP client for communicating with the Mac's MCP server.
#[derive(Clone)]
pub struct McpClient {
    client: reqwest::Client,
    base_url: String,
    auth_token: String,
    mac_address: Option<String>,
}

#[derive(Serialize)]
struct ToolCallRequest {
    name: String,
    arguments: Value,
}

#[derive(Deserialize)]
struct ToolCallResponse {
    content: String,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct ToolDefinitionsResponse {
    tools: Vec<ToolDefinition>,
}

#[derive(Deserialize)]
struct ToolDefinition {
    name: String,
    description: String,
    input_schema: Value,
}

/// Chat request to the LLM proxy.
#[derive(Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Value>>,
}

/// A chat message.
#[derive(Serialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: ChatContent,
}

/// Content can be text or a list of content blocks.
#[derive(Serialize, Clone)]
#[serde(untagged)]
pub enum ChatContent {
    Text(String),
    Blocks(Vec<Value>),
}

/// Chat response from the LLM proxy.
#[derive(Deserialize)]
pub struct ChatResponse {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default)]
    pub usage: Value,
}

/// A tool call from the LLM.
#[derive(Deserialize, Clone)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ToolCallFunction,
}

/// Tool call function details.
#[derive(Deserialize, Clone)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

/// Error response from the chat endpoint.
#[derive(Deserialize)]
struct ChatError {
    error: String,
    message: String,
}

impl McpClient {
    /// Create a new MCP client from configuration.
    #[must_use]
    pub fn from_config(config: &McpConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url: config.url.trim_end_matches('/').to_string(),
            auth_token: config.auth_token.clone(),
            mac_address: config.mac_address.clone(),
        }
    }

    /// Check if the MCP server is reachable.
    pub async fn health_check(&self) -> Result<bool> {
        let response = self
            .client
            .get(format!("{}/health", self.base_url))
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .send()
            .await;

        response.map_or(Ok(false), |resp| Ok(resp.status().is_success()))
    }

    /// Send Wake-on-LAN packet to wake the Mac.
    pub fn wake_mac(&self) -> Result<()> {
        let mac_address = self.mac_address.as_ref().context(
            "Wake-on-LAN not configured.\n\n\
                     No MAC address found in config.\n\n\
                     Try:\n\
                     • Add 'mac_address' to [mcp] section in config\n\
                     • Find MAC address in Mac System Settings > Network\n\
                     • Format: aa:bb:cc:dd:ee:ff",
        )?;

        tracing::info!("Sending Wake-on-LAN packet to {}", mac_address);

        let status = Command::new("wakeonlan")
            .arg(mac_address)
            .status()
            .context(
                "Wake-on-LAN failed.\n\n\
                     Cannot execute 'wakeonlan' command.\n\n\
                     Try:\n\
                     • Install wakeonlan: apt install wakeonlan\n\
                     • Verify wakeonlan is in PATH\n\
                     • Check command availability: which wakeonlan",
            )?;

        if status.success() {
            tracing::info!("Wake-on-LAN packet sent successfully");
            Ok(())
        } else {
            anyhow::bail!(
                "Wake-on-LAN command failed (status: {status})\n\n\
                Try:\n\
                • Verify MAC address is correct\n\
                • Check network connectivity\n\
                • Ensure Mac is on same network\n\
                • Enable Wake-on-LAN in Mac System Settings"
            );
        }
    }

    /// Get tool definitions from the MCP server.
    pub async fn get_tool_definitions(&self) -> Result<Vec<Tool>> {
        let response = self
            .client
            .get(format!("{}/tools", self.base_url))
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .send()
            .await
            .map_err(|e| {
                Self::format_connection_error(&e, &self.base_url, "get tool definitions")
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();

            return Err(anyhow::anyhow!(
                "MCP server error ({status})\n\n\
                Error: {body}\n\n\
                Try:\n\
                • Check if the MCP server is running\n\
                • Verify authentication token in config\n\
                • Ensure MCP server is at: {}\n\
                • Check server logs for details",
                self.base_url
            ));
        }

        let defs: ToolDefinitionsResponse = response.json().await.context(
            "Failed to parse tool definitions from MCP server.\n\n\
                     The server may be running an incompatible version.\n\n\
                     Try:\n\
                     • Update the MCP server\n\
                     • Check server logs for errors\n\
                     • Verify server is responding correctly",
        )?;

        Ok(defs
            .tools
            .into_iter()
            .map(|t| Tool {
                name: t.name,
                description: t.description,
                input_schema: t.input_schema,
            })
            .collect())
    }

    /// Call a tool on the MCP server.
    ///
    /// If the server is unreachable and a MAC address is configured,
    /// this will attempt to wake the Mac and retry.
    #[allow(clippy::cognitive_complexity)]
    pub async fn call_tool(&self, name: &str, input: &Value) -> Result<String> {
        // First attempt
        let first_result = self.try_call_tool(name, input).await;
        if first_result.is_ok() {
            return first_result;
        }

        let first_error = first_result.unwrap_err();
        tracing::warn!("MCP call failed: {}", first_error);

        // Try Wake-on-LAN if we have a MAC address
        if self.mac_address.is_none() {
            return Err(first_error);
        }

        tracing::info!("Attempting Wake-on-LAN...");
        if let Err(wol_err) = self.wake_mac() {
            tracing::warn!("Wake-on-LAN failed: {}", wol_err);
            return Err(first_error);
        }

        // Wait for Mac to wake up
        tracing::info!("Waiting for Mac to wake up...");
        tokio::time::sleep(Duration::from_secs(10)).await;

        // Retry with longer timeout
        self.try_call_tool_with_retry(name, input, 3).await.context(
            "Failed to connect after Wake-on-LAN attempt.\n\n\
                     Try:\n\
                     • Wait longer for Mac to wake up\n\
                     • Check Mac power/network status manually\n\
                     • Verify Wake-on-LAN is enabled in Mac settings\n\
                     • Ensure Mac is on same network\n\
                     • Try pinging Mac to verify it's awake",
        )
    }

    async fn try_call_tool(&self, name: &str, input: &Value) -> Result<String> {
        tracing::debug!("MCP: Calling tool {} at {}", name, self.base_url);

        let request = ToolCallRequest {
            name: name.to_string(),
            arguments: input.clone(),
        };

        let response = self
            .client
            .post(format!("{}/tools/call", self.base_url))
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                Self::format_connection_error(&e, &self.base_url, &format!("call tool '{name}'"))
            })?;

        tracing::debug!("MCP: Received response with status {}", response.status());

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();

            let error_msg = match status.as_u16() {
                401 => format!(
                    "Authentication failed (401)\n\n\
                    Error: {body}\n\n\
                    Try:\n\
                    • Check authentication token in config\n\
                    • Verify token matches MCP server config\n\
                    • Regenerate token if expired"
                ),
                404 => format!(
                    "Tool not found (404)\n\n\
                    Error: Tool '{name}' not available\n\n\
                    Try:\n\
                    • Check tool name spelling\n\
                    • Verify MCP server version\n\
                    • List available tools with /tools endpoint"
                ),
                500..=599 => format!(
                    "MCP server error ({status})\n\n\
                    Error: {body}\n\n\
                    Try:\n\
                    • Check MCP server logs\n\
                    • Verify vault path is accessible\n\
                    • Restart MCP server if needed\n\
                    • Check server resource usage"
                ),
                _ => format!(
                    "MCP server error ({status})\n\n\
                    Error: {body}\n\n\
                    Try:\n\
                    • Check MCP server status\n\
                    • Verify request parameters\n\
                    • Check server logs for details"
                ),
            };

            return Err(anyhow::anyhow!(error_msg));
        }

        let result: ToolCallResponse = response.json().await.context(
            "Failed to parse tool call response from MCP server.\n\n\
                     The server may have returned invalid data.\n\n\
                     Try:\n\
                     • Check MCP server logs\n\
                     • Verify server is running correctly\n\
                     • Update MCP server if outdated",
        )?;

        if let Some(error) = result.error {
            return Err(anyhow::anyhow!(
                "Tool execution failed\n\n\
                Error: {error}\n\n\
                Try:\n\
                • Verify tool parameters are correct\n\
                • Check vault path exists and is accessible\n\
                • Ensure file/directory permissions are correct\n\
                • Use list_dir to verify paths"
            ));
        }

        Ok(result.content)
    }

    async fn try_call_tool_with_retry(
        &self,
        name: &str,
        input: &Value,
        max_retries: u32,
    ) -> Result<String> {
        let mut last_error = anyhow::anyhow!("All retries failed");

        for attempt in 0..max_retries {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_secs(5)).await;
            }

            match self.try_call_tool(name, input).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    tracing::warn!("Attempt {} failed: {e}", attempt + 1);
                    last_error = e;
                }
            }
        }

        Err(last_error)
    }

    /// Format connection errors with helpful suggestions.
    fn format_connection_error(error: &reqwest::Error, url: &str, action: &str) -> anyhow::Error {
        let msg = if error.is_timeout() {
            format!(
                "Unable to {action} via MCP server.\n\n\
                Error: Request timed out connecting to {url}\n\n\
                Try:\n\
                • Check if Mac is awake and reachable\n\
                • Verify network connectivity\n\
                • Ensure MCP server is running on Mac\n\
                • Check firewall settings\n\
                • Wake Mac with /wake command if configured"
            )
        } else if error.is_connect() {
            format!(
                "Unable to {action} via MCP server.\n\n\
                Error: Cannot connect to MCP server at {url}\n\n\
                Try:\n\
                • Check if MCP server is running on Mac\n\
                • Verify URL in config is correct\n\
                • Ensure Mac is on network and reachable\n\
                • Check firewall is not blocking port\n\
                • Try waking Mac with /wake command\n\
                • Ping Mac to verify network connectivity"
            )
        } else if error.is_status() {
            let status = error.status().map_or(String::new(), |s| format!(" ({s})"));
            format!(
                "Unable to {action} via MCP server.\n\n\
                Error: MCP server returned error{status}\n\n\
                Try:\n\
                • Check MCP server logs\n\
                • Verify authentication token\n\
                • Ensure server is running correctly\n\
                • Restart MCP server if needed"
            )
        } else if error.is_body() || error.is_decode() {
            format!(
                "Unable to {action} via MCP server.\n\n\
                Error: Invalid response from MCP server\n\n\
                Try:\n\
                • Check MCP server version compatibility\n\
                • Verify server is running correctly\n\
                • Check server logs for errors\n\
                • Update MCP server if outdated"
            )
        } else {
            format!(
                "Unable to {action} via MCP server.\n\n\
                Error: {error}\n\n\
                Try:\n\
                • Check network connectivity\n\
                • Verify MCP server is accessible\n\
                • Check server logs for details\n\
                • Ensure all configuration is correct"
            )
        };

        anyhow::anyhow!(msg)
    }

    /// Send a chat request to the MCP server's LLM proxy.
    pub async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse> {
        let response = self
            .client
            .post(format!("{}/chat", self.base_url))
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .header("Content-Type", "application/json")
            .json(request)
            .send()
            .await
            .map_err(|e| Self::format_connection_error(&e, &self.base_url, "chat"))?;

        let status = response.status();

        if !status.is_success() {
            let error: ChatError = response.json().await.unwrap_or(ChatError {
                error: "unknown".to_string(),
                message: format!("HTTP {status}"),
            });

            let msg = match error.error.as_str() {
                "auth_failed" => "Invalid API credentials. Check MCP server config.".to_string(),
                "budget_exceeded" => {
                    "Credits exhausted. Add credits or switch models.".to_string()
                }
                "rate_limit" => "Rate limited. Wait and retry.".to_string(),
                "invalid_input" => error.message,
                _ => error.message,
            };

            return Err(anyhow::anyhow!(msg));
        }

        response
            .json()
            .await
            .context("Failed to parse chat response")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_config_creates_client() {
        let config = McpConfig {
            url: "http://localhost:8200".to_string(),
            auth_token: "test-token".to_string(),
            mac_address: Some("aa:bb:cc:dd:ee:ff".to_string()),
        };

        let client = McpClient::from_config(&config);

        assert_eq!(client.base_url, "http://localhost:8200");
        assert_eq!(client.auth_token, "test-token");
        assert_eq!(client.mac_address, Some("aa:bb:cc:dd:ee:ff".to_string()));
    }

    #[test]
    fn from_config_strips_trailing_slash() {
        let config = McpConfig {
            url: "http://localhost:8200/".to_string(),
            auth_token: "test-token".to_string(),
            mac_address: None,
        };

        let client = McpClient::from_config(&config);

        assert_eq!(client.base_url, "http://localhost:8200");
    }
}
