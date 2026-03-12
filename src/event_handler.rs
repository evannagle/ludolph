//! Event handler for processing MCP events.
//!
//! Receives events from the SSE stream and handles them appropriately.
//! Channel messages are processed through the LLM and responses are
//! sent back through the MCP client.


use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;
use tokio::time::sleep;
use tracing::{debug, info};

use crate::channel::Channel;
use crate::llm::Llm;
use crate::mcp_client::McpClient;
use crate::sse_client::Event;

/// Our bot's identifier for detecting our own messages.
const BOT_SENDER_ID: &str = "lu";

/// Default delay in seconds before responding to channel messages.
/// Helps throttle conversation pace and manage API costs.
const DEFAULT_CHANNEL_DELAY_SECS: u64 = 0;

/// Git context from the sender's working environment.
#[derive(Debug, Deserialize, Default)]
struct GitContext {
    /// Repository name.
    #[serde(default)]
    repo: Option<String>,
    /// Current branch name.
    #[serde(default)]
    branch: Option<String>,
    /// Recent commit messages (oneline format).
    #[serde(default)]
    recent_commits: Vec<String>,
}

/// Channel message data from event.
#[derive(Debug, Deserialize)]
struct ChannelMessageData {
    /// Unique message identifier.
    id: u64,
    /// Sender identifier (e.g., "user" or "lu").
    from: String,
    /// Message content.
    content: String,
    /// ID of message this is replying to, if any.
    #[serde(default)]
    #[allow(dead_code)] // Protocol field, may be used for threading
    reply_to: Option<u64>,
    /// Git context from sender's environment.
    #[serde(default)]
    context: Option<GitContext>,
}

/// Handle an event from the MCP stream.
///
/// Dispatches to appropriate handlers based on event type.
///
/// # Arguments
///
/// * `event` - The event received from the SSE stream
/// * `llm` - LLM client for processing messages
/// * `mcp` - MCP client for sending responses
/// * `channel` - Channel for storing responses (available via HTTP API)
///
/// # Errors
///
/// Returns an error if event handling fails.
pub async fn handle_event(
    event: Event,
    llm: &Llm,
    mcp: &McpClient,
    channel: &Channel,
) -> Result<()> {
    match event.event_type.as_str() {
        "channel_message" => {
            handle_channel_message(event.data, llm, mcp, channel).await?;
        }
        "system_status" => {
            info!("System status: {:?}", event.data);
        }
        "heartbeat" | "keepalive" => {
            debug!("Received keepalive");
        }
        _ => {
            debug!("Unknown event type: {}", event.event_type);
        }
    }
    Ok(())
}

/// Handle a channel message event.
///
/// Parses the message, skips our own messages, processes through LLM,
/// and sends the response back to the channel.
///
/// Respects `LU_CHANNEL_DELAY` environment variable for throttling (in seconds).
async fn handle_channel_message(
    data: serde_json::Value,
    llm: &Llm,
    mcp: &McpClient,
    channel: &Channel,
) -> Result<()> {
    let msg: ChannelMessageData =
        serde_json::from_value(data).context("Failed to parse channel message data")?;

    if msg.from == BOT_SENDER_ID {
        debug!("Skipping own message (id={})", msg.id);
        return Ok(());
    }

    apply_throttle_delay().await;
    log_channel_message(&msg);

    let message_with_context = build_message_with_context(&msg);
    let response = process_llm_response(llm, mcp, channel, &msg, &message_with_context).await?;

    info!("LLM response: {}", truncate_for_log(&response, 100));
    send_response(mcp, channel, &msg, &response).await
}

/// Apply configured throttle delay before responding.
async fn apply_throttle_delay() {
    let delay_secs = std::env::var("LU_CHANNEL_DELAY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_CHANNEL_DELAY_SECS);
    if delay_secs > 0 {
        debug!("Throttling channel response for {delay_secs}s");
        sleep(Duration::from_secs(delay_secs)).await;
    }
}

/// Log the incoming channel message with context if available.
fn log_channel_message(msg: &ChannelMessageData) {
    if let Some(ctx) = &msg.context {
        info!(
            "Channel message from {} (id={}) [repo={}, branch={}]: {}",
            msg.from,
            msg.id,
            ctx.repo.as_deref().unwrap_or("?"),
            ctx.branch.as_deref().unwrap_or("?"),
            truncate_for_log(&msg.content, 100)
        );
    } else {
        info!(
            "Channel message from {} (id={}): {}",
            msg.from,
            msg.id,
            truncate_for_log(&msg.content, 100)
        );
    }
}

/// Build a context string from git context.
fn build_context_string(ctx: &GitContext) -> Option<String> {
    let mut parts = Vec::new();

    if let Some(repo) = &ctx.repo {
        parts.push(format!("repo: {repo}"));
    }
    if let Some(branch) = &ctx.branch {
        parts.push(format!("branch: {branch}"));
    }
    if !ctx.recent_commits.is_empty() {
        let commits = ctx
            .recent_commits
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(format!("recent commits: {commits}"));
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" | "))
    }
}

