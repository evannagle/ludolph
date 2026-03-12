# Lu/MCP Architecture Fix Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix Pi↔Mac communication reliability by adding SSE endpoint, smart WoL, improved tool validation, and removing dual listeners.

**Architecture:** Pi is a thin client that connects to Mac's MCP server via HTTP/SSE. Mac pushes events to Pi, Pi processes them through Mac's LLM proxy, and responds back through the channel API.

**Tech Stack:** Python/Flask (Mac), Rust/tokio (Pi), Server-Sent Events, Wake-on-LAN

---

## Chunk 1: SSE Events Endpoint (Mac)

### Task 1: Add /events SSE Endpoint to server.py

**Files:**
- Modify: `src/mcp/server.py:175-180` (add import and endpoint)
- Test: Manual testing with curl

- [ ] **Step 1: Add threading import for SSE generator**

At the top of `src/mcp/server.py`, add the threading import alongside existing imports:

```python
import threading
import time
```

- [ ] **Step 2: Add channel storage for SSE events**

After the `_registry` global variable (around line 53), add:

```python
# Channel for SSE events - thread-safe queue
_event_subscribers: dict[str, list[dict]] = {}
_event_lock = threading.Lock()
_next_event_id = 1


def push_event(event_type: str, data: dict) -> int:
    """Push an event to all subscribers."""
    global _next_event_id
    with _event_lock:
        event_id = _next_event_id
        _next_event_id += 1
        event = {
            "id": event_id,
            "type": event_type,
            "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "data": data,
        }
        for subscriber_events in _event_subscribers.values():
            subscriber_events.append(event)
        return event_id
```

- [ ] **Step 3: Add /events SSE endpoint**

After the `/health` endpoint (around line 188), add:

```python
@app.route("/events")
@require_auth
def events():
    """Server-Sent Events endpoint for real-time notifications."""
    subscriber = request.args.get("subscriber", "unknown")

    # Initialize subscriber's event queue
    with _event_lock:
        if subscriber not in _event_subscribers:
            _event_subscribers[subscriber] = []

    def generate():
        last_heartbeat = time.time()
        while True:
            # Check for new events
            with _event_lock:
                events_to_send = _event_subscribers.get(subscriber, [])
                _event_subscribers[subscriber] = []

            for event in events_to_send:
                yield f"data: {json.dumps(event)}\n\n"

            # Send heartbeat every 30 seconds
            if time.time() - last_heartbeat >= 30:
                heartbeat = {
                    "id": 0,
                    "type": "heartbeat",
                    "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                    "data": {},
                }
                yield f"data: {json.dumps(heartbeat)}\n\n"
                last_heartbeat = time.time()

            time.sleep(0.5)

    return Response(generate(), mimetype="text/event-stream")
```

- [ ] **Step 4: Push event when channel message received**

Modify the channel send handling. First, add a channel message endpoint if not exists. After the `/chat/stream` endpoint (around line 364), check if `/channel/send` exists. If not, add it. If the endpoint exists on Pi's side, we need to add the SSE push. For now, add a helper to push channel events:

```python
# Add after push_event function
def push_channel_message(sender: str, content: str, message_id: int, reply_to: int | None = None):
    """Push a channel message event to SSE subscribers."""
    push_event("channel_message", {
        "sender": sender,
        "content": content,
        "message_id": message_id,
        "reply_to": reply_to,
    })
```

- [ ] **Step 5: Run the server and test with curl**

Run: `curl -N -H "Authorization: Bearer $AUTH_TOKEN" "http://localhost:8200/events?subscriber=test"`
Expected: Connection stays open, receives heartbeat events every 30s

- [ ] **Step 6: Commit**

```bash
git add src/mcp/server.py
git commit -m "$(cat <<'EOF'
feat: add /events SSE endpoint to MCP server

Implements Server-Sent Events for real-time push notifications
from Mac to Pi. Supports channel_message events for Claude Code
communication and heartbeat events for connection health.
EOF
)"
```

---

## Chunk 2: Smart Wake-on-LAN (Pi)

### Task 2: Add Health Check Before WoL

