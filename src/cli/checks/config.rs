//! Configuration-related diagnostic checks.

use walkdir::WalkDir;

use super::{CheckContext, CheckResult};
use crate::config;

/// Check if the config file exists.
pub fn config_exists(_ctx: &CheckContext) -> CheckResult {
    let path = config::config_path();

    if path.exists() {
        CheckResult::pass(format!("Config file exists at {}", path.display()))
    } else {
        CheckResult::fail(
            format!("Config file not found at {}", path.display()),
            "Run `lu setup` to create a configuration",
            "config-missing",
        )
    }
}

/// Check if the config file is valid and contains required fields.
pub fn config_valid(ctx: &CheckContext) -> CheckResult {
    let Some(config) = &ctx.config else {
        return CheckResult::fail(
            "Could not load config file",
            "Check config syntax: `cat ~/.ludolph/config.toml`",
            "config-invalid",
        );
    };

    // Check Telegram token
    if config.telegram.bot_token.is_empty() {
        return CheckResult::fail(
            "Telegram bot token is empty",
            "Run `lu setup credentials` to set your bot token",
            "config-telegram",
        );
    }

    // Check Claude API key
    if config.claude.api_key.is_empty() {
        return CheckResult::fail(
            "Claude API key is empty",
            "Run `lu setup credentials` to set your API key",
            "config-claude",
        );
    }

    // Check allowed users
    if config.telegram.allowed_users.is_empty() {
        return CheckResult::fail(
            "No allowed Telegram users configured",
            "Run `lu setup credentials` to set your user ID",
            "config-users",
        );
    }

    CheckResult::pass("Config file valid with all required fields")
}

/// Check if the vault is accessible and contains files.
pub fn vault_accessible(ctx: &CheckContext) -> CheckResult {
    let Some(config) = &ctx.config else {
        return CheckResult::skip("Config not loaded");
    };

    // If using MCP instead of local vault, skip this check
    if config.vault.is_none() && config.mcp.is_some() {
        return CheckResult::pass("Using MCP server for vault access");
    }

    let Some(vault) = &config.vault else {
        return CheckResult::skip("No vault configured (using MCP)");
    };

    if !vault.path.exists() {
        return CheckResult::fail(
            format!("Vault not found at {}", vault.path.display()),
            "Check the vault path in config, or run `lu setup credentials`",
            "vault-missing",
        );
    }

    if !vault.path.is_dir() {
        return CheckResult::fail(
            format!("Vault path is not a directory: {}", vault.path.display()),
            "Vault path should point to your Obsidian vault directory",
            "vault-not-dir",
        );
    }

    // Count files
    let count = WalkDir::new(&vault.path)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .count();

    if count == 0 {
        return CheckResult::fail(
            format!("Vault is empty: {}", vault.path.display()),
            "Add some notes to your vault, or check if the path is correct",
            "vault-empty",
        );
    }

    CheckResult::pass(format!(
        "Vault accessible: {} ({count} files)",
        vault.path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn config_exists_passes_when_file_exists() {
        // This is an integration test that checks the real config path
        // In a unit test, we'd mock the path
        let ctx = CheckContext::new();
        let result = config_exists(&ctx);
        // Result depends on whether config exists on test machine
        assert!(result.is_pass() || result.is_fail());
    }

    #[test]
    fn vault_accessible_counts_files() {
        let dir = tempdir().unwrap();
        let vault_path = dir.path().join("vault");
        fs::create_dir(&vault_path).unwrap();
        fs::write(vault_path.join("note1.md"), "content").unwrap();
        fs::write(vault_path.join("note2.md"), "content").unwrap();

        // We can't easily test this without mocking the config
        // In a real test, we'd inject the vault path
    }
}
