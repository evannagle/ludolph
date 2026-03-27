# Focus Layer Design

## Problem

Lu loses track of files mid-conversation. When a user says "fix the typo," Lu doesn't remember which file they were editing and has to ask again. This breaks flow and makes the assistant feel forgetful.

**Root cause:** Tool results (including file contents from `read_file`) are not persisted in memory. Only user messages and assistant responses are stored. The file content lives in the tool loop and evaporates between turns.

## Solution: Focus Layer

Add a lightweight "working memory" layer that tracks files currently being worked on. Think of it as the desk where open documents sit, separate from the filing cabinet (conversation history).

## Data Model

```sql
CREATE TABLE focus_files (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    file_path TEXT NOT NULL,
    last_accessed TEXT NOT NULL,
    preview TEXT,           -- First ~500 chars or extracted summary
    line_count INTEGER,
    file_size INTEGER,
    UNIQUE(user_id, file_path)
);

CREATE INDEX idx_focus_user ON focus_files(user_id, last_accessed DESC);
```

**Fields:**
- `user_id` - Telegram user ID
- `file_path` - Relative path within vault
- `last_accessed` - ISO timestamp of last read
- `preview` - First ~500 characters for context
- `line_count` - Total lines in file
- `file_size` - Bytes, to gauge if refetch is worthwhile

## Behavior

### Automatic Focus (on read_file)

When `read_file` executes successfully:

1. Extract metadata:
   - Line count
   - File size
   - Preview (first 500 chars, or first 10 lines, whichever is smaller)

2. Upsert into `focus_files`:
   - If file exists for user, update `last_accessed` and `preview`
   - If new, insert

3. Prune old entries:
   - Keep max 5 files per user
   - Remove entries older than 1 hour
   - Prune on each new read

### System Prompt Injection

When building the system prompt, include focus context:

```
## Files in Focus

You are currently working with these files:

• poetry/chicken-sonnet.md (14 lines, 847 bytes)
  Last accessed: 2 minutes ago
  Preview: "The money flutters in the dust, / Its green like parrots molting..."

• notes/writing-ideas.md (42 lines, 2.1 KB)
  Last accessed: 8 minutes ago
  Preview: "# Writing Ideas\n\nPoems to revise:\n- chicken sonnet (needs volta)..."

If you need the full content of any file, use read_file to fetch it again.
Don't hesitate to re-read files - it's better to have current content than to guess.
```

### Refetch Encouragement

The system prompt should explicitly tell Lu:

> "If you're unsure about file contents, re-read the file. It's always better to fetch current content than to work from memory. The focus list shows what's been touched recently, but the preview is just a snippet."

This prevents Lu from:
- Guessing at content that's not in context
- Making edits based on stale memory
- Asking "what file?" when the focus list makes it clear

### Focus Expiry

Files leave focus when:
- **Time-based:** Not accessed for 1 hour
- **Count-based:** More than 5 files in focus (oldest removed)
- **Explicit:** User starts a new topic (detected via `/clear` or similar)

## API Changes

### Focus Module (`src/focus.rs`)

```rust
pub struct FocusFile {
    pub file_path: String,
    pub last_accessed: DateTime<Utc>,
    pub preview: String,
    pub line_count: usize,
    pub file_size: usize,
}

pub struct Focus {
    conn: Mutex<Connection>,
    max_files: usize,        // default 5
    max_age_secs: u64,       // default 3600 (1 hour)
    preview_chars: usize,    // default 500
}

impl Focus {
    pub fn open(db_path: &Path, config: &FocusConfig) -> Result<Self>;

    /// Record that a file was accessed. Called by read_file tool.
    pub fn touch(&self, user_id: i64, file_path: &str, content: &str) -> Result<()>;

    /// Get files currently in focus for a user.
    pub fn get_focus(&self, user_id: i64) -> Result<Vec<FocusFile>>;

    /// Clear all focus for a user.
    pub fn clear(&self, user_id: i64) -> Result<()>;

    /// Remove a specific file from focus.
    pub fn remove(&self, user_id: i64, file_path: &str) -> Result<()>;
}
```

### Integration Points

1. **read_file tool** - Call `focus.touch()` after successful read
2. **Llm::build_system_prompt()** - Include focus context
3. **Config** - Add `[focus]` section for tuning

### Config

```toml
[focus]
max_files = 5           # Max files in focus per user
max_age_secs = 3600     # Files expire after 1 hour
preview_chars = 500     # Characters to store for preview
```

## Optional: Focus Tools

Future enhancement - give Lu explicit control:

```
focus_list    - Show current focus (already in system prompt, but explicit tool)
focus_pin     - Keep a file in focus longer (doesn't auto-expire)
focus_clear   - Clear all focus, start fresh
```

Not in initial implementation. The automatic behavior should handle 90% of cases.

## Edge Cases

**Large files:** Preview is capped at 500 chars. For large files, Lu should refetch.

**Binary files:** Skip focus tracking for non-text files.

**Renamed/deleted files:** Focus entry becomes stale. Lu will get an error on refetch, which is fine - it surfaces the issue naturally.

**Multiple conversations:** Each user has independent focus. No cross-user leakage.

## Success Criteria

After implementation, this conversation should work:

```
User: Here's a poem I'm working on [shares file path]
Lu: [reads file, analyzes it, gives feedback]
User: fix the typo on line 13
Lu: [knows which file, re-reads it, fixes the typo]
```

Lu should never have to ask "which file?" when files are in focus.

## Testing

1. Read a file, verify it appears in focus
2. Read multiple files, verify oldest drops when exceeding max
3. Wait for expiry, verify file leaves focus
4. Clear user focus, verify empty
5. System prompt includes focus context
6. Preview is correctly truncated

## Non-Goals

- **Full file caching:** Focus stores previews, not full content. Lu still needs to refetch for edits.
- **Version tracking:** We don't track changes to files. Each read gets current content.
- **Cross-session persistence:** Focus is ephemeral. New session = fresh focus.
