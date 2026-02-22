//! Input prompts with π prefix.

use anyhow::Result;
use console::style;
use dialoguer::Input;

/// Prompt for input with validation and optional existing value.
///
/// If `existing` is Some, shows a masked hint and allows Enter to keep it.
/// Use `mask: true` for sensitive values (API keys), `false` for paths.
pub fn prompt_validated<F>(
    label: &str,
    help: &str,
    existing: Option<&str>,
    validator: F,
) -> Result<String>
where
    F: Fn(&str) -> std::result::Result<(), &'static str> + Clone,
{
    prompt_validated_inner(label, help, existing, validator, true)
}

/// Prompt for input without masking the existing value.
pub fn prompt_validated_visible<F>(
    label: &str,
    help: &str,
    existing: Option<&str>,
    validator: F,
) -> Result<String>
where
    F: Fn(&str) -> std::result::Result<(), &'static str> + Clone,
{
    prompt_validated_inner(label, help, existing, validator, false)
}

fn prompt_validated_inner<F>(
    label: &str,
    help: &str,
    existing: Option<&str>,
    validator: F,
    mask: bool,
) -> Result<String>
where
    F: Fn(&str) -> std::result::Result<(), &'static str> + Clone,
{
    println!();
    println!("{} {}", style("π").bold(), label);
    println!("  {}", style(help).dim());

    if let Some(val) = existing {
        let display = if mask {
            mask_value(val)
        } else {
            val.to_string()
        };
        println!(
            "  {}",
            style(format!("Current: {display} (Enter to keep)")).dim()
        );
    }

    let mut input_builder = Input::<String>::new().with_prompt("  ");

    // If we have an existing value, use empty string as default (we'll substitute later)
    if existing.is_some() {
        input_builder = input_builder.allow_empty(true);
    }

    let has_existing = existing.is_some();

    let value: String = input_builder
        .validate_with(move |input: &String| -> std::result::Result<(), &str> {
            if input.is_empty() && has_existing {
                return Ok(()); // Empty is valid if we have existing
            }
            validator(input.as_str())
        })
        .interact_text()?;

    // Return existing value if input was empty
    if value.is_empty() {
        if let Some(existing_val) = existing {
            return Ok(existing_val.to_string());
        }
    }

    Ok(value)
}

/// Mask a sensitive value for display.
fn mask_value(value: &str) -> String {
    if value.len() <= 8 {
        return "***".to_string();
    }
    let prefix: String = value.chars().take(4).collect();
    let suffix: String = value
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{prefix}...{suffix}")
}

/// Prompt for confirmation (yes/no).
pub fn confirm(message: &str) -> Result<bool> {
    let result = dialoguer::Confirm::new()
        .with_prompt(format!("{} {}", style("π").bold(), message))
        .default(false)
        .interact()?;
    Ok(result)
}

/// Validate a Telegram bot token format.
///
/// Format: `123456789:ABCdefGHIjklMNOpqrsTUVwxyz`
pub fn validate_telegram_token(token: &str) -> std::result::Result<(), &'static str> {
    if token.is_empty() {
        return Err("Token cannot be empty");
    }

    let parts: Vec<&str> = token.split(':').collect();
    if parts.len() != 2 {
        return Err("Invalid format (expected ID:token)");
    }

    if parts[0].parse::<u64>().is_err() {
        return Err("Invalid bot ID (should be numeric)");
    }

    if parts[1].len() < 30 {
        return Err("Token part looks too short");
    }

    Ok(())
}

/// Validate a Claude API key format.
///
/// Format: starts with `sk-ant-`
pub fn validate_claude_key(key: &str) -> std::result::Result<(), &'static str> {
    if key.is_empty() {
        return Err("API key cannot be empty");
    }

    if !key.starts_with("sk-ant-") {
        return Err("Should start with 'sk-ant-'");
    }

    if key.len() < 40 {
        return Err("Key looks too short");
    }

    Ok(())
}

/// Validate a Telegram user ID (numeric).
pub fn validate_telegram_user_id(id: &str) -> std::result::Result<(), &'static str> {
    if id.is_empty() {
        return Err("User ID cannot be empty");
    }

    if id.parse::<u64>().is_err() {
        return Err("User ID must be a number");
    }

    Ok(())
}

/// Validate a vault path exists.
pub fn validate_vault_path(path: &str) -> std::result::Result<(), &'static str> {
    if path.is_empty() {
        return Err("Path cannot be empty");
    }

    let expanded = shellexpand::tilde(path);
    let path = std::path::Path::new(expanded.as_ref());

    if !path.exists() {
        return Err("Path does not exist");
    }

    if !path.is_dir() {
        return Err("Path must be a directory");
    }

    Ok(())
}

/// Validate a hostname or IP address.
pub fn validate_hostname(s: &str) -> std::result::Result<(), &'static str> {
    if s.is_empty() {
        return Err("Hostname cannot be empty");
    }
    if s.contains(' ') {
        return Err("Hostname cannot contain spaces");
    }
    Ok(())
}

/// Prompt for input with a default value.
///
/// If `existing` is Some, shows the existing value and allows Enter to keep it.
/// If `existing` is None but `default` is provided, uses that as fallback.
pub fn prompt_with_default(label: &str, default: &str, existing: Option<&str>) -> Result<String> {
    println!();
    println!("{} {}", style("π").bold(), label);

    let effective_default = existing.unwrap_or(default);
    println!(
        "  {}",
        style(format!("Default: {effective_default} (Enter to keep)")).dim()
    );

    let value: String = Input::<String>::new()
        .with_prompt("  ")
        .allow_empty(true)
        .interact_text()?;

    if value.is_empty() {
        Ok(effective_default.to_string())
    } else {
        Ok(value)
    }
}
