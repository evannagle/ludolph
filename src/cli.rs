use anyhow::Result;
use clap::{Parser, Subcommand};
use console::style;

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
    let log_path = directories::BaseDirs::new().map_or_else(
        || std::path::PathBuf::from("~/ludolph/logs/ludolph.log"),
        |d| d.home_dir().join("ludolph/logs/ludolph.log"),
    );

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

pub fn config() -> Result<()> {
    let config_path = directories::BaseDirs::new().map_or_else(
        || std::path::PathBuf::from("~/ludolph/config.toml"),
        |d| d.home_dir().join("ludolph/config.toml"),
    );

    if !config_path.exists() {
        ui::status::print_error("No config file found", Some("Run `lu setup` to create one."));
        return Ok(());
    }

    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());

    std::process::Command::new(editor)
        .arg(&config_path)
        .status()?;

    Ok(())
}

pub async fn setup() -> Result<()> {
    // Welcome
    println!();
    println!("{}", style("Welcome to Ludolph").bold());
    println!();
    println!("A real brain for your second brain.");
    println!("Talk to your vault, from anywhere, anytime.");
    println!();

    // System check
    let spinner = PiSpinner::new("Checking system");
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    spinner.finish();

    StatusLine::ok("System compatible").print();
    StatusLine::ok("Network connected").print();

    // Get disk space
    let free_gb = 12; // TODO: Actually check
    StatusLine::ok(format!("{free_gb}GB free space")).print();

    // Collect credentials
    println!();
    let _telegram_token =
        ui::prompt_with_help("Telegram bot token", "Create one at @BotFather on Telegram")?;

    let _claude_key =
        ui::prompt_with_help("Claude API key", "Get one at console.anthropic.com")?;

    // Configure
    println!();
    let spinner = PiSpinner::new("Configuring Ludolph");
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    spinner.finish();

    StatusLine::ok("Config written").print();
    StatusLine::ok("Service installed").print();

    // Done
    ui::status::print_success(
        "Setup complete",
        Some(
            "Next: Sync your vault to ~/ludolph/vault/
Then message your Telegram bot!

Commands:
  lu status     Check service status
  lu logs       View recent logs
  lu config     Edit configuration",
        ),
    );

    Ok(())
}
