# Release Skill Design

## Overview

Add a `/release` skill for Claude Code and a `lu check` command to validate ARM builds on Raspberry Pi before pushing to production.

## Components

### 1. `lu check` command

A health check command that verifies the binary and configuration.

**Output (configured system):**
```
lu 0.2.1
[•ok] CLI
[•ok] Config loaded
[•ok] Vault accessible (1,234 files)
```

**Output (unconfigured system):**
```
lu 0.2.1
[•ok] CLI
[•--] Config (not found)
[•--] Vault (not configured)
```

**Exit codes:**
- `0` — All checks passed (skipped checks don't count as failures)
- `1` — A check failed (config invalid, vault inaccessible, etc.)

**Implementation:** Add `Check` variant to `Command` enum in `src/cli/mod.rs`, implement in `src/cli/commands.rs`.

### 2. `/release` skill

A Claude Code skill that orchestrates the release process.

**Flow:**
1. `cargo fmt --check`
2. `cargo clippy -- -D warnings`
3. `cargo test`
4. `ssh pi "cd ~/ludolph && git pull origin develop && cargo build --release"`
5. `ssh pi "~/ludolph/target/release/lu check"`
6. `git push origin develop:production`
7. Print instructions to merge release-please PR

**Prerequisites:**
- Must be on `develop` branch
- Working directory clean
- Pi SSH configured via `lu setup`

**Failure behavior:**
- Any step fails → stop immediately, show error
- Pi build is blocking — release aborts if ARM build fails

**Implementation:** Create `.claude/skills/release.md`.

## Success Criteria

- `lu check` exits 0 on healthy system, 1 on failure
- `/release` validates ARM build before pushing to production
- Release aborts early if any check fails
