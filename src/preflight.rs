//! Pre-flight checks for sync setup.
//!
//! These checks run before any sync configuration to ensure:
//! - `gitleaks` is installed for secret detection
//! - Vault is inside a git repo
//! - No secrets are detected in the vault
//! - Git LFS files are hydrated (not pointers)

use anyhow::{Result, anyhow};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Check if gitleaks is installed.
pub fn check_gitleaks() -> Result<()> {
    let output = Command::new("which").arg("gitleaks").output()?;

    if !output.status.success() {
        return Err(anyhow!(
            "gitleaks not found\n\n\
             Required for secret detection before sync.\n\
             Install with: brew install gitleaks\n\n\
             Then run: lu sync setup"
        ));
    }
    Ok(())
}

/// Find the git repo root for a path (walks up to find .git).
pub fn find_repo_root(path: &Path) -> Result<PathBuf> {
    let output = Command::new("git")
        .args([
            "-C",
            &path.to_string_lossy(),
            "rev-parse",
            "--show-toplevel",
        ])
        .output()?;

    if !output.status.success() {
        return Err(anyhow!(
            "Vault is not inside a git repository\n\n\
             Initialize with:\n\
             cd {} && git init && git remote add origin <url>",
            path.display()
        ));
    }

    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(PathBuf::from(root))
}

/// Get vault subdirectory relative to repo root.
/// Returns `None` if vault path IS the repo root.
pub fn get_vault_subdir(vault_path: &Path, repo_root: &Path) -> Option<PathBuf> {
    vault_path
        .strip_prefix(repo_root)
        .ok()
        .filter(|p| !p.as_os_str().is_empty())
        .map(PathBuf::from)
}

/// Run gitleaks to detect secrets.
pub fn check_secrets(path: &Path) -> Result<()> {
    let output = Command::new("gitleaks")
        .args(["detect", "--source"])
        .arg(path)
        .args(["--no-git", "--exit-code", "1"])
        .output()?;

    // gitleaks exit codes: 0 = no secrets, 1 = secrets found, other = error
    if output.status.code() == Some(1) {
        let findings = String::from_utf8_lossy(&output.stdout);
        return Err(anyhow!(
            "Secrets detected in vault!\n\n\
             Remove or add to .gitignore before syncing:\n\n\
             {findings}"
        ));
    }
    Ok(())
}

/// Get git remote URL from repo root.
pub fn get_git_remote(repo_root: &Path) -> Result<String> {
    let output = Command::new("git")
        .args([
            "-C",
            &repo_root.to_string_lossy(),
            "remote",
            "get-url",
            "origin",
        ])
        .output()?;

    if !output.status.success() {
        return Err(anyhow!(
            "No git remote configured\n\n\
             Add one with: git remote add origin <url>"
        ));
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(url)
}

/// Convert HTTPS URL to SSH URL for cloning.
pub fn to_ssh_url(url: &str) -> String {
    if url.starts_with("https://github.com/") {
        url.replace("https://github.com/", "git@github.com:")
    } else {
        url.to_string()
    }
}

/// Check if repo uses git-lfs (has .gitattributes with filter=lfs).
pub fn uses_git_lfs(repo_root: &Path) -> bool {
    let gitattributes = repo_root.join(".gitattributes");
    gitattributes.exists()
        && std::fs::read_to_string(&gitattributes)
            .map(|s| s.contains("filter=lfs"))
            .unwrap_or(false)
}

/// Check if git-lfs files are hydrated (not pointers).
pub fn check_lfs_hydrated(repo_root: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["-C", &repo_root.to_string_lossy(), "lfs", "status"])
        .output()?;

    let status = String::from_utf8_lossy(&output.stdout);

    // If any files show as "not checked in", they may be pointers
    // A cleaner check: see if any LFS files exist that aren't hydrated
    if status.contains("Objects to be pushed") {
        // This is fine - just means there are LFS objects
        return Ok(());
    }

    // Check for actual pointer files by looking at ls-files
    let ls_output = Command::new("git")
        .args(["-C", &repo_root.to_string_lossy(), "lfs", "ls-files"])
        .output()?;

    let ls_status = String::from_utf8_lossy(&ls_output.stdout);

    // Files with '-' prefix are NOT checked out (still pointers)
    // Files with '*' prefix ARE checked out (hydrated)
    for line in ls_status.lines() {
        if line.starts_with('-') {
            return Err(anyhow!(
                "Git LFS files not hydrated locally.\n\n\
                 Run: cd {} && git lfs pull\n\n\
                 Then retry: lu sync setup",
                repo_root.display()
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_ssh_url_converts_https() {
        let https = "https://github.com/user/repo.git";
        let ssh = to_ssh_url(https);
        assert_eq!(ssh, "git@github.com:user/repo.git");
    }

    #[test]
    fn test_to_ssh_url_preserves_ssh() {
        let ssh = "git@github.com:user/repo.git";
        assert_eq!(to_ssh_url(ssh), ssh);
    }

    #[test]
    fn test_get_vault_subdir_returns_none_for_same_path() {
        let path = PathBuf::from("/home/user/vault");
        let result = get_vault_subdir(&path, &path);
        assert!(result.is_none());
    }

    #[test]
    fn test_get_vault_subdir_returns_subdir() {
        let repo = PathBuf::from("/home/user/Noggin");
        let vault = PathBuf::from("/home/user/Noggin/noggin");
        let result = get_vault_subdir(&vault, &repo);
        assert_eq!(result, Some(PathBuf::from("noggin")));
    }
}
