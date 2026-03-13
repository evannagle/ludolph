# Plugin Creation Experience Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `lu plugin create` and `lu plugin publish` commands with scaffold generation and GitHub registry integration.

**Architecture:** Local Rust CLI commands that generate scaffold files from embedded templates. Create command does not require MCP server. Publish command opens browser with prefilled PR URL (simpler than `gh pr create` which requires pre-forking the registry).

**Tech Stack:** Rust, clap for CLI, regex for name validation, dialoguer for prompts, open crate for browser launch, urlencoding for URL parameters.

---

## File Structure

| File | Responsibility |
|------|---------------|
| `src/cli/mod.rs` | Add `Create` and `Publish` variants to `PluginAction` enum |
| `src/cli/plugin/mod.rs` | Main plugin commands (converted from plugin.rs) |
| `src/cli/plugin/templates.rs` | Embedded template strings (new file) |
| `src/main.rs` | Wire up new commands in match statement |

---

## Chunk 1: Add CLI Commands and Dependencies

### Task 1: Add dependencies first

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add open and urlencoding dependencies**

In `Cargo.toml` dependencies section, add:

```toml
open = "5"
urlencoding = "2"
```

- [ ] **Step 2: Verify dependencies resolve**

Run: `cargo check`
Expected: Compiles (dependencies download and resolve)

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add open and urlencoding dependencies for plugin commands"
```

---

### Task 2: Add Create and Publish to PluginAction enum

**Files:**
- Modify: `src/cli/mod.rs:80-135`

- [ ] **Step 1: Add Create variant to PluginAction**

In `src/cli/mod.rs`, add inside `PluginAction` enum after line 134 (after Logs variant, before closing brace):

```rust
    /// Create a new plugin from template
    Create {
        /// Plugin name (lowercase alphanumeric with hyphens)
        name: String,
    },
    /// Publish plugin to community registry
    Publish,
```

- [ ] **Step 2: Verify enum compiles**

Run: `cargo check`
Expected: Compiles (enum variants added, handlers not wired yet)

- [ ] **Step 3: Commit**

```bash
git add src/cli/mod.rs
git commit -m "feat: add Create and Publish variants to PluginAction enum"
```

---

### Task 3: Wire up commands in main.rs

**Files:**
- Modify: `src/main.rs:72-88`

- [ ] **Step 1: Add Create and Publish command handlers**

In `src/main.rs`, inside the `Plugin { action }` match arm, add before the closing brace (after line 85):

```rust
                cli::PluginAction::Create { name } => cli::plugin_create(&name).await?,
                cli::PluginAction::Publish => cli::plugin_publish().await?,
```

- [ ] **Step 2: Verify compilation fails with expected error**

Run: `cargo check`
Expected: Error "cannot find function `plugin_create` in crate `cli`" (functions don't exist yet)

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: wire up plugin create and publish commands in main"
```

---

## Chunk 2: Implement Templates Module

### Task 4: Convert plugin.rs to directory module and add templates

**Files:**
- Create: `src/cli/plugin/mod.rs` (move from plugin.rs)
- Create: `src/cli/plugin/templates.rs`

- [ ] **Step 1: Convert plugin.rs to directory module**

```bash
mkdir -p src/cli/plugin
mv src/cli/plugin.rs src/cli/plugin/mod.rs
```

- [ ] **Step 2: Create templates.rs with embedded templates**

Create `src/cli/plugin/templates.rs`:

