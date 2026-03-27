# MCP Resilience Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Pi-to-Mac MCP connection self-healing so Lu stays responsive when network conditions change, ports shift, or tokens drift.

**Architecture:** Five focused changes: (1) SSE client tries fallback URL when primary fails, reporting status to Telegram, (2) Pi bot validates Mac connectivity on startup, (3) port 8202 becomes a single constant instead of 6 hardcoded copies, (4) consolidate to one token file (`channel_token`), (5) fix the `pi_mcp_connectivity` doctor check.

**Tech Stack:** Rust 2024, tokio, eventsource_client, reqwest, teloxide

---

## File Map

| File | Role | Tasks |
|------|------|-------|
| `src/config.rs` | Port constant lives here, token file loading | 3, 4 |
| `src/sse_client.rs` | SSE connection with reconnection loop | 1 |
| `src/bot.rs` | Spawns SSE listener, bot startup | 1, 2 |
| `src/cli/checks/services.rs` | Health checks, port constant consumer | 3, 4 |
| `src/cli/checks/network.rs` | Pi MCP connectivity check | 5 |
| `src/cli/checks/mod.rs` | Check registration | 5 |
| `src/cli/commands.rs` | MCP restart, port reference | 3 |
| `src/cli/setup/mcp.rs` | Setup creates token file, port reference | 3, 4 |
| `src/cli/setup/deploy.rs` | Deploys token to Pi, port reference | 3, 4 |
| `src/cli/setup/verify.rs` | Post-setup verification, port reference | 3, 4 |

---

## Task 1: SSE Client Fallback with Telegram Notifications

The SSE client currently connects to `mcp_config.url` only. When Tailscale is down, it retries forever against the unreachable primary URL even though the LAN fallback works. The fix: pass both URLs to the SSE client and switch to fallback when primary fails.

Additionally, the Pi bot should send a one-time Telegram notification when the SSE connection degrades to fallback or fails entirely, so the user knows Lu is having trouble receiving channel messages.

**Files:**
- Modify: `src/sse_client.rs` (add fallback_url to config and reconnection logic)
- Modify: `src/bot.rs:1209-1249` (pass fallback_url to SSE config, add Telegram notification on connection state changes)

### Step-by-step

- [ ] **Step 1: Add fallback_url to SseConfig**

In `src/sse_client.rs`, add `fallback_url` field to `SseConfig`:

```rust
/// SSE client configuration.
#[derive(Debug, Clone)]
pub struct SseConfig {
    pub url: String,
    pub fallback_url: Option<String>,
    pub auth_token: String,
    pub subscriber_id: String,
}
```

- [ ] **Step 2: Add connection state tracking**

In `src/sse_client.rs`, add a connection state enum and a callback type above the `connect` function:

```rust
/// Current SSE connection state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    /// Connected to primary URL.
    Primary,
    /// Connected to fallback URL (primary unreachable).
    Fallback,
    /// Both URLs unreachable.
    Disconnected,
}

/// Callback invoked when connection state changes.
pub type StateCallback = Box<dyn Fn(ConnectionState) + Send + Sync>;
```

- [ ] **Step 3: Update `connect()` to try fallback URL**

Modify the `connect()` function signature to accept a state callback, and update the reconnection loop to try fallback when primary fails:

