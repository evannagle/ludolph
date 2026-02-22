//! Syncthing installation and configuration for vault sync.
//!
//! Provides automated setup of Syncthing for real-time bidirectional
//! sync between Mac and Pi.

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use crate::ssh;

const API_PORT: u16 = 8384;
const FOLDER_ID: &str = "ludolph-vault";

/// Syncthing folder status from API.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderStatus {
    pub state: String,
    pub global_files: u64,
    pub global_bytes: u64,
    #[allow(dead_code)]
    pub local_files: u64,
    #[allow(dead_code)]
    pub local_bytes: u64,
}

/// Syncthing device configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceConfig {
    device_id: String,
    name: String,
    addresses: Vec<String>,
    #[serde(default)]
    auto_accept_folders: bool,
}

/// Syncthing folder configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FolderConfig {
    id: String,
    label: String,
    path: String,
    devices: Vec<FolderDevice>,
    #[serde(rename = "type")]
    folder_type: String,
    #[serde(default)]
    rescan_interval_s: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FolderDevice {
    device_id: String,
}

/// Full Syncthing config from API.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
struct SyncthingConfig {
    devices: Vec<DeviceConfig>,
    folders: Vec<FolderConfig>,
}

/// Result of an install/update operation.
#[derive(Debug)]
pub enum InstallResult {
    /// Fresh install completed.
    Installed(String),
    /// Updated from one version to another.
    Updated { from: String, to: String },
    /// Already at latest version.
    UpToDate(String),
    /// User chose to skip update.
    Skipped(String),
}

impl InstallResult {
    /// Get the current version after the operation.
    pub fn version(&self) -> &str {
        match self {
            Self::Installed(v) | Self::UpToDate(v) | Self::Skipped(v) => v,
            Self::Updated { to, .. } => to,
        }
    }
}

/// Check if Syncthing is installed on Mac.
pub fn is_installed_mac() -> bool {
    Command::new("which")
        .arg("syncthing")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get Syncthing version on Mac.
pub fn get_version_mac() -> Option<String> {
    Command::new("syncthing")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .nth(1) // "syncthing v1.27.0 ..."
                .map(|s| s.trim_start_matches('v').to_string())
        })
}

/// Get Syncthing version on Pi.
pub fn get_version_pi(host: &str, user: &str) -> Option<String> {
    ssh::run(host, user, "syncthing --version 2>/dev/null")
        .ok()
        .and_then(|out| {
            out.split_whitespace()
                .nth(1)
                .map(|s| s.trim_start_matches('v').to_string())
        })
}

/// Get latest available Syncthing version from Homebrew.
pub fn get_latest_version_brew() -> Option<String> {
    Command::new("brew")
        .args(["info", "--json=v2", "syncthing"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
        .and_then(|v| {
            v["formulae"][0]["versions"]["stable"]
                .as_str()
                .map(String::from)
        })
}

/// Check if Homebrew is available.
fn has_homebrew() -> bool {
    Command::new("which")
        .arg("brew")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Install Syncthing on Mac via Homebrew.
pub fn install_mac() -> Result<()> {
    if !has_homebrew() {
        return Err(anyhow!("Homebrew not found. Install from https://brew.sh"));
    }

    if !is_installed_mac() {
        let output = Command::new("brew")
            .args(["install", "syncthing"])
            .output()
            .context("Failed to run brew install")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("brew install syncthing failed: {stderr}"));
        }
    }

    // Start as background service (suppress output)
    let output = Command::new("brew")
        .args(["services", "start", "syncthing"])
        .output()
        .context("Failed to start syncthing service")?;

    if !output.status.success() {
        // Check if it's just "already started" which is fine
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("already started") {
            return Err(anyhow!("Failed to start syncthing service"));
        }
    }

    Ok(())
}

/// Ensure Syncthing is installed on Mac, optionally updating if exists.
pub fn ensure_installed_mac(update_if_exists: bool) -> Result<InstallResult> {
    let current = get_version_mac();

    match current {
        None => {
            // Not installed - install it
            install_mac()?;
            let version = get_version_mac().unwrap_or_else(|| "unknown".to_string());
            Ok(InstallResult::Installed(version))
        }
        Some(v) if update_if_exists => {
            // Installed - check for update
            let latest = get_latest_version_brew();
            if latest.as_ref() == Some(&v) {
                Ok(InstallResult::UpToDate(v))
            } else {
                // Upgrade via Homebrew
                let output = Command::new("brew")
                    .args(["upgrade", "syncthing"])
                    .output()
                    .context("Failed to run brew upgrade")?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    // "already installed" or "already the newest version" is fine
                    if !stderr.contains("already") {
                        return Err(anyhow!("brew upgrade syncthing failed: {stderr}"));
                    }
                }

                let new_version = get_version_mac().unwrap_or_else(|| "unknown".to_string());
                if new_version == v {
                    Ok(InstallResult::UpToDate(v))
                } else {
                    Ok(InstallResult::Updated {
                        from: v,
                        to: new_version,
                    })
                }
            }
        }
        Some(v) => {
            // Installed - skip (but ensure service running)
            let _ = Command::new("brew")
                .args(["services", "start", "syncthing"])
                .output();
            Ok(InstallResult::Skipped(v))
        }
    }
}

