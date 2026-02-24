//! Telegram bot handler.

use std::collections::HashSet;

use anyhow::{Context, Result};
use console::style;
use teloxide::prelude::*;

use crate::claude::Claude;
use crate::config::{Config, McpConfig};
use crate::mcp_client::McpClient;
use crate::ui::StatusLine;

/// Fetch bot info from Telegram API.
async fn get_bot_username(token: &str) -> Result<String> {
    let url = format!("https://api.telegram.org/bot{token}/getMe");
    let response: serde_json::Value = reqwest::get(&url)
        .await
        .context("Failed to connect to Telegram")?
        .json()
        .await
        .context("Failed to parse Telegram response")?;

    if response.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
        anyhow::bail!("Telegram API error");
    }

    response
        .get("result")
        .and_then(|r| r.get("username"))
        .and_then(serde_json::Value::as_str)
        .map(String::from)
        .context("Missing username in response")
}

pub async fn run() -> Result<()> {
    let config = Config::load()?;

    // Fetch bot info first (needed for header)
    let bot_username = get_bot_username(&config.telegram.bot_token).await?;

    // Header
    println!();
    println!("{}", style(&bot_username).bold());
    println!();

    // Validate vault or MCP connection
    if let Some(ref mcp) = config.mcp {
        // Using MCP - vault is on remote Mac
        StatusLine::ok(format!("MCP: {}", mcp.url)).print();
    } else if let Some(ref vault) = config.vault {
        // Using local vault
        if !vault.path.exists() {
            StatusLine::error(format!("Vault not found: {}", vault.path.display())).print();
            anyhow::bail!("Vault directory does not exist");
        }
        StatusLine::ok(format!("Vault: {}", vault.path.display())).print();
    } else {
        StatusLine::error("No vault or MCP configured".to_string()).print();
        anyhow::bail!("Configure either [vault] or [mcp] in config.toml");
    }

    // Telegram validated (already fetched username above)
    StatusLine::ok(format!("Telegram: @{bot_username}")).print();

    // Ready
    println!();
    println!("  Listening... {}", style("(Ctrl+C to stop)").dim());
    println!();

    // Run bot
    let bot = Bot::new(&config.telegram.bot_token);
    let claude = Claude::from_config(&config);
    let allowed_users: HashSet<u64> = config.telegram.allowed_users.iter().copied().collect();
    let mcp_config = config.mcp.clone();

    Box::pin(teloxide::repl(bot, move |bot: Bot, msg: Message| {
        let claude = claude.clone();
        let allowed_users = allowed_users.clone();
        let mcp_config = mcp_config.clone();
        async move {
            // Check if user is authorized
            let user_id = msg.from.as_ref().map(|u| u.id.0);

            if let Some(id) = user_id {
                if !allowed_users.contains(&id) {
                    return Ok(());
                }
            } else {
                return Ok(());
            }

            if let Some(text) = msg.text() {
                let response = if text.starts_with('/') {
                    handle_command(text, mcp_config.as_ref()).await
                } else {
                    claude
                        .chat(text)
                        .await
                        .unwrap_or_else(|e| format!("Error: {e}"))
                };

                bot.send_message(msg.chat.id, response).await?;
            }
            Ok(())
        }
    }))
    .await;

    // Clean exit message
    println!();
    println!("  Stopped.");
    println!();

    Ok(())
}

/// Handle bot commands (messages starting with /).
async fn handle_command(text: &str, mcp_config: Option<&McpConfig>) -> String {
    let command = text.split_whitespace().next().unwrap_or("");

    match command {
        "/poke" => {
            if let Some(mcp) = mcp_config {
                let client = McpClient::from_config(mcp);
                match client.health_check().await {
                    Ok(true) => "MCP connected".to_string(),
                    Ok(false) => "MCP unreachable".to_string(),
                    Err(e) => format!("MCP error: {e}"),
                }
            } else {
                "Local vault mode (no MCP)".to_string()
            }
        }
        "/help" => "Commands:\n/poke - Test MCP connection\n/help - Show this message".to_string(),
        "/start" => "Hello! I'm Ludolph, your vault assistant.\n\nSend /help to see available commands, or just ask me anything about your vault.".to_string(),
        _ => format!("Unknown command: {command}\n\nSend /help for available commands."),
    }
}
