# Conversation Philosophy Implementation

Design spec for implementing Lu's conversation philosophy (scoping, pacing, Ma) into the system.

## Summary

Lu should conduct conversations with three principles: scope complexity before diving in, pace questions one at a time, and allow breathing room (Ma) when appropriate. This spec covers how these principles get embedded into Lu's behavior through prompts, files, and memory integration.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    System Prompt                         │
│  ┌─────────────────────────────────────────────────┐    │
│  │ Core Principles (~150 words)                     │    │
│  │ - Scoping, Pacing, Ma                            │    │
│  └─────────────────────────────────────────────────┘    │
│                         +                                │
│  ┌─────────────────────────────────────────────────┐    │
│  │ .lu/philosophy.md (loaded at runtime)            │    │
│  │ - Detailed guidance, examples, anti-patterns     │    │
│  └─────────────────────────────────────────────────┘    │
│                         +                                │
│  ┌─────────────────────────────────────────────────┐    │
│  │ Open Topics (from memory)                        │    │
│  │ - Unresolved topics from current conversation    │    │
│  └─────────────────────────────────────────────────┘    │
│                         +                                │
│  ┌─────────────────────────────────────────────────┐    │
│  │ Lu.md (user preferences)                         │    │
│  └─────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────┘
```

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Scoping trigger | Always-on (2+ topics) | Natural behavior, no user action needed |
| Scoping visibility | Silent | Tool is internal, conversations feel natural |
| Ma implementation | Contextual awareness | Not formulaic, read the room |
| Philosophy location | File-based + core prompt | Iterate without rebuild, stable core |
| Topic storage | Memory module (SQLite) | Single source of truth with messages |
| Missing philosophy file | Auto-create with defaults | Zero-config first run |

## Components

### 1. Core System Prompt Changes

Location: `src/llm.rs`, `build_system_prompt()` function

Add conversation principles before existing formatting rules:

```
CONVERSATION PRINCIPLES:

Scoping: When a message contains multiple topics or questions, silently
note them using conversation_scope, then address one at a time. Don't
announce the structure - just naturally work through them without losing
track.

Pacing: Ask one question per message. Wait for the response before asking
the next. Acknowledge what the user said before moving on.

Ma: Not every response needs to advance an agenda. Sometimes notice
something without acting on it. Sometimes appreciate a moment before
rushing forward. Read the user's energy - if they're reflective, be
reflective. If task-focused, stay efficient.

For detailed guidance, see the philosophy context below.
```

### 2. Philosophy File

Location: `.lu/philosophy.md` in user's vault

Auto-created on first conversation if missing.

```markdown
# Conversation Philosophy

## Scoping

When you detect 2+ topics in a message:
1. Call conversation_scope to register them
2. Address the first naturally
3. After resolving, transition: "Now about [next topic]..."
4. If user redirects, follow their lead

## Pacing

- One question per message
- Acknowledge before asking next
- Don't stack questions

## Ma

Read the room:
- User finished something big → pause, appreciate
- User is venting → listen, don't solve immediately
- User is task-focused → stay efficient
- Silence is okay

## Anti-patterns

Avoid:
- Question dumps
- Rushing past emotional moments
- "Great! Awesome!" empty acknowledgments
- Forgetting topics that were raised
```

### 3. Memory Integration

Location: `src/memory.rs`

Add topics table to existing SQLite schema:

```sql
CREATE TABLE IF NOT EXISTS topics (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    topic TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'open',  -- open, resolved, stale
    created_at TEXT NOT NULL,
    resolved_at TEXT,
    UNIQUE(user_id, topic)
);
CREATE INDEX IF NOT EXISTS idx_topics_user_status ON topics(user_id, status);
```

New methods on Memory struct:

```rust
pub fn add_topics(&self, user_id: i64, topics: &[&str]) -> Result<()>
pub fn resolve_topic(&self, user_id: i64, topic: &str) -> Result<bool>
pub fn get_open_topics(&self, user_id: i64) -> Result<Vec<String>>
pub fn clear_topics(&self, user_id: i64) -> Result<()>
pub fn expire_stale_topics(&self, user_id: i64, max_age_hours: u32) -> Result<u32>
```

### 4. conversation_scope Tool Update

Location: `src/mcp/tools/conversation.py`

Change from file-based storage to calling memory via MCP endpoint. Add new MCP endpoints:

```
POST /memory/topics/add     { user_id, topics: [] }
POST /memory/topics/resolve { user_id, topic }
GET  /memory/topics/open    { user_id }
POST /memory/topics/clear   { user_id }
```

Tool becomes thin wrapper around these endpoints.

### 5. Context Loading

Location: `src/llm.rs`

Update `build_system_prompt()` load order:

1. Core principles (hardcoded in Rust)
2. Philosophy file via `load_philosophy_context()`
3. Open topics via memory
4. Lu.md via existing `load_lu_context()`
5. Recent messages via existing memory loading

New function:

```rust
async fn load_philosophy_context(&self) -> Option<String> {
    let result = self
        .execute_tool("read_file", &json!({"path": ".lu/philosophy.md"}))
        .await;

    if result.contains("not found") {
        // Create default philosophy file
        self.execute_tool("write_file", &json!({
            "path": ".lu/philosophy.md",
            "content": DEFAULT_PHILOSOPHY
        })).await;
        Some(DEFAULT_PHILOSOPHY.to_string())
    } else if result.contains("Error:") {
        None
    } else {
        Some(result)
    }
}
```

### 6. Open Topics in Context

When building the system prompt, include open topics:

```rust
let open_topics = self.memory.get_open_topics(user_id)?;
let topics_context = if open_topics.is_empty() {
    String::new()
} else {
    format!(
        "\n\n## Open Topics\nThese topics were raised but not yet resolved:\n{}",
        open_topics.iter().map(|t| format!("- {}", t)).collect::<Vec<_>>().join("\n")
    )
};
```

## Edge Cases

| Scenario | Behavior |
|----------|----------|
| Philosophy file missing | Auto-create with defaults |
| 10+ topics | Scope all, work through naturally |
| Topic stale (>24h) | Auto-expire, Lu can ask if still relevant |
| User says "forget it" | Clear all open topics |
| Setup mode | Same scoping behavior |

## Files Changed

| File | Change |
|------|--------|
| `src/llm.rs` | Add principles to prompt, load philosophy file, include open topics |
| `src/memory.rs` | Add topics table and methods |
| `src/mcp/tools/conversation.py` | Switch from file-based to memory-based |
| `src/mcp/server.py` | Add /memory/topics/* endpoints |

## Testing

1. **Unit tests** for memory topic methods
2. **Integration test**: Send multi-topic message, verify topics tracked
3. **Integration test**: Resolve topic, verify state updates
4. **Manual test**: Full conversation flow with scoping

## Not Included (YAGNI)

- Topic priorities/ordering (FIFO is fine)
- Topic dependencies
- Cross-user topics
- Topic analytics
