//! Plugin management CLI commands.
//!
//! These commands interact with the Mac MCP server's plugin management endpoints.
//! The Pi bot forwards these to Mac via HTTP for execution.

mod templates;

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::Result;
use regex::Regex;

use crate::config::Config;
use crate::ui::{Spinner, StatusLine};

/// Reserved plugin names that cannot be used.
const RESERVED_NAMES: &[&str] = &["lu", "plugin", "test"];

/// Validate plugin name format.
/// Must be lowercase alphanumeric with hyphens, start with letter, max 50 chars.
fn validate_plugin_name(name: &str) -> Result<(), String> {
    if name.len() > 50 {
        return Err("Plugin name must be 50 characters or less".to_string());
    }

    let re = Regex::new(r"^[a-z][a-z0-9-]*$").unwrap();
    if !re.is_match(name) {
        return Err(
            "Invalid plugin name. Use lowercase letters, numbers, and hyphens only. Must start with a letter.".to_string()
        );
    }

    if RESERVED_NAMES.contains(&name) {
        return Err(format!("'{name}' is a reserved name and cannot be used"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_plugin_name_valid() {
        assert!(validate_plugin_name("my-plugin").is_ok());
        assert!(validate_plugin_name("a").is_ok());
        assert!(validate_plugin_name("plugin123").is_ok());
        assert!(validate_plugin_name("my-cool-plugin-2").is_ok());
    }

    #[test]
    fn test_validate_plugin_name_invalid_start() {
        assert!(validate_plugin_name("123-plugin").is_err());
        assert!(validate_plugin_name("-plugin").is_err());
    }

    #[test]
    fn test_validate_plugin_name_invalid_chars() {
        assert!(validate_plugin_name("My Plugin").is_err());
        assert!(validate_plugin_name("my_plugin").is_err());
        assert!(validate_plugin_name("MyPlugin").is_err());
    }

    #[test]
    fn test_validate_plugin_name_reserved() {
        assert!(validate_plugin_name("lu").is_err());
        assert!(validate_plugin_name("plugin").is_err());
        assert!(validate_plugin_name("test").is_err());
    }

    #[test]
    fn test_validate_plugin_name_too_long() {
        let long_name = "a".repeat(51);
        assert!(validate_plugin_name(&long_name).is_err());
    }
}

/// Search for plugins in the community registry.
pub async fn plugin_search(query: &str) -> Result<()> {
    println!();
    println!("Searching for plugins matching: {query}");
    println!();

    let config = Config::load()?;
    let mcp_url = config
        .mcp
        .as_ref()
        .map_or("http://localhost:8200", |m| m.url.as_str());

    let spinner = Spinner::new("Searching registry...");

    let client = reqwest::Client::new();
    let response = client
        .get(format!("{mcp_url}/plugin/search"))
        .query(&[("q", query)])
        .header(
            "Authorization",
            format!(
                "Bearer {}",
                config.mcp.as_ref().map_or("", |m| m.auth_token.as_str())
            ),
        )
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            spinner.finish();
            let body: serde_json::Value = resp.json().await?;
            if let Some(plugins) = body.get("plugins").and_then(|p| p.as_array()) {
                if plugins.is_empty() {
                    StatusLine::skip("No plugins found").print();
                } else {
                    for plugin in plugins {
                        let name = plugin.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                        let desc = plugin
                            .get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("");
                        println!("  {name} - {desc}");
                    }
                }
            }
        }
        Ok(resp) => {
            spinner.finish_error();
            let status = resp.status();
            StatusLine::error(format!("Search failed: {status}")).print();
        }
        Err(e) => {
            spinner.finish_error();
            StatusLine::error(format!("Connection failed: {e}")).print();
            crate::ui::status::hint("Is the MCP server running? Try: lu mcp restart");
        }
    }

    println!();
    Ok(())
}

/// Install a plugin from git URL, registry name, or local path.
pub async fn plugin_install(source: &str) -> Result<()> {
    println!();
    println!("Installing plugin: {source}");
    println!();

    let config = Config::load()?;
    let mcp_url = config
        .mcp
        .as_ref()
        .map_or("http://localhost:8200", |m| m.url.as_str());

    let spinner = Spinner::new("Installing plugin...");

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{mcp_url}/plugin/install"))
        .header(
            "Authorization",
            format!(
                "Bearer {}",
                config.mcp.as_ref().map_or("", |m| m.auth_token.as_str())
            ),
        )
        .json(&serde_json::json!({ "source": source }))
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            spinner.finish();
            let body: serde_json::Value = resp.json().await?;
            let name = body.get("name").and_then(|n| n.as_str()).unwrap_or(source);
            let version = body.get("version").and_then(|v| v.as_str()).unwrap_or("?");
            StatusLine::ok(format!("Installed {name} v{version}")).print();

            // Check if setup is needed
            if let Some(needs_setup) = body.get("needs_setup").and_then(serde_json::Value::as_bool) {
                if needs_setup {
                    crate::ui::status::hint(&format!("Run `lu plugin setup {name}` to configure credentials"));
                }
            }
        }
        Ok(resp) => {
            spinner.finish_error();
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            let error = body
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("Unknown error");
            StatusLine::error(format!("Install failed: {error}")).print();
        }
        Err(e) => {
            spinner.finish_error();
            StatusLine::error(format!("Connection failed: {e}")).print();
            crate::ui::status::hint("Is the MCP server running? Try: lu mcp restart");
        }
    }

    println!();
    Ok(())
}

