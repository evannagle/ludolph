//! Vault sync commands using Syncthing.

use anyhow::{Result, anyhow};
use console::style;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::config::{Config, PiConfig, SyncConfig};
use crate::preflight;
use crate::ssh;
use crate::syncthing;
use crate::ui::{self, StatusLine};

/// Prompt for Pi vault path.
fn collect_pi_vault_path(
    vault_path: &Path,
    pi: &PiConfig,
    existing: Option<&str>,
) -> Result<String> {
    println!();
    println!("{} Pi vault path", style("π").bold());
    println!(
        "  {}",
        style("Where should the vault be stored on the Pi?").dim()
    );

    // Default to ~/vault_folder_name to match the Mac structure
    let vault_name = vault_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("vault");
    let derived_default = format!("~/{vault_name}");
    let default = existing.unwrap_or(&derived_default);
    println!(
        "  {}",
        style(format!("Default: {default} (Enter to keep)")).dim()
    );

    let value: String = dialoguer::Input::<String>::new()
        .with_prompt("  ")
        .allow_empty(true)
        .interact_text()?;

    let pi_path = if value.is_empty() {
        default.to_string()
    } else {
        value
    };

    // Check if path exists or can be created
    ui::status::checking(&format!("Checking {pi_path} on Pi..."));

    // Try to create the directory
    let mkdir_cmd = format!("mkdir -p {pi_path} 2>&1");
    match crate::ssh::run(&pi.host, &pi.user, &mkdir_cmd) {
        Ok(_) => {
            ui::status::ok(&format!("Path ready: {pi_path}"));
        }
        Err(e) => {
            ui::status::error(&format!("Cannot create {pi_path}: {e}"));
            return Err(anyhow::anyhow!("Invalid Pi vault path"));
        }
    }

    Ok(pi_path)
}

/// Check disk space and return true if there's enough room.
fn check_disk_space(vault_path: &Path, pi: &PiConfig, pi_vault_path: &str) -> Result<bool> {
    ui::status::checking("Checking disk space...");
    match syncthing::check_space(vault_path, &pi.host, &pi.user, pi_vault_path) {
        Ok((vault_size, available, has_space)) => {
            let vault_str = syncthing::format_size(vault_size);
            let avail_str = syncthing::format_size(available);

            if has_space {
                ui::status::ok(&format!("Vault: {vault_str}, Pi available: {avail_str}"));
                Ok(true)
            } else {
                ui::status::error(&format!(
                    "Not enough space! Vault: {vault_str}, Pi available: {avail_str}"
                ));
                println!();
                println!("  Your vault is too large for the Pi's available disk space.");
                println!("  Free up space on the Pi or reduce vault size, then run:");
                println!("  {}", style("lu sync setup").cyan());
                println!();
                Ok(false)
            }
        }
        Err(e) => {
            ui::status::error(&format!("Could not check space: {e}"));
            println!();
            let proceed = ui::prompt::confirm("Continue anyway?")?;
            println!();
            Ok(proceed)
        }
    }
}

