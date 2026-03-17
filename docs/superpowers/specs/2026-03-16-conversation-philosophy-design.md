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

### 3. Topic Storage

Location: `.lu/conversations/{user_id}.json` in vault

Topics stay in vault files (already implemented in `conversation_scope` tool). This keeps storage Mac-side where tools can access it directly.

Architecture note: `memory.rs` (Rust) runs on Pi for local message caching. Topic state lives in vault files accessible to Mac's Python MCP server.

File structure:
```json
{
  "id": "user_123",
  "created": "2026-03-16T...",
  "topics": ["Parker project", "Call Mom", "Recipe"],
  "resolved": ["Parker project"],
  "current": "Call Mom",
  "notes": []
}
```

### 4. conversation_scope Tool

Location: `src/mcp/tools/conversation.py`

Already implemented with vault file storage. No changes needed to storage mechanism.

Add helper to check for stale topics (>24h):
```python
def expire_stale_topics(conversation_id: str, max_age_hours: int = 24) -> int:
    """Move topics older than max_age_hours to 'stale' status."""
    ...
```

### 5. Context Loading

Location: `src/mcp/server.py` (Mac-side, where LLM calls happen)

The Mac MCP server builds context before calling LLM. Update to include:

1. Core principles (added to system prompt construction)
2. Philosophy file via `read_file` tool
3. Open topics via `conversation_scope(action="list")`
4. Lu.md (existing)

New helper in server.py:

```python
async def load_philosophy_context() -> str | None:
    """Load .lu/philosophy.md, create with defaults if missing."""
    result = call_tool("read_file", {"path": ".lu/philosophy.md"})

    if "not found" in result.get("content", "").lower():
        # Create default
        call_tool("write_file", {
            "path": ".lu/philosophy.md",
            "content": DEFAULT_PHILOSOPHY
        })
        return DEFAULT_PHILOSOPHY

    return result.get("content")
```

### 6. Open Topics in Context

Before each LLM call, check for open topics:

```python
def get_topics_context(user_id: str) -> str:
    """Get open topics for inclusion in system prompt."""
    result = conversation_scope(
        conversation_id=user_id,
        action="list"
    )

    # Parse result and format for prompt
    if "No topics" in result:
        return ""

    return f"\n\n## Open Topics\n{result}"
```

This gets prepended to the user's context so Lu always knows what's unresolved.

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
| `src/mcp/server.py` | Add philosophy loading, topics context, update chat endpoint |
| `src/mcp/tools/conversation.py` | Add stale topic expiration |
| `src/mcp/llm.py` | Update system prompt with core principles |
| `src/setup.rs` | Update setup wizard to use new principles |

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
