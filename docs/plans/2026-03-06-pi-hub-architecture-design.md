# Pi as Hub Architecture Design

Date: 2026-03-06
Status: Approved

## Problem

Claude Code communicates with Lu via the Mac's HTTP server, which acts as a relay. This creates unnecessary fail points:
- Mac must be awake for Claude Code to talk to Lu
- Two processes on Mac (HTTP server + MCP server) with separate state
- Messages stored in memory on Mac, lost on restart

## Solution

Make Pi the messaging hub. Mac provides vault access only.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                           MAC                                       │
│                                                                     │
│  ┌─────────────────┐                                               │
│  │  Claude Code    │                                               │
│  │                 │   MCP (stdio)    ┌─────────────────────┐      │
│  │  channel_send   │─────────────────►│  MCP Server         │      │
│  │  channel_history│◄─────────────────│  (Python, stdio)    │      │
│  └─────────────────┘                  └──────────┬──────────┘      │
│                                                  │                  │
│  ┌─────────────────────────────────────┐         │ HTTP             │
│  │  Flask Server (vault access only)   │         │ (to Pi)          │
│  │  /tools/call (read_file, search)    │         │                  │
│  │  Port 8201                          │         │                  │
│  └─────────────────────────────────────┘         │                  │
└──────────────────────────────────────────────────┼──────────────────┘
                                                   │
                                       ════════════════════════
                                              Internet
                                       ════════════════════════
                                                   │
┌──────────────────────────────────────────────────┼──────────────────┐
│                           PI                     │                  │
│                                                  ▼                  │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │  Ludolph Bot (Rust)                                          │  │
│  │                                                              │  │
│  │  /channel/send     ◄── Claude Code messages arrive here     │  │
│  │  /channel/history  ◄── Claude Code reads history here       │  │
│  │                                                              │  │
│  │  EventBus + Channel (in-memory, single process)              │  │
│  │                                                              │  │
│  │  Telegram Bot ◄── User messages arrive here                 │  │
│  └──────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
```

## Components

### Pi-side (Rust)

| Component | Change |
|-----------|--------|
| `src/api.rs` (new) | HTTP server exposing `/channel/send`, `/channel/history` |
| `src/channel.rs` (new) | Channel struct with in-memory message storage |
| `src/bot.rs` | Integrate channel - Lu responses go to channel |
| `src/main.rs` | Spawn HTTP server alongside Telegram bot |

### Mac-side (Python)

| Component | Change |
|-----------|--------|
| `src/mcp/mcp_server.py` | Call Pi's HTTP endpoint instead of local Channel |
| `src/mcp/server.py` | Remove `/channel/*` endpoints (keep vault tools only) |
| `src/mcp/channel.py` | Delete (no longer needed on Mac) |

### Configuration

| Setting | Location | Purpose |
|---------|----------|---------|
| `PI_HOST` | `~/.ludolph/config.toml` | Pi's address for MCP server |
| `PI_CHANNEL_PORT` | Same | Port for channel API (default: 8202) |
| `CHANNEL_AUTH_TOKEN` | Same | Shared secret for channel auth |

### Installer/Packaging

| Component | Change |
|-----------|--------|
| `docs/install` | Configure Pi channel server, generate auth token, prompt Mac for Pi host |
| `scripts/package-mcp.sh` | Include updated `mcp_server.py` |
| Pi binary release | Include channel HTTP server |
| Config template | Add `pi_host`, `channel_port`, `channel_auth_token` |

## Data Flow

### Claude Code to Lu

1. User invokes channel_send in Claude Code
2. MCP server receives tool call (stdio)
3. MCP server POSTs to `http://{PI_HOST}:{CHANNEL_PORT}/channel/send`
4. Pi stores in Channel, triggers Lu processing
5. Lu generates response, stores in Channel with reply_to
6. MCP server polls /channel/history
7. MCP server returns Lu's response to Claude Code

### Telegram User to Lu

1. User sends Telegram message
2. Pi bot receives via teloxide (existing)
3. Pi processes with Claude API (existing)
4. Response sent via Telegram (existing)
5. Optionally: message also stored in Channel

### Lu to Vault

1. Lu's tool call includes read_file, search, etc.
2. Pi calls Mac's Flask server: `http://{MAC_HOST}:8201/tools/call`
3. Mac reads vault, returns content
4. Pi forwards to Claude API

## Error Handling

### Pi unreachable

| Scenario | Behavior |
|----------|----------|
| MCP can't connect | Return: "Pi unreachable at {host}:{port}" |
| Timeout waiting for Lu | Return: "Message sent but no response within {timeout}s" |
| Auth token mismatch | Return: "Authentication failed" |

### Mac unreachable

| Scenario | Behavior |
|----------|----------|
| Pi can't reach Mac for vault | Lu receives tool error, can still respond conversationally |
| Mac sleeping | Pi triggers Wake-on-LAN, retry |

### Graceful degradation

1. Claude Code ↔ Lu conversation works (Pi only)
2. Lu can access vault files (requires Mac)
3. Message history persists in Pi memory

## Testing

### Unit tests (Pi - Rust)

- `channel_stores_messages`
- `channel_reply_threading`
- `api_auth_required`
- `api_send_returns_id`

### Unit tests (Mac - Python)

- `mcp_server_calls_pi`
- `mcp_server_handles_timeout`
- `mcp_server_parses_response`

### Integration tests

- `end_to_end_message`
- `vault_access_during_conversation`
- `pi_restart_recovers`

### Manual verification

- [ ] Claude Code can send message to Lu via MCP tool
- [ ] Lu responds within reasonable time
- [ ] channel_history shows conversation
- [ ] Telegram messages still work independently
- [ ] Lu can read vault files during MCP conversation
- [ ] Mac sleeping doesn't block Claude Code ↔ Lu
