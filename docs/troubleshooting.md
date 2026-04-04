# Troubleshooting

Things break. Lu involves a Pi, a Mac, a network, an API, and a Telegram bot — that's five things that can go wrong independently. Start here:

```bash
lu doctor
```

This checks everything and tells you which piece is unhappy. If `lu doctor` itself fails, you probably have a config problem (jump to [Configuration Issues](#configuration-issues)).

For issues with the learn/teach pipeline, see [Learning and Teaching](learn.md).


## Configuration Issues

### Config Missing {#config-missing}

**Symptom:** `lu doctor` shows "Config file not found"

**Cause:** Lu hasn't been set up yet or config was deleted.

**Fix:**

```bash
lu setup
```

### Config Invalid {#config-invalid}

**Symptom:** `lu doctor` shows "Could not load config file"

**Cause:** TOML syntax error or missing required fields.

**Fix:**

1. Check the config file:
   ```bash
   cat ~/.ludolph/config.toml
   ```

2. Validate TOML syntax (look for missing quotes, brackets, etc.)

3. Re-run setup to fix:
   ```bash
   lu setup credentials
   ```

### Telegram Token Empty {#config-telegram}

**Symptom:** `lu doctor` shows "Telegram bot token is empty"

**Fix:**

```bash
lu setup credentials
```

Enter your bot token from [@BotFather](https://t.me/BotFather).

### Claude API Key Empty {#config-claude}

**Symptom:** `lu doctor` shows "Claude API key is empty"

**Fix:**

```bash
lu setup credentials
```

Enter your API key from the [Anthropic Console](https://console.anthropic.com/).

### No Allowed Users {#config-users}

**Symptom:** `lu doctor` shows "No allowed Telegram users configured"

**Cause:** Bot won't respond to anyone without allowed user IDs.

**Fix:**

1. Get your Telegram user ID by messaging [@userinfobot](https://t.me/userinfobot)

2. Add it during setup:
   ```bash
   lu setup credentials
   ```

---

## Vault Issues

### Vault Missing {#vault-missing}

**Symptom:** `lu doctor` shows "Vault not found at..."

**Causes:**
- Wrong path in config
- Vault directory was moved or deleted
- External drive not mounted

**Fix:**

1. Check if the vault exists:
   ```bash
   ls ~/path/to/your/vault
   ```

2. Update the path:
   ```bash
   lu setup credentials
   ```

### Vault Not Directory {#vault-not-dir}

**Symptom:** `lu doctor` shows "Vault path is not a directory"

**Cause:** Config points to a file instead of a directory.

**Fix:**

Set the vault path to your Obsidian vault folder (not a specific file):

```bash
lu setup credentials
```

### Vault Empty {#vault-empty}

**Symptom:** `lu doctor` shows "Vault is empty"

**Cause:** No files in the vault directory.

**Fix:**

Add some notes to your vault, or verify you're pointing to the correct directory.

---

## Pi Connectivity Issues

### Pi Offline {#pi-offline}

**Symptom:** `lu doctor` shows "Pi unreachable"

**Causes:**
- Pi lost power
- Pi not connected to network
- Tailscale not running on Pi after reboot

**Fixes:**

1. **Check if Pi has power:** Look for status lights.

2. **If Pi rebooted**, Tailscale may need to be started:

   Physically access the Pi and run:
   ```bash
   sudo tailscale up
   ```

3. **Enable Tailscale autostart** (prevents this issue):
   ```bash
   sudo systemctl enable tailscaled
   ```

4. **If using local network**, check the Pi's IP hasn't changed:
   ```bash
   ping raspberrypi.local
   ```

### SSH Auth Failed {#pi-ssh-auth}

**Symptom:** `lu doctor` shows "SSH key auth failed"

**Cause:** SSH key not set up or wrong key.

**Fix:**

1. Copy your SSH key to the Pi:
   ```bash
   ssh-copy-id pi@YOUR_PI_IP
   ```

2. Test connection:
   ```bash
   ssh pi@YOUR_PI_IP
   ```

### SSH Error {#pi-ssh-error}

**Symptom:** `lu doctor` shows "SSH connection failed"

**Debug:**

```bash
ssh -v pi@YOUR_PI_IP
```

Common issues:
- Hostname changed (use IP instead)
- SSH service not running on Pi
- Firewall blocking port 22

---

## Pi Service Issues

### Service Stopped {#pi-service-stopped}

**Symptom:** `lu doctor` shows "Pi service status: inactive"

**Fix:**

Start the service:
```bash
ssh pi@YOUR_PI_IP 'systemctl --user start ludolph.service'
```

Enable autostart:
```bash
ssh pi@YOUR_PI_IP 'systemctl --user enable ludolph.service'
```

### Service Missing {#pi-service-missing}

**Symptom:** `lu doctor` shows "Pi service not found"

**Cause:** Service was never deployed or was removed.

**Fix:**

Redeploy to Pi:
```bash
lu setup deploy
```

---

## MCP Server Issues

### MCP No Token {#mcp-no-token}

**Symptom:** `lu doctor` shows "No MCP auth token found"

**Cause:** Token files missing from ~/.ludolph/

**Fix:**

Regenerate MCP setup:
```bash
lu setup mcp
```

### MCP Unreachable {#mcp-unreachable}

**Symptom:** `lu doctor` shows "Mac MCP server not responding"

**Causes:**
- MCP server crashed
- Service not started
- Port blocked by firewall

**Fixes:**

1. **Check MCP logs:**
   ```bash
   tail -f ~/.ludolph/mcp/mcp_server.log
   ```

2. **Restart MCP service:**
   ```bash
   launchctl kickstart gui/$(id -u)/dev.ludolph.mcp
   ```

3. **Or use the CLI:**
   ```bash
   lu mcp restart
   ```

4. **If service won't start**, reinstall:
   ```bash
   lu setup mcp
   ```

### Pi Cannot Reach MCP {#pi-mcp-connectivity}

**Symptom:** `lu doctor` shows "Pi cannot reach Mac MCP"

**Causes:**
- Mac is asleep
- Firewall blocking connection
- Wrong MCP URL in Pi config
- Auth token mismatch

**Fixes:**

1. **Wake the Mac:** Move mouse, press key, or use Wake-on-LAN.

2. **Check Mac firewall:** System Settings → Network → Firewall
   - Ensure Python is allowed for incoming connections

3. **Check the URL:** On Pi, verify config:
   ```bash
   cat ~/.ludolph/config.toml | grep url
   ```

4. **If auth mismatch:**
   ```bash
   # On Mac
   lu setup mcp
   lu setup deploy
   ```

### MCP Auth Mismatch {#mcp-auth-mismatch}

**Symptom:** `lu doctor` shows "Pi rejected by Mac MCP (auth token mismatch)"

**Cause:** Auth token on Pi doesn't match Mac's token.

**Fix:**

Regenerate and redeploy:
```bash
lu setup mcp
lu setup deploy
```

---

## Telegram Issues

### Bot Not Responding {#telegram-silent}

**Symptom:** Messages sent to bot get no response.

**Debug steps:**

1. Run diagnostics:
   ```bash
   lu doctor
   ```

2. Check Pi service:
   ```bash
   ssh pi@YOUR_PI_IP 'systemctl --user status ludolph.service'
   ```

3. Check Pi logs:
   ```bash
   ssh pi@YOUR_PI_IP 'tail -f ~/.ludolph/ludolph.log'
   ```

4. Verify your user ID is in allowed_users:
   ```bash
   cat ~/.ludolph/config.toml | grep allowed_users
   ```

### Wrong Bot Token {#telegram-token}

**Symptom:** Bot starts but Telegram says "Bot not found"

**Fix:**

1. Create a new bot with [@BotFather](https://t.me/BotFather)
2. Update the token:
   ```bash
   lu setup credentials
   ```

---

## Lu Says Something Wrong

### "Telegram isn't configured"

If Lu says this in a scheduled message delivered via Telegram — yes, the irony. This was a bug in v0.12.x where scheduled tasks didn't know they were being delivered through Telegram. Fixed in v0.13.0. Update with `lu update`.

### Lu ignores your preferences

Lu stores preferences as observations. Check what it knows:

Ask Lu in Telegram: "What do you know about me?"

If observations are empty or wrong, tell Lu directly: "Remember that I prefer X." If observations aren't saving at all, check the MCP server logs for SQLite errors.

### Lu can't find something in your vault

The vault index might be stale or missing:

```bash
lu index --status          # Check if index exists
lu index                   # Rebuild if needed
lu knowledge               # See what Lu knows overall
```

If you recently added files, the file watcher should pick them up within 5 seconds. If it doesn't, `lu index --rebuild` forces a clean rebuild.

### Lu hallucinates file paths

Lu sometimes invents paths that don't exist. This is a model behavior, not a Lu bug. The sandbox prevents any damage — Lu can't write to files that don't exist without creating them first, and `lu doctor` will catch path issues.

If it keeps happening, try building a deeper index: `lu index --tier deep` generates AI summaries per chunk, giving Lu better context for where things actually are.

## Clean Install

If multiple things are broken, start fresh:

```bash
lu uninstall --all
lu setup
```

This preserves your vault, SSH keys, and Tailscale configuration. Your observations and learned content at `~/.ludolph/` are also preserved unless you manually delete them.
