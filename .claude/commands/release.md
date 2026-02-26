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
