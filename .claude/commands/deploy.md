---
name: deploy
description: Download release binaries and install on Pi and Mac
---

# /deploy - Deploy Release Binaries

Download pre-built binaries from the latest GitHub release and install on Pi and Mac.

## Prerequisites

- Release exists on GitHub (run `/release` first)
- Pi SSH configured (`ssh pi` works)
- GitHub CLI configured (`gh auth status`)

## Process

1. Get latest release version
2. Download and install Pi binary
3. Download and install MCP on Mac
4. Verify both installations
5. Print summary

## Steps

### Step 1: Get Latest Release

Get latest release tag:
```bash
VERSION=$(gh release view --json tagName -q '.tagName')
echo "Latest release: ${VERSION}"
```

If no release found, abort: "No releases found. Run /release first."

List available assets:
```bash
gh release view ${VERSION} --json assets -q '.assets[].name'
```

### Step 2: Download and Install Pi Binary

Create temp directory:
```bash
TMPDIR=$(mktemp -d)
cd ${TMPDIR}
```

Download ARM Linux binary:
```bash
gh release download ${VERSION} -p "lu-aarch64-unknown-linux-gnu"
```

If download fails, abort: "Failed to download Pi binary. Check release assets."

Copy to Pi:
```bash
scp lu-aarch64-unknown-linux-gnu pi:~/.ludolph/bin/lu.new
```

Install and restart service:
```bash
ssh pi "chmod +x ~/.ludolph/bin/lu.new && mv ~/.ludolph/bin/lu.new ~/.ludolph/bin/lu && systemctl --user restart ludolph.service"
```

Wait and verify:
```bash
sleep 3
PI_VERSION=$(ssh pi "~/.ludolph/bin/lu --version 2>/dev/null || echo 'FAILED'")
echo "Pi version: ${PI_VERSION}"
```

### Step 3: Download and Install MCP on Mac

Download MCP package:
```bash
gh release download ${VERSION} -p "ludolph-mcp-*.tar.gz"
```

If download fails, abort: "Failed to download MCP package. Check release assets."

Determine MCP install location (check Claude Code config or use default):
```bash
MCP_DIR="${HOME}/.config/claude-code/mcp/ludolph"
mkdir -p ${MCP_DIR}
```

Extract and install:
```bash
tar -xzf ludolph-mcp-*.tar.gz -C ${MCP_DIR} --strip-components=1
```

Verify MCP version file:
```bash
MCP_VERSION=$(cat ${MCP_DIR}/VERSION 2>/dev/null || echo "UNKNOWN")
echo "MCP version: ${MCP_VERSION}"
```

### Step 4: Cleanup

Remove temp directory:
```bash
rm -rf ${TMPDIR}
```

### Step 5: Print Summary

```
Deploy complete!

Pi (aarch64-linux):
  Binary: ~/.ludolph/bin/lu
  Version: ${PI_VERSION}
  Service: ludolph.service (restarted)

Mac MCP:
  Location: ${MCP_DIR}
  Version: ${MCP_VERSION}

To verify Pi bot:
  ssh pi "journalctl --user -u ludolph.service -n 20"

To test Pi bot:
  Send a message to your Telegram bot

To verify MCP:
  Check Claude Code can use ludolph MCP tools
```

## Troubleshooting

### Pi binary won't run

```bash
# Check permissions
ssh pi "ls -la ~/.ludolph/bin/lu"

# Check architecture
ssh pi "file ~/.ludolph/bin/lu"

# Run manually
ssh pi "~/.ludolph/bin/lu check"
```

### MCP not recognized

```bash
# Check files exist
ls -la ~/.config/claude-code/mcp/ludolph/

# Check VERSION file
cat ~/.config/claude-code/mcp/ludolph/VERSION

# Restart Claude Code to pick up new MCP
```

### Service won't start

```bash
# Pi logs
ssh pi "journalctl --user -u ludolph.service -n 50"

# Pi service status
ssh pi "systemctl --user status ludolph.service"
```
