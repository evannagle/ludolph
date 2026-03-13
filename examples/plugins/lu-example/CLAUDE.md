# Lu Plugin Development

This is a Lu plugin template. Lu plugins extend Ludolph with new capabilities while following the vault-first design principle.

## Quick Start

```bash
# Install dependencies
uv sync

# Run tests
uv run pytest

# Test MCP server locally
uv run mcp dev src/server.py

# Install in Lu
lu plugin install /path/to/this/plugin
lu plugin setup lu-example
```

## Architecture

Lu plugins are MCP (Model Context Protocol) servers that:
1. Receive tool calls from Lu's LLM
2. Execute actions (API calls, file operations, etc.)
3. Return results, optionally writing to the vault

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   Lu Bot    │────▶│  MCP Server │────▶│  External   │
│  (Telegram) │     │  (Plugin)   │     │   Service   │
└─────────────┘     └─────────────┘     └─────────────┘
                           │
                           ▼
                    ┌─────────────┐
                    │   Obsidian  │
                    │    Vault    │
                    └─────────────┘
```

## Project Structure

```
lu-example/
├── lu-plugin.toml      # Plugin manifest (REQUIRED)
├── CLAUDE.md           # This file - AI development guidance
├── README.md           # User documentation
├── pyproject.toml      # Python project configuration
├── src/
│   ├── __init__.py
│   └── server.py       # MCP server implementation
└── tests/
    └── test_tools.py   # Tool tests
```

## Plugin Manifest (lu-plugin.toml)

The manifest defines your plugin's metadata, tools, credentials, and schedules.

### Required Fields
- `name`: Plugin identifier (e.g., "lu-email")
- `version`: Semantic version (e.g., "1.0.0")

### Key Sections

```toml
[plugin]
name = "lu-example"
version = "1.0.0"
description = "What this plugin does"

[plugin.runtime]
type = "external"       # Plugin runs as separate process
package = "lu-example"  # Package name for npx/uvx
runtime = "uvx"         # Use uvx (Python) or npx (Node)

[plugin.credentials]
API_KEY = { description = "API key", required = true }

[[plugin.tools]]
name = "tool_name"
description = "What this tool does"
vault_output = true                    # Writes to vault
output_path = "Inbox/{date}.md"        # Where in vault

[[plugin.schedules]]
name = "daily_run"
cron = "0 8 * * *"      # Every day at 8am
tool = "tool_name"      # Which tool to run
notify = true           # Send Telegram notification
```

## Implementing Tools

### Basic Tool Pattern

```python
from mcp.server import Server
from mcp.types import Tool, TextContent

server = Server("lu-example")

@server.list_tools()
async def list_tools():
    return [
        Tool(
            name="example_greet",
            description="Generate a greeting",
            inputSchema={
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "Name to greet"}
                },
                "required": ["name"]
            }
        )
    ]

@server.call_tool()
async def call_tool(name: str, arguments: dict):
    if name == "example_greet":
        greeting = f"Hello, {arguments['name']}!"
        return [TextContent(type="text", text=greeting)]

    raise ValueError(f"Unknown tool: {name}")
```

### Vault-Output Tool Pattern

When `vault_output = true`, format output as markdown that Lu will save:

```python
@server.call_tool()
async def call_tool(name: str, arguments: dict):
    if name == "example_summarize":
        # Generate markdown content
        content = f"""---
date: {datetime.now().isoformat()}
source: example-plugin
---

# Summary

{summary_text}

## Details

- Item 1
- Item 2
"""
        return [TextContent(type="text", text=content)]
```

### Accessing Credentials

Credentials are passed via environment variables:

```python
import os

@server.call_tool()
async def call_tool(name: str, arguments: dict):
    api_key = os.environ.get("EXAMPLE_API_KEY")
    if not api_key:
        raise ValueError("EXAMPLE_API_KEY not configured")

    # Use api_key for API calls...
```

## Vault-First Design

**Every plugin interaction should produce vault content when appropriate.**

| Plugin Action | Vault Output |
|---------------|--------------|
| Email summary | `Inbox/email-summaries/2026-03-12.md` |
| Meeting notes | `Tasks/meetings/standup-2026-03-12.md` |
| Invoice created | `Finances/invoices/acme-corp.md` |
| Research result | `Resources/research/topic-name.md` |

### Output Path Templates

Use these placeholders in `output_path`:
- `{date}` - Current date (YYYY-MM-DD)
- `{datetime}` - Current datetime
- `{name}` - Dynamic name from tool arguments

## Testing

### Unit Tests

```python
import pytest
from src.server import call_tool

@pytest.mark.asyncio
async def test_greet_returns_greeting():
    result = await call_tool("example_greet", {"name": "World"})
    assert "Hello, World!" in result[0].text

@pytest.mark.asyncio
async def test_greet_requires_name():
    with pytest.raises(ValueError):
        await call_tool("example_greet", {})
```

### Integration Testing

```bash
# Start server in dev mode
uv run mcp dev src/server.py

# In another terminal, test with mcp client
echo '{"method": "tools/call", "params": {"name": "example_greet", "arguments": {"name": "Test"}}}' | \
  uv run mcp client src/server.py
```

## Common Tasks

### Adding a New Tool

1. Add tool definition to `lu-plugin.toml`:
   ```toml
   [[plugin.tools]]
   name = "new_tool"
   description = "What it does"
   ```

2. Add to `list_tools()` in `server.py`:
   ```python
   Tool(
       name="new_tool",
       description="What it does",
       inputSchema={...}
   )
   ```

3. Handle in `call_tool()`:
   ```python
   if name == "new_tool":
       # Implementation
       return [TextContent(type="text", text=result)]
   ```

4. Add tests in `tests/test_tools.py`

### Adding Credentials

1. Add to manifest:
   ```toml
   [plugin.credentials]
   NEW_API_KEY = { description = "New service API key", required = true }
   ```

2. Access in code:
   ```python
   api_key = os.environ.get("NEW_API_KEY")
   ```

### Adding a Schedule

```toml
[[plugin.schedules]]
name = "hourly_check"
cron = "0 * * * *"      # Every hour
tool = "check_status"
notify = false          # Silent execution
```

## Publishing

### To Lu Plugin Registry

1. Push to GitHub under `ludolph-community/` org
2. Ensure `lu-plugin.toml` is valid
3. Tag a release: `git tag v1.0.0 && git push --tags`

### Local Development Install

```bash
lu plugin install /path/to/plugin
lu plugin setup plugin-name
lu plugin check plugin-name
```

## Debugging

### View Logs

```bash
lu plugin logs lu-example -n 50
```

### Test Server Directly

```bash
# Run with debug logging
RUST_LOG=debug uv run python -m src.server
```

### Common Issues

| Issue | Solution |
|-------|----------|
| "Credential not found" | Run `lu plugin setup plugin-name` |
| Tool not appearing | Check `lu-plugin.toml` syntax |
| Server won't start | Check `uv run mcp dev` output |

## Code Quality

- Use type hints
- Handle errors gracefully (return error messages, don't crash)
- Keep tools focused (one action per tool)
- Document tool parameters in inputSchema descriptions
