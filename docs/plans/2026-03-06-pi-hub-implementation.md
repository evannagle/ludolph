# Pi as Hub Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make Pi the messaging hub for Claude Code ↔ Lu communication, with Mac providing vault access only.

**Architecture:** Pi exposes HTTP endpoints for channel messaging. MCP server on Mac calls Pi directly. Mac Flask server keeps only vault tools.

**Tech Stack:** Rust (axum for HTTP), Python (requests for HTTP client), TOML config

---

## Task 1: Add Channel Module to Pi (Rust)

**Files:**
- Create: `src/channel.rs`
- Modify: `src/main.rs:1-13` (add mod declaration)
- Test: `src/channel.rs` (inline tests)

**Step 1: Write the failing test**

Add to bottom of new `src/channel.rs`:

```rust
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
```

**Step 2: Run test to verify it fails**

Run: `cargo test channel_stores`
Expected: FAIL with "failed to resolve: use of undeclared crate or module `channel`"

**Step 3: Write minimal implementation**

Create `src/channel.rs`:

```rust
//! In-memory channel for Claude Code ↔ Lu messaging.

use std::sync::{Arc, Mutex};
use std::collections::VecDeque;
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
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ChannelInner {
                messages: VecDeque::with_capacity(MAX_MESSAGES),
                next_id: 1,
            })),
        }
    }

    /// Send a message to the channel.
    pub fn send(
        &self,
        sender: &str,
        content: &str,
        reply_to: Option<u64>,
        context: Option<serde_json::Value>,
    ) -> ChannelMessage {
        let mut inner = self.inner.lock().unwrap();

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
    pub fn history(&self, limit: usize) -> Vec<ChannelMessage> {
        let inner = self.inner.lock().unwrap();
        inner.messages.iter().rev().take(limit).cloned().collect::<Vec<_>>().into_iter().rev().collect()
    }

    /// Find a message by ID.
    pub fn get(&self, id: u64) -> Option<ChannelMessage> {
        let inner = self.inner.lock().unwrap();
        inner.messages.iter().find(|m| m.id == id).cloned()
    }
}

impl Default for Channel {
    fn default() -> Self {
        Self::new()
    }
}
```

Add to `src/main.rs` after line 5:

```rust
mod channel;
```

**Step 4: Run test to verify it passes**

Run: `cargo test channel_stores && cargo test channel_tracks`
Expected: PASS

**Step 5: Commit**

```bash
git add src/channel.rs src/main.rs
git commit -m "feat(pi): add Channel module for in-memory message storage"
```

---

## Task 2: Add HTTP API to Pi (Rust)

**Files:**
- Create: `src/api.rs`
- Modify: `src/main.rs` (add mod, spawn server)
- Modify: `Cargo.toml` (add axum dependency)
- Test: `src/api.rs` (inline tests)

**Step 1: Add axum dependency**

Add to `Cargo.toml` dependencies:

```toml
axum = "0.7"
tower-http = { version = "0.5", features = ["cors"] }
```

**Step 2: Run cargo check**

Run: `cargo check`
Expected: PASS (dependencies resolve)

**Step 3: Write the API module**

Create `src/api.rs`:

