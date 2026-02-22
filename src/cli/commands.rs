//! CLI commands.

use std::process::ExitCode;

use anyhow::Result;
use console::style;
use walkdir::WalkDir;

use crate::config::{self, Config};
use crate::ssh;
use crate::ui::{self, Spinner, StatusLine};

/// Run health checks and return appropriate exit code.
pub fn check() -> ExitCode {
    // Print version
    println!();
    println!("lu {}", env!("CARGO_PKG_VERSION"));
    println!();

    // CLI check (always passes if we got here)
    StatusLine::ok("CLI").print();

    // Config check
    let config = Config::load().map_or_else(
        |_| {
            StatusLine::skip("Config (not found)").print();
            None
        },
        |cfg| {
            StatusLine::ok("Config loaded").print();
            Some(cfg)
        },
    );

    // Vault check
    match config.as_ref().map(|c| &c.vault.path) {
        Some(path) if path.exists() => {
            let count = WalkDir::new(path)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_file())
                .count();
            StatusLine::ok(format!("Vault accessible ({count} files)")).print();
        }
        Some(path) => {
            StatusLine::error(format!("Vault not found: {}", path.display())).print();
            println!();
            return ExitCode::FAILURE;
        }
        None => {
            StatusLine::skip("Vault (not configured)").print();
        }
    }

    println!();
    ExitCode::SUCCESS
}

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