```rust
pub async fn connect(
    config: SseConfig,
    tx: mpsc::Sender<Event>,
    on_state_change: Option<StateCallback>,
) -> Result<()> {
    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(30);
    let mut current_state = ConnectionState::Disconnected;

    loop {
        // Try primary URL first
        let primary_url = build_url(&config.url, &config.subscriber_id);
        info!("SSE connecting to primary: {}", config.url);

        match connect_once(&primary_url, &config.auth_token, &tx).await {
            Ok(()) => {
                // Clean disconnect from primary - reset backoff
                backoff = Duration::from_secs(1);
                update_state(&mut current_state, ConnectionState::Primary, &on_state_change);
                continue;
            }
            Err(e) => {
                warn!("SSE primary connection failed: {}", e);
            }
        }

        // Try fallback URL if available
        if let Some(ref fallback) = config.fallback_url {
            let fallback_url = build_url(fallback, &config.subscriber_id);
            info!("SSE trying fallback: {}", fallback);

            match connect_once(&fallback_url, &config.auth_token, &tx).await {
                Ok(()) => {
                    // Clean disconnect from fallback - reset backoff
                    backoff = Duration::from_secs(1);
                    update_state(&mut current_state, ConnectionState::Fallback, &on_state_change);
                    continue;
                }
                Err(e) => {
                    warn!("SSE fallback connection failed: {}", e);
                }
            }
        }

        // Both failed
        update_state(&mut current_state, ConnectionState::Disconnected, &on_state_change);

        warn!("SSE reconnecting in {:?}", backoff);
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(max_backoff);
    }
}

fn build_url(base: &str, subscriber_id: &str) -> String {
    format!("{base}/events?subscriber={subscriber_id}")
}

fn update_state(
    current: &mut ConnectionState,
    new: ConnectionState,
    callback: &Option<StateCallback>,
) {
    if *current != new {
        *current = new.clone();
        if let Some(cb) = callback {
            cb(new);
        }
    }
}
```

Note: The existing `connect_once()` function stays unchanged -- it takes a full URL string and returns `Result<()>`. We just call it with different URLs now.

- [ ] **Step 4: Update `spawn_sse_listener` in bot.rs to pass fallback and notify Telegram**

In `src/bot.rs`, update `spawn_sse_listener` to:
1. Accept a `Bot` and `chat_id` for Telegram notifications
2. Pass `fallback_url` from `mcp_config` to `SseConfig`
3. Create a state callback that sends Telegram messages on state changes

```rust
fn spawn_sse_listener(
    mcp_config: &McpConfig,
    llm: Llm,
    channel: Channel,
    bot: Bot,
    notify_user: Option<teloxide::types::ChatId>,
) {
    let sse_config = crate::sse_client::SseConfig {
        url: mcp_config.url.clone(),
        fallback_url: mcp_config.fallback_url.clone(),
        auth_token: mcp_config.auth_token.clone(),
        subscriber_id: "pi_bot".to_string(),
    };

    let mcp_client = McpClient::from_config(mcp_config);

    tokio::spawn(async move {
        let (tx, mut rx) = tokio::sync::mpsc::channel(100);

        // State change callback for Telegram notifications
        let bot_clone = bot.clone();
        let state_callback: Option<crate::sse_client::StateCallback> = notify_user.map(|chat_id| {
            Box::new(move |state: crate::sse_client::ConnectionState| {
                let bot = bot_clone.clone();
                let msg = match state {
                    crate::sse_client::ConnectionState::Primary => {
                        "MCP connection restored (primary)".to_string()
                    }
                    crate::sse_client::ConnectionState::Fallback => {
                        "MCP using fallback connection. Primary URL unreachable.".to_string()
                    }
                    crate::sse_client::ConnectionState::Disconnected => {
                        "MCP disconnected. Both primary and fallback URLs unreachable. \
                         Channel messages won't be processed until connection is restored."
                            .to_string()
                    }
                };
                tokio::spawn(async move {
                    let _ = bot.send_message(chat_id, msg).await;
                });
            }) as Box<dyn Fn(crate::sse_client::ConnectionState) + Send + Sync>
        });

        // Spawn SSE connection
        let sse_config_clone = sse_config.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::sse_client::connect(sse_config_clone, tx, state_callback).await {
                tracing::error!("SSE client fatal error: {}", e);
            }
        });

        tracing::info!("SSE event listener started for {}", sse_config.url);

        // Process events as they arrive (unchanged)
        while let Some(event) = rx.recv().await {
            // ... existing event processing code stays the same
        }
    });
}
```

Update the call site in `run_bot()` to pass `bot` and the first allowed user's chat ID:

```rust
if let Some(ref mcp) = mcp_config {
    let notify_user = config
        .telegram
        .allowed_users
        .first()
        .and_then(|uid| i64::try_from(*uid).ok())
        .map(teloxide::types::ChatId);
    spawn_sse_listener(mcp, llm.clone(), channel.clone(), bot.clone(), notify_user);
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --all-features`
Run: `cargo clippy --all-features -- -D warnings`

