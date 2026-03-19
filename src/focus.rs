//! Focus layer for file tracking.
//!
//! Tracks files that have been accessed during a conversation,
//! providing context about what the user is currently working on.
//! Think of it as "working memory" - the desk where open documents sit.

#![allow(clippy::significant_drop_tightening)]

use std::fmt::Write as _;
use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::config::FocusConfig;

/// A file currently in focus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusFile {
    /// Path relative to vault root
    pub file_path: String,
    /// When the file was last accessed
    pub last_accessed: DateTime<Utc>,
    /// Preview of file contents (first N chars)
    pub preview: String,
    /// Total lines in file
    pub line_count: usize,
    /// File size in bytes
    pub file_size: usize,
}

/// Focus tracking backed by `SQLite`.
///
/// Thread-safe via internal `Mutex`.
pub struct Focus {
    conn: Mutex<Connection>,
    max_files: usize,
    max_age_secs: u64,
    preview_chars: usize,
}

impl Focus {
    /// Open or create the focus database.
    ///
    /// # Arguments
    /// * `db_path` - Path to `SQLite` database file
    /// * `config` - Focus configuration
    pub fn open(db_path: &Path, config: &FocusConfig) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }

        let conn = Connection::open(db_path)
            .with_context(|| format!("Failed to open database: {}", db_path.display()))?;

        // Initialize schema
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS focus_files (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id INTEGER NOT NULL,
                file_path TEXT NOT NULL,
                last_accessed TEXT NOT NULL,
                preview TEXT,
                line_count INTEGER,
                file_size INTEGER,
                UNIQUE(user_id, file_path)
            );
            CREATE INDEX IF NOT EXISTS idx_focus_user ON focus_files(user_id, last_accessed DESC);",
        )
        .context("Failed to initialize focus database schema")?;

        Ok(Self {
            conn: Mutex::new(conn),
            max_files: config.max_files,
            max_age_secs: config.max_age_secs,
            preview_chars: config.preview_chars,
        })
    }

    /// Record that a file was accessed.
    ///
    /// Called after a successful `read_file` operation.
    /// Extracts metadata and upserts into focus table.
    pub fn touch(&self, user_id: i64, file_path: &str, content: &str) -> Result<()> {
        let timestamp = Utc::now().to_rfc3339();

        // Extract metadata
        let line_count = content.lines().count();
        let file_size = content.len();
        let preview = extract_preview(content, self.preview_chars);

        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

        // Upsert: insert or replace if exists
        conn.execute(
            "INSERT INTO focus_files (user_id, file_path, last_accessed, preview, line_count, file_size)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(user_id, file_path) DO UPDATE SET
                last_accessed = excluded.last_accessed,
                preview = excluded.preview,
                line_count = excluded.line_count,
                file_size = excluded.file_size",
            params![user_id, file_path, timestamp, preview, line_count, file_size],
        )
        .context("Failed to touch focus file")?;

        // Prune old entries and enforce max files
        self.prune_internal(&conn, user_id)?;

        Ok(())
    }

    /// Get files currently in focus for a user.
    ///
    /// Returns files ordered by last accessed (most recent first).
    pub fn get_focus(&self, user_id: i64) -> Result<Vec<FocusFile>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

        let mut stmt = conn.prepare(
            "SELECT file_path, last_accessed, preview, line_count, file_size
             FROM focus_files
             WHERE user_id = ?1
             ORDER BY last_accessed DESC
             LIMIT ?2",
        )?;

        let files: Vec<FocusFile> = stmt
            .query_map(params![user_id, self.max_files], |row| {
                let timestamp_str: String = row.get(1)?;
                let last_accessed = DateTime::parse_from_rfc3339(&timestamp_str)
                    .map_or_else(|_| Utc::now(), |dt| dt.with_timezone(&Utc));

                Ok(FocusFile {
                    file_path: row.get(0)?,
                    last_accessed,
                    preview: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    line_count: row.get::<_, Option<usize>>(3)?.unwrap_or(0),
                    file_size: row.get::<_, Option<usize>>(4)?.unwrap_or(0),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(files)
    }

    /// Clear all focus for a user.
    #[allow(dead_code)] // Part of public API, not yet used
    pub fn clear(&self, user_id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

        conn.execute(
            "DELETE FROM focus_files WHERE user_id = ?1",
            params![user_id],
        )
        .context("Failed to clear focus")?;

        Ok(())
    }

    /// Remove a specific file from focus.
    #[allow(dead_code)] // Part of public API, not yet used
    pub fn remove(&self, user_id: i64, file_path: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

        conn.execute(
            "DELETE FROM focus_files WHERE user_id = ?1 AND file_path = ?2",
            params![user_id, file_path],
        )
        .context("Failed to remove focus file")?;

        Ok(())
    }

    /// Get configuration values for debugging.
    #[must_use]
    pub const fn config(&self) -> (usize, u64, usize) {
        (self.max_files, self.max_age_secs, self.preview_chars)
    }

    /// Prune old entries (internal, called with lock held).
    #[allow(clippy::cast_possible_wrap)]
    fn prune_internal(&self, conn: &Connection, user_id: i64) -> Result<()> {
        // Remove entries older than max_age (cast is safe for practical values)
        let cutoff = Utc::now() - Duration::seconds(self.max_age_secs as i64);
        let cutoff_str = cutoff.to_rfc3339();

        conn.execute(
            "DELETE FROM focus_files WHERE user_id = ?1 AND last_accessed < ?2",
            params![user_id, cutoff_str],
        )?;

        // Enforce max_files by removing oldest entries beyond limit
        conn.execute(
            "DELETE FROM focus_files
             WHERE user_id = ?1
             AND id NOT IN (
                 SELECT id FROM focus_files
                 WHERE user_id = ?1
                 ORDER BY last_accessed DESC
                 LIMIT ?2
             )",
            params![user_id, self.max_files],
        )?;

        Ok(())
    }
}

/// Extract a preview from content.
///
/// Takes the first N characters, but tries to end at a newline
/// or word boundary for cleaner output.
fn extract_preview(content: &str, max_chars: usize) -> String {
    if content.len() <= max_chars {
        return content.to_string();
    }

    let truncated: String = content.chars().take(max_chars).collect();

    // Try to end at a newline
    if let Some(last_newline) = truncated.rfind('\n') {
        if last_newline > max_chars / 2 {
            return truncated[..last_newline].to_string();
        }
    }

    // Otherwise end at a word boundary
    if let Some(last_space) = truncated.rfind(' ') {
        if last_space > max_chars * 3 / 4 {
            return format!("{}...", truncated[..last_space].trim());
        }
    }

    format!("{}...", truncated.trim())
}

/// Format focus files for inclusion in system prompt.
pub fn format_focus_context(files: &[FocusFile]) -> String {
    if files.is_empty() {
        return String::new();
    }

    let mut output = String::from("\n\n## Files in Focus\n\n");
    output.push_str("You are currently working with these files:\n\n");

    for file in files {
        let age = format_age(file.last_accessed);
        let size = format_size(file.file_size);

        let _ = writeln!(
            output,
            "- {} ({} lines, {})\n  Last accessed: {}",
            file.file_path, file.line_count, size, age
        );

        if !file.preview.is_empty() {
            // Indent preview lines
            let preview_lines: Vec<&str> = file.preview.lines().take(3).collect();
            output.push_str("  Preview: \"");
            output.push_str(&preview_lines.join(" / "));
            if file.preview.lines().count() > 3 {
                output.push_str("...");
            }
            output.push_str("\"\n");
        }
        output.push('\n');
    }

    output.push_str("If you need the full content of any file, use read_file to fetch it again.\n");
    output.push_str(
        "Don't hesitate to re-read files - it's better to have current content than to guess.\n",
    );

    output
}

/// Format a duration as human-readable age.
fn format_age(timestamp: DateTime<Utc>) -> String {
    let now = Utc::now();
    let diff = now.signed_duration_since(timestamp);

    let secs = diff.num_seconds();
    if secs < 60 {
        return "just now".to_string();
    }

    let mins = diff.num_minutes();
    if mins < 60 {
        return format!("{} minute{} ago", mins, if mins == 1 { "" } else { "s" });
    }

    let hours = diff.num_hours();
    format!("{} hour{} ago", hours, if hours == 1 { "" } else { "s" })
}

/// Format file size as human-readable.
#[allow(clippy::cast_precision_loss)]
fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} bytes")
    } else {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_focus() -> (Focus, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test_focus.db");
        let config = FocusConfig {
            max_files: 3,
            max_age_secs: 3600,
            preview_chars: 100,
        };
        let focus = Focus::open(&db_path, &config).unwrap();
        (focus, dir)
    }

    #[test]
    fn touch_and_retrieve_file() {
        let (focus, _dir) = test_focus();
        let user_id = 12345;

        focus
            .touch(
                user_id,
                "notes/todo.md",
                "# Todo\n\n- Buy milk\n- Call dentist",
            )
            .unwrap();

        let files = focus.get_focus(user_id).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_path, "notes/todo.md");
        assert_eq!(files[0].line_count, 4);
        assert!(files[0].preview.contains("Todo"));
    }

    #[test]
    fn touch_updates_existing_file() {
        let (focus, _dir) = test_focus();
        let user_id = 12345;

        focus.touch(user_id, "notes/todo.md", "version 1").unwrap();
        focus
            .touch(user_id, "notes/todo.md", "version 2 with more content")
            .unwrap();

        let files = focus.get_focus(user_id).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].preview.contains("version 2"));
    }

    #[test]
    fn max_files_enforced() {
        let (focus, _dir) = test_focus();
        let user_id = 12345;

        // Add more than max_files (3)
        focus.touch(user_id, "file1.md", "content 1").unwrap();
        focus.touch(user_id, "file2.md", "content 2").unwrap();
        focus.touch(user_id, "file3.md", "content 3").unwrap();
        focus.touch(user_id, "file4.md", "content 4").unwrap();
        focus.touch(user_id, "file5.md", "content 5").unwrap();

        let files = focus.get_focus(user_id).unwrap();
        assert_eq!(files.len(), 3); // Only 3 most recent
        assert_eq!(files[0].file_path, "file5.md"); // Most recent first
        assert_eq!(files[1].file_path, "file4.md");
        assert_eq!(files[2].file_path, "file3.md");
    }

    #[test]
    fn users_have_separate_focus() {
        let (focus, _dir) = test_focus();

        focus.touch(100, "user_a.md", "user A content").unwrap();
        focus.touch(200, "user_b.md", "user B content").unwrap();

        let files_a = focus.get_focus(100).unwrap();
        let files_b = focus.get_focus(200).unwrap();

        assert_eq!(files_a.len(), 1);
        assert_eq!(files_b.len(), 1);
        assert_eq!(files_a[0].file_path, "user_a.md");
        assert_eq!(files_b[0].file_path, "user_b.md");
    }

    #[test]
    fn clear_removes_all_focus() {
        let (focus, _dir) = test_focus();
        let user_id = 12345;

        focus.touch(user_id, "file1.md", "content 1").unwrap();
        focus.touch(user_id, "file2.md", "content 2").unwrap();

        focus.clear(user_id).unwrap();

        let files = focus.get_focus(user_id).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn remove_specific_file() {
        let (focus, _dir) = test_focus();
        let user_id = 12345;

        focus.touch(user_id, "file1.md", "content 1").unwrap();
        focus.touch(user_id, "file2.md", "content 2").unwrap();

        focus.remove(user_id, "file1.md").unwrap();

        let files = focus.get_focus(user_id).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_path, "file2.md");
    }

    #[test]
    fn extract_preview_short_content() {
        let content = "Short content";
        let preview = extract_preview(content, 100);
        assert_eq!(preview, content);
    }

    #[test]
    fn extract_preview_truncates_at_newline() {
        let content = "Line 1\nLine 2\nLine 3\nLine 4\nLine 5";
        let preview = extract_preview(content, 20);
        // Should truncate at a newline if reasonable
        assert!(preview.len() <= 20 || preview.ends_with("..."));
    }

    #[test]
    fn format_focus_context_empty() {
        let context = format_focus_context(&[]);
        assert!(context.is_empty());
    }

    #[test]
    fn format_focus_context_with_files() {
        let files = vec![FocusFile {
            file_path: "notes/todo.md".to_string(),
            last_accessed: Utc::now(),
            preview: "# Todo\n- Buy milk".to_string(),
            line_count: 10,
            file_size: 256,
        }];

        let context = format_focus_context(&files);
        assert!(context.contains("Files in Focus"));
        assert!(context.contains("notes/todo.md"));
        assert!(context.contains("10 lines"));
        assert!(context.contains("read_file"));
    }
}
