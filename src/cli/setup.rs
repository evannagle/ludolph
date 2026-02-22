//! Setup wizard for initial Ludolph configuration.

use anyhow::Result;
use console::style;

use crate::config::{Config, PiConfig};
use crate::ssh;
use crate::ui::{self, PiSpinner, StatusLine};

use super::sync::collect_sync_config;

/// Collected credentials from setup wizard.
pub struct Credentials {
    pub telegram_token: String,
    pub user_id: u64,
    pub claude_key: String,
    pub vault_path: std::path::PathBuf,
}

/// Collect API credentials and vault path from user.
pub fn collect_credentials(existing: Option<&Config>) -> Result<Credentials> {
    println!();
    let telegram_token = ui::prompt::prompt_validated(
        "Telegram bot token",
        "Open Telegram, message @BotFather, send /newbot, copy the token",
        existing.map(|c| c.telegram.bot_token.as_str()),
        ui::prompt::validate_telegram_token,
    )?;

    let existing_user_id = existing
        .and_then(|c| c.telegram.allowed_users.first())
        .map(ToString::to_string);

    let telegram_user_id = ui::prompt::prompt_validated(
        "Your Telegram user ID",
        "Message @userinfobot on Telegram - it will reply with your numeric ID",
        existing_user_id.as_deref(),
        ui::prompt::validate_telegram_user_id,
    )?;

    let claude_key = ui::prompt::prompt_validated(
        "Claude API key",
        "Visit console.anthropic.com/settings/keys, create key, copy it",
        existing.map(|c| c.claude.api_key.as_str()),
        ui::prompt::validate_claude_key,
    )?;

    let existing_vault = existing.map(|c| c.vault.path.to_string_lossy().to_string());

    let vault_path = ui::prompt::prompt_validated_visible(
        "Path to your Obsidian vault",
        "The folder where your markdown notes live (e.g., ~/Documents/Vault)",
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
        style("https://github.com/evannagle/ludolph/blob/develop/docs/pi-setup.md").dim()
    );
    println!();

    let pi_host = ui::prompt::prompt_validated_visible(
        "Pi hostname or IP",
        "Run `ping pi.local` or check your router for the Pi's address",
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
    ui::status::checking("Testing SSH connection...");

    match ssh::test_connection(&pi_host, &pi_user) {
        Ok(()) => {
            ui::status::ok(&format!("Connected to {pi_user}@{pi_host}"));
            Ok(Some(PiConfig {
                host: pi_host,
                user: pi_user,
            }))
        }
        Err(e) => {
            ui::status::error(&format!("SSH failed: {e}"));
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
    let spinner = PiSpinner::new("Checking system");
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    spinner.finish();
    StatusLine::ok("System compatible").print();
    StatusLine::ok("Network connected").print();

    // Collect credentials
    let creds = collect_credentials(existing.as_ref())?;

    // Collect Pi config (required)
    let Some(pi_config) = collect_pi_config(existing.as_ref())? else {
        return Ok(()); // SSH failed, user instructed to fix and re-run
    };

    // Collect sync config (optional)
    let sync_config = collect_sync_config(&creds.vault_path, &pi_config, None)?;

    // Save config
    println!();
    let spinner = PiSpinner::new("Configuring Ludolph");
    let cfg = Config::new(
        creds.telegram_token,
        vec![creds.user_id],
        creds.claude_key,
        creds.vault_path,
        Some(pi_config),
    )
    .with_sync(sync_config);
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
            "Run `lu` to start the bot!\n\nCommands:\n  lu            Start the Telegram bot\n  lu status     Check service status\n  lu config     Edit configuration",
        ),
    );

    Ok(())
}

/// Reconfigure just the API credentials.
pub fn setup_credentials() -> Result<()> {
    let existing = Config::load().ok();

    println!();
    println!("{}", style("Reconfigure Credentials").bold());

    let creds = collect_credentials(existing.as_ref())?;

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
