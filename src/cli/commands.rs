//! CLI commands.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Result;
use console::style;
use walkdir::WalkDir;

use super::checks::{self, CheckResult};
use crate::config::{self, Config, IndexTier};
use crate::index::indexer::Indexer;
use crate::index::manifest::Manifest;
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
            // Kill any stale process holding the MCP port before restarting
            let port_output = std::process::Command::new("lsof")
                .args(["-ti", &format!(":{}", crate::config::DEFAULT_CHANNEL_PORT)])
                .output();
            if let Ok(output) = port_output {
                if output.status.success() && !output.stdout.is_empty() {
                    let pids = String::from_utf8_lossy(&output.stdout);
                    for pid in pids.trim().lines() {
                        let _ = std::process::Command::new("kill").arg(pid.trim()).status();
                    }
                }
            }

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
#[allow(clippy::too_many_lines)]
pub async fn doctor(fix: bool) -> ExitCode {
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
    let mut has_mcp_failure = false;
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

                // Track if MCP-related checks fail
                if matches!(
                    *name,
                    "mcp_config_consistent" | "mac_mcp_port_available" | "mac_mcp_running"
                ) {
                    has_mcp_failure = true;
                }

                // Print fix hint indented (only if not in fix mode)
                if !fix {
                    for line in fix_hint.lines() {
                        println!("      {}", style(line).dim());
                    }
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

    // Attempt fixes if requested
    if fix && has_mcp_failure {
        println!();
        println!("{}", style("Attempting fixes...").bold());

        // Run fix in blocking task
        let fix_result = tokio::task::spawn_blocking(checks::fix_mcp_config)
            .await
            .expect("spawn_blocking failed");

        match fix_result {
            Ok(result) if result.fixed => {
                StatusLine::ok(&result.message).print();

                // Re-run checks to verify
                println!();
                println!("{}", style("Verifying...").bold());

                let verify_results = tokio::task::spawn_blocking(|| {
                    let (_, results) = checks::run_all_checks();
                    results
                })
                .await
                .expect("spawn_blocking failed");

                let mut verify_failures = false;
                for (name, result) in &verify_results {
                    match result {
                        CheckResult::Pass { message } => {
                            StatusLine::ok(message).print();
                        }
                        CheckResult::Fail { message, .. } => {
                            verify_failures = true;
                            StatusLine::error(message).print();
                        }
                        CheckResult::Skip { reason } => {
                            StatusLine::skip(format!("Cannot check: {name} ({reason})")).print();
                        }
                    }
                }

                println!();
                if verify_failures {
                    ui::status::print_error("Some issues remain after fixes", None);
                    return ExitCode::FAILURE;
                }
                ui::status::print_success("All checks passed after fixes", None);
                return ExitCode::SUCCESS;
            }
            Ok(result) => {
                StatusLine::skip(&result.message).print();
            }
            Err(e) => {
                StatusLine::error(format!("Fix failed: {e}")).print();
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
            if !fix {
                println!(
                    "{}",
                    style("TIP: Run `lu doctor --fix` to attempt automatic repair").dim()
                );
            }
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
#[allow(clippy::fn_params_excessive_bools, unused_variables)]
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

// =============================================================================
// Index Command
// =============================================================================

/// Count markdown files in a vault path, excluding hidden directories.
fn count_vault_files(vault_path: &Path) -> usize {
    WalkDir::new(vault_path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            !name.starts_with('.')
        })
        .filter_map(Result::ok)
        .filter(|e| {
            e.path().is_file()
                && e.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| x.eq_ignore_ascii_case("md"))
        })
        .count()
}

/// Estimate the cost of a Deep tier indexing run.
///
/// Assumes ~3 chunks/file avg, ~250 input tokens/chunk.
/// Haiku pricing: $0.25/M input, $1.25/M output.
#[allow(clippy::cast_precision_loss)]
fn estimate_deep_cost(file_count: usize) -> f64 {
    const AVG_CHUNKS_PER_FILE: usize = 3;
    const INPUT_TOKENS_PER_CHUNK: usize = 250;
    const OUTPUT_TOKENS_PER_CHUNK: usize = 50;
    const INPUT_PRICE_PER_MILLION: f64 = 0.25;
    const OUTPUT_PRICE_PER_MILLION: f64 = 1.25;

    let total_chunks = file_count * AVG_CHUNKS_PER_FILE;
    let total_input = (total_chunks * INPUT_TOKENS_PER_CHUNK) as f64;
    let total_output = (total_chunks * OUTPUT_TOKENS_PER_CHUNK) as f64;

    (total_input / 1_000_000.0).mul_add(
        INPUT_PRICE_PER_MILLION,
        (total_output / 1_000_000.0) * OUTPUT_PRICE_PER_MILLION,
    )
}

/// Build or rebuild the vault index.
#[allow(clippy::too_many_lines)]
pub async fn index_cmd(tier_override: Option<String>, rebuild: bool, status: bool) -> Result<()> {
    println!();

    // Load config and get vault path.
    let Ok(config) = Config::load() else {
        ui::status::print_error("No config found", Some("Run `lu setup` first."));
        return Ok(());
    };

    let Some(vault_config) = config.vault.as_ref() else {
        ui::status::print_error(
            "No vault configured",
            Some("Run `lu setup` to configure your vault path."),
        );
        return Ok(());
    };

    // Expand tilde in vault path.
    let vault_path_str = vault_config.path.to_string_lossy();
    let expanded = shellexpand::tilde(vault_path_str.as_ref());
    let vault_path = PathBuf::from(expanded.as_ref());

    if !vault_path.exists() {
        ui::status::print_error(
            &format!("Vault path not found: {}", vault_path.display()),
            Some("Check your config.toml vault.path setting."),
        );
        return Ok(());
    }

    // --status: show manifest info and exit.
    if status {
        let index_dir = Manifest::index_dir(&vault_path);
        if let Ok(manifest) = Manifest::load(&index_dir) {
            StatusLine::ok(format!("Tier:         {}", manifest.tier)).print();
            StatusLine::ok(format!("Files:        {}", manifest.file_count)).print();
            StatusLine::ok(format!("Chunks:       {}", manifest.chunk_count)).print();
            StatusLine::ok(format!("Last indexed: {}", manifest.last_indexed)).print();
            if !manifest.folders.is_empty() {
                println!();
                println!("  Folders:");
                let mut folder_list: Vec<_> = manifest.folders.iter().collect();
                folder_list.sort_by_key(|(k, _)| k.as_str());
                for (folder, folder_stats) in folder_list {
                    println!(
                        "    {:30} {} files, {} chunks",
                        folder, folder_stats.file_count, folder_stats.chunk_count
                    );
                }
            }
        } else {
            StatusLine::skip("No index found").print();
            ui::status::hint("Run `lu index` to build the index.");
        }
        println!();
        return Ok(());
    }

    // Parse tier from override or fall back to config default.
    let tier = match tier_override.as_deref() {
        Some("quick") => IndexTier::Quick,
        Some("standard") => IndexTier::Standard,
        Some("deep") => IndexTier::Deep,
        Some(other) => {
            ui::status::print_error(
                &format!("Unknown tier: '{other}'"),
                Some("Valid tiers: quick, standard, deep"),
            );
            return Ok(());
        }
        None => config.index.tier,
    };

    // For Deep tier on >100 files, show cost estimate and confirm.
    if tier == IndexTier::Deep {
        let file_count = count_vault_files(&vault_path);
        if file_count > 100 {
            let cost = estimate_deep_cost(file_count);
            println!(
                "  {} Deep tier on {} files — estimated cost: ${:.4}",
                style("Note:").yellow(),
                file_count,
                cost
            );
            println!();
            if !prompt::confirm("Continue?")? {
                println!();
                StatusLine::skip("Index cancelled").print();
                println!();
                return Ok(());
            }
            println!();
        }
    }

    let action = if rebuild { "Rebuilding" } else { "Indexing" };
    let spinner = Spinner::new(&format!("{action} vault ({tier})..."));

    match Indexer::new(vault_path, tier).run(rebuild).await {
        Ok(run_stats) => {
            spinner.finish();
            StatusLine::ok(format!(
                "Indexed {} files, {} chunks ({} skipped)",
                run_stats.files_indexed, run_stats.chunks_created, run_stats.files_skipped
            ))
            .print();
            if run_stats.files_removed > 0 {
                StatusLine::ok(format!(
                    "Removed {} stale chunk files",
                    run_stats.files_removed
                ))
                .print();
            }
        }
        Err(e) => {
            spinner.finish_error();
            anyhow::bail!("Indexing failed: {e}");
        }
    }

    println!();
    Ok(())
}

// =============================================================================
// Knowledge Command
// =============================================================================

/// Show what Lu knows — index, embeddings, observations, learned content.
pub async fn knowledge_cmd() -> Result<()> {
    println!();

    let Ok(config) = Config::load() else {
        ui::status::print_error("No config found", Some("Run `lu setup` first."));
        return Ok(());
    };

    // 1. Vault index status
    if let Some(vault_config) = config.vault.as_ref() {
        let vault_path_str = vault_config.path.to_string_lossy();
        let expanded = shellexpand::tilde(vault_path_str.as_ref());
        let vault_path = PathBuf::from(expanded.as_ref());

        let index_dir = Manifest::index_dir(&vault_path);
        if let Ok(manifest) = Manifest::load(&index_dir) {
            StatusLine::ok(format!(
                "Vault index: {} files, {} chunks ({})",
                manifest.file_count, manifest.chunk_count, manifest.tier
            ))
            .print();
        } else {
            StatusLine::skip("Vault index: not built").print();
            ui::status::hint("Run `lu index` to build the vault index.");
        }
    } else {
        StatusLine::skip("Vault: not configured").print();
    }

    // 2. Embeddings and learned content (via MCP)
    if let Some(mcp_config) = config.mcp.as_ref() {
        let client = crate::mcp_client::McpClient::from_config(mcp_config);

        // Learned content status
        let result = client
            .call_tool("learned_status", &serde_json::json!({}))
            .await;
        match result {
            Ok(status) => {
                for line in status.lines() {
                    StatusLine::ok(line.to_string()).print();
                }
            }
            Err(_) => {
                StatusLine::skip("Learned content: MCP unreachable").print();
            }
        }

        // Observations count
        let obs = client.get_observations(0, 100).await;
        if obs.is_empty() {
            StatusLine::skip("Observations: none saved").print();
        } else {
            let prefs = obs.iter().filter(|o| o.category == "preference").count();
            let facts = obs.iter().filter(|o| o.category == "fact").count();
            let ctx = obs.iter().filter(|o| o.category == "context").count();
            StatusLine::ok(format!(
                "Observations: {} total ({} preferences, {} facts, {} context)",
                obs.len(),
                prefs,
                facts,
                ctx
            ))
            .print();
        }
    } else {
        StatusLine::skip("MCP: not configured").print();
    }

    println!();
    Ok(())
}

// =============================================================================
// Publish Command
// =============================================================================

/// Publish vault profile to the community registry.
pub async fn publish_cmd(
    name: Option<String>,
    owner: Option<String>,
    description: Option<String>,
) -> Result<()> {
    println!();

    let config =
        Config::load().map_err(|_| anyhow::anyhow!("No config found. Run `lu setup` first."))?;

    let mcp_config = config
        .mcp
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("MCP not configured. Run `lu setup` first."))?;

    let client = crate::mcp_client::McpClient::from_config(mcp_config);

    // Prompt for required fields if not provided
    let vault_name = name.unwrap_or_else(|| {
        ui::prompt::prompt_with_default("Vault name", "", None).unwrap_or_default()
    });
    let vault_owner = owner.unwrap_or_else(|| {
        ui::prompt::prompt_with_default("GitHub username", "", None).unwrap_or_default()
    });
    let vault_desc = description.unwrap_or_else(|| {
        ui::prompt::prompt_with_default("Description", "", None).unwrap_or_default()
    });

    if vault_name.is_empty() || vault_owner.is_empty() {
        ui::status::print_error("Name and owner are required", None);
        return Ok(());
    }

    let spinner = Spinner::new("Generating vault profile...");
    let result = client
        .call_tool(
            "vault_publish",
            &serde_json::json!({
                "name": vault_name,
                "owner": vault_owner,
                "description": vault_desc,
            }),
        )
        .await?;
    spinner.finish();

    println!("{result}");
    println!();
    Ok(())
}

// =============================================================================
// Learn Command
// =============================================================================

/// Learn from files, URLs, or folders.
pub async fn learn_cmd(source: &str, forget: bool, status: bool) -> Result<()> {
    println!();

    let config =
        Config::load().map_err(|_| anyhow::anyhow!("No config found. Run `lu setup` first."))?;

    let mcp_config = config
        .mcp
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("MCP not configured. Run `lu setup` first."))?;

    let client = crate::mcp_client::McpClient::from_config(mcp_config);

    if status {
        let spinner = Spinner::new("Checking knowledge base...");
        let result = client
            .call_tool("learned_status", &serde_json::json!({}))
            .await?;
        spinner.finish();
        println!("  {result}");
        println!();
        return Ok(());
    }

    if forget {
        let spinner = Spinner::new(&format!("Forgetting '{source}'..."));
        let result = client
            .call_tool("forget", &serde_json::json!({ "source": source }))
            .await?;
        spinner.finish();
        println!("  {result}");
        println!();
        return Ok(());
    }

    // Determine source type and call appropriate tool
    let is_url = source.starts_with("http://") || source.starts_with("https://");
    let is_github = source.starts_with("github:") || source.starts_with("https://github.com/");
    let path = PathBuf::from(shellexpand::tilde(source).as_ref());
    let is_dir = !is_url && !is_github && path.is_dir();

    let (tool_name, args) = if is_github {
        ("learn_github", serde_json::json!({ "repo": source }))
    } else if is_url {
        ("learn_url", serde_json::json!({ "url": source }))
    } else if is_dir {
        (
            "learn_folder",
            serde_json::json!({ "path": path.to_string_lossy() }),
        )
    } else {
        (
            "learn_file",
            serde_json::json!({ "path": path.to_string_lossy() }),
        )
    };

    let label = if is_github {
        "Learning from GitHub repo"
    } else if is_url {
        "Learning from URL"
    } else if is_dir {
        "Learning from folder"
    } else {
        "Learning from file"
    };
    let spinner = Spinner::new(&format!("{label}..."));
    let result = client.call_tool(tool_name, &args).await?;
    spinner.finish();

    println!("  {result}");
    println!();
    Ok(())
}

// =============================================================================
// Teach Command
// =============================================================================

/// Teach a topic using Lu's knowledge base.
pub async fn teach_cmd(topic: &str, audience: &str, export: bool, tier: u8) -> Result<()> {
    println!();

    let config =
        Config::load().map_err(|_| anyhow::anyhow!("No config found. Run `lu setup` first."))?;

    let mcp_config = config
        .mcp
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("MCP not configured. Run `lu setup` first."))?;

    let client = crate::mcp_client::McpClient::from_config(mcp_config);

    if export {
        let spinner = Spinner::new(&format!("Exporting '{topic}' as .ludo (tier {tier})..."));
        let result = client
            .call_tool(
                "teach_export",
                &serde_json::json!({
                    "topic": topic,
                    "tier": tier,
                }),
            )
            .await?;
        spinner.finish();
        println!("{result}");
    } else {
        let spinner = Spinner::new(&format!("Teaching '{topic}' for {audience}..."));
        let result = client
            .call_tool(
                "teach",
                &serde_json::json!({
                    "topic": topic,
                    "audience": audience,
                }),
            )
            .await?;
        spinner.finish();
        println!("{result}");
    }

    println!();
    Ok(())
}

