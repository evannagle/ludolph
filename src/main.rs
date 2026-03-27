mod api;
mod bot;
mod channel;
mod cli;
mod config;
mod event_handler;
mod focus;
mod index;
mod llm;
mod mcp_client;
mod memory;
mod scheduler;
mod setup;
mod sse_client;
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
        Some(Command::Setup { full: _, step }) => {
            match step {
                Some(cli::SetupStep::Credentials) => cli::setup_credentials_cmd().await?,
                Some(cli::SetupStep::Pi) => cli::setup_pi_cmd()?,
                Some(cli::SetupStep::Mcp) => cli::setup_mcp_cmd().await?,
                Some(cli::SetupStep::Deploy) => cli::setup_deploy_cmd().await?,
                Some(cli::SetupStep::Verify) => cli::setup_verify_cmd().await?,
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
        Some(Command::Plugin { action }) => {
            match action {
                cli::PluginAction::Search { query } => cli::plugin_search(&query).await?,
                cli::PluginAction::Install { source } => cli::plugin_install(&source).await?,
                cli::PluginAction::Setup { name } => cli::plugin_setup(&name).await?,
                cli::PluginAction::List => cli::plugin_list().await?,
                cli::PluginAction::Enable { name } => cli::plugin_enable(&name).await?,
                cli::PluginAction::Disable { name } => cli::plugin_disable(&name).await?,
                cli::PluginAction::Update { all, name } => {
                    cli::plugin_update(all, name.as_deref()).await?;
                }
                cli::PluginAction::Remove { name } => cli::plugin_remove(&name).await?,
                cli::PluginAction::Check { name } => cli::plugin_check(&name).await?,
                cli::PluginAction::Logs { name, lines } => cli::plugin_logs(&name, lines).await?,
                cli::PluginAction::Create { name } => cli::plugin_create(&name).await?,
                cli::PluginAction::Publish => cli::plugin_publish().await?,
            }
            Ok(ExitCode::SUCCESS)
        }
        Some(Command::Doctor { fix }) => Ok(cli::doctor(fix).await),
        Some(Command::Uninstall { mac, pi, all, yes }) => {
            cli::uninstall(mac, pi, all, yes)?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Command::Update) => {
            cli::update().await?;
            Ok(ExitCode::SUCCESS)
        }
        None => {
            bot::run().await?;
            Ok(ExitCode::SUCCESS)
        }
    }
}
