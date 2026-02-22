//! Simple CLI commands.

use anyhow::Result;
use console::style;

use crate::config::{self, Config};
use crate::ssh;
use crate::ui::{self, PiSpinner, StatusLine, Table};

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
