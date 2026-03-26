//! Verify phase for Ludolph setup.
//!
//! This phase:
//! 1. HTTP: Tests Mac MCP health (localhost:8202/health)
//! 2. HTTP: Tests Pi channel health ({pi}:8202/health)
//! 3. Reports success/failure

use std::process::Command;
use std::time::Duration;

use anyhow::Result;
use console::style;

#[cfg(target_os = "macos")]
use std::fs;

#[cfg(target_os = "macos")]
use crate::config::{self, DEFAULT_CHANNEL_PORT};
use crate::config::{Config, PiConfig};
use crate::ui::{self, Spinner, StatusLine};

/// Get the ludolph directory (~/.ludolph).
#[cfg(target_os = "macos")]
fn ludolph_dir() -> std::path::PathBuf {
    config::config_dir()
}

/// Test if a health endpoint is responding.
#[cfg(target_os = "macos")]
fn test_health(url: &str, auth_token: &str) -> Result<bool> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

    let resp = client
        .get(url)
        .header("Authorization", format!("Bearer {auth_token}"))
        .send();

    Ok(resp.is_ok_and(|r| r.status().is_success()))
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

/// Check Mac MCP server health.
#[cfg(target_os = "macos")]
fn check_mac_mcp(auth_token: &str) -> Result<bool> {
    let spinner = Spinner::new("Checking Mac MCP server...");
    let mcp_url = format!("http://localhost:{DEFAULT_CHANNEL_PORT}/health");

    if test_health(&mcp_url, auth_token)? {
        spinner.finish();
        StatusLine::ok(format!(
            "Mac MCP server healthy (port {DEFAULT_CHANNEL_PORT})"
        ))
        .print();
        Ok(true)
    } else {
        spinner.finish_error();
        StatusLine::error("Mac MCP server not responding").print();
        println!();
        println!("  Check the server logs:");
        println!(
            "  {}",
            style(format!(
                "tail -f {}/mcp/mcp_server.log",
                ludolph_dir().display()
            ))
            .cyan()
        );
        println!();
        println!("  Restart the server:");
        println!(
            "  {}",
            style("launchctl kickstart gui/$(id -u)/dev.ludolph.mcp").cyan()
        );
        println!();
        Ok(false)
    }
}

/// Check Pi service health via SSH (systemd status).
fn check_pi_service(pi: &PiConfig) -> bool {
    let spinner = Spinner::new(&format!("Checking Pi service at {}...", pi.host));

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

    if let Ok(o) = output {
        if o.status.success() {
            let status = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if status == "active" {
                spinner.finish();
                StatusLine::ok(format!("Pi service running at {}", pi.host)).print();
                return true;
            }
        }
    }

    spinner.finish_error();
    StatusLine::error(format!("Pi service not running at {}", pi.host)).print();
    println!();
    println!("  Check the Pi service:");
    println!(
        "  {}",
        style(format!(
            "ssh {}@{} 'systemctl --user status ludolph.service'",
            pi.user, pi.host
        ))
        .cyan()
    );
    println!();
    println!("  View logs:");
    println!(
        "  {}",
        style(format!(
            "ssh {}@{} 'tail -f ~/.ludolph/ludolph.log'",
            pi.user, pi.host
        ))
        .cyan()
    );
    println!();
    false
}

/// Print success message with Telegram bot info.
fn print_success_with_bot_info() {
    ui::status::print_success("All services healthy", None);

    // Show Telegram bot info if available
    let config = Config::load().ok();
    if let Some(cfg) = config {
        // Try to get bot username
        let client = reqwest::blocking::Client::new();
        if let Ok(resp) = client
            .get(format!(
                "https://api.telegram.org/bot{}/getMe",
                cfg.telegram.bot_token
            ))
            .timeout(Duration::from_secs(5))
            .send()
        {
            if let Ok(json) = resp.json::<serde_json::Value>() {
                if let Some(username) = json["result"]["username"].as_str() {
                    println!();
                    println!(
                        "  Next step: Open your bot in Telegram and send {}",
                        style("/setup").cyan()
                    );
                    println!("  {}", style(format!("https://t.me/{username}")).cyan());
                }
            }
        }
    }
}

/// Run the verify phase.
pub async fn setup_verify(pi: Option<&PiConfig>) -> Result<()> {
    println!();
    ui::status::section("Verification");
    println!();

    let mut all_ok = true;

    // Step 1: Test Mac MCP server (if not on Pi)
    #[cfg(target_os = "macos")]
    {
        // Load auth token
        let Some(auth_token) = load_auth_token() else {
            StatusLine::error("No auth token found").print();
            return Ok(());
        };

        if !check_mac_mcp(&auth_token)? {
            all_ok = false;
        }
    }

    // Step 2: Test Pi service (if Pi is configured)
    if let Some(pi) = pi {
        if !check_pi_service(pi) {
            all_ok = false;
        }
    }

    // Summary
    println!();
    if all_ok {
        print_success_with_bot_info();
    } else {
        ui::status::print_error(
            "Some services failed",
            Some("Fix the issues above and run `lu setup verify` again."),
        );
    }

    Ok(())
}
