---
name: deploy
description: Deploy MCP to Mac and restart bot on Pi after a release
---

# /deploy - Deploy to Production

Deploy the MCP server to Mac and restart the bot service on Pi.

## Prerequisites

- Release has been pushed to production (run `/release` first)
- Pi SSH configured (`ssh pi` works)
- MCP launchd service configured on Mac (`dev.ludolph.mcp`)
- Bot systemd service configured on Pi (`ludolph.service`)

## Process

1. Verify prerequisites
2. Deploy MCP to Mac:
   - Stop launchctl service
   - Copy src/mcp/ to deployment location
   - Start launchctl service
   - Verify health endpoint
3. Deploy bot to Pi:
   - Pull latest code
   - Restart systemd service
   - Verify bot is running
4. Print summary

## Steps

### Step 1: Verify prerequisites

Check that this is being run from the ludolph repo:
```bash
test -f Cargo.toml && grep -q "ludolph" Cargo.toml
```

Check Pi SSH:
```bash
ssh pi "echo ok" 2>/dev/null
```

### Step 2: Deploy MCP to Mac

The MCP server runs locally on Mac. The source is at `src/mcp/`.

Stop the MCP service:
```bash
launchctl stop dev.ludolph.mcp 2>/dev/null || true
```

The MCP code is already in place (same repo). Just restart:
```bash
launchctl start dev.ludolph.mcp
```

Wait 2 seconds, then verify health:
```bash
sleep 2
curl -s http://localhost:8200/ | grep -q "running" && echo "MCP OK" || echo "MCP FAILED"
```

If MCP has a different port configured, adjust accordingly.

### Step 3: Deploy bot to Pi

Pull latest code on Pi:
```bash
ssh pi "cd ~/ludolph && git pull origin production"
```

The binary was built during `/release` at `~/ludolph/target/release/lu`.
Copy it to the install location used by the systemd service:
```bash
ssh pi "systemctl --user stop ludolph.service && cp ~/ludolph/target/release/lu ~/.ludolph/bin/lu && systemctl --user start ludolph.service"
```

Wait 3 seconds, then verify:
```bash
sleep 3
ssh pi "systemctl --user is-active ludolph.service && ~/.ludolph/bin/lu --version"
```

### Step 4: Print summary

Print:
```
Deployment complete:
  MCP (Mac): [status]
  Bot (Pi):  [status]

To check logs:
  Mac MCP: tail -f ~/Library/Logs/ludolph-mcp.log
  Pi Bot:  ssh pi "journalctl --user -u ludolph.service -f"
```

## Troubleshooting

### MCP won't start
```bash
# Check logs
tail -50 ~/Library/Logs/ludolph-mcp.log

# Check if port is in use
lsof -i :8200

# Manual start for debugging
cd ~/Repos/ludolph && VAULT_PATH=~/Vault AUTH_TOKEN=xxx python -m src.mcp.server
```

### Bot won't start
```bash
# Check logs
ssh pi "journalctl --user -u ludolph.service -n 50"

# Check config
ssh pi "cat ~/.ludolph/config.toml"

# Manual start for debugging
ssh pi "cd ~/ludolph && ./target/release/lu bot"
```