- [ ] **Step 6: Commit**

```bash
git add src/sse_client.rs src/bot.rs
git commit -m "feat: SSE client tries fallback URL with Telegram notifications"
```

---

## Task 2: Startup Self-Test for Mac MCP Connectivity

The Pi bot prints `[ok] MCP: http://...` at startup without verifying the URL is reachable. If the Mac is off or the URL is wrong, the bot silently fails to receive channel messages. Add a connectivity test at startup that logs a clear warning and sends a Telegram message if the Mac MCP is unreachable.

**Files:**
- Modify: `src/bot.rs` (add connectivity check after printing MCP status line)

### Step-by-step

- [ ] **Step 1: Add MCP connectivity test at startup**

In `src/bot.rs`, after the line that prints `StatusLine::ok(format!("MCP: {}", mcp.url))`, add a health check before spawning the SSE listener:

```rust
if let Some(ref mcp) = mcp_config {
    // Test MCP connectivity before spawning listener
    let mcp_client = McpClient::from_config(mcp);
    let status = mcp_client.get_status().await;

    if status.connected {
        if status.using_fallback {
            StatusLine::ok(format!(
                "MCP: connected via fallback (primary {} unreachable)",
                mcp.url
            ))
            .print();
        } else {
            StatusLine::ok(format!("MCP: {}", mcp.url)).print();
        }
    } else {
        let reason = status.disconnect_reason.map_or_else(
            || "unknown".to_string(),
            |r| format!("{r:?}"),
        );
        StatusLine::error(format!("MCP: unreachable ({reason})")).print();
        ui::status::hint("Channel messages won't be processed until Mac MCP is reachable");
        ui::status::hint(&format!("Primary: {}", mcp.url));
        if let Some(ref fallback) = mcp.fallback_url {
            ui::status::hint(&format!("Fallback: {fallback}"));
        }

        // Notify via Telegram
        let notify_user = config
            .telegram
            .allowed_users
            .first()
            .and_then(|uid| i64::try_from(*uid).ok())
        .map(teloxide::types::ChatId);
        if let Some(chat_id) = notify_user {
            let _ = bot
                .send_message(
                    chat_id,
                    format!(
                        "Lu started but can't reach Mac MCP server.\n\
                         Primary: {}\n\
                         Channel messages won't work until Mac is reachable.",
                        mcp.url
                    ),
                )
                .await;
        }
    }

    // Spawn SSE listener (still spawn even if unreachable -- it has reconnection logic)
    let notify_user = config
        .telegram
        .allowed_users
        .first()
        .and_then(|uid| i64::try_from(*uid).ok())
        .map(teloxide::types::ChatId);
    spawn_sse_listener(mcp, llm.clone(), channel.clone(), bot.clone(), notify_user);
}
```

Note: This replaces the existing `StatusLine::ok(format!("MCP: {}", mcp.url)).print()` and the existing `spawn_sse_listener` call. The SSE listener is still spawned even when unreachable because it has built-in reconnection.

**Dependency:** This task must be implemented AFTER Task 1 because it uses the updated `spawn_sse_listener` signature (with `bot` and `notify_user` parameters).

- [ ] **Step 2: Run tests**

Run: `cargo test --all-features`
Run: `cargo clippy --all-features -- -D warnings`

- [ ] **Step 3: Commit**

```bash
git add src/bot.rs
git commit -m "feat: validate Mac MCP connectivity at Pi startup"
```

---

## Task 3: Single Port Constant

Port 8202 is hardcoded in 6 files. A single `pub const` in `config.rs` eliminates drift.

**Files:**
- Modify: `src/config.rs:107-109` (make `default_channel_port` a `pub const`)
- Modify: `src/cli/checks/services.rs:20` (remove local const, import from config)
- Modify: `src/cli/commands.rs:297` (use const instead of string literal)
- Modify: `src/cli/setup/mcp.rs:27` (remove local const, import from config)
- Modify: `src/cli/setup/deploy.rs:309` (remove local var, import from config)
- Modify: `src/cli/setup/verify.rs:23` (remove local const, import from config)

### Step-by-step

