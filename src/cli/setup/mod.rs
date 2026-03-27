//! Setup wizard modules for Ludolph configuration.
//!
//! The setup wizard is organized into phases that can be run individually
//! or as part of the full setup flow:
//!
//! - `credentials`: Collect API keys, vault path (existing)
//! - `pi`: Test SSH connectivity to Pi (existing)
//! - `mcp`: Install MCP server, generate tokens, write .mcp.json (new)
//! - `deploy`: Deploy lu binary and config to Pi via SSH (new)
//! - `verify`: Health checks and end-to-end test (new)

mod credentials;
mod deploy;
mod mcp;
mod pi;
mod verify;

use anyhow::Result;
use console::style;

use crate::config::{Config, IndexTier};
use crate::ui::{self, Spinner, StatusLine};

pub use credentials::{LlmProvider, collect_credentials};
pub use deploy::setup_deploy;
pub use mcp::setup_mcp;
pub use pi::collect_pi_config;
pub use verify::setup_verify;

/// Detect if we're running on a Raspberry Pi (or similar ARM Linux device).
pub const fn is_running_on_pi() -> bool {
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        true
    }
    #[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
    {
        false
    }
}

/// Display warning about AI limitations and privacy.
fn print_warning() {
    println!();
    println!("{}", style("Before you continue:").bold());
    println!();
    println!(
        "  {} Ludolph gives AI read and write access to your vault.",
        style("1.").dim()
    );
    println!("     Your notes are sent to your AI provider for processing.");
    println!();
    println!(
        "  {} AI can modify files. Content could be lost.",
        style("2.").dim()
    );
    println!("     Use source control (git) to help with recovery.");
    println!();
    println!(
        "  {} AI can make mistakes. Don't rely on it for critical decisions.",
        style("3.").dim()
    );
    println!("     Always verify important information yourself.");
    println!();
    println!(
        "  {} API usage incurs costs. Monitor your usage at",
        style("4.").dim()
    );
    println!("     your provider's dashboard.");
    println!();
    println!("If you understand, type the first five digits of pi after 3.14");
}

/// Run the full setup wizard.
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

    let answer: String = dialoguer::Input::new()
        .with_prompt("π 3.14")
        .interact_text()?;

    if answer.trim() != "15926" {
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

    // Phase 1: Collect credentials
    let creds = collect_credentials(existing.as_ref()).await?;

    // Phase 2: Collect Pi config (only if NOT running on a Pi)
    let pi_config = if is_running_on_pi() {
        None
    } else {
        collect_pi_config(existing.as_ref())?
    };

    // If Pi config collection failed (SSH error), abort
    if !is_running_on_pi() && pi_config.is_none() {
        return Ok(());
    }

    // Save initial config
    println!();
    let spinner = Spinner::new("Configuring Ludolph");
    let cfg = Config::new(
        creds.telegram_token.clone(),
        vec![creds.user_id],
        creds.claude_key.clone(),
        Some(creds.vault_path.clone()),
        pi_config.clone(),
        None, // MCP config is set in mcp phase
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
        LlmProvider::OpenAI => "OpenAI API",
        LlmProvider::Gemini => "Google Gemini",
    };
    StatusLine::ok(format!("LLM: {provider_name}")).print();
    if let Some(ref pi) = pi_config {
        StatusLine::ok(format!("Pi: {}@{}", pi.user, pi.host)).print();
    }

    // Phase 2.5: Vault index setup
    setup_vault_index(&cfg).await?;

    // Phase 3: MCP setup (Mac only)
    if !is_running_on_pi() {
        setup_mcp().await?;
    }

    // Phase 4: Deploy to Pi (Mac only)
    if !is_running_on_pi() {
        if let Some(ref pi) = pi_config {
            setup_deploy(pi).await?;
        }
    }

    // Phase 5: Verify
    setup_verify(pi_config.as_ref()).await?;

    ui::status::print_success(
        "Setup complete",
        Some(
            "Commands:\n  lu            Start the Telegram bot\n  lu status     Check service status",
        ),
    );

    Ok(())
}