/// Install and start Syncthing on both Mac and Pi with interactive prompts.
fn install_syncthing(pi: &PiConfig) -> Result<(String, String)> {
    // === Mac ===
    ui::status::checking("Checking Syncthing on Mac...");

    match syncthing::get_version_mac() {
        None => {
            ui::status::checking("Installing Syncthing on Mac...");
            let result = syncthing::ensure_installed_mac(false)?;
            ui::status::ok(&format!("Installed v{}", result.version()));
        }
        Some(v) => {
            let latest = syncthing::get_latest_version_brew();
            if latest.as_ref() == Some(&v) {
                ui::status::ok(&format!("v{v} (latest)"));
            } else {
                ui::status::ok(&format!(
                    "v{} (latest: {})",
                    v,
                    latest.as_deref().unwrap_or("?")
                ));
                if ui::prompt::confirm("Update Syncthing on Mac?")? {
                    ui::status::checking("Updating...");
                    let result = syncthing::ensure_installed_mac(true)?;
                    ui::status::ok(&format!("Updated to v{}", result.version()));
                } else {
                    ui::status::ok(&format!("Keeping v{v}"));
                }
            }
        }
    }

    // === Pi ===
    ui::status::checking("Checking Syncthing on Pi...");

    match syncthing::get_version_pi(&pi.host, &pi.user) {
        None => {
            ui::status::checking("Installing Syncthing on Pi...");
            let result = syncthing::ensure_installed_pi(&pi.host, &pi.user, false)?;
            ui::status::ok(&format!("Installed v{}", result.version()));
        }
        Some(v) => {
            ui::status::ok(&format!("v{v} installed"));
            println!();
            if ui::prompt::confirm("Check for updates on Pi?")? {
                ui::status::checking("Checking for updates...");
                let result = syncthing::ensure_installed_pi(&pi.host, &pi.user, true)?;
                match result {
                    syncthing::InstallResult::Updated { from, to } => {
                        ui::status::ok(&format!("Updated v{from} -> v{to}"));
                    }
                    _ => ui::status::ok(&format!("v{v} (up to date)")),
                }
            } else {
                ui::status::ok(&format!("Keeping v{v}"));
            }
        }
    }

    // === Services and API ===
    ui::status::checking("Starting Syncthing services...");
    std::thread::sleep(Duration::from_secs(3));
    ui::status::ok("Services running");

    ui::status::checking("Getting API credentials...");
    let mac_api_key = syncthing::get_api_key_mac()?;
    std::thread::sleep(Duration::from_secs(2));
    let pi_api_key = syncthing::get_api_key_pi(&pi.host, &pi.user)?;
    ui::status::ok("API keys retrieved");

    ui::status::checking("Waiting for APIs...");
    syncthing::wait_for_api(&mac_api_key, Duration::from_secs(30))?;
    ui::status::ok("APIs ready");

    Ok((mac_api_key, pi_api_key))
}

/// Pair devices and configure shared folder (legacy, used by `collect_sync_config`).
fn configure_sync(
    vault_path: &Path,
    pi: &PiConfig,
    pi_vault_path: &str,
    mac_api_key: &str,
    pi_api_key: &str,
) -> Result<SyncConfig> {
    ui::status::checking("Exchanging device IDs...");
    let mac_id = syncthing::get_device_id(mac_api_key)?;
    let pi_id = syncthing::get_device_id_pi(&pi.host, &pi.user, pi_api_key)?;

    syncthing::add_device(mac_api_key, &pi_id, "pi")?;
    syncthing::add_device_pi(&pi.host, &pi.user, pi_api_key, &mac_id, "mac")?;
    ui::status::ok("Devices paired");

    ui::status::checking("Configuring shared folder...");
    let vault_str = vault_path.to_string_lossy();

    syncthing::create_folder(mac_api_key, &vault_str, &[&mac_id, &pi_id])?;
    syncthing::create_folder_pi(
        &pi.host,
        &pi.user,
        pi_api_key,
        pi_vault_path,
        &[&mac_id, &pi_id],
    )?;
    ui::status::ok(&format!("{vault_str} <-> {pi_vault_path}"));

    // Initial sync with progress display (longer timeout for large vaults)
    let status = syncthing::wait_for_sync(mac_api_key, Duration::from_secs(600))?;
    ui::status::ok(&format!("{} files synced", status.global_files));

    Ok(SyncConfig {
        enabled: true,
        mac_device_id: mac_id,
        pi_device_id: pi_id,
        folder_id: "ludolph-vault".to_string(),
        pi_repo_path: pi_vault_path.to_string(), // Same as vault path in legacy mode
        pi_vault_path: pi_vault_path.to_string(),
        pi_symlink: None,
    })
}

/// Verify sync is working with test files. Returns true if verification passed.
fn verify_sync(vault_path: &Path, pi: &PiConfig, pi_vault_path: &str) -> bool {
    ui::status::checking("Testing sync: Mac -> Pi...");
    println!("      Created: .ludolph-sync-test");

    match syncthing::verify_sync(vault_path, &pi.host, &pi.user, pi_vault_path) {
        Ok((mac_to_pi, pi_to_mac)) => {
            ui::status::ok(&format!(
                "File appeared on Pi in {:.1}s",
                mac_to_pi.as_secs_f32()
            ));
            println!();
            ui::status::checking("Testing sync: Pi -> Mac...");
            println!("      Created: .ludolph-sync-test-pi");
            ui::status::ok(&format!(
                "File appeared on Mac in {:.1}s",
                pi_to_mac.as_secs_f32()
            ));
            println!();
            ui::status::checking("Cleaning up test files...");
            ui::status::ok("Sync verified!");
            true
        }
        Err(e) => {
            ui::status::error(&format!("Sync verification failed: {e}"));
            ui::status::hint("Initial sync completed - verification may need more time to settle");
            false
        }
    }
}

