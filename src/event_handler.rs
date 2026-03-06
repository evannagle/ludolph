//! Event handler for processing MCP events.
//!
//! Receives events from the SSE stream and handles them appropriately.
//! Channel messages are processed through the LLM and responses are
//! sent back through the MCP client.

// Module is prepared for integration in Task 5.5
#![allow(dead_code)]

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
pub async fn handle_event(event: Event, llm: &Llm, mcp: &McpClient, channel: &Channel) -> Result<()> {
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
async fn handle_channel_message(data: serde_json::Value, llm: &Llm, mcp: &McpClient, channel: &Channel) -> Result<()> {
    let msg: ChannelMessageData =
        serde_json::from_value(data).context("Failed to parse channel message data")?;

    // Don't respond to our own messages
    if msg.from == BOT_SENDER_ID {
        debug!("Skipping own message (id={})", msg.id);
        return Ok(());
    }

    // Apply throttle delay if configured (for managing conversation pace/costs)
    let delay_secs = std::env::var("LU_CHANNEL_DELAY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_CHANNEL_DELAY_SECS);
    if delay_secs > 0 {
        debug!("Throttling channel response for {}s", delay_secs);
        sleep(Duration::from_secs(delay_secs)).await;
    }

    // Log context if present
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

    // Build message with context prefix if available
    let message_with_context = if let Some(ctx) = &msg.context {
        let mut context_parts = Vec::new();
        if let Some(repo) = &ctx.repo {
            context_parts.push(format!("repo: {}", repo));
        }
        if let Some(branch) = &ctx.branch {
            context_parts.push(format!("branch: {}", branch));
        }
        if !ctx.recent_commits.is_empty() {
            let commits = ctx
                .recent_commits
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            context_parts.push(format!("recent commits: {}", commits));
        }

        if context_parts.is_empty() {
            msg.content.clone()
        } else {
            format!(
                "[Context: {}]\n\n{}",
                context_parts.join(" | "),
                msg.content
            )
        }
    } else {
        msg.content.clone()
    };

    // Process through LLM, sending user-friendly errors back to the channel
    // Note: Using None for user_id since channel messages don't have Telegram user IDs.
    // A future enhancement could map channel users to IDs for conversation memory.
    let response = match llm.chat(&message_with_context, None).await {
        Ok(response) => response,
        Err(e) => {
            let error_msg = format_user_error(&e);
            tracing::error!("LLM error: {}", e);

            // Store error in channel for HTTP API access
            channel.send(BOT_SENDER_ID, &error_msg, Some(msg.id), None);

            // Also send via MCP so user sees it immediately
            let _ = mcp
                .channel_send(BOT_SENDER_ID, &error_msg, Some(msg.id))
                .await;

            return Err(e).context("Failed to get LLM response");
        }
    };

    info!("LLM response: {}", truncate_for_log(&response, 100));

    // Store response in channel for HTTP API access
    channel.send(BOT_SENDER_ID, &response, Some(msg.id), None);

    // Send response back via MCP for immediate SSE delivery
    mcp.channel_send(BOT_SENDER_ID, &response, Some(msg.id))
        .await
        .context("Failed to send response to channel")?;

    Ok(())
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
