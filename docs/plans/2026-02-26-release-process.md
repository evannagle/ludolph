# Enhanced Release Process Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Automate the release process with pre-flight validation, version bumping, CI monitoring, and binary deployment.

**Architecture:** Four files - enhanced `/release` command, enhanced `/deploy` command, `RELEASE.md` documentation, and Cargo.toml comment. The `/release` command handles everything from validation to CI monitoring; `/deploy` downloads pre-built binaries and installs them.

**Tech Stack:** Claude Code skills (markdown), GitHub CLI (`gh`), Bash

---

### Task 1: Create RELEASE.md Documentation

**Files:**
- Create: `RELEASE.md`

**Step 1: Write the documentation file**

```markdown
# Release Process

## Quick Start

Run `/release` in Claude Code. It handles everything:
1. Validates code (fmt, clippy, tests, Python syntax)
2. Tests ARM build on Pi
3. Bumps version and commits
4. Creates GitHub release
5. Monitors CI until all builds pass

Then run `/deploy` to install binaries on Pi and Mac.

## CI Requirements

### GitHub Runners

| Target | Runner | Notes |
|--------|--------|-------|
| x86_64-linux | ubuntu-latest | Native build |
| aarch64-linux | ubuntu-latest | Uses `cross` tool with Docker |
| x86_64-macos | macos-15-intel | Intel hardware (macos-13 retired Dec 2025) |
| aarch64-macos | macos-latest | ARM (Apple Silicon) |

### Dependencies

**Cargo.toml:**
- `openssl = { version = "0.10", features = ["vendored"] }` - Required for ARM cross-compilation. The `anthropic-sdk-rust` crate pulls in `native-tls` which requires OpenSSL. Vendored feature compiles OpenSSL from source, avoiding system library requirements.

**CI Workflow:**
- `cross` - Installed at build time for ARM Linux cross-compilation
- `PKG_CONFIG_ALLOW_CROSS=1` - Environment variable for cross builds

### Known Issues

| Issue | Cause | Solution |
|-------|-------|----------|
| macos-13 not found | Runner retired Dec 2025 | Use `macos-15-intel` |
| ARM build fails with OpenSSL error | `native-tls` requires system OpenSSL | Add `openssl` with `vendored` feature |
| cross requires newer Rust | Version mismatch | Use `toolchain: stable` |

## Manual Release (if needed)

If `/release` fails or you need manual control:

```bash
# 1. Bump version
sed -i '' 's/version = ".*"/version = "X.Y.Z"/' Cargo.toml

# 2. Commit and push
git add Cargo.toml
git commit -m "chore: release vX.Y.Z"
git push origin develop
git push origin develop:production

# 3. Create release
gh release create vX.Y.Z --generate-notes

# 4. Monitor at https://github.com/evannagle/ludolph/actions
```

## Manual Deploy (if needed)

```bash
# Download and install Pi binary
gh release download vX.Y.Z -p lu-aarch64-unknown-linux-gnu
scp lu-aarch64-unknown-linux-gnu pi:~/.ludolph/bin/lu
ssh pi "chmod +x ~/.ludolph/bin/lu && systemctl --user restart ludolph.service"
ssh pi "~/.ludolph/bin/lu --version"

# Download and install MCP
gh release download vX.Y.Z -p 'ludolph-mcp-*.tar.gz'
tar -xzf ludolph-mcp-*.tar.gz -C ~/.config/claude-code/mcp/ludolph/
```
```

**Step 2: Verify file was created**

Run: `head -20 RELEASE.md`
Expected: Shows the Quick Start section

**Step 3: Commit**

```bash
git add RELEASE.md
git commit -m "docs: add RELEASE.md with CI requirements and troubleshooting"
```

---

### Task 2: Add Cargo.toml Comment

**Files:**
- Modify: `Cargo.toml:39-40`

**Step 1: Read current state**

Run: `grep -n "openssl" Cargo.toml`
Expected: Shows line with openssl dependency

**Step 2: Add explanatory comment**

Change:
```toml
openssl = { version = "0.10", features = ["vendored"] }
```

To:
```toml
# Required for ARM cross-compilation - anthropic-sdk-rust pulls native-tls which needs OpenSSL.
# Vendored feature compiles OpenSSL from source, avoiding system library requirements in CI.
openssl = { version = "0.10", features = ["vendored"] }
```

**Step 3: Verify change**

Run: `grep -B2 "openssl" Cargo.toml`
Expected: Shows comment above openssl line

**Step 4: Commit**

```bash
git add Cargo.toml
git commit -m "docs: add comment explaining vendored OpenSSL requirement"
```

---

### Task 3: Update /release Command

**Files:**
- Modify: `.claude/commands/release.md`

**Step 1: Replace entire file with enhanced version**

```markdown
---
name: release
description: Validate, version bump, create release, and monitor CI
---

# /release - Release to Production

Validate the codebase, bump version, create GitHub release, and monitor CI until all builds pass.

## Prerequisites

- On `develop` branch
- Working directory clean
- Pi SSH configured (`ssh pi` works)
- GitHub CLI configured (`gh auth status`)

## Process

1. Pre-flight validation
2. Check version not already released
3. Bump version (prompt for major/minor/patch)
4. Commit and push to develop + production
5. Create GitHub release
6. Monitor CI until complete
7. Print deploy instructions

## Steps

### Step 1: Pre-flight Validation

Check branch:
```bash
git branch --show-current
```
Must be `develop`. If not, abort: "Must be on develop branch to release."

Check working directory:
```bash
git status --porcelain
```
Must be empty. If not, abort: "Working directory not clean. Commit or stash changes first."

Run Rust checks (stop on first failure):
```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

