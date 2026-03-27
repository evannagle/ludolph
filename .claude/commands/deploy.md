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

### Step 2b: Pi Smoke Tests

Run these checks after install. If ANY fails, stop and report the failure.

**Version check:**
```bash
sleep 3
PI_VERSION=$(ssh pi "~/.ludolph/bin/lu --version 2>/dev/null || echo 'FAILED'")
echo "Pi version: ${PI_VERSION}"
```
If `FAILED` or version doesn't contain the expected release number, abort: "Pi binary failed to run. Check `ssh pi '~/.ludolph/bin/lu --version'`"

**Service check:**
```bash
PI_SERVICE=$(ssh pi "systemctl --user is-active ludolph.service 2>/dev/null || echo 'FAILED'")
echo "Pi service: ${PI_SERVICE}"
```
If not `active`, abort: "Pi service not running. Check `ssh pi 'journalctl --user -u ludolph.service -n 50'`"

**Doctor check:**
```bash
ssh pi "~/.ludolph/bin/lu doctor 2>&1"
```
Run `lu doctor` on Pi. If exit code is non-zero or output contains `[•!!]`, report which checks failed but continue (doctor failures may be pre-existing).

**Index command check (new feature gate):**
```bash
ssh pi "~/.ludolph/bin/lu index --status 2>&1"
```
Verify the command runs without error. This confirms the new index module and its dependencies (pulldown-cmark, notify, xxhash, serde_yaml) are working on ARM. If it fails, abort: "lu index command failed on Pi — new dependencies may have ARM build issues."

All Pi smoke tests passed? Print: "Pi smoke tests passed (version, service, doctor, index)"

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

### Step 3b: Mac MCP Smoke Tests

**Version match:**
Compare `${MCP_VERSION}` with expected release version (strip leading `v`). If mismatch, warn: "MCP VERSION file doesn't match release."

**Health endpoint:**
```bash
MCP_HEALTH=$(curl -s -o /dev/null -w "%{http_code}" http://localhost:8202/health 2>/dev/null || echo "000")
echo "MCP health: ${MCP_HEALTH}"
```
If not `200`, try restarting:
```bash
launchctl kickstart -k gui/$(id -u)/dev.ludolph.mcp
sleep 3
MCP_HEALTH=$(curl -s -o /dev/null -w "%{http_code}" http://localhost:8202/health 2>/dev/null || echo "000")
```
If still not `200`, warn: "MCP server not responding. Check `cat ~/.ludolph/mcp/server.log`"

**Launchd service:**
```bash
launchctl list dev.ludolph.mcp 2>/dev/null
```
If exit code non-zero, warn: "MCP launchd service not loaded."

All Mac smoke tests passed? Print: "Mac MCP smoke tests passed (version, health, service)"

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
