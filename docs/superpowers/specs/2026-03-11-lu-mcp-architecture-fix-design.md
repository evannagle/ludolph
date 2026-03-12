# Lu/MCP Architecture Fix

**Date:** 2026-03-11
**Status:** Approved

## Summary

Fix reliability issues in Pi↔Mac communication for Ludolph. Pi is a thin client; Mac hosts all intelligence via MCP server.

## Problems Addressed

1. **Missing SSE endpoint** — Pi's SSE client connects to `/events` that doesn't exist on Mac
2. **Aggressive Wake-on-LAN** — WoL triggers on any first request failure, even when Mac is awake
3. **Malformed tool creation** — Lu generates tools with wrong schema keys, breaking all tool parsing
4. **Dual listener confusion** — Two listeners compete to process messages

## Architecture Decision

**Pi as thin client, Mac as brain.**

- Pi handles Telegram interface and forwards to Mac
- Mac runs MCP server with vault access, tools, and LLM proxy
- Mac pushes events to Pi via SSE

## Design

### 1. SSE Events Endpoint (Mac)

Add `/events` endpoint to `server.py`:

```python
@app.route("/events")
def events():
    subscriber = request.args.get("subscriber", "unknown")

    def generate():
        while True:
            # Check for new channel messages
            # Send heartbeat every 30s
            ...

    return Response(generate(), mimetype="text/event-stream")
```

**Events:**
- `channel_message` — New message in channel (from Claude Code)
- `heartbeat` — Keep-alive every 30s

**Flow:**
1. Claude Code → POST `/channel/send` on Mac
2. Mac stores message, pushes SSE event
3. Pi receives event, processes through LLM (via Mac's `/chat`)
4. Pi sends response via POST `/channel/send`

### 2. Smart Wake-on-LAN (Pi)

Replace aggressive WoL with smart recovery:

```rust
async fn request_with_recovery<T>(&self, request_fn: impl Fn()) -> Result<T> {
    match request_fn().await {
        Ok(result) => Ok(result),
        Err(_) => {
            // Quick health check (2s timeout)
            if self.health_check().await.is_ok() {
                // Mac is awake, just retry
                return request_fn().await;
            }
            // Mac unreachable, try WoL
            self.wake_and_retry(request_fn).await
        }
    }
}
```

**Logic:**
1. Request fails → quick health check
2. Health OK → retry immediately (transient failure)
3. Health fails → send WoL, wait 15s, retry
4. Still fails → return error

**Consolidation:** Extract WoL retry logic from 3 locations into one `wake_and_retry()` helper.

### 3. Tool Creation Validation (Mac)

**Strict validation in `meta.py`:**

```python
def _validate_tool_code(code: str) -> str | None:
    if '"parameters"' in code:
        return "Error: Use 'input_schema' not 'parameters'\n\nExample:\n  'input_schema': {'type': 'object', ...}"
    if '"inputSchema"' in code:
        return "Error: Use 'input_schema' (snake_case) not 'inputSchema'"
    if "input_schema" not in code:
        return "Error: TOOLS must include 'input_schema' for each tool"
    return None
```

**Resilient parsing in `mcp_client.rs`:**
- Already implemented: skip malformed tools with warning instead of failing

**Better tool description:**
- Update `create_tool` description with explicit format and example

### 4. Remove Dual Listener (Pi)

**Remove from `bot.rs`:**
- `spawn_channel_listener()` function
- `message_tx` channel creation
- `AppState.message_tx` field

**Keep:**
- Pi's channel API for `GET /health` and `GET /channel/history`
- SSE listener as the sole message processor

**Simplified flow:**
```
Claude Code → Mac /channel/send → SSE event → Pi SSE listener → Mac /chat → Mac /channel/send
```

### 5. Observability (Phase 2, Deferred)

**Structured logging:**
- Request IDs for tracing
- Tool fetch/call durations
- Connection state changes

**In-memory metrics:**
- Tool call counts/latencies
- SSE reconnection count
- WoL trigger count

**Log rotation:**
- Max 10MB per file, 3 rotated files
- Prevents Pi disk bloat

## Files Changed

| File | Change |
|------|--------|
| `src/mcp/server.py` | Add `/events` SSE endpoint |
| `src/mcp_client.rs` | Smart WoL with `request_with_recovery()` |
| `src/mcp/tools/meta.py` | Strict tool validation with examples |
| `src/bot.rs` | Remove `spawn_channel_listener`, keep SSE only |
| `src/api.rs` | Remove `message_tx` from `AppState` |

## Success Criteria

1. Pi can connect to Mac's `/events` endpoint without 404
2. WoL only triggers when Mac is actually unreachable
3. Lu cannot create tools with wrong schema keys
4. Single message processing path (no duplicates)
5. Lu can reliably access tools and respond to Claude Code messages

## Out of Scope

- Full telemetry/metrics (Phase 2)
- Pi-local MCP server (user chose thin client)
- Prometheus endpoint
