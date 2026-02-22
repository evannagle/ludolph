# Release Skill Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add `/release` skill and `lu check` command to validate ARM builds on Pi before pushing to production.

**Architecture:** Add `Check` command to CLI that prints version and validates config/vault. Create Claude Code skill that runs local checks, SSH to Pi for ARM build test, then pushes to production.

**Tech Stack:** Rust (clap, console), Claude Code skills (markdown)

---

## Task 1: Add Skip status variant

**Files:**
- Modify: `src/ui/status.rs:9-14`
- Test: `src/ui/status.rs` (existing test module)

**Step 1: Add Skip variant to Status enum**

In `src/ui/status.rs`, add the `Skip` variant:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Success - green `[•ok]`
    Ok,
    /// Error - red `[•!!]`
    Error,
    /// Skipped - dim `[•--]`
    Skip,
}
```

**Step 2: Add render for Skip**

Update the `render` method:

```rust
impl Status {
    #[must_use]
    pub fn render(self) -> String {
        match self {
            Self::Ok => format!("[{}]", style("•ok").green()),
            Self::Error => format!("[{}]", style("•!!").red()),
            Self::Skip => format!("[{}]", style("•--").dim()),
        }
    }
}
```

**Step 3: Add StatusLine::skip constructor**

After `StatusLine::error`:

```rust
/// Create a skipped status line.
#[must_use]
pub fn skip(message: impl Into<String>) -> Self {
    Self::new(Status::Skip, message)
}
```

**Step 4: Run tests**

Run: `cargo test status`
Expected: All tests pass

**Step 5: Commit**

```bash
git add src/ui/status.rs
git commit -m "feat: add Skip status variant for lu check"
```

---

## Task 2: Add Check command to CLI

**Files:**
- Modify: `src/cli/mod.rs:21-31`
- Modify: `src/cli/commands.rs`
- Modify: `src/main.rs:22-31`

**Step 1: Add Check variant to Command enum**

In `src/cli/mod.rs`:

```rust
#[derive(Subcommand)]
pub enum Command {
    /// Health check
    Check,
    /// Open config in editor
    Config,
    /// Initial setup wizard (or run specific step)
    Setup {
        #[command(subcommand)]
        step: Option<SetupStep>,
    },
    /// Check Pi connectivity
    Pi,
}
```

**Step 2: Export check function**

Update the pub use line in `src/cli/mod.rs`:

```rust
pub use commands::{check, config_cmd, pi};
```

**Step 3: Implement check command**

Add to `src/cli/commands.rs`:

```rust
use crate::ui::StatusLine;
use std::process::ExitCode;
use walkdir::WalkDir;

/// Run health checks and return appropriate exit code.
pub fn check() -> ExitCode {
    // Print version
    println!();
    println!("lu {}", env!("CARGO_PKG_VERSION"));
    println!();

    // CLI check (always passes if we got here)
    StatusLine::ok("CLI").print();

    // Config check
    let config = match Config::load() {
        Ok(cfg) => {
            StatusLine::ok("Config loaded").print();
            Some(cfg)
        }
        Err(_) => {
            StatusLine::skip("Config (not found)").print();
            None
        }
    };

    // Vault check
    match config.as_ref().map(|c| &c.vault.path) {
        Some(path) if path.exists() => {
            let count = WalkDir::new(path)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_file())
                .count();
            StatusLine::ok(format!("Vault accessible ({count} files)")).print();
        }
        Some(path) => {
            StatusLine::error(format!("Vault not found: {}", path.display())).print();
            println!();
            return ExitCode::FAILURE;
        }
        None => {
            StatusLine::skip("Vault (not configured)").print();
        }
    }

    println!();
    ExitCode::SUCCESS
}
```

**Step 4: Wire up in main.rs**

Update the match in `src/main.rs`:

```rust
match cli.command {
    Some(Command::Check) => return Ok(cli::check().into()),
    Some(Command::Config) => cli::config_cmd()?,
    Some(Command::Setup { step }) => match step {
        Some(cli::SetupStep::Credentials) => cli::setup_credentials().await?,
        Some(cli::SetupStep::Pi) => cli::setup_pi()?,
        None => cli::setup().await?,
    },
    Some(Command::Pi) => cli::pi()?,
    None => bot::run().await?,
}
```

Wait - `ExitCode` doesn't convert to `Result<()>`. Let me fix that.

**Step 4 (revised): Wire up in main.rs**

Change main to use `Termination`:

```rust
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    // Only enable tracing if RUST_LOG is set
    if std::env::var("RUST_LOG").is_ok() {
        tracing_subscriber::fmt::init();
    }

    let cli = Cli::parse();

    let result = run(cli).await;

    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Some(Command::Check) => Ok(cli::check()),
        Some(Command::Config) => {
            cli::config_cmd()?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Command::Setup { step }) => {
            match step {
                Some(cli::SetupStep::Credentials) => cli::setup_credentials().await?,
                Some(cli::SetupStep::Pi) => cli::setup_pi()?,
                None => cli::setup().await?,
            }
            Ok(ExitCode::SUCCESS)
        }
        Some(Command::Pi) => {
            cli::pi()?;
            Ok(ExitCode::SUCCESS)
        }
        None => {
            bot::run().await?;
            Ok(ExitCode::SUCCESS)
        }
    }
}
```

**Step 5: Run checks**

Run: `cargo clippy -- -D warnings && cargo test`
Expected: All pass

**Step 6: Manual test**

Run: `cargo run -- check`
Expected output:
```
lu 0.2.1
[•ok] CLI
[•ok] Config loaded
[•ok] Vault accessible (N files)
```

**Step 7: Commit**

```bash
git add src/cli/mod.rs src/cli/commands.rs src/main.rs
git commit -m "feat: add lu check command"
```

---

## Task 3: Create /release skill

**Files:**
- Create: `.claude/skills/release.md`

**Step 1: Create the skill file**

```markdown
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
4. Push to production:
   - `git push origin develop:production`
5. Print next steps

## Failure Behavior

- Any step fails → stop immediately, show error
- Pi build is **blocking** — release aborts if ARM build fails

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

Get Pi config from `lu pi` or config file. Run:

```bash
ssh <user>@<host> "cd ~/ludolph && git pull origin develop && cargo build --release"
```

If build succeeds, verify binary:

```bash
ssh <user>@<host> "~/ludolph/target/release/lu check"
```

If either fails, abort with error output.

### Step 4: Push to production

```bash
git push origin develop:production
```

### Step 5: Print next steps

Print:
```
Pushed to production. release-please will create a PR.

Next steps:
1. Wait for release-please PR at https://github.com/evannagle/ludolph/pulls
2. Review the changelog
3. Merge the PR to publish the release
```
```

**Step 2: Commit**

```bash
git add .claude/skills/release.md
git commit -m "feat: add /release skill"
```

---

## Task 4: Final verification

**Step 1: Run full test suite**

```bash
cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

**Step 2: Test lu check locally**

```bash
cargo run -- check
echo "Exit code: $?"
```

Expected: Exit code 0

**Step 3: Push to develop**

```bash
git push origin develop
```

---

## Summary

| Task | Description |
|------|-------------|
| 1 | Add Skip status variant to ui/status.rs |
| 2 | Add `lu check` command |
| 3 | Create `/release` skill |
| 4 | Final verification and push |
