mod bot;
mod cli;
mod config;
mod llm;
mod mcp_client;
mod memory;
mod setup;
mod ssh;
mod telegram;
mod tools;
mod ui;

use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};

#[tokio::main]
async fn main() -> ExitCode {
    if std::env::var("RUST_LOG").is_ok() {
        tracing_subscriber::fmt::init();
    }

    let cli = Cli::parse();

    let result = run(cli).await;

    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Some(Command::Check) => Ok(cli::check()),
        Some(Command::Config) => {
            cli::config_cmd()?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Command::Setup { step }) => {
            match step {
                Some(cli::SetupStep::Credentials) => cli::setup_credentials().await?,
                Some(cli::SetupStep::Pi) => cli::setup_pi()?,
                None => cli::setup().await?,
            }
            Ok(ExitCode::SUCCESS)
        }
        Some(Command::Pi) => {
            cli::pi()?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Command::Mcp { action }) => {
            match action {
                cli::McpAction::Update => cli::mcp_update()?,
                cli::McpAction::Version => cli::mcp_version()?,
                cli::McpAction::Restart => cli::mcp_restart()?,
            }
            Ok(ExitCode::SUCCESS)
        }
        None => {
            bot::run().await?;
            Ok(ExitCode::SUCCESS)
        }
    }
}