/// Ensure Syncthing is installed on Pi, optionally updating if exists.
pub fn ensure_installed_pi(
    host: &str,
    user: &str,
    update_if_exists: bool,
) -> Result<InstallResult> {
    let current = get_version_pi(host, user);

    match current {
        None => {
            // Not installed - install it
            ssh::run(
                host,
                user,
                "sudo apt update && sudo apt install -y syncthing",
            )
            .context("Failed to install syncthing on Pi")?;

            ssh::run(host, user, "systemctl --user enable --now syncthing")
                .context("Failed to enable syncthing service")?;

            let version = get_version_pi(host, user).unwrap_or_else(|| "unknown".to_string());
            Ok(InstallResult::Installed(version))
        }
        Some(v) if update_if_exists => {
            // Check for update via apt
            ssh::run(
                host,
                user,
                "sudo apt update && sudo apt upgrade -y syncthing",
            )?;

            let new_version = get_version_pi(host, user).unwrap_or_else(|| "unknown".to_string());
            if new_version == v {
                Ok(InstallResult::UpToDate(v))
            } else {
                Ok(InstallResult::Updated {
                    from: v,
                    to: new_version,
                })
            }
        }
        Some(v) => {
            // Installed - skip (but ensure service running)
            let _ = ssh::run(host, user, "systemctl --user start syncthing");
            Ok(InstallResult::Skipped(v))
        }
    }
}

/// Wait for Syncthing API to be ready.
pub fn wait_for_api(api_key: &str, timeout: Duration) -> Result<()> {
    let start = Instant::now();
    let url = format!("http://127.0.0.1:{API_PORT}/rest/system/status");

    while start.elapsed() < timeout {
        let result = reqwest::blocking::Client::new()
            .get(&url)
            .header("X-API-Key", api_key)
            .timeout(Duration::from_secs(2))
            .send();

        if result.is_ok() {
            return Ok(());
        }

        std::thread::sleep(Duration::from_millis(500));
    }

    Err(anyhow!("Syncthing API did not become ready"))
}

/// Get API key from Syncthing config file.
pub fn get_api_key_mac() -> Result<String> {
    let config_path = dirs::home_dir()
        .ok_or_else(|| anyhow!("Could not find home directory"))?
        .join("Library/Application Support/Syncthing/config.xml");

    if !config_path.exists() {
        return Err(anyhow!(
            "Syncthing config not found at {}",
            config_path.display()
        ));
    }

    let content =
        std::fs::read_to_string(&config_path).context("Failed to read Syncthing config")?;

    // Parse API key from XML (simple regex extraction)
    let re = regex::Regex::new(r"<apikey>([^<]+)</apikey>").expect("valid regex");

    re.captures(&content)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .ok_or_else(|| anyhow!("API key not found in Syncthing config"))
}

/// Get API key from Pi's Syncthing config.
pub fn get_api_key_pi(host: &str, user: &str) -> Result<String> {
    let output = ssh::run(
        host,
        user,
        "cat ~/.config/syncthing/config.xml 2>/dev/null || cat ~/.local/state/syncthing/config.xml 2>/dev/null",
    )?;

    let re = regex::Regex::new(r"<apikey>([^<]+)</apikey>").expect("valid regex");

    re.captures(&output)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .ok_or_else(|| anyhow!("API key not found in Pi's Syncthing config"))
}