- [ ] **Step 1: Export the port constant from config.rs**

In `src/config.rs`, replace the private `default_channel_port` function with a public constant:

```rust
/// Default port for the channel API server.
pub const DEFAULT_CHANNEL_PORT: u16 = 8202;
```

Update `ChannelConfig` to use it:

```rust
#[serde(default = "default_channel_port")]
pub port: u16,
```

Keep the private function for serde's `default` attribute (it requires a function path):

```rust
const fn default_channel_port() -> u16 {
    DEFAULT_CHANNEL_PORT
}
```

Update `Default` impl and `from_env`:

```rust
impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_CHANNEL_PORT,
            auth_token: String::new(),
        }
    }
}

impl ChannelConfig {
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            port: std::env::var("LU_CHANNEL_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(DEFAULT_CHANNEL_PORT),
            auth_token: std::env::var("LU_CHANNEL_AUTH_TOKEN").unwrap_or_default(),
        }
    }
}
```

- [ ] **Step 2: Update services.rs to use the shared constant**

Remove line 20:
```rust
// DELETE: const CHANNEL_PORT: u16 = 8202;
```

Add import:
```rust
use crate::config::DEFAULT_CHANNEL_PORT;
```

Replace all `CHANNEL_PORT` with `DEFAULT_CHANNEL_PORT` in the file. All references are already behind `#[cfg(target_os = "macos")]`, and the import of `crate::config` is also behind the same cfg gate (line 17), so add the constant to the existing import.

- [ ] **Step 3: Update commands.rs to use the shared constant**

Replace the hardcoded `":8202"` string on line 297:

```rust
// Before:
.args(["-ti", ":8202"])

// After:
.args(["-ti", &format!(":{}", crate::config::DEFAULT_CHANNEL_PORT)])
```

- [ ] **Step 4: Update setup/mcp.rs to use the shared constant**

Remove line 27:
```rust
// DELETE: const MCP_PORT: u16 = 8202;
```

Add import and replace all `MCP_PORT` with `DEFAULT_CHANNEL_PORT`:

```rust
use crate::config::DEFAULT_CHANNEL_PORT;
```

Replace references: `MCP_PORT` -> `DEFAULT_CHANNEL_PORT` throughout the file.

- [ ] **Step 5: Update setup/deploy.rs to use the shared constant**

Remove line 309:
```rust
// DELETE: let mcp_port = 8202;
```

Replace `mcp_port` usage with `crate::config::DEFAULT_CHANNEL_PORT`:

```rust
let mcp_url = format!("http://{mac_ip}:{}", crate::config::DEFAULT_CHANNEL_PORT);
```

- [ ] **Step 6: Update setup/verify.rs to use the shared constant**

Remove line 23:
```rust
// DELETE: const CHANNEL_PORT: u16 = 8202;
```

Add import and replace `CHANNEL_PORT` with `DEFAULT_CHANNEL_PORT`:

```rust
use crate::config::DEFAULT_CHANNEL_PORT;
```

- [ ] **Step 7: Run tests and clippy**

Run: `cargo test --all-features`
Run: `cargo clippy --all-features -- -D warnings`

- [ ] **Step 8: Commit**

```bash
git add src/config.rs src/cli/checks/services.rs src/cli/commands.rs src/cli/setup/mcp.rs src/cli/setup/deploy.rs src/cli/setup/verify.rs
git commit -m "refactor: single port constant in config.rs"
```

---

## Task 4: One Token File

Consolidate to `channel_token` only. Stop reading or writing `mcp_token`. The `mcp_token` file was a legacy name from before the channel system was formalized. Both files store the same value, and having two creates confusion.

The token is already shared between Pi and Mac -- `lu setup deploy` copies the local `channel_token` to the Pi. No cross-machine sharing change needed.

**Files:**
- Modify: `src/config.rs:263` (remove mcp_token from fallback list)
- Modify: `src/cli/checks/services.rs:35` (remove mcp_token from fallback list)
- Modify: `src/cli/setup/verify.rs:47-62` (remove mcp_token fallback)

### Step-by-step

- [ ] **Step 1: Update config.rs token loading**

