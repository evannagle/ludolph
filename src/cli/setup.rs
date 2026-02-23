//! Setup wizard for initial Ludolph configuration.

use anyhow::Result;
use console::style;

use crate::config::{Config, PiConfig};

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

/// Collected credentials from setup wizard.
pub struct Credentials {
    pub telegram_token: String,
    pub user_id: u64,
    pub claude_key: String,
    pub vault_path: std::path::PathBuf,
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

    // Claude API key
    let claude_config = PromptConfig::new("Claude API key", "Powers the AI responses.")
        .with_url("https://console.anthropic.com/settings/keys");

    let claude_key = ui::prompt::prompt_validated(
        &claude_config,
        existing.map(|c| c.claude.api_key.as_str()),
        ui::prompt::validate_claude_key,
    )?;

    // Validate Claude key against API
    let existing_key = existing.map_or("", |c| c.claude.api_key.as_str());
    if claude_key != existing_key {
        let spinner = Spinner::new("Validating API key...");
        match ui::prompt::validate_claude_key_api(&claude_key).await {
            Ok(()) => spinner.finish(),
            Err(e) => {
                spinner.finish_error();
                anyhow::bail!("API key validation failed: {e}");
            }
        }
    }

    // Vault path
    let vault_config = PromptConfig::new(
        "Path to your Obsidian vault (on this machine)",
        "Local folder with your notes. Sync from desktop if needed.",
    );

    let existing_vault = existing.map(|c| c.vault.path.to_string_lossy().to_string());

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
    println!();

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
        creds.vault_path,
        pi_config,
    );
    cfg.save()?;
    spinner.finish();

    StatusLine::ok("Config written").print();
    StatusLine::ok(format!("Vault: {}", cfg.vault.path.display())).print();
    StatusLine::ok(format!("Authorized user: {}", creds.user_id)).print();
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
            creds.vault_path.clone(),
            None,
        )
    });

    // Update credentials
    config.telegram.bot_token = creds.telegram_token;
    config.telegram.allowed_users = vec![creds.user_id];
    config.claude.api_key = creds.claude_key;
    config.vault.path = creds.vault_path;

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
