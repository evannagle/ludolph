//! Service-related diagnostic checks.

use std::process::Command;

#[cfg(target_os = "macos")]
use std::fs;
#[cfg(target_os = "macos")]
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::time::Duration;

#[cfg(target_os = "macos")]
use anyhow::Result;

use super::{CheckContext, CheckResult};
#[cfg(target_os = "macos")]
use crate::config::{self, DEFAULT_CHANNEL_PORT};

/// Get the ludolph directory (~/.ludolph).
#[cfg(target_os = "macos")]
fn ludolph_dir() -> std::path::PathBuf {
    config::config_dir()
}

/// Load auth token from config files.
///
/// Tries `channel_token` first, then `mcp_token`. Skips empty files.
#[cfg(target_os = "macos")]
fn load_auth_token() -> Option<String> {
    let ludolph_dir = ludolph_dir();

    for filename in &["channel_token", "mcp_token"] {
        let path = ludolph_dir.join(filename);
        if let Ok(content) = fs::read_to_string(&path) {
            let trimmed = content.trim().to_string();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }

    None
}

/// Test if a health endpoint is responding.
#[cfg(target_os = "macos")]
fn test_health(url: &str, auth_token: &str) -> bool {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build();

    let Ok(client) = client else {
        return false;
    };

    let resp = client
        .get(url)
        .header("Authorization", format!("Bearer {auth_token}"))
        .send();

    resp.is_ok_and(|r| r.status().is_success())
}

/// Check if Mac MCP server is running.
pub fn mac_mcp_running(ctx: &CheckContext) -> CheckResult {
    // Only check on macOS
    #[cfg(not(target_os = "macos"))]
    {
        let _ = ctx;
        CheckResult::skip("Mac MCP check only runs on macOS")
    }

    #[cfg(target_os = "macos")]
    {
        let Some(_config) = &ctx.config else {
            return CheckResult::skip("Config not loaded");
        };

        // Load auth token
        let Some(auth_token) = load_auth_token() else {
            return CheckResult::fail(
                "No MCP auth token found",
                "Check ~/.ludolph/channel_token or ~/.ludolph/mcp_token",
                "mcp-no-token",
            );
        };

        let mcp_url = format!("http://localhost:{DEFAULT_CHANNEL_PORT}/health");

        if test_health(&mcp_url, &auth_token) {
            CheckResult::pass(format!(
                "Mac MCP server healthy (port {DEFAULT_CHANNEL_PORT})"
            ))
        } else {
            CheckResult::fail(
                "Mac MCP server not responding",
                format!(
                    "Check logs: tail -f {}/mcp/mcp_server.log\n\
                     Restart: launchctl kickstart gui/$(id -u)/dev.ludolph.mcp",
                    ludolph_dir().display()
                ),
                "mcp-unreachable",
            )
        }
    }
}

/// Check if Pi ludolph service is running.
pub fn pi_service_running(ctx: &CheckContext) -> CheckResult {
    let Some(config) = &ctx.config else {
        return CheckResult::skip("Config not loaded");
    };

    let Some(pi) = &config.pi else {
        return CheckResult::skip("No Pi configured");
    };

    // Check systemd service status via SSH
    let output = Command::new("ssh")
        .args([
            "-n",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=5",
            &format!("{}@{}", pi.user, pi.host),
            "systemctl --user is-active ludolph.service 2>/dev/null",
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let status = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if status == "active" {
                CheckResult::pass(format!("Pi service running at {}", pi.host))
            } else {
                CheckResult::fail(
                    format!("Pi service status: {status}"),
                    format!(
                        "Start service: ssh {}@{} 'systemctl --user start ludolph.service'",
                        pi.user, pi.host
                    ),
                    "pi-service-stopped",
                )
            }
        }
        Ok(_) => CheckResult::fail(
            "Pi service not found or not running",
            format!(
                "Check status: ssh {}@{} 'systemctl --user status ludolph.service'\n\
                 View logs: ssh {}@{} 'tail -f ~/.ludolph/ludolph.log'",
                pi.user, pi.host, pi.user, pi.host
            ),
            "pi-service-missing",
        ),
        Err(e) => CheckResult::fail(
            format!("Could not check Pi service: {e}"),
            "Check SSH connectivity with `lu pi`",
            "pi-ssh-error",
        ),
    }
}

/// Check if MCP port is available or held by a healthy process.
pub fn mac_mcp_port_available(ctx: &CheckContext) -> CheckResult {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = ctx;
        CheckResult::skip("MCP port check only runs on macOS")
    }

    #[cfg(target_os = "macos")]
    {
        let _ = ctx;

        // Check if anything is listening on the MCP port
        let output = Command::new("lsof")
            .args(["-ti", &format!(":{DEFAULT_CHANNEL_PORT}")])
            .output();

        let Ok(output) = output else {
            return CheckResult::skip("Could not run lsof to check port");
        };

        if !output.status.success() || output.stdout.is_empty() {
            // Nothing listening — port is available
            return CheckResult::pass(format!("Port {DEFAULT_CHANNEL_PORT} available"));
        }

        // Something is listening — check if health endpoint responds
        let auth_token = load_auth_token().unwrap_or_default();
        let mcp_url = format!("http://localhost:{DEFAULT_CHANNEL_PORT}/health");

        if test_health(&mcp_url, &auth_token) {
            CheckResult::pass(format!(
                "Port {DEFAULT_CHANNEL_PORT} in use by healthy MCP server"
            ))
        } else {
            let pids = String::from_utf8_lossy(&output.stdout).trim().to_string();
            CheckResult::fail(
                format!(
                    "Port {DEFAULT_CHANNEL_PORT} held by stale process (PID: {})",
                    pids.lines().collect::<Vec<_>>().join(", ")
                ),
                format!(
                    "Kill stale process: kill {pids}\n\
                     Then restart: launchctl kickstart gui/$(id -u)/dev.ludolph.mcp"
                ),
                "mcp-port-conflict",
            )
        }
    }
}

