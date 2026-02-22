//! CLI command handling for Ludolph.

mod commands;
mod setup;

use clap::{Parser, Subcommand};

pub use commands::{config_cmd, pi};
pub use setup::{setup, setup_credentials, setup_pi};

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
    /// Open config in editor
    Config,
    /// Initial setup wizard (or run specific step)
    Setup {
        #[command(subcommand)]
        step: Option<SetupStep>,
    },
    /// Check Pi connectivity
    Pi,
}

#[derive(Subcommand)]
pub enum SetupStep {
    /// Configure API credentials (Telegram, Claude)
    Credentials,
    /// Configure Pi SSH connection
    Pi,
}
