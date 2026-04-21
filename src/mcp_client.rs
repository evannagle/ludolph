//! MCP client for connecting to Mac's MCP server.
//!
//! This module provides HTTP client functionality for the Pi thin client
//! to communicate with the Mac's MCP server, including Wake-on-LAN support.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::process::Command;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crate::config::McpConfig;
use crate::tools::Tool;

/// Default retry configuration for MCP requests.
const DEFAULT_MAX_RETRIES: u32 = 3;
/// Base delay between retries (doubles each attempt: 1s, 2s, 4s).
const RETRY_BASE_DELAY: Duration = Duration::from_secs(1);

/// MCP client for communicating with the Mac's MCP server.
#[derive(Clone)]
pub struct McpClient {
    client: reqwest::Client,
    base_url: String,
    fallback_url: Option<String>,
    auth_token: String,
    mac_address: Option<String>,
    /// Tracks the last URL that successfully responded.
    /// Starts as `base_url`; switches to fallback on primary failure.
    active_url: Arc<RwLock<String>>,
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
    /// Any content that was streamed before the failure. Populated by
    /// the MCP server when it captures partial output from the LLM.
    #[serde(default)]
    partial_content: String,
    /// Path to the scratch file holding the partial content, if any.
    #[serde(default)]
    scratch_path: Option<String>,
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

/// An observation about the user (fact, preference, or context).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub id: String,
    pub title: Option<String>,
    pub text: String,
    pub category: String,
    pub created_at: String,
}