```rust
//! HTTP API for channel messaging.
//!
//! Exposes endpoints for Claude Code to send messages and read history.

use std::sync::Arc;

use axum::{
    Router,
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::channel::{Channel, ChannelMessage};

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub channel: Channel,
    pub auth_token: String,
}

/// Request body for sending a message.
#[derive(Debug, Deserialize)]
pub struct SendRequest {
    pub from: String,
    pub content: String,
    #[serde(default)]
    pub reply_to: Option<u64>,
    #[serde(default)]
    pub context: Option<serde_json::Value>,
}

/// Response for send endpoint.
#[derive(Debug, Serialize)]
pub struct SendResponse {
    pub status: String,
    pub id: u64,
    pub timestamp: String,
}

/// Query params for history endpoint.
#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    20
}

/// Response for history endpoint.
#[derive(Debug, Serialize)]
pub struct HistoryResponse {
    pub messages: Vec<ChannelMessage>,
}

/// Health check response.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

/// Check authorization header.
fn check_auth(headers: &HeaderMap, expected_token: &str) -> Result<(), StatusCode> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let token = auth.strip_prefix("Bearer ").ok_or(StatusCode::UNAUTHORIZED)?;

    if token != expected_token {
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(())
}

/// Health check endpoint.
async fn health() -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// Send a message to the channel.
async fn channel_send(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<SendRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    check_auth(&headers, &state.auth_token)?;

    let msg = state.channel.send(&req.from, &req.content, req.reply_to, req.context);

    Ok(Json(SendResponse {
        status: "sent".to_string(),
        id: msg.id,
        timestamp: msg.timestamp.to_rfc3339(),
    }))
}

/// Get channel message history.
async fn channel_history(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<HistoryQuery>,
) -> Result<impl IntoResponse, StatusCode> {
    check_auth(&headers, &state.auth_token)?;

    let messages = state.channel.history(query.limit);

    Ok(Json(HistoryResponse { messages }))
}

/// Create the API router.
pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/channel/send", post(channel_send))
        .route("/channel/history", get(channel_history))
        .with_state(state)
}

/// Run the API server.
pub async fn run_server(state: Arc<AppState>, port: u16) -> anyhow::Result<()> {
    let router = create_router(state);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));

    tracing::info!("Channel API listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}
```

Add to `src/main.rs` after channel mod:

```rust
mod api;
```

**Step 4: Run cargo check**

Run: `cargo check`
Expected: PASS

**Step 5: Commit**

```bash
git add src/api.rs src/main.rs Cargo.toml
git commit -m "feat(pi): add HTTP API for channel messaging"
```

---

## Task 3: Integrate API Server with Bot Startup

**Files:**
- Modify: `src/bot.rs` (add channel and API server spawn)
- Modify: `src/config.rs` (add channel_port and channel_auth_token)

**Step 1: Update config.rs**

Add new fields to Config struct and load from environment/file:

```rust
// Add to Config struct:
pub channel_port: u16,
pub channel_auth_token: String,

// Add defaults:
channel_port: std::env::var("LU_CHANNEL_PORT")
    .ok()
    .and_then(|s| s.parse().ok())
    .unwrap_or(8202),
channel_auth_token: std::env::var("LU_CHANNEL_AUTH_TOKEN")
    .unwrap_or_else(|_| "".to_string()),
```

**Step 2: Update bot.rs run() function**

Before the Telegram bot starts, spawn the API server:

```rust
use crate::api::{AppState, run_server};
use crate::channel::Channel;

// In run() function, before bot.dispatch():
let channel = Channel::new();
let api_state = Arc::new(AppState {
    channel: channel.clone(),
    auth_token: config.channel_auth_token.clone(),
});

// Spawn API server in background
let api_port = config.channel_port;
tokio::spawn(async move {
    if let Err(e) = run_server(api_state, api_port).await {
        tracing::error!("API server error: {}", e);
    }
});
```

**Step 3: Run cargo check**

Run: `cargo check`
Expected: PASS

**Step 4: Test manually**

Run: `LU_CHANNEL_AUTH_TOKEN=test cargo run`
In another terminal: `curl -H "Authorization: Bearer test" http://localhost:8202/health`
Expected: `{"status":"ok","version":"0.9.0"}`

**Step 5: Commit**

```bash
git add src/bot.rs src/config.rs
git commit -m "feat(pi): integrate channel API server with bot startup"
```

---

## Task 4: Connect Channel to LLM Processing

**Files:**
- Modify: `src/bot.rs` (pass channel to event handler)
- Modify: `src/event_handler.rs` (use channel for responses)

**Step 1: Update event_handler to accept Channel**

Modify `handle_event` and `handle_channel_message` signatures to accept `&Channel`:

```rust
pub async fn handle_event(event: Event, llm: &Llm, mcp: &McpClient, channel: &Channel) -> Result<()>

async fn handle_channel_message(data: serde_json::Value, llm: &Llm, mcp: &McpClient, channel: &Channel) -> Result<()>
```

**Step 2: Store responses in channel**

In `handle_channel_message`, after getting LLM response, store it:

