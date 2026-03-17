# lu update - Self-Update Mechanism Design

## Summary

Add a `lu update` command that allows users to update the Lu binary and MCP server to the latest release without needing Claude Code or manual intervention.

## Problem Statement

Currently, updating Lu requires:
- Running `/deploy` skill from Claude Code (users don't have this)
- Or manually downloading binaries from GitHub releases
- Or re-running the installer script

Users need a simple `lu update` command that handles everything.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Scope | Mac updates both Mac + Pi | Mac is control plane, Pi is worker |
| Confirmation | Always confirm | Safest for end users |
| Versioning | Latest only | Simpler, fewer edge cases |
| Service restarts | Automatic | Seamless update experience |

## Architecture

```
lu update
    │
    ├─► Check GitHub for latest release
    │
    ├─► Compare versions (Mac binary, MCP, Pi binary)
    │
    ├─► Show update summary, confirm with user
    │
    ├─► Update Mac binary (if needed)
    │       └─► Download to temp, atomic replace
    │
    ├─► Update MCP server (if needed)
    │       └─► Reuse existing mcp_update() function
    │
    ├─► Update Pi binary via SSH (if needed)
    │       └─► Download on Pi → replace → restart systemd
    │
    └─► Health check all services
```

## Components

### Version Detection

**Current version sources:**
- Mac binary: `lu --version` (from Cargo.toml at compile time)
- MCP server: `~/.ludolph/mcp/VERSION` file
- Pi binary: `ssh pi 'lu --version'`

**Latest version source:**
- GitHub API: `https://api.github.com/repos/evannagle/ludolph/releases/latest`

**Comparison:**
- Strip `v` prefix from tags
- Simple string comparison (semver-compatible)
- Skip component if already at latest

### Binary Update (Mac)

**Platform detection (compile-time):**
```rust
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
const BINARY_NAME: &str = "lu-x86_64-apple-darwin";

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const BINARY_NAME: &str = "lu-aarch64-apple-darwin";
```

**Update process:**
1. Download binary to temp file (`/tmp/lu-update-XXXXX`)
2. Verify download succeeded (size > 0)
3. Make executable (`chmod +x`)
4. Get path to current binary (`std::env::current_exe()`)
5. Atomic replace: rename temp to current path
6. Verify with subprocess: `lu --version`

**Self-update note:** On macOS, a running binary CAN replace itself via rename. The old binary stays in memory until the process exits.

### MCP Update

Reuse existing `mcp_update()` function from `src/cli/commands.rs`:
1. Check current version from `~/.ludolph/mcp/VERSION`
2. Fetch latest release tag from GitHub
3. Download tarball
4. Backup current MCP to `mcp.bak`
5. Extract new MCP
6. Restart launchd service
7. Remove backup on success, restore on failure

### Pi Update via SSH

**Precondition:** Pi must be configured in config and reachable.

**Process:**
```bash
# 1. Download binary directly on Pi
ssh pi "curl -sSL -o /tmp/lu 'https://github.com/evannagle/ludolph/releases/download/{tag}/lu-aarch64-unknown-linux-gnu'"

# 2. Make executable and replace
ssh pi "chmod +x /tmp/lu && mv /tmp/lu ~/.ludolph/bin/lu"

# 3. Restart service
ssh pi "systemctl --user restart ludolph.service"

# 4. Verify
ssh pi "~/.ludolph/bin/lu --version"
```

**Skip conditions:**
- No Pi configured in config
- Pi unreachable (SSH fails)
- Pi already at latest version

## User Interface

### Update available
```
$ lu update

lu 0.9.5 → 0.9.6

Checking for updates...
[•ok] Current: lu v0.9.5, MCP v0.9.5, Pi v0.9.5
[•ok] Latest: v0.9.6

Updates available:
  - Mac binary: 0.9.5 → 0.9.6
  - MCP server: 0.9.5 → 0.9.6
  - Pi binary: 0.9.5 → 0.9.6

Proceed with update? [y/N] y

[31415] Updating Mac binary...
[•ok] Mac binary updated

[31415] Updating MCP server...
[•ok] MCP server updated
[•ok] MCP service restarted

[31415] Updating Pi (100.x.x.x)...
[•ok] Pi binary updated
[•ok] Pi service restarted

[•ok] All updates complete. Running v0.9.6
```

### Already up-to-date
```
$ lu update

lu 0.9.6

[•ok] Already up to date (v0.9.6)
```

### Pi unreachable
```
$ lu update

lu 0.9.5 → 0.9.6

...
[•ok] Mac binary updated
[•ok] MCP server updated
[•!!] Pi unreachable - skipping Pi update
      Update Pi manually: ssh pi 'curl ... | bash'

[•ok] Mac/MCP updated to v0.9.6. Pi still at v0.9.5.
```

## Error Handling

| Scenario | Handling |
|----------|----------|
| GitHub unreachable | Show error, suggest checking network |
| Mac download fails | Abort, no changes made |
| Mac binary replacement fails | Keep old binary, show error with path |
| MCP update fails | Restore from backup (existing behavior) |
| Pi unreachable | Skip Pi update, warn user, continue |
| Pi download fails | Skip Pi update, warn user, continue |
| Pi service won't restart | Warn user, show manual restart command |

**Partial failure:** If Mac succeeds but Pi fails, clearly communicate what succeeded and what needs manual attention.

## Files to Create/Modify

| File | Change |
|------|--------|
| `src/cli/commands.rs` | Add `update()` function |
| `src/cli.rs` | Add `Update` command variant |
| `src/main.rs` | Wire up update command |

## Testing

### Manual test cases
1. `lu update` when already at latest → shows "up to date"
2. `lu update` with older version → downloads and updates all components
3. `lu update` with Pi unreachable → updates Mac/MCP, skips Pi with warning
4. `lu update` with no network → shows network error

### Automated tests
- Unit tests for version comparison logic
- Unit tests for platform detection (compile-time, so just verify constants)