/// Run plugin credential setup.
#[allow(clippy::too_many_lines)]
pub async fn plugin_setup(name: &str) -> Result<()> {
    println!();
    println!("Setting up plugin: {name}");
    println!();

    let config = Config::load()?;
    let mcp_url = config
        .mcp
        .as_ref()
        .map_or("http://localhost:8200", |m| m.url.as_str());

    // First, get the plugin's credential requirements
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{mcp_url}/plugin/{name}/credentials"))
        .header(
            "Authorization",
            format!(
                "Bearer {}",
                config.mcp.as_ref().map_or("", |m| m.auth_token.as_str())
            ),
        )
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            let body: serde_json::Value = resp.json().await?;
            let credentials = body
                .get("credentials")
                .and_then(|c| c.as_array())
                .cloned()
                .unwrap_or_default();

            if credentials.is_empty() {
                StatusLine::ok("No credentials required").print();
                println!();
                return Ok(());
            }

            // Prompt for each credential
            let mut credential_values: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();

            for cred in &credentials {
                let cred_name = cred.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                let description = cred
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("");
                let oauth_flow = cred.get("oauth_flow").and_then(|o| o.as_str());

                if let Some(flow) = oauth_flow {
                    // OAuth flow - redirect to browser
                    println!("  {cred_name}: OAuth ({flow})");
                    crate::ui::status::hint(&format!(
                        "OAuth setup will open in your browser. Run: lu plugin setup {name} --oauth"
                    ));
                } else {
                    // Manual credential entry
                    println!("  {cred_name}: {description}");
                    let value = dialoguer::Password::new()
                        .with_prompt(format!("  Enter {cred_name}"))
                        .interact()?;
                    credential_values.insert(cred_name.to_string(), value);
                }
            }

            // Submit credentials
            if !credential_values.is_empty() {
                let spinner = Spinner::new("Saving credentials...");

                let response = client
                    .post(format!("{mcp_url}/plugin/{name}/credentials"))
                    .header(
                        "Authorization",
                        format!(
                            "Bearer {}",
                            config.mcp.as_ref().map_or("", |m| m.auth_token.as_str())
                        ),
                    )
                    .json(&credential_values)
                    .send()
                    .await;

                match response {
                    Ok(resp) if resp.status().is_success() => {
                        spinner.finish();
                        StatusLine::ok("Credentials saved").print();
                    }
                    Ok(resp) => {
                        spinner.finish_error();
                        let status = resp.status();
                        StatusLine::error(format!("Failed to save: {status}")).print();
                    }
                    Err(e) => {
                        spinner.finish_error();
                        StatusLine::error(format!("Connection failed: {e}")).print();
                    }
                }
            }
        }
        Ok(resp) if resp.status().as_u16() == 404 => {
            StatusLine::error(format!("Plugin not found: {name}")).print();
            crate::ui::status::hint("Check installed plugins with: lu plugin list");
        }
        Ok(resp) => {
            let status = resp.status();
            StatusLine::error(format!("Failed to get credentials: {status}")).print();
        }
        Err(e) => {
            StatusLine::error(format!("Connection failed: {e}")).print();
            crate::ui::status::hint("Is the MCP server running? Try: lu mcp restart");
        }
    }

    println!();
    Ok(())
}

