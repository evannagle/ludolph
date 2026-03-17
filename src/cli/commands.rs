//! CLI commands.

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use console::style;
use walkdir::WalkDir;

use super::checks::{self, CheckResult};
use crate::config::{self, Config};
use crate::ssh;
use crate::ui::{self, Spinner, StatusLine, prompt};

const REPO: &str = "evannagle/ludolph";
const LUDOLPH_DIR: &str = ".ludolph";

/// Run health checks and return appropriate exit code.
pub fn check() -> ExitCode {
    // Print version
    println!();
    println!("lu {}", env!("CARGO_PKG_VERSION"));
    println!();

    // CLI check (always passes if we got here)
    StatusLine::ok("CLI").print();

    // Config check
    let config = Config::load().map_or_else(
        |_| {
            StatusLine::skip("Config (not found)").print();
            None
        },
        |cfg| {
            StatusLine::ok("Config loaded").print();
            Some(cfg)
        },
    );

    // Vault/MCP check
    if let Some(cfg) = config.as_ref() {
        if let Some(ref mcp) = cfg.mcp {
            StatusLine::ok(format!("MCP: {}", mcp.url)).print();
        } else if let Some(ref vault) = cfg.vault {
            if vault.path.exists() {
                let count = WalkDir::new(&vault.path)
                    .into_iter()
                    .filter_map(Result::ok)
                    .filter(|e| e.file_type().is_file())
                    .count();
                StatusLine::ok(format!("Vault accessible ({count} files)")).print();
            } else {
                StatusLine::error(format!("Vault not found: {}", vault.path.display())).print();
                println!();
                return ExitCode::FAILURE;
            }
        } else {
            StatusLine::skip("Vault/MCP (not configured)").print();
        }
    } else {
        StatusLine::skip("Vault/MCP (no config)").print();
    }

    println!();
    ExitCode::SUCCESS
}

pub fn config_cmd() -> Result<()> {
    let config_path = config::config_path();

    if !config_path.exists() {
        ui::status::print_error(
            "No config file found",
            Some("Run `lu setup` to create one."),
        );
        return Ok(());
    }

    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());

    std::process::Command::new(editor)
        .arg(&config_path)
        .status()?;

    Ok(())
}

#[allow(clippy::unnecessary_wraps)]
pub fn pi() -> Result<()> {
    let Ok(config) = Config::load() else {
        ui::status::print_error("No config found", Some("Run `lu setup` first."));
        return Ok(());
    };

    let Some(pi) = config.pi else {
        println!();
        StatusLine::error("No Pi configured").print();
        ui::status::hint("Run `lu setup` to configure Pi connection");
        println!();
        return Ok(());
    };

    println!();
    println!("{}", style("Pi Connection").bold());
    println!();

    let spinner = Spinner::new(&format!("Connecting to {}@{}...", pi.user, pi.host));

    match ssh::test_connection(&pi.host, &pi.user) {
        Ok(()) => {
            spinner.finish();
        }
        Err(e) => {
            spinner.finish_error();
            ui::status::hint(&format!("Connection failed: {e}"));
            ui::status::hint("Check if Pi is online and SSH key auth is set up");
        }
    }

    println!();
    Ok(())
}

// =============================================================================
// MCP Commands
// =============================================================================

fn ludolph_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(LUDOLPH_DIR)
}

fn mcp_version_file() -> PathBuf {
    ludolph_dir().join("mcp").join("VERSION")
}

/// Show current MCP server version.
#[allow(clippy::unnecessary_wraps)]
pub fn mcp_version() -> Result<()> {
    println!();

    let version_file = mcp_version_file();
    if version_file.exists() {
        let version = fs::read_to_string(&version_file)
            .map_or_else(|_| "unknown".to_string(), |v| v.trim().to_string());
        StatusLine::ok(format!("MCP server version: {version}")).print();
    } else {
        StatusLine::error("MCP server not installed").print();
        ui::status::hint("Run the installer: curl -sSL https://ludolph.dev/install | bash");
    }

    println!();
    Ok(())
}

