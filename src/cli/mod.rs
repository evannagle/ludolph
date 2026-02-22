//! CLI command handling for Ludolph.

mod commands;
mod setup;
mod sync;

use clap::{Parser, Subcommand};

pub use commands::{config_cmd, logs, pi, restart, status, uninstall, update};
pub use setup::{setup, setup_credentials, setup_pi};
pub use sync::{sync_setup, sync_status};

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
    /// Initial setup wizard (or run specific step)
    Setup {
        #[command(subcommand)]
        step: Option<SetupStep>,
    },
    /// Check Pi connectivity
    Pi,
    /// Vault sync management
    Sync {
        #[command(subcommand)]
        command: Option<SyncCommand>,
    },
}

#[derive(Subcommand)]
pub enum SetupStep {
    /// Configure API credentials (Telegram, Claude)
    Credentials,
    /// Configure Pi SSH connection
    Pi,
    /// Configure vault sync (alias for `lu sync setup`)
    Sync,
}

#[derive(Subcommand)]
pub enum SyncCommand {
    /// Set up Syncthing for vault sync
    Setup,
    /// Show sync status
    Status,
}
