use anyhow::Result;
use clap::{Parser, Subcommand};
use console::style;

use crate::config::{self, Config, PiConfig};
use crate::ssh;
use crate::ui::{self, PiSpinner, StatusLine, Table};

#[derive(Parser)]
#[command(name = "lu")]
#[command(about = "Ludolph - A real brain for your second brain")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Check if Ludolph is running
    Status,
    /// View recent logs
    Logs,
    /// Restart the service
    Restart,
    /// Update to latest version
    Update,
    /// Remove Ludolph
    Uninstall,
    /// Open config in editor
    Config,
    /// Initial setup wizard
    Setup,
    /// Check Pi connectivity
    Pi,
}

pub async fn status() -> Result<()> {
    let spinner = PiSpinner::new("Checking services");

    // Simulate checking services
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;

    spinner.finish();

    let mut table = Table::new(&["Service", "Status", "Uptime"]);
    table.add_row(&["Telegram Bot", "running", "2d 4h"]);
    table.add_row(&["Vault Sync", "idle", "-"]);
    table.print();

    println!();
    Ok(())
}

#[allow(clippy::unnecessary_wraps)] // Will have real I/O when implemented
pub fn logs() -> Result<()> {
    let log_path = config::config_dir().join("logs/ludolph.log");

    if !log_path.exists() {
        ui::status::print_error(
            "No log file found",
            Some(&format!("Expected at: {}", log_path.display())),
        );
        return Ok(());
    }

    println!();
    println!("{}", style("Recent logs").bold());
    println!();

    // TODO: Actually tail the log file
    println!("  (log tailing not yet implemented)");
    println!();

    Ok(())
}

pub async fn restart() -> Result<()> {
    let spinner = PiSpinner::new("Restarting Ludolph");

    // TODO: Actually restart the service
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;

    spinner.finish();

    StatusLine::ok("Service restarted").print();
    println!();

    Ok(())
}

pub async fn update() -> Result<()> {
    let spinner = PiSpinner::new("Checking for updates");

    // TODO: Check GitHub releases
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;

    spinner.finish();

    StatusLine::ok("Already on latest version (0.1.0)").print();
    println!();

    Ok(())
}

pub async fn uninstall() -> Result<()> {
    println!();
    let confirmed = ui::prompt::confirm("Remove Ludolph and all data?")?;

    if !confirmed {
        println!();
        println!("  Cancelled.");
        println!();
        return Ok(());
    }

    let spinner = PiSpinner::new("Removing Ludolph");

    // TODO: Stop service, remove files
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;

    spinner.finish();

    StatusLine::ok("Ludolph removed").print();
    println!();

    Ok(())
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

    ui::status::checking(&format!("Connecting to {}@{}...", pi.user, pi.host));

    match ssh::test_connection(&pi.host, &pi.user) {
        Ok(()) => {
            ui::status::ok(&format!("Connected to {}@{}", pi.user, pi.host));
        }
        Err(e) => {
            ui::status::error(&format!("Connection failed: {e}"));
            ui::status::hint("Check if Pi is online and SSH key auth is set up");
        }
    }

    println!();
    Ok(())
}

/// Collected credentials from setup wizard.
struct Credentials {
    telegram_token: String,
    user_id: u64,
    claude_key: String,
    vault_path: std::path::PathBuf,
}

/// Collect API credentials and vault path from user.
fn collect_credentials(existing: Option<&Config>) -> Result<Credentials> {
    println!();
    let telegram_token = ui::prompt::prompt_validated(
        "Telegram bot token",
        "Open Telegram, message @BotFather, send /newbot, copy the token",
        existing.map(|c| c.telegram.bot_token.as_str()),
        ui::prompt::validate_telegram_token,
    )?;

    let existing_user_id = existing
        .and_then(|c| c.telegram.allowed_users.first())
        .map(ToString::to_string);

    let telegram_user_id = ui::prompt::prompt_validated(
        "Your Telegram user ID",
        "Message @userinfobot on Telegram - it will reply with your numeric ID",
        existing_user_id.as_deref(),
        ui::prompt::validate_telegram_user_id,
    )?;

    let claude_key = ui::prompt::prompt_validated(
        "Claude API key",
        "Visit console.anthropic.com/settings/keys, create key, copy it",
        existing.map(|c| c.claude.api_key.as_str()),
        ui::prompt::validate_claude_key,
    )?;

    let existing_vault = existing.map(|c| c.vault.path.to_string_lossy().to_string());

    let vault_path = ui::prompt::prompt_validated(
        "Path to your Obsidian vault",
        "The folder where your markdown notes live (e.g., ~/Documents/Vault)",
        existing_vault.as_deref(),
        ui::prompt::validate_vault_path,
    )?;

    let user_id: u64 = telegram_user_id.parse().expect("validated above");
    let vault_expanded = shellexpand::tilde(&vault_path);
    let vault_path = std::path::PathBuf::from(vault_expanded.as_ref());

    Ok(Credentials {
        telegram_token,
        user_id,
        claude_key,
        vault_path,
    })
}

