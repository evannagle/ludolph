# lu-example

An example Lu plugin demonstrating the plugin architecture. Use this as a template for building your own plugins.

## Features

- **example_greet** - Generate a greeting message with customizable style
- **example_summarize** - Summarize text and save to vault as markdown

## Installation

```bash
# From local path (development)
lu plugin install /path/to/lu-example

# Configure credentials
lu plugin setup lu-example

# Verify installation
lu plugin check lu-example
```

## Usage

Once installed, the tools are available through Lu's chat interface:

> "Greet Alice in pirate style"
>
> "Summarize this article and save it to my vault"

### Tools

#### example_greet

Generate a greeting message.

**Parameters:**
- `name` (required): The name to greet
- `style` (optional): `formal`, `casual`, or `pirate` (default: `casual`)

**Example:**
```
Greet Bob formally
```

#### example_summarize

Summarize text and format as vault-ready markdown.

**Parameters:**
- `text` (required): The text to summarize
- `title` (optional): Title for the summary note

**Vault Output:** `Inbox/summaries/{date}.md`

## Scheduled Tasks

The plugin includes a daily greeting task that runs at 8am.

To see scheduled tasks:
```bash
lu plugin check lu-example
```

## Development

### Setup

```bash
# Clone this plugin
git clone https://github.com/ludolph-community/lu-example
cd lu-example

# Install dependencies
uv sync

# Run tests
uv run pytest
```

### Testing the Server

```bash
# Run in development mode
uv run mcp dev src/server.py

# Or run directly
uv run python -m src.server
```

### Project Structure

```
lu-example/
├── lu-plugin.toml      # Plugin manifest
├── CLAUDE.md           # AI development guidance
├── README.md           # This file
├── pyproject.toml      # Python project config
├── src/
│   ├── __init__.py
│   └── server.py       # MCP server implementation
└── tests/
    └── test_tools.py   # Tool tests
```

## Creating Your Own Plugin

1. Copy this directory as a starting point
2. Update `lu-plugin.toml` with your plugin's details
3. Implement tools in `src/server.py`
4. Add tests in `tests/`
5. Update documentation

See [CLAUDE.md](./CLAUDE.md) for detailed development guidance.

## License

MIT
