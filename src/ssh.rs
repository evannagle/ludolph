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

/// Run a command on Pi via SSH.
pub fn run(host: &str, user: &str, cmd: &str) -> Result<String> {
    let output = Command::new("ssh")
        .args([&format!("{user}@{host}"), cmd])
        .output()?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow!("Command failed: {}", stderr.trim()))
    }
}

/// Check if git is installed on Pi.
pub fn check_git(host: &str, user: &str) -> Result<bool> {
    run(host, user, "which git").map(|_| true).or(Ok(false))
}

/// Install git on Pi.
pub fn install_git(host: &str, user: &str) -> Result<()> {
    run(host, user, "sudo apt update && sudo apt install -y git")?;
    Ok(())
}

/// Check if SSH key exists on Pi.
pub fn has_ssh_key(host: &str, user: &str) -> Result<bool> {
    run(host, user, "test -f ~/.ssh/id_ed25519")
        .map(|_| true)
        .or(Ok(false))
}

/// Generate SSH key on Pi (for GitHub deploy key).
pub fn generate_ssh_key(host: &str, user: &str) -> Result<String> {
    // Generate key without passphrase
    run(
        host,
        user,
        "ssh-keygen -t ed25519 -f ~/.ssh/id_ed25519 -N '' -C 'ludolph-pi'",
    )?;

    // Return public key
    run(host, user, "cat ~/.ssh/id_ed25519.pub")
}

/// Test GitHub SSH access.
pub fn test_github_access(host: &str, user: &str) -> bool {
    let result = run(
        host,
        user,
        "ssh -o StrictHostKeyChecking=accept-new -T git@github.com 2>&1",
    );
    // GitHub returns exit code 1 but says "successfully authenticated"
    match result {
        Ok(output) => output.contains("successfully authenticated"),
        Err(e) => {
            let msg = e.to_string();
            msg.contains("successfully authenticated")
        }
    }
}

/// Clone repo to Pi.
pub fn clone_repo(host: &str, user: &str, repo_url: &str, dest: &str) -> Result<()> {
    let cmd = format!("git clone {repo_url} {dest}");
    run(host, user, &cmd)?;
    Ok(())
}

/// Check if path exists on Pi.
pub fn path_exists(host: &str, user: &str, path: &str) -> Result<bool> {
    run(host, user, &format!("test -e {path}"))
        .map(|_| true)
        .or(Ok(false))
}

/// Create symlink on Pi.
pub fn create_symlink(host: &str, user: &str, target: &str, link: &str) -> Result<()> {
    // Remove existing symlink if present
    let _ = run(host, user, &format!("rm -f {link}"));
    run(host, user, &format!("ln -s {target} {link}"))?;
    Ok(())
}

/// Git pull on Pi.
pub fn git_pull(host: &str, user: &str, repo_path: &str) -> Result<()> {
    run(host, user, &format!("cd {repo_path} && git pull"))?;
    Ok(())
}

/// Check if `git-lfs` is installed on Pi.
pub fn has_git_lfs(host: &str, user: &str) -> Result<bool> {
    run(host, user, "git lfs version")
        .map(|_| true)
        .or(Ok(false))
}

/// Install `git-lfs` on Pi.
pub fn install_git_lfs(host: &str, user: &str) -> Result<()> {
    run(host, user, "sudo apt install -y git-lfs && git lfs install")?;
    Ok(())
}

/// Pull `git-lfs` files (hydrate pointers to actual content).
pub fn git_lfs_pull(host: &str, user: &str, repo_path: &str) -> Result<()> {
    run(host, user, &format!("cd {repo_path} && git lfs pull"))?;
    Ok(())
}

/// Count files in a directory on Pi.
pub fn count_files(host: &str, user: &str, path: &str) -> Result<u64> {
    let output = run(host, user, &format!("find {path} -type f | wc -l"))?;
    output
        .trim()
        .parse()
        .map_err(|_| anyhow!("Could not parse file count"))
}

/// Stop Syncthing service on Pi.
pub fn stop_syncthing(host: &str, user: &str) {
    let _ = run(host, user, "systemctl --user stop syncthing");
}

/// Remove directories on Pi.
pub fn remove_dirs(host: &str, user: &str, paths: &[&str]) -> Result<()> {
    let paths_str = paths.join(" ");
    run(host, user, &format!("rm -rf {paths_str}"))?;
    Ok(())
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