In `src/config.rs`, simplify the token fallback loop in `Config::load()`:

```rust
// Before:
for filename in &["channel_token", "mcp_token"] {

// After - just read channel_token:
let token_path = dir.join("channel_token");
if let Ok(content) = std::fs::read_to_string(&token_path) {
    let trimmed = content.trim().to_string();
    if !trimmed.is_empty() {
        config.channel.auth_token = trimmed;
    }
}
```

- [ ] **Step 2: Update services.rs token loading**

In `src/cli/checks/services.rs`, simplify `load_auth_token()`:

```rust
#[cfg(target_os = "macos")]
fn load_auth_token() -> Option<String> {
    let path = ludolph_dir().join("channel_token");
    fs::read_to_string(&path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
```

Update the `mcp_config_consistent` check hint messages to only reference `channel_token`:

```rust
// Before:
"Check ~/.ludolph/channel_token or ~/.ludolph/mcp_token",

// After:
"Run `lu setup mcp` to generate ~/.ludolph/channel_token",
```

- [ ] **Step 3: Update verify.rs token loading**

In `src/cli/setup/verify.rs`, simplify `load_auth_token()` to only check `channel_token`:

```rust
fn load_auth_token(ludolph_dir: &Path) -> Option<String> {
    let token_file = ludolph_dir.join("channel_token");
    fs::read_to_string(&token_file)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
```

- [ ] **Step 4: Run tests and clippy**

Run: `cargo test --all-features`
Run: `cargo clippy --all-features -- -D warnings`

- [ ] **Step 5: Commit**

```bash
git add src/config.rs src/cli/checks/services.rs src/cli/setup/verify.rs
git commit -m "refactor: consolidate to single channel_token file"
```

---

## Task 5: Fix `pi_mcp_connectivity` Check

The check skips with "No MCP configuration found" because it reads `config.mcp` which only exists on Pi configs. On the Mac (where `lu doctor` runs), there's no `[mcp]` section. But the check's job is to verify the Pi can reach the Mac MCP -- so it should test from the Mac side by SSHing to the Pi and curling the Mac's health endpoint.

The check should use the Pi config from `config.pi` (which does exist on Mac) and test that the Pi can reach back to the Mac on the configured MCP port.

**Files:**
- Modify: `src/cli/checks/network.rs` (rewrite `pi_mcp_connectivity` to SSH from Mac to Pi and test connectivity back)

### Step-by-step

- [ ] **Step 1: Read current `pi_mcp_connectivity` implementation**

Read `src/cli/checks/network.rs` to understand the current implementation.

- [ ] **Step 2: Rewrite `pi_mcp_connectivity`**

The check should:
1. Get the Pi connection info from `config.pi`
2. Get the Mac's local IP and channel port
3. Load the auth token for the health check
4. SSH to the Pi and curl the Mac's health endpoint with auth
5. Report whether the Pi can reach the Mac MCP

Use the same IP discovery pattern as `get_mac_address()` in `src/cli/setup/deploy.rs`: try Tailscale first (`tailscale ip -4`), then fall back to `en0` and `en1`.

