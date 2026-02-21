mod cli;
mod bot;
mod claude;
mod tools;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Some(Command::Status) => cli::status().await?,
        Some(Command::Logs) => cli::logs().await?,
        Some(Command::Restart) => cli::restart().await?,
        Some(Command::Update) => cli::update().await?,
        Some(Command::Uninstall) => cli::uninstall().await?,
        Some(Command::Config) => cli::config().await?,
        None => bot::run().await?,
    }

    Ok(())
}
