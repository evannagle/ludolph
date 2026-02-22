use std::collections::HashSet;

use anyhow::Result;
use teloxide::prelude::*;

use crate::claude::Claude;
use crate::config::Config;

pub async fn run() -> Result<()> {
    let config = Config::load()?;

    tracing::info!("Starting Ludolph bot...");

    let bot = Bot::new(&config.telegram.bot_token);
    let claude = Claude::from_config(&config);
    let allowed_users: HashSet<u64> = config.telegram.allowed_users.into_iter().collect();

    if allowed_users.is_empty() {
        tracing::warn!("No allowed users configured - bot will ignore all messages");
    }

    Box::pin(teloxide::repl(bot, move |bot: Bot, msg: Message| {
        let claude = claude.clone();
        let allowed_users = allowed_users.clone();
        async move {
            // Check if user is authorized
            let user_id = msg.from.as_ref().map(|u| u.id.0);

            if let Some(id) = user_id {
                if !allowed_users.contains(&id) {
                    tracing::debug!("Ignoring message from unauthorized user: {id}");
                    return Ok(());
                }
            } else {
                tracing::debug!("Ignoring message with no user ID");
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

    Ok(())
}
