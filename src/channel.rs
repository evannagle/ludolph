//! In-memory channel for Claude Code ↔ Lu messaging.
//!
//! This module provides thread-safe message storage for communication between
//! Claude Code (via HTTP API) and Lu (the Telegram bot). Messages are stored
//! in-memory with a configurable history limit.

#![allow(dead_code)] // Module is built incrementally; usage comes in Task 2-4

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const MAX_MESSAGES: usize = 500;

/// A message in the channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMessage {
    pub id: u64,
    pub sender: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Value>,
}

/// Thread-safe channel for message storage.
#[derive(Clone)]
pub struct Channel {
    inner: Arc<Mutex<ChannelInner>>,
}

struct ChannelInner {
    messages: VecDeque<ChannelMessage>,
    next_id: u64,
}

impl Channel {
    /// Create a new empty channel.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ChannelInner {
                messages: VecDeque::with_capacity(MAX_MESSAGES),
                next_id: 1,
            })),
        }
    }

    /// Send a message to the channel.
    #[allow(clippy::similar_names)] // content and context are intentionally distinct domain terms
    pub fn send(
        &self,
        sender: &str,
        content: &str,
        reply_to: Option<u64>,
        context: Option<serde_json::Value>,
    ) -> ChannelMessage {
        let mut inner = self.inner.lock().expect("channel lock poisoned");

        let msg = ChannelMessage {
            id: inner.next_id,
            sender: sender.to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
            reply_to,
            context,
        };

        inner.next_id += 1;

        if inner.messages.len() >= MAX_MESSAGES {
            inner.messages.pop_front();
        }
        inner.messages.push_back(msg.clone());

        msg
    }

    /// Get recent message history.
    #[must_use]
    pub fn history(&self, limit: usize) -> Vec<ChannelMessage> {
        let inner = self.inner.lock().expect("channel lock poisoned");
        inner
            .messages
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    /// Find a message by ID.
    #[must_use]
    pub fn get(&self, id: u64) -> Option<ChannelMessage> {
        let inner = self.inner.lock().expect("channel lock poisoned");
        inner.messages.iter().find(|m| m.id == id).cloned()
    }
}

impl Default for Channel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_stores_and_retrieves_messages() {
        let channel = Channel::new();
        let msg = channel.send("claude_code", "Hello Lu", None, None);

        assert_eq!(msg.id, 1);
        assert_eq!(msg.sender, "claude_code");
        assert_eq!(msg.content, "Hello Lu");

        let history = channel.history(10);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, 1);
    }

    #[test]
    fn channel_tracks_reply_to() {
        let channel = Channel::new();
        let msg1 = channel.send("claude_code", "Question?", None, None);
        let msg2 = channel.send("lu", "Answer!", Some(msg1.id), None);

        assert_eq!(msg2.reply_to, Some(1));
    }
}
