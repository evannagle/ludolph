//! CLI commands.

use anyhow::Result;
use console::style;

use crate::config::{self, Config};
use crate::ssh;
use crate::ui::{self, Spinner, StatusLine};

pub fn config_cmd() -> Result<()> {
    let config_path = config::config_path();

    if !config_path.exists() {
        ui::status::print_error(
            "No config file found",
            Some("Run `lu setup` to create one."),
        );
        return Ok(());
    }

    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());

    std::process::Command::new(editor)
        .arg(&config_path)
        .status()?;

    Ok(())
}

#[allow(clippy::unnecessary_wraps)]
pub fn pi() -> Result<()> {
    let Ok(config) = Config::load() else {
        ui::status::print_error("No config found", Some("Run `lu setup` first."));
        return Ok(());
    };

    let Some(pi) = config.pi else {
        println!();
        StatusLine::error("No Pi configured").print();
        ui::status::hint("Run `lu setup` to configure Pi connection");
        println!();
        return Ok(());
    };

    println!();
    println!("{}", style("Pi Connection").bold());
    println!();

    let spinner = Spinner::new(&format!("Connecting to {}@{}...", pi.user, pi.host));

    match ssh::test_connection(&pi.host, &pi.user) {
        Ok(()) => {
            spinner.finish();
        }
        Err(e) => {
            spinner.finish_error();
            ui::status::hint(&format!("Connection failed: {e}"));
            ui::status::hint("Check if Pi is online and SSH key auth is set up");
        }
    }

    println!();
    Ok(())
}