Run Python checks:
```bash
find src/mcp -name "*.py" -exec python3 -m py_compile {} \;
```

Test ARM build on Pi:
```bash
ssh pi "source ~/.cargo/env && cd ~/ludolph && git fetch origin develop && git checkout origin/develop && cargo build --release"
ssh pi "~/.ludolph/bin/lu --version || ~/ludolph/target/release/lu --version"
```

If any check fails, abort with error output.

### Step 2: Check Version Not Released

Get current version:
```bash
grep '^version' Cargo.toml | head -1 | cut -d'"' -f2
```

Check if tag exists:
```bash
gh release view v${VERSION} 2>/dev/null && echo "EXISTS" || echo "NEW"
```

If EXISTS, abort: "Version ${VERSION} already released. Bump version first."

### Step 3: Prompt for Version Bump

Display current version and ask:
```
Current version: 0.5.6

Version bump type?
1. patch (0.5.6 → 0.5.7) - Bug fixes
2. minor (0.5.6 → 0.6.0) - New features
3. major (0.5.6 → 1.0.0) - Breaking changes
```

Wait for user selection (default: patch).

Calculate new version and update Cargo.toml:
```bash
# For patch bump example:
NEW_VERSION="0.5.7"
sed -i '' "s/^version = \".*\"/version = \"${NEW_VERSION}\"/" Cargo.toml
```

### Step 4: Commit and Push

Create commit:
```bash
git add Cargo.toml
git commit -m "chore: release v${NEW_VERSION}"
```

Push to both branches:
```bash
git push origin develop
git push origin develop:production
```

### Step 5: Create GitHub Release

Generate release notes from commits since last tag:
```bash
LAST_TAG=$(git describe --tags --abbrev=0 2>/dev/null || echo "")
if [ -n "$LAST_TAG" ]; then
  NOTES=$(git log ${LAST_TAG}..HEAD --pretty=format:"- %s" | grep -E "^- (feat|fix|docs|refactor):")
else
  NOTES="Initial release"
fi
```

Create release:
```bash
gh release create v${NEW_VERSION} --title "v${NEW_VERSION}" --notes "${NOTES}"
```

Print release URL.

### Step 6: Monitor CI

Get workflow run ID:
```bash
sleep 5  # Wait for workflow to start
RUN_ID=$(gh run list --limit 1 --json databaseId -q '.[0].databaseId')
```

Poll until complete (check every 30 seconds, timeout after 20 minutes):
```bash
gh run view ${RUN_ID}
```

Print status for each job as it completes:
```
Monitoring CI...
  package-mcp: passed (7s)
  x86_64-linux: passed (3m54s)
  aarch64-linux: running...
  x86_64-macos: running...
  aarch64-macos: passed (4m37s)
```

If any job fails, print error and link to logs.

### Step 7: Print Deploy Instructions

If all jobs passed:
```
Release v${NEW_VERSION} complete!

All 5 assets uploaded:
  - lu-x86_64-unknown-linux-gnu
  - lu-aarch64-unknown-linux-gnu
  - lu-x86_64-apple-darwin
  - lu-aarch64-apple-darwin
  - ludolph-mcp-v${NEW_VERSION}.tar.gz

Run /deploy to install on Pi and Mac.
```

If any job failed:
```
Release v${NEW_VERSION} created but some builds failed.

Check: https://github.com/evannagle/ludolph/actions/runs/${RUN_ID}

You may need to fix issues and create a new release.
```

## Troubleshooting

See RELEASE.md for CI requirements and known issues.
```

**Step 2: Verify file structure**

Run: `head -30 .claude/commands/release.md`
Expected: Shows new frontmatter and process overview

**Step 3: Commit**

```bash
git add .claude/commands/release.md
git commit -m "feat: enhance /release with version bump, release creation, and CI monitoring"
```

---

### Task 4: Update /deploy Command

**Files:**
- Modify: `.claude/commands/deploy.md`

**Step 1: Replace entire file with binary-download version**

```markdown
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
```

**Step 2: Verify file structure**

Run: `head -30 .claude/commands/deploy.md`
Expected: Shows new frontmatter and binary-download process

**Step 3: Commit**

```bash
git add .claude/commands/deploy.md
git commit -m "feat: enhance /deploy to download release binaries instead of building"
```

---

### Task 5: Final Verification

**Step 1: Check all files exist**

Run: `ls -la RELEASE.md .claude/commands/release.md .claude/commands/deploy.md`
Expected: All three files exist

**Step 2: Verify git history**

Run: `git log --oneline -5`
Expected: Shows 4 commits for this implementation

**Step 3: Push changes**

```bash
git push origin develop
```

**Step 4: Test /release command**

Invoke `/release` in Claude Code and verify:
- Pre-flight checks run
- Version bump prompt appears
- Process can be cancelled safely

---

## Summary

| Task | File | Change |
|------|------|--------|
| 1 | `RELEASE.md` | NEW: CI requirements documentation |
| 2 | `Cargo.toml` | Add comment explaining vendored OpenSSL |
| 3 | `.claude/commands/release.md` | Enhanced with automation and monitoring |
| 4 | `.claude/commands/deploy.md` | Download binaries instead of building |
| 5 | - | Verification and push |
