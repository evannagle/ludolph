//! MCP client for connecting to Mac's MCP server.
//!
//! This module provides HTTP client functionality for the Pi thin client
//! to communicate with the Mac's MCP server, including Wake-on-LAN support.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::process::Command;
use std::time::{Duration, Instant};

use crate::config::McpConfig;
use crate::tools::Tool;

/// MCP client for communicating with the Mac's MCP server.
#[derive(Clone)]
pub struct McpClient {
    client: reqwest::Client,
    base_url: String,
    fallback_url: Option<String>,
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
    /// Input schema is optional to handle malformed tools gracefully
    #[serde(default)]
    input_schema: Option<Value>,
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
#[derive(Serialize, Deserialize, Clone)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ToolCallFunction,
}

/// Tool call function details.
#[derive(Serialize, Deserialize, Clone)]
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

/// Information about a tool available on the MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    /// Name of the tool.
    pub name: String,
    /// Description of what the tool does.
    pub description: String,
}

/// Information about an available MCP in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRegistryEntry {
    /// Unique identifier for the MCP.
    pub name: String,
    /// Description of what the MCP provides.
    pub description: String,
    /// Whether this MCP is currently enabled for the user.
    pub enabled: bool,
}

/// Why the MCP connection failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisconnectReason {
    /// Server not reachable (connection refused, timeout).
    Unreachable,
    /// Authentication failed (401/403).
    AuthFailed,
    /// Other error.
    Error(String),
}

/// Status information from the MCP server.
#[derive(Debug, Clone)]
pub struct McpStatus {
    /// Whether the server is connected and responding.
    pub connected: bool,
    /// The endpoint URL being checked.
    pub endpoint: String,
    /// Round-trip latency in milliseconds.
    pub latency_ms: u64,
    /// Tools available on the server.
    pub tools: Vec<ToolInfo>,
    /// Whether we connected via fallback URL (primary failed).
    pub using_fallback: bool,
    /// Why the connection failed (if not connected).
    pub disconnect_reason: Option<DisconnectReason>,
}

