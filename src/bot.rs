//! Telegram bot handler.

#![allow(clippy::too_many_lines)]

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use tokio::sync::Mutex as AsyncMutex;
use tokio_util::sync::CancellationToken;

use anyhow::{Context, Result};
use console::style;
use teloxide::prelude::*;
use teloxide::types::{ChatAction, ChatId, MessageId, ParseMode, ReactionType, ReplyParameters};
use tokio::sync::oneshot;
use tokio::time::{Duration, interval};

use crate::api::{AppState, run_server};
use crate::channel::Channel;
use crate::config::{Config, McpConfig, config_dir};
use crate::focus::Focus;
use crate::llm::Llm;
use crate::mcp_client::{DisconnectReason, McpClient};
use crate::memory::Memory;
use crate::scheduler::Scheduler;
use crate::setup::{SETUP_SYSTEM_PROMPT, initial_setup_message};
use crate::telegram::to_telegram_html;
use crate::ui::StatusLine;

/// Tracks a user's pending messages and current processing state.
///
/// Used to consolidate rapid successive messages into a single LLM request
/// and enable cancellation of in-flight requests.
#[derive(Default)]
struct UserConversation {
    /// Messages waiting to be processed (or added mid-processing)
    pending: VecDeque<String>,
    /// Token to cancel current LLM request
    cancel_token: Option<CancellationToken>,
    /// Whether we're currently processing for this user
    processing: bool,
    /// Placeholder message ID for streaming edits
    placeholder_id: Option<MessageId>,
    /// Chat ID for this user (needed for cleanup)
    chat_id: Option<ChatId>,
}

/// Global state: `user_id` -> conversation state.
///
/// Uses `tokio::sync::Mutex` (not `std::sync::Mutex`) because we need to hold
/// the lock across await points in some operations.
type ConversationState = Arc<AsyncMutex<HashMap<u64, UserConversation>>>;

/// Queue a message for a user and determine if processing should start.
///
/// Returns true if a new processing task should be spawned.
#[allow(clippy::significant_drop_tightening)]
async fn queue_message_for_user(
    state: &ConversationState,
    user_id: u64,
    text: &str,
    chat_id: ChatId,
) -> bool {
    let mut guard = state.lock().await;
    let conv = guard.entry(user_id).or_default();

    // Store chat_id for later use
    conv.chat_id = Some(chat_id);

    // Add message to pending queue
    conv.pending.push_back(text.to_string());

    // If already processing, message will be picked up by poller
    if conv.processing {
        tracing::debug!("User {} already processing, message queued", user_id);
        false
    } else {
        // Start processing
        conv.processing = true;
        conv.cancel_token = Some(CancellationToken::new());
        true
    }
}

/// Consolidate multiple messages into a single prompt.
///
/// If there's only one message, returns it unchanged.
/// Multiple messages are formatted with explicit numbering so the LLM
/// understands they were sent in sequence.
fn consolidate_messages(messages: &[String]) -> String {
    match messages.len() {
        0 => String::new(),
        1 => messages[0].clone(),
        _ => {
            let mut prompt = String::from("[Multiple messages from user]\n\n");
            for (i, msg) in messages.iter().enumerate() {
                let _ = writeln!(prompt, "Message {}: {}", i + 1, msg);
            }
            prompt
                .push_str("\n---\n\nPlease respond to all of the above as a single conversation.");
            prompt
        }
    }
}

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
            {"command": "cancel", "description": "Cancel current operation"},
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

