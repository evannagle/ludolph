# Plugin Documentation Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create user-facing documentation for Lu's plugin system across README and docs/plugins/ folder.

**Architecture:** Four documentation files: README section for discovery, getting-started.md for users, creating-plugins.md for developers, manifest-reference.md for technical reference. Content is already written in the spec — this plan just places it in the right files.

**Tech Stack:** Markdown only. No code changes.

**Spec:** `docs/superpowers/specs/2026-03-15-plugin-documentation-design.md`

---

## Chunk 1: Documentation Files

### Task 1: Add Plugins Section to README

**Files:**
- Modify: `README.md:212` (insert after MCP Server section, before Building from Source)

- [ ] **Step 1: Insert Plugins section into README**

Insert the following content after line 212 (`</br clear="right">`) and before the "Building from Source" heading:

```markdown

## Plugins

Out of the box, Lu can read, write, and search your vault. But maybe you want more — email summaries that land in your inbox folder, calendar alerts that become tasks, invoices that write themselves. Plugins let you extend Lu without touching the core.

```bash
lu plugin search email          # Find plugins
lu plugin install lu-email      # Install one
lu plugin setup lu-email        # Configure credentials
```

Plugins are MCP servers that run alongside Lu's vault tools. Each one adds new capabilities while following the same vault-first design: data flows into your vault as searchable notes, not into some separate database you'll forget exists.

**Building your own?** Start with `lu plugin create my-plugin` — it scaffolds everything you need. The [lu-example](examples/plugins/lu-example) plugin is a working reference with two tools you can copy and adapt. See the [Plugin Guide](docs/plugins/getting-started.md) for the full story.

```

- [ ] **Step 2: Verify README renders correctly**

Run: `open README.md` (or preview in editor)
Expected: Plugins section appears between MCP Server and Building from Source with properly formatted code blocks.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: add Plugins section to README"
```

---

### Task 2: Create docs/plugins/ Directory Structure

**Files:**
- Create: `docs/plugins/` directory

- [ ] **Step 1: Create plugins documentation directory**

```bash
mkdir -p docs/plugins
```

- [ ] **Step 2: Verify directory exists**

```bash
ls -la docs/plugins/
```
Expected: Empty directory exists.

---

### Task 3: Create getting-started.md

**Files:**
- Create: `docs/plugins/getting-started.md`

- [ ] **Step 1: Create getting-started.md with full content**

Write the following content to `docs/plugins/getting-started.md`:

```markdown
# Getting Started with Plugins

Plugins extend Lu with new capabilities — email, calendar, billing, whatever you need.
They run as separate MCP servers alongside Lu's core vault tools.

## Finding Plugins

```bash
lu plugin search email
lu plugin search calendar
```

