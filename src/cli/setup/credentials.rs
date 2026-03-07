//! Credential collection for Ludolph setup.

use anyhow::Result;
use console::style;
use dialoguer::Select;

use crate::config::Config;
use crate::ui::prompt::PromptConfig;
use crate::ui::{self, Spinner};

/// LLM provider choice from setup wizard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmProvider {
    /// Use Claude Code subscription (OAuth token via `claude setup-token`)
    ClaudeCode,
    /// Use Anthropic API key (pay-per-use)
    AnthropicApi,
    /// Use `OpenAI` API (GPT-4, `ChatGPT`)
    OpenAI,
    /// Use Google Gemini API
    Gemini,
}

/// Collected credentials from setup wizard.
pub struct Credentials {
    pub telegram_token: String,
    pub user_id: u64,
    pub claude_key: String,
    pub vault_path: std::path::PathBuf,
    pub llm_provider: LlmProvider,
}

/// Prompt user to select their LLM provider.
fn select_llm_provider() -> Result<LlmProvider> {
    println!();
    println!(
        "{} Which AI provider would you like to use?",
        style("π").bold()
    );
    println!();

    let options = &[
        "Claude Code subscription (uses your Max plan credits)",
        "Anthropic API (Claude, pay-per-use)",
        "OpenAI API (GPT-4, ChatGPT)",
        "Google Gemini API",
    ];

    let selection = Select::new().items(options).default(0).interact()?;

    Ok(match selection {
        0 => LlmProvider::ClaudeCode,
        2 => LlmProvider::OpenAI,
        3 => LlmProvider::Gemini,
        _ => LlmProvider::AnthropicApi,
    })
}

/// Get Claude Code OAuth token using `claude setup-token`.
fn get_claude_code_token() -> Result<String> {
    use std::process::Command;

    // Check if claude CLI is available
    let check = Command::new("claude").arg("--version").output();

    if check.is_err() || !check.as_ref().is_ok_and(|o| o.status.success()) {
        anyhow::bail!(
            "Claude Code CLI not found. Install it first:\n  \
             npm install -g @anthropic-ai/claude-code"
        );
    }

    println!();
    println!("  This will open a browser to authenticate with your Claude subscription.");
    println!("  The generated token will be saved for Ludolph to use.");
    println!();

    let proceed = ui::prompt::confirm("Continue with Claude Code authentication?")?;
    if !proceed {
        anyhow::bail!("Authentication cancelled");
    }

    println!();
    let spinner = Spinner::new("Running claude setup-token...");

    let output = Command::new("claude")
        .arg("setup-token")
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run claude setup-token: {e}"))?;

    if !output.status.success() {
        spinner.finish_error();
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("claude setup-token failed: {stderr}");
    }

    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Validate the token format
    if token.starts_with("sk-ant-") && token.len() > 40 {
        spinner.finish();
        return Ok(token);
    }

    // Token might need manual entry (interactive mode)
    spinner.finish();
    println!();
    println!("  If a token was displayed, enter it below.");
    println!("  Otherwise, check the browser window that opened.");
    println!();

    let manual_token = ui::prompt::prompt_validated(
        &PromptConfig::new("OAuth token", "Paste the token from claude setup-token"),
        None,
        ui::prompt::validate_claude_key,
    )?;

    Ok(manual_token)
}

/// Collect LLM provider credentials based on selected provider.
async fn collect_llm_key(provider: LlmProvider, existing: Option<&Config>) -> Result<String> {
    match provider {
        LlmProvider::ClaudeCode => get_claude_code_token(),
        LlmProvider::AnthropicApi => {
            let config = PromptConfig::new("Anthropic API key", "Powers the AI responses.")
                .with_url("https://console.anthropic.com/settings/keys");
            let key = ui::prompt::prompt_validated(
                &config,
                existing.map(|c| c.claude.api_key.as_str()),
                ui::prompt::validate_claude_key,
            )?;
            let existing_key = existing.map_or("", |c| c.claude.api_key.as_str());
            if key != existing_key {
                let spinner = Spinner::new("Validating API key...");
                match ui::prompt::validate_claude_key_api(&key).await {
                    Ok(()) => spinner.finish(),
                    Err(e) => {
                        spinner.finish_error();
                        anyhow::bail!("API key validation failed: {e}");
                    }
                }
            }
            Ok(key)
        }
        LlmProvider::OpenAI => {
            let config = PromptConfig::new("OpenAI API key", "Powers GPT-4 responses.")
                .with_url("https://platform.openai.com/api-keys");
            ui::prompt::prompt_validated(&config, None, |key| {
                if key.is_empty() {
                    return Err("API key cannot be empty");
                }
                if !key.starts_with("sk-") {
                    return Err("Should start with 'sk-'");
                }
                Ok(())
            })
        }
        LlmProvider::Gemini => {
            let config = PromptConfig::new("Google Gemini API key", "Powers Gemini responses.")
                .with_url("https://aistudio.google.com/apikey");
            ui::prompt::prompt_validated(&config, None, |key| {
                if key.is_empty() {
                    return Err("API key cannot be empty");
                }
                if key.len() < 20 {
                    return Err("Key looks too short");
                }
                Ok(())
            })
        }
    }
}

/// Collect API credentials and vault path from user.
pub async fn collect_credentials(existing: Option<&Config>) -> Result<Credentials> {
    // Telegram bot token
    let telegram_config = PromptConfig::new(
        "Telegram bot token",
        "Ludolph receives messages through Telegram's Bot API.",
    )
    .with_url("https://t.me/botfather");

    let telegram_token = ui::prompt::prompt_validated(
        &telegram_config,
        existing.map(|c| c.telegram.bot_token.as_str()),
        ui::prompt::validate_telegram_token,
    )?;

    // Validate token against Telegram API
    let existing_token = existing.map_or("", |c| c.telegram.bot_token.as_str());
    if telegram_token != existing_token {
        let spinner = Spinner::new("Validating token...");
        match ui::prompt::validate_telegram_token_api(&telegram_token).await {
            Ok(()) => spinner.finish(),
            Err(e) => {
                spinner.finish_error();
                anyhow::bail!("Token validation failed: {e}");
            }
        }
    }

    // Telegram user ID
    let user_id_config =
        PromptConfig::new("Your Telegram user ID", "Only you can talk to this bot.")
            .with_url("https://t.me/userinfobot");

    let existing_user_id = existing
        .and_then(|c| c.telegram.allowed_users.first())
        .map(ToString::to_string);

    let telegram_user_id = ui::prompt::prompt_validated(
        &user_id_config,
        existing_user_id.as_deref(),
        ui::prompt::validate_telegram_user_id,
    )?;

    // LLM provider selection
    let llm_provider = select_llm_provider()?;
    let claude_key = collect_llm_key(llm_provider, existing).await?;

    // Vault path
    let vault_config = PromptConfig::new(
        "Path to your Obsidian vault (on this machine)",
        "Local folder with your notes. Sync from desktop if needed.",
    );

    let existing_vault = existing.and_then(|c| {
        c.vault
            .as_ref()
            .map(|v| v.path.to_string_lossy().to_string())
    });

    let vault_path = ui::prompt::prompt_validated_visible(
        &vault_config,
        existing_vault.as_deref(),
        ui::prompt::validate_vault_path,
    )?;

    let user_id: u64 = telegram_user_id.parse().expect("validated above");
    let vault_expanded = shellexpand::tilde(&vault_path);
    let vault_path = std::path::PathBuf::from(vault_expanded.as_ref());

    Ok(Credentials {
        telegram_token,
        user_id,
        claude_key,
        vault_path,
        llm_provider,
    })
}