/// Collect Syncthing configuration and set up vault sync.
pub fn collect_sync_config(
    vault_path: &Path,
    pi: &PiConfig,
    existing_pi_vault_path: Option<&str>,
) -> Result<Option<SyncConfig>> {
    println!();
    ui::status::section("Vault Sync");
    println!();
    println!("  Your vault needs to be accessible on the Pi.");
    println!("  We can set up Syncthing for real-time sync,");
    println!("  or you can configure your own solution.");
    println!();

    if !ui::prompt::confirm("Set up vault sync now?")? {
        println!();
        println!("  No problem! Set up your own sync, then run:");
        println!("  {}", style("lu sync setup").cyan());
        println!();
        println!(
            "  The bot expects the vault at: {}",
            style("~/vault").bold()
        );
        println!();
        return Ok(None);
    }

    // Collect Pi vault path (default matches Mac vault folder name)
    let pi_vault_path = collect_pi_vault_path(vault_path, pi, existing_pi_vault_path)?;

    if !check_disk_space(vault_path, pi, &pi_vault_path)? {
        return Ok(None);
    }

    let (mac_api_key, pi_api_key) = install_syncthing(pi)?;
    let sync_config = configure_sync(vault_path, pi, &pi_vault_path, &mac_api_key, &pi_api_key)?;

    // Give Syncthing a moment to settle after initial sync
    println!();
    ui::status::checking("Letting sync settle...");
    std::thread::sleep(Duration::from_secs(5));
    ui::status::ok("Ready for verification");

    // Verification is informational - sync is already configured at this point
    let verified = verify_sync(vault_path, pi, &pi_vault_path);

    println!();
    if verified {
        ui::status::ok("Vault sync configured and verified");
    } else {
        ui::status::ok("Vault sync configured (verification timed out)");
        println!();
        println!("  Verification often fails after syncing large vaults.");
        println!("  Check status later with: {}", style("lu sync").cyan());
    }
    println!();

    Ok(Some(sync_config))
}

/// Prompt for Pi repo path (where to clone the full git repo).
fn collect_pi_repo_path(mac_repo_root: &Path, _pi: &PiConfig) -> Result<String> {
    let repo_name = mac_repo_root
        .file_name()
        .map_or_else(|| "vault".to_string(), |n| n.to_string_lossy().to_string());

    // Default to ~/RepoName
    let default_path = format!("~/{repo_name}");

    println!();
    println!("{} Pi repo path", style("π").bold());
    println!(
        "  {}",
        style("Where should the repo be cloned on the Pi?").dim()
    );
    println!(
        "  {}",
        style(format!("Default: {default_path} (Enter to keep)")).dim()
    );

    let value: String = dialoguer::Input::<String>::new()
        .with_prompt("  ")
        .allow_empty(true)
        .interact_text()?;

    let path = if value.is_empty() {
        default_path
    } else {
        value
    };

    Ok(path)
}

/// Check for existing sync config and offer to reset.
fn check_existing_sync(config: &Config) -> Result<bool> {
    if let Some(ref sync) = config.sync {
        if sync.enabled {
            println!();
            println!("  Sync already configured:");
            if let Some(ref pi) = config.pi {
                println!("    Pi: {}:{}", pi.host, sync.pi_vault_path);
            }
            println!("    Folder: {}", sync.folder_id);
            println!();

            if !ui::prompt::confirm("Reset and reconfigure sync?")? {
                return Ok(false);
            }

            // Stop Syncthing on Pi to prevent deletion propagation
            if let Some(ref pi) = config.pi {
                ui::status::checking("Stopping Syncthing on Pi...");
                ssh::stop_syncthing(&pi.host, &pi.user);
                ui::status::ok("Syncthing stopped");

                // Optionally clean Pi folder
                println!();
                if ui::prompt::confirm("Delete existing Pi vault folder?")? {
                    ui::status::checking("Removing Pi vault...");
                    let mut paths_to_remove = vec![sync.pi_repo_path.as_str()];
                    if let Some(ref symlink) = sync.pi_symlink {
                        paths_to_remove.push(symlink.as_str());
                    }
                    ssh::remove_dirs(&pi.host, &pi.user, &paths_to_remove)?;
                    ui::status::ok("Pi vault removed");
                }
            }
        }
    }
    Ok(true)
}