```rust
//! Embedded templates for plugin scaffolding.

pub const LU_PLUGIN_TOML: &str = r#"[plugin]
name = "{{name}}"
version = "0.1.0"
description = "{{description}}"
author = ""
license = "MIT"
repository = ""

[plugin.runtime]
type = "external"
package = "{{name}}"
runtime = "uvx"

# Uncomment and configure as needed:
# [plugin.credentials]
# API_KEY = { description = "API key for service", required = true }

# [[plugin.tools]]
# name = "tool_name"
# description = "What this tool does"
# vault_output = false

# [[plugin.schedules]]
# name = "daily_sync"
# cron = "0 8 * * *"
# tool = "tool_name"
# notify = true
"#;

pub const PYPROJECT_TOML: &str = r#"[project]
name = "{{name}}"
version = "0.1.0"
description = "{{description}}"
requires-python = ">=3.10"
dependencies = [
    "mcp>=1.0.0",
]

[project.optional-dependencies]
dev = [
    "pytest>=7.0.0",
    "pytest-asyncio>=0.21.0",
]

[tool.pytest.ini_options]
asyncio_mode = "auto"
testpaths = ["tests"]
"#;

pub const README_MD: &str = r#"# {{name}}

{{description}}

## Installation

```bash
lu plugin install /path/to/{{name}}
lu plugin setup {{name}}
```

## Usage

Once installed, the tools are available through Lu's chat interface.

## Development

```bash
uv sync
uv run pytest
uv run mcp dev src/server.py
```

## License

MIT
"#;

pub const SRC_INIT_PY: &str = r#""""{{name}} - {{description}}"""

__version__ = "0.1.0"
"#;

pub const SERVER_PY: &str = r#"#!/usr/bin/env python3
"""{{name}} - {{description}}"""

from mcp.server import Server
from mcp.server.stdio import stdio_server
from mcp.types import TextContent, Tool

server = Server("{{name}}")


@server.list_tools()
async def list_tools() -> list[Tool]:
    return [
        # Add your tools here
        # Tool(
        #     name="example_tool",
        #     description="What this tool does",
        #     inputSchema={
        #         "type": "object",
        #         "properties": {},
        #         "required": [],
        #     },
        # ),
    ]


@server.call_tool()
async def call_tool(name: str, arguments: dict) -> list[TextContent]:
    # Handle your tools here
    raise ValueError(f"Unknown tool: {name}")


async def main():
    async with stdio_server() as (read_stream, write_stream):
        await server.run(
            read_stream,
            write_stream,
            server.create_initialization_options(),
        )


if __name__ == "__main__":
    import asyncio
    asyncio.run(main())
"#;

pub const TESTS_INIT_PY: &str = r#""""Tests for {{name}} plugin."""
"#;

pub const TEST_TOOLS_PY: &str = r#""""Tests for {{name}} plugin."""

import pytest

from src.server import list_tools


class TestListTools:
    @pytest.mark.asyncio
    async def test_list_tools_returns_list(self):
        tools = await list_tools()
        assert isinstance(tools, list)
"#;

/// CLAUDE.md template - loaded from examples/plugins/lu-example/CLAUDE.md at compile time
pub const CLAUDE_MD: &str = include_str!("../../../examples/plugins/lu-example/CLAUDE.md");
```

- [ ] **Step 3: Add templates module to plugin/mod.rs**

At the top of `src/cli/plugin/mod.rs`, add after the doc comment:

```rust
mod templates;
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check`
Expected: Compiles (templates module loads, include_str! finds file)

- [ ] **Step 5: Commit**

```bash
git add src/cli/plugin/
git commit -m "feat: add embedded templates for plugin scaffolding"
```

---

## Chunk 3: Implement plugin_create

### Task 5: Add validation function with tests

**Files:**
- Modify: `src/cli/plugin/mod.rs`

- [ ] **Step 1: Add necessary imports**

At the top of `src/cli/plugin/mod.rs`, add these imports (after existing ones):

```rust
use regex::Regex;
use std::fs;
use std::path::Path;
use std::process::Command;
```

- [ ] **Step 2: Add name validation function and tests**

Add before the `plugin_search` function:

```rust
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
```

- [ ] **Step 3: Run tests to verify validation**

Run: `cargo test validate_plugin_name`
Expected: All 5 tests pass

- [ ] **Step 4: Commit**

```bash
git add src/cli/plugin/mod.rs
git commit -m "feat: add plugin name validation with tests"
```

---

### Task 6: Implement plugin_create function

**Files:**
- Modify: `src/cli/plugin/mod.rs`

- [ ] **Step 1: Add plugin_create function**

Add after the validation function and tests (before `plugin_search`):

```rust
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
```

- [ ] **Step 2: Update mod.rs exports**

In `src/cli/mod.rs`, update the pub use statement (lines 10-13) to include new functions:

