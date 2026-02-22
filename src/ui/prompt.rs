//! Input prompts with π prefix.

use anyhow::Result;
use console::style;
use dialoguer::Input;

use super::Spinner;

/// Prompt configuration with context and optional URL.
pub struct PromptConfig<'a> {
    pub label: &'a str,
    pub context: &'a str,
    pub url: Option<&'a str>,
}

impl<'a> PromptConfig<'a> {
    #[must_use]
    pub const fn new(label: &'a str, context: &'a str) -> Self {
        Self {
            label,
            context,
            url: None,
        }
    }

    #[must_use]
    pub const fn with_url(mut self, url: &'a str) -> Self {
        self.url = Some(url);
        self
    }
}

/// Prompt for input with validation and optional existing value.
///
/// If `existing` is Some, shows a masked hint and allows Enter to keep it.
pub fn prompt_validated<F>(
    config: &PromptConfig<'_>,
    existing: Option<&str>,
    validator: F,
) -> Result<String>
where
    F: Fn(&str) -> std::result::Result<(), &'static str> + Clone,
{
    prompt_validated_inner(config, existing, validator, true)
}

/// Prompt for input without masking the existing value.
pub fn prompt_validated_visible<F>(
    config: &PromptConfig<'_>,
    existing: Option<&str>,
    validator: F,
) -> Result<String>
where
    F: Fn(&str) -> std::result::Result<(), &'static str> + Clone,
{
    prompt_validated_inner(config, existing, validator, false)
}

fn prompt_validated_inner<F>(
    config: &PromptConfig<'_>,
    existing: Option<&str>,
    validator: F,
    mask: bool,
) -> Result<String>
where
    F: Fn(&str) -> std::result::Result<(), &'static str> + Clone,
{
    println!();
    println!("{} {}", style("π").bold(), config.label);
    println!("  {}", style(config.context).dim());

    if let Some(url) = config.url {
        println!("  {}", style(url).cyan());
    }

    if let Some(val) = existing {
        let display = if mask {
            mask_value(val)
        } else {
            val.to_string()
        };
        println!();
        println!(
            "  {}",
            style(format!("Current: {display} (Enter to keep)")).dim()
        );
    }

    let mut input_builder = Input::<String>::new().with_prompt("  >");

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
    if value.is_empty()
        && let Some(existing_val) = existing
    {
        return Ok(existing_val.to_string());
    }

    Ok(value)
}

/// Prompt for input with async API validation.
///
/// First validates format locally, then validates against API.
#[allow(dead_code)]
pub async fn prompt_with_api_validation<F, V>(
    config: &PromptConfig<'_>,
    existing: Option<&str>,
    format_validator: F,
    api_validator: V,
    validation_message: &str,
) -> Result<String>
where
    F: Fn(&str) -> std::result::Result<(), &'static str> + Clone,
    V: std::future::Future<Output = Result<()>>,
{
    let value = prompt_validated(config, existing, format_validator)?;

    // If keeping existing value, skip API validation
    if existing.is_some() && value == existing.unwrap_or("") {
        return Ok(value);
    }

    // Validate against API
    let spinner = Spinner::new(validation_message);

    // We need to run the future here. Since we already have the value,
    // we validate it.
    match api_validator.await {
        Ok(()) => {
            spinner.finish();
            Ok(value)
        }
        Err(e) => {
            spinner.finish_error();
            Err(e)
        }
    }
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

/// Validate a Telegram bot token against the Telegram API.
pub async fn validate_telegram_token_api(token: &str) -> Result<()> {
    let url = format!("https://api.telegram.org/bot{token}/getMe");

    let response = reqwest::get(&url).await?;

    if response.status().is_success() {
        let body: serde_json::Value = response.json().await?;
        if body.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
            return Ok(());
        }
    }

    anyhow::bail!("Invalid Telegram bot token")
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

/// Validate a Claude API key against the Claude API.
pub async fn validate_claude_key_api(key: &str) -> Result<()> {
    let client = reqwest::Client::new();

    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .body(r#"{"model":"claude-3-haiku-20240307","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}"#)
        .send()
        .await?;

    // 200 = valid, 401 = invalid key, other errors are network issues
    if response.status().is_success() {
        return Ok(());
    }

    if response.status().as_u16() == 401 {
        anyhow::bail!("Invalid API key");
    }

    // Other errors (rate limit, etc) mean the key format is valid
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
    println!();
    println!(
        "  {}",
        style(format!("Default: {effective_default} (Enter to keep)")).dim()
    );

    let value: String = Input::<String>::new()
        .with_prompt("  >")
        .allow_empty(true)
        .interact_text()?;

    if value.is_empty() {
        Ok(effective_default.to_string())
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_value_short_values() {
        assert_eq!(mask_value("abc"), "***");
        assert_eq!(mask_value("12345678"), "***");
    }

    #[test]
    fn mask_value_shows_prefix_and_suffix() {
        let masked = mask_value("1234567890abcdef");
        assert!(masked.starts_with("1234"));
        assert!(masked.ends_with("cdef"));
        assert!(masked.contains("..."));
    }

    #[test]
    fn validate_telegram_token_accepts_valid() {
        assert!(validate_telegram_token("123456789:ABCdefGHIjklmnopqrstuvwxyz123456789").is_ok());
    }

    #[test]
    fn validate_telegram_token_rejects_empty() {
        assert!(validate_telegram_token("").is_err());
    }

    #[test]
    fn validate_telegram_token_rejects_no_colon() {
        assert!(validate_telegram_token("123456789ABCdefGHIjklmnopqrstuvwxyz").is_err());
    }

    #[test]
    fn validate_claude_key_accepts_valid() {
        assert!(validate_claude_key("sk-ant-api03-abcdefghijklmnopqrstuvwxyz1234567890").is_ok());
    }

    #[test]
    fn validate_claude_key_rejects_wrong_prefix() {
        assert!(validate_claude_key("sk-other-1234567890abcdefghijklmnopqrstuvwxyz").is_err());
    }

    #[test]
    fn validate_user_id_accepts_numeric() {
        assert!(validate_telegram_user_id("123456789").is_ok());
    }

    #[test]
    fn validate_user_id_rejects_non_numeric() {
        assert!(validate_telegram_user_id("abc").is_err());
    }

    #[test]
    fn validate_hostname_accepts_valid() {
        assert!(validate_hostname("pi.local").is_ok());
        assert!(validate_hostname("192.168.1.1").is_ok());
    }

    #[test]
    fn validate_hostname_rejects_spaces() {
        assert!(validate_hostname("my host").is_err());
    }

    #[test]
    fn prompt_config_creates_with_url() {
        let config = PromptConfig::new("Test", "Context").with_url("https://example.com");
        assert_eq!(config.url, Some("https://example.com"));
    }
}