/// Check MCP configuration consistency.
///
/// Validates that:
/// - Launchd plist uses the correct port (8202)
/// - Auth token in plist matches the canonical token
/// - Claude Code's `~/.mcp.json` has matching token
pub fn mcp_config_consistent(_ctx: &CheckContext) -> CheckResult {
    #[cfg(not(target_os = "macos"))]
    {
        CheckResult::skip("MCP config check only runs on macOS")
    }

    #[cfg(target_os = "macos")]
    {
        use std::path::PathBuf;

        let home = std::env::var("HOME").unwrap_or_default();
        let plist_path = PathBuf::from(&home).join("Library/LaunchAgents/dev.ludolph.mcp.plist");
        let mcp_json_path = PathBuf::from(&home).join(".mcp.json");

        // Check plist exists
        if !plist_path.exists() {
            return CheckResult::fail(
                "Launchd plist not found",
                "Run installer: ./scripts/install-mcp.sh",
                "mcp-plist-missing",
            );
        }

        // Read plist and check port
        let plist_content = match fs::read_to_string(&plist_path) {
            Ok(c) => c,
            Err(e) => {
                return CheckResult::fail(
                    format!("Cannot read plist: {e}"),
                    "Check file permissions",
                    "mcp-plist-unreadable",
                );
            }
        };

        // Check port in plist (look for <string>8201</string> or <string>8202</string>)
        if plist_content.contains("<string>8201</string>")
            && plist_content.contains("<key>PORT</key>")
        {
            return CheckResult::fail(
                "Launchd plist uses wrong port (8201 instead of 8202)",
                "Update plist PORT to 8202 or re-run installer:\n\
                 ./scripts/install-mcp.sh",
                "mcp-port-mismatch",
            );
        }

        // Load canonical token (checks channel_token first, then mcp_token)
        let Some(canonical_token) = load_auth_token() else {
            return CheckResult::fail(
                "No auth token file found",
                "Check ~/.ludolph/channel_token or ~/.ludolph/mcp_token",
                "mcp-token-missing",
            );
        };

        // Check ~/.mcp.json token consistency
        if mcp_json_path.exists() {
            if let Ok(content) = fs::read_to_string(&mcp_json_path) {
                if content.contains("CHANNEL_AUTH_TOKEN") && !content.contains(&canonical_token) {
                    return CheckResult::fail(
                        "Token mismatch: ~/.mcp.json has different token than auth token file",
                        format!("Update CHANNEL_AUTH_TOKEN in ~/.mcp.json to:\n{canonical_token}"),
                        "mcp-token-mismatch",
                    );
                }
            }
        }

        CheckResult::pass("MCP configuration consistent")
    }
}

// =============================================================================
// Fix Functions
// =============================================================================

/// Result of attempting to fix a configuration issue.
#[derive(Debug, Clone)]
pub struct FixResult {
    /// Whether any fixes were applied.
    pub fixed: bool,
    /// Description of what was fixed (or why it couldn't be fixed).
    pub message: String,
}

impl FixResult {
    /// Create a result indicating fixes were applied.
    #[must_use]
    #[allow(dead_code)]
    pub fn fixed(message: impl Into<String>) -> Self {
        Self {
            fixed: true,
            message: message.into(),
        }
    }

    /// Create a result indicating no fixes were needed.
    #[must_use]
    pub fn no_fix_needed(message: impl Into<String>) -> Self {
        Self {
            fixed: false,
            message: message.into(),
        }
    }
}

