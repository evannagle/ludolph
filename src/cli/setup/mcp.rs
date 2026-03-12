//! MCP server setup for Ludolph.
//!
//! This phase:
//! 1. Checks Python 3 is available
//! 2. Creates ~/.ludolph/mcp/ directory
//! 3. Copies MCP server files from release
//! 4. Creates Python venv and installs dependencies
//! 5. Generates channel auth token (32-byte hex)
//! 6. Writes ~/.ludolph/.mcp.json with real values
//! 7. Creates symlink: cwd/.mcp.json → ~/.ludolph/.mcp.json
//! 8. Creates launchd plist
//! 9. Starts MCP service

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
#[cfg(target_os = "macos")]
use console::style;

use crate::config::{self, Config};
use crate::ui::{self, Spinner, StatusLine};

const MCP_PORT: u16 = 8202;

/// Get the ludolph directory (~/.ludolph).
fn ludolph_dir() -> PathBuf {
    config::config_dir()
}

/// Check if Python 3 is available and return its path.
fn check_python() -> Result<PathBuf> {
    // Try python3 first, then python
    for cmd in &["python3", "python"] {
        if let Ok(output) = Command::new(cmd).arg("--version").output() {
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout);
                if version.contains("Python 3")
                    || String::from_utf8_lossy(&output.stderr).contains("Python 3")
                {
                    return Ok(PathBuf::from(cmd));
                }
            }
        }
    }
    anyhow::bail!("Python 3 not found. Install Python 3 and try again.")
}

/// Generate a 32-byte hex auth token.
fn generate_auth_token() -> String {
    // Use system random if available, otherwise fall back to time-based
    let mut bytes = [0u8; 32];

    #[cfg(unix)]
    {
        use std::io::Read;
        if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
            let _ = file.read_exact(&mut bytes);
        }
    }

    // Mix in some time-based entropy
    if let Ok(duration) = SystemTime::now().duration_since(UNIX_EPOCH) {
        let time_bytes = duration.as_nanos().to_le_bytes();
        for (i, b) in time_bytes.iter().enumerate() {
            bytes[i % 32] ^= b;
        }
    }

    // Convert to hex using write! to avoid allocations
    let mut hex = String::with_capacity(64);
    for b in bytes {
        let _ = write!(hex, "{b:02x}");
    }
    hex
}

/// Copy local MCP files recursively (dev mode).
fn copy_local_mcp_files(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = path.file_name().unwrap();

        // Skip __pycache__ and .venv
        if file_name == "__pycache__" || file_name == ".venv" {
            continue;
        }

        let dest_path = dest.join(file_name);

        if path.is_dir() {
            copy_local_mcp_files(&path, &dest_path)?;
        } else {
            fs::copy(&path, &dest_path)?;
        }
    }

    Ok(())
}

/// Download MCP server files from the latest release.
fn download_mcp_files(mcp_dir: &Path) -> Result<()> {
    // First, try to get the version from the current binary
    let version = env!("CARGO_PKG_VERSION");
    let version_tag = format!("v{version}");

    let url = format!(
        "https://github.com/evannagle/ludolph/releases/download/{version_tag}/ludolph-mcp-{version_tag}.tar.gz"
    );

    // Download and extract
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "curl -sSL '{}' | tar -xz -C '{}'",
            url,
            mcp_dir.parent().unwrap().display()
        ))
        .status()
        .context("Failed to run curl")?;

    if !status.success() {
        anyhow::bail!("Failed to download MCP server from {url}");
    }

    Ok(())
}

/// Create Python virtual environment and install dependencies.
fn setup_venv(mcp_dir: &Path, python: &Path) -> Result<PathBuf> {
    let venv_dir = mcp_dir.join(".venv");

    // Create venv
    let status = Command::new(python)
        .args(["-m", "venv", venv_dir.to_str().unwrap()])
        .status()
        .context("Failed to create virtual environment")?;

    if !status.success() {
        anyhow::bail!("Failed to create virtual environment");
    }

    // Get the venv's pip
    let venv_pip = if cfg!(windows) {
        venv_dir.join("Scripts").join("pip")
    } else {
        venv_dir.join("bin").join("pip")
    };

    // Install the ludolph-mcp package (which pulls in dependencies)
    let status = Command::new(&venv_pip)
        .args([
            "install",
            "-q",
            "--disable-pip-version-check",
            "-e",
            mcp_dir.to_str().unwrap(),
        ])
        .status()
        .context("Failed to install ludolph-mcp package")?;

    if !status.success() {
        anyhow::bail!("Failed to install ludolph-mcp package");
    }

    // Return path to venv python
    let venv_python = if cfg!(windows) {
        venv_dir.join("Scripts").join("python")
    } else {
        venv_dir.join("bin").join("python")
    };

    Ok(venv_python)
}