// =============================================================================
// Update Command
// =============================================================================

/// Platform-specific binary name for downloads.
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
const BINARY_NAME: &str = "lu-x86_64-apple-darwin";

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const BINARY_NAME: &str = "lu-aarch64-apple-darwin";

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const BINARY_NAME: &str = "lu-x86_64-unknown-linux-gnu";

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const BINARY_NAME: &str = "lu-aarch64-unknown-linux-gnu";

/// Get the current CLI version from Cargo.toml (compile-time).
const fn current_cli_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Get Pi's current CLI version via SSH.
fn get_pi_version(host: &str, user: &str) -> Option<String> {
    let output = std::process::Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=5",
            &format!("{user}@{host}"),
            "~/.ludolph/bin/lu --version 2>/dev/null || echo unknown",
        ])
        .output()
        .ok()?;

    if output.status.success() {
        let version_line = String::from_utf8_lossy(&output.stdout);
        // Parse "lu X.Y.Z" output
        let version = version_line
            .trim()
            .strip_prefix("lu ")
            .unwrap_or_else(|| version_line.trim());
        if version == "unknown" || version.is_empty() {
            None
        } else {
            Some(version.to_string())
        }
    } else {
        None
    }
}

/// Update Mac binary to latest version.
fn update_mac_binary(tag: &str) -> Result<bool> {
    let current = current_cli_version();
    let latest = tag.trim_start_matches('v');

    if current == latest {
        return Ok(false);
    }

    let spinner = Spinner::new("Updating Mac binary...");

    // Download to temp file
    let temp_path = std::env::temp_dir().join("lu-update-temp");
    let url = format!("https://github.com/{REPO}/releases/download/{tag}/{BINARY_NAME}");

    let status = std::process::Command::new("curl")
        .args(["-sSL", "-o", temp_path.to_str().unwrap(), &url])
        .status()?;

    if !status.success() {
        spinner.finish_error();
        anyhow::bail!("Failed to download Mac binary");
    }

    // Check file size
    let metadata = fs::metadata(&temp_path)?;
    if metadata.len() == 0 {
        spinner.finish_error();
        fs::remove_file(&temp_path)?;
        anyhow::bail!("Downloaded file is empty");
    }

    // Make executable
    std::process::Command::new("chmod")
        .args(["+x", temp_path.to_str().unwrap()])
        .status()?;

    // Get current binary path and replace
    let current_exe = std::env::current_exe()?;
    fs::rename(&temp_path, &current_exe)?;

    spinner.finish();
    StatusLine::ok(format!("Mac binary updated ({current} → {latest})")).print();

    Ok(true)
}