/// List installed plugins.
pub async fn plugin_list() -> Result<()> {
    println!();

    let config = Config::load()?;
    let mcp_url = config
        .mcp
        .as_ref()
        .map_or("http://localhost:8200", |m| m.url.as_str());

    let spinner = Spinner::new("Loading plugins...");

    let client = reqwest::Client::new();
    let response = client
        .get(format!("{mcp_url}/plugin/list"))
        .header(
            "Authorization",
            format!(
                "Bearer {}",
                config.mcp.as_ref().map_or("", |m| m.auth_token.as_str())
            ),
        )
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            spinner.finish();
            let body: serde_json::Value = resp.json().await?;
            if let Some(plugins) = body.get("plugins").and_then(|p| p.as_array()) {
                if plugins.is_empty() {
                    StatusLine::skip("No plugins installed").print();
                    crate::ui::status::hint("Install plugins with: lu plugin install <name>");
                } else {
                    println!("Installed plugins:");
                    println!();
                    for plugin in plugins {
                        let name = plugin.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                        let version = plugin
                            .get("version")
                            .and_then(|v| v.as_str())
                            .unwrap_or("?");
                        let enabled = plugin
                            .get("enabled")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false);
                        let status = if enabled { "enabled" } else { "disabled" };
                        println!("  {name} v{version} ({status})");
                    }
                }
            }
        }
        Ok(resp) => {
            spinner.finish_error();
            let status = resp.status();
            StatusLine::error(format!("Failed to list plugins: {status}")).print();
        }
        Err(e) => {
            spinner.finish_error();
            StatusLine::error(format!("Connection failed: {e}")).print();
            crate::ui::status::hint("Is the MCP server running? Try: lu mcp restart");
        }
    }

    println!();
    Ok(())
}

/// Enable a plugin.
pub async fn plugin_enable(name: &str) -> Result<()> {
    println!();

    let config = Config::load()?;
    let mcp_url = config
        .mcp
        .as_ref()
        .map_or("http://localhost:8200", |m| m.url.as_str());

    let spinner = Spinner::new(&format!("Enabling {name}..."));

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{mcp_url}/plugin/{name}/enable"))
        .header(
            "Authorization",
            format!(
                "Bearer {}",
                config.mcp.as_ref().map_or("", |m| m.auth_token.as_str())
            ),
        )
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            spinner.finish();
            StatusLine::ok(format!("{name} enabled")).print();
        }
        Ok(resp) if resp.status().as_u16() == 404 => {
            spinner.finish_error();
            StatusLine::error(format!("Plugin not found: {name}")).print();
        }
        Ok(resp) => {
            spinner.finish_error();
            let status = resp.status();
            StatusLine::error(format!("Failed to enable: {status}")).print();
        }
        Err(e) => {
            spinner.finish_error();
            StatusLine::error(format!("Connection failed: {e}")).print();
        }
    }

    println!();
    Ok(())
}

/// Disable a plugin.
pub async fn plugin_disable(name: &str) -> Result<()> {
    println!();

    let config = Config::load()?;
    let mcp_url = config
        .mcp
        .as_ref()
        .map_or("http://localhost:8200", |m| m.url.as_str());

    let spinner = Spinner::new(&format!("Disabling {name}..."));

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{mcp_url}/plugin/{name}/disable"))
        .header(
            "Authorization",
            format!(
                "Bearer {}",
                config.mcp.as_ref().map_or("", |m| m.auth_token.as_str())
            ),
        )
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            spinner.finish();
            StatusLine::ok(format!("{name} disabled")).print();
        }
        Ok(resp) if resp.status().as_u16() == 404 => {
            spinner.finish_error();
            StatusLine::error(format!("Plugin not found: {name}")).print();
        }
        Ok(resp) => {
            spinner.finish_error();
            let status = resp.status();
            StatusLine::error(format!("Failed to disable: {status}")).print();
        }
        Err(e) => {
            spinner.finish_error();
            StatusLine::error(format!("Connection failed: {e}")).print();
        }
    }

    println!();
    Ok(())
}