**Files:**
- Modify: `src/mcp_client.rs:425-461` (refactor get_tool_definitions)
- Test: `cargo test mcp_client`

- [ ] **Step 1: Write test for health check function**

Add to `src/mcp_client.rs` in the tests module:

```rust
#[tokio::test]
async fn health_check_returns_false_for_unreachable_server() {
    let config = McpConfig {
        url: "http://127.0.0.1:1".to_string(),
        fallback_url: None,
        auth_token: "test-token".to_string(),
        mac_address: None,
    };

    let client = McpClient::from_config(&config);
    let is_healthy = client.quick_health_check().await;

    assert!(!is_healthy);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test health_check_returns_false_for_unreachable_server -v`
Expected: FAIL with "method not found"

- [ ] **Step 3: Implement quick_health_check method**

Add after the `get_status` method (around line 217):

```rust
/// Quick health check with 2 second timeout.
///
/// Returns true if server responds with 200 OK, false otherwise.
/// This is faster than get_status() and doesn't parse response body.
pub async fn quick_health_check(&self) -> bool {
    let response = self
        .client
        .get(format!("{}/health", self.base_url))
        .header("Authorization", format!("Bearer {}", self.auth_token))
        .timeout(Duration::from_secs(2))
        .send()
        .await;

    matches!(response, Ok(r) if r.status().is_success())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test health_check_returns_false_for_unreachable_server -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/mcp_client.rs
git commit -m "feat: add quick_health_check for smart WoL"
```

### Task 3: Implement request_with_recovery Pattern

**Files:**
- Modify: `src/mcp_client.rs:425-461` (refactor WoL logic)

- [ ] **Step 1: Write test for smart WoL behavior**

Add to tests module:

```rust
#[test]
fn client_has_mac_address_returns_correct_value() {
    let config_with = McpConfig {
        url: "http://localhost:8200".to_string(),
        fallback_url: None,
        auth_token: "test".to_string(),
        mac_address: Some("aa:bb:cc:dd:ee:ff".to_string()),
    };

    let config_without = McpConfig {
        url: "http://localhost:8200".to_string(),
        fallback_url: None,
        auth_token: "test".to_string(),
        mac_address: None,
    };

    let client_with = McpClient::from_config(&config_with);
    let client_without = McpClient::from_config(&config_without);

    assert!(client_with.has_mac_address());
    assert!(!client_without.has_mac_address());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test client_has_mac_address -v`
Expected: FAIL with "method not found"

- [ ] **Step 3: Add has_mac_address helper**

Add after `from_config`:

```rust
/// Check if a MAC address is configured for Wake-on-LAN.
#[must_use]
pub fn has_mac_address(&self) -> bool {
    self.mac_address.is_some()
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test client_has_mac_address -v`
Expected: PASS

- [ ] **Step 5: Refactor get_tool_definitions to use smart WoL**

Replace the current `get_tool_definitions` method (lines ~431-461) with:

```rust
/// Get tool definitions from the MCP server.
///
/// Uses smart recovery: if request fails, checks if server is actually
/// unreachable before triggering Wake-on-LAN.
#[allow(clippy::cognitive_complexity)]
pub async fn get_tool_definitions(&self) -> Result<Vec<Tool>> {
    // First attempt
    match self.try_get_tool_definitions().await {
        Ok(tools) => return Ok(tools),
        Err(e) => {
            tracing::warn!("Failed to get tools: {}", e);

            // Quick health check before assuming server is down
            if self.quick_health_check().await {
                tracing::info!("Health check passed, retrying immediately");
                return self.try_get_tool_definitions().await;
            }

            // Server truly unreachable, try WoL if configured
            if self.mac_address.is_none() {
                return Err(e);
            }

            tracing::info!("Server unreachable, attempting Wake-on-LAN...");
            if let Err(wol_err) = self.wake_mac() {
                tracing::warn!("Wake-on-LAN failed: {}", wol_err);
                return Err(e);
            }

            // Wait for Mac to wake up
            tracing::info!("Waiting 15s for Mac to wake up...");
            tokio::time::sleep(Duration::from_secs(15)).await;

            // Retry
            self.try_get_tool_definitions().await.context(
                "Failed to get tools after Wake-on-LAN.\n\n\
                 The Mac may still be waking up. Try again in a moment.",
            )
        }
    }
}
```