/// Update MCP server to latest version (reuses existing logic).
fn update_mcp(tag: &str) -> Result<bool> {
    let mcp_dir = ludolph_dir().join("mcp");

    // Skip if MCP not installed
    if !mcp_dir.exists() {
        StatusLine::skip("MCP server not installed").print();
        return Ok(false);
    }

    let current_version = fs::read_to_string(mcp_version_file())
        .map_or_else(|_| "unknown".to_string(), |v| v.trim().to_string());

    let latest_version = tag.trim_start_matches('v');

    if current_version == latest_version {
        return Ok(false);
    }

    let spinner = Spinner::new("Updating MCP server...");

    let url = format!("https://github.com/{REPO}/releases/download/{tag}/ludolph-mcp-{tag}.tar.gz");

    // Backup current MCP
    let backup_dir = ludolph_dir().join("mcp.bak");
    if backup_dir.exists() {
        fs::remove_dir_all(&backup_dir)?;
    }
    fs::rename(&mcp_dir, &backup_dir)?;

    // Download and extract
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
        anyhow::bail!("Failed to download MCP server");
    }

    // Remove backup
    if backup_dir.exists() {
        fs::remove_dir_all(&backup_dir)?;
    }

    spinner.finish();
    StatusLine::ok(format!(
        "MCP server updated ({current_version} → {latest_version})"
    ))
    .print();

    // Restart service
    mcp_restart_service()?;
    StatusLine::ok("MCP service restarted").print();

    Ok(true)
}