```rust
// After: let response = match llm.chat(...) { ... };
channel.send(BOT_SENDER_ID, &response, Some(msg.id), None);
```

**Step 3: Update bot.rs to pass channel**

Pass the channel to event handler calls.

**Step 4: Run tests**

Run: `cargo test`
Expected: PASS

**Step 5: Commit**

```bash
git add src/event_handler.rs src/bot.rs
git commit -m "feat(pi): connect channel to LLM response storage"
```

---

## Task 5: Update Mac MCP Server to Call Pi

**Files:**
- Modify: `src/mcp/mcp_server.py`
- Test: Manual test with Pi running

**Step 1: Update mcp_server.py to call Pi**

Replace the local channel calls with HTTP requests to Pi:

```python
import os
import requests

PI_HOST = os.environ.get("PI_HOST", "localhost")
PI_CHANNEL_PORT = os.environ.get("PI_CHANNEL_PORT", "8202")
CHANNEL_AUTH_TOKEN = os.environ.get("CHANNEL_AUTH_TOKEN", "")

def _get_pi_url(path: str) -> str:
    return f"http://{PI_HOST}:{PI_CHANNEL_PORT}{path}"

def _get_headers() -> dict:
    return {"Authorization": f"Bearer {CHANNEL_AUTH_TOKEN}"}

@mcp.tool()
def channel_send(
    content: str,
    reply_to: int | None = None,
    wait_for_response: bool = True,
    timeout: int = 60,
) -> str:
    """Send a message to Lu and optionally wait for response."""
    content = content.strip()
    if not content:
        return "Error: Message content is required"

    try:
        # Send to Pi
        resp = requests.post(
            _get_pi_url("/channel/send"),
            headers=_get_headers(),
            json={"from": "claude_code", "content": content, "reply_to": reply_to},
            timeout=10,
        )
        resp.raise_for_status()
        data = resp.json()
        sent_id = data["id"]

        if not wait_for_response:
            return f"Message sent (ID: {sent_id}). Lu will respond shortly."

        # Poll for Lu's response
        import time
        start_time = time.time()
        while time.time() - start_time < timeout:
            time.sleep(2)
            hist_resp = requests.get(
                _get_pi_url("/channel/history"),
                headers=_get_headers(),
                params={"limit": 10},
                timeout=10,
            )
            hist_resp.raise_for_status()
            messages = hist_resp.json()["messages"]
            for m in reversed(messages):
                if m["sender"] == "lu" and m.get("reply_to") == sent_id:
                    return f"Lu's response:\n\n{m['content']}"

        return f"Message sent (ID: {sent_id}) but no response within {timeout}s."

    except requests.exceptions.ConnectionError:
        return f"Error: Pi unreachable at {PI_HOST}:{PI_CHANNEL_PORT}. Is Ludolph running?"
    except Exception as e:
        return f"Error: {e}"

@mcp.tool()
def channel_history(limit: int = 20) -> str:
    """Get recent channel conversation history."""
    try:
        resp = requests.get(
            _get_pi_url("/channel/history"),
            headers=_get_headers(),
            params={"limit": limit},
            timeout=10,
        )
        resp.raise_for_status()
        messages = resp.json()["messages"]

        if not messages:
            return "No channel messages yet."

        lines = ["Channel History:\n"]
        for msg in messages:
            ts = msg["timestamp"][11:19]
            direction = "<- lu" if msg["sender"] == "lu" else "-> lu"
            line = f"[{ts}] {msg['sender']} {direction}: {msg['content'][:100]}"
            if len(msg["content"]) > 100:
                line += "..."
            lines.append(line)

        return "\n".join(lines)

    except requests.exceptions.ConnectionError:
        return f"Error: Pi unreachable at {PI_HOST}:{PI_CHANNEL_PORT}"
    except Exception as e:
        return f"Error: {e}"
```

**Step 2: Remove local channel imports**

Remove the imports for local channel.py and event_bus.py.

**Step 3: Test manually**

With Pi running: test MCP tools via Claude Code.

**Step 4: Commit**

```bash
git add src/mcp/mcp_server.py
git commit -m "feat(mac): update MCP server to call Pi for channel messaging"
```