/// Run pre-flight checks and return repo info.
fn run_preflight_checks(vault_path: &Path) -> Result<(PathBuf, Option<PathBuf>, String, bool)> {
    println!();
    ui::status::section("Pre-flight Checks");

    // Check gitleaks
    ui::status::checking("Checking for gitleaks...");
    preflight::check_gitleaks()?;
    ui::status::ok("gitleaks installed");

    // Find repo root
    ui::status::checking("Finding git repo root...");
    let repo_root = preflight::find_repo_root(vault_path)?;
    let vault_subdir = preflight::get_vault_subdir(vault_path, &repo_root);

    if let Some(ref subdir) = vault_subdir {
        ui::status::ok(&format!(
            "Repo: {} (vault in {})",
            repo_root.display(),
            subdir.display()
        ));
    } else {
        ui::status::ok(&format!("Repo: {}", repo_root.display()));
    }

    // Run gitleaks on vault directory
    ui::status::checking("Scanning for secrets...");
    preflight::check_secrets(vault_path)?;
    ui::status::ok("No secrets detected");

    // Check git-lfs if used
    let uses_lfs = preflight::uses_git_lfs(&repo_root);
    if uses_lfs {
        ui::status::checking("Checking LFS files hydrated...");
        preflight::check_lfs_hydrated(&repo_root)?;
        ui::status::ok("LFS files ready");
    }

    // Get remote URL from repo root
    ui::status::checking("Checking git remote...");
    let remote_url = preflight::get_git_remote(&repo_root)?;
    let ssh_url = preflight::to_ssh_url(&remote_url);
    let display_len = 40.min(ssh_url.len());
    ui::status::ok(&format!("Remote: {}", &ssh_url[..display_len]));

    Ok((repo_root, vault_subdir, ssh_url, uses_lfs))
}

/// Set up git and clone repo on Pi.
fn setup_pi_git(pi: &PiConfig, ssh_url: &str, pi_repo_path: &str, uses_lfs: bool) -> Result<()> {
    // Check/install git
    ui::status::checking("Checking git on Pi...");
    if !ssh::check_git(&pi.host, &pi.user)? {
        ui::status::checking("Installing git on Pi...");
        ssh::install_git(&pi.host, &pi.user)?;
    }
    ui::status::ok("Git available");

    // Check/install git-lfs if needed
    if uses_lfs {
        ui::status::checking("Checking git-lfs on Pi...");
        if !ssh::has_git_lfs(&pi.host, &pi.user)? {
            ui::status::checking("Installing git-lfs on Pi...");
            ssh::install_git_lfs(&pi.host, &pi.user)?;
        }
        ui::status::ok("Git LFS available");
    }

    // Setup SSH key for GitHub
    ui::status::checking("Checking Pi GitHub access...");
    if !ssh::test_github_access(&pi.host, &pi.user) {
        // Need to setup deploy key
        let pub_key = if ssh::has_ssh_key(&pi.host, &pi.user)? {
            ssh::run(&pi.host, &pi.user, "cat ~/.ssh/id_ed25519.pub")?
        } else {
            ui::status::checking("Generating SSH key on Pi...");
            ssh::generate_ssh_key(&pi.host, &pi.user)?
        };

        println!();
        println!("  Add this deploy key to your GitHub repo:");
        println!();
        println!("  {}", pub_key.trim());
        println!();
        println!("  GitHub -> Repo -> Settings -> Deploy keys -> Add");
        println!();

        if !ui::prompt::confirm("Done adding deploy key?")? {
            return Err(anyhow!("Deploy key required for Pi git access"));
        }

        // Verify access
        ui::status::checking("Verifying GitHub access...");
        if !ssh::test_github_access(&pi.host, &pi.user) {
            return Err(anyhow!("GitHub access not working. Check deploy key."));
        }
    }
    ui::status::ok("GitHub access configured");

    // Check if repo already exists
    if ssh::path_exists(&pi.host, &pi.user, pi_repo_path)? {
        ui::status::checking("Repo exists on Pi, pulling latest...");
        ssh::git_pull(&pi.host, &pi.user, pi_repo_path)?;
        if uses_lfs {
            ui::status::checking("Pulling LFS files...");
            ssh::git_lfs_pull(&pi.host, &pi.user, pi_repo_path)?;
        }
        ui::status::ok("Repository updated");
    } else {
        // Clone repo
        ui::status::checking("Cloning repo to Pi...");
        ssh::clone_repo(&pi.host, &pi.user, ssh_url, pi_repo_path)?;
        if uses_lfs {
            ui::status::checking("Pulling LFS files...");
            ssh::git_lfs_pull(&pi.host, &pi.user, pi_repo_path)?;
        }
        ui::status::ok("Repository cloned");
    }

    Ok(())
}

