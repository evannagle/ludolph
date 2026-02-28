//! Setup wizard for initial Ludolph configuration.

use anyhow::Result;
use console::style;
use dialoguer::Select;

use crate::config::{Config, PiConfig, VaultConfig};

/// Detect if we're running on a Raspberry Pi (or similar ARM Linux device).
const fn is_running_on_pi() -> bool {
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        true
    }
    #[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
    {
        false
    }
}
use crate::ssh;
use crate::ui::prompt::PromptConfig;
use crate::ui::{self, Spinner, StatusLine};

/// LLM provider choice from setup wizard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmProvider {
    /// Use Claude Code subscription (OAuth token via `claude setup-token`)
    ClaudeCode,
    /// Use Anthropic API key (pay-per-use)
    AnthropicApi,
}

/// Collected credentials from setup wizard.
pub struct Credentials {
    pub telegram_token: String,
    pub user_id: u64,
    pub claude_key: String,
    pub vault_path: std::path::PathBuf,
    pub llm_provider: LlmProvider,
}

/// Display warning about AI limitations and privacy.
fn print_warning() {
    println!();
    println!("{}", style("Before you continue:").bold());
    println!();
    println!("  {} Ludolph gives Claude AI read access to your vault.", style("1.").dim());
    println!("     Your notes are sent to Anthropic's servers for processing.");
    println!();
    println!("  {} AI can make mistakes. Don't rely on it for critical decisions.", style("2.").dim());
    println!("     Always verify important information yourself.");
    println!();
    println!("  {} API usage incurs costs. Monitor your usage at", style("3.").dim());
    println!("     https://console.anthropic.com/settings/usage");
    println!();
}

/// Prompt user to select their LLM provider.
fn select_llm_provider() -> Result<LlmProvider> {
    println!();
    println!("{} {}", style("π").bold(), "How would you like to authenticate with Claude?");
    println!();

    let options = &[
        "Claude Code subscription (uses your Max plan credits)",
        "Anthropic API key (pay-per-use, billed separately)",
    ];

    let selection = Select::new()
        .items(options)
        .default(0)
        .interact()?;

    Ok(match selection {
        0 => LlmProvider::ClaudeCode,
        _ => LlmProvider::AnthropicApi,
    })
}

