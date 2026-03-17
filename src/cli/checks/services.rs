//! Service-related diagnostic checks.

use std::process::Command;

#[cfg(target_os = "macos")]
use std::fs;
#[cfg(target_os = "macos")]
use std::time::Duration;

use super::{CheckContext, CheckResult};
#[cfg(target_os = "macos")]
use crate::config;

#[cfg(target_os = "macos")]
const CHANNEL_PORT: u16 = 8202;

/// Get the ludolph directory (~/.ludolph).
#[cfg(target_os = "macos")]
fn ludolph_dir() -> std::path::PathBuf {
    config::config_dir()
}

/// Load auth token from config files.
#[cfg(target_os = "macos")]
fn load_auth_token() -> Option<String> {
    let ludolph_dir = ludolph_dir();
    let channel_token_file = ludolph_dir.join("channel_token");
    let mcp_token_file = ludolph_dir.join("mcp_token");

    if channel_token_file.exists() {
        fs::read_to_string(&channel_token_file)
            .ok()
            .map(|s| s.trim().to_string())
    } else if mcp_token_file.exists() {
        fs::read_to_string(&mcp_token_file)
            .ok()
            .map(|s| s.trim().to_string())
    } else {
        None
    }
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

        let mcp_url = format!("http://localhost:{CHANNEL_PORT}/health");

        if test_health(&mcp_url, &auth_token) {
            CheckResult::pass(format!("Mac MCP server healthy (port {CHANNEL_PORT})"))
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn load_auth_token_handles_missing_files() {
        // Token loading depends on home directory
        // Just verify it doesn't panic
        let _ = load_auth_token();
    }
}
