//! Telegram bot handler.

use std::collections::HashSet;

use anyhow::{Context, Result};
use console::style;
use teloxide::prelude::*;

use crate::claude::Claude;
use crate::config::Config;
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

    // Validate vault
    if !config.vault.path.exists() {
        StatusLine::error(format!("Vault not found: {}", config.vault.path.display())).print();
        anyhow::bail!("Vault directory does not exist");
    }
    StatusLine::ok(format!("Vault: {}", config.vault.path.display())).print();

    // Telegram validated (already fetched username above)
    StatusLine::ok(format!("Telegram: @{bot_username}")).print();

    // Ready
    println!();
    println!("  Listening... {}", style("(Ctrl+C to stop)").dim());
    println!();

    // Run bot
    let bot = Bot::new(&config.telegram.bot_token);
    let claude = Claude::from_config(&config);
    let allowed_users: HashSet<u64> = config.telegram.allowed_users.into_iter().collect();

    Box::pin(teloxide::repl(bot, move |bot: Bot, msg: Message| {
        let claude = claude.clone();
        let allowed_users = allowed_users.clone();
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
                let response = claude
                    .chat(text)
                    .await
                    .unwrap_or_else(|e| format!("Error: {e}"));

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