/// Write the .mcp.json file with real values.
fn write_mcp_json(
    ludolph_dir: &Path,
    pi_host: &str,
    auth_token: &str,
    venv_python: &Path,
) -> Result<PathBuf> {
    let mcp_json_path = ludolph_dir.join(".mcp.json");
    let mcp_dir = ludolph_dir.join("mcp");

    let mcp_json = serde_json::json!({
        "mcpServers": {
            "ludolph": {
                "type": "stdio",
                "command": venv_python.to_str().unwrap(),
                "args": [mcp_dir.join("mcp_server.py").to_str().unwrap()],
                "env": {
                    "PI_HOST": pi_host,
                    "PI_CHANNEL_PORT": MCP_PORT.to_string(),
                    "CHANNEL_AUTH_TOKEN": auth_token,
                    "PYTHONPATH": mcp_dir.to_str().unwrap()
                }
            }
        }
    });

    let content = serde_json::to_string_pretty(&mcp_json)?;
    fs::write(&mcp_json_path, content)?;

    Ok(mcp_json_path)
}

/// Create symlink from cwd/.mcp.json to ~/.ludolph/.mcp.json.
fn create_mcp_symlink(source: &Path) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let link_path = cwd.join(".mcp.json");

    // Remove existing symlink or file
    if link_path.exists() || link_path.is_symlink() {
        fs::remove_file(&link_path)?;
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, &link_path)?;
    }

    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(source, &link_path)?;
    }

    Ok(())
}

/// Create launchd plist for auto-start (macOS only).
#[cfg(target_os = "macos")]
fn create_launchd_plist(
    mcp_dir: &Path,
    venv_python: &Path,
    auth_token: &str,
    vault_path: &Path,
    claude_api_key: &str,
) -> Result<PathBuf> {
    let plist_dir = dirs::home_dir()
        .expect("home dir")
        .join("Library/LaunchAgents");
    fs::create_dir_all(&plist_dir)?;

    let plist_path = plist_dir.join("dev.ludolph.mcp.plist");

    let plist_content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>dev.ludolph.mcp</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        <string>{}/server.py</string>
    </array>
    <key>WorkingDirectory</key>
    <string>{}</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>VAULT_PATH</key>
        <string>{}</string>
        <key>AUTH_TOKEN</key>
        <string>{}</string>
        <key>PORT</key>
        <string>{}</string>
        <key>PYTHONPATH</key>
        <string>{}</string>
        <key>ANTHROPIC_API_KEY</key>
        <string>{}</string>
    </dict>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{}/mcp_server.log</string>
    <key>StandardErrorPath</key>
    <string>{}/mcp_server.log</string>
</dict>
</plist>
"#,
        venv_python.display(),
        mcp_dir.display(),
        mcp_dir.display(),
        vault_path.display(),
        auth_token,
        MCP_PORT,
        mcp_dir.display(),
        claude_api_key,
        mcp_dir.display(),
        mcp_dir.display(),
    );

    fs::write(&plist_path, plist_content)?;

    Ok(plist_path)
}

/// Get the current user ID.
#[cfg(target_os = "macos")]
fn get_user_id() -> Result<u32> {
    let output = Command::new("id")
        .arg("-u")
        .output()
        .context("Failed to get user ID")?;

    let uid_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    uid_str.parse().context("Failed to parse user ID")
}

/// Start the MCP service (macOS).
#[cfg(target_os = "macos")]
fn start_mcp_service(plist_path: &Path) -> Result<()> {
    let user_id = get_user_id()?;
    let service_target = format!("gui/{user_id}/dev.ludolph.mcp");

    // Stop any existing service
    let _ = Command::new("launchctl")
        .args(["bootout", &service_target])
        .status();

    // Wait a moment
    std::thread::sleep(std::time::Duration::from_secs(1));

    // Load and start the service
    let _ = Command::new("launchctl")
        .args([
            "bootstrap",
            &format!("gui/{user_id}"),
            plist_path.to_str().unwrap(),
        ])
        .status();

    let _ = Command::new("launchctl")
        .args(["kickstart", &service_target])
        .status();

    Ok(())
}

/// Verify MCP server is running by checking health endpoint.
#[cfg(target_os = "macos")]
fn verify_mcp_running(auth_token: &str) -> bool {
    for _ in 0..5 {
        std::thread::sleep(std::time::Duration::from_secs(1));

        let client = reqwest::blocking::Client::new();
        let resp = client
            .get(format!("http://localhost:{MCP_PORT}/health"))
            .header("Authorization", format!("Bearer {auth_token}"))
            .timeout(std::time::Duration::from_secs(5))
            .send();

        if let Ok(r) = resp {
            if r.status().is_success() {
                return true;
            }
        }
    }

    false
}

