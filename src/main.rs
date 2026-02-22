mod bot;
mod claude;
mod cli;
mod config;
mod ssh;
mod tools;
mod ui;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};

#[tokio::main]
async fn main() -> Result<()> {
    // Only enable tracing if RUST_LOG is set
    if std::env::var("RUST_LOG").is_ok() {
        tracing_subscriber::fmt::init();
    }

    let cli = Cli::parse();

    match cli.command {
        Some(Command::Config) => cli::config_cmd()?,
        Some(Command::Setup { step }) => match step {
            Some(cli::SetupStep::Credentials) => cli::setup_credentials().await?,
            Some(cli::SetupStep::Pi) => cli::setup_pi()?,
            None => cli::setup().await?,
        },
        Some(Command::Pi) => cli::pi()?,
        None => bot::run().await?,
    }

    Ok(())
}
