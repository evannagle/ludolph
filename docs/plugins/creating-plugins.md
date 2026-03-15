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