This searches the [community registry](https://github.com/ludolph-community/plugin-registry).

## Installing

```bash
lu plugin install lu-email
```

Or from a local path during development:

```bash
lu plugin install /path/to/my-plugin
```

## Setting Up Credentials

Most plugins need API keys or OAuth tokens:

```bash
lu plugin setup lu-email
```

Follow the prompts. Credentials are stored locally in `~/.ludolph/plugins/`.

## Checking Status

```bash
lu plugin list              # Show installed plugins
lu plugin check lu-email    # Health check a specific plugin
lu plugin logs lu-email     # View recent logs
```

## Scheduled Tasks

Some plugins run automatically on a schedule — morning email digests, meeting note imports, daily syncs. Results appear as vault notes you can review whenever.

To see what's scheduled:

```bash
lu plugin list --schedules
```

You'll get a Telegram notification when scheduled tasks complete (if `notify = true` in the plugin config).

## Updating

```bash
lu plugin update lu-email   # Update one
lu plugin update --all      # Update all
```

## Removing

```bash
lu plugin remove lu-email
```

## Troubleshooting

| Problem | Fix |
|---------|-----|
| "Plugin not found" | Check spelling, or install from path |
| "Credential not configured" | Run `lu plugin setup <name>` |
| Tools not appearing | Run `lu plugin check <name>` |
```

- [ ] **Step 2: Verify file renders correctly**

Run: `open docs/plugins/getting-started.md` (or preview in editor)
Expected: All sections display with proper heading hierarchy and formatted code blocks.

- [ ] **Step 3: Commit**

```bash
git add docs/plugins/getting-started.md
git commit -m "docs: add plugin getting-started guide"
```

---

### Task 4: Create creating-plugins.md

**Files:**
- Create: `docs/plugins/creating-plugins.md`

- [ ] **Step 1: Create creating-plugins.md with full content**

Write the following content to `docs/plugins/creating-plugins.md`:

```markdown
# Creating Plugins

Build your own Lu plugin in minutes. The scaffold gives you everything —
Claude Code handles the rest.

## Quickstart

```bash
lu plugin create my-plugin
```

You'll be prompted for a description. Then:

```
my-plugin/
├── lu-plugin.toml      # Plugin manifest
├── CLAUDE.md           # Development guidance for Claude Code
├── README.md           # Documentation
├── pyproject.toml      # Python dependencies
├── src/
│   ├── __init__.py
│   └── server.py       # Your MCP server
└── tests/
    └── test_tools.py   # Tests
```

## Develop

Open the plugin directory in Claude Code:

```bash
cd my-plugin
claude
```

Claude reads CLAUDE.md and knows how to help — adding tools, handling credentials,
writing tests, formatting vault output. Just describe what you want.

## Test

Requires Python 3.10+.

```bash
uv sync                        # Install dependencies
uv run pytest                  # Run tests
uv run mcp dev src/server.py   # Test server interactively
```

## Install Locally

```bash
lu plugin install .            # Install from current directory
lu plugin check my-plugin      # Verify it works
```

## Publish

When you're ready to share:

```bash
lu plugin publish
```

This opens a browser to create a PR against the community registry.

## Reference

- **[lu-example](../../examples/plugins/lu-example)** — Working reference plugin with two tools
- **[Manifest Reference](manifest-reference.md)** — All lu-plugin.toml fields
- **[MCP Documentation](https://modelcontextprotocol.io)** — Protocol details
```

- [ ] **Step 2: Verify file renders correctly**

Run: `open docs/plugins/creating-plugins.md` (or preview in editor)
Expected: All sections display with directory tree rendering properly.

- [ ] **Step 3: Commit**

```bash
git add docs/plugins/creating-plugins.md
git commit -m "docs: add plugin creation quickstart guide"
```

---

### Task 5: Create manifest-reference.md

**Files:**
- Create: `docs/plugins/manifest-reference.md`

- [ ] **Step 1: Create manifest-reference.md with full content**

Write the following content to `docs/plugins/manifest-reference.md`:

```markdown
# Plugin Manifest Reference

The `lu-plugin.toml` file defines your plugin's metadata, tools, credentials, and schedules.

## Required Fields

```toml
[plugin]
name = "my-plugin"              # Lowercase, hyphens only
version = "0.1.0"               # Semantic version
description = "What it does"    # One line
```

## Optional Metadata

```toml
[plugin]
author = "your-name"
license = "MIT"
repository = "https://github.com/you/my-plugin"
```

## Runtime

How Lu runs your plugin:

```toml
[plugin.runtime]
type = "external"       # Plugin runs as separate process
package = "my-plugin"   # Package name
runtime = "uvx"         # "uvx" (Python) or "npx" (Node)
```

## Credentials

Secrets your plugin needs. Lu prompts for these during `lu plugin setup`.

```toml
[plugin.credentials]
API_KEY = { description = "Service API key", required = true }
WEBHOOK_URL = { description = "Optional webhook", required = false }
```

Access in Python via environment variables:

```python
import os
api_key = os.environ.get("API_KEY")
```

## Tools

Declare tools for discoverability (implementation lives in server.py):

```toml
[[plugin.tools]]
name = "summarize_inbox"
description = "Summarize unread emails"
vault_output = true
output_path = "Inbox/email/{date}.md"

[[plugin.tools]]
name = "send_reply"
description = "Send an email reply"
vault_output = false
```

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Tool identifier |
| `description` | string | What it does |
| `vault_output` | bool | Creates vault notes? |
| `output_path` | string | Where notes land |

**Output path variables:** `{date}` (YYYY-MM-DD), `{datetime}` (ISO format), `{name}` (from tool arguments)

## Schedules

Run tools automatically:

```toml
[[plugin.schedules]]
name = "morning_digest"
cron = "0 8 * * *"          # 8am daily
tool = "summarize_inbox"
notify = true               # Telegram notification when done

[[plugin.schedules]]
name = "hourly_sync"
cron = "0 * * * *"          # Every hour
tool = "sync_tasks"
notify = false
```

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Schedule identifier |
| `cron` | string | Cron expression |
| `tool` | string | Tool to run |
| `notify` | bool | Send Telegram notification |

## Full Example

```toml
[plugin]
name = "lu-email"
version = "1.0.0"
description = "Gmail integration with vault-first email management"
author = "ludolph-community"
license = "MIT"
repository = "https://github.com/ludolph-community/lu-email"

[plugin.runtime]
type = "external"
package = "lu-email"
runtime = "uvx"

[plugin.credentials]
GOOGLE_CLIENT_ID = { description = "OAuth client ID", required = true }
GOOGLE_CLIENT_SECRET = { description = "OAuth client secret", required = true }

[[plugin.tools]]
name = "email_summarize_inbox"
description = "Summarize unread emails and create vault note"
vault_output = true
output_path = "Inbox/email-summaries/{date}.md"

[[plugin.schedules]]
name = "inbox_digest"
cron = "0 8 * * *"
tool = "email_summarize_inbox"
notify = true
```
```

- [ ] **Step 2: Verify file renders correctly**

Run: `open docs/plugins/manifest-reference.md` (or preview in editor)
Expected: All TOML code blocks render with proper syntax, tables display correctly.

- [ ] **Step 3: Commit**

```bash
git add docs/plugins/manifest-reference.md
git commit -m "docs: add plugin manifest reference"
```

---

### Task 6: Verify All Links

**Files:**
- Verify: `README.md`, `docs/plugins/*.md`

- [ ] **Step 1: Check internal link targets exist**

Verify these paths exist or note which are expected to exist later:

```bash
# From README Plugins section
ls examples/plugins/lu-example 2>/dev/null || echo "lu-example: will be created later"
ls docs/plugins/getting-started.md

# From creating-plugins.md
ls examples/plugins/lu-example 2>/dev/null || echo "lu-example: will be created later"
ls docs/plugins/manifest-reference.md
```

Expected: getting-started.md and manifest-reference.md exist. lu-example directory may not exist yet (it's a future task from the broader plugin architecture plan).

- [ ] **Step 2: Verify cross-references between docs**

Check that docs/plugins/creating-plugins.md links to manifest-reference.md correctly:

```bash
grep -n "manifest-reference.md" docs/plugins/creating-plugins.md
```

Expected: Line containing `[Manifest Reference](manifest-reference.md)`

- [ ] **Step 3: Final verification commit**

```bash
git status
# Should show all documentation files committed
git log --oneline -5
```

Expected: Four commits for README and three docs/plugins/ files.

---

### Task 7: Final Review

- [ ] **Step 1: Preview all documentation**

Open each file and verify formatting:
- README.md — Plugins section between MCP Server and Building from Source
- docs/plugins/getting-started.md — User-focused guide with troubleshooting table
- docs/plugins/creating-plugins.md — Developer quickstart with directory tree
- docs/plugins/manifest-reference.md — Technical reference with TOML examples

- [ ] **Step 2: Verify tone matches README**

Skim each file for consistency:
- Conversational, not corporate
- Explains "why" before "how"
- Code examples are practical, not abstract

- [ ] **Step 3: Done**

All documentation complete. The lu-example reference plugin is a separate task from the broader plugin architecture plan.
