---
name: release
description: Validate ARM build on Pi and push to production for release
---

# /release - Release to Production

Validate the codebase, test ARM build on Pi, and push to production to trigger release-please.

## Prerequisites

- On `develop` branch
- Working directory clean
- Pi SSH configured (`lu pi` succeeds)

## Process

1. Verify prerequisites
2. Run local checks:
   - `cargo fmt --check`
   - `cargo clippy -- -D warnings`
   - `cargo test`
3. Test ARM build on Pi:
   - `ssh pi "cd ~/ludolph && git pull origin develop && cargo build --release"`
   - `ssh pi "~/ludolph/target/release/lu check"`
4. Confirm release
5. Push to production:
   - `git push origin develop:production`
6. Print next steps

## Rules

- Must be on `develop` branch
- Working directory must be clean
- Pi SSH must be configured (`lu pi` succeeds)
- ARM build is blocking — release aborts if Pi build fails
- Always confirm before pushing to production

## Steps

### Step 1: Verify prerequisites

Check branch:
```bash
git branch --show-current
```
Must be `develop`. If not, abort with: "Must be on develop branch to release."

Check working directory:
```bash
git status --porcelain
```
Must be empty. If not, abort with: "Working directory not clean. Commit or stash changes first."

### Step 2: Run local checks

Run each in sequence, stop on first failure:

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

### Step 3: Test ARM build on Pi

Use the SSH alias `pi` (configured via `lu setup`):

```bash
ssh pi "source ~/.cargo/env && cd ~/ludolph && git pull origin develop && cargo build --release"
```

Note: ARM release builds can take 10-20 minutes.

If build succeeds, verify binary:

```bash
ssh pi "source ~/.cargo/env && ~/ludolph/target/release/lu check"
```

If either fails, abort with error output.

### Step 4: Confirm release

Print summary:
```
Local checks: passed
ARM build on Pi: passed

Ready to push develop -> production?
```

Wait for user confirmation before proceeding.

### Step 5: Push to production

```bash
git push origin develop:production
```

### Step 6: Print next steps

Print:
```
Pushed to production. release-please will create a PR.

Next steps:
1. Wait for release-please PR at https://github.com/evannagle/ludolph/pulls
2. Review the changelog
3. Merge the PR to publish the release
```