- [ ] **Step 6: Apply same pattern to call_tool**

Replace the current `call_tool` method (lines ~522-557) with:

```rust
/// Call a tool on the MCP server.
///
/// Uses smart recovery: if request fails, checks if server is actually
/// unreachable before triggering Wake-on-LAN.
#[allow(clippy::cognitive_complexity)]
pub async fn call_tool(&self, name: &str, input: &Value) -> Result<String> {
    // First attempt
    match self.try_call_tool(name, input).await {
        Ok(result) => return Ok(result),
        Err(e) => {
            tracing::warn!("MCP call failed: {}", e);

            // Quick health check before assuming server is down
            if self.quick_health_check().await {
                tracing::info!("Health check passed, retrying immediately");
                return self.try_call_tool(name, input).await;
            }

            // Server truly unreachable, try WoL if configured
            if self.mac_address.is_none() {
                return Err(e);
            }

            tracing::info!("Server unreachable, attempting Wake-on-LAN...");
            if let Err(wol_err) = self.wake_mac() {
                tracing::warn!("Wake-on-LAN failed: {}", wol_err);
                return Err(e);
            }

            // Wait for Mac to wake up
            tracing::info!("Waiting 10s for Mac to wake up...");
            tokio::time::sleep(Duration::from_secs(10)).await;

            // Retry with retries
            self.try_call_tool_with_retry(name, input, 3).await.context(
                "Failed to connect after Wake-on-LAN attempt.\n\n\
                 Try:\n\
                 • Wait longer for Mac to wake up\n\
                 • Check Mac power/network status manually\n\
                 • Verify Wake-on-LAN is enabled in Mac settings",
            )
        }
    }
}
```

- [ ] **Step 7: Run all tests**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 8: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No errors

- [ ] **Step 9: Commit**

```bash
git add src/mcp_client.rs
git commit -m "feat: implement smart WoL with health check first"
```

---

## Chunk 3: Tool Creation Validation (Mac)

### Task 4: Enhance Tool Validation Error Messages

**Files:**
- Modify: `src/mcp/tools/meta.py:105-135` (improve validation)

Note: Basic validation was already added in earlier work. This task adds more helpful examples.

- [ ] **Step 1: Read current validation**

Verify the current state of `_validate_tool_code` already includes the basic checks from earlier.

- [ ] **Step 2: Enhance error messages with examples**

Replace the `_validate_tool_code` function with more detailed error messages:

```python
def _validate_tool_code(code: str) -> str | None:
    """Validate tool code for security and correctness. Returns error message or None if valid."""
    if not code:
        return "Code is required"

    # Check for required exports
    if "TOOLS" not in code:
        return "Code must define TOOLS list"
    if "HANDLERS" not in code:
        return "Code must define HANDLERS dict"

    # Check for common schema mistakes (must use snake_case 'input_schema')
    if '"parameters"' in code or "'parameters'" in code:
        return (
            "Error: Use 'input_schema' not 'parameters'\n\n"
            "Example:\n"
            "TOOLS = [{\n"
            "    'name': 'my_tool',\n"
            "    'description': 'Does something',\n"
            "    'input_schema': {  # NOT 'parameters'\n"
            "        'type': 'object',\n"
            "        'properties': {...}\n"
            "    }\n"
            "}]"
        )
    if '"inputSchema"' in code or "'inputSchema'" in code:
        return (
            "Error: Use 'input_schema' (snake_case) not 'inputSchema' (camelCase)\n\n"
            "Example:\n"
            "TOOLS = [{\n"
            "    'name': 'my_tool',\n"
            "    'description': 'Does something',\n"
            "    'input_schema': {  # NOT 'inputSchema'\n"
            "        'type': 'object',\n"
            "        'properties': {...}\n"
            "    }\n"
            "}]"
        )
    if "input_schema" not in code:
        return (
            "Error: TOOLS must include 'input_schema' for each tool\n\n"
            "Example:\n"
            "TOOLS = [{\n"
            "    'name': 'my_tool',\n"
            "    'description': 'Does something',\n"
            "    'input_schema': {\n"
            "        'type': 'object',\n"
            "        'properties': {\n"
            "            'arg1': {'type': 'string', 'description': 'First argument'}\n"
            "        },\n"
            "        'required': ['arg1']\n"
            "    }\n"
            "}]"
        )

    # Check for forbidden patterns
    for pattern in FORBIDDEN_PATTERNS:
        if re.search(pattern, code):
            return f"Forbidden pattern detected: {pattern}"

    # Try to compile (syntax check)
    try:
        compile(code, "<custom_tool>", "exec")
    except SyntaxError as e:
        return f"Syntax error: {e}"

    return None
```

