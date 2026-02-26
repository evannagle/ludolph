# Enhanced Release Process Design

## Overview

Codify the release process to ensure stable builds by enhancing the `/release` skill with pre-flight validation, automation, and documentation.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    /release command                      │
├─────────────────────────────────────────────────────────┤
│  1. Pre-flight validation (local + Docker ARM test)     │
│  2. Version bump + commit + push                        │
│  3. Create GitHub release (triggers CI build)           │
│  4. Monitor CI until all builds pass                    │
└─────────────────────────────────────────────────────────┘
```

**Supporting files:**
- `RELEASE.md` - Documents dependencies, runner requirements, known issues
- `.claude/commands/release.md` - Enhanced skill with full automation
- `Cargo.toml` comments - Explains vendored OpenSSL requirement

## Components

### 1. Pre-flight Validation

| Check | Purpose | Blocking? |
|-------|---------|-----------|
| Branch is `develop` | Ensure correct source | Yes |
| Working directory clean | No uncommitted changes | Yes |
| `cargo fmt --check` | Code formatting | Yes |
| `cargo clippy` | Lints pass | Yes |
| `cargo test` | Tests pass | Yes |
| Python syntax check | MCP files valid | Yes |
| Pi ARM build (SSH) | Real ARM compilation | Yes |
| Version not already released | Prevent duplicate tags | Yes |

**Optional Docker ARM cross-compilation test:**

If Docker is available, test ARM cross-compilation locally before pushing:
```bash
docker run --rm -v $(pwd):/app -w /app \
  ghcr.io/cross-rs/aarch64-unknown-linux-gnu:main \
  cargo build --release --target aarch64-unknown-linux-gnu
```

This catches OpenSSL/cross-compilation issues before they hit CI.

### 2. Automated Release Flow

After validation passes:

1. **Bump version** - Increment patch in `Cargo.toml` (or prompt for minor/major)
2. **Create commit** - `chore: release vX.Y.Z`
3. **Push to both branches** - `develop` and `production`
4. **Create GitHub release** - Using `gh release create vX.Y.Z`
5. **Monitor CI** - Poll workflow status until all 5 jobs complete
6. **Report results** - Show pass/fail for each platform

**Release notes generation:**
- Extract commits since last tag using `git log`
- Group by type (feat, fix, etc.)
- Format as markdown for release body

**Example output:**
```
Pre-flight: passed
Version: 0.5.6 → 0.5.7
Commit: chore: release v0.5.7
Pushed: develop, production
Release: https://github.com/evannagle/ludolph/releases/tag/v0.5.7

Monitoring CI...
  package-mcp: passed (7s)
  x86_64-linux: passed (3m54s)
  aarch64-linux: passed (4m52s)
  x86_64-macos: passed (14m8s)
  aarch64-macos: passed (4m37s)

Release v0.5.7 complete. All 5 assets uploaded.
```

### 3. Documentation (RELEASE.md)

Single source of truth for CI requirements:

```markdown
# Release Process

## Quick Start
Run `/release` in Claude Code. It handles everything.

## CI Requirements

### GitHub Runners
| Target | Runner | Notes |
|--------|--------|-------|
| x86_64-linux | ubuntu-latest | Native build |
| aarch64-linux | ubuntu-latest | Uses `cross` tool |
| x86_64-macos | macos-15-intel | Intel hardware required |
| aarch64-macos | macos-latest | ARM (Apple Silicon) |

### Dependencies
- `openssl` with `vendored` feature - Required for ARM cross-compilation
- `cross` - Installed at build time for ARM Linux

### Known Issues
- macos-13 retired Dec 2025 - Use macos-15-intel for x86_64
- ARM cross-compilation needs vendored OpenSSL - anthropic-sdk-rust pulls native-tls

## Manual Release (if needed)
1. Bump version in Cargo.toml
2. Commit and push to develop + production
3. `gh release create vX.Y.Z --generate-notes`
4. Monitor workflow at Actions tab
```

### 4. Deploy Command

**`/deploy` - Install and verify new binaries:**

```
┌─────────────────────────────────────────────────────────┐
│                    /deploy command                       │
├─────────────────────────────────────────────────────────┤
│  1. Download latest release binaries                    │
│  2. Install on Pi (ARM Linux) + restart bot             │
│  3. Install MCP on Mac                                  │
│  4. Verify both are running correct version             │
└─────────────────────────────────────────────────────────┘
```

**Steps:**

| Step | Command | Verification |
|------|---------|--------------|
| Get latest version | `gh release view --json tagName` | Tag exists |
| Download Pi binary | `gh release download -p lu-aarch64-unknown-linux-gnu` | File downloaded |
| Install on Pi | `scp lu-* pi:~/bin/lu && ssh pi "chmod +x ~/bin/lu"` | File executable |
| Restart Pi bot | `ssh pi "sudo systemctl restart ludolph"` | Service running |
| Verify Pi | `ssh pi "lu --version"` | Shows correct version |
| Download MCP | `gh release download -p ludolph-mcp-*.tar.gz` | File downloaded |
| Install MCP | Extract to configured MCP location | Files in place |
| Verify MCP | Check MCP server responds | Server healthy |

**Example output:**
```
Latest release: v0.5.7

Pi (aarch64-linux):
  Downloaded: lu-aarch64-unknown-linux-gnu
  Installed: /home/pi/bin/lu
  Restarted: ludolph.service
  Version: Ludolph v0.5.7 ✓

Mac MCP:
  Downloaded: ludolph-mcp-v0.5.7.tar.gz
  Installed: ~/.config/claude-code/mcp/ludolph/
  Version: 0.5.7 ✓

Deploy complete.
```

## Files to Modify

| File | Change |
|------|--------|
| `.claude/commands/release.md` | Enhanced skill with automation and monitoring |
| `.claude/commands/deploy.md` | NEW: Deploy and verify binaries |
| `RELEASE.md` | NEW: CI requirements documentation |
| `Cargo.toml` | Add comment explaining vendored OpenSSL |

## Success Criteria

- `/release` validates, bumps version, creates release, and monitors CI in one command
- `/deploy` installs binaries on Pi and Mac, verifies correct versions
- CI issues are caught before pushing (pre-flight validation)
- `RELEASE.md` documents all runner and dependency requirements
- No manual GitHub release creation or binary installation needed
