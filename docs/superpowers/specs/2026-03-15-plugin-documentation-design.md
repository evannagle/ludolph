# Plugin Documentation Design

## Summary

Create user-facing documentation for Lu's plugin system. Primary audience is users who want to install and use plugins, with a path for developers who want to build their own.

## Goals

1. Explain what plugins are and why they exist (README section)
2. Show users how to find, install, and manage plugins (getting-started.md)
3. Give developers a quickstart that relies on Claude Code + CLAUDE.md scaffold (creating-plugins.md)
4. Provide technical reference for the manifest format (manifest-reference.md)
5. Promote lu-example as the reference implementation (like Obsidian's sample-plugin)

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Doc location | README section + docs/plugins/ folder | README stays focused, docs can grow |
| Developer depth | Quickstart only | CLAUDE.md in scaffold handles detailed guidance |
| Reference plugin | lu-example as first-class citizen | Gives developers working code to copy |
| Tone | Match README | Conversational, explains "why" before "how" |

## Documentation Structure

| File | Purpose | Audience |
|------|---------|----------|
| README.md (new section) | Warm intro, quick install example, link to docs | Everyone |
| docs/plugins/getting-started.md | Search, install, setup, schedules, update, remove | Users |
| docs/plugins/creating-plugins.md | Quickstart scaffold, Claude Code workflow, publish | Developers |
| docs/plugins/manifest-reference.md | lu-plugin.toml field reference with examples | Developers |

## Content

### README Section

Place after "MCP Server", before "Building from Source":

```markdown
## Plugins

Out of the box, Lu can read, write, and search your vault. But maybe you want more — email summaries that land in your inbox folder, calendar alerts that become tasks, invoices that write themselves. Plugins let you extend Lu without touching the core.

\```bash
lu plugin search email          # Find plugins
lu plugin install lu-email      # Install one
lu plugin setup lu-email        # Configure credentials
\```

Plugins are MCP servers that run alongside Lu's vault tools. Each one adds new capabilities while following the same vault-first design: data flows into your vault as searchable notes, not into some separate database you'll forget exists.

**Building your own?** Start with `lu plugin create my-plugin` — it scaffolds everything you need. The [lu-example](examples/plugins/lu-example) plugin is a working reference with two tools you can copy and adapt. See the [Plugin Guide](docs/plugins/getting-started.md) for the full story.
```

### docs/plugins/getting-started.md

```markdown
# Getting Started with Plugins

Plugins extend Lu with new capabilities — email, calendar, billing, whatever you need.
They run as separate MCP servers alongside Lu's core vault tools.

## Finding Plugins

\```bash
lu plugin search email
lu plugin search calendar
\```

This searches the [community registry](https://github.com/ludolph-community/plugin-registry).

## Installing

\```bash
lu plugin install lu-email
\```

Or from a local path during development:

\```bash
lu plugin install /path/to/my-plugin
\```

## Setting Up Credentials

Most plugins need API keys or OAuth tokens:

\```bash
lu plugin setup lu-email
\```

Follow the prompts. Credentials are stored locally in `~/.ludolph/plugins/`.

## Checking Status

\```bash
lu plugin list              # Show installed plugins
lu plugin check lu-email    # Health check a specific plugin
lu plugin logs lu-email     # View recent logs
\```

## Scheduled Tasks

Some plugins run automatically on a schedule — morning email digests, meeting note imports, daily syncs. Results appear as vault notes you can review whenever.

To see what's scheduled:

\```bash
lu plugin list --schedules
\```

You'll get a Telegram notification when scheduled tasks complete (if `notify = true` in the plugin config).

## Updating

\```bash
lu plugin update lu-email   # Update one
lu plugin update --all      # Update all
\```

## Removing

\```bash
lu plugin remove lu-email
\```

## Troubleshooting

| Problem | Fix |
|---------|-----|
| "Plugin not found" | Check spelling, or install from path |
| "Credential not configured" | Run `lu plugin setup <name>` |
| Tools not appearing | Run `lu plugin check <name>` |
```

### docs/plugins/creating-plugins.md

```markdown
# Creating Plugins

Build your own Lu plugin in minutes. The scaffold gives you everything —
Claude Code handles the rest.

## Quickstart

\```bash
lu plugin create my-plugin
\```

You'll be prompted for a description. Then:

\```
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
\```

## Develop

Open the plugin directory in Claude Code:

\```bash
cd my-plugin
claude
\```

Claude reads CLAUDE.md and knows how to help — adding tools, handling credentials,
writing tests, formatting vault output. Just describe what you want.

## Test

Requires Python 3.10+.

\```bash
uv sync                        # Install dependencies
uv run pytest                  # Run tests
uv run mcp dev src/server.py   # Test server interactively
\```

## Install Locally

\```bash
lu plugin install .            # Install from current directory
lu plugin check my-plugin      # Verify it works
\```

## Publish

When you're ready to share:

\```bash
lu plugin publish
\```

This opens a browser to create a PR against the community registry.

## Reference

- **[lu-example](../../examples/plugins/lu-example)** — Working reference plugin with two tools
- **[Manifest Reference](manifest-reference.md)** — All lu-plugin.toml fields
- **[MCP Documentation](https://modelcontextprotocol.io)** — Protocol details
```

### docs/plugins/manifest-reference.md

```markdown
# Plugin Manifest Reference

The `lu-plugin.toml` file defines your plugin's metadata, tools, credentials, and schedules.

## Required Fields

\```toml
[plugin]
name = "my-plugin"              # Lowercase, hyphens only
version = "0.1.0"               # Semantic version
description = "What it does"    # One line
\```

## Optional Metadata

\```toml
[plugin]
author = "your-name"
license = "MIT"
repository = "https://github.com/you/my-plugin"
\```

## Runtime

How Lu runs your plugin:

\```toml
[plugin.runtime]
type = "external"       # Plugin runs as separate process
package = "my-plugin"   # Package name
runtime = "uvx"         # "uvx" (Python) or "npx" (Node)
\```

## Credentials

Secrets your plugin needs. Lu prompts for these during `lu plugin setup`.

\```toml
[plugin.credentials]
API_KEY = { description = "Service API key", required = true }
WEBHOOK_URL = { description = "Optional webhook", required = false }
\```

Access in Python via environment variables:

\```python
import os
api_key = os.environ.get("API_KEY")
\```

## Tools

Declare tools for discoverability (implementation lives in server.py):

\```toml
[[plugin.tools]]
name = "summarize_inbox"
description = "Summarize unread emails"
vault_output = true
output_path = "Inbox/email/{date}.md"

[[plugin.tools]]
name = "send_reply"
description = "Send an email reply"
vault_output = false
\```

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Tool identifier |
| `description` | string | What it does |
| `vault_output` | bool | Creates vault notes? |
| `output_path` | string | Where notes land |

**Output path variables:** `{date}` (YYYY-MM-DD), `{datetime}` (ISO format), `{name}` (from tool arguments)

## Schedules

Run tools automatically:

\```toml
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
\```

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Schedule identifier |
| `cron` | string | Cron expression |
| `tool` | string | Tool to run |
| `notify` | bool | Send Telegram notification |

## Full Example

\```toml
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
\```
```

## Implementation

### Files to Create/Modify

| File | Change |
|------|--------|
| README.md | Add Plugins section after MCP Server |
| docs/plugins/getting-started.md | Create (user guide) |
| docs/plugins/creating-plugins.md | Create (developer quickstart) |
| docs/plugins/manifest-reference.md | Create (technical reference) |

### Verification

1. README section appears correctly formatted
2. All internal links work (lu-example, manifest-reference.md, etc.)
3. Code blocks render properly
4. Tone matches existing README style
