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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync: Option<SyncConfig>,
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
    /// Path to the vault directory (may be subdir of repo).
    #[serde(default = "default_vault_path")]
    pub path: PathBuf,
    /// Git repo root (if vault is a subdirectory). Defaults to vault path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<PathBuf>,
}

impl VaultConfig {
    /// Get the repo root (defaults to vault path if not set).
    #[must_use]
    #[allow(dead_code)] // Used by future sync operations
    pub fn repo_root(&self) -> &std::path::Path {
        self.repo_root.as_ref().unwrap_or(&self.path)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiConfig {
    pub host: String,
    pub user: String,
}

/// Sync configuration for Syncthing-based vault sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    /// Whether sync is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Mac's Syncthing device ID.
    pub mac_device_id: String,
    /// Pi's Syncthing device ID.
    pub pi_device_id: String,
    /// Syncthing folder ID.
    pub folder_id: String,
    /// Where to clone the full repo on Pi (e.g., "~/Noggin").
    pub pi_repo_path: String,
    /// Vault path on Pi for Syncthing (e.g., "~/Noggin/noggin").
    pub pi_vault_path: String,
    /// Optional symlink for convenience (e.g., "~/noggin" -> `pi_vault_path`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pi_symlink: Option<String>,
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
            repo_root: None,
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
            vault: VaultConfig {
                path: vault_path,
                repo_root: None,
            },
            pi,
            sync: None,
        }
    }

    /// Set sync configuration.
    #[must_use]
    pub fn with_sync(mut self, sync: Option<SyncConfig>) -> Self {
        self.sync = sync;
        self
    }
}

/// Get the Ludolph config directory (~/.ludolph or ~/ludolph).
pub fn config_dir() -> PathBuf {
    directories::BaseDirs::new().map_or_else(
        || PathBuf::from("./ludolph"),
        |d| d.home_dir().join("ludolph"),
    )
}

/// Get the config file path.
pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}
