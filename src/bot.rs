//! Telegram bot handler.

#![allow(clippy::too_many_lines)]

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use console::style;
use teloxide::prelude::*;
use teloxide::types::{ChatAction, ChatId, MessageId, ParseMode, ReactionType};
use tokio::sync::oneshot;
use tokio::time::{Duration, interval};

use crate::claude::Claude;
use crate::config::{Config, McpConfig, config_dir};
use crate::mcp_client::McpClient;
use crate::memory::Memory;
use crate::setup::{SETUP_SYSTEM_PROMPT, initial_setup_message};
use crate::telegram::{thinking_message, to_telegram_html};
use crate::ui::StatusLine;

/// Set a reaction emoji on a message.
async fn set_reaction(bot: &Bot, chat_id: ChatId, message_id: MessageId, emoji: &str) {
    let _ = bot
        .set_message_reaction(chat_id, message_id)
        .reaction(vec![ReactionType::Emoji {
            emoji: emoji.to_string(),
        }])
        .await;
}

/// Clear all reactions from a message.
async fn clear_reactions(bot: &Bot, chat_id: ChatId, message_id: MessageId) {
    let _ = bot
        .set_message_reaction(chat_id, message_id)
        .reaction(vec![])
        .await;
}

/// Start showing typing indicator, refreshing every 5 seconds.
///
/// Returns a channel sender that stops the typing when dropped.
fn start_typing(bot: Bot, chat_id: ChatId) -> oneshot::Sender<()> {
    let (tx, mut rx) = oneshot::channel();

    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(5));

        loop {
            // Send typing action
            let _ = bot.send_chat_action(chat_id, ChatAction::Typing).await;

            // Wait for either 5 seconds or cancellation signal
            tokio::select! {
                _ = ticker.tick() => {
                    // Continue loop - send typing again in 5s
                }
                _ = &mut rx => {
                    // Cancellation received - stop typing
                    break;
                }
            }
        }
    });

    tx
}

