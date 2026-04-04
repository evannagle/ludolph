# Setup

Setup is the boring part. Let's make it quick.

The wizard asks for three things (Telegram token, Claude API key, vault path), tests your Pi connection, and you're done. Five minutes if everything goes right, fifteen if your Pi is being difficult.

```bash
curl -sSL https://ludolph.dev/install | bash
```

Or if you built from source:

```bash
lu setup
```

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

## After Setup

Once Lu is running, you might want to:

- **Build the vault index** — `lu index` gives Lu semantic search over your notes
- **Set up jetpacks** — Automated workflows like morning briefs. See [Jetpacks](jetpacks.md)
- **Install plugins** — Email, Slack, calendar integrations. See `lu plugin search`
- **Teach Lu about yourself** — Just talk to Lu in Telegram. Mention your preferences, your projects, your timezone. Lu saves observations and remembers them across conversations

## Reconfiguring

Update specific parts without redoing everything:

```bash
lu setup credentials   # API keys and tokens
lu setup pi            # Pi connection
lu setup mcp           # MCP server on Mac
lu config              # Edit config directly
```

## Something broke?

See [Troubleshooting](troubleshooting.md) for the full decision tree. Quick fixes:

```bash
lu doctor              # Diagnose everything at once
lu doctor --fix        # Auto-fix what it can
lu mcp restart         # Restart the MCP server
```

If multiple things are broken, `lu uninstall --all && lu setup` starts fresh without touching your vault, SSH keys, or Tailscale.
