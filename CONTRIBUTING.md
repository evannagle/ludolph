# Contributing to Ludolph

Thanks for your interest in contributing!

## Getting Started

```bash
git clone https://github.com/evannagle/ludolph
cd ludolph
cargo build
```

## Branch Strategy

- `production` — Stable releases
- `develop` — Integration branch (PRs target here)

### Branch Naming

```
<type>/<short-description>
```

| Prefix | Use for | Merges to |
|--------|---------|-----------|
| `feature/` | New functionality | `develop` |
| `fix/` | Bug fixes | `develop` |
| `hotfix/` | Critical production fix | `production` |
| `docs/` | Documentation | `develop` |
| `refactor/` | Code restructuring | `develop` |
| `chore/` | Maintenance, deps | `develop` |

**Examples:**
- `feature/conversation-memory`
- `fix/empty-vault-crash`
- `hotfix/auth-bypass`
- `docs/install-guide`

### Hotfix Flow

For critical bugs in production:

1. Branch from `production`: `git checkout -b hotfix/critical-bug production`
2. Fix and open PR targeting `production`
3. After merge, also merge `production` into `develop`

## Making Changes

1. Fork the repo
2. Create a branch from `develop`: `git checkout -b feature/my-feature develop`
3. Make your changes
4. Run checks: `cargo fmt && cargo clippy && cargo test`
5. Commit using [Conventional Commits](#commit-messages)
6. Open a PR targeting `develop`

## Commit Messages

We use [Conventional Commits](https://www.conventionalcommits.org/) to automate versioning and changelogs.

### Format

```
<type>: <description>

[optional body]
```

### Types

| Type | When to use | Version bump |
|------|-------------|--------------|
| `feat` | New feature | Minor |
| `fix` | Bug fix | Patch |
| `docs` | Documentation only | None |
| `refactor` | Code change that neither fixes nor adds | None |
| `test` | Adding or updating tests | None |
| `chore` | Maintenance, dependencies | None |
| `ci` | CI/CD changes | None |

### Breaking Changes

Add `!` after the type for breaking changes:

```
feat!: change config format
```

This triggers a major version bump.

### Examples

```
feat: add conversation memory
fix: handle empty vault directory
docs: update install instructions
refactor: simplify tool execution
feat!: require config.toml instead of env vars
```

## Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy` and fix warnings
- No `unsafe` code (enforced by lints)

## Testing

```bash
cargo test
```

## Questions?

Open an issue or start a discussion.
