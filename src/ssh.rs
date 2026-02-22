//! SSH operations for Pi connectivity.

use anyhow::{Result, anyhow};
use std::process::Command;

/// Test SSH connection to Pi.
///
/// Uses `BatchMode=yes` to fail immediately if key auth isn't set up,
/// and `ConnectTimeout=5` to avoid long waits.
pub fn test_connection(host: &str, user: &str) -> Result<()> {
    let status = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=5",
            "-o",
            "StrictHostKeyChecking=accept-new",
            &format!("{user}@{host}"),
            "echo ok",
        ])
        .output()?;

    if status.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&status.stderr);
        Err(anyhow!("SSH connection failed: {}", stderr.trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_fails_on_invalid_host() {
        // Should fail fast on nonexistent host
        let result = test_connection("nonexistent.invalid.local", "pi");
        assert!(result.is_err());
    }
}
