//! Configuration loading and management.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Ludolph configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub telegram: TelegramConfig,
    pub claude: ClaudeConfig,
    /// LLM configuration (new style - uses MCP proxy)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm: Option<LlmConfig>,
    /// Local vault path (only needed on Mac, not on Pi with MCP)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vault: Option<VaultConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pi: Option<PiConfig>,
    /// MCP server configuration (used by Pi thin client to connect to Mac)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp: Option<McpConfig>,
    /// Memory configuration for conversation context
    #[serde(default)]
    pub memory: MemoryConfig,
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

/// LLM configuration (new style - provider-agnostic).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// Model identifier (e.g., "claude-sonnet-4", "gpt-4o", "ollama/llama3")
    #[serde(default = "default_model")]
    pub model: String,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            model: default_model(),
        }
    }
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

/// MCP server connection configuration (for Pi thin client).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    /// MCP server URL (e.g., `http://mac.local:8200`)
    pub url: String,
    /// Authentication token for MCP server
    pub auth_token: String,
    /// MAC address for Wake-on-LAN (e.g., "a4:83:e7:xx:xx:xx")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac_address: Option<String>,
}

/// Memory configuration for conversation context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Number of recent messages to include in context (default: 8)
    #[serde(default = "default_window_size")]
    pub window_size: usize,
    /// Persist to vault when this many messages accumulate (default: 16)
    #[serde(default = "default_persist_threshold")]
    pub persist_threshold: usize,
    /// Maximum bytes of context to include (default: 32KB, protects Pi memory)
    /// Messages are trimmed from oldest first if this limit is exceeded.
    #[serde(default = "default_max_context_bytes")]
    pub max_context_bytes: usize,
}

const fn default_window_size() -> usize {
    8
}

const fn default_persist_threshold() -> usize {
    16
}

const fn default_max_context_bytes() -> usize {
    32 * 1024 // 32KB default - conservative for Pi
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            window_size: default_window_size(),
            persist_threshold: default_persist_threshold(),
            max_context_bytes: default_max_context_bytes(),
        }
    }
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
        vault_path: Option<PathBuf>,
        pi: Option<PiConfig>,
        mcp: Option<McpConfig>,
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
            llm: None, // Uses claude config by default for backward compatibility
            vault: vault_path.map(|path| VaultConfig { path }),
            pi,
            mcp,
            memory: MemoryConfig::default(),
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
