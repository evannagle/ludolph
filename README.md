# Ludolph

**A real brain for your second brain.**

Talk to your vault, from anywhere, anytime.

---

Ludolph is a self-hosted Telegram bot that gives Claude read-only access to your Obsidian vault. It runs on a Raspberry Pi (or any always-on machine), letting you ask questions about your notes from anywhere.

## Features

- **Sandboxed access** — Claude can only read files inside your vault directory
- **Always available** — Runs on your Pi, answers via Telegram
- **Single binary** — No Python, no dependencies, just download and run
- **Your API key** — You control costs and data

## Quick Start

```bash
curl -sSL https://ludolph.dev/install | bash
```

The installer will:
1. Download the `lu` binary
2. Create `~/ludolph/` directory structure
3. Prompt for your Telegram bot token and Claude API key
4. Set up the systemd service

Then sync your Obsidian vault to `~/ludolph/vault/` however you prefer (rsync, Syncthing, git, etc.).

## Usage

Once running, message your Telegram bot:

- "What's in my daily note?"
- "Find all notes mentioning 'project alpha'"
- "Summarize my meeting notes from last week"

## CLI Commands

```bash
lu status      # Check if Ludolph is running
lu logs        # View recent logs
lu restart     # Restart the service
lu update      # Update to latest version
lu uninstall   # Remove Ludolph
```

## Configuration

Config lives at `~/ludolph/config.toml`:

```toml
[telegram]
bot_token = "your-telegram-bot-token"

[claude]
api_key = "your-anthropic-api-key"
model = "claude-sonnet-4-20250514"

[vault]
path = "~/ludolph/vault"
```

## Building from Source

```bash
git clone https://github.com/evannagle/ludolph
cd ludolph
cargo build --release
```

Cross-compile for Raspberry Pi:

```bash
cargo build --release --target aarch64-unknown-linux-gnu
```

## Named After

[Ludolph van Ceulen](https://en.wikipedia.org/wiki/Ludolph_van_Ceulen) (1540–1610), the mathematician who spent 25 years calculating pi to 35 decimal places. He had the digits engraved on his tombstone.

That kind of patient dedication to getting things right is what your notes deserve.

## License

MIT