/// Setup Python environment and install dependencies.
fn setup_python_env(mcp_dir: &Path) -> Result<PathBuf> {
    // Check Python
    let spinner = Spinner::new("Checking Python...");
    let python = match check_python() {
        Ok(p) => {
            spinner.finish();
            StatusLine::ok(format!("Python found: {}", p.display())).print();
            p
        }
        Err(e) => {
            spinner.finish_error();
            return Err(e);
        }
    };

    // Create directories
    fs::create_dir_all(mcp_dir)?;
    StatusLine::ok("MCP directory created").print();

    // Use local source if available (dev mode), otherwise download
    let local_mcp = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/mcp");
    if local_mcp.exists() {
        let spinner = Spinner::new("Copying MCP server files...");
        copy_local_mcp_files(&local_mcp, mcp_dir)?;
        spinner.finish();
        StatusLine::ok("MCP server files installed").print();
    } else {
        let spinner = Spinner::new("Downloading MCP server...");
        match download_mcp_files(mcp_dir) {
            Ok(()) => {
                spinner.finish();
                StatusLine::ok("MCP server downloaded").print();
            }
            Err(e) => {
                spinner.finish_error();
                return Err(e);
            }
        }
    }

    // Create venv and install deps
    let spinner = Spinner::new("Setting up Python environment...");
    match setup_venv(mcp_dir, &python) {
        Ok(p) => {
            spinner.finish();
            StatusLine::ok("Python dependencies installed").print();
            Ok(p)
        }
        Err(e) => {
            spinner.finish_error();
            Err(e)
        }
    }
}

/// Generate or load channel auth token.
fn setup_auth_token(ludolph_dir: &Path) -> Result<String> {
    let token_file = ludolph_dir.join("channel_token");
    if token_file.exists() {
        let token = fs::read_to_string(&token_file)?.trim().to_string();
        StatusLine::ok("Using existing channel auth token").print();
        Ok(token)
    } else {
        let spinner = Spinner::new("Generating channel auth token...");
        let token = generate_auth_token();
        fs::write(&token_file, &token)?;

        // Set restrictive permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&token_file, fs::Permissions::from_mode(0o600))?;
        }

        spinner.finish();
        StatusLine::ok("Channel auth token generated").print();
        Ok(token)
    }
}

/// Setup MCP JSON config and symlink.
fn setup_mcp_config(
    ludolph_dir: &Path,
    pi_host: &str,
    auth_token: &str,
    venv_python: &Path,
) -> Result<()> {
    // Write .mcp.json
    let spinner = Spinner::new("Writing .mcp.json...");
    let mcp_json_path = write_mcp_json(ludolph_dir, pi_host, auth_token, venv_python)?;
    spinner.finish();
    StatusLine::ok(format!(".mcp.json written to {}", mcp_json_path.display())).print();

    // Create symlink (optional, may fail if not in a project dir)
    match create_mcp_symlink(&mcp_json_path) {
        Ok(()) => {
            StatusLine::ok("Symlink created in current directory").print();
        }
        Err(_) => {
            StatusLine::skip("Symlink skipped (not in project directory)").print();
        }
    }

    Ok(())
}

/// Start and verify the MCP service (macOS only).
#[cfg(target_os = "macos")]
fn start_and_verify_service(
    mcp_dir: &Path,
    venv_python: &Path,
    auth_token: &str,
    vault_path: &Path,
    claude_api_key: &str,
) -> Result<()> {
    let spinner = Spinner::new("Setting up launchd service...");
    let plist_path =
        create_launchd_plist(mcp_dir, venv_python, auth_token, vault_path, claude_api_key)?;
    spinner.finish();
    StatusLine::ok("Launchd plist created").print();

    let spinner = Spinner::new("Starting MCP service...");
    start_mcp_service(&plist_path)?;

    // Verify it's running
    if verify_mcp_running(auth_token) {
        spinner.finish();
        StatusLine::ok(format!("MCP server running on port {MCP_PORT}")).print();
    } else {
        spinner.finish_error();
        println!();
        println!("  MCP server failed to start. Check logs:");
        println!(
            "  {}",
            style(format!("tail -f {}/mcp_server.log", mcp_dir.display())).cyan()
        );
        println!();
    }

    Ok(())
}

/// Run the MCP setup phase.
#[allow(unused_variables)]
pub async fn setup_mcp() -> Result<()> {
    println!();
    ui::status::section("MCP Server Setup");
    println!();
    println!("  Setting up the MCP server for Claude Code integration.");
    println!();

    let ludolph_dir = ludolph_dir();
    let mcp_dir = ludolph_dir.join("mcp");

    // Setup Python environment
    let venv_python = setup_python_env(&mcp_dir)?;

    // Generate or load auth token
    let auth_token = setup_auth_token(&ludolph_dir)?;

    // Get config for vault path, Pi host, and Claude API key
    let config = Config::load().context("No config found. Run `lu setup credentials` first.")?;
    let pi_host = config
        .pi
        .as_ref()
        .map_or_else(|| "localhost".to_string(), |p| p.host.clone());
    let vault_path = config.vault.as_ref().map_or_else(
        || dirs::home_dir().unwrap().join("vault"),
        |v| v.path.clone(),
    );
    let claude_api_key = &config.claude.api_key;

    // Setup MCP config
    setup_mcp_config(&ludolph_dir, &pi_host, &auth_token, &venv_python)?;

    // Start service (macOS only)
    #[cfg(target_os = "macos")]
    {
        start_and_verify_service(
            &mcp_dir,
            &venv_python,
            &auth_token,
            &vault_path,
            claude_api_key,
        )?;
    }

    #[cfg(not(target_os = "macos"))]
    {
        StatusLine::skip("Service setup skipped (not macOS)").print();
    }

    Ok(())
}