/// Update plugins.
pub async fn plugin_update(all: bool, name: Option<&str>) -> Result<()> {
    println!();

    let config = Config::load()?;
    let mcp_url = config
        .mcp
        .as_ref()
        .map_or("http://localhost:8200", |m| m.url.as_str());

    let target = if all {
        "all plugins".to_string()
    } else {
        name.unwrap_or("?").to_string()
    };

    let spinner = Spinner::new(&format!("Updating {target}..."));

    let client = reqwest::Client::new();
    let url = if all {
        format!("{mcp_url}/plugin/update")
    } else {
        format!("{mcp_url}/plugin/{}/update", name.unwrap_or(""))
    };

    let response = client
        .post(&url)
        .header(
            "Authorization",
            format!(
                "Bearer {}",
                config.mcp.as_ref().map_or("", |m| m.auth_token.as_str())
            ),
        )
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            spinner.finish();
            let body: serde_json::Value = resp.json().await?;
            let updated = body
                .get("updated")
                .and_then(|u| u.as_array())
                .map_or(0, std::vec::Vec::len);
            if updated > 0 {
                StatusLine::ok(format!("Updated {updated} plugin(s)")).print();
            } else {
                StatusLine::ok("All plugins up to date").print();
            }
        }
        Ok(resp) if resp.status().as_u16() == 404 => {
            spinner.finish_error();
            StatusLine::error(format!("Plugin not found: {target}")).print();
        }
        Ok(resp) => {
            spinner.finish_error();
            let status = resp.status();
            StatusLine::error(format!("Failed to update: {status}")).print();
        }
        Err(e) => {
            spinner.finish_error();
            StatusLine::error(format!("Connection failed: {e}")).print();
        }
    }

    println!();
    Ok(())
}

/// Remove a plugin.
pub async fn plugin_remove(name: &str) -> Result<()> {
    println!();

    let config = Config::load()?;
    let mcp_url = config
        .mcp
        .as_ref()
        .map_or("http://localhost:8200", |m| m.url.as_str());

    let spinner = Spinner::new(&format!("Removing {name}..."));

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{mcp_url}/plugin/{name}/remove"))
        .header(
            "Authorization",
            format!(
                "Bearer {}",
                config.mcp.as_ref().map_or("", |m| m.auth_token.as_str())
            ),
        )
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            spinner.finish();
            StatusLine::ok(format!("{name} removed")).print();
        }
        Ok(resp) if resp.status().as_u16() == 404 => {
            spinner.finish_error();
            StatusLine::error(format!("Plugin not found: {name}")).print();
        }
        Ok(resp) => {
            spinner.finish_error();
            let status = resp.status();
            StatusLine::error(format!("Failed to remove: {status}")).print();
        }
        Err(e) => {
            spinner.finish_error();
            StatusLine::error(format!("Connection failed: {e}")).print();
        }
    }

    println!();
    Ok(())
}

/// Health check for a plugin.
pub async fn plugin_check(name: &str) -> Result<()> {
    println!();

    let config = Config::load()?;
    let mcp_url = config
        .mcp
        .as_ref()
        .map_or("http://localhost:8200", |m| m.url.as_str());

    let spinner = Spinner::new(&format!("Checking {name}..."));

    let client = reqwest::Client::new();
    let response = client
        .get(format!("{mcp_url}/plugin/{name}/check"))
        .header(
            "Authorization",
            format!(
                "Bearer {}",
                config.mcp.as_ref().map_or("", |m| m.auth_token.as_str())
            ),
        )
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            spinner.finish();
            let body: serde_json::Value = resp.json().await?;

            // Show plugin info
            let version = body.get("version").and_then(|v| v.as_str()).unwrap_or("?");
            let enabled = body
                .get("enabled")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);

            StatusLine::ok(format!("{name} v{version}")).print();

            if enabled {
                StatusLine::ok("Status: enabled").print();
            } else {
                StatusLine::skip("Status: disabled").print();
            }

            // Check credentials
            if let Some(creds) = body.get("credentials").and_then(|c| c.as_object()) {
                let configured: Vec<_> = creds
                    .iter()
                    .filter(|(_, v)| v.as_bool().unwrap_or(false))
                    .map(|(k, _)| k.as_str())
                    .collect();
                let missing: Vec<_> = creds
                    .iter()
                    .filter(|(_, v)| !v.as_bool().unwrap_or(false))
                    .map(|(k, _)| k.as_str())
                    .collect();

                if !configured.is_empty() {
                    StatusLine::ok(format!("Credentials: {} configured", configured.len())).print();
                }
                if !missing.is_empty() {
                    StatusLine::error(format!("Missing: {}", missing.join(", "))).print();
                    crate::ui::status::hint(&format!("Run: lu plugin setup {name}"));
                }
            }

            // Check tools
            if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
                StatusLine::ok(format!("Tools: {} available", tools.len())).print();
            }
        }
        Ok(resp) if resp.status().as_u16() == 404 => {
            spinner.finish_error();
            StatusLine::error(format!("Plugin not found: {name}")).print();
        }
        Ok(resp) => {
            spinner.finish_error();
            let status = resp.status();
            StatusLine::error(format!("Check failed: {status}")).print();
        }
        Err(e) => {
            spinner.finish_error();
            StatusLine::error(format!("Connection failed: {e}")).print();
        }
    }

    println!();
    Ok(())
}

