mod bot;
mod claude;
mod cli;
mod config;
mod preflight;
mod ssh;
mod syncthing;
mod tools;
mod ui;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Some(Command::Status) => cli::status().await?,
        Some(Command::Logs) => cli::logs()?,
        Some(Command::Restart) => cli::restart().await?,
        Some(Command::Update) => cli::update().await?,
        Some(Command::Uninstall) => cli::uninstall().await?,
        Some(Command::Config) => cli::config_cmd()?,
        Some(Command::Setup { step }) => match step {
            Some(cli::SetupStep::Credentials) => cli::setup_credentials()?,
            Some(cli::SetupStep::Pi) => cli::setup_pi()?,
            Some(cli::SetupStep::Sync) => cli::sync_setup()?,
            None => cli::setup().await?,
        },
        Some(Command::Pi) => cli::pi()?,
        Some(Command::Sync { command }) => match command {
            Some(cli::SyncCommand::Setup) => cli::sync_setup()?,
            Some(cli::SyncCommand::Status) | None => cli::sync_status()?,
        },
        None => bot::run().await?,
    }

    Ok(())
}