---

## Task 6: Remove Channel Endpoints from Mac Flask Server

**Files:**
- Modify: `src/mcp/server.py` (remove /channel/* routes)
- Delete: `src/mcp/channel.py` (no longer needed)
- Delete: `src/mcp/event_bus.py` (no longer needed)

**Step 1: Remove channel routes from server.py**

Delete the following routes:
- `/channel/send`
- `/channel/history`
- `/channel/throttle`

Delete related helper functions and state variables.

**Step 2: Delete channel.py and event_bus.py**

```bash
rm src/mcp/channel.py src/mcp/event_bus.py
```

**Step 3: Update imports in server.py**

Remove imports for channel and event_bus modules.

**Step 4: Run tests**

Run: `cd src/mcp && python -m pytest tests/ -v`
Expected: PASS (channel tests removed or skipped)

**Step 5: Commit**

```bash
git add -A src/mcp/
git commit -m "refactor(mac): remove channel from Flask server, keep vault tools only"
```

---

## Task 7: Update Configuration and Installer

**Files:**
- Modify: `docs/install`
- Modify: `.mcp.json` (update env vars)

**Step 1: Update installer for Pi**

Add channel config section:

```bash
# Generate channel auth token
CHANNEL_AUTH_TOKEN=$(openssl rand -hex 32)
echo "CHANNEL_AUTH_TOKEN=$CHANNEL_AUTH_TOKEN" >> ~/.ludolph/env

# Display for Mac setup
echo "Channel auth token (save for Mac setup): $CHANNEL_AUTH_TOKEN"
```

**Step 2: Update installer for Mac**

Prompt for Pi host and channel auth token:

```bash
read -p "Pi hostname or IP: " PI_HOST
read -p "Channel auth token (from Pi setup): " CHANNEL_AUTH_TOKEN

cat >> ~/.ludolph/config.toml << EOF
pi_host = "$PI_HOST"
channel_port = 8202
channel_auth_token = "$CHANNEL_AUTH_TOKEN"
EOF
```

**Step 3: Update .mcp.json template**

```json
{
  "mcpServers": {
    "ludolph-channel": {
      "type": "stdio",
      "command": "python3",
      "args": ["~/.ludolph/mcp/mcp_server.py"],
      "env": {
        "VAULT_PATH": "~/vault",
        "PI_HOST": "pi.local",
        "PI_CHANNEL_PORT": "8202",
        "CHANNEL_AUTH_TOKEN": "your-token-here"
      }
    }
  }
}
```

**Step 4: Commit**

```bash
git add docs/install .mcp.json
git commit -m "feat: update installer and config for Pi hub architecture"
```

---

## Task 8: Integration Testing

**Files:**
- Test: Manual verification checklist

**Step 1: Deploy to Pi**

```bash
ssh pi "cd ~/ludolph && git pull && cargo build --release"
ssh pi "sudo systemctl restart ludolph"
```

**Step 2: Verify Pi API**

```bash
curl -H "Authorization: Bearer $TOKEN" http://pi.local:8202/health
# Expected: {"status":"ok","version":"0.9.0"}
```

**Step 3: Test from Claude Code**

Restart Claude Code to reload MCP config.
Call `channel_send` tool with a test message.
Verify Lu responds.

**Step 4: Verify Telegram still works**

Send a message via Telegram.
Verify Lu responds.

**Step 5: Document results**

Update manual verification checklist in design doc.

**Step 6: Commit any fixes**

```bash
git add -A
git commit -m "fix: integration test fixes for Pi hub"
```

---

## Summary

| Task | Description | Est. Time |
|------|-------------|-----------|
| 1 | Add Channel module to Pi | 15 min |
| 2 | Add HTTP API to Pi | 20 min |
| 3 | Integrate API with bot startup | 10 min |
| 4 | Connect channel to LLM processing | 10 min |
| 5 | Update Mac MCP server to call Pi | 15 min |
| 6 | Remove channel from Mac Flask server | 10 min |
| 7 | Update configuration and installer | 15 min |
| 8 | Integration testing | 20 min |
