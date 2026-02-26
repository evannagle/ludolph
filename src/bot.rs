//! Telegram bot handler.

#![allow(clippy::too_many_lines)]

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{Context, Result};
use console::style;
use teloxide::prelude::*;
use teloxide::types::{ParseMode, ReactionType};

use crate::claude::Claude;
use crate::config::{Config, McpConfig, config_dir};
use crate::mcp_client::McpClient;
use crate::memory::Memory;
use crate::telegram::{thinking_message, to_telegram_html};
use crate::ui::StatusLine;

/// Register bot commands with Telegram for autocomplete.
async fn register_commands(token: &str) -> Result<()> {
    let commands = serde_json::json!({
        "commands": [
            {"command": "poke", "description": "Test MCP connection to your vault"},
            {"command": "help", "description": "Show available commands"},
        ]
    });

    let url = format!("https://api.telegram.org/bot{token}/setMyCommands");
    let client = reqwest::Client::new();

    let response = client
        .post(&url)
        .json(&commands)
        .send()
        .await
        .context("Failed to register commands")?;

    if !response.status().is_success() {
        let body = response.text().await.unwrap_or_default();
        tracing::warn!("Failed to register commands: {}", body);
    }

    Ok(())
}

/// Bot identity from Telegram API.
struct BotInfo {
    /// Bot's display name (e.g., "Lu")
    name: String,
    /// Bot's username (e.g., `LudolphPiBot`)
    username: String,
}

/// Fetch bot info from Telegram API.
async fn get_bot_info(token: &str) -> Result<BotInfo> {
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

    let result = response
        .get("result")
        .context("Missing result in response")?;

    let username = result
        .get("username")
        .and_then(serde_json::Value::as_str)
        .map(String::from)
        .context("Missing username in response")?;

    // Use first_name as the friendly name, fallback to username
    let name = result
        .get("first_name")
        .and_then(serde_json::Value::as_str)
        .map_or_else(|| username.clone(), String::from);

    Ok(BotInfo { name, username })
}

/// Version from Cargo.toml
const VERSION: &str = env!("CARGO_PKG_VERSION");

pub async fn run() -> Result<()> {
    let config = Config::load()?;

    // Fetch bot info first (needed for header)
    let bot_info = get_bot_info(&config.telegram.bot_token).await?;

    // Register bot commands for autocomplete
    register_commands(&config.telegram.bot_token).await?;

    // Header with version
    println!();
    println!(
        "{} (@{}) {}",
        style(&bot_info.name).bold(),
        style(&bot_info.username).dim(),
        style(format!("v{VERSION}")).dim()
    );
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
    StatusLine::ok(format!("Telegram: @{}", bot_info.username)).print();

    // Initialize memory
    let memory = match Memory::open(&config_dir().join("conversations.db"), &config.memory) {
        Ok(mem) => {
            let (window, threshold, max_bytes) = mem.config();
            StatusLine::ok(format!(
                "Memory: window={window}, persist={threshold}, max={}KB",
                max_bytes / 1024
            ))
            .print();
            Some(Arc::new(mem))
        }
        Err(e) => {
            StatusLine::error(format!("Memory disabled: {e}")).print();
            None
        }
    };

    // Ready
    println!();
    println!("  Listening... {}", style("(Ctrl+C to stop)").dim());
    println!();

    // Run bot
    let bot = Bot::new(&config.telegram.bot_token);
    let claude = Claude::from_config_with_memory(&config, memory);
    let allowed_users: HashSet<u64> = config.telegram.allowed_users.iter().copied().collect();
    let mcp_config = config.mcp.clone();
    let bot_name = bot_info.name.clone();

    Box::pin(teloxide::repl(bot, move |bot: Bot, msg: Message| {
        let claude = claude.clone();
        let allowed_users = allowed_users.clone();
        let mcp_config = mcp_config.clone();
        let bot_name = bot_name.clone();
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
                    // Commands are fast, no thinking indicator needed
                    handle_command(text, &bot_name, mcp_config.as_ref()).await
                } else {
                    // Add eyes reaction to show we're working on it
                    let _ = bot
                        .set_message_reaction(msg.chat.id, msg.id)
                        .reaction(vec![ReactionType::Emoji {
                            emoji: "👀".to_string(),
                        }])
                        .await;

                    // Send thinking message
                    let thinking = bot.send_message(msg.chat.id, thinking_message()).await.ok();

                    // Show typing indicator while processing
                    let _ = bot
                        .send_chat_action(msg.chat.id, teloxide::types::ChatAction::Typing)
                        .await;

                    // Get response from Claude (pass user_id for memory)
                    // Safe: Telegram user IDs fit in i64
                    #[allow(clippy::cast_possible_wrap)]
                    let uid = user_id.map(|id| id as i64);
                    let response = claude
                        .chat(text, uid)
                        .await
                        .unwrap_or_else(|e| format!("Error: {e}"));

                    // Delete thinking message
                    if let Some(thinking_msg) = thinking {
                        let _ = bot.delete_message(msg.chat.id, thinking_msg.id).await;
                    }

                    // Remove eyes reaction
                    let _ = bot
                        .set_message_reaction(msg.chat.id, msg.id)
                        .reaction(vec![])
                        .await;

                    response
                };

                // Send formatted response
                let formatted = to_telegram_html(&response);
                let send_result = bot
                    .send_message(msg.chat.id, &formatted)
                    .parse_mode(ParseMode::Html)
                    .await;

                // Fallback to plain text if HTML parsing fails
                if send_result.is_err() {
                    bot.send_message(msg.chat.id, response).await?;
                }
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
async fn handle_command(text: &str, bot_name: &str, mcp_config: Option<&McpConfig>) -> String {
    let command = text.split_whitespace().next().unwrap_or("");

    match command {
        "/poke" | "/start" => {
            let connection_status = if let Some(mcp) = mcp_config {
                let client = McpClient::from_config(mcp);
                match client.health_check().await {
                    Ok(true) => "connected to your vault",
                    Ok(false) => "having trouble reaching your vault",
                    Err(_) => "unable to connect to your vault",
                }
            } else {
                "connected to your local vault"
            };

            format!(
                "Hi! I'm {bot_name}, and I'm {connection_status}.\n\n\
                I can help you:\n\
                - Search through your notes\n\
                - Read and summarize files\n\
                - Find information across your vault\n\
                - Answer questions about your notes\n\n\
                Just ask me anything, or try: \"What did I write about last week?\""
            )
        }
        "/help" => format!(
            "I'm {bot_name}, your vault assistant.\n\n\
            Commands:\n\
            /poke - Check vault connection\n\
            /help - Show this message\n\n\
            Or just send me a message and I'll search your vault to help answer it."
        ),
        _ => "I don't recognize that command.\n\nSend /help to see what I can do, or just ask me a question about your vault.".to_string(),
    }
}
