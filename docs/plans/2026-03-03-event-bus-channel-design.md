# Event Bus & Channel Design

Two-way communication between Claude Code and Lu via a generic event bus on Mac MCP.

## Goals

- Claude Code can send messages to Lu and receive responses
- Lu processes CC messages automatically (like user messages)
- All conversations logged to vault for searchability
- Extensible event system for future use cases (reminders, vault changes, etc.)

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         Mac MCP Server                          │
│                                                                 │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────────────┐ │
│  │ Event Bus   │    │  Channel    │    │  Vault Logger       │ │
│  │             │    │  (messages) │    │  (.lu/channel/)     │ │
│  │ - publish() │───▶│ - send()    │───▶│  - log_event()      │ │
│  │ - subscribe │    │ - history() │    │                     │ │
│  └──────┬──────┘    └─────────────┘    └─────────────────────┘ │
│         │                                                       │
│         │ SSE: GET /events                                      │
└─────────┼───────────────────────────────────────────────────────┘
          │
    ┌─────┴─────┐
    │           │
    ▼           ▼
┌───────┐   ┌─────────────┐
│ Pi Lu │   │ Claude Code │
│       │   │             │
│ SSE   │   │ MCP tools   │
│ client│   │             │
└───────┘   └─────────────┘
```

## Event Format

```json
{
  "id": 12345,
  "type": "channel_message",
  "timestamp": "2026-03-03T20:45:00Z",
  "data": { ... }
}
```

### Event Types

| Type | Source | Data | Purpose |
|------|--------|------|---------|
| `channel_message` | Claude Code or Lu | `{from, to, content}` | Bidirectional chat |
| `channel_typing` | Claude Code or Lu | `{from}` | Typing indicator |
| `channel_error` | Mac MCP | `{reply_to, error}` | Error responses |
| `system_status` | Mac MCP | `{service, status}` | Health updates |

### Channel Message Data

```json
{
  "from": "claude_code",
  "to": "lu",
  "content": "Can you search for notes about project planning?",
  "context": null,
  "reply_to": null
}
```

## API Endpoints

| Method | Endpoint | Purpose |
|--------|----------|---------|
| `GET` | `/events?subscriber={id}` | SSE stream of events |
| `POST` | `/channel/send` | Send a channel message |
| `GET` | `/channel/history?limit=20` | Get recent messages |

### SSE Stream

```
GET /events?subscriber=pi_bot
Authorization: Bearer $TOKEN

< HTTP/1.1 200 OK
< Content-Type: text/event-stream

data: {"id":1,"type":"channel_message","data":{"from":"claude_code","content":"Hi"}}

: keepalive
```

### Channel Send

```
POST /channel/send
Authorization: Bearer $TOKEN
Content-Type: application/json

{"from": "claude_code", "content": "Search for project notes"}

< {"id": 123, "status": "sent"}
```

## Pi Integration

Pi maintains SSE connection to Mac MCP, dispatches events to handlers:

```rust
match event.type {
    "channel_message" => {
        if event.data.from != "lu" {
            let response = llm.chat(&event.data.content).await?;
            mcp.channel_send("lu", &response).await?;
        }
    }
    "system_status" => { /* log */ }
    _ => { /* ignore */ }
}
```

**Reconnection:** Exponential backoff (1s, 2s, 4s, max 30s).

## Claude Code Integration

MCP tools for sending/receiving:

| Tool | Purpose |
|------|---------|
| `channel_send` | Send message to Lu |
| `channel_history` | Get recent conversation |

Claude Code reads responses from vault log at `.lu/channel/YYYY-MM-DD.md`.

## Vault Log Format

```markdown
# Channel Log - 2026-03-03

### 20:45:00 [claude_code → lu]
Search for notes about project planning

### 20:45:03 [lu → claude_code]
Found 3 relevant notes:
- projects/planning-guide.md
...
```

## Error Handling

| Scenario | Handling |
|----------|----------|
| Pi SSE disconnects | Auto-reconnect with backoff |
| Mac MCP down | Pi queues messages, retries on reconnect |
| Lu processing fails | Error event sent to channel |
| Invalid message | 400 response, not published |

## Files to Create/Modify

### Mac MCP (Python)

| File | Change |
|------|--------|
| `src/mcp/event_bus.py` | NEW: Event bus with pub/sub |
| `src/mcp/channel.py` | NEW: Channel message handling |
| `src/mcp/server.py` | Add `/events`, `/channel/*` endpoints |
| `src/mcp/tools/channel.py` | NEW: MCP tools for Claude Code |

### Pi Bot (Rust)

| File | Change |
|------|--------|
| `src/sse_client.rs` | NEW: SSE client with reconnection |
| `src/event_handler.rs` | NEW: Event dispatcher |
| `src/bot.rs` | Spawn SSE task on startup |

## Testing

```bash
# Test channel send
curl -X POST http://localhost:8201/channel/send \
  -H "Authorization: Bearer $TOKEN" \
  -d '{"from":"test","content":"ping"}'

# Check vault log
cat ~/.ludolph/vault/.lu/channel/$(date +%Y-%m-%d).md

# Watch events
curl -N http://localhost:8201/events?subscriber=test \
  -H "Authorization: Bearer $TOKEN"
```

## Future Extensions

Event types that can be added without architectural changes:
- `reminder` - Scheduled reminders
- `vault_change` - File modifications
- `task_update` - Task status changes
