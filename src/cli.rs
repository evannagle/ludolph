use anyhow::Result;
use clap::{Parser, Subcommand};

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
}

pub async fn status() -> Result<()> {
    println!("Checking Ludolph status...");
    // TODO: Check systemd/launchd service status
    Ok(())
}

pub async fn logs() -> Result<()> {
    println!("Tailing logs...");
    // TODO: Tail ~/ludolph/logs/
    Ok(())
}

pub async fn restart() -> Result<()> {
    println!("Restarting Ludolph...");
    // TODO: Restart systemd/launchd service
    Ok(())
}

pub async fn update() -> Result<()> {
    println!("Updating Ludolph...");
    // TODO: Download latest binary, replace, restart
    Ok(())
}

pub async fn uninstall() -> Result<()> {
    println!("Uninstalling Ludolph...");
    // TODO: Prompt for confirmation, stop service, remove files
    Ok(())
}

pub async fn config() -> Result<()> {
    let config_path = directories::BaseDirs::new()
        .map(|d| d.home_dir().join("ludolph/config.toml"))
        .unwrap_or_else(|| std::path::PathBuf::from("~/ludolph/config.toml"));

    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());

    std::process::Command::new(editor)
        .arg(&config_path)
        .status()?;

    Ok(())
}