/// Update MCP server to latest version.
pub fn mcp_update() -> Result<()> {
    println!();
    println!("{}", style("MCP Server Update").bold());
    println!();

    let mcp_dir = ludolph_dir().join("mcp");

    // Check if MCP is installed
    if !mcp_dir.exists() {
        StatusLine::error("MCP server not installed").print();
        ui::status::hint("Run the installer: curl -sSL https://ludolph.dev/install | bash");
        println!();
        return Ok(());
    }

    // Get current version
    let current_version = fs::read_to_string(mcp_version_file())
        .map_or_else(|_| "unknown".to_string(), |v| v.trim().to_string());
    StatusLine::ok(format!("Current version: {current_version}")).print();

    // Fetch latest release tag
    let spinner = Spinner::new("Checking for updates...");
    let latest_tag = fetch_latest_release_tag()?;
    spinner.finish();

    let latest_version = latest_tag.trim_start_matches('v');
    if current_version == latest_version {
        StatusLine::ok("Already up to date").print();
        println!();
        return Ok(());
    }

    StatusLine::ok(format!("New version available: {latest_version}")).print();

    // Download and extract
    let spinner = Spinner::new("Downloading update...");
    let url = format!(
        "https://github.com/{REPO}/releases/download/{latest_tag}/ludolph-mcp-{latest_tag}.tar.gz"
    );

    // Backup current MCP
    let backup_dir = ludolph_dir().join("mcp.bak");
    if backup_dir.exists() {
        fs::remove_dir_all(&backup_dir)?;
    }
    fs::rename(&mcp_dir, &backup_dir)?;

    // Download and extract new version
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!(
            "curl -sSL '{}' | tar -xz -C '{}'",
            url,
            ludolph_dir().display()
        ))
        .status()?;

    if !status.success() {
        // Restore backup
        if backup_dir.exists() {
            fs::rename(&backup_dir, &mcp_dir)?;
        }
        spinner.finish_error();
        ui::status::hint("Download failed. Restored previous version.");
        println!();
        return Ok(());
    }

    // Remove backup
    if backup_dir.exists() {
        fs::remove_dir_all(&backup_dir)?;
    }

    spinner.finish();

    // Restart service
    mcp_restart_service()?;

    let new_version = fs::read_to_string(mcp_version_file())
        .map_or_else(|_| latest_version.to_string(), |v| v.trim().to_string());
    StatusLine::ok(format!("Updated to version: {new_version}")).print();

    println!();
    Ok(())
}

/// Restart MCP server.
#[allow(clippy::unnecessary_wraps)]
pub fn mcp_restart() -> Result<()> {
    println!();
    println!("{}", style("MCP Server Restart").bold());
    println!();

    mcp_restart_service()?;

    println!();
    Ok(())
}

fn fetch_latest_release_tag() -> Result<String> {
    let output = std::process::Command::new("curl")
        .args([
            "-sSL",
            &format!("https://api.github.com/repos/{REPO}/releases/latest"),
        ])
        .output()?;

    let body = String::from_utf8_lossy(&output.stdout);

    // Extract tag_name from JSON (simple parsing without serde)
    for line in body.lines() {
        if line.contains("\"tag_name\"")
            && let Some(start) = line.find(": \"")
        {
            let rest = &line[start + 3..];
            if let Some(end) = rest.find('"') {
                return Ok(rest[..end].to_string());
            }
        }
    }

    anyhow::bail!("Could not parse release tag from GitHub API")
}

#[allow(clippy::unnecessary_wraps)]
fn mcp_restart_service() -> Result<()> {
    let spinner = Spinner::new("Restarting MCP server...");

    // macOS: launchctl
    #[cfg(target_os = "macos")]
    {
        let plist = dirs::home_dir()
            .expect("home dir")
            .join("Library/LaunchAgents/dev.ludolph.mcp.plist");

        if plist.exists() {
            let _ = std::process::Command::new("launchctl")
                .args(["unload", plist.to_str().unwrap()])
                .status();

            std::thread::sleep(std::time::Duration::from_secs(1));

            let _ = std::process::Command::new("launchctl")
                .args(["load", plist.to_str().unwrap()])
                .status();

            spinner.finish();
        } else {
            spinner.finish_error();
            ui::status::hint("Could not find launchd plist. Restart manually.");
        }
        Ok(())
    }

    // Linux: systemctl
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "restart", "ludolph-mcp"])
            .status();
        spinner.finish();
        Ok(())
    }

    // Fallback for other platforms
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        spinner.finish_error();
        ui::status::hint("Could not restart MCP service. Restart manually.");
        Ok(())
    }
}

// =============================================================================
// Doctor Command
// =============================================================================