```rust
/// Check if Pi can reach Mac MCP server.
pub fn pi_mcp_connectivity(ctx: &CheckContext) -> CheckResult {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = ctx;
        CheckResult::skip("Pi MCP connectivity check only runs on macOS")
    }

    #[cfg(target_os = "macos")]
    {
        let Some(config) = &ctx.config else {
            return CheckResult::skip("Config not loaded");
        };

        let Some(pi) = &config.pi else {
            return CheckResult::skip("No Pi configured");
        };

        let port = config.channel.port;

        // Discover Mac IP: try Tailscale first, then LAN interfaces
        let mac_ip = get_mac_ip();
        let Some(mac_ip) = mac_ip else {
            return CheckResult::skip("Could not determine Mac IP address");
        };

        // Load auth token for the health check
        let auth_token = load_channel_token().unwrap_or_default();

        // SSH to Pi and curl Mac's health endpoint
        let test_url = format!("http://{mac_ip}:{port}/health");
        let curl_cmd = if auth_token.is_empty() {
            format!(
                "curl -s -o /dev/null -w '%{{http_code}}' --max-time 5 '{test_url}'"
            )
        } else {
            format!(
                "curl -s -o /dev/null -w '%{{http_code}}' --max-time 5 \
                 -H 'Authorization: Bearer {auth_token}' '{test_url}'"
            )
        };

        let output = std::process::Command::new("ssh")
            .args([
                "-n",
                "-o", "BatchMode=yes",
                "-o", "ConnectTimeout=5",
                &format!("{}@{}", pi.user, pi.host),
                &curl_cmd,
            ])
            .output();

        match output {
            Ok(o) if o.status.success() => {
                let status_code = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if status_code == "200" {
                    CheckResult::pass(format!(
                        "Pi can reach Mac MCP ({mac_ip}:{port})"
                    ))
                } else {
                    CheckResult::fail(
                        format!("Pi got HTTP {status_code} from Mac MCP"),
                        format!(
                            "Check Mac MCP auth token and firewall\n\
                             Tested: {test_url}"
                        ),
                        "pi-mcp-auth-error",
                    )
                }
            }
            Ok(_) => CheckResult::fail(
                "Pi cannot reach Mac MCP server",
                format!(
                    "Check network connectivity between Pi and Mac\n\
                     Tested: {test_url}\n\
                     Verify Mac firewall allows port {port}"
                ),
                "pi-mcp-unreachable",
            ),
            Err(e) => CheckResult::fail(
                format!("SSH test failed: {e}"),
                "Check SSH connectivity with `lu pi`",
                "pi-ssh-error",
            ),
        }
    }
}

/// Get Mac's IP address, preferring Tailscale, then LAN interfaces.
#[cfg(target_os = "macos")]
fn get_mac_ip() -> Option<String> {
    // Try Tailscale first
    if let Ok(output) = std::process::Command::new("tailscale")
        .args(["ip", "-4"])
        .output()
    {
        if output.status.success() {
            let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !ip.is_empty() {
                return Some(ip);
            }
        }
    }

    // Fall back to LAN interfaces
    for iface in &["en0", "en1"] {
        if let Ok(output) = std::process::Command::new("ipconfig")
            .args(["getifaddr", iface])
            .output()
        {
            if output.status.success() {
                let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !ip.is_empty() {
                    return Some(ip);
                }
            }
        }
    }

    None
}

/// Load channel token from ~/.ludolph/channel_token.
#[cfg(target_os = "macos")]
fn load_channel_token() -> Option<String> {
    let path = crate::config::config_dir().join("channel_token");
    std::fs::read_to_string(&path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
```
```

- [ ] **Step 3: Update check dependencies in mod.rs**

The `pi_mcp_connectivity` check currently depends on `["pi_reachable", "mac_mcp_running"]`. This is correct -- we need the Pi to be SSH-reachable and the Mac MCP to be running before testing Pi-to-Mac connectivity. No change needed to deps, but verify the dependency list is still correct after the rewrite.

- [ ] **Step 4: Run tests and clippy**

Run: `cargo test --all-features`
Run: `cargo clippy --all-features -- -D warnings`

- [ ] **Step 5: Manual test**

Run: `cargo run -- doctor`

Expected: The `pi_mcp_connectivity` check should now show either:
- `[ok] Pi can reach Mac MCP (192.168.86.49:8202)`
- `[!!] Pi cannot reach Mac MCP server` (with actionable fix hint)

Instead of the previous `[--] Cannot check: pi_mcp_connectivity (No MCP configuration found)`.

- [ ] **Step 6: Commit**

```bash
git add src/cli/checks/network.rs
git commit -m "fix: pi_mcp_connectivity checks actual Pi-to-Mac connectivity"
```

---

## Verification

After all 5 tasks, run the full validation suite:

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-features -- -D warnings`
- [ ] `cargo test --all-features`
- [ ] `cargo run -- doctor` (all checks should pass including pi_mcp_connectivity)
- [ ] Test SSE fallback: temporarily set Pi's primary MCP URL to a bad address, restart, verify it connects via fallback and sends Telegram notification
- [ ] Push to develop and verify CI passes (both Linux and macOS clippy jobs)