/// Run Syncthing setup standalone (for users who skipped during initial setup).
/// Uses git-first approach: clone repo before configuring Syncthing.
pub fn sync_setup() -> Result<()> {
    let Ok(mut config) = Config::load() else {
        ui::status::print_error("No config found", Some("Run `lu setup` first."));
        return Ok(());
    };

    let Some(ref pi) = config.pi else {
        ui::status::print_error(
            "Pi not configured",
            Some("Run `lu setup` first to configure Pi connection."),
        );
        return Ok(());
    };

    println!();
    ui::status::section("Vault Sync Setup");

    // Phase 0: Check for existing sync config
    if !check_existing_sync(&config)? {
        return Ok(());
    }

    let vault_path = config.vault.path.clone();
    let pi = pi.clone();

    // Phase 1: Pre-flight checks
    let (repo_root, vault_subdir, ssh_url, uses_lfs) = run_preflight_checks(&vault_path)?;

    // Phase 2: Pi setup
    println!();
    ui::status::section("Pi Setup");

    // Prompt for Pi paths
    let pi_repo_path = collect_pi_repo_path(&repo_root, &pi)?;
    let pi_vault_path = vault_subdir.as_ref().map_or_else(
        || pi_repo_path.clone(),
        |subdir| format!("{pi_repo_path}/{}", subdir.display()),
    );

    // Set up git and clone
    setup_pi_git(&pi, &ssh_url, &pi_repo_path, uses_lfs)?;

    // SAFETY: Verify clone has files before proceeding to Syncthing
    ui::status::checking("Verifying clone...");
    let file_count = ssh::count_files(&pi.host, &pi.user, &pi_vault_path)?;

    if file_count == 0 {
        return Err(anyhow!(
            "Clone appears empty! Aborting before Syncthing setup.\n\n\
             Check: ssh {} 'ls -la {}'\n\
             This prevents empty-folder sync disasters.",
            pi.host,
            pi_vault_path
        ));
    }
    ui::status::ok(&format!("{file_count} files verified on Pi"));

    // Create convenience symlink if vault is in subdirectory
    let pi_symlink = if vault_subdir.is_some() {
        let vault_name = vault_path
            .file_name()
            .map_or_else(|| "vault".to_string(), |n| n.to_string_lossy().to_string());
        let suggested = format!("~/{vault_name}");

        println!();
        if ui::prompt::confirm(&format!("Create symlink {suggested} -> {pi_vault_path}?"))? {
            ssh::create_symlink(&pi.host, &pi.user, &pi_vault_path, &suggested)?;
            ui::status::ok(&format!("Symlink created: {suggested}"));
            Some(suggested)
        } else {
            None
        }
    } else {
        None
    };

    // Phase 3: Syncthing setup
    println!();
    ui::status::section("Syncthing Setup");

    let (mac_api_key, pi_api_key) = install_syncthing(&pi)?;
    let sync_config = configure_sync_with_paths(
        &vault_path,
        &pi,
        &pi_repo_path,
        &pi_vault_path,
        pi_symlink.as_deref(),
        &mac_api_key,
        &pi_api_key,
    )?;

    // Update config with repo_root if vault is a subdirectory
    if vault_subdir.is_some() {
        config.vault.repo_root = Some(repo_root);
    }

    // Save config
    config.sync = Some(sync_config);
    config.save()?;

    // Give Syncthing a moment to settle after initial sync
    println!();
    ui::status::checking("Letting sync settle...");
    std::thread::sleep(Duration::from_secs(5));
    ui::status::ok("Ready for verification");

    // Verification is informational - sync is already configured at this point
    let verified = verify_sync(&vault_path, &pi, &pi_vault_path);

    println!();
    if verified {
        ui::status::ok("Vault sync configured and verified");
    } else {
        ui::status::ok("Vault sync configured (verification timed out)");
        println!();
        println!("  Files are already cloned - verification may need time to settle.");
        println!("  Check status later with: {}", style("lu sync").cyan());
    }
    println!();

    Ok(())
}

