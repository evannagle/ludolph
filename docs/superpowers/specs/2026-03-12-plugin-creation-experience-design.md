# Plugin Creation Experience Design

## Summary

Design the developer experience for creating Lu plugins, including a `lu plugin create` command and a GitHub-based plugin registry for discovery and publishing.

## Goals

1. Make plugin creation fast - scaffold in seconds, start developing immediately
2. Leverage Claude Code for development (not embedded AI in the wizard)
3. Enable community contribution through a transparent PR-based registry
4. Follow conventions of other ecosystems (npm, cargo) for familiarity

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Entry point | `lu plugin create <name>` | Familiar pattern from npm/cargo |
| Marketplace | GitHub-based registry | Simple, transparent, no infrastructure |
| AI integration | Scaffold + CLAUDE.md | Clean separation of concerns |
| Wizard info | Minimal (name + description) | Get developers into Claude Code quickly |
| Location | Current directory | Developers manage their own project organization |

## Design

### Command: `lu plugin create`

```bash
lu plugin create my-plugin
```

**Note:** This command runs locally and does not require the MCP server to be running. It only creates files on disk.

**Name validation:**
- Must be lowercase alphanumeric with hyphens only: `[a-z0-9-]+`
- Must start with a letter: `^[a-z]`
- Maximum 50 characters
- Reserved names: `lu`, `plugin`, `test`

**Error handling:**
- If `./name/` already exists, fail with: `Directory 'name' already exists. Remove it or choose a different name.`
- If name is invalid, fail with: `Invalid plugin name. Use lowercase letters, numbers, and hyphens only.`

**Interactive flow:**
```
Creating plugin: my-plugin

Description (one line):
π What does this plugin do?
> Sync tasks between Todoist and vault

[•ok] Created my-plugin/
      ├── lu-plugin.toml
      ├── CLAUDE.md
      ├── README.md
      ├── pyproject.toml
      ├── src/
      │   ├── __init__.py
      │   └── server.py
      └── tests/
          ├── __init__.py
          └── test_tools.py

Next steps:
  cd my-plugin
  claude                    # develop with Claude Code
  uv run pytest             # run tests
  lu plugin install .       # test locally
  lu plugin publish         # submit to registry
```

### Plugin Registry

**Repository:** `ludolph-community/plugin-registry`

**Structure:**
```
plugin-registry/
├── README.md              # How to submit plugins
├── plugins.toml           # Plugin index
└── CODEOWNERS             # Maintainers who review PRs
```

**plugins.toml format:**
```toml
[[plugins]]
name = "lu-email"
description = "Gmail integration with vault-first email management"
repository = "https://github.com/ludolph-community/lu-email"
version = "1.0.0"
author = "ludolph-community"
tags = ["email", "gmail", "productivity"]

[[plugins]]
name = "lu-billing"
description = "FreshBooks time tracking and invoicing"
repository = "https://github.com/evannagle/lu-billing"
version = "0.2.0"
author = "evannagle"
tags = ["billing", "freshbooks", "time-tracking"]
```

**CLI integration:**
```bash
lu plugin search email     # Searches registry plugins.toml
lu plugin install lu-email # Fetches from repository URL in registry
```

### Command: `lu plugin publish`

```bash
lu plugin publish
```

**Flow:**
1. Read `lu-plugin.toml` from current directory
2. Validate required fields (name, version, description)
3. Infer repository URL from git remote: `git remote get-url origin`
   - If no git remote, prompt: `Repository URL (e.g., https://github.com/you/lu-myplugin):`
4. Generate PR body markdown with plugin metadata
5. Use `gh pr create` if gh CLI available, otherwise open browser URL:
   ```
   https://github.com/ludolph-community/plugin-registry/compare/main...?quick_pull=1&title=Add+{name}&body={encoded_body}
   ```

**Validation errors:**
- Missing `lu-plugin.toml`: `No lu-plugin.toml found. Run this command from a plugin directory.`
- Missing required field: `Missing required field: {field}. Update lu-plugin.toml and try again.`

### Scaffold Templates

**Template interpolation:** Use simple string replacement with `{{name}}` and `{{description}}` markers. The Rust implementation uses `.replace("{{name}}", &name)`. No template engine required.

**Escaping:** If user description contains `{{`, replace with literal text (no special handling needed since these markers are only used during generation).

---

**lu-plugin.toml:**
```toml
[plugin]
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
```

---

**README.md:**
```markdown
# {{name}}

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
```

---