/// Update Pi binary via SSH.
#[cfg(target_os = "macos")]
fn update_pi_binary(tag: &str, host: &str, user: &str) -> Result<bool> {
    let latest = tag.trim_start_matches('v');

    // Get current Pi version
    let Some(current) = get_pi_version(host, user) else {
        StatusLine::skip("Could not get Pi version").print();
        return Ok(false);
    };

    if current == latest {
        return Ok(false);
    }

    let spinner = Spinner::new(&format!("Updating Pi ({host})..."));

    // Download binary directly on Pi
    let download_url =
        format!("https://github.com/{REPO}/releases/download/{tag}/lu-aarch64-unknown-linux-gnu");

    let ssh_cmd = format!(
        "curl -sSL -o /tmp/lu-new '{download_url}' && \
         chmod +x /tmp/lu-new && \
         mv /tmp/lu-new ~/.ludolph/bin/lu && \
         systemctl --user restart ludolph.service"
    );

    let status = std::process::Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            &format!("{user}@{host}"),
            &ssh_cmd,
        ])
        .status()?;

    if !status.success() {
        spinner.finish_error();
        ui::status::hint(&format!(
            "Pi update failed. Update manually:\n      ssh {user}@{host} 'curl -sSL -o ~/.ludolph/bin/lu \"{download_url}\" && chmod +x ~/.ludolph/bin/lu'"
        ));
        return Ok(false);
    }

    spinner.finish();
    StatusLine::ok(format!("Pi binary updated ({current} → {latest})")).print();
    StatusLine::ok("Pi service restarted").print();

    Ok(true)
}