- [ ] **Step 3: Run MCP server to verify syntax**

Run: `python src/mcp/server.py`
Expected: Server starts without syntax errors

- [ ] **Step 4: Commit**

```bash
git add src/mcp/tools/meta.py
git commit -m "feat: enhance tool validation with example code"
```

---

## Chunk 4: Remove Dual Listener (Pi)

### Task 5: Remove spawn_channel_listener from bot.rs

**Files:**
- Modify: `src/bot.rs:204-244` (remove channel listener setup)
- Modify: `src/bot.rs:963-1003` (remove function)
- Modify: `src/api.rs:22-29` (remove message_tx from AppState)

- [ ] **Step 1: Remove message_tx from AppState in api.rs**

Change `AppState` struct (lines 22-29) to remove the message_tx field:

```rust
/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub channel: Channel,
    pub auth_token: String,
}
```

- [ ] **Step 2: Remove notification logic from channel_send handler**

Update `channel_send` function (lines 101-125) to remove the notification logic:

```rust
/// Send a message to the channel.
async fn channel_send(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<SendRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    check_auth(&headers, &state.auth_token)?;

    let msg = state
        .channel
        .send(&req.from, &req.content, req.reply_to, req.context);

    Ok(Json(SendResponse {
        status: "sent".to_string(),
        id: msg.id,
        timestamp: msg.timestamp.to_rfc3339(),
    }))
}
```

- [ ] **Step 3: Remove unused import in api.rs**

Remove `mpsc` from the tokio imports (line 17):

```rust
// Remove this line if mpsc is no longer used elsewhere
// use tokio::sync::mpsc;
```

Check if mpsc is used elsewhere in api.rs. If not, remove the import.

- [ ] **Step 4: Remove channel listener setup from bot.rs run()**

Remove lines 207-214 (the message channel creation and AppState construction with message_tx):

Replace:
```rust
// Create notification channel for incoming messages (capacity 100)
let (message_tx, message_rx) = tokio::sync::mpsc::channel(100);

let api_state = Arc::new(AppState {
    channel: channel.clone(),
    auth_token: config.channel.auth_token.clone(),
    message_tx: Some(message_tx),
});
```

With:
```rust
let api_state = Arc::new(AppState {
    channel: channel.clone(),
    auth_token: config.channel.auth_token.clone(),
});
```

- [ ] **Step 5: Remove spawn_channel_listener call**

Remove line 243:
```rust
// Remove this line
spawn_channel_listener(message_rx, llm.clone(), channel);
```

- [ ] **Step 6: Remove spawn_channel_listener function**

Delete the entire function (lines 963-1003):
```rust
// DELETE THIS ENTIRE FUNCTION
fn spawn_channel_listener(
    mut rx: tokio::sync::mpsc::Receiver<crate::channel::ChannelMessage>,
    llm: Llm,
    channel: Channel,
) {
    // ...
}
```

- [ ] **Step 7: Remove unused import in bot.rs**

The import for `crate::channel::ChannelMessage` in bot.rs may now be unused if spawn_channel_listener was the only user. Check and remove if needed.

- [ ] **Step 8: Run cargo check**

Run: `cargo check`
Expected: No errors (may have warnings about unused items)