/// Response from the /status endpoint.
#[derive(Deserialize)]
struct StatusResponse {
    #[allow(dead_code)]
    status: String,
    tools: Vec<ToolInfo>,
    #[allow(dead_code)]
    version: String,
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
            fallback_url: config
                .fallback_url
                .as_ref()
                .map(|u| u.trim_end_matches('/').to_string()),
            auth_token: config.auth_token.clone(),
            mac_address: config.mac_address.clone(),
        }
    }

    /// Check if a MAC address is configured for Wake-on-LAN.
    #[must_use]
    pub const fn has_mac_address(&self) -> bool {
        self.mac_address.is_some()
    }

    /// Get detailed status information from the MCP server.
    ///
    /// Returns status including connection state, latency, and available tools.
    /// Tries primary URL first, then fallback URL if configured.
    /// If both are unreachable, returns `McpStatus` with `connected: false`.
    pub async fn get_status(&self) -> McpStatus {
        // Try primary URL first
        let primary_result = self.try_status(&self.base_url, false).await;
        if let Ok(status) = primary_result {
            return status;
        }
        let mut last_reason = primary_result.unwrap_err();

        // Try fallback if configured
        if let Some(fallback) = &self.fallback_url {
            match self.try_status(fallback, true).await {
                Ok(status) => return status,
                Err(reason) => last_reason = reason,
            }
        }

        // Both failed - return status with reason from last attempt
        McpStatus {
            connected: false,
            endpoint: self.base_url.clone(),
            latency_ms: 0,
            tools: Vec::new(),
            using_fallback: false,
            disconnect_reason: Some(last_reason),
        }
    }

    /// Quick health check with 2 second timeout.
    ///
    /// Returns true if server responds with 200 OK, false otherwise.
    /// This is faster than `get_status()` and doesn't parse response body.
    #[allow(dead_code)] // Will be used in smart WoL implementation
    pub async fn quick_health_check(&self) -> bool {
        let response = self
            .client
            .get(format!("{}/health", self.base_url))
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .timeout(Duration::from_secs(2))
            .send()
            .await;

        matches!(response, Ok(r) if r.status().is_success())
    }

    /// Try to get status from a specific URL.
    /// Returns `Ok(McpStatus)` if connected, `Err(DisconnectReason)` if failed.
    async fn try_status(
        &self,
        base_url: &str,
        is_fallback: bool,
    ) -> Result<McpStatus, DisconnectReason> {
        let start = Instant::now();
        let endpoint = format!("{base_url}/status");

        let response = self
            .client
            .get(&endpoint)
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .timeout(Duration::from_secs(5))
            .send()
            .await;

        let latency_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

        self.parse_status_response(response, base_url, is_fallback, latency_ms)
            .await
    }

    /// Parse the status response into an `McpStatus`.
    #[allow(clippy::cognitive_complexity)]
    async fn parse_status_response(
        &self,
        response: Result<reqwest::Response, reqwest::Error>,
        base_url: &str,
        is_fallback: bool,
        latency_ms: u64,
    ) -> Result<McpStatus, DisconnectReason> {
        let resp = match response {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!("Failed to connect to {base_url}: {e}");
                return Err(DisconnectReason::Unreachable);
            }
        };

        let status_code = resp.status();
        if status_code == reqwest::StatusCode::UNAUTHORIZED
            || status_code == reqwest::StatusCode::FORBIDDEN
        {
            tracing::warn!("Auth failed for {base_url}: {status_code}");
            return Err(DisconnectReason::AuthFailed);
        }

        if !status_code.is_success() {
            tracing::warn!("Status endpoint {base_url} returned error: {status_code}");
            return Err(DisconnectReason::Error(status_code.to_string()));
        }

        match resp.json::<StatusResponse>().await {
            Ok(status) => Ok(McpStatus {
                connected: true,
                endpoint: base_url.to_string(),
                latency_ms,
                tools: status.tools,
                using_fallback: is_fallback,
                disconnect_reason: None,
            }),
            Err(e) => {
                tracing::warn!("Failed to parse status response from {base_url}: {e}");
                Err(DisconnectReason::Error(e.to_string()))
            }
        }
    }

    /// List available MCPs from the registry with their enabled status.
    ///
    /// Returns a list of all MCPs that can be enabled, along with
    /// whether each one is currently enabled for the user.
    ///
    /// Note: This currently returns mock data. The actual endpoints
    /// will be implemented in a future task.
    #[allow(clippy::unused_async)] // Will be async when real endpoints are added
    pub async fn list_mcps(&self) -> Vec<McpRegistryEntry> {
        // TODO: Call actual MCP server endpoints when available
        // For now, return mock data to demonstrate the UI
        vec![
            McpRegistryEntry {
                name: "vault".to_string(),
                description: "File operations for Obsidian vault".to_string(),
                enabled: true,
            },
            McpRegistryEntry {
                name: "memory".to_string(),
                description: "Conversation memory and search".to_string(),
                enabled: true,
            },
            McpRegistryEntry {
                name: "slack".to_string(),
                description: "Read and send Slack messages".to_string(),
                enabled: false,
            },
            McpRegistryEntry {
                name: "calendar".to_string(),
                description: "Google Calendar integration".to_string(),
                enabled: false,
            },
        ]
    }

    /// Enable an MCP for a user.
    ///
    /// # Arguments
    /// * `user_id` - Telegram user ID
    /// * `name` - Name of the MCP to enable
    ///
    /// # Returns
    /// * `Ok(true)` - MCP was successfully enabled
    /// * `Ok(false)` - MCP not found in registry
    /// * `Err` - Network or server error
    pub async fn enable_mcp(&self, user_id: i64, name: &str) -> Result<bool> {
        let url = format!("{}/mcp/user/{}/enable/{}", self.base_url, user_id, name);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .send()
            .await
            .map_err(|e| Self::format_connection_error(&e, &self.base_url, "enable MCP"))?;

        match response.status().as_u16() {
            200 => Ok(true),
            404 => Ok(false),
            status => {
                let body = response.text().await.unwrap_or_default();
                Err(anyhow::anyhow!("Failed to enable MCP ({status}): {body}"))
            }
        }
    }

    /// Disable an MCP for a user.
    ///
    /// # Arguments
    /// * `user_id` - Telegram user ID
    /// * `name` - Name of the MCP to disable
    ///
    /// # Returns
    /// * `Ok(true)` - MCP was successfully disabled
    /// * `Ok(false)` - MCP not found in registry
    /// * `Err` - Network or server error
    pub async fn disable_mcp(&self, user_id: i64, name: &str) -> Result<bool> {
        let url = format!("{}/mcp/user/{}/disable/{}", self.base_url, user_id, name);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .send()
            .await
            .map_err(|e| Self::format_connection_error(&e, &self.base_url, "disable MCP"))?;

        match response.status().as_u16() {
            200 => Ok(true),
            404 => Ok(false),
            status => {
                let body = response.text().await.unwrap_or_default();
                Err(anyhow::anyhow!("Failed to disable MCP ({status}): {body}"))
            }
        }
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
    ///
    /// Uses smart recovery: if request fails, checks if server is actually
    /// unreachable before triggering Wake-on-LAN.
    #[allow(clippy::cognitive_complexity)]
    pub async fn get_tool_definitions(&self) -> Result<Vec<Tool>> {
        // First attempt
        match self.try_get_tool_definitions().await {
            Ok(tools) => Ok(tools),
            Err(e) => {
                tracing::warn!("Failed to get tools: {}", e);

                // Quick health check before assuming server is down
                if self.quick_health_check().await {
                    tracing::info!("Health check passed, retrying immediately");
                    return self.try_get_tool_definitions().await;
                }

                // Server truly unreachable, try WoL if configured
                if !self.has_mac_address() {
                    return Err(e);
                }

                tracing::info!("Server unreachable, attempting Wake-on-LAN...");
                if let Err(wol_err) = self.wake_mac() {
                    tracing::warn!("Wake-on-LAN failed: {}", wol_err);
                    return Err(e);
                }

                // Wait for Mac to wake up
                tracing::info!("Waiting 15s for Mac to wake up...");
                tokio::time::sleep(Duration::from_secs(15)).await;

                // Retry
                self.try_get_tool_definitions().await.context(
                    "Failed to get tools after Wake-on-LAN.\n\n\
                     The Mac may still be waking up. Try again in a moment.",
                )
            }
        }
    }

    /// Try to get tool definitions (single attempt).
    async fn try_get_tool_definitions(&self) -> Result<Vec<Tool>> {
        let response = self
            .client
            .get(format!("{}/tools", self.base_url))
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .timeout(Duration::from_secs(10))
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

        // Filter out malformed tools (missing input_schema) and log warnings
        let mut valid_tools = Vec::new();
        for t in defs.tools {
            if let Some(schema) = t.input_schema {
                valid_tools.push(Tool {
                    name: t.name,
                    description: t.description,
                    input_schema: schema,
                });
            } else {
                tracing::warn!("Skipping malformed tool '{}': missing input_schema", t.name);
            }
        }
        Ok(valid_tools)
    }

    /// Call a tool on the MCP server.
    ///
    /// Uses smart recovery: if request fails, checks if server is actually
    /// unreachable before triggering Wake-on-LAN.
    #[allow(clippy::cognitive_complexity)]
    pub async fn call_tool(&self, name: &str, input: &Value) -> Result<String> {
        // First attempt
        match self.try_call_tool(name, input).await {
            Ok(result) => Ok(result),
            Err(e) => {
                tracing::warn!("MCP call failed: {}", e);

                // Quick health check before assuming server is down
                if self.quick_health_check().await {
                    tracing::info!("Health check passed, retrying immediately");
                    return self.try_call_tool(name, input).await;
                }

                // Server truly unreachable, try WoL if configured
                if !self.has_mac_address() {
                    return Err(e);
                }

                tracing::info!("Server unreachable, attempting Wake-on-LAN...");
                if let Err(wol_err) = self.wake_mac() {
                    tracing::warn!("Wake-on-LAN failed: {}", wol_err);
                    return Err(e);
                }

                // Wait for Mac to wake up
                tracing::info!("Waiting 10s for Mac to wake up...");
                tokio::time::sleep(Duration::from_secs(10)).await;

                // Retry with retries
                self.try_call_tool_with_retry(name, input, 3).await.context(
                    "Failed to connect after Wake-on-LAN attempt.\n\n\
                     Try:\n\
                     • Wait longer for Mac to wake up\n\
                     • Check Mac power/network status manually\n\
                     • Verify Wake-on-LAN is enabled in Mac settings",
                )
            }
        }
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

    /// Send a message to the channel.
    ///
    /// # Arguments
    ///
    /// * `from` - Sender identifier (e.g., "lu" for the bot)
    /// * `content` - Message content to send
    /// * `reply_to` - Optional ID of message this is replying to
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the server returns an error status.
    pub async fn channel_send(
        &self,
        from: &str,
        content: &str,
        reply_to: Option<u64>,
    ) -> Result<()> {
        let url = format!("{}/channel/send", self.base_url);

        let mut body = serde_json::json!({
            "from": from,
            "content": content,
        });

        if let Some(id) = reply_to {
            body["reply_to"] = serde_json::json!(id);
        }

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                Self::format_connection_error(&e, &self.base_url, "send channel message")
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Channel send failed: {status} - {text}");
        }

        Ok(())
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
            let error: ChatError = response.json().await.unwrap_or_else(|_| ChatError {
                error: "unknown".to_string(),
                message: format!("HTTP {status}"),
            });

            let msg = match error.error.as_str() {
                "api_key_missing" => "API key not configured on Mac.\n\
                     Run: lu setup mcp"
                    .to_string(),
                "auth_failed" => "Invalid API key. Get a new one from:\n\
                     console.anthropic.com/account/keys\n\
                     Then run: lu setup credentials"
                    .to_string(),
                "budget_exceeded" => "API credits exhausted.\n\
                     Add credits at console.anthropic.com"
                    .to_string(),
                "rate_limit" => "Rate limited. Wait a moment and try again.".to_string(),
                _ => error.message,
            };

            return Err(anyhow::anyhow!(msg));
        }

        response
            .json()
            .await
            .context("Failed to parse chat response")
    }

    /// Check API key health on the MCP server.
    ///
    /// Returns information about whether the API key is valid and working.
    pub async fn check_api_health(&self) -> Result<ApiHealthStatus> {
        let response = self
            .client
            .get(format!("{}/admin/health", self.base_url))
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .send()
            .await
            .map_err(|e| Self::format_connection_error(&e, &self.base_url, "check API health"))?;

        response
            .json()
            .await
            .context("Failed to parse health response")
    }
}

