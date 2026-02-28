//! Conversation memory for context retention across messages.
//!
//! Provides a two-tier memory system:
//! - Short-term: `SQLite` sliding window (last N messages per user)
//! - Long-term: Vault files via MCP (persisted conversations)

#![allow(clippy::significant_drop_tightening)]

use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::config::MemoryConfig;

/// A message in conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

/// Short-term conversation memory backed by `SQLite`.
///
/// Thread-safe via internal `Mutex`.
pub struct Memory {
    conn: Mutex<Connection>,
    window_size: usize,
    persist_threshold: usize,
    max_context_bytes: usize,
}

impl Memory {
    /// Open or create the memory database.
    ///
    /// # Arguments
    /// * `db_path` - Path to `SQLite` database file
    /// * `config` - Memory configuration (window size, persist threshold)
    pub fn open(db_path: &Path, config: &MemoryConfig) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }

        let conn = Connection::open(db_path)
            .with_context(|| format!("Failed to open database: {}", db_path.display()))?;

        // Initialize schema
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id INTEGER NOT NULL,
                timestamp TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                persisted INTEGER DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_user_time ON messages(user_id, timestamp DESC);",
        )
        .context("Failed to initialize database schema")?;

        Ok(Self {
            conn: Mutex::new(conn),
            window_size: config.window_size,
            persist_threshold: config.persist_threshold,
            max_context_bytes: config.max_context_bytes,
        })
    }

    /// Add a message to the conversation history.
    ///
    /// Content is compacted to reduce storage footprint while preserving semantics.
    pub fn add_message(&self, user_id: i64, role: &str, content: &str) -> Result<()> {
        let timestamp = Utc::now().to_rfc3339();
        let compacted = compact_content(content);
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

        conn.execute(
            "INSERT INTO messages (user_id, timestamp, role, content) VALUES (?1, ?2, ?3, ?4)",
            params![user_id, timestamp, role, compacted],
        )
        .context("Failed to insert message")?;

        Ok(())
    }

    /// Get recent conversation context for a user.
    ///
    /// Returns the last `window_size` messages in chronological order,
    /// trimmed to stay within `max_context_bytes`.
    pub fn get_context(&self, user_id: i64) -> Result<Vec<ConversationMessage>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

        let mut stmt = conn.prepare(
            "SELECT role, content, timestamp FROM messages
             WHERE user_id = ?1
             ORDER BY timestamp DESC
             LIMIT ?2",
        )?;

        let messages: Vec<ConversationMessage> = stmt
            .query_map(params![user_id, self.window_size], |row| {
                let timestamp_str: String = row.get(2)?;
                let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
                    .map_or_else(|_| Utc::now(), |dt| dt.with_timezone(&Utc));

                Ok(ConversationMessage {
                    role: row.get(0)?,
                    content: row.get(1)?,
                    timestamp,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        // Reverse to get chronological order (oldest first)
        let mut messages = messages;
        messages.reverse();

        // Enforce byte limit by trimming oldest messages first
        self.trim_to_byte_limit(&mut messages);

        Ok(messages)
    }

    /// Trim messages from the front (oldest) to stay within byte limit.
    fn trim_to_byte_limit(&self, messages: &mut Vec<ConversationMessage>) {
        let mut total_bytes: usize = messages.iter().map(|m| m.content.len()).sum();

        while total_bytes > self.max_context_bytes && !messages.is_empty() {
            if let Some(oldest) = messages.first() {
                total_bytes -= oldest.content.len();
            }
            messages.remove(0);
        }
    }

    /// Get configuration values for debugging.
    #[must_use]
    pub const fn config(&self) -> (usize, usize, usize) {
        (
            self.window_size,
            self.persist_threshold,
            self.max_context_bytes,
        )
    }
}

/// Compact message content to reduce storage/memory footprint.
///
/// - Collapses multiple whitespace to single space
/// - Trims leading/trailing whitespace
/// - Removes excessive newlines (max 2 consecutive)
/// - Preserves semantic content
fn compact_content(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut prev_was_whitespace = false;
    let mut newline_count = 0;

    for ch in content.chars() {
        if ch == '\n' {
            newline_count += 1;
            if newline_count <= 2 {
                result.push('\n');
            }
            prev_was_whitespace = true;
        } else if ch.is_whitespace() {
            if !prev_was_whitespace {
                result.push(' ');
            }
            prev_was_whitespace = true;
            newline_count = 0;
        } else {
            result.push(ch);
            prev_was_whitespace = false;
            newline_count = 0;
        }
    }

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_memory() -> (Memory, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let config = MemoryConfig {
            window_size: 4,
            persist_threshold: 8,
            max_context_bytes: 32 * 1024,
        };
        let memory = Memory::open(&db_path, &config).unwrap();
        (memory, dir)
    }

    #[test]
    fn add_and_retrieve_messages() {
        let (memory, _dir) = test_memory();
        let user_id = 12345;

        memory.add_message(user_id, "user", "Hello").unwrap();
        memory
            .add_message(user_id, "assistant", "Hi there!")
            .unwrap();

        let context = memory.get_context(user_id).unwrap();
        assert_eq!(context.len(), 2);
        assert_eq!(context[0].role, "user");
        assert_eq!(context[0].content, "Hello");
        assert_eq!(context[1].role, "assistant");
        assert_eq!(context[1].content, "Hi there!");
    }

    #[test]
    fn window_size_limits_context() {
        let (memory, _dir) = test_memory();
        let user_id = 12345;

        // Add more messages than window size (4)
        for i in 0..6 {
            memory
                .add_message(user_id, "user", &format!("Message {i}"))
                .unwrap();
        }

        let context = memory.get_context(user_id).unwrap();
        assert_eq!(context.len(), 4); // Only last 4
        assert_eq!(context[0].content, "Message 2"); // Oldest in window
        assert_eq!(context[3].content, "Message 5"); // Most recent
    }

    #[test]
    fn users_have_separate_histories() {
        let (memory, _dir) = test_memory();

        memory.add_message(100, "user", "User A message").unwrap();
        memory.add_message(200, "user", "User B message").unwrap();

        let context_a = memory.get_context(100).unwrap();
        let context_b = memory.get_context(200).unwrap();

        assert_eq!(context_a.len(), 1);
        assert_eq!(context_b.len(), 1);
        assert_eq!(context_a[0].content, "User A message");
        assert_eq!(context_b[0].content, "User B message");
    }

    #[test]
    fn compact_collapses_whitespace() {
        assert_eq!(compact_content("hello   world"), "hello world");
        assert_eq!(compact_content("  leading"), "leading");
        assert_eq!(compact_content("trailing  "), "trailing");
        assert_eq!(compact_content("a\n\n\n\nb"), "a\n\nb"); // Max 2 newlines
    }

    #[test]
    fn byte_limit_trims_oldest_messages() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let config = MemoryConfig {
            window_size: 10, // Allow many messages
            persist_threshold: 20,
            max_context_bytes: 50, // But strict byte limit
        };
        let memory = Memory::open(&db_path, &config).unwrap();
        let user_id = 12345;

        // Add messages that exceed byte limit
        memory.add_message(user_id, "user", "AAAAAAAAAA").unwrap(); // 10 bytes
        memory.add_message(user_id, "user", "BBBBBBBBBB").unwrap(); // 10 bytes
        memory.add_message(user_id, "user", "CCCCCCCCCC").unwrap(); // 10 bytes
        memory.add_message(user_id, "user", "DDDDDDDDDD").unwrap(); // 10 bytes
        memory.add_message(user_id, "user", "EEEEEEEEEE").unwrap(); // 10 bytes
        memory.add_message(user_id, "user", "FFFFFFFFFF").unwrap(); // 10 bytes = 60 total

        let context = memory.get_context(user_id).unwrap();
        // Should trim to stay under 50 bytes
        let total_bytes: usize = context.iter().map(|m| m.content.len()).sum();
        assert!(
            total_bytes <= 50,
            "Should be under 50 bytes, got {total_bytes}"
        );
        // Should have trimmed the oldest message(s)
        assert!(!context.iter().any(|m| m.content == "AAAAAAAAAA"));
    }
}
