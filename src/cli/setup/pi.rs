//! Pi SSH configuration for Ludolph setup.

use anyhow::Result;
use console::style;

use crate::config::{Config, PiConfig};
use crate::ssh;
use crate::ui::prompt::PromptConfig;
use crate::ui::{self, Spinner};

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
