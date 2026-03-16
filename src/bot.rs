//! Telegram bot handler.

#![allow(clippy::too_many_lines)]

use std::collections::HashSet;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result};
use console::style;
use teloxide::prelude::*;
use teloxide::types::{ChatAction, ChatId, MessageId, ParseMode, ReactionType, ReplyParameters};
use tokio::sync::oneshot;
use tokio::time::{Duration, interval};

use crate::api::{AppState, run_server};
use crate::channel::Channel;
use crate::config::{Config, McpConfig, config_dir};
use crate::llm::Llm;
use crate::mcp_client::{DisconnectReason, McpClient};
use crate::memory::Memory;
use crate::setup::{SETUP_SYSTEM_PROMPT, initial_setup_message};
use crate::telegram::to_telegram_html;
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
            {"command": "mcp", "description": "Show MCP connection details and tools"},
            {"command": "wake", "description": "Wake up Mac via Wake-on-LAN"},
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

    // Initialize channel API server for Claude Code communication
    let channel = Channel::new();

    let api_state = Arc::new(AppState {
        channel: channel.clone(),
        auth_token: config.channel.auth_token.clone(),
    });

    let api_port = config.channel.port;
    tokio::spawn(async move {
        if let Err(e) = run_server(api_state, api_port).await {
            tracing::error!("API server error: {}", e);
        }
    });

    StatusLine::ok(format!("Channel API: port {}", config.channel.port)).print();

    // Ready
    println!();
    println!("  Listening... {}", style("(Ctrl+C to stop)").dim());
    println!();

    // Run bot
    let bot = Bot::new(&config.telegram.bot_token);
    let llm = Llm::from_config_with_memory(&config, memory)?;
    let allowed_users: HashSet<u64> = config.telegram.allowed_users.iter().copied().collect();
    let mcp_config = config.mcp.clone();
    let bot_name = bot_info.name.clone();

    // Spawn SSE event listener if MCP is configured
    if let Some(ref mcp) = mcp_config {
        spawn_sse_listener(mcp, llm.clone(), channel.clone());
    }

    // Track users currently in setup mode
    let setup_users: Arc<Mutex<HashSet<u64>>> = Arc::new(Mutex::new(HashSet::new()));

    Box::pin(teloxide::repl(bot, move |bot: Bot, msg: Message| {
        let llm = llm.clone();
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
                            // Check MCP connectivity first
                            if let Some(mcp) = &mcp_config {
                                let client = McpClient::from_config(mcp);
                                let status = client.get_status().await;
                                if !status.connected {
                                    set_reaction(&bot, msg.chat.id, msg.id, "❌").await;
                                    clear_reactions(&bot, msg.chat.id, msg.id).await;

                                    let (status_msg, hint) = match &status.disconnect_reason {
                                        Some(DisconnectReason::AuthFailed) => (
                                            "Authentication failed",
                                            "Token mismatch between Pi and Mac.\n\
                                            Re-run setup to sync tokens:\n\
                                              lu setup deploy",
                                        ),
                                        Some(DisconnectReason::Unreachable) => (
                                            "Server unreachable",
                                            if status.endpoint.contains("100.") {
                                                "This looks like a Tailscale IP. Check:\n\
                                                • Tailscale running on Mac\n\
                                                • Tailscale running on Pi\n\
                                                • MCP server running on Mac"
                                            } else {
                                                "Check that the MCP server is running:\n\
                                                  launchctl kickstart gui/$(id -u)/dev.ludolph.mcp"
                                            },
                                        ),
                                        _ => (
                                            "Disconnected",
                                            "Check the MCP server on your Mac:\n\
                                              launchctl kickstart gui/$(id -u)/dev.ludolph.mcp",
                                        ),
                                    };

                                    bot.send_message(
                                        msg.chat.id,
                                        format!(
                                            "Setup requires MCP connection.\n\n\
                                            Status: {status_msg}\n\
                                            Endpoint: {}\n\n\
                                            {hint}",
                                            status.endpoint
                                        ),
                                    )
                                    .await?;
                                    return Ok(());
                                }
                            }

                            // Enter setup mode
                            if let Ok(mut guard) = setup_users.lock() {
                                guard.insert(uid);
                            }

                            // Clear previous conversation history for fresh setup
                            #[allow(clippy::cast_possible_wrap)]
                            llm.clear_user_memory(uid as i64);

                            // Show status indicators
                            set_reaction(&bot, msg.chat.id, msg.id, "👀").await;
                            let typing = start_typing(bot.clone(), msg.chat.id);

                            // Start setup conversation
                            #[allow(clippy::cast_possible_wrap)]
                            let result = llm
                                .chat_with_system(
                                    &initial_setup_message(&bot_name),
                                    SETUP_SYSTEM_PROMPT,
                                    Some(uid as i64),
                                )
                                .await;

                            // Stop typing
                            drop(typing);

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
                                    format!("Setup error: {}", format_api_error(&e))
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
                        "/wake" => {
                            if let Some(mcp) = &mcp_config {
                                let client = McpClient::from_config(mcp);

                                // Check if already awake
                                let status = client.get_status().await;
                                if status.connected {
                                    "Mac is already awake and responding.".to_string()
                                } else {
                                    // Try to wake
                                    match client.wake_mac() {
                                        Ok(()) => {
                                            // Wait and verify
                                            tokio::time::sleep(Duration::from_secs(15)).await;
                                            let status = client.get_status().await;
                                            if status.connected {
                                                "Mac is awake and responding.".to_string()
                                            } else {
                                                "Wake-on-LAN sent but Mac not responding yet.\n\
                                                 Try again in a moment, or check:\n\
                                                 • Mac power settings\n\
                                                 • Wake-on-LAN enabled in System Settings"
                                                    .to_string()
                                            }
                                        }
                                        Err(e) => format!("Wake failed: {e}"),
                                    }
                                }
                            } else {
                                "MCP not configured. Run /setup first.".to_string()
                            }
                        }
                        _ => {
                            // Other commands handled by handle_command
                            #[allow(clippy::cast_possible_wrap)]
                            handle_command(text, &bot_name, mcp_config.as_ref(), uid as i64).await
                        }
                    }
                } else if in_setup {
                    // Continue setup conversation
                    set_reaction(&bot, msg.chat.id, msg.id, "👀").await;
                    let typing = start_typing(bot.clone(), msg.chat.id);

                    #[allow(clippy::cast_possible_wrap)]
                    let result = llm
                        .chat_with_system(text, SETUP_SYSTEM_PROMPT, Some(uid as i64))
                        .await;

                    drop(typing);

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
                            format!("Setup error: {}", format_api_error(&e))
                        }
                    }
                } else {
                    // Normal chat with streaming
                    tracing::info!("Processing chat message from user {}: {}", uid, text);
                    set_reaction(&bot, msg.chat.id, msg.id, "👀").await;
                    let typing = start_typing(bot.clone(), msg.chat.id);

                    // Send placeholder message for streaming edits
                    let placeholder_result = bot.send_message(msg.chat.id, "...").await;

                    if let Ok(placeholder) = placeholder_result {
                        let placeholder_id = placeholder.id;
                        let stream_bot = bot.clone();
                        let stream_chat_id = msg.chat.id;
                        let last_edit = Arc::new(Mutex::new(Instant::now()));
                        let edit_counter = Arc::new(Mutex::new(0u32));

                        #[allow(clippy::cast_possible_wrap)]
                        let result = llm
                            .chat_streaming(text, Some(uid as i64), |partial: &str| {
                                // Debounce edits to every 500ms
                                let mut last = last_edit.lock().unwrap();
                                if last.elapsed() >= Duration::from_millis(500) {
                                    // Clone values for the spawned task
                                    let formatted = to_telegram_html(partial);
                                    let text_with_indicator = format!("{formatted}...");
                                    let bot_clone = stream_bot.clone();
                                    let chat_id = stream_chat_id;
                                    let msg_id = placeholder_id;
                                    let counter = edit_counter.clone();

                                    tokio::spawn(async move {
                                        // Increment counter for debugging
                                        if let Ok(mut c) = counter.lock() {
                                            *c += 1;
                                            tracing::trace!("Stream edit #{}", *c);
                                        }
                                        let _ = bot_clone
                                            .edit_message_text(
                                                chat_id,
                                                msg_id,
                                                &text_with_indicator,
                                            )
                                            .parse_mode(ParseMode::Html)
                                            .await;
                                    });

                                    *last = Instant::now();
                                }
                            })
                            .await;

                        tracing::info!("Streaming chat result received for user {}", uid);
                        drop(typing);

                        match result {
                            Ok(response) => {
                                // Final edit without "..." indicator
                                let formatted_final = to_telegram_html(&response);
                                let _ = bot
                                    .edit_message_text(
                                        msg.chat.id,
                                        placeholder_id,
                                        &formatted_final,
                                    )
                                    .parse_mode(ParseMode::Html)
                                    .await;

                                set_reaction(&bot, msg.chat.id, msg.id, "✅").await;
                                clear_reactions(&bot, msg.chat.id, msg.id).await;
                                // Return empty - we already sent via edit
                                String::new()
                            }
                            Err(e) => {
                                // Delete placeholder and send error
                                let _ = bot.delete_message(msg.chat.id, placeholder_id).await;
                                set_reaction(&bot, msg.chat.id, msg.id, "❌").await;
                                clear_reactions(&bot, msg.chat.id, msg.id).await;
                                format_api_error(&e)
                            }
                        }
                    } else {
                        // Fallback to non-streaming if placeholder failed
                        #[allow(clippy::cast_possible_wrap)]
                        let result = llm.chat(text, Some(uid as i64)).await;

                        tracing::info!("Chat result received for user {}", uid);
                        drop(typing);

                        match result {
                            Ok(response) => {
                                set_reaction(&bot, msg.chat.id, msg.id, "✅").await;
                                clear_reactions(&bot, msg.chat.id, msg.id).await;
                                response
                            }
                            Err(e) => {
                                set_reaction(&bot, msg.chat.id, msg.id, "❌").await;
                                clear_reactions(&bot, msg.chat.id, msg.id).await;
                                format_api_error(&e)
                            }
                        }
                    }
                };

                // Send formatted response (skip if empty - streaming already handled it)
                if !response.is_empty() {
                    let formatted = to_telegram_html(&response);
                    let mut message = bot.send_message(msg.chat.id, &formatted);
                    message = message.parse_mode(ParseMode::Html);

                    // If this is a reply to another message, preserve thread context
                    if let Some(reply_msg) = msg.reply_to_message() {
                        message = message.reply_parameters(ReplyParameters::new(reply_msg.id));
                    }

                    let send_result = message.await;

                    // Fallback to plain text if HTML parsing fails
                    if send_result.is_err() {
                        let mut plain_message = bot.send_message(msg.chat.id, response);
                        if let Some(reply_msg) = msg.reply_to_message() {
                            plain_message =
                                plain_message.reply_parameters(ReplyParameters::new(reply_msg.id));
                        }
                        plain_message.await?;
                    }
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

/// Format API errors with user-friendly messages.
///
/// Extracts the root cause from anyhow error chains and provides
/// clear, actionable messages for common API errors.
///
/// Messages distinguish between:
/// - Issues users can fix (rate limits, network)
/// - Issues the Mac admin needs to fix (API key, credits)
fn format_api_error(error: &anyhow::Error) -> String {
    // Get full error chain as string
    let full_error = format!("{error:?}");

    // Check for credit/billing errors (Mac admin needs to fix)
    if full_error.contains("credit balance is too low") || full_error.contains("budget") {
        return "⚠️ API credits exhausted.\n\n\
                The Mac admin needs to add credits at:\n\
                console.anthropic.com/settings/billing\n\n\
                Use /status to check when it's fixed."
            .to_string();
    }

    // Check for rate limit errors (user can wait)
    if full_error.contains("rate_limit") || full_error.contains("Rate limit") {
        return "⏳ Rate limited. Please wait a moment and try again.".to_string();
    }

    // Check for auth errors (Mac admin needs to fix)
    if full_error.contains("authentication")
        || full_error.contains("invalid_api_key")
        || full_error.contains("auth_failed")
        || full_error.contains("API credentials")
    {
        return "🔑 API key is invalid or expired.\n\n\
                The Mac admin needs to update it:\n\
                1. Get a new key from console.anthropic.com/account/keys\n\
                2. Run: ./scripts/install-mcp.sh\n\n\
                Use /status to check when it's fixed."
            .to_string();
    }

    // Check for MCP connection errors (Mac might be asleep/offline)
    if full_error.contains("MCP") && full_error.contains("connection") {
        return "💤 Can't reach the Mac MCP server.\n\n\
                The Mac might be asleep or offline.\n\
                Try sending a message to wake it up."
            .to_string();
    }

    // Check for general network errors
    if full_error.contains("connection") || full_error.contains("timeout") {
        return "🌐 Network error. Check your internet connection.".to_string();
    }

    // Default: show the error chain more clearly
    let mut msg = String::from("Error: ");
    for (i, cause) in error.chain().enumerate() {
        if i > 0 {
            msg.push_str(" → ");
        }
        msg.push_str(&cause.to_string());
    }
    msg
}

/// Handle bot commands (messages starting with /).
async fn handle_command(
    text: &str,
    bot_name: &str,
    mcp_config: Option<&McpConfig>,
    user_id: i64,
) -> String {
    let parts: Vec<&str> = text.split_whitespace().collect();
    let command = parts.first().copied().unwrap_or("");

    match command {
        "/mcp" | "/start" => {
            // Check for subcommands
            let subcommand = parts.get(1).copied();

            match subcommand {
                Some("list") => handle_mcp_list(mcp_config).await,
                Some("add") => {
                    let mcp_name = parts.get(2).copied();
                    handle_mcp_add(mcp_name, mcp_config, user_id).await
                }
                Some("remove") => {
                    let mcp_name = parts.get(2).copied();
                    handle_mcp_remove(mcp_name, mcp_config, user_id).await
                }
                Some("fix") => {
                    // Placeholder for self-healing API key update
                    "🔧 /mcp fix coming soon!\n\n\
                     For now, ask the Mac admin to run:\n\
                     ./scripts/install-mcp.sh"
                        .to_string()
                }
                // No subcommand - show status (existing behavior)
                None | Some(_) => handle_mcp_status(mcp_config).await,
            }
        }
        "/help" => format!(
            "I'm {bot_name}, your vault assistant.\n\n\
            Commands:\n\
            /setup - Configure your assistant (creates Lu.md)\n\
            /wake - Wake up Mac via Wake-on-LAN\n\
            /version - Show version info\n\
            /mcp - Show MCP connection status\n\
            /mcp list - Show available MCPs\n\
            /mcp add <name> - Enable an MCP\n\
            /mcp remove <name> - Disable an MCP\n\
            /cancel - Cancel setup in progress\n\
            /help - Show this message\n\n\
            Or just send me a message and I'll search your vault to help answer it."
        ),
        _ => "I don't recognize that command.\n\nSend /help to see what I can do, or just ask me a question about your vault.".to_string(),
    }
}

/// Handle /mcp (no subcommand) - show MCP connection status.
async fn handle_mcp_status(mcp_config: Option<&McpConfig>) -> String {
    if let Some(mcp) = mcp_config {
        let client = McpClient::from_config(mcp);
        let status = client.get_status().await;

        if status.connected {
            // Check API key health
            let api_status = match client.check_api_health().await {
                Ok(health) if health.api_key_valid => "✅ Valid".to_string(),
                Ok(health) => format!(
                    "❌ Invalid\n   {}",
                    health
                        .fix
                        .unwrap_or_else(|| "Get new key from console.anthropic.com".to_string())
                ),
                Err(_) => "⚠️ Unable to check".to_string(),
            };

            let tools_list = if status.tools.is_empty() {
                String::new()
            } else {
                let mut list = String::from("\n\nAvailable Tools:\n");
                for tool in &status.tools {
                    list.push_str("  - ");
                    list.push_str(&tool.name);
                    list.push('\n');
                }
                list
            };

            let fallback_warning = if status.using_fallback {
                "\n\nNote: Using fallback URL (primary unreachable).\n\
                Check Tailscale or primary network connection."
            } else {
                ""
            };

            format!(
                "MCP Connection\n\n\
                Status: Connected\n\
                Endpoint: {}\n\
                Latency: {}ms\n\
                API Key: {api_status}\
                {tools_list}{fallback_warning}",
                status.endpoint, status.latency_ms
            )
        } else {
            use crate::mcp_client::DisconnectReason;

            let (status_msg, hint) = match &status.disconnect_reason {
                Some(DisconnectReason::AuthFailed) => (
                    "Authentication failed",
                    "Token mismatch between Pi and Mac.\n\
                    Re-run setup to sync tokens:\n\
                      lu setup deploy"
                        .to_string(),
                ),
                Some(DisconnectReason::Unreachable) => {
                    let is_tailscale = status.endpoint.contains("100.");
                    let has_fallback = mcp.fallback_url.is_some();
                    let mut h = if is_tailscale {
                        "This looks like a Tailscale IP. Check:\n\
                        • Tailscale running on Mac\n\
                        • Tailscale running on Pi\n\
                        • MCP server running on Mac"
                            .to_string()
                    } else {
                        "Unable to reach MCP server. Check that the server is running.".to_string()
                    };
                    if !has_fallback && is_tailscale {
                        h.push_str(
                            "\n\nTip: Add fallback_url to config.toml with your\n\
                            LAN IP so Lu works when Tailscale is down.",
                        );
                    }
                    ("Server unreachable", h)
                }
                _ => (
                    "Disconnected",
                    "Check the MCP server on your Mac.".to_string(),
                ),
            };

            format!(
                "MCP Connection\n\n\
                Status: {status_msg}\n\
                Endpoint: {}\n\n\
                {hint}",
                status.endpoint
            )
        }
    } else {
        "MCP Connection\n\n\
        Status: Not configured\n\n\
        No MCP server configured. Using local vault."
            .to_string()
    }
}

/// Handle /mcp list - show available and enabled MCPs.
async fn handle_mcp_list(mcp_config: Option<&McpConfig>) -> String {
    let Some(mcp) = mcp_config else {
        return "MCP not configured.\n\n\
            Configure [mcp] in config.toml to use MCP features."
            .to_string();
    };

    let client = McpClient::from_config(mcp);
    let mcps = client.list_mcps().await;

    if mcps.is_empty() {
        return "No MCPs available in registry.".to_string();
    }

    let mut output = String::from("Available MCPs:\n\n");

    for entry in &mcps {
        let status = if entry.enabled { "[enabled]" } else { "[--]" };
        let _ = writeln!(output, "  {status} {} - {}", entry.name, entry.description);
    }

    output.push_str("\nUse /mcp add <name> to enable an MCP");

    output
}

/// Handle /mcp add <name> - enable an MCP.
async fn handle_mcp_add(
    mcp_name: Option<&str>,
    mcp_config: Option<&McpConfig>,
    user_id: i64,
) -> String {
    let Some(name) = mcp_name else {
        return "Usage: /mcp add <name>\n\n\
            Use /mcp list to see available MCPs."
            .to_string();
    };

    let Some(mcp) = mcp_config else {
        return "MCP not configured.\n\n\
            Configure [mcp] in config.toml to use MCP features."
            .to_string();
    };

    let client = McpClient::from_config(mcp);

    match client.enable_mcp(user_id, name).await {
        Ok(true) => format!(
            "{name} enabled!\n\n\
            To use {name} tools, you may need to set credentials.\n\
            Use /mcp setup {name} to configure."
        ),
        Ok(false) => format!(
            "MCP '{name}' not found in registry.\n\n\
            Use /mcp list to see available MCPs."
        ),
        Err(e) => {
            tracing::error!("Failed to enable MCP '{}': {}", name, e);
            format!("Failed to enable MCP '{name}': {e}")
        }
    }
}

/// Handle /mcp remove <name> - disable an MCP.
async fn handle_mcp_remove(
    mcp_name: Option<&str>,
    mcp_config: Option<&McpConfig>,
    user_id: i64,
) -> String {
    let Some(name) = mcp_name else {
        return "Usage: /mcp remove <name>\n\n\
            Use /mcp list to see enabled MCPs."
            .to_string();
    };

    let Some(mcp) = mcp_config else {
        return "MCP not configured.\n\n\
            Configure [mcp] in config.toml to use MCP features."
            .to_string();
    };

    let client = McpClient::from_config(mcp);

    match client.disable_mcp(user_id, name).await {
        Ok(true) => format!("{name} disabled"),
        Ok(false) => format!(
            "MCP '{name}' not found in registry.\n\n\
            Use /mcp list to see available MCPs."
        ),
        Err(e) => {
            tracing::error!("Failed to disable MCP '{}': {}", name, e);
            format!("Failed to disable MCP '{name}': {e}")
        }
    }
}

/// Spawn background task to listen for SSE events from the MCP server.
///
/// This connects to the MCP event stream and processes channel messages,
/// allowing the bot to respond to messages sent via the Mac's channel system.
fn spawn_sse_listener(mcp_config: &McpConfig, llm: Llm, channel: Channel) {
    let sse_config = crate::sse_client::SseConfig {
        url: mcp_config.url.clone(),
        auth_token: mcp_config.auth_token.clone(),
        subscriber_id: "pi_bot".to_string(),
    };

    let mcp_client = McpClient::from_config(mcp_config);

    tokio::spawn(async move {
        let (tx, mut rx) = tokio::sync::mpsc::channel(100);

        // Spawn SSE connection (runs forever with reconnection)
        let sse_config_clone = sse_config.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::sse_client::connect(sse_config_clone, tx).await {
                tracing::error!("SSE client fatal error: {}", e);
            }
        });

        tracing::info!("SSE event listener started for {}", sse_config.url);

        // Process events as they arrive
        while let Some(event) = rx.recv().await {
            tracing::debug!(
                "Processing event: type={}, id={}",
                event.event_type,
                event.id
            );

            if let Err(e) =
                crate::event_handler::handle_event(event, &llm, &mcp_client, &channel).await
            {
                tracing::error!("Event handler error: {}", e);
                // Continue processing - don't let one error crash the listener
            }
        }

        tracing::warn!("SSE event channel closed - listener shutting down");
    });
}