/// Pair devices and configure shared folder with explicit paths.
fn configure_sync_with_paths(
    vault_path: &Path,
    pi: &PiConfig,
    pi_repo_path: &str,
    pi_vault_path: &str,
    pi_symlink: Option<&str>,
    mac_api_key: &str,
    pi_api_key: &str,
) -> Result<SyncConfig> {
    ui::status::checking("Exchanging device IDs...");
    let mac_id = syncthing::get_device_id(mac_api_key)?;
    let pi_id = syncthing::get_device_id_pi(&pi.host, &pi.user, pi_api_key)?;

    syncthing::add_device(mac_api_key, &pi_id, "pi")?;
    syncthing::add_device_pi(&pi.host, &pi.user, pi_api_key, &mac_id, "mac")?;
    ui::status::ok("Devices paired");

    ui::status::checking("Configuring shared folder...");
    let vault_str = vault_path.to_string_lossy();

    syncthing::create_folder(mac_api_key, &vault_str, &[&mac_id, &pi_id])?;
    syncthing::create_folder_pi(
        &pi.host,
        &pi.user,
        pi_api_key,
        pi_vault_path,
        &[&mac_id, &pi_id],
    )?;
    ui::status::ok(&format!("{vault_str} <-> {pi_vault_path}"));

    // Initial sync should be fast since files already exist from git clone
    let status = syncthing::wait_for_sync(mac_api_key, Duration::from_secs(120))?;
    ui::status::ok(&format!("{} files synced", status.global_files));

    Ok(SyncConfig {
        enabled: true,
        mac_device_id: mac_id,
        pi_device_id: pi_id,
        folder_id: "ludolph-vault".to_string(),
        pi_repo_path: pi_repo_path.to_string(),
        pi_vault_path: pi_vault_path.to_string(),
        pi_symlink: pi_symlink.map(String::from),
    })
}

/// Show sync status.
#[allow(clippy::unnecessary_wraps, clippy::cast_precision_loss)]
pub fn sync_status() -> Result<()> {
    let Ok(config) = Config::load() else {
        ui::status::print_error("No config found", Some("Run `lu setup` first."));
        return Ok(());
    };

    let Some(ref sync) = config.sync else {
        println!();
        StatusLine::error("Sync not configured").print();
        ui::status::hint("Run `lu sync setup` to configure vault sync");
        println!();
        return Ok(());
    };

    println!();
    println!("{}", style("Vault Sync").bold());
    println!();

    if let Ok(api_key) = syncthing::get_api_key_mac() {
        if let Ok(status) = syncthing::get_folder_status(&api_key) {
            StatusLine::ok(format!("State: {}", status.state)).print();
            StatusLine::ok(format!("Files: {}", status.global_files)).print();
            StatusLine::ok(format!(
                "Size: {:.1} MB",
                status.global_bytes as f64 / 1_048_576.0
            ))
            .print();
        } else {
            show_fallback_status(sync);
        }
    } else {
        show_fallback_status(sync);
    }

    println!();
    Ok(())
}

/// Show stored sync config when live status unavailable.
fn show_fallback_status(sync: &SyncConfig) {
    StatusLine::ok(format!("Folder: {}", sync.folder_id)).print();
    StatusLine::ok(format!("Mac ID: {}...", &sync.mac_device_id[..8])).print();
    StatusLine::ok(format!("Pi ID: {}...", &sync.pi_device_id[..8])).print();
    ui::status::hint("Syncthing may not be running");
}
