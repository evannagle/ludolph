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
    /// Channel API configuration for Claude Code communication
    #[serde(default)]
    pub channel: ChannelConfig,
    /// Memory configuration for conversation context
    #[serde(default)]
    pub memory: MemoryConfig,
    /// Focus configuration for file tracking
    #[serde(default)]
    pub focus: FocusConfig,
    /// Scheduler configuration for automated tasks
    #[serde(default)]
    pub scheduler: SchedulerConfig,
    /// Index configuration for vault file indexing
    #[serde(default)]
    pub index: IndexConfig,
    /// User's timezone (e.g., `Pacific/Honolulu`, `America/New_York`)
    #[serde(default = "default_timezone")]
    pub timezone: String,
}

fn default_timezone() -> String {
    detect_timezone()
}

/// Detect the system timezone from environment or system config.
pub fn detect_timezone() -> String {
    // Check TZ environment variable first
    if let Ok(tz) = std::env::var("TZ") {
        if !tz.is_empty() {
            return tz;
        }
    }

    // macOS: read from /etc/localtime symlink
    #[cfg(target_os = "macos")]
    {
        if let Ok(link) = std::fs::read_link("/etc/localtime") {
            let path = link.to_string_lossy();
            if let Some(tz) = path.strip_prefix("/var/db/timezone/zoneinfo/") {
                return tz.to_string();
            }
        }
    }

    // Linux: read /etc/timezone
    #[cfg(target_os = "linux")]
    {
        if let Ok(tz) = std::fs::read_to_string("/etc/timezone") {
            let tz = tz.trim();
            if !tz.is_empty() {
                return tz.to_string();
            }
        }
    }

    "UTC".to_string()
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
    /// MCP server URL (e.g., `http://mac.local:8200` or Tailscale IP)
    pub url: String,
    /// Fallback URL if primary fails (e.g., LAN IP when Tailscale is down)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_url: Option<String>,
    /// Authentication token for MCP server
    pub auth_token: String,
    /// MAC address for Wake-on-LAN (e.g., "a4:83:e7:xx:xx:xx")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac_address: Option<String>,
}

/// Channel API configuration for Claude Code communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    /// Port for the channel API server (default: 8202)
    #[serde(default = "default_channel_port")]
    pub port: u16,
    /// Authentication token for channel API (required for security)
    #[serde(default)]
    pub auth_token: String,
}

/// Default port for the channel API server.
pub const DEFAULT_CHANNEL_PORT: u16 = 8202;

const fn default_channel_port() -> u16 {
    DEFAULT_CHANNEL_PORT
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            port: default_channel_port(),
            auth_token: String::new(),
        }
    }
}

impl ChannelConfig {
    /// Load channel config from environment variables with fallback to defaults.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            port: std::env::var("LU_CHANNEL_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(default_channel_port),
            auth_token: std::env::var("LU_CHANNEL_AUTH_TOKEN").unwrap_or_default(),
        }
    }
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

/// Focus configuration for file tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusConfig {
    /// Maximum files to keep in focus per user (default: 5)
    #[serde(default = "default_max_focus_files")]
    pub max_files: usize,
    /// Files expire after this many seconds (default: 3600 = 1 hour)
    #[serde(default = "default_focus_max_age_secs")]
    pub max_age_secs: u64,
    /// Characters to store for preview (default: 500)
    #[serde(default = "default_focus_preview_chars")]
    pub preview_chars: usize,
}

const fn default_max_focus_files() -> usize {
    5
}

const fn default_focus_max_age_secs() -> u64 {
    3600 // 1 hour
}

const fn default_focus_preview_chars() -> usize {
    500
}

impl Default for FocusConfig {
    fn default() -> Self {
        Self {
            max_files: default_max_focus_files(),
            max_age_secs: default_focus_max_age_secs(),
            preview_chars: default_focus_preview_chars(),
        }
    }
}

/// Scheduler configuration for automated tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    /// How often to check for due schedules in seconds (default: 60)
    #[serde(default = "default_scheduler_check_interval")]
    pub check_interval_secs: u64,
    /// Maximum concurrent schedule executions (default: 3)
    #[serde(default = "default_scheduler_max_concurrent")]
    pub max_concurrent: usize,
}

const fn default_scheduler_check_interval() -> u64 {
    60
}

const fn default_scheduler_max_concurrent() -> usize {
    3
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            check_interval_secs: default_scheduler_check_interval(),
            max_concurrent: default_scheduler_max_concurrent(),
        }
    }
}

/// Index tier controls how deeply files are processed during indexing.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IndexTier {
    Quick,
    Standard,
    Deep,
}

impl Default for IndexTier {
    fn default() -> Self {
        Self::Standard
    }
}

impl std::fmt::Display for IndexTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Quick => write!(f, "quick"),
            Self::Standard => write!(f, "standard"),
            Self::Deep => write!(f, "deep"),
        }
    }
}

/// Index configuration for vault file indexing.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexConfig {
    #[serde(default)]
    pub tier: IndexTier,
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
        let mut config: Self =
            toml::from_str(&contents).with_context(|| "Failed to parse config.toml")?;

        // If no channel auth token configured, try token file
        if config.channel.auth_token.is_empty() {
            let token_path = config_dir().join("channel_token");
            if let Ok(content) = std::fs::read_to_string(&token_path) {
                let trimmed = content.trim().to_string();
                if !trimmed.is_empty() {
                    config.channel.auth_token = trimmed;
                }
            }
        }

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
            channel: ChannelConfig::from_env(),
            memory: MemoryConfig::default(),
            focus: FocusConfig::default(),
            scheduler: SchedulerConfig::default(),
            index: IndexConfig::default(),
            timezone: detect_timezone(),
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