- [ ] **Step 9: Run cargo clippy**

Run: `cargo clippy -- -D warnings`
Expected: No errors

- [ ] **Step 10: Run tests**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 11: Commit**

```bash
git add src/bot.rs src/api.rs
git commit -m "$(cat <<'EOF'
refactor: remove dual listener, SSE is sole message processor

Removes spawn_channel_listener and message_tx channel from Pi.
Messages from Claude Code now flow exclusively through SSE events
from Mac, eliminating duplicate message processing.
EOF
)"
```

---

## Chunk 5: Integration and Testing

### Task 6: Sync Changes to Deployed MCP

**Files:**
- Deploy: `~/.ludolph/mcp/` (installed location)

- [ ] **Step 1: Copy updated files to installed location**

```bash
cp src/mcp/server.py ~/.ludolph/mcp/
cp src/mcp/tools/meta.py ~/.ludolph/mcp/tools/
```

- [ ] **Step 2: Restart MCP server**

```bash
launchctl kickstart -k gui/$(id -u)/dev.ludolph.mcp
```

- [ ] **Step 3: Verify /events endpoint works**

```bash
curl -N -H "Authorization: Bearer $AUTH_TOKEN" "http://localhost:8200/events?subscriber=test" &
sleep 35  # Wait for heartbeat
kill %1
```

Expected: Receives heartbeat event after ~30 seconds

### Task 7: Build and Deploy Pi Binary

**Files:**
- Build: `target/release/lu`

- [ ] **Step 1: Build release binary locally**

```bash
cargo build --release
```

- [ ] **Step 2: Cross-compile for Pi (if on Mac)**

If building on Mac for Pi deployment:
```bash
cross build --release --target aarch64-unknown-linux-gnu
```

Or build directly on Pi:
```bash
ssh pi@<pi-ip> "cd ~/ludolph && git pull && cargo build --release"
```

- [ ] **Step 3: Deploy to Pi**

```bash
scp target/release/lu pi@<pi-ip>:~/.ludolph/bin/lu
# Or if building on Pi, just:
ssh pi@<pi-ip> "cp ~/ludolph/target/release/lu ~/.ludolph/bin/lu"
```

- [ ] **Step 4: Restart Lu on Pi**

```bash
ssh pi@<pi-ip> "sudo systemctl restart ludolph"
```

### Task 8: End-to-End Verification

- [ ] **Step 1: Check Pi connects to SSE endpoint**

```bash
ssh pi@<pi-ip> "journalctl -u ludolph -f" &
```

Look for: `SSE connected, status: 200`

- [ ] **Step 2: Test tool fetching doesn't trigger unnecessary WoL**

With Mac awake, send a message to Lu via Telegram. Check Pi logs:
- Should NOT see "Attempting Wake-on-LAN"
- Should see successful tool fetch

- [ ] **Step 3: Test malformed tool rejection**

Create a custom tool with wrong schema:
```bash
cat > ~/.ludolph/custom_tools/bad_tool.py << 'EOF'
TOOLS = [{"name": "bad", "description": "test", "parameters": {}}]
HANDLERS = {"bad": lambda args: {"content": "hi", "error": None}}
EOF
```

Send SIGHUP to MCP server:
```bash
kill -HUP $(pgrep -f "python.*server.py")
```

Check that Lu can still use other tools (bad_tool is skipped with warning).

Then clean up:
```bash
rm ~/.ludolph/custom_tools/bad_tool.py
kill -HUP $(pgrep -f "python.*server.py")
```

- [ ] **Step 4: Commit integration verification**

```bash
git add .
git commit -m "chore: verify Lu/MCP architecture fix deployment"
```

---

## Success Criteria Checklist

- [ ] Pi can connect to Mac's `/events` endpoint without 404
- [ ] WoL only triggers when Mac is actually unreachable (health check fails)
- [ ] Tool creation validation rejects `parameters` and `inputSchema` with helpful errors
- [ ] Single message processing path (SSE only, no dual listeners)
- [ ] Lu can reliably access tools and respond to Claude Code messages