/// View plugin logs.
pub async fn plugin_logs(name: &str, lines: usize) -> Result<()> {
    println!();

    let config = Config::load()?;
    let mcp_url = config
        .mcp
        .as_ref()
        .map_or("http://localhost:8200", |m| m.url.as_str());

    let client = reqwest::Client::new();
    let response = client
        .get(format!("{mcp_url}/plugin/{name}/logs"))
        .query(&[("lines", lines.to_string())])
        .header(
            "Authorization",
            format!(
                "Bearer {}",
                config.mcp.as_ref().map_or("", |m| m.auth_token.as_str())
            ),
        )
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            let body: serde_json::Value = resp.json().await?;
            if let Some(logs) = body.get("logs").and_then(|l| l.as_str()) {
                if logs.is_empty() {
                    StatusLine::skip(format!("No logs for {name}")).print();
                } else {
                    println!("{logs}");
                }
            }
        }
        Ok(resp) if resp.status().as_u16() == 404 => {
            StatusLine::error(format!("Plugin not found: {name}")).print();
        }
        Ok(resp) => {
            let status = resp.status();
            StatusLine::error(format!("Failed to get logs: {status}")).print();
        }
        Err(e) => {
            StatusLine::error(format!("Connection failed: {e}")).print();
        }
    }

    println!();
    Ok(())
}

/// Create a new plugin from template.
pub async fn plugin_create(name: &str) -> Result<()> {
    println!();
    println!("Creating plugin: {name}");
    println!();

    // Validate name
    if let Err(e) = validate_plugin_name(name) {
        crate::ui::status::print_error("Invalid name", Some(&e));
        return Ok(());
    }

    // Check if directory exists
    let plugin_dir = Path::new(name);
    if plugin_dir.exists() {
        crate::ui::status::print_error(
            "Directory exists",
            Some(&format!(
                "Directory '{name}' already exists. Remove it or choose a different name."
            )),
        );
        return Ok(());
    }

    // Prompt for description
    let description: String = dialoguer::Input::new()
        .with_prompt("π What does this plugin do?")
        .interact_text()?;

    let spinner = Spinner::new("Creating plugin...");

    // Create directory structure
    fs::create_dir_all(plugin_dir.join("src"))?;
    fs::create_dir_all(plugin_dir.join("tests"))?;

    // Helper to interpolate templates
    let interpolate = |template: &str| -> String {
        template
            .replace("{{name}}", name)
            .replace("{{description}}", &description)
    };

    // Write files
    fs::write(
        plugin_dir.join("lu-plugin.toml"),
        interpolate(templates::LU_PLUGIN_TOML),
    )?;
    fs::write(
        plugin_dir.join("pyproject.toml"),
        interpolate(templates::PYPROJECT_TOML),
    )?;
    fs::write(
        plugin_dir.join("README.md"),
        interpolate(templates::README_MD),
    )?;
    fs::write(
        plugin_dir.join("src/__init__.py"),
        interpolate(templates::SRC_INIT_PY),
    )?;
    fs::write(
        plugin_dir.join("src/server.py"),
        interpolate(templates::SERVER_PY),
    )?;
    fs::write(
        plugin_dir.join("tests/__init__.py"),
        interpolate(templates::TESTS_INIT_PY),
    )?;
    fs::write(
        plugin_dir.join("tests/test_tools.py"),
        interpolate(templates::TEST_TOOLS_PY),
    )?;

    // CLAUDE.md needs special handling - replace lu-example references
    let claude_md = templates::CLAUDE_MD
        .replace("lu-example", name)
        .replace("lu plugin setup lu-example", &format!("lu plugin setup {name}"));
    fs::write(plugin_dir.join("CLAUDE.md"), claude_md)?;

    spinner.finish();

    // Print success with tree structure
    StatusLine::ok(format!("Created {name}/")).print();
    println!("      ├── lu-plugin.toml");
    println!("      ├── CLAUDE.md");
    println!("      ├── README.md");
    println!("      ├── pyproject.toml");
    println!("      ├── src/");
    println!("      │   ├── __init__.py");
    println!("      │   └── server.py");
    println!("      └── tests/");
    println!("          ├── __init__.py");
    println!("          └── test_tools.py");
    println!();
    println!("Next steps:");
    println!("  cd {name}");
    println!("  claude                    # develop with Claude Code");
    println!("  uv run pytest             # run tests");
    println!("  lu plugin install .       # test locally");
    println!("  lu plugin publish         # submit to registry");
    println!();

    Ok(())
}