```rust
pub use plugin::{
    plugin_check, plugin_create, plugin_disable, plugin_enable, plugin_install, plugin_list,
    plugin_logs, plugin_publish, plugin_remove, plugin_search, plugin_setup, plugin_update,
};
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check`
Expected: Error about `plugin_publish` not found (we'll add it next)

- [ ] **Step 4: Commit**

```bash
git add src/cli/plugin/mod.rs src/cli/mod.rs
git commit -m "feat: implement plugin_create command"
```

---

## Chunk 4: Implement plugin_publish

### Task 7: Implement plugin_publish function

**Files:**
- Modify: `src/cli/plugin/mod.rs`

- [ ] **Step 1: Add plugin_publish function**

Add after `plugin_create`:

```rust
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

    let repository = match repository {
        Some(url) => url,
        None => {
            // Prompt for repository
            let url: String = dialoguer::Input::new()
                .with_prompt("π Repository URL (e.g., https://github.com/you/lu-myplugin)")
                .interact_text()?;
            url
        }
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
    let encoded_title = urlencoding::encode(&format!("Add {name}"));
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
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check`
Expected: Compiles successfully

- [ ] **Step 3: Run all tests**

Run: `cargo test`
Expected: All tests pass (including validation tests)

- [ ] **Step 4: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings

- [ ] **Step 5: Commit**

```bash
git add src/cli/plugin/mod.rs
git commit -m "feat: implement plugin_publish command"
```

---

## Chunk 5: Update plugin_search and Final Verification

### Task 8: Update plugin_search to fetch from GitHub

**Files:**
- Modify: `src/cli/plugin/mod.rs`

- [ ] **Step 1: Replace plugin_search function**

Replace the existing `plugin_search` function with:

```rust
/// Search for plugins in the community registry.
pub async fn plugin_search(query: &str) -> Result<()> {
    println!();
    println!("Searching for plugins matching: {query}");
    println!();

    let spinner = Spinner::new("Searching registry...");

    // Fetch plugins.toml from GitHub
    let client = reqwest::Client::new();
    let response = client
        .get("https://raw.githubusercontent.com/ludolph-community/plugin-registry/main/plugins.toml")
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            spinner.finish();

            let content = resp.text().await?;
            let registry: toml::Value = match toml::from_str(&content) {
                Ok(v) => v,
                Err(e) => {
                    StatusLine::error(format!("Failed to parse registry: {e}")).print();
                    println!();
                    return Ok(());
                }
            };

            let plugins = registry
                .get("plugins")
                .and_then(|p| p.as_array())
                .cloned()
                .unwrap_or_default();

            let query_lower = query.to_lowercase();
            let matches: Vec<_> = plugins
                .iter()
                .filter(|p| {
                    let name = p.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let desc = p.get("description").and_then(|d| d.as_str()).unwrap_or("");
                    let tags = p
                        .get("tags")
                        .and_then(|t| t.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|t| t.as_str())
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .unwrap_or_default();

                    name.to_lowercase().contains(&query_lower)
                        || desc.to_lowercase().contains(&query_lower)
                        || tags.to_lowercase().contains(&query_lower)
                })
                .collect();

            if matches.is_empty() {
                StatusLine::skip("No plugins found").print();
            } else {
                println!("Found {} plugin(s):", matches.len());
                println!();
                for plugin in matches {
                    let name = plugin.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                    let desc = plugin
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("");
                    let version = plugin
                        .get("version")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    println!("  {name} v{version}");
                    println!("    {desc}");
                    println!();
                }
            }
        }
        Ok(resp) => {
            spinner.finish_error();
            let status = resp.status();
            if status.as_u16() == 404 {
                StatusLine::skip("Registry not found").print();
                crate::ui::status::hint(
                    "The plugin registry hasn't been created yet at ludolph-community/plugin-registry",
                );
            } else {
                StatusLine::error(format!("Search failed: {status}")).print();
            }
        }
        Err(e) => {
            spinner.finish_error();
            StatusLine::error(format!("Connection failed: {e}")).print();
            crate::ui::status::hint("Check your internet connection");
        }
    }

    println!();
    Ok(())
}
```

- [ ] **Step 2: Remove unused Config import if present**

Check if `use crate::config::Config;` is still used elsewhere in the file. If only used by old search, remove it.

- [ ] **Step 3: Verify compilation**

Run: `cargo check`
Expected: Compiles

- [ ] **Step 4: Commit**

```bash
git add src/cli/plugin/mod.rs
git commit -m "feat: update plugin_search to fetch from GitHub registry"
```

---

### Task 9: Full verification

**Files:**
- None (testing only)

- [ ] **Step 1: Run all Rust tests**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings

- [ ] **Step 3: Run fmt**

Run: `cargo fmt`

- [ ] **Step 4: Build release**

Run: `cargo build --release`
Expected: Builds successfully

- [ ] **Step 5: Manual test - create plugin**

```bash
./target/release/lu plugin create my-test-plugin
# Enter description: "A test plugin"
```

Expected: Directory created with all files

- [ ] **Step 6: Manual test - verify scaffold**

```bash
cat my-test-plugin/lu-plugin.toml
cat my-test-plugin/src/server.py
```

Expected: Templates interpolated with "my-test-plugin"

- [ ] **Step 7: Manual test - run plugin tests**

```bash
cd my-test-plugin && uv sync && uv run pytest
```

Expected: 1 test passes

- [ ] **Step 8: Manual test - validation errors**

```bash
cd ..
./target/release/lu plugin create my-test-plugin  # Should fail - exists
./target/release/lu plugin create "Bad Name"       # Should fail - invalid
./target/release/lu plugin create lu               # Should fail - reserved
```

Expected: All fail with clear error messages

- [ ] **Step 9: Cleanup**

```bash
rm -rf my-test-plugin
```

- [ ] **Step 10: Commit any fixes**

```bash
git add -A
git commit -m "chore: final verification and fixes"
```

---

## Success Criteria

1. `lu plugin create my-plugin` creates scaffold in < 2 seconds
2. Generated scaffold passes `uv run pytest`
3. Invalid names are rejected with clear errors
4. Existing directories are not overwritten
5. `lu plugin publish` opens browser with PR template
6. `lu plugin search` fetches from GitHub registry
7. All tests pass, no clippy warnings
8. Name validation has unit test coverage
