use anyhow::Result;
use teloxide::prelude::*;
use crate::claude::Claude;

pub async fn run() -> Result<()> {
    tracing::info!("Starting Ludolph bot...");

    let bot = Bot::from_env();
    let claude = Claude::new()?;

    teloxide::repl(bot, move |bot: Bot, msg: Message| {
        let claude = claude.clone();
        async move {
            if let Some(text) = msg.text() {
                let response = claude.chat(text).await.unwrap_or_else(|e| {
                    format!("Error: {}", e)
                });

                bot.send_message(msg.chat.id, response).await?;
            }
            Ok(())
        }
    })
    .await;

    Ok(())
}