/// Publish plugin to community registry.
pub async fn plugin_publish() -> Result<()> {
    println!();
    println!("Publishing plugin to Lu registry");
    println!();

    // Check for lu-plugin.toml
    let manifest_path = Path::new("lu-plugin.toml");
    if !manifest_path.exists() {
        crate::ui::status::print_error(
            "Not a plugin directory",
            Some("No lu-plugin.toml found. Run this command from a plugin directory."),
        );
        return Ok(());
    }

    // Parse manifest
    let manifest_content = fs::read_to_string(manifest_path)?;
    let manifest: toml::Value = toml::from_str(&manifest_content)?;

    let plugin = manifest.get("plugin").ok_or_else(|| {
        anyhow::anyhow!("Invalid manifest: missing [plugin] section")
    })?;

    let name = plugin
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required field: name. Update lu-plugin.toml and try again."))?;

    let version = plugin
        .get("version")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required field: version. Update lu-plugin.toml and try again."))?;

    let description = plugin
        .get("description")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required field: description. Update lu-plugin.toml and try again."))?;

    let author = plugin
        .get("author")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Get repository URL from git remote or manifest
    let repository = plugin
        .get("repository")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or_else(|| {
            // Try git remote
            Command::new("git")
                .args(["remote", "get-url", "origin"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        });

    let repository = if let Some(url) = repository {
        url
    } else {
        // Prompt for repository
        dialoguer::Input::new()
            .with_prompt("π Repository URL (e.g., https://github.com/you/lu-myplugin)")
            .interact_text()?
    };

    // Generate PR body
    let pr_body = format!(
        r#"## Add {name}

- **Description:** {description}
- **Repository:** {repository}
- **Version:** {version}
- **Author:** {author}

### plugins.toml entry

```toml
[[plugins]]
name = "{name}"
description = "{description}"
repository = "{repository}"
version = "{version}"
author = "{author}"
tags = []
```
"#
    );

    // Open browser with prefilled PR URL
    let title = format!("Add {name}");
    let encoded_title = urlencoding::encode(&title);
    let encoded_body = urlencoding::encode(&pr_body);
    let url = format!(
        "https://github.com/ludolph-community/plugin-registry/compare/main...main?quick_pull=1&title={encoded_title}&body={encoded_body}"
    );

    println!("Opening browser for PR creation...");
    println!();

    if let Err(e) = open::that(&url) {
        StatusLine::error(format!("Failed to open browser: {e}")).print();
        println!();
        println!("Open this URL manually:");
        println!("{url}");
    } else {
        StatusLine::ok("Opened browser for PR creation").print();
    }

    println!();
    println!("Next steps:");
    println!("  1. Fork ludolph-community/plugin-registry if you haven't");
    println!("  2. Add the plugins.toml entry shown above to your fork");
    println!("  3. Create a PR to the main repository");
    println!();

    Ok(())
}