/// Response from the /observations/recent endpoint.
#[derive(Deserialize)]
struct ObservationsResponse {
    observations: Vec<Observation>,
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

/// Whether a request error is transient and worth retrying.
fn is_transient_error(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect() || error.is_request()
}

/// Whether an error message indicates a transient failure.
fn is_transient_message(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("connection")
        || lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("rate limit")
        || lower.contains("temporarily unavailable")
        || lower.contains("unreachable")
}

impl McpClient {
    /// Create a new MCP client from configuration.
    #[must_use]
    pub fn from_config(config: &McpConfig) -> Self {
        let base_url = config.url.trim_end_matches('/').to_string();
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            active_url: Arc::new(RwLock::new(base_url.clone())),
            client,
            base_url,
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

    /// Get the currently active URL (last one that worked).
    fn active_url(&self) -> String {
        self.active_url
            .read()
            .map_or_else(|_| self.base_url.clone(), |u| u.clone())
    }

    /// Record a successful connection to a URL so future requests prefer it.
    fn set_active_url(&self, url: &str) {
        if let Ok(mut active) = self.active_url.write() {
            if *active != url {
                tracing::info!("MCP: switching active endpoint to {url}");
                *active = url.to_string();
            }
        }
    }

    /// Get the list of URLs to try, starting with the active one.
    ///
    /// Returns `[active, other]` if a fallback is configured and differs
    /// from the active URL, otherwise just `[active]`.
    fn urls_to_try(&self) -> Vec<String> {
        let active = self.active_url();
        let mut urls = vec![active.clone()];

        // Add the other URL as fallback
        if let Some(ref fallback) = self.fallback_url {
            if *fallback != active {
                urls.push(fallback.clone());
            }
        }
        if self.base_url != active && !urls.contains(&self.base_url) {
            urls.push(self.base_url.clone());
        }

        urls
    }

    /// Make an authenticated GET request with retry and fallback.
    ///
    /// Tries the active URL first, then fallback. Each URL gets up to
    /// `max_retries` attempts with exponential backoff (1s, 2s, 4s).
    async fn get_with_retry(
        &self,
        path: &str,
        timeout: Duration,
        max_retries: u32,
    ) -> Result<reqwest::Response> {
        let urls = self.urls_to_try();
        let mut last_error = None;

        for url in &urls {
            let endpoint = format!("{url}{path}");

            for attempt in 0..max_retries {
                if attempt > 0 {
                    let delay = RETRY_BASE_DELAY * (1 << (attempt - 1));
                    tracing::debug!("Retry {attempt}/{max_retries} for {endpoint} in {delay:?}");
                    tokio::time::sleep(delay).await;
                }

                match self
                    .client
                    .get(&endpoint)
                    .header("Authorization", format!("Bearer {}", self.auth_token))
                    .timeout(timeout)
                    .send()
                    .await
                {
                    Ok(resp) if resp.status().is_success() => {
                        self.set_active_url(url);
                        return Ok(resp);
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        // Auth failures are not transient — don't retry
                        if status == reqwest::StatusCode::UNAUTHORIZED
                            || status == reqwest::StatusCode::FORBIDDEN
                        {
                            return Err(anyhow::anyhow!(
                                "Authentication failed ({status}) at {endpoint}"
                            ));
                        }
                        last_error = Some(anyhow::anyhow!("Server error ({status}) at {endpoint}"));
                    }
                    Err(e) if is_transient_error(&e) && attempt + 1 < max_retries => {
                        tracing::warn!(
                            "Transient error on {endpoint} (attempt {}): {e}",
                            attempt + 1
                        );
                        last_error = Some(Self::format_connection_error(&e, url, path));
                    }
                    Err(e) => {
                        last_error = Some(Self::format_connection_error(&e, url, path));
                        break; // Non-transient error, try next URL
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("All endpoints unreachable")))
    }

    /// Make an authenticated POST request with retry and fallback.
    ///
    /// Body is pre-serialized to `Value` so the future is `Send`.
    async fn post_with_retry(
        &self,
        path: &str,
        body: &Value,
        timeout: Duration,
        max_retries: u32,
    ) -> Result<reqwest::Response> {
        let urls = self.urls_to_try();
        let mut last_error = None;

        for url in &urls {
            let endpoint = format!("{url}{path}");

            for attempt in 0..max_retries {
                if attempt > 0 {
                    let delay = RETRY_BASE_DELAY * (1 << (attempt - 1));
                    tracing::debug!(
                        "Retry {attempt}/{max_retries} for POST {endpoint} in {delay:?}"
                    );
                    tokio::time::sleep(delay).await;
                }

                match self
                    .client
                    .post(&endpoint)
                    .header("Authorization", format!("Bearer {}", self.auth_token))
                    .header("Content-Type", "application/json")
                    .json(body)
                    .timeout(timeout)
                    .send()
                    .await
                {
                    Ok(resp) if resp.status().is_success() => {
                        self.set_active_url(url);
                        return Ok(resp);
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        if status == reqwest::StatusCode::UNAUTHORIZED
                            || status == reqwest::StatusCode::FORBIDDEN
                        {
                            return Err(anyhow::anyhow!(
                                "Authentication failed ({status}) at {endpoint}"
                            ));
                        }
                        // For POST, return non-success responses for caller to handle
                        // (e.g. 404 for missing tools, 500 with error body)
                        self.set_active_url(url);
                        return Ok(resp);
                    }
                    Err(e) if is_transient_error(&e) && attempt + 1 < max_retries => {
                        tracing::warn!(
                            "Transient error on POST {endpoint} (attempt {}): {e}",
                            attempt + 1
                        );
                        last_error = Some(Self::format_connection_error(
                            &e,
                            url,
                            &format!("POST {path}"),
                        ));
                    }
                    Err(e) => {
                        last_error = Some(Self::format_connection_error(
                            &e,
                            url,
                            &format!("POST {path}"),
                        ));
                        break;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("All endpoints unreachable")))
    }

    /// Get detailed status information from the MCP server.
    ///
    /// Tries each URL (active first, then fallback) with a single retry
    /// on transient failures. Returns `McpStatus` with `connected: false`
    /// if all endpoints are unreachable.
    pub async fn get_status(&self) -> McpStatus {
        let urls = self.urls_to_try();
        let mut last_reason = DisconnectReason::Unreachable;

        for (i, url) in urls.iter().enumerate() {
            let is_fallback = *url != self.base_url;

            // Try each URL up to 2 times (initial + 1 retry)
            for attempt in 0..2u32 {
                if attempt > 0 {
                    tokio::time::sleep(RETRY_BASE_DELAY).await;
                }

                match self.try_status(url, is_fallback).await {
                    Ok(status) => {
                        self.set_active_url(url);
                        return status;
                    }
                    Err(reason) => {
                        tracing::debug!(
                            "Status check failed for {url} (url {}/{}, attempt {}): {reason:?}",
                            i + 1,
                            urls.len(),
                            attempt + 1,
                        );
                        // Auth failures are definitive — don't retry
                        if reason == DisconnectReason::AuthFailed {
                            return McpStatus {
                                connected: false,
                                endpoint: url.clone(),
                                latency_ms: 0,
                                tools: Vec::new(),
                                using_fallback: is_fallback,
                                disconnect_reason: Some(reason),
                            };
                        }
                        last_reason = reason;
                    }
                }
            }
        }

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
    /// Tries the active URL first, then fallback. Returns true if any
    /// endpoint responds with 200 OK.
    pub async fn quick_health_check(&self) -> bool {
        for url in &self.urls_to_try() {
            let response = self
                .client
                .get(format!("{url}/health"))
                .header("Authorization", format!("Bearer {}", self.auth_token))
                .timeout(Duration::from_secs(2))
                .send()
                .await;

            if matches!(response, Ok(r) if r.status().is_success()) {
                self.set_active_url(url);
                return true;
            }
        }
        false
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
        let path = format!("/mcp/user/{user_id}/enable/{name}");
        let empty = serde_json::json!({});
        let response = self
            .post_with_retry(&path, &empty, Duration::from_secs(10), DEFAULT_MAX_RETRIES)
            .await?;

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
        let path = format!("/mcp/user/{user_id}/disable/{name}");
        let empty = serde_json::json!({});
        let response = self
            .post_with_retry(&path, &empty, Duration::from_secs(10), DEFAULT_MAX_RETRIES)
            .await?;

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

    /// Get recent observations for a user from the MCP server.
    ///
    /// Used to inject known facts/preferences into the system prompt.
    /// Returns empty vec on any error (observations are optional context).
    pub async fn get_observations(&self, user_id: i64, limit: usize) -> Vec<Observation> {
        let path = format!("/observations/recent?user_id={user_id}&limit={limit}");

        // Use retry but only 2 attempts — observations are optional context
        match self.get_with_retry(&path, Duration::from_secs(5), 2).await {
            Ok(r) => r
                .json::<ObservationsResponse>()
                .await
                .map_or_else(|_| Vec::new(), |resp| resp.observations),
            Err(e) => {
                tracing::debug!("Failed to fetch observations: {e}");
                Vec::new()
            }
        }
    }

    /// Push a schedule run record to the MCP server.
    ///
    /// Retries on transient failures. Fire-and-forget: failures are logged
    /// but don't affect the local scheduler.
    #[allow(clippy::too_many_arguments)]
    pub async fn record_schedule_run(
        &self,
        schedule_id: &str,
        schedule_name: &str,
        user_id: i64,
        status: &str,
        started_at: &str,
        completed_at: Option<&str>,
        result_summary: Option<&str>,
        error_message: Option<&str>,
    ) {
        let body = serde_json::json!({
            "schedule_id": schedule_id,
            "schedule_name": schedule_name,
            "user_id": user_id,
            "status": status,
            "started_at": started_at,
            "completed_at": completed_at,
            "result_summary": result_summary,
            "error_message": error_message,
        });

        if let Err(e) = self
            .post_with_retry(
                "/schedule_runs/record",
                &body,
                Duration::from_secs(10),
                DEFAULT_MAX_RETRIES,
            )
            .await
        {
            tracing::warn!("Failed to push schedule run to MCP: {e}");
        }
    }

    /// Get tool definitions from the MCP server.
    ///
    /// Uses retry with fallback, then `WoL` recovery as a last resort.
    pub async fn get_tool_definitions(&self) -> Result<Vec<Tool>> {
        match self.try_get_tool_definitions().await {
            Ok(tools) => Ok(tools),
            Err(e) => {
                self.attempt_wol_recovery("get tools", 15).await?;
                self.try_get_tool_definitions().await.with_context(|| {
                    format!("Failed to get tools after Wake-on-LAN.\n\nOriginal error: {e}")
                })
            }
        }
    }

    /// Try to get tool definitions with retry and fallback.
    async fn try_get_tool_definitions(&self) -> Result<Vec<Tool>> {
        let response = self
            .get_with_retry("/tools", Duration::from_secs(10), DEFAULT_MAX_RETRIES)
            .await?;

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
    /// Uses retry with fallback, then `WoL` recovery as a last resort.
    pub async fn call_tool(&self, name: &str, input: &Value) -> Result<String> {
        match self.try_call_tool(name, input).await {
            Ok(result) => Ok(result),
            Err(e) => {
                self.attempt_wol_recovery(&format!("call tool '{name}'"), 10)
                    .await?;
                self.try_call_tool(name, input).await.with_context(|| {
                    format!("Failed to call tool '{name}' after Wake-on-LAN.\n\nOriginal: {e}")
                })
            }
        }
    }

    /// Attempt `WoL` recovery when all endpoints are unreachable.
    ///
    /// Returns `Ok(())` if `WoL` was sent and we waited for the Mac,
    /// or `Err` if `WoL` is not configured or the health check passes
    /// (meaning the server is actually reachable and the problem is
    /// something else).
    async fn attempt_wol_recovery(&self, action: &str, wait_secs: u64) -> Result<()> {
        if !self.has_mac_address() {
            anyhow::bail!("MCP server unreachable and Wake-on-LAN not configured");
        }

        // Quick health check — maybe only one endpoint was down
        if self.quick_health_check().await {
            tracing::info!("Health check passed — server is reachable, skipping WoL");
            return Ok(());
        }

        tracing::info!("All endpoints unreachable for {action}, attempting Wake-on-LAN...");
        self.wake_mac()
            .context("Wake-on-LAN failed — check Mac address and wakeonlan command")?;

        tracing::info!("Waiting {wait_secs}s for Mac to wake up...");
        tokio::time::sleep(Duration::from_secs(wait_secs)).await;
        Ok(())
    }

    /// Try to call a tool with retry and fallback across all endpoints.
    async fn try_call_tool(&self, name: &str, input: &Value) -> Result<String> {
        tracing::debug!("MCP: Calling tool {name}");

        let request = serde_json::to_value(ToolCallRequest {
            name: name.to_string(),
            arguments: input.clone(),
        })
        .context("Failed to serialize tool call request")?;

        let response = self
            .post_with_retry(
                "/tools/call",
                &request,
                Duration::from_secs(10),
                DEFAULT_MAX_RETRIES,
            )
            .await?;

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

    /// Format connection errors with helpful suggestions.
    fn format_connection_error(error: &reqwest::Error, url: &str, action: &str) -> anyhow::Error {
        let msg = if error.is_timeout() {
            format!(
                "Request timed out connecting to {url} ({action}).\n\n\
                Try:\n\
                • Check if Mac is awake and reachable\n\
                • Verify network connectivity\n\
                • Ensure MCP server is running on Mac\n\
                • Wake Mac with /wake command if configured"
            )
        } else if error.is_connect() {
            format!(
                "Cannot connect to MCP server at {url} ({action}).\n\n\
                Try:\n\
                • Check if MCP server is running on Mac\n\
                • Verify URL in config is correct\n\
                • Ensure Mac is on network and reachable\n\
                • Try waking Mac with /wake command"
            )
        } else if error.is_status() {
            let status = error.status().map_or(String::new(), |s| format!(" ({s})"));
            format!(
                "MCP server returned error{status} at {url} ({action}).\n\n\
                Try:\n\
                • Check MCP server logs\n\
                • Verify authentication token\n\
                • Restart MCP server if needed"
            )
        } else if error.is_body() || error.is_decode() {
            format!(
                "Invalid response from MCP server at {url} ({action}).\n\n\
                Try:\n\
                • Check MCP server version compatibility\n\
                • Check server logs for errors"
            )
        } else {
            format!(
                "MCP connection error at {url} ({action}): {error}\n\n\
                Try:\n\
                • Check network connectivity\n\
                • Verify MCP server is accessible"
            )
        };

        anyhow::anyhow!(msg)
    }

    /// Send a message to the channel.
    ///
    /// Retries with exponential backoff and fallback on transient failures.
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
        let mut body = serde_json::json!({
            "from": from,
            "content": content,
        });

        if let Some(id) = reply_to {
            body["reply_to"] = serde_json::json!(id);
        }

        let response = self
            .post_with_retry(
                "/channel/send",
                &body,
                Duration::from_secs(10),
                DEFAULT_MAX_RETRIES,
            )
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Channel send failed: {status} - {text}");
        }

        Ok(())
    }

    /// Send a chat request to the MCP server's LLM proxy.
    ///
    /// Retries on transient errors (connection failures, timeouts, rate limits)
    /// with exponential backoff (1s, 2s, 4s). Tries fallback URL on failure.
    pub async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse> {
        let mut last_err = None;

        for attempt in 0..DEFAULT_MAX_RETRIES {
            match self.chat_once(request).await {
                Ok(resp) => return Ok(resp),
                Err(e) => {
                    let msg = e.to_string();

                    if !is_transient_message(&msg) || attempt + 1 == DEFAULT_MAX_RETRIES {
                        return Err(e);
                    }

                    let delay = RETRY_BASE_DELAY * (1 << attempt);
                    tracing::warn!(
                        "Chat attempt {} failed (retrying in {delay:?}): {msg}",
                        attempt + 1,
                    );
                    tokio::time::sleep(delay).await;
                    last_err = Some(e);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("Chat failed after retries")))
    }

    /// Single attempt at sending a chat request.
    ///
    /// Uses the active URL (no automatic fallback here because chat responses
    /// can be large and we want the caller's retry loop to handle switching).
    async fn chat_once(&self, request: &ChatRequest) -> Result<ChatResponse> {
        let url = self.active_url();
        let response = self
            .client
            .post(format!("{url}/chat"))
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .header("Content-Type", "application/json")
            .json(request)
            .send()
            .await
            .map_err(|e| Self::format_connection_error(&e, &url, "chat"))?;

        let status = response.status();

        if !status.is_success() {
            let error: ChatError = response.json().await.unwrap_or_else(|_| ChatError {
                error: "unknown".to_string(),
                message: format!("HTTP {status}"),
                partial_content: String::new(),
                scratch_path: None,
            });

            let base_msg = match error.error.as_str() {
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
                "timeout" => "The Claude API connection stalled and timed out.".to_string(),
                _ => error.message.clone(),
            };

            // If partial content was captured before the failure, include
            // it in the error so the user sees what was generated. The
            // scratch file stays on the Mac for full recovery.
            let full_msg = if error.partial_content.is_empty() {
                base_msg
            } else {
                let scratch_note = error
                    .scratch_path
                    .as_ref()
                    .map(|p| format!("\n\n(Full partial response saved on Mac at {p})"))
                    .unwrap_or_default();
                format!(
                    "{base_msg}\n\n--- Partial response (before connection died) ---\n\n{}{}",
                    error.partial_content, scratch_note
                )
            };

            return Err(anyhow::anyhow!(full_msg));
        }

        response
            .json()
            .await
            .context("Failed to parse chat response")
    }

    /// Check API key health on the MCP server.
    ///
    /// Returns information about whether the API key is valid and working.
    /// Retries with fallback on transient failures.
    pub async fn check_api_health(&self) -> Result<ApiHealthStatus> {
        let response = self
            .get_with_retry(
                "/admin/health",
                Duration::from_secs(10),
                DEFAULT_MAX_RETRIES,
            )
            .await?;

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

    #[test]
    fn active_url_defaults_to_base_url() {
        let config = McpConfig {
            url: "http://localhost:8200".to_string(),
            fallback_url: Some("http://fallback:8200".to_string()),
            auth_token: "test".to_string(),
            mac_address: None,
        };

        let client = McpClient::from_config(&config);
        assert_eq!(client.active_url(), "http://localhost:8200");
    }

    #[test]
    fn set_active_url_switches_endpoint() {
        let config = McpConfig {
            url: "http://primary:8200".to_string(),
            fallback_url: Some("http://fallback:8200".to_string()),
            auth_token: "test".to_string(),
            mac_address: None,
        };

        let client = McpClient::from_config(&config);
        client.set_active_url("http://fallback:8200");
        assert_eq!(client.active_url(), "http://fallback:8200");
    }

    #[test]
    fn urls_to_try_starts_with_active_then_other() {
        let config = McpConfig {
            url: "http://primary:8200".to_string(),
            fallback_url: Some("http://fallback:8200".to_string()),
            auth_token: "test".to_string(),
            mac_address: None,
        };

        let client = McpClient::from_config(&config);

        // Default: active = primary
        let urls = client.urls_to_try();
        assert_eq!(urls, vec!["http://primary:8200", "http://fallback:8200"]);

        // After switching: active = fallback, primary is second
        client.set_active_url("http://fallback:8200");
        let urls = client.urls_to_try();
        assert_eq!(urls, vec!["http://fallback:8200", "http://primary:8200"]);
    }

    #[test]
    fn urls_to_try_without_fallback_returns_single_url() {
        let config = McpConfig {
            url: "http://primary:8200".to_string(),
            fallback_url: None,
            auth_token: "test".to_string(),
            mac_address: None,
        };

        let client = McpClient::from_config(&config);
        let urls = client.urls_to_try();
        assert_eq!(urls, vec!["http://primary:8200"]);
    }

    #[test]
    fn is_transient_error_detects_timeout() {
        assert!(is_transient_message("Connection timed out"));
        assert!(is_transient_message("Request timeout after 10s"));
        assert!(is_transient_message("Rate limited, try again"));
        assert!(is_transient_message("Server temporarily unavailable"));
        assert!(is_transient_message("host unreachable"));
    }

    #[test]
    fn is_transient_error_rejects_permanent_errors() {
        assert!(!is_transient_message("Authentication failed"));
        assert!(!is_transient_message("Tool not found"));
        assert!(!is_transient_message("Invalid API key"));
    }

    #[tokio::test]
    async fn get_with_retry_returns_error_for_all_unreachable() {
        let config = McpConfig {
            url: "http://127.0.0.1:1".to_string(),
            fallback_url: Some("http://127.0.0.1:2".to_string()),
            auth_token: "test".to_string(),
            mac_address: None,
        };

        let client = McpClient::from_config(&config);
        let result = client
            .get_with_retry("/health", Duration::from_secs(1), 1)
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn observations_returns_empty_for_unreachable() {
        let config = McpConfig {
            url: "http://127.0.0.1:1".to_string(),
            fallback_url: None,
            auth_token: "test".to_string(),
            mac_address: None,
        };

        let client = McpClient::from_config(&config);
        let observations = client.get_observations(123, 5).await;
        assert!(observations.is_empty());
    }
}
