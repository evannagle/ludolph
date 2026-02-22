# Setup

This guide walks you through configuring Ludolph for the first time.

## Quick Start

```bash
cargo install ludolph && lu setup
```

The wizard guides you through entering credentials and testing your Pi connection.

## Getting Your Credentials

### Telegram Bot Token

Ludolph uses Telegram as its messaging interface. You need to create a bot:

1. Open Telegram and message [@BotFather](https://t.me/botfather)
2. Send `/newbot`
3. Choose a name for your bot (e.g., "My Vault Assistant")
4. Choose a username ending in `bot` (e.g., `myvault_bot`)
5. BotFather replies with your token:
   ```
   Use this token to access the HTTP API:
   123456789:ABCdefGHI-jklmnopQRSTuvwxyz
   ```
6. Copy the entire token including the number and colon

The token format is `<bot_id>:<secret>`, like `123456789:ABCdefGHI...`.

### Your Telegram User ID

Ludolph only responds to your messages. Find your user ID:

1. Message [@userinfobot](https://t.me/userinfobot) on Telegram
2. It replies with your info:
   ```
   Id: 987654321
   First: Your
   Last: Name
   ```
3. Copy just the number after "Id:"

Your user ID is a number like `987654321`.

### Claude API Key

Ludolph uses Claude to understand and respond to your questions:

1. Go to [console.anthropic.com/settings/keys](https://console.anthropic.com/settings/keys)
2. Sign in or create an account
3. Click "Create Key"
4. Name it something recognizable (e.g., "Ludolph Pi")
5. Copy the key immediately - you won't see it again

The key format is `sk-ant-api03-...` (starts with `sk-ant-`).

**Security notes:**
- Keep your key secret - anyone with it can use your API credits
- Don't commit it to git or share it in screenshots
- Ludolph stores it in `~/.config/ludolph/config.toml` (chmod 600)

### Vault Path

Your Obsidian vault is the folder containing your markdown notes:

**Common locations:**
- macOS: `~/Documents/Obsidian/MyVault`
- Linux: `~/Obsidian/MyVault`
- Custom: wherever you put it

**What makes it a vault?**
- Contains `.obsidian/` directory (Obsidian config)
- Contains your `.md` files
- Can have any folder structure

You can use `~` for home directory:
```
~/Documents/Vault
```

## Running Setup

When you run `lu setup`:

```
Welcome to Ludolph

A real brain for your second brain.
Talk to your vault, from anywhere, anytime.

[*  ] Checking system
[•ok] Checking system
[•ok] System compatible
[•ok] Network connected

π Telegram bot token
  Ludolph receives messages through Telegram's Bot API.
  https://t.me/botfather

  > 123456789:ABCdefGHI-jklmnopQRSTuvwxyz
[*  ] Validating token...
[•ok] Validating token...

π Your Telegram user ID
  Only you can talk to this bot.
  https://t.me/userinfobot

  > 987654321

π Claude API key
  Powers the AI responses.
  https://console.anthropic.com/settings/keys

  > sk-ant-api03-...
[*  ] Validating API key...
[•ok] Validating API key...

π Path to your Obsidian vault
  The folder where your markdown notes live.

  > ~/Documents/Vault

───────────────────────────────────────────────
  Raspberry Pi
───────────────────────────────────────────────

  Ludolph runs on your Pi. Set up SSH access first:
  https://github.com/evannagle/ludolph/blob/develop/docs/pi-setup.md

π Pi hostname or IP
  The network address of your Raspberry Pi.

  > pi.local

π SSH user
  Default: pi (Enter to keep)

  >
[*  ] Connecting to pi@pi.local...
[•ok] Connecting to pi@pi.local...

[*  ] Configuring Ludolph
[•ok] Configuring Ludolph
[•ok] Config written
[•ok] Vault: /Users/you/Documents/Vault
[•ok] Authorized user: 987654321
[•ok] Pi: pi@pi.local

Setup complete ✓

  Commands:
  lu            Start the Telegram bot
  lu status     Check service status
```

## Reconfiguring

Update specific parts without redoing everything:

```bash
# Update API credentials only
lu setup credentials

# Update Pi connection only
lu setup pi

# Edit config directly
lu config
```

## Troubleshooting

### Invalid Token

```
[•!!] Validating token...
Token validation failed: Invalid Telegram bot token
```

**Fix:** Double-check you copied the entire token from BotFather, including the number before the colon.

### Invalid API Key

```
[•!!] Validating API key...
API key validation failed: Invalid API key
```

**Fix:**
- Verify the key starts with `sk-ant-`
- Check it hasn't been revoked in the Anthropic console
- Create a new key if needed

### Vault Not Found

```
Error: Path does not exist
```

**Fix:**
- Use absolute path or `~` for home
- Verify the folder exists: `ls ~/Documents/Vault`
- Check for typos

### SSH Connection Failed

```
[•!!] Connecting to pi@pi.local...
SSH failed: Connection refused
```

**Fix:** See [Pi Setup Guide](pi-setup.md) for SSH configuration.
