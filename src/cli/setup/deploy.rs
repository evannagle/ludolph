//! Deploy phase for Ludolph setup.
//!
//! This phase:
//! 1. Tests SSH to Pi
//! 2. Creates ~/.ludolph/ on Pi
//! 3. Copies lu binary
//! 4. Writes config.toml (with channel token)
//! 5. Creates systemd user service
//! 6. Enables and starts service

use std::fmt::Write as _;
use std::fs;
use std::process::Command;

use anyhow::{Context, Result};
use console::style;

use crate::config::{self, Config, PiConfig};
use crate::ssh;
use crate::ui::{self, Spinner, StatusLine};

const REPO: &str = "evannagle/ludolph";

/// Get the ludolph directory (~/.ludolph).
fn ludolph_dir() -> std::path::PathBuf {
    config::config_dir()
}

/// Run SSH command and return output.
fn ssh_run(pi: &PiConfig, cmd: &str) -> Result<String> {
    let output = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "StrictHostKeyChecking=accept-new",
            &format!("{}@{}", pi.user, pi.host),
            cmd,
        ])
        .output()
        .context("Failed to run SSH command")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("SSH command failed: {}", stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Run SSH command, ignoring failures.
fn ssh_run_ignore(pi: &PiConfig, cmd: &str) {
    let _ = Command::new("ssh")
        .args([
            "-n",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            &format!("{}@{}", pi.user, pi.host),
            cmd,
        ])
        .output();
}

/// Copy file to Pi via SCP.
fn scp_copy(pi: &PiConfig, local_path: &str, remote_path: &str) -> Result<()> {
    let status = Command::new("scp")
        .args([
            "-q",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            local_path,
            &format!("{}@{}:{}", pi.user, pi.host, remote_path),
        ])
        .status()
        .context("Failed to run SCP")?;

    if !status.success() {
        anyhow::bail!("SCP failed to copy {local_path} to {remote_path}");
    }

    Ok(())
}

/// Generate the Pi config.toml content.
fn generate_pi_config(
    config: &Config,
    mcp_url: &str,
    mcp_auth_token: &str,
    mac_address: Option<&str>,
) -> String {
    let mut content = format!(
        r#"[telegram]
bot_token = "{}"
allowed_users = [{}]

[claude]
api_key = "{}"
model = "{}"

[mcp]
url = "{}"
auth_token = "{}"
"#,
        config.telegram.bot_token,
        config
            .telegram
            .allowed_users
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", "),
        config.claude.api_key,
        config.claude.model,
        mcp_url,
        mcp_auth_token,
    );

    if let Some(mac) = mac_address {
        let _ = writeln!(content, "mac_address = \"{mac}\"");
    }

    content
}

/// Create systemd user service file content.
const fn systemd_service_content() -> &'static str {
    r"[Unit]
Description=Ludolph Telegram Bot
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=%h/.ludolph/bin/lu
Restart=always
RestartSec=5
StandardOutput=append:%h/.ludolph/ludolph.log
StandardError=append:%h/.ludolph/ludolph.log

[Install]
WantedBy=default.target
"
}

/// Download the Pi binary from GitHub releases.
fn download_pi_binary() -> Result<std::path::PathBuf> {
    let version = env!("CARGO_PKG_VERSION");
    let version_tag = format!("v{version}");
    let target = "aarch64-unknown-linux-gnu";

    let url = format!("https://github.com/{REPO}/releases/download/{version_tag}/lu-{target}");

    let temp_file = std::env::temp_dir().join("lu-pi-binary");

    let status = Command::new("curl")
        .args(["-fsSL", "-o", temp_file.to_str().unwrap(), &url])
        .status()
        .context("Failed to download Pi binary")?;

    if !status.success() {
        anyhow::bail!("Failed to download Pi binary from {url}");
    }

    Ok(temp_file)
}

/// Get the Mac's network address for Pi to connect back.
fn get_mac_address() -> Result<String> {
    // Try Tailscale first
    if let Ok(output) = Command::new("tailscale").args(["ip", "-4"]).output() {
        if output.status.success() {
            let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !ip.is_empty() {
                return Ok(ip);
            }
        }
    }

    // Fall back to local network IP
    #[cfg(target_os = "macos")]
    {
        for iface in &["en0", "en1"] {
            if let Ok(output) = Command::new("ipconfig").args(["getifaddr", iface]).output() {
                if output.status.success() {
                    let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !ip.is_empty() {
                        return Ok(ip);
                    }
                }
            }
        }
    }

    anyhow::bail!("Could not determine Mac's IP address")
}

