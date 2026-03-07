//! CLI command handling for Ludolph.

mod commands;
mod setup;

use clap::{Parser, Subcommand};

pub use commands::{check, config_cmd, mcp_restart, mcp_update, mcp_version, pi};
pub use setup::{
    setup, setup_credentials_cmd, setup_deploy_cmd, setup_mcp_cmd, setup_pi_cmd, setup_verify_cmd,
};

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
    /// Health check
    Check,
    /// Open config in editor
    Config,
    /// Initial setup wizard (or run specific step)
    Setup {
        /// Run all setup phases in order
        #[arg(long)]
        full: bool,

        #[command(subcommand)]
        step: Option<SetupStep>,
    },
    /// Check Pi connectivity
    Pi,
    /// Manage MCP server
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
}

#[derive(Subcommand)]
pub enum SetupStep {
    /// Configure API credentials (Telegram, Claude, vault path)
    Credentials,
    /// Configure Pi SSH connection
    Pi,
    /// Set up MCP server (Mac only)
    Mcp,
    /// Deploy lu binary and config to Pi (Mac only)
    Deploy,
    /// Verify all services are running
    Verify,
}

#[derive(Subcommand)]
pub enum McpAction {
    /// Update MCP server to latest version
    Update,
    /// Show current MCP version
    Version,
    /// Restart MCP server
    Restart,
}
