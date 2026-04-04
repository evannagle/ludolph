# Ludolph Memory System

Lu remembers things in three ways: short-term conversation context (what you just said), long-term conversation logs (what you said last week), and observations (what Lu has learned about you as a person). The first two are mechanical — messages in, messages stored. Observations are the interesting part.

## Observations

Observations are facts Lu saves about you across conversations. They're the closest thing Lu has to actually knowing who you are.

Tell Lu something once:
- "I prefer morning briefs without newsletters"
- "Elvis is my son, he's 4"
- "I'm working on a book about a philosopher named Karl"

Lu saves it as an observation with a category:

| Category | What it stores | Example |
|----------|---------------|---------|
| preference | How you like things done | "Prefers terse responses, no summaries" |
| fact | Biographical details | "Has a son named Elvis, age 4" |
| context | Active projects and goals | "Book project: Karl, satirical nonfiction" |

Observations are loaded into Lu's system prompt on every conversation. You don't need to repeat yourself. If something changes, just tell Lu — "Actually, Elvis is 5 now" — and the observation updates.

To see what Lu knows: ask "What do you know about me?" in Telegram.

Observations are stored in SQLite at `~/.ludolph/observations.db` on the Mac. They're separate from the conversation log and the vault — they're Lu's working model of you, not a record of what was said.

For the difference between observations and the knowledge base (files, URLs, repos), see [Learning and Teaching](learn.md).

## Conversation Memory

The conversation system is more mechanical. Here's how it works.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           Raspberry Pi                                  │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │  Short-term Memory (SQLite)                                      │   │
│  │  ~/.ludolph/conversations.db                                     │   │
│  │                                                                  │   │
│  │  • Last N messages per user (configurable, default 8)            │   │
│  │  • Injected into every Claude API call                           │   │
│  │  • Lightweight, fast access                                      │   │
│  │  • Auto-persists to long-term when window fills                  │   │
│  └─────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    │ MCP: save_conversation()
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                              Mac (MCP Server)                           │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │  Long-term Memory (Vault Files)                                  │   │
│  │  ~/Vault/.lu/conversations/                                      │   │
│  │                                                                  │   │
│  │  • Markdown files organized by date (YYYY-MM-DD.md)              │   │
│  │  • Searchable via existing vault tools                           │   │
│  │  • Full conversation history preserved                           │   │
│  │  • Accessible to Lu for context recall                           │   │
│  └─────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
```

## Data Flow

1. **User sends message** → Pi receives via Telegram
2. **Load context** → Pi reads last N messages from SQLite
3. **API call** → Pi sends message + context to Claude
4. **Store exchange** → Pi saves user message + Lu response to SQLite
5. **Persist overflow** → If SQLite has >N messages, oldest are sent to MCP and deleted locally

## Memory Scope

Memory is **per-user**, not per-chat. This means:
- A single conversation thread follows each user across DMs and group chats
- Lu remembers context whether you message in a group or directly
- Different users have isolated conversation histories

## Short-term Memory (Pi-side)

### Configuration

Add to `~/.ludolph/config.toml`:

```toml
[memory]
# Number of recent messages to keep in context (default: 8)
# Lower this if running on resource-constrained Pi
window_size = 8

# Maximum messages before auto-persist triggers (default: 16)
# When reached, oldest messages are sent to vault and deleted locally
persist_threshold = 16
```

### SQLite Schema

```sql
CREATE TABLE messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,      -- Telegram user_id (per-user scope)
    timestamp TEXT NOT NULL,       -- ISO 8601
    role TEXT NOT NULL,            -- 'user' | 'assistant'
    content TEXT NOT NULL,
    persisted INTEGER DEFAULT 0    -- 1 if written to vault
);

CREATE INDEX idx_user_time ON messages(user_id, timestamp DESC);
```

### Resource Considerations

The SQLite database is designed to stay small:
- Only active conversation windows are kept
- Older messages are persisted to vault and deleted
- Default settings use ~50KB per active user

For very constrained environments (e.g., older Pi models):
```toml
[memory]
window_size = 4              # Smaller context window
persist_threshold = 8        # More aggressive persistence
max_context_bytes = 16384    # 16KB limit (default is 32KB)
```

Content is automatically compacted (whitespace collapsed) before storage to reduce footprint.

## Long-term Memory (MCP-side)

### MCP Tools

The MCP server exposes memory tools following the claude-mem pattern:

| Tool | Purpose |
|------|---------|
| `save_conversation` | Persist messages from Pi to vault files |
| `search_conversations` | Search past conversations by content |
| `get_conversation` | Retrieve a specific conversation by date |

### Vault Storage

Conversations are stored in `.lu/conversations/` within the vault:

```
~/Vault/.lu/conversations/
├── 2026-02-25.md
├── 2026-02-24.md
└── ...
```

### File Format

```markdown
## 2026-02-25

### 10:32 AM
**User**: Who is Jaimie Nagle?

**Lu**: Based on your vault, Jaimie Nagle is your wife. I found references in:
- journal/2022-07-09.md (birthday celebration)
- people/jaimie.md (profile note)

---

### 10:35 AM
**User**: Can you fix the note where it says she turned 29?

**Lu**: I found that reference in journal/2022-07-09.md. The note says she
turned 29, but based on your question, it should be 39. Would you like me
to show you that file so you can update it?

---
```

### Searchability

Long-term memories are searchable via existing vault tools:
- `search` tool can find conversations by content
- `read_file` can retrieve specific conversation files
- Lu's system prompt instructs it to search `.lu/conversations/` when asked about past discussions

## System Prompt Integration

Lu's system prompt includes awareness of the memory system:

```
You have access to conversation history:
- Recent messages from this user are included in context below
- Older conversations are stored in .lu/conversations/ - search there if the
  user asks about past discussions ("what did we talk about last week?")
```

## Privacy Notes

- Conversations are stored locally (SQLite on Pi, files in your vault)
- No conversation data is sent to external services beyond Claude API calls
- The vault's `.lu/` directory can be excluded from sync if desired

## Troubleshooting

### Check short-term memory
```bash
sqlite3 ~/.ludolph/conversations.db "SELECT * FROM messages ORDER BY timestamp DESC LIMIT 10"
```

### Check long-term memory
```bash
ls -la ~/Vault/.lu/conversations/
cat ~/Vault/.lu/conversations/$(date +%Y-%m-%d).md
```

### Clear conversation history
```bash
# Short-term only
sqlite3 ~/.ludolph/conversations.db "DELETE FROM messages"

# Long-term: delete files in .lu/conversations/
```

### Memory not working?
1. Check config has `[memory]` section (or defaults will be used)
2. Verify MCP connection for long-term storage: `lu poke`
3. Check SQLite database exists: `ls ~/.ludolph/conversations.db`