/// Get Mac's MAC address for Wake-on-LAN.
fn get_mac_hw_address() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        for iface in &["en0", "en1"] {
            if let Ok(output) = Command::new("ifconfig").arg(iface).output() {
                if output.status.success() {
                    let text = String::from_utf8_lossy(&output.stdout);
                    for line in text.lines() {
                        if line.contains("ether") {
                            let parts: Vec<&str> = line.split_whitespace().collect();
                            if parts.len() >= 2 {
                                return Some(parts[1].to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Deploy binary to Pi.
fn deploy_binary(pi: &PiConfig) -> Result<()> {
    // Stop any existing service
    let spinner = Spinner::new("Stopping existing Ludolph...");
    ssh_run_ignore(
        pi,
        "systemctl --user stop ludolph.service 2>/dev/null; pkill -f 'ludolph/bin/lu' 2>/dev/null || true",
    );
    spinner.finish();

    // Create directories on Pi
    let spinner = Spinner::new("Creating directories on Pi...");
    ssh_run(pi, "mkdir -p ~/.ludolph/bin ~/.config/systemd/user")?;
    spinner.finish();
    StatusLine::ok("Directories created").print();

    // Download and copy binary
    let spinner = Spinner::new("Downloading Pi binary...");
    let binary_path = download_pi_binary()?;
    spinner.finish();
    StatusLine::ok("Pi binary downloaded").print();

    let spinner = Spinner::new("Copying binary to Pi...");
    scp_copy(pi, binary_path.to_str().unwrap(), "~/.ludolph/bin/lu")?;
    ssh_run(pi, "chmod +x ~/.ludolph/bin/lu")?;
    spinner.finish();
    StatusLine::ok("Binary installed").print();

    // Clean up temp file
    let _ = fs::remove_file(&binary_path);

    Ok(())
}

/// Deploy config to Pi.
fn deploy_config(pi: &PiConfig, config: &Config) -> Result<()> {
    let spinner = Spinner::new("Configuring Pi...");

    // Get Mac's address for MCP connection
    let mac_ip = get_mac_address()?;
    let mcp_port = 8202; // Default MCP port
    let mcp_url = format!("http://{mac_ip}:{mcp_port}");

    // Read MCP auth token
    let mcp_token_file = ludolph_dir().join("mcp_token");
    let mcp_auth_token = if mcp_token_file.exists() {
        fs::read_to_string(&mcp_token_file)?.trim().to_string()
    } else {
        let channel_token_file = ludolph_dir().join("channel_token");
        if channel_token_file.exists() {
            fs::read_to_string(&channel_token_file)?.trim().to_string()
        } else {
            anyhow::bail!("No auth token found. Run `lu setup mcp` first.");
        }
    };

    let mac_hw_address = get_mac_hw_address();
    let pi_config_content =
        generate_pi_config(config, &mcp_url, &mcp_auth_token, mac_hw_address.as_deref());

    // Write config via SSH
    ssh_run(
        pi,
        &format!(
            "cat > ~/.ludolph/config.toml << 'LUDOLPH_CONFIG_EOF'\n{pi_config_content}LUDOLPH_CONFIG_EOF"
        ),
    )?;

    // Write channel token to Pi
    let channel_token_file = ludolph_dir().join("channel_token");
    if channel_token_file.exists() {
        let token = fs::read_to_string(&channel_token_file)?.trim().to_string();
        ssh_run(
            pi,
            &format!(
                "echo '{token}' > ~/.ludolph/channel_token && chmod 600 ~/.ludolph/channel_token"
            ),
        )?;
    }

    spinner.finish();
    StatusLine::ok("Configuration written").print();

    Ok(())
}

/// Install and start systemd service on Pi.
fn install_systemd_service(pi: &PiConfig) -> Result<()> {
    // Install systemd service
    let spinner = Spinner::new("Installing systemd service...");
    let service_content = systemd_service_content();
    ssh_run(
        pi,
        &format!(
            "cat > ~/.config/systemd/user/ludolph.service << 'LUDOLPH_SERVICE_EOF'\n{service_content}LUDOLPH_SERVICE_EOF"
        ),
    )?;
    spinner.finish();
    StatusLine::ok("Systemd service installed").print();

    // Enable lingering and start service
    let spinner = Spinner::new("Starting Ludolph service...");
    ssh_run_ignore(pi, "loginctl enable-linger $USER 2>/dev/null");
    ssh_run(
        pi,
        "systemctl --user daemon-reload && systemctl --user enable ludolph.service && systemctl --user restart ludolph.service",
    )?;
    spinner.finish();
    StatusLine::ok("Service started").print();

    // Verify service is running
    std::thread::sleep(std::time::Duration::from_secs(2));
    let status = ssh_run(
        pi,
        "systemctl --user is-active ludolph.service 2>/dev/null || echo 'unknown'",
    )?;
    if status.trim() == "active" {
        StatusLine::ok("Ludolph is running (auto-restarts on crash)").print();
    } else {
        println!();
        println!("  Service may not have started. Check with:");
        println!(
            "  {}",
            style(format!(
                "ssh {}@{} 'systemctl --user status ludolph.service'",
                pi.user, pi.host
            ))
            .cyan()
        );
    }

    Ok(())
}

/// Run the deploy phase.
pub async fn setup_deploy(pi: &PiConfig) -> Result<()> {
    println!();
    ui::status::section("Deploy to Pi");
    println!();

    let config = Config::load().context("No config found. Run `lu setup credentials` first.")?;

    // Test SSH connection
    let spinner = Spinner::new(&format!("Testing SSH to {}@{}...", pi.user, pi.host));
    match ssh::test_connection(&pi.host, &pi.user) {
        Ok(()) => spinner.finish(),
        Err(e) => {
            spinner.finish_error();
            anyhow::bail!("SSH connection failed: {e}");
        }
    }
    StatusLine::ok(format!("Connected to {}@{}", pi.user, pi.host)).print();

    // Deploy binary
    deploy_binary(pi)?;

    // Deploy config
    deploy_config(pi, &config)?;

    // Install and start service
    install_systemd_service(pi)?;

    // Print PATH hint
    println!();
    println!("  The lu binary is installed. To use it directly on Pi:");
    println!(
        "  {}",
        style(format!(
            "ssh {}@{} 'echo \"export PATH=\\$HOME/.ludolph/bin:\\$PATH\" >> ~/.bashrc'",
            pi.user, pi.host
        ))
        .cyan()
    );

    Ok(())
}
