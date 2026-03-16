//! CLI command handling for Ludolph.

mod checks;
mod commands;
mod plugin;
mod setup;

use clap::{Parser, Subcommand};

pub use commands::{
    check, config_cmd, doctor, mcp_restart, mcp_update, mcp_version, pi, uninstall,
};
pub use plugin::{
    plugin_check, plugin_create, plugin_disable, plugin_enable, plugin_install, plugin_list,
    plugin_logs, plugin_publish, plugin_remove, plugin_search, plugin_setup, plugin_update,
};
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
    /// Manage plugins
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },
    /// Diagnose Ludolph installation
    Doctor,
    /// Uninstall Ludolph
    Uninstall {
        /// Uninstall from Mac only
        #[arg(long)]
        mac: bool,
        /// Uninstall from Pi only
        #[arg(long)]
        pi: bool,
        /// Uninstall from both Mac and Pi
        #[arg(long)]
        all: bool,
        /// Skip confirmation prompt
        #[arg(long, short = 'y')]
        yes: bool,
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

#[derive(Subcommand)]
pub enum PluginAction {
    /// Search for plugins in the community registry
    Search {
        /// Search query
        query: String,
    },
    /// Install a plugin from git URL or registry
    Install {
        /// Plugin name, git URL, or local path
        source: String,
    },
    /// Run plugin credential setup
    Setup {
        /// Plugin name
        name: String,
    },
    /// List installed plugins
    List,
    /// Enable a plugin
    Enable {
        /// Plugin name
        name: String,
    },
    /// Disable a plugin
    Disable {
        /// Plugin name
        name: String,
    },
    /// Update plugins
    Update {
        /// Update all plugins
        #[arg(long)]
        all: bool,
        /// Plugin name (if not --all)
        name: Option<String>,
    },
    /// Remove a plugin
    Remove {
        /// Plugin name
        name: String,
    },
    /// Health check for a plugin
    Check {
        /// Plugin name
        name: String,
    },
    /// View plugin logs
    Logs {
        /// Plugin name
        name: String,
        /// Number of lines to show
        #[arg(short = 'n', default_value = "20")]
        lines: usize,
    },
    /// Create a new plugin from template
    Create {
        /// Plugin name (lowercase alphanumeric with hyphens)
        name: String,
    },
    /// Publish plugin to community registry
    Publish,
}