/// Get device ID from local Syncthing API.
pub fn get_device_id(api_key: &str) -> Result<String> {
    let url = format!("http://127.0.0.1:{API_PORT}/rest/system/status");

    let resp: serde_json::Value = reqwest::blocking::Client::new()
        .get(&url)
        .header("X-API-Key", api_key)
        .send()
        .context("Failed to connect to Syncthing API")?
        .json()
        .context("Failed to parse Syncthing response")?;

    resp["myID"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| anyhow!("Device ID not found in response"))
}

/// Get device ID from Pi's Syncthing.
pub fn get_device_id_pi(host: &str, user: &str, api_key: &str) -> Result<String> {
    // Use SSH tunnel to access Pi's Syncthing API
    let cmd =
        format!("curl -s -H 'X-API-Key: {api_key}' http://127.0.0.1:{API_PORT}/rest/system/status");

    let output = ssh::run(host, user, &cmd)?;
    let resp: serde_json::Value =
        serde_json::from_str(&output).context("Failed to parse Pi Syncthing response")?;

    resp["myID"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| anyhow!("Device ID not found in Pi response"))
}

/// Add a device to local Syncthing.
pub fn add_device(api_key: &str, device_id: &str, name: &str) -> Result<()> {
    let url = format!("http://127.0.0.1:{API_PORT}/rest/config/devices");

    let device = DeviceConfig {
        device_id: device_id.to_string(),
        name: name.to_string(),
        addresses: vec!["dynamic".to_string()],
        auto_accept_folders: true,
    };

    let resp = reqwest::blocking::Client::new()
        .post(&url)
        .header("X-API-Key", api_key)
        .json(&device)
        .send()
        .context("Failed to add device")?;

    if !resp.status().is_success() {
        let text = resp.text().unwrap_or_default();
        // Ignore "already exists" errors
        if !text.contains("already exists") {
            return Err(anyhow!("Failed to add device: {text}"));
        }
    }

    Ok(())
}

/// Add a device to Pi's Syncthing.
pub fn add_device_pi(
    host: &str,
    user: &str,
    api_key: &str,
    device_id: &str,
    name: &str,
) -> Result<()> {
    let device = DeviceConfig {
        device_id: device_id.to_string(),
        name: name.to_string(),
        addresses: vec!["dynamic".to_string()],
        auto_accept_folders: true,
    };

    let json = serde_json::to_string(&device)?;
    let cmd = format!(
        "curl -s -X POST -H 'X-API-Key: {api_key}' -H 'Content-Type: application/json' \
         -d '{}' http://127.0.0.1:{API_PORT}/rest/config/devices",
        json.replace('\'', "'\\''")
    );

    let _ = ssh::run(host, user, &cmd)?;
    Ok(())
}

/// Create a shared folder on local Syncthing.
pub fn create_folder(api_key: &str, path: &str, devices: &[&str]) -> Result<()> {
    let url = format!("http://127.0.0.1:{API_PORT}/rest/config/folders");

    let folder = FolderConfig {
        id: FOLDER_ID.to_string(),
        label: "Ludolph Vault".to_string(),
        path: path.to_string(),
        devices: devices
            .iter()
            .map(|d| FolderDevice {
                device_id: (*d).to_string(),
            })
            .collect(),
        folder_type: "sendreceive".to_string(),
        rescan_interval_s: 60,
    };

    let resp = reqwest::blocking::Client::new()
        .post(&url)
        .header("X-API-Key", api_key)
        .json(&folder)
        .send()
        .context("Failed to create folder")?;

    if !resp.status().is_success() {
        let text = resp.text().unwrap_or_default();
        if !text.contains("already exists") {
            return Err(anyhow!("Failed to create folder: {text}"));
        }
    }

    Ok(())
}