/// API health check response.
#[derive(Deserialize)]
#[allow(dead_code)]
pub struct ApiHealthStatus {
    /// Whether the API key is valid.
    pub api_key_valid: bool,
    /// Error message if key is invalid.
    pub error: Option<String>,
    /// Suggested fix if key is invalid.
    pub fix: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_config_creates_client() {
        let config = McpConfig {
            url: "http://localhost:8200".to_string(),
            fallback_url: None,
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
            fallback_url: None,
            auth_token: "test-token".to_string(),
            mac_address: None,
        };

        let client = McpClient::from_config(&config);

        assert_eq!(client.base_url, "http://localhost:8200");
    }

    #[tokio::test]
    async fn get_status_returns_disconnected_for_unreachable_server() {
        let config = McpConfig {
            url: "http://127.0.0.1:1".to_string(), // Unreachable port
            fallback_url: None,
            auth_token: "test-token".to_string(),
            mac_address: None,
        };

        let client = McpClient::from_config(&config);
        let status = client.get_status().await;

        assert!(!status.connected);
        assert_eq!(status.endpoint, "http://127.0.0.1:1");
        assert!(status.tools.is_empty());
    }

    #[test]
    fn mcp_status_struct_has_expected_fields() {
        let status = McpStatus {
            connected: true,
            endpoint: "http://localhost:8200".to_string(),
            latency_ms: 42,
            tools: vec![ToolInfo {
                name: "test_tool".to_string(),
                description: "A test tool".to_string(),
            }],
            using_fallback: false,
            disconnect_reason: None,
        };

        assert!(status.connected);
        assert_eq!(status.endpoint, "http://localhost:8200");
        assert_eq!(status.latency_ms, 42);
        assert_eq!(status.tools.len(), 1);
        assert_eq!(status.tools[0].name, "test_tool");
        assert_eq!(status.tools[0].description, "A test tool");
        assert!(!status.using_fallback);
    }

