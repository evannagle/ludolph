//! Network-related diagnostic checks.

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

/// Get Mac's IP address, preferring Tailscale, then LAN interfaces.
#[cfg(target_os = "macos")]
fn get_mac_ip() -> Option<String> {
    use std::process::Command;
    // Try Tailscale first
    if let Ok(output) = Command::new("tailscale").args(["ip", "-4"]).output() {
        if output.status.success() {
            let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !ip.is_empty() {
                return Some(ip);
            }
        }
    }

    // Fall back to LAN interfaces
    for iface in &["en0", "en1"] {
        if let Ok(output) = Command::new("ipconfig").args(["getifaddr", iface]).output() {
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

/// Load channel token from `~/.ludolph/channel_token`.
#[cfg(target_os = "macos")]
fn load_channel_token() -> Option<String> {
    let path = crate::config::config_dir().join("channel_token");
    std::fs::read_to_string(&path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Check if Pi can reach Mac MCP server.
///
/// Connects via SSH from the Mac to the Pi and curls the Mac's channel `/health`
/// endpoint back, verifying actual bidirectional connectivity.
pub fn pi_mcp_connectivity(ctx: &CheckContext) -> CheckResult {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = ctx;
        CheckResult::skip("Pi MCP connectivity check only runs on macOS")
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        let Some(config) = &ctx.config else {
            return CheckResult::skip("Config not loaded");
        };

        let Some(pi) = &config.pi else {
            return CheckResult::skip("No Pi configured");
        };

        let port = config.channel.port;

        let Some(mac_ip) = get_mac_ip() else {
            return CheckResult::fail(
                "Cannot determine Mac IP address",
                "Ensure Tailscale is running (`tailscale up`) or that \
                 en0/en1 has an IP address.",
                "mac-ip-unknown",
            );
        };

        let Some(token) = load_channel_token() else {
            return CheckResult::fail(
                "No channel token found",
                "Generate a token: `lu setup channel`\n\
                 Token file: ~/.ludolph/channel_token",
                "channel-token-missing",
            );
        };

        let url = format!("http://{mac_ip}:{port}/health");

        let output = Command::new("ssh")
            .args([
                "-n",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=5",
                &format!("{}@{}", pi.user, pi.host),
                &format!(
                    "curl -s -o /dev/null -w '%{{http_code}}' \
                     --max-time 5 \
                     -H 'Authorization: Bearer {token}' \
                     '{url}'"
                ),
            ])
            .output();

        match output {
            Ok(o) if o.status.success() => {
                let response = String::from_utf8_lossy(&o.stdout).trim().to_string();

                if response == "200" {
                    CheckResult::pass(format!("Pi can reach Mac MCP at {url}"))
                } else if response == "401" || response == "403" {
                    CheckResult::fail(
                        format!(
                            "Pi got HTTP {response} from Mac MCP \
                             (auth token mismatch)"
                        ),
                        "Regenerate token: `lu setup channel`\n\
                         Then redeploy to Pi: `lu setup deploy`",
                        "mcp-auth-mismatch",
                    )
                } else {
                    CheckResult::fail(
                        format!("Pi got HTTP {response} from Mac MCP at {url}"),
                        "Check MCP logs: \
                         tail -f ~/.ludolph/mcp/mcp_server.log",
                        "mcp-error",
                    )
                }
            }
            Ok(_) | Err(_) => CheckResult::fail(
                "Pi cannot reach Mac MCP server",
                format!(
                    "SSH to Pi or curl to {url} failed.\n\
                     Check: Is the channel server running? (`lu doctor`)\n\
                     Check: Can Pi resolve {mac_ip}? \
                     (`ssh {}@{} ping -c1 {mac_ip}`)\n\
                     Check firewall settings on Mac.",
                    pi.user, pi.host
                ),
                "mcp-unreachable",
            ),
        }
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
