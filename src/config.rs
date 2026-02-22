//! Configuration loading and management.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Ludolph configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub telegram: TelegramConfig,
    pub claude: ClaudeConfig,
    #[serde(default)]
    pub vault: VaultConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pi: Option<PiConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub bot_token: String,
    #[serde(default)]
    pub allowed_users: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeConfig {
    pub api_key: String,
    #[serde(default = "default_model")]
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultConfig {
    /// Path to the vault directory.
    #[serde(default = "default_vault_path")]
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiConfig {
    pub host: String,
    pub user: String,
}

fn default_model() -> String {
    "claude-sonnet-4-20250514".to_string()
}

fn default_vault_path() -> PathBuf {
    config_dir().join("vault")
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            path: default_vault_path(),
        }
    }
}

impl Config {
    /// Load configuration from the default path.
    pub fn load() -> Result<Self> {
        let path = config_path();
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config from {}", path.display()))?;
        let config: Self =
            toml::from_str(&contents).with_context(|| "Failed to parse config.toml")?;
        Ok(config)
    }

    /// Save configuration to the default path.
    pub fn save(&self) -> Result<()> {
        let path = config_path();

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }

        let contents = toml::to_string_pretty(self).context("Failed to serialize config")?;
        std::fs::write(&path, contents)
            .with_context(|| format!("Failed to write config to {}", path.display()))?;

        Ok(())
    }

    /// Create a new config with the given tokens and settings.
    #[must_use]
    pub fn new(
        telegram_token: String,
        allowed_users: Vec<u64>,
        claude_api_key: String,
        vault_path: PathBuf,
        pi: Option<PiConfig>,
    ) -> Self {
        Self {
            telegram: TelegramConfig {
                bot_token: telegram_token,
                allowed_users,
            },
            claude: ClaudeConfig {
                api_key: claude_api_key,
                model: default_model(),
            },
            vault: VaultConfig { path: vault_path },
            pi,
        }
    }
}

/// Get the Ludolph config directory (~/.ludolph).
pub fn config_dir() -> PathBuf {
    directories::BaseDirs::new().map_or_else(
        || PathBuf::from("./.ludolph"),
        |d| d.home_dir().join(".ludolph"),
    )
}

/// Get the config file path.
pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}
