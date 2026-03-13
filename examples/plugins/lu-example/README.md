# lu-example

An example Lu plugin demonstrating the plugin architecture.

## Installation

```bash
# Install from local path (for development)
lu plugin install /path/to/lu-example

# Or from git
lu plugin install https://github.com/ludolph-community/lu-example
```

## Setup

Configure required credentials:

```bash
lu plugin setup lu-example
```

This will prompt for:
- `EXAMPLE_API_KEY`: Your API key for the example service

## Usage

Once installed and configured, the following tools become available:

### example_greet

Generates a greeting message and saves it to your vault.

**Vault output:** `Inbox/greetings/{date}.md`

### example_summarize

Summarizes provided text content.

## Scheduled Tasks

The plugin includes a daily greeting task that runs at 8am.

To see scheduled tasks:
```bash
lu plugin check lu-example
```

## Development

To develop this plugin locally:

1. Clone the repository
2. Make changes to `lu-plugin.toml` and the MCP implementation
3. Test with: `lu plugin install /path/to/local/copy`
4. Remove and reinstall to test changes

## License

MIT