/// Attempt to fix MCP configuration issues.
///
/// This function repairs:
/// - Plist port (8201 -> 8202)
/// - Token mismatch in `~/.mcp.json`
/// - Missing plist (by returning an error suggesting re-running setup)
///
/// Returns a `FixResult` indicating what was fixed.
#[cfg(target_os = "macos")]
pub fn fix_mcp_config() -> Result<FixResult> {
    let home = std::env::var("HOME")?;
    let plist_path = PathBuf::from(&home).join("Library/LaunchAgents/dev.ludolph.mcp.plist");
    let mcp_json_path = PathBuf::from(&home).join(".mcp.json");

    let mut fixes: Vec<String> = Vec::new();
    let mut needs_restart = false;

    // Check if plist exists
    if !plist_path.exists() {
        return Ok(FixResult::no_fix_needed(
            "Plist missing - run `lu setup mcp` to create it",
        ));
    }

    // Fix 1: Plist port
    if let Ok(content) = fs::read_to_string(&plist_path) {
        if content.contains("<string>8201</string>") && content.contains("<key>PORT</key>") {
            let fixed_content = content.replace("<string>8201</string>", "<string>8202</string>");
            fs::write(&plist_path, fixed_content)?;
            fixes.push("Updated plist port to 8202".to_string());
            needs_restart = true;
        }
    }

    // Fix 2: Token in ~/.mcp.json (uses load_auth_token for channel_token/mcp_token fallback)
    if let Some(canonical_token) = load_auth_token() {
        if mcp_json_path.exists() {
            if let Ok(content) = fs::read_to_string(&mcp_json_path) {
                if content.contains("CHANNEL_AUTH_TOKEN") && !content.contains(&canonical_token) {
                    if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(servers) = json.get_mut("mcpServers") {
                            if let Some(ludolph) = servers.get_mut("ludolph") {
                                if let Some(env) = ludolph.get_mut("env") {
                                    if let Some(token) = env.get_mut("CHANNEL_AUTH_TOKEN") {
                                        *token = serde_json::Value::String(canonical_token);

                                        let updated = serde_json::to_string_pretty(&json)?;
                                        fs::write(&mcp_json_path, updated)?;
                                        fixes.push(
                                            "Updated CHANNEL_AUTH_TOKEN in ~/.mcp.json".to_string(),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Fix 3: Kill stale process on MCP port
    let port_output = Command::new("lsof")
        .args(["-ti", &format!(":{DEFAULT_CHANNEL_PORT}")])
        .output();
    if let Ok(output) = port_output {
        if output.status.success() && !output.stdout.is_empty() {
            // Something is on the port — check if it's healthy
            let auth_token = load_auth_token().unwrap_or_default();
            let mcp_url = format!("http://localhost:{DEFAULT_CHANNEL_PORT}/health");
            if !test_health(&mcp_url, &auth_token) {
                let pids = String::from_utf8_lossy(&output.stdout);
                for pid in pids.trim().lines() {
                    let _ = Command::new("kill").arg(pid.trim()).status();
                }
                fixes.push(format!(
                    "Killed stale process on port {DEFAULT_CHANNEL_PORT}"
                ));
                needs_restart = true;
            }
        }
    }

    // Reload launchd service if we made changes
    if needs_restart {
        let _ = Command::new("launchctl")
            .args(["unload", plist_path.to_str().unwrap()])
            .status();

        std::thread::sleep(Duration::from_secs(1));

        let _ = Command::new("launchctl")
            .args(["load", plist_path.to_str().unwrap()])
            .status();

        fixes.push("Reloaded launchd service".to_string());
    }

    if fixes.is_empty() {
        Ok(FixResult::no_fix_needed("No fixes needed"))
    } else {
        Ok(FixResult::fixed(fixes.join(", ")))
    }
}

/// Stub for non-macOS platforms.
#[cfg(not(target_os = "macos"))]
#[allow(clippy::unnecessary_wraps)]
pub fn fix_mcp_config() -> anyhow::Result<FixResult> {
    Ok(FixResult::no_fix_needed("MCP fix only runs on macOS"))
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn load_auth_token_handles_missing_files() {
        // Token loading depends on home directory
        // Just verify it doesn't panic
        let _ = load_auth_token();
    }

    #[test]
    fn fix_result_constructors() {
        let fixed = FixResult::fixed("test fix");
        assert!(fixed.fixed);
        assert_eq!(fixed.message, "test fix");

        let no_fix = FixResult::no_fix_needed("nothing to do");
        assert!(!no_fix.fixed);
        assert_eq!(no_fix.message, "nothing to do");
    }
}