/// Process messages for a user with debouncing and cancellation support.
///
/// This function runs in a loop:
/// 1. Collects all pending messages
/// 2. Consolidates into single prompt
/// 3. Sends to LLM with cancellation support
/// 4. If new messages arrive during processing, restarts
/// 5. When complete (or cancelled), cleans up state
#[allow(clippy::cognitive_complexity)]
#[allow(clippy::significant_drop_tightening)]
async fn process_user_conversation(
    user_id: u64,
    state: ConversationState,
    bot: Bot,
    llm: Llm,
    chat_id: ChatId,
) {
    loop {
        // Step 1: Get pending messages (short lock)
        let (messages, cancel_token, needs_placeholder) = {
            let mut guard = state.lock().await;
            let Some(conv) = guard.get_mut(&user_id) else {
                return; // User state was cleared
            };

            if conv.pending.is_empty() {
                // Nothing to process, we're done
                conv.processing = false;
                conv.cancel_token = None;
                return;
            }

            // Take all pending messages
            let msgs: Vec<String> = conv.pending.drain(..).collect();
            let token = conv
                .cancel_token
                .clone()
                .unwrap_or_else(CancellationToken::new);
            let needs_ph = conv.placeholder_id.is_none();

            (msgs, token, needs_ph)
        }; // Lock dropped here

        // Step 2: Create placeholder OUTSIDE the lock (involves await)
        let placeholder_id = if needs_placeholder {
            let msg = bot.send_message(chat_id, "...").await.ok();
            let msg_id = msg.as_ref().map(|m| m.id);

            // Store placeholder ID (short lock)
            if let Some(id) = msg_id {
                let mut guard = state.lock().await;
                if let Some(conv) = guard.get_mut(&user_id) {
                    conv.placeholder_id = Some(id);
                }
            }
            msg_id
        } else {
            let guard = state.lock().await;
            guard.get(&user_id).and_then(|c| c.placeholder_id)
        };

        // Step 3: Consolidate messages
        let prompt = consolidate_messages(&messages);
        tracing::info!(
            "Processing {} message(s) for user {}: {:?}",
            messages.len(),
            user_id,
            if messages.len() == 1 {
                messages[0].chars().take(50).collect::<String>()
            } else {
                format!("{} messages", messages.len())
            }
        );

        // Step 4: Create closure to check for new messages
        let state_clone = state.clone();
        let check_new_messages = move || {
            // Use try_lock to avoid blocking - if we can't get lock, assume no new messages
            state_clone
                .try_lock()
                .is_ok_and(|guard| guard.get(&user_id).is_some_and(|c| !c.pending.is_empty()))
        };

        // Step 5: Process with cancellation
        #[allow(clippy::cast_possible_wrap)]
        let result = llm
            .chat_cancellable(
                &prompt,
                Some(user_id as i64),
                cancel_token.clone(),
                check_new_messages,
                |response| {
                    // Final response callback - update placeholder
                    if let Some(msg_id) = placeholder_id {
                        let formatted = to_telegram_html(response);
                        let bot_clone = bot.clone();

                        tokio::spawn(async move {
                            let _ = bot_clone
                                .edit_message_text(chat_id, msg_id, &formatted)
                                .parse_mode(ParseMode::Html)
                                .await;
                        });
                    }
                },
            )
            .await;

        match result {
            Ok(Some(response)) => {
                // Check if new messages arrived during processing
                let has_new = {
                    let guard = state.lock().await;
                    guard.get(&user_id).is_some_and(|c| !c.pending.is_empty())
                };

                if has_new {
                    // New messages arrived, restart with consolidated
                    tracing::info!("New messages arrived for user {}, restarting", user_id);
                    continue;
                }

                // Send final response
                if let Some(msg_id) = placeholder_id {
                    let formatted_final = to_telegram_html(&response);
                    let _ = bot
                        .edit_message_text(chat_id, msg_id, &formatted_final)
                        .parse_mode(ParseMode::Html)
                        .await;
                }

                // Clear state and exit
                {
                    let mut guard = state.lock().await;
                    if let Some(conv) = guard.get_mut(&user_id) {
                        conv.processing = false;
                        conv.cancel_token = None;
                        conv.placeholder_id = None;
                    }
                }

                // Mark success
                // Note: We can't access msg.id here, so skip reaction update
                tracing::info!("Chat complete for user {}", user_id);
                return;
            }
            Ok(None) => {
                // Cancelled or new messages detected during polling
                let has_new = {
                    let guard = state.lock().await;
                    guard.get(&user_id).is_some_and(|c| !c.pending.is_empty())
                };

                if has_new {
                    // Restart with new messages
                    tracing::info!("Restarting for user {} due to new messages", user_id);
                    continue;
                }

                // User cancelled, cleanup
                tracing::info!("Chat cancelled for user {}", user_id);
                if let Some(msg_id) = placeholder_id {
                    let _ = bot.delete_message(chat_id, msg_id).await;
                }

                {
                    let mut guard = state.lock().await;
                    if let Some(conv) = guard.get_mut(&user_id) {
                        conv.processing = false;
                        conv.cancel_token = None;
                        conv.placeholder_id = None;
                    }
                }
                return;
            }
            Err(e) => {
                // Error - report and cleanup
                tracing::error!("Chat error for user {}: {}", user_id, e);
                if let Some(msg_id) = placeholder_id {
                    let error_msg = format_api_error(&e);
                    let _ = bot.edit_message_text(chat_id, msg_id, &error_msg).await;
                }

                {
                    let mut guard = state.lock().await;
                    if let Some(conv) = guard.get_mut(&user_id) {
                        conv.processing = false;
                        conv.cancel_token = None;
                        conv.placeholder_id = None;
                    }
                }
                return;
            }
        }
    }
}

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

    // Initialize focus (file tracking)
    let focus = match Focus::open(&config_dir().join("focus.db"), &config.focus) {
        Ok(foc) => {
            let (max_files, max_age, preview_chars) = foc.config();
            StatusLine::ok(format!(
                "Focus: max={max_files}, age={}m, preview={preview_chars}",
                max_age / 60
            ))
            .print();
            Some(Arc::new(foc))
        }
        Err(e) => {
            StatusLine::error(format!("Focus disabled: {e}")).print();
            None
        }
    };

    // Initialize scheduler (automated tasks)
    let scheduler = match Scheduler::open(&config_dir().join("schedules.db"), &config.scheduler) {
        Ok(sched) => {
            StatusLine::ok(format!(
                "Scheduler: interval={}s",
                config.scheduler.check_interval_secs
            ))
            .print();
            Some(Arc::new(sched))
        }
        Err(e) => {
            StatusLine::error(format!("Scheduler disabled: {e}")).print();
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
    let llm = Llm::from_config_full(&config, memory, focus, scheduler.clone())?;
    let allowed_users: HashSet<u64> = config.telegram.allowed_users.iter().copied().collect();
    let mcp_config = config.mcp.clone();
    let bot_name = bot_info.name.clone();

    // Spawn SSE event listener if MCP is configured
    if let Some(ref mcp) = mcp_config {
        spawn_sse_listener(mcp, llm.clone(), channel.clone());
    }

    // Spawn background scheduler task if scheduler is available
    if let Some(ref sched) = scheduler {
        spawn_scheduler_task(
            sched.clone(),
            llm.clone(),
            bot.clone(),
            mcp_config.clone(),
            config.scheduler.check_interval_secs,
        );
    }

    // Track users currently in setup mode
    let setup_users: Arc<Mutex<HashSet<u64>>> = Arc::new(Mutex::new(HashSet::new()));

    // Track per-user conversation state for message debouncing
    let conversation_state: ConversationState = Arc::new(AsyncMutex::new(HashMap::new()));

    Box::pin(teloxide::repl(bot, move |bot: Bot, msg: Message| {
        let llm = llm.clone();
        let allowed_users = allowed_users.clone();
        let mcp_config = mcp_config.clone();
        let bot_name = bot_name.clone();
        let setup_users = setup_users.clone();
        let conversation_state = conversation_state.clone();
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
                            // Handle setup cancellation
                            if in_setup {
                                if let Ok(mut guard) = setup_users.lock() {
                                    guard.remove(&uid);
                                }
                                "Setup cancelled.".to_string()
                            } else {
                                // Handle chat cancellation
                                let result = {
                                    let mut guard = conversation_state.lock().await;
                                    if let Some(conv) = guard.get_mut(&uid) {
                                        if conv.processing {
                                            // Cancel current request
                                            if let Some(token) = &conv.cancel_token {
                                                token.cancel();
                                            }
                                            // Clear pending messages
                                            conv.pending.clear();
                                            // Return placeholder to delete
                                            conv.placeholder_id.take()
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                }; // Lock dropped here before await

                                if let Some(msg_id) = result {
                                    let _ = bot.delete_message(msg.chat.id, msg_id).await;
                                    "Cancelled.".to_string()
                                } else {
                                    "Nothing to cancel.".to_string()
                                }
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
                    // Normal chat with debouncing
                    tracing::info!("Received chat message from user {}: {}", uid, text);

                    // Add message to queue and potentially start processing
                    let should_spawn =
                        queue_message_for_user(&conversation_state, uid, text, msg.chat.id).await;

                    if should_spawn {
                        // Show processing indicators
                        set_reaction(&bot, msg.chat.id, msg.id, "👀").await;

                        // Spawn processing task
                        let state_clone = conversation_state.clone();
                        let bot_clone = bot.clone();
                        let llm_clone = llm.clone();
                        let chat_id = msg.chat.id;
                        let msg_id = msg.id;

                        tokio::spawn(async move {
                            process_user_conversation(
                                uid,
                                state_clone.clone(),
                                bot_clone.clone(),
                                llm_clone,
                                chat_id,
                            )
                            .await;

                            // Update reaction on completion
                            set_reaction(&bot_clone, chat_id, msg_id, "✅").await;
                            clear_reactions(&bot_clone, chat_id, msg_id).await;
                        });
                    } else {
                        // Message queued, show pending indicator
                        set_reaction(&bot, msg.chat.id, msg.id, "⏳").await;
                    }

                    // Return empty - response will be sent by processing task
                    String::new()
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

/// Spawn background task to check and execute scheduled tasks.
///
/// Checks for due schedules every `check_interval_secs` seconds,
/// executes them via the LLM, and sends notifications.
fn spawn_scheduler_task(
    scheduler: Arc<Scheduler>,
    llm: Llm,
    bot: Bot,
    mcp_config: Option<McpConfig>,
    check_interval_secs: u64,
) {
    use crate::scheduler::RunStatus;
    use teloxide::types::ChatId;

    tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(check_interval_secs));
        tracing::info!(
            "Scheduler task started, checking every {}s",
            check_interval_secs
        );

        loop {
            interval.tick().await;

            let due = match scheduler.get_due_schedules() {
                Ok(list) => list,
                Err(e) => {
                    tracing::error!("Failed to get due schedules: {e}");
                    continue;
                }
            };

            if due.is_empty() {
                continue;
            }

            tracing::info!("Found {} due schedule(s) to execute", due.len());

            for schedule in due {
                let user_id = schedule.user_id;
                let schedule_id = schedule.id.clone();
                let schedule_name = schedule.name.clone();

                tracing::info!(
                    "Executing schedule '{}' (ID: {}) for user {}",
                    schedule_name,
                    schedule_id,
                    user_id
                );

                // Record run start
                let run_id = match scheduler.record_run_start(&schedule_id, user_id) {
                    Ok(id) => id,
                    Err(e) => {
                        tracing::error!("Failed to record run start: {e}");
                        continue;
                    }
                };

                // Wake Mac if needed (using MCP client)
                if let Some(ref mcp) = mcp_config {
                    let client = McpClient::from_config(mcp);
                    let status = client.get_status().await;
                    if !status.connected {
                        if let Some(ref _mac_addr) = mcp.mac_address {
                            if let Err(e) = client.wake_mac() {
                                tracing::warn!(
                                    "Failed to wake Mac for schedule '{}': {}",
                                    schedule_name,
                                    e
                                );
                                // Wait a bit and check again
                                tokio::time::sleep(Duration::from_secs(30)).await;
                            }
                        }
                    }
                }

                // Send pre-notification if enabled
                if schedule.notify_before {
                    let msg = format!("Starting scheduled task: {schedule_name}");
                    let _ = bot.send_message(ChatId(user_id), &msg).await;
                }

                // Execute the schedule's prompt via LLM
                let result = llm.chat(&schedule.prompt, Some(user_id)).await;

                // Record run completion and send notification
                match result {
                    Ok(response) => {
                        let summary = if response.len() > 500 {
                            format!("{}...", &response[..497])
                        } else {
                            response.clone()
                        };

                        if let Err(e) = scheduler.record_run_complete(
                            run_id,
                            &schedule_id,
                            RunStatus::Success,
                            Some(&summary),
                            None,
                        ) {
                            tracing::error!("Failed to record run completion: {}", e);
                        }

                        if schedule.notify_after {
                            let msg = format!("Completed: {schedule_name}\n\n{response}");
                            let formatted = to_telegram_html(&msg);
                            let _ = bot
                                .send_message(ChatId(user_id), &formatted)
                                .parse_mode(teloxide::types::ParseMode::Html)
                                .await;
                        }

                        tracing::info!("Schedule '{}' completed successfully", schedule_name);
                    }
                    Err(e) => {
                        let error_msg = format!("{e}");

                        if let Err(record_err) = scheduler.record_run_complete(
                            run_id,
                            &schedule_id,
                            RunStatus::Error,
                            None,
                            Some(&error_msg),
                        ) {
                            tracing::error!("Failed to record run error: {record_err}");
                        }

                        // Always notify on error
                        let msg = format!("Schedule '{schedule_name}' failed:\n{error_msg}");
                        let _ = bot.send_message(ChatId(user_id), &msg).await;

                        tracing::error!("Schedule '{schedule_name}' failed: {error_msg}");
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consolidate_single_message() {
        let msgs = vec!["Hello".to_string()];
        assert_eq!(consolidate_messages(&msgs), "Hello");
    }

    #[test]
    fn test_consolidate_multiple_messages() {
        let msgs = vec!["Hello".to_string(), "How are you?".to_string()];
        let result = consolidate_messages(&msgs);
        assert!(result.contains("Message 1: Hello"));
        assert!(result.contains("Message 2: How are you?"));
        assert!(result.contains("Please respond to all"));
    }

    #[test]
    fn test_consolidate_empty_returns_empty() {
        let msgs: Vec<String> = vec![];
        assert_eq!(consolidate_messages(&msgs), "");
    }
}
