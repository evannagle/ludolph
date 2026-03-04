//! Event handler for processing MCP events.
//!
//! Receives events from the SSE stream and handles them appropriately.
//! Channel messages are processed through the LLM and responses are
//! sent back through the MCP client.

// Module is prepared for integration in Task 5.5
#![allow(dead_code)]

use anyhow::{Context, Result};
use serde::Deserialize;
use tracing::{debug, info};

use crate::llm::Llm;
use crate::mcp_client::McpClient;
use crate::sse_client::Event;

/// Our bot's identifier for detecting our own messages.
const BOT_SENDER_ID: &str = "lu";

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
}

/// Handle an event from the MCP stream.
///
/// Dispatches to appropriate handlers based on event type.
///
/// # Arguments
///
/// * `event` - The event received from the SSE stream
/// * `llm` - LLM client for processing messages
/// * `mcp` - MCP client for sending responses (used when `channel_send` is added)
///
/// # Errors
///
/// Returns an error if event handling fails.
pub async fn handle_event(event: Event, llm: &Llm, mcp: &McpClient) -> Result<()> {
    match event.event_type.as_str() {
        "channel_message" => {
            handle_channel_message(event.data, llm, mcp).await?;
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
async fn handle_channel_message(data: serde_json::Value, llm: &Llm, mcp: &McpClient) -> Result<()> {
    let msg: ChannelMessageData =
        serde_json::from_value(data).context("Failed to parse channel message data")?;

    // Don't respond to our own messages
    if msg.from == BOT_SENDER_ID {
        debug!("Skipping own message (id={})", msg.id);
        return Ok(());
    }

    info!(
        "Channel message from {} (id={}): {}",
        msg.from,
        msg.id,
        truncate_for_log(&msg.content, 100)
    );

    // Process through LLM
    // Note: Using None for user_id since channel messages don't have Telegram user IDs.
    // A future enhancement could map channel users to IDs for conversation memory.
    let response = llm
        .chat(&msg.content, None)
        .await
        .context("Failed to get LLM response")?;

    info!("LLM response: {}", truncate_for_log(&response, 100));

    // Send response back to channel
    mcp.channel_send(BOT_SENDER_ID, &response, Some(msg.id))
        .await
        .context("Failed to send response to channel")?;

    Ok(())
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