/// Run diagnostic checks and report results.
pub async fn doctor() -> ExitCode {
    println!();
    println!("{}", style("Ludolph Doctor").bold());
    println!();

    // Run checks in a blocking task to avoid issues with reqwest::blocking in async context
    let results = tokio::task::spawn_blocking(|| {
        let (_, results) = checks::run_all_checks();
        results
    })
    .await
    .expect("spawn_blocking failed");

    let mut has_failures = false;
    let mut diagnosis: Option<(&str, &str)> = None;

    for (name, result) in &results {
        match result {
            CheckResult::Pass { message } => {
                StatusLine::ok(message).print();
            }
            CheckResult::Fail {
                message,
                fix_hint,
                doc_anchor,
            } => {
                has_failures = true;
                StatusLine::error(message).print();

                // Print fix hint indented
                for line in fix_hint.lines() {
                    println!("      {}", style(line).dim());
                }

                // Remember first failure for diagnosis
                if diagnosis.is_none() {
                    diagnosis = Some((name, doc_anchor));
                }
            }
            CheckResult::Skip { reason } => {
                StatusLine::skip(format!("Cannot check: {name} ({reason})")).print();
            }
        }
    }

    println!();

    if has_failures {
        if let Some((check_name, anchor)) = diagnosis {
            println!(
                "{}",
                style(format!(
                    "DIAGNOSIS: {} issue. See docs/troubleshooting.md#{anchor}",
                    check_name.replace('_', " ")
                ))
                .yellow()
            );
        }
        println!();
        ExitCode::FAILURE
    } else {
        ui::status::print_success("All checks passed", None);
        ExitCode::SUCCESS
    }
}

// =============================================================================
// Uninstall Command
// =============================================================================

/// Uninstall Ludolph from specified targets.
#[allow(clippy::fn_params_excessive_bools)]
pub fn uninstall(mac: bool, pi: bool, all: bool, yes: bool) -> Result<()> {
    println!();
    println!("{}", style("Ludolph Uninstall").bold());
    println!();

    // Determine what to uninstall
    // If no flags provided, default to mac only on macOS
    #[cfg(target_os = "macos")]
    let uninstall_mac = mac || all || !pi;
    #[cfg(not(target_os = "macos"))]
    let uninstall_mac = false;

    let uninstall_pi = pi || all;

    if !uninstall_mac && !uninstall_pi {
        println!("  Usage: lu uninstall [--mac] [--pi] [--all]");
        println!();
        return Ok(());
    }

    // Show what will be uninstalled
    println!("  This will remove:");
    if uninstall_mac {
        println!("    - ~/.ludolph/ directory (Mac)");
        println!("    - MCP launchd service (Mac)");
    }
    if uninstall_pi {
        println!("    - ~/.ludolph/ directory (Pi)");
        println!("    - ludolph systemd service (Pi)");
    }
    println!();
    println!("  {}:", style("Preserved").dim());
    println!("    - Your Obsidian vault");
    println!("    - SSH keys");
    println!("    - Tailscale configuration");
    println!();

    // Confirmation (skip if --yes flag provided)
    if !yes && !prompt::confirm("Proceed with uninstall?")? {
        println!();
        StatusLine::skip("Uninstall cancelled").print();
        println!();
        return Ok(());
    }

    println!();

    // Uninstall Mac
    if uninstall_mac {
        uninstall_mac_internal()?;
    }

    // Uninstall Pi
    if uninstall_pi {
        uninstall_pi_internal()?;
    }

    println!();
    ui::status::print_success("Uninstall complete", None);
    Ok(())
}

#[cfg(target_os = "macos")]
fn uninstall_mac_internal() -> Result<()> {
    let spinner = Spinner::new("Uninstalling from Mac...");

    // Stop and remove launchd service
    let plist = dirs::home_dir()
        .expect("home dir")
        .join("Library/LaunchAgents/dev.ludolph.mcp.plist");

    if plist.exists() {
        let _ = std::process::Command::new("launchctl")
            .args(["unload", plist.to_str().unwrap()])
            .status();

        fs::remove_file(&plist)?;
    }

    // Remove ~/.ludolph directory
    let ludolph_dir = ludolph_dir();
    if ludolph_dir.exists() {
        fs::remove_dir_all(&ludolph_dir)?;
    }

    spinner.finish();
    StatusLine::ok("Mac uninstalled").print();
    Ok(())
}

#[cfg(not(target_os = "macos"))]
#[allow(clippy::unnecessary_wraps)]
fn uninstall_mac_internal() -> Result<()> {
    StatusLine::skip("Mac uninstall only runs on macOS").print();
    Ok(())
}

#[allow(clippy::unnecessary_wraps)]
fn uninstall_pi_internal() -> Result<()> {
    let config = Config::load().ok();

    let Some(pi) = config.as_ref().and_then(|c| c.pi.as_ref()) else {
        StatusLine::skip("No Pi configured").print();
        return Ok(());
    };

    let spinner = Spinner::new(&format!(
        "Uninstalling from Pi ({}@{})...",
        pi.user, pi.host
    ));

    // Stop and disable systemd service
    let _ = std::process::Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            &format!("{}@{}", pi.user, pi.host),
            "systemctl --user stop ludolph.service 2>/dev/null; \
             systemctl --user disable ludolph.service 2>/dev/null; \
             rm -f ~/.config/systemd/user/ludolph.service; \
             rm -rf ~/.ludolph",
        ])
        .status();

    spinner.finish();
    StatusLine::ok(format!("Pi uninstalled ({})", pi.host)).print();
    Ok(())
}
