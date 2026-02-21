mod bot;
mod claude;
mod cli;
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
        Some(Command::Config) => cli::config()?,
        Some(Command::Setup) => cli::setup().await?,
        None => bot::run().await?,
    }

    Ok(())
}