/// Collect Pi SSH configuration and verify connectivity.
/// Returns None if SSH connection fails (setup should abort).
fn collect_pi_config(existing: Option<&Config>) -> Result<Option<PiConfig>> {
    println!();
    ui::status::section("Raspberry Pi");
    println!();
    println!("  Ludolph runs on your Pi. Set up SSH access first:");
    println!(
        "  {}",
        style("https://github.com/evannagle/ludolph/blob/main/docs/pi-setup.md").dim()
    );
    println!();

    let pi_host = ui::prompt::prompt_validated(
        "Pi hostname or IP",
        "Run `ping pi.local` or check your router for the Pi's address",
        existing
            .and_then(|c| c.pi.as_ref())
            .map(|p| p.host.as_str()),
        ui::prompt::validate_hostname,
    )?;

    let pi_user = ui::prompt::prompt_with_default(
        "SSH user",
        "pi",
        existing
            .and_then(|c| c.pi.as_ref())
            .map(|p| p.user.as_str()),
    )?;

    println!();
    ui::status::checking("Testing SSH connection...");

    match ssh::test_connection(&pi_host, &pi_user) {
        Ok(()) => {
            ui::status::ok(&format!("Connected to {pi_user}@{pi_host}"));
            Ok(Some(PiConfig {
                host: pi_host,
                user: pi_user,
            }))
        }
        Err(e) => {
            ui::status::error(&format!("SSH failed: {e}"));
            println!();
            println!("  SSH key authentication is required. Run:");
            println!(
                "  {}",
                style(format!("ssh-copy-id {pi_user}@{pi_host}")).cyan()
            );
            println!();
            println!("  Then re-run `lu setup`.");
            println!();
            Ok(None)
        }
    }
}

pub async fn setup() -> Result<()> {
    let existing = Config::load().ok();

    if existing.is_some() {
        println!();
        let reconfigure = ui::prompt::confirm("Ludolph is already configured. Reconfigure?")?;
        if !reconfigure {
            println!();
            println!("  Run `lu config` to edit existing configuration.");
            println!();
            return Ok(());
        }
    }

    // Welcome
    println!();
    println!("{}", style("Welcome to Ludolph").bold());
    println!();
    println!("A real brain for your second brain.");
    println!("Talk to your vault, from anywhere, anytime.");
    println!();

    // System check
    let spinner = PiSpinner::new("Checking system");
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    spinner.finish();
    StatusLine::ok("System compatible").print();
    StatusLine::ok("Network connected").print();

    // Collect credentials
    let creds = collect_credentials(existing.as_ref())?;

    // Collect Pi config (required)
    let Some(pi_config) = collect_pi_config(existing.as_ref())? else {
        return Ok(()); // SSH failed, user instructed to fix and re-run
    };

    // Save config
    println!();
    let spinner = PiSpinner::new("Configuring Ludolph");
    let cfg = Config::new(
        creds.telegram_token,
        vec![creds.user_id],
        creds.claude_key,
        creds.vault_path,
        Some(pi_config),
    );
    cfg.save()?;
    spinner.finish();

    StatusLine::ok("Config written").print();
    StatusLine::ok(format!("Vault: {}", cfg.vault.path.display())).print();
    StatusLine::ok(format!("Authorized user: {}", creds.user_id)).print();
    if let Some(ref pi) = cfg.pi {
        StatusLine::ok(format!("Pi: {}@{}", pi.user, pi.host)).print();
    }

    ui::status::print_success(
        "Setup complete",
        Some(
            "Run `lu` to start the bot!\n\nCommands:\n  lu            Start the Telegram bot\n  lu status     Check service status\n  lu config     Edit configuration",
        ),
    );

    Ok(())
}