/// Create a shared folder on Pi's Syncthing.
pub fn create_folder_pi(
    host: &str,
    user: &str,
    api_key: &str,
    path: &str,
    devices: &[&str],
) -> Result<()> {
    // Expand ~ to absolute path (Syncthing API needs absolute paths)
    let abs_path = if path.starts_with('~') {
        let home_cmd = "echo $HOME";
        let home = ssh::run(host, user, home_cmd)?.trim().to_string();
        path.replacen('~', &home, 1)
    } else {
        path.to_string()
    };

    // Ensure the vault directory exists
    let mkdir_cmd = format!("mkdir -p {abs_path}");
    ssh::run(host, user, &mkdir_cmd)?;

    let folder = FolderConfig {
        id: FOLDER_ID.to_string(),
        label: "Ludolph Vault".to_string(),
        path: abs_path,
        devices: devices
            .iter()
            .map(|d| FolderDevice {
                device_id: (*d).to_string(),
            })
            .collect(),
        folder_type: "sendreceive".to_string(),
        rescan_interval_s: 60,
    };

    let json = serde_json::to_string(&folder)?;
    let cmd = format!(
        "curl -s -X POST -H 'X-API-Key: {api_key}' -H 'Content-Type: application/json' \
         -d '{}' http://127.0.0.1:{API_PORT}/rest/config/folders",
        json.replace('\'', "'\\''")
    );

    let _ = ssh::run(host, user, &cmd)?;
    Ok(())
}

/// Get folder sync status.
pub fn get_folder_status(api_key: &str) -> Result<FolderStatus> {
    let url = format!("http://127.0.0.1:{API_PORT}/rest/db/status?folder={FOLDER_ID}");

    let resp: FolderStatus = reqwest::blocking::Client::new()
        .get(&url)
        .header("X-API-Key", api_key)
        .send()
        .context("Failed to get folder status")?
        .json()
        .context("Failed to parse folder status")?;

    Ok(resp)
}

/// First 200 digits of pi (enough for ~3 minutes of animation at 1 tick/sec).
const PI_DIGITS: &str = "31415926535897932384626433832795028841971693993751058209749445923078164062862089986280348253421170679821480865132823066470938446095505822317253594081284811174502841027019385211055596446229489549303819644288109756659334461284756482337867831652712019091";

/// Generate tick strings for pi spinner: [00000] → [00003] → [0003.] → [003.1] → [03.14] → [3.141] → [14159] → ...
fn pi_tick_string(tick: usize) -> String {
    // Intro sequence with decimal point
    let intro = ["00000", "00003", "0003.", "003.1", "03.14", "3.141"];

    if tick < intro.len() {
        return intro[tick].to_string();
    }

    // After intro: sliding window through pi digits (no cycling, stop at end)
    let idx = tick - intro.len();
    if idx + 5 <= PI_DIGITS.len() {
        PI_DIGITS[idx..idx + 5].to_string()
    } else {
        // Stay at the last valid window
        PI_DIGITS[PI_DIGITS.len() - 5..].to_string()
    }
}

/// Wait for initial sync to complete (or timeout), showing progress.
pub fn wait_for_sync(api_key: &str, timeout: Duration) -> Result<FolderStatus> {
    use std::io::{self, Write};

    let start = Instant::now();
    let url = format!("http://127.0.0.1:{API_PORT}/rest/db/status?folder={FOLDER_ID}");
    let mut tick: usize = 0;

    loop {
        if start.elapsed() > timeout {
            println!();
            return get_folder_status(api_key);
        }

        let resp: serde_json::Value = reqwest::blocking::Client::new()
            .get(&url)
            .header("X-API-Key", api_key)
            .send()
            .context("Failed to get sync status")?
            .json()
            .context("Failed to parse sync status")?;

        let state = resp["state"].as_str().unwrap_or("unknown");
        let global_bytes = resp["globalBytes"].as_u64().unwrap_or(0);
        let in_sync_bytes = resp["inSyncBytes"].as_u64().unwrap_or(0);

        // Integer percentage (saturating to prevent overflow)
        let percent = if global_bytes > 0 {
            in_sync_bytes.saturating_mul(100) / global_bytes
        } else {
            0
        };

        // Get animated pi digits (zeros → pi sliding in → cycling)
        let pi_window = pi_tick_string(tick);
        tick += 1;

        let synced_str = format_size(in_sync_bytes);
        let total_str = format_size(global_bytes);
        let elapsed = start.elapsed().as_secs();

        // Match the [•??] style used elsewhere
        print!(
            "\r  [•{}] Syncing... {}% | {}/{} ({:02}:{:02})    ",
            pi_window,
            percent,
            synced_str,
            total_str,
            elapsed / 60,
            elapsed % 60
        );
        let _ = io::stdout().flush();

        // "idle" means sync is complete
        if state == "idle" {
            // Clear the progress line completely
            print!("\r                                                                    \r");
            let _ = io::stdout().flush();
            return get_folder_status(api_key);
        }

        std::thread::sleep(Duration::from_secs(1));
    }
}