#[cfg(not(target_os = "macos"))]
#[allow(clippy::unnecessary_wraps, clippy::missing_const_for_fn)]
fn update_pi_binary(_tag: &str, _host: &str, _user: &str) -> Result<bool> {
    // Pi update only runs from Mac
    Ok(false)
}

/// Update Lu and MCP to latest version.
pub async fn update() -> Result<()> {
    println!();
    println!("{}", style("Ludolph Update").bold());
    println!();

    // Fetch latest release
    let spinner = Spinner::new("Checking for updates...");
    let latest_tag = fetch_latest_release_tag()?;
    spinner.finish();

    let latest_version = latest_tag.trim_start_matches('v');
    let current_cli = current_cli_version();

    // Get MCP version
    let current_mcp = fs::read_to_string(mcp_version_file())
        .map_or_else(|_| "not installed".to_string(), |v| v.trim().to_string());

    // Get Pi info
    let config = Config::load().ok();
    let pi_info = config.as_ref().and_then(|c| c.pi.as_ref());

    let (pi_version, pi_reachable) = pi_info.map_or_else(
        || ("not configured".to_string(), false),
        |pi| match crate::ssh::test_connection(&pi.host, &pi.user) {
            Ok(()) => {
                let v = get_pi_version(&pi.host, &pi.user).unwrap_or_else(|| "unknown".to_string());
                (v, true)
            }
            Err(_) => ("unreachable".to_string(), false),
        },
    );

    // Show current state
    StatusLine::ok(format!(
        "Current: lu v{current_cli}, MCP v{current_mcp}, Pi v{pi_version}"
    ))
    .print();
    StatusLine::ok(format!("Latest: v{latest_version}")).print();

    // Check what needs updating
    let cli_needs_update = current_cli != latest_version;
    let mcp_needs_update = current_mcp != latest_version && current_mcp != "not installed";
    let pi_needs_update = pi_reachable && pi_version != latest_version && pi_version != "unknown";

    if !cli_needs_update && !mcp_needs_update && !pi_needs_update {
        StatusLine::ok("Already up to date").print();
        println!();
        return Ok(());
    }

    // Show what will be updated
    println!();
    println!("Updates available:");
    if cli_needs_update {
        println!("  - Mac binary: {current_cli} → {latest_version}");
    }
    if mcp_needs_update {
        println!("  - MCP server: {current_mcp} → {latest_version}");
    }
    if pi_needs_update {
        if let Some(pi) = pi_info {
            println!(
                "  - Pi binary ({}): {pi_version} → {latest_version}",
                pi.host
            );
        }
    }
    if !pi_reachable && pi_info.is_some() {
        println!("  - Pi: skipped (unreachable)");
    }
    println!();

    // Confirm
    if !prompt::confirm("Proceed with update?")? {
        println!();
        StatusLine::skip("Update cancelled").print();
        println!();
        return Ok(());
    }

    println!();

    // Perform updates
    let cli_updated = cli_needs_update && update_mac_binary(&latest_tag)?;
    let mcp_updated = mcp_needs_update && update_mcp(&latest_tag)?;
    let pi_updated = pi_needs_update
        && pi_info
            .map(|pi| update_pi_binary(&latest_tag, &pi.host, &pi.user))
            .transpose()?
            .unwrap_or(false);

    let any_updated = cli_updated || mcp_updated || pi_updated;

    // After MCP update, verify/repair config
    #[cfg(target_os = "macos")]
    if mcp_updated {
        print!("Verifying MCP configuration... ");
        match checks::fix_mcp_config() {
            Ok(result) if result.fixed => {
                println!("{}", style("fixed").green());
                println!("      {}", style(&result.message).dim());
            }
            Ok(_) => {
                println!("{}", style("ok").green());
            }
            Err(e) => {
                println!("{}", style("warning").yellow());
                println!("      Config repair failed: {e}");
            }
        }
    }

    println!();
    if any_updated {
        ui::status::print_success(&format!("Updated to v{latest_version}"), None);
    } else {
        StatusLine::ok("No updates applied").print();
    }

    Ok(())
}