/// Build message content with context prefix if available.
fn build_message_with_context(msg: &ChannelMessageData) -> String {
    msg.context
        .as_ref()
        .and_then(build_context_string)
        .map_or_else(
            || msg.content.clone(),
            |ctx| format!("[Context: {ctx}]\n\n{}", msg.content),
        )
}

/// Process the message through LLM, handling errors gracefully.
async fn process_llm_response(
    llm: &Llm,
    mcp: &McpClient,
    channel: &Channel,
    msg: &ChannelMessageData,
    message_with_context: &str,
) -> Result<String> {
    match llm.chat(message_with_context, None).await {
        Ok(response) => Ok(response),
        Err(e) => {
            let error_msg = format_user_error(&e);
            tracing::error!("LLM error: {e}");

            channel.send(BOT_SENDER_ID, &error_msg, Some(msg.id), None);
            let _ = mcp
                .channel_send(BOT_SENDER_ID, &error_msg, Some(msg.id))
                .await;

            Err(e).context("Failed to get LLM response")
        }
    }
}

/// Send response to both channel storage and MCP.
async fn send_response(
    mcp: &McpClient,
    channel: &Channel,
    msg: &ChannelMessageData,
    response: &str,
) -> Result<()> {
    channel.send(BOT_SENDER_ID, response, Some(msg.id), None);
    mcp.channel_send(BOT_SENDER_ID, response, Some(msg.id))
        .await
        .context("Failed to send response to channel")
}

/// Convert an error into a user-friendly message for the channel.
fn format_user_error(e: &anyhow::Error) -> String {
    let error_str = e.to_string().to_lowercase();

    if error_str.contains("api credentials") || error_str.contains("auth") {
        "I can't respond right now - the API key needs to be updated. \
         Ask the admin to run the install script or update the MCP server config."
            .to_string()
    } else if error_str.contains("rate limit") {
        "I'm being rate limited. Please try again in a minute.".to_string()
    } else if error_str.contains("budget") || error_str.contains("credits") {
        "API credits are exhausted. Ask the admin to add credits or switch models.".to_string()
    } else if error_str.contains("connection") || error_str.contains("unreachable") {
        "I can't reach the MCP server. It might be offline or the Mac is asleep.".to_string()
    } else {
        format!(
            "Something went wrong: {}",
            truncate_for_log(&e.to_string(), 100)
        )
    }
}

/// Truncate a string for logging, adding ellipsis if truncated.
fn truncate_for_log(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_message_data_deserializes_correctly() {
        let json = serde_json::json!({
            "id": 42,
            "from": "user",
            "content": "Hello, Ludolph!",
            "reply_to": null
        });

        let msg: ChannelMessageData = serde_json::from_value(json).unwrap();

        assert_eq!(msg.id, 42);
        assert_eq!(msg.from, "user");
        assert_eq!(msg.content, "Hello, Ludolph!");
        assert!(msg.reply_to.is_none());
    }

    #[test]
    fn channel_message_data_deserializes_with_reply_to() {
        let json = serde_json::json!({
            "id": 43,
            "from": "user",
            "content": "Thanks!",
            "reply_to": 42
        });

        let msg: ChannelMessageData = serde_json::from_value(json).unwrap();

        assert_eq!(msg.id, 43);
        assert_eq!(msg.reply_to, Some(42));
    }

    #[test]
    fn channel_message_data_deserializes_without_reply_to_field() {
        let json = serde_json::json!({
            "id": 44,
            "from": "lu",
            "content": "Hi there!"
        });

        let msg: ChannelMessageData = serde_json::from_value(json).unwrap();

        assert_eq!(msg.id, 44);
        assert_eq!(msg.from, "lu");
        assert!(msg.reply_to.is_none());
    }

    #[test]
    fn truncate_for_log_short_string() {
        assert_eq!(truncate_for_log("hello", 10), "hello");
    }

    #[test]
    fn truncate_for_log_exact_length() {
        assert_eq!(truncate_for_log("hello", 5), "hello");
    }

    #[test]
    fn truncate_for_log_long_string() {
        assert_eq!(truncate_for_log("hello world", 5), "hello...");
    }
}
