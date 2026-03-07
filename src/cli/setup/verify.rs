//! Verify phase for Ludolph setup.
//!
//! This phase:
//! 1. HTTP: Tests Mac MCP health (localhost:8202/health)
//! 2. HTTP: Tests Pi channel health ({pi}:8202/health)
//! 3. Reports success/failure

use std::fs;
use std::process::Command;
use std::time::Duration;

use anyhow::Result;
use console::style;

use crate::config::{self, Config, PiConfig};
use crate::ui::{self, Spinner, StatusLine};

const CHANNEL_PORT: u16 = 8202;

/// Get the ludolph directory (~/.ludolph).
fn ludolph_dir() -> std::path::PathBuf {
    config::config_dir()
}

/// Test if a health endpoint is responding.
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

/// Test Pi health via SSH + curl (more reliable than direct connection).
fn test_pi_health_via_ssh(pi: &PiConfig, auth_token: &str) -> bool {
    let output = Command::new("ssh")
        .args([
            "-n",
            "-o", "BatchMode=yes",
            "-o", "ConnectTimeout=5",
            &format!("{}@{}", pi.user, pi.host),
            &format!(
                "curl -s -m 5 -H 'Authorization: Bearer {auth_token}' 'http://localhost:{CHANNEL_PORT}/health' 2>/dev/null",
            ),
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let body = String::from_utf8_lossy(&o.stdout);
            body.contains("ok") || body.contains("status")
        }
        _ => false,
    }
}

/// Load auth token from config files.
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
    let mcp_url = format!("http://localhost:{CHANNEL_PORT}/health");

    if test_health(&mcp_url, auth_token)? {
        spinner.finish();
        StatusLine::ok(format!("Mac MCP server healthy (port {CHANNEL_PORT})")).print();
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

/// Check Pi service health.
fn check_pi_service(pi: &PiConfig, auth_token: &str) -> Result<bool> {
    let spinner = Spinner::new(&format!("Checking Pi service at {}...", pi.host));

    // Try SSH + curl first (more reliable)
    if test_pi_health_via_ssh(pi, auth_token) {
        spinner.finish();
        StatusLine::ok(format!("Pi service healthy at {}", pi.host)).print();
        return Ok(true);
    }

    // Try direct connection as fallback
    let pi_url = format!("http://{}:{CHANNEL_PORT}/health", pi.host);
    if test_health(&pi_url, auth_token)? {
        spinner.finish();
        StatusLine::ok(format!("Pi service healthy at {}", pi.host)).print();
        return Ok(true);
    }

    spinner.finish_error();
    StatusLine::error(format!("Pi service not responding at {}", pi.host)).print();
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
    Ok(false)
}

/// Print success message with Telegram bot info.
fn print_success_with_bot_info() {
    ui::status::print_success("All services healthy", None);

    // Show Telegram bot info if available
    let config = Config::load().ok();
    if let Some(cfg) = config {
        println!();
        println!("  Send a message to your bot in Telegram to test!");

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

    // Load auth token
    let Some(auth_token) = load_auth_token() else {
        StatusLine::error("No auth token found").print();
        return Ok(());
    };

    // Step 1: Test Mac MCP server (if not on Pi)
    #[cfg(target_os = "macos")]
    {
        if !check_mac_mcp(&auth_token)? {
            all_ok = false;
        }
    }

    // Step 2: Test Pi channel API (if Pi is configured)
    if let Some(pi) = pi {
        if !check_pi_service(pi, &auth_token)? {
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