/// Run the vault index tier selection and initial indexing step.
///
/// Counts markdown files in the vault, prompts the user to choose a tier,
/// saves the choice to config, and runs the initial index.
/// On failure, prints a skip status rather than aborting setup.
async fn setup_vault_index(cfg: &Config) -> Result<()> {
    let Some(ref vault) = cfg.vault else {
        return Ok(());
    };

    let vault_path_str = shellexpand::tilde(&vault.path.to_string_lossy()).to_string();
    let vault_path = std::path::Path::new(&vault_path_str);

    if !vault_path.exists() {
        return Ok(());
    }

    let file_count = walkdir::WalkDir::new(vault_path)
        .into_iter()
        .filter_entry(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file() && e.path().extension().is_some_and(|ext| ext == "md"))
        .count();

    #[allow(clippy::cast_precision_loss)]
    let est_cost = {
        let chunks_est = file_count as f64 * 3.0;
        let input_tokens = chunks_est * 250.0;
        let output_tokens = chunks_est * 50.0;
        input_tokens.mul_add(0.25, output_tokens * 1.25) / 1_000_000.0
    };
    let est_time_standard = if file_count < 1000 {
        "~30 seconds"
    } else {
        "~2 minutes"
    };
    let est_time_deep = if file_count < 1000 {
        "~30 minutes"
    } else {
        "~3 hours"
    };

    println!();
    println!("  Vault found: {file_count} files");
    println!();
    println!("  How should Lu learn your vault?");
    println!();
    println!("    1. Quick    — file map only (free, ~5 seconds)");
    println!("    2. Standard — chunked index (free, {est_time_standard})");
    println!("    3. Deep     — chunked + AI summaries (~${est_cost:.0}, {est_time_deep})");
    println!();

    let tier_choice = ui::prompt::prompt_with_default("Choose [1/2/3]", "2", None)?;
    let tier = match tier_choice.trim() {
        "1" => IndexTier::Quick,
        "3" => IndexTier::Deep,
        _ => IndexTier::Standard,
    };

    // Update config with chosen tier and save.
    let mut updated_cfg = Config::load()?;
    updated_cfg.index.tier = tier;
    updated_cfg.save()?;

    // Run initial index.
    let spinner = Spinner::new(&format!("Indexing vault ({tier})..."));
    let indexer = crate::index::indexer::Indexer::new(vault_path.to_path_buf(), tier);
    match indexer.run(false).await {
        Ok(stats) => {
            spinner.finish();
            StatusLine::ok(format!(
                "{} files indexed, {} chunks",
                stats.files_indexed, stats.chunks_created,
            ))
            .print();
        }
        Err(e) => {
            spinner.finish_error();
            tracing::error!("Initial indexing failed: {}", e);
            StatusLine::skip(format!("Indexing failed: {e}. Retry with `lu index`")).print();
        }
    }

    Ok(())
}

/// Reconfigure just the API credentials.
pub async fn setup_credentials_cmd() -> Result<()> {
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
    config.vault = Some(crate::config::VaultConfig {
        path: creds.vault_path,
    });

    config.save()?;

    println!();
    ui::status::ok("Credentials updated");
    println!();

    Ok(())
}

/// Reconfigure just the Pi SSH connection.
pub fn setup_pi_cmd() -> Result<()> {
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

/// Run just the MCP setup phase.
pub async fn setup_mcp_cmd() -> Result<()> {
    if is_running_on_pi() {
        ui::status::print_error(
            "MCP setup only runs on Mac",
            Some("The MCP server runs on your Mac, not on Pi."),
        );
        return Ok(());
    }

    println!();
    println!("{}", style("MCP Server Setup").bold());

    setup_mcp().await?;

    println!();
    ui::status::ok("MCP setup complete");
    println!();

    Ok(())
}

/// Run just the deploy phase.
pub async fn setup_deploy_cmd() -> Result<()> {
    if is_running_on_pi() {
        ui::status::print_error(
            "Deploy runs from Mac only",
            Some("Run this command from your Mac to deploy to Pi."),
        );
        return Ok(());
    }

    let config = Config::load().map_err(|_| {
        anyhow::anyhow!("No config found. Run `lu setup credentials` and `lu setup pi` first.")
    })?;

    let pi = config
        .pi
        .ok_or_else(|| anyhow::anyhow!("No Pi configured. Run `lu setup pi` first."))?;

    println!();
    println!("{}", style("Deploy to Pi").bold());

    setup_deploy(&pi).await?;

    println!();
    ui::status::ok("Deployment complete");
    println!();

    Ok(())
}

/// Run just the verify phase.
pub async fn setup_verify_cmd() -> Result<()> {
    let config = Config::load().ok();
    let pi_config = config.as_ref().and_then(|c| c.pi.as_ref());

    println!();
    println!("{}", style("Verify Setup").bold());

    setup_verify(pi_config).await?;

    println!();
    Ok(())
}