/// Get vault size in bytes.
pub fn get_vault_size(vault_path: &Path) -> Result<u64> {
    let output = Command::new("du")
        .args(["-sk", vault_path.to_str().unwrap_or(".")])
        .output()
        .context("Failed to get vault size")?;

    if !output.status.success() {
        return Err(anyhow!("Failed to get vault size"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let size_kb: u64 = stdout
        .split_whitespace()
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    Ok(size_kb * 1024) // Convert KB to bytes
}

/// Get available disk space on Pi in bytes for a given path.
pub fn get_pi_available_space(host: &str, user: &str, pi_path: &str) -> Result<u64> {
    // Get available space at the specified path
    let cmd = format!("df -k {pi_path} 2>/dev/null | tail -1 | awk '{{print $4}}'");
    let output = ssh::run(host, user, &cmd)?;
    let size_kb: u64 = output.trim().parse().unwrap_or(0);
    Ok(size_kb * 1024) // Convert KB to bytes
}

/// Check if Pi has enough space for vault (with 20% buffer).
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub fn check_space(
    vault_path: &Path,
    host: &str,
    user: &str,
    pi_path: &str,
) -> Result<(u64, u64, bool)> {
    let vault_size = get_vault_size(vault_path)?;
    let available = get_pi_available_space(host, user, pi_path)?;

    // Require 20% extra space as buffer
    let required = (vault_size as f64 * 1.2) as u64;
    let has_space = available >= required;

    Ok((vault_size, available, has_space))
}

/// Format bytes as human-readable size.
#[allow(clippy::cast_precision_loss)]
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// Verify sync is working by creating test files.
pub fn verify_sync(
    vault_path: &Path,
    host: &str,
    user: &str,
    pi_vault_path: &str,
) -> Result<(Duration, Duration)> {
    let test_file = vault_path.join(".ludolph-sync-test");
    let pi_test_file = vault_path.join(".ludolph-sync-test-pi");

    // Mac → Pi test
    std::fs::write(&test_file, "sync-test-mac")?;

    let mac_to_pi_start = Instant::now();
    let mac_to_pi_duration;

    loop {
        if mac_to_pi_start.elapsed() > Duration::from_secs(60) {
            let _ = std::fs::remove_file(&test_file);
            return Err(anyhow!("Timeout waiting for Mac → Pi sync"));
        }

        let cmd = format!("cat {pi_vault_path}/.ludolph-sync-test 2>/dev/null");
        let result = ssh::run(host, user, &cmd);
        if result.is_ok() {
            mac_to_pi_duration = mac_to_pi_start.elapsed();
            break;
        }

        std::thread::sleep(Duration::from_millis(500));
    }

    // Pi → Mac test
    let write_cmd = format!("echo 'sync-test-pi' > {pi_vault_path}/.ludolph-sync-test-pi");
    ssh::run(host, user, &write_cmd)?;

    let pi_to_mac_start = Instant::now();
    let pi_to_mac_duration;

    loop {
        if pi_to_mac_start.elapsed() > Duration::from_secs(60) {
            let _ = std::fs::remove_file(&test_file);
            let cleanup = format!(
                "rm -f {pi_vault_path}/.ludolph-sync-test {pi_vault_path}/.ludolph-sync-test-pi"
            );
            let _ = ssh::run(host, user, &cleanup);
            return Err(anyhow!("Timeout waiting for Pi → Mac sync"));
        }

        if pi_test_file.exists() {
            pi_to_mac_duration = pi_to_mac_start.elapsed();
            break;
        }

        std::thread::sleep(Duration::from_millis(500));
    }

    // Cleanup
    let _ = std::fs::remove_file(&test_file);
    let _ = std::fs::remove_file(&pi_test_file);
    let cleanup =
        format!("rm -f {pi_vault_path}/.ludolph-sync-test {pi_vault_path}/.ludolph-sync-test-pi");
    let _ = ssh::run(host, user, &cleanup);

    Ok((mac_to_pi_duration, pi_to_mac_duration))
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_api_key_regex() {
        let xml = r#"<configuration><gui><apikey>test-key-123</apikey></gui></configuration>"#;
        let re = regex::Regex::new(r"<apikey>([^<]+)</apikey>").unwrap();
        let key = re.captures(xml).and_then(|c| c.get(1)).map(|m| m.as_str());
        assert_eq!(key, Some("test-key-123"));
    }
}