    #[tokio::test]
    async fn enable_mcp_returns_error_for_unreachable_server() {
        let config = McpConfig {
            url: "http://127.0.0.1:1".to_string(), // Unreachable port
            fallback_url: None,
            auth_token: "test-token".to_string(),
            mac_address: None,
        };

        let client = McpClient::from_config(&config);
        let result = client.enable_mcp(123, "slack").await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn disable_mcp_returns_error_for_unreachable_server() {
        let config = McpConfig {
            url: "http://127.0.0.1:1".to_string(), // Unreachable port
            fallback_url: None,
            auth_token: "test-token".to_string(),
            mac_address: None,
        };

        let client = McpClient::from_config(&config);
        let result = client.disable_mcp(123, "slack").await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn health_check_returns_false_for_unreachable_server() {
        let config = McpConfig {
            url: "http://127.0.0.1:1".to_string(),
            fallback_url: None,
            auth_token: "test-token".to_string(),
            mac_address: None,
        };

        let client = McpClient::from_config(&config);
        let is_healthy = client.quick_health_check().await;

        assert!(!is_healthy);
    }

    #[test]
    fn client_has_mac_address_returns_correct_value() {
        let config_with = McpConfig {
            url: "http://localhost:8200".to_string(),
            fallback_url: None,
            auth_token: "test".to_string(),
            mac_address: Some("aa:bb:cc:dd:ee:ff".to_string()),
        };

        let config_without = McpConfig {
            url: "http://localhost:8200".to_string(),
            fallback_url: None,
            auth_token: "test".to_string(),
            mac_address: None,
        };

        let client_with = McpClient::from_config(&config_with);
        let client_without = McpClient::from_config(&config_without);

        assert!(client_with.has_mac_address());
        assert!(!client_without.has_mac_address());
    }
}