/// Register bot commands with Telegram for autocomplete.
async fn register_commands(token: &str) -> Result<()> {
    let commands = serde_json::json!({
        "commands": [
            {"command": "setup", "description": "Configure your vault assistant"},
            {"command": "version", "description": "Show version info"},
            {"command": "poke", "description": "Show connection status and available tools"},
            {"command": "help", "description": "Show available commands"},
            {"command": "cancel", "description": "Cancel setup in progress"},
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

    // Track users currently in setup mode
    let setup_users: Arc<Mutex<HashSet<u64>>> = Arc::new(Mutex::new(HashSet::new()));

    Box::pin(teloxide::repl(bot, move |bot: Bot, msg: Message| {
        let claude = claude.clone();
        let allowed_users = allowed_users.clone();
        let mcp_config = mcp_config.clone();
        let bot_name = bot_name.clone();
        let setup_users = setup_users.clone();
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

            // Safe to unwrap - we verified user_id exists above
            let uid = user_id.unwrap();

            if let Some(text) = msg.text() {
                // Check if user is in setup mode
                let in_setup = setup_users
                    .lock()
                    .map(|guard| guard.contains(&uid))
                    .unwrap_or(false);

                let response = if text.starts_with('/') {
                    // Handle commands
                    match text.split_whitespace().next().unwrap_or("") {
                        "/setup" => {
                            // Enter setup mode
                            if let Ok(mut guard) = setup_users.lock() {
                                guard.insert(uid);
                            }

                            // Show thinking indicator for setup
                            set_reaction(&bot, msg.chat.id, msg.id, "👀").await;
                            let thinking =
                                bot.send_message(msg.chat.id, thinking_message()).await.ok();
                            let typing = start_typing(bot.clone(), msg.chat.id);

                            // Start setup conversation
                            #[allow(clippy::cast_possible_wrap)]
                            let result = claude
                                .chat_with_system(
                                    &initial_setup_message(&bot_name),
                                    SETUP_SYSTEM_PROMPT,
                                    Some(uid as i64),
                                )
                                .await;

                            // Cleanup indicators
                            drop(typing);
                            if let Some(thinking_msg) = thinking {
                                let _ = bot.delete_message(msg.chat.id, thinking_msg.id).await;
                            }

                            match result {
                                Ok(chat_result) => {
                                    if chat_result.setup_completed {
                                        if let Ok(mut guard) = setup_users.lock() {
                                            guard.remove(&uid);
                                        }
                                    }
                                    set_reaction(&bot, msg.chat.id, msg.id, "✅").await;
                                    clear_reactions(&bot, msg.chat.id, msg.id).await;
                                    chat_result.response
                                }
                                Err(e) => {
                                    // Log full error chain
                                    tracing::error!("Setup failed: {e:?}");

                                    // Exit setup mode on error
                                    if let Ok(mut guard) = setup_users.lock() {
                                        guard.remove(&uid);
                                    }
                                    set_reaction(&bot, msg.chat.id, msg.id, "❌").await;
                                    clear_reactions(&bot, msg.chat.id, msg.id).await;
                                    format!("Setup error: {e}")
                                }
                            }
                        }
                        "/cancel" => {
                            if in_setup {
                                if let Ok(mut guard) = setup_users.lock() {
                                    guard.remove(&uid);
                                }
                                "Setup cancelled.".to_string()
                            } else {
                                "Nothing to cancel.".to_string()
                            }
                        }
                        "/version" => {
                            format!("Ludolph v{VERSION}")
                        }
                        _ => {
                            // Other commands handled by handle_command
                            handle_command(text, &bot_name, mcp_config.as_ref()).await
                        }
                    }
                } else if in_setup {
                    // Continue setup conversation
                    set_reaction(&bot, msg.chat.id, msg.id, "👀").await;
                    let thinking = bot.send_message(msg.chat.id, thinking_message()).await.ok();
                    let typing = start_typing(bot.clone(), msg.chat.id);

                    #[allow(clippy::cast_possible_wrap)]
                    let result = claude
                        .chat_with_system(text, SETUP_SYSTEM_PROMPT, Some(uid as i64))
                        .await;

                    drop(typing);
                    if let Some(thinking_msg) = thinking {
                        let _ = bot.delete_message(msg.chat.id, thinking_msg.id).await;
                    }

                    match result {
                        Ok(chat_result) => {
                            if chat_result.setup_completed {
                                if let Ok(mut guard) = setup_users.lock() {
                                    guard.remove(&uid);
                                }
                            }
                            set_reaction(&bot, msg.chat.id, msg.id, "✅").await;
                            clear_reactions(&bot, msg.chat.id, msg.id).await;
                            chat_result.response
                        }
                        Err(e) => {
                            // Log full error chain
                            tracing::error!("Setup failed: {e:?}");

                            if let Ok(mut guard) = setup_users.lock() {
                                guard.remove(&uid);
                            }
                            set_reaction(&bot, msg.chat.id, msg.id, "❌").await;
                            clear_reactions(&bot, msg.chat.id, msg.id).await;
                            format!("Setup error: {e}")
                        }
                    }
                } else {
                    // Normal chat
                    set_reaction(&bot, msg.chat.id, msg.id, "👀").await;
                    let thinking = bot.send_message(msg.chat.id, thinking_message()).await.ok();
                    let typing = start_typing(bot.clone(), msg.chat.id);

                    #[allow(clippy::cast_possible_wrap)]
                    let result = claude.chat(text, Some(uid as i64)).await;

                    drop(typing);
                    if let Some(thinking_msg) = thinking {
                        let _ = bot.delete_message(msg.chat.id, thinking_msg.id).await;
                    }

                    match result {
                        Ok(response) => {
                            set_reaction(&bot, msg.chat.id, msg.id, "✅").await;
                            clear_reactions(&bot, msg.chat.id, msg.id).await;
                            response
                        }
                        Err(e) => {
                            set_reaction(&bot, msg.chat.id, msg.id, "❌").await;
                            clear_reactions(&bot, msg.chat.id, msg.id).await;
                            format!("Error: {e}")
                        }
                    }
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
            let version = env!("CARGO_PKG_VERSION");

            if let Some(mcp) = mcp_config {
                let client = McpClient::from_config(mcp);

                // Check connection
                let connection_status = match client.health_check().await {
                    Ok(true) => "✓ Connected to vault",
                    Ok(false) => "⚠ Having trouble reaching vault",
                    Err(_) => "✗ Unable to connect to vault",
                };

                // Get available tools
                let tools_list = client
                    .get_tool_definitions()
                    .await
                    .map_or_else(
                        |_| String::new(),
                        |tools| {
                            let mut list = String::from("\n\n**Available Tools:**\n");
                            for tool in tools {
                                list.push_str("• ");
                                list.push_str(&tool.name);
                                list.push_str(" - ");
                                list.push_str(&tool.description);
                                list.push('\n');
                            }
                            list
                        },
                    );

                format!(
                    "{bot_name} v{version}\n{connection_status}{tools_list}"
                )
            } else {
                format!(
                    "{bot_name} v{version}\n✓ Connected to local vault"
                )
            }
        }
        "/help" => format!(
            "I'm {bot_name}, your vault assistant.\n\n\
            Commands:\n\
            /setup - Configure your assistant (creates Lu.md)\n\
            /version - Show version info\n\
            /poke - Show connection status and available tools\n\
            /cancel - Cancel setup in progress\n\
            /help - Show this message\n\n\
            Or just send me a message and I'll search your vault to help answer it."
        ),
        _ => "I don't recognize that command.\n\nSend /help to see what I can do, or just ask me a question about your vault.".to_string(),
    }
}