**src/__init__.py:**
```python
"""{{name}} - {{description}}"""

__version__ = "0.1.0"
```

---

**server.py:**
```python
#!/usr/bin/env python3
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
```

---

**CLAUDE.md:** Copy from `examples/plugins/lu-example/CLAUDE.md` with `{{name}}` and `{{description}}` substituted in the header sections.

---

**pyproject.toml:**
```toml
[project]
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
```

---

**tests/__init__.py:**
```python
"""Tests for {{name}} plugin."""
```

---

**tests/test_tools.py:**
```python
"""Tests for {{name}} plugin."""

import pytest

from src.server import list_tools


class TestListTools:
    @pytest.mark.asyncio
    async def test_list_tools_returns_list(self):
        tools = await list_tools()
        assert isinstance(tools, list)
```

## Implementation

### Files to Modify

| File | Change |
|------|--------|
| `src/cli/mod.rs` | Add `Create` and `Publish` variants to `PluginAction` |
| `src/cli/plugin.rs` | Add `plugin_create()` and `plugin_publish()` functions |
| `src/main.rs` | Wire up new commands in match statement |

### New Functions

**plugin_create(name: &str):**
1. Validate name (regex: `^[a-z][a-z0-9-]{0,49}$`)
2. Check if directory exists, fail if so
3. Prompt for description
4. Create directory `./name/`
5. Create subdirectories: `src/`, `tests/`
6. Write scaffold files with `.replace("{{name}}", &name).replace("{{description}}", &desc)`
7. Copy CLAUDE.md from embedded template
8. Print success message with next steps

**plugin_publish():**
1. Read `lu-plugin.toml` from current directory
2. Validate required fields (name, version, description)
3. Get repository URL: try `git remote get-url origin`, fallback to prompt
4. Generate PR body markdown:
   ```markdown
   ## Add {{name}}

   - **Description:** {{description}}
   - **Repository:** {{repository}}
   - **Version:** {{version}}
   - **Author:** {{author}}
   ```
5. Check if `gh` CLI available (`which gh`)
   - If yes: Run `gh pr create` against `ludolph-community/plugin-registry`
   - If no: Open browser to PR creation URL with query params

### External Setup

1. Create `ludolph-community/plugin-registry` repository
2. Add initial `plugins.toml` (can be empty or seed with lu-example)
3. Add `README.md` explaining submission process
4. Configure CODEOWNERS for review

### Search Enhancement

Update `plugin_search()` to fetch directly from GitHub (no MCP server required):

1. Fetch `https://raw.githubusercontent.com/ludolph-community/plugin-registry/main/plugins.toml`
2. Parse TOML and filter by query (name, description, tags)
3. Display matching plugins

**Note:** This changes `plugin_search` from MCP-dependent to standalone. The existing MCP endpoint can be deprecated or kept as a proxy.

## Verification

### Test Plugin Creation
```bash
# Create new plugin
lu plugin create test-plugin
# Enter description: "Test plugin for verification"

# Verify structure
ls -la test-plugin/
cat test-plugin/lu-plugin.toml
cat test-plugin/src/server.py
cat test-plugin/src/__init__.py
cat test-plugin/tests/__init__.py

# Verify tests run
cd test-plugin
uv sync
uv run pytest

# Cleanup
cd .. && rm -rf test-plugin/
```

### Test Validation
```bash
# Should fail: directory exists
mkdir existing-dir
lu plugin create existing-dir
# Expected: "Directory 'existing-dir' already exists..."

# Should fail: invalid name
lu plugin create "My Plugin"
# Expected: "Invalid plugin name..."

lu plugin create 123-starts-with-number
# Expected: "Invalid plugin name..."

# Cleanup
rmdir existing-dir
```

### Test Publishing Flow
```bash
# In a plugin directory with git initialized
cd test-plugin
git init
git remote add origin https://github.com/user/test-plugin
lu plugin publish
# Should open browser or run gh pr create
```

### Test Registry Search
```bash
# After registry is set up with at least one plugin
lu plugin search email
# Should display matching plugins from registry
```

## Success Criteria

1. `lu plugin create my-plugin` completes in under 2 seconds
2. Generated scaffold passes `uv run pytest` out of the box
3. CLAUDE.md provides sufficient guidance for Claude Code development
4. `lu plugin publish` successfully generates PR URL or creates PR via gh
5. `lu plugin search` returns results from registry (fetched directly from GitHub)
6. Invalid plugin names are rejected with clear error messages
7. Existing directories are not overwritten