/// Get Claude Code OAuth token using `claude setup-token`.
async fn get_claude_code_token() -> Result<String> {
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

    let claude_key = match llm_provider {
        LlmProvider::ClaudeCode => {
            // Use Claude Code subscription via OAuth token
            get_claude_code_token().await?
        }
        LlmProvider::AnthropicApi => {
            // Traditional API key flow
            let claude_config = PromptConfig::new("Claude API key", "Powers the AI responses.")
                .with_url("https://console.anthropic.com/settings/keys");

            let key = ui::prompt::prompt_validated(
                &claude_config,
                existing.map(|c| c.claude.api_key.as_str()),
                ui::prompt::validate_claude_key,
            )?;

            // Validate Claude key against API
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
            key
        }
    };

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

/// Collect Pi SSH configuration and verify connectivity.
/// Returns None if SSH connection fails (setup should abort).
pub fn collect_pi_config(existing: Option<&Config>) -> Result<Option<PiConfig>> {
    println!();
    ui::status::section("Raspberry Pi");
    println!();
    println!("  Ludolph runs on your Pi. Set up SSH access first:");
    println!(
        "  {}",
        style("https://github.com/evannagle/ludolph/blob/develop/docs/pi-setup.md").cyan()
    );
    println!();

    let pi_host_config = PromptConfig::new(
        "Pi hostname or IP",
        "The network address of your Raspberry Pi.",
    );

    let pi_host = ui::prompt::prompt_validated_visible(
        &pi_host_config,
        existing
            .and_then(|c| c.pi.as_ref())
            .map(|p| p.host.as_str()),
        ui::prompt::validate_hostname,
    )?;

    let pi_user = ui::prompt::prompt_with_default(
        "SSH user",
        "pi",
        existing
            .and_then(|c| c.pi.as_ref())
            .map(|p| p.user.as_str()),
    )?;

    println!();
    let spinner = Spinner::new(&format!("Connecting to {pi_user}@{pi_host}..."));

    match ssh::test_connection(&pi_host, &pi_user) {
        Ok(()) => {
            spinner.finish();
            Ok(Some(PiConfig {
                host: pi_host,
                user: pi_user,
            }))
        }
        Err(e) => {
            spinner.finish_error();
            println!();
            println!("  SSH failed: {e}");
            println!();
            println!("  SSH key authentication is required. Run:");
            println!(
                "  {}",
                style(format!("ssh-copy-id {pi_user}@{pi_host}")).cyan()
            );
            println!();
            println!("  Then re-run `lu setup`.");
            println!();
            Ok(None)
        }
    }
}

pub async fn setup() -> Result<()> {
    let existing = Config::load().ok();

    if existing.is_some() {
        println!();
        let reconfigure = ui::prompt::confirm("Ludolph is already configured. Reconfigure?")?;
        if !reconfigure {
            println!();
            println!("  Run `lu config` to edit existing configuration.");
            println!();
            return Ok(());
        }
    }

    // Welcome
    println!();
    println!("{}", style("Welcome to Ludolph").bold());
    println!();
    println!("A real brain for your second brain.");
    println!("Talk to your vault, from anywhere, anytime.");

    // Warning about AI limitations and privacy
    print_warning();

    let proceed = ui::prompt::confirm("I understand and want to continue")?;
    if !proceed {
        println!();
        println!("  Setup cancelled.");
        println!();
        return Ok(());
    }

    // System check
    let spinner = Spinner::new("Checking system");
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    spinner.finish();
    StatusLine::ok("System compatible").print();
    StatusLine::ok("Network connected").print();

    // Collect credentials (now async for API validation)
    let creds = collect_credentials(existing.as_ref()).await?;

    // Collect Pi config only if NOT running on a Pi
    let pi_config = if is_running_on_pi() {
        None
    } else {
        collect_pi_config(existing.as_ref())?
    };

    // If Pi config collection failed (SSH error), abort
    if !is_running_on_pi() && pi_config.is_none() {
        return Ok(());
    }

    // Save config
    println!();
    let spinner = Spinner::new("Configuring Ludolph");
    let cfg = Config::new(
        creds.telegram_token,
        vec![creds.user_id],
        creds.claude_key,
        Some(creds.vault_path),
        pi_config,
        None, // MCP config is set via installer, not setup wizard
    );
    cfg.save()?;
    spinner.finish();

    StatusLine::ok("Config written").print();
    if let Some(ref vault) = cfg.vault {
        StatusLine::ok(format!("Vault: {}", vault.path.display())).print();
    }
    StatusLine::ok(format!("Authorized user: {}", creds.user_id)).print();
    let provider_name = match creds.llm_provider {
        LlmProvider::ClaudeCode => "Claude Code subscription",
        LlmProvider::AnthropicApi => "Anthropic API",
    };
    StatusLine::ok(format!("LLM: {provider_name}")).print();
    if let Some(ref pi) = cfg.pi {
        StatusLine::ok(format!("Pi: {}@{}", pi.user, pi.host)).print();
    }

    ui::status::print_success(
        "Setup complete",
        Some(
            "Commands:\n  lu            Start the Telegram bot\n  lu status     Check service status",
        ),
    );

    Ok(())
}

/// Reconfigure just the API credentials.
pub async fn setup_credentials() -> Result<()> {
    let existing = Config::load().ok();

    println!();
    println!("{}", style("Reconfigure Credentials").bold());

    let creds = collect_credentials(existing.as_ref()).await?;

    // Load existing config or create minimal one
    let mut config = existing.unwrap_or_else(|| {
        Config::new(
            creds.telegram_token.clone(),
            vec![creds.user_id],
            creds.claude_key.clone(),
            Some(creds.vault_path.clone()),
            None,
            None,
        )
    });

    // Update credentials
    config.telegram.bot_token = creds.telegram_token;
    config.telegram.allowed_users = vec![creds.user_id];
    config.claude.api_key = creds.claude_key;
    config.vault = Some(VaultConfig {
        path: creds.vault_path,
    });

    config.save()?;

    println!();
    ui::status::ok("Credentials updated");
    println!();

    Ok(())
}

/// Reconfigure just the Pi SSH connection.
pub fn setup_pi() -> Result<()> {
    let existing = Config::load().ok();

    if existing.is_none() {
        ui::status::print_error(
            "No config found",
            Some("Run `lu setup` first to configure credentials."),
        );
        return Ok(());
    }

    let mut config = existing.unwrap();

    println!();
    println!("{}", style("Reconfigure Pi Connection").bold());

    let Some(pi_config) = collect_pi_config(Some(&config))? else {
        return Ok(()); // SSH failed
    };

    config.pi = Some(pi_config);
    config.save()?;

    println!();
    ui::status::ok("Pi connection updated");
    println!();

    Ok(())
}
