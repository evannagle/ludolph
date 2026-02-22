# Ludolph

<p align="center">
  <img src="docs/images/ludolph-hero.png" alt="Ludolph" width="400">
</p>

<p align="center">
  <strong>A real brain for your second brain.</strong><br>
  Talk to your vault, from anywhere, anytime.
</p>

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

<img src="docs/images/ludolph-pi.png" align="right" width="180">

The installer will:
1. Download the `lu` binary for your platform
2. Run `lu setup` to configure credentials
3. Validate your API keys

Then sync your Obsidian vault however you prefer (rsync, Syncthing, git, etc.).

See [Setup Guide](docs/setup.md) for detailed instructions.

<br clear="right">

## Usage

Once running, message your Telegram bot:

- "What's in my daily note?"
- "Find all notes mentioning 'project alpha'"
- "Summarize my meeting notes from last week"

## CLI Commands

```bash
lu               # Start the bot
lu setup         # Run setup wizard
lu config        # Open config in editor
lu pi            # Check Pi connectivity
```

## Configuration

<img src="docs/images/ludolph-raspberries.png" align="right" width="160">

Config lives at `~/.ludolph/config.toml`:

```toml
[telegram]
bot_token = "your-telegram-bot-token"
allowed_users = [123456789]

[claude]
api_key = "your-anthropic-api-key"
model = "claude-sonnet-4-20250514"

[vault]
path = "/path/to/your/vault"
```

<br clear="right">

## Raspberry Pi Setup

See [Pi Setup Guide](docs/pi-setup.md) for instructions on deploying to a Raspberry Pi.

## Building from Source

<img src="docs/images/ludolph-yak.png" align="right" width="160">

```bash
git clone https://github.com/evannagle/ludolph
cd ludolph
cargo build --release
```

Pre-built binaries for Linux (x86, ARM), macOS (Intel, Apple Silicon) are available on the [releases page](https://github.com/evannagle/ludolph/releases).

<br clear="right">

## Named After

[Ludolph van Ceulen](https://en.wikipedia.org/wiki/Ludolph_van_Ceulen) (1540–1610), the mathematician who spent 25 years calculating pi to 35 decimal places. He had the digits engraved on his tombstone.

## License

MIT
