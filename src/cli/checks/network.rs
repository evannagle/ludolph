//! Network-related diagnostic checks.

use std::process::Command;

use super::{CheckContext, CheckResult};
use crate::ssh;

/// Check if Pi is reachable via SSH.
pub fn pi_reachable(ctx: &CheckContext) -> CheckResult {
    let Some(config) = &ctx.config else {
        return CheckResult::skip("Config not loaded");
    };

    let Some(pi) = &config.pi else {
        return CheckResult::skip("No Pi configured");
    };

    match ssh::test_connection(&pi.host, &pi.user) {
        Ok(()) => CheckResult::pass(format!("Pi reachable at {}@{}", pi.user, pi.host)),
        Err(e) => {
            let error_msg = e.to_string();

            // Check for common failure modes
            if error_msg.contains("timed out") || error_msg.contains("Connection refused") {
                CheckResult::fail(
                    format!("Pi unreachable: {}@{}", pi.user, pi.host),
                    "Tailscale may not be running on Pi after reboot.\n\
                     Physical access: run `sudo tailscale up` on the Pi.\n\
                     Or check if Pi has power and network connection.",
                    "pi-offline",
                )
            } else if error_msg.contains("Permission denied") {
                CheckResult::fail(
                    format!("SSH key auth failed for {}@{}", pi.user, pi.host),
                    "Check SSH key: `ssh -v {}@{}`\n\
                     Copy key: `ssh-copy-id {}@{}`",
                    "pi-ssh-auth",
                )
            } else {
                CheckResult::fail(
                    format!("SSH connection failed: {e}"),
                    format!("Debug with: ssh -v {}@{}", pi.user, pi.host),
                    "pi-ssh-error",
                )
            }
        }
    }
}

/// Check if Pi can reach Mac MCP server.
pub fn pi_mcp_connectivity(ctx: &CheckContext) -> CheckResult {
    let Some(config) = &ctx.config else {
        return CheckResult::skip("Config not loaded");
    };

    let Some(pi) = &config.pi else {
        return CheckResult::skip("No Pi configured");
    };

    let Some(mcp) = &config.mcp else {
        return CheckResult::skip("No MCP configuration found");
    };

    // Extract host and port from MCP URL
    let mcp_url = &mcp.url;

    // Test connectivity from Pi to Mac MCP using curl
    let output = Command::new("ssh")
        .args([
            "-n",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=5",
            &format!("{}@{}", pi.user, pi.host),
            &format!(
                "curl -sf -o /dev/null -w '%{{http_code}}' \
                 -H 'Authorization: Bearer {}' \
                 '{}/health' 2>/dev/null || echo 'FAIL'",
                mcp.auth_token, mcp_url
            ),
        ])
        .output();

    match output {
        Ok(o) => {
            let response = String::from_utf8_lossy(&o.stdout).trim().to_string();

            if response == "200" {
                CheckResult::pass(format!("Pi can reach Mac MCP at {mcp_url}"))
            } else if response == "FAIL" || response.is_empty() {
                // Check if it's a network issue or Mac is asleep
                CheckResult::fail(
                    format!("Pi cannot reach Mac MCP at {mcp_url}"),
                    "Mac may be asleep. Try waking it with Wake-on-LAN.\n\
                     Check firewall settings on Mac.\n\
                     Verify MCP URL is correct in Pi config.",
                    "mcp-unreachable",
                )
            } else if response == "401" || response == "403" {
                CheckResult::fail(
                    "Pi rejected by Mac MCP (auth token mismatch)",
                    "Regenerate auth token: `lu setup mcp`\n\
                     Then redeploy to Pi: `lu setup deploy`",
                    "mcp-auth-mismatch",
                )
            } else {
                CheckResult::fail(
                    format!("Mac MCP returned unexpected status: {response}"),
                    "Check MCP logs: tail -f ~/.ludolph/mcp/mcp_server.log",
                    "mcp-error",
                )
            }
        }
        Err(e) => CheckResult::fail(
            format!("Could not test Pi→Mac connectivity: {e}"),
            "Check SSH connectivity with `lu pi`",
            "pi-ssh-error",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pi_reachable_skips_without_config() {
        let ctx = CheckContext {
            config: None,
            results: std::collections::HashMap::new(),
        };
        let result = pi_reachable(&ctx);
        assert!(result.is_skip());
    }
}
