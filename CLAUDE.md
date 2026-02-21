# Ludolph

A real brain for your second brain. Talk to your vault, from anywhere, anytime.

## Overview

Ludolph is a Telegram bot that gives Claude sandboxed read-only access to an Obsidian vault. It runs on a Raspberry Pi or any Unix system, providing always-available AI assistance for your personal knowledge base.

## Project Structure

```
ludolph/
├── src/
│   ├── main.rs           # Entry point, CLI dispatch
│   ├── cli.rs            # CLI commands (clap)
│   ├── bot.rs            # Telegram bot (teloxide)
│   ├── claude.rs         # Claude API client with tool loop
│   ├── config.rs         # Configuration loading (TOML)
│   ├── tools/            # Claude tool implementations
│   │   ├── mod.rs        # Tool registry, sandbox enforcement
│   │   ├── read_file.rs  # Read file contents
│   │   ├── list_dir.rs   # List directory contents
│   │   └── search.rs     # Search vault contents
│   └── ui/               # CLI styling
│       ├── mod.rs        # Re-exports
│       ├── spinner.rs    # Pi spinner [31415]
│       ├── status.rs     # Status indicators [•ok]
│       ├── prompt.rs     # Input prompts with π
│       └── table.rs      # Minimal line tables
├── tests/                # Integration tests
├── docs/                 # GitHub Pages site
├── Cargo.toml            # Dependencies and lints
├── CLAUDE.md             # This file
├── STYLE.md              # CLI output formatting guide
├── CONTRIBUTING.md       # Git workflow, commit format
└── README.md             # User-facing documentation
```

## Coding Standards

### Language & Tooling

- **Edition:** Rust 2024
- **Formatter:** `cargo fmt` (rustfmt defaults)
- **Linter:** `cargo clippy` with pedantic + nursery warnings as errors
- **Safety:** `unsafe_code = "forbid"` — no exceptions

### Naming Conventions

| Element | Convention | Example |
|---------|------------|---------|
| Functions | snake_case | `get_vault_path()` |
| Types | PascalCase | `ChatResponse` |
| Constants | SCREAMING_SNAKE_CASE | `PI_DIGITS` |
| Modules | snake_case | `read_file.rs` |

### Code Style

- **Line length:** 100 characters max
- **Indentation:** 4 spaces (rustfmt default)
- **Imports:** Group by std, external crates, internal modules
- **Comments:** Use `///` doc comments for public items

### Error Handling

- **Never use `.unwrap()` in production code** — use `?` or proper error handling
- Use `anyhow::Result` for application errors
- Use `thiserror` for library error types
- Log errors with `tracing::error!` before propagating

```rust
// Good
let config = load_config().map_err(|e| {
    tracing::error!("Failed to load config: {}", e);
    e
})?;

// Bad
let config = load_config().unwrap();
```

### Borrowing & Ownership

- Prefer borrowing over ownership when possible
- Use `&str` in function parameters, not `String`
- Clone only when necessary, document why

### Async Code

- Use `tokio` for async runtime
- Keep async functions small and focused
- Avoid blocking in async contexts

## Coding Paradigms

### Structural Patterns

- **Composition over complexity** — Prefer simple structs with methods over complex trait hierarchies
- **Single responsibility** — Each module does one thing well
- **Flat structure** — Avoid deep nesting; prefer `src/tools/search.rs` over `src/tools/search/impl/core.rs`

### Rust Idioms

```rust
// Prefer iterators over manual loops
// Good
let names: Vec<_> = users.iter().map(|u| &u.name).collect();

// Avoid
let mut names = Vec::new();
for user in &users {
    names.push(&user.name);
}

// Use match for exhaustive handling
match result {
    Ok(value) => process(value),
    Err(e) => handle_error(e),
}

// Use if let for single-variant checks
if let Some(config) = optional_config {
    apply(config);
}

// Prefer combinators when clean
let name = user.and_then(|u| u.profile).map(|p| p.name);
```

### State & Mutability

- **Immutable by default** — Only use `mut` when necessary
- **Minimize mutable scope** — Declare mutable variables close to where they're mutated
- **No global mutable state** — Pass state explicitly through function parameters
- **Prefer transformations** — Return new values rather than mutating in place

### API Design

```rust
// Accept flexible input types
fn read_file(path: impl AsRef<Path>) -> Result<String>

// Return owned types from constructors
fn new() -> Self  // not fn new() -> &Self

// Use builders for complex configuration
let client = ClientBuilder::new()
    .timeout(Duration::from_secs(30))
    .retry(3)
    .build()?;
```

### YAGNI (You Ain't Gonna Need It)

**Only build what's needed now.**

- Don't add features "for later"
- Don't create abstractions for hypothetical use cases
- Don't add configuration options nobody asked for
- Delete dead code immediately

```rust
// Bad: "We might need other formats later"
enum OutputFormat { Json, Xml, Yaml, Toml, Csv }

// Good: Build what you need now
fn to_json(&self) -> String

// Bad: Premature flexibility
fn process<T: Processor + Send + Sync + 'static>(p: T)

// Good: Concrete until proven otherwise
fn process(p: &SimpleProcessor)
```

**When tempted to add flexibility, ask:** Do we have a concrete use case today?

### Spec-First Development

**Write specs before code.**

For any non-trivial feature:

1. **Define the interface** — What functions/methods exist? What do they accept and return?
2. **Document edge cases** — Empty input, invalid input, boundary conditions
3. **Write examples** — Show expected input → output
4. **Then implement** — Code to match the spec

```rust
/// Resolves a path safely within the vault sandbox.
///
/// # Arguments
/// * `vault_path` - The root vault directory
/// * `relative_path` - User-provided path to resolve
///
/// # Returns
/// * `Some(path)` - Resolved canonical path within vault
/// * `None` - Path escapes vault or is invalid
///
/// # Edge Cases
/// * Paths with `..` are always rejected
/// * Symlinks pointing outside vault are rejected
/// * Non-existent files return None unless parent exists in vault
///
/// # Examples
/// ```
/// let vault = Path::new("/home/user/vault");
/// assert_eq!(safe_resolve(vault, "notes/todo.md"), Some(...));
/// assert_eq!(safe_resolve(vault, "../etc/passwd"), None);
/// assert_eq!(safe_resolve(vault, "notes/../../../etc/passwd"), None);
/// ```
pub fn safe_resolve(vault_path: &Path, relative_path: &str) -> Option<PathBuf>
```

### What to Avoid

- **Premature abstraction** — Don't create traits until you have 2+ implementations
- **Over-engineering** — Simple functions beat complex frameworks
- **Stringly-typed code** — Use enums and newtypes, not raw strings
- **Magic numbers** — Use named constants

## Security

### Sandbox Enforcement

The `safe_resolve()` function in `src/tools/mod.rs` is the security boundary. **All file operations must go through it.**

```rust
// Always resolve paths through the sandbox
let path = safe_resolve(&vault_path, user_input)?
    .ok_or_else(|| anyhow!("Path outside vault"))?;
```

**Security rules:**
- Reject any path containing `..`
- Canonicalize paths and verify they start with vault path
- Never trust user input for file operations

### API Keys

- Never log API keys
- Load from environment or config file, never hardcode
- Use `secrecy` crate if handling sensitive data in memory

## Testing

### Test-Driven Development

**Write tests first, then implementation.**

1. Write a failing test that defines expected behavior
2. Run it — confirm it fails
3. Write minimal code to make it pass
4. Refactor while keeping tests green
5. Commit

### Test Structure

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_resolve_accepts_valid_paths() {
        // Arrange
        let vault = tempdir().unwrap();
        let file = vault.path().join("notes/todo.md");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "content").unwrap();

        // Act
        let result = safe_resolve(vault.path(), "notes/todo.md");

        // Assert
        assert!(result.is_some());
        assert_eq!(result.unwrap(), file.canonicalize().unwrap());
    }

    #[test]
    fn safe_resolve_rejects_path_traversal() {
        let vault = tempdir().unwrap();

        // All of these must be rejected
        assert!(safe_resolve(vault.path(), "../etc/passwd").is_none());
        assert!(safe_resolve(vault.path(), "notes/../../../etc/passwd").is_none());
        assert!(safe_resolve(vault.path(), "..").is_none());
    }
}
```

### Test Naming

Use descriptive names that explain behavior:

```rust
// Good: Describes what and when
fn safe_resolve_rejects_path_traversal()
fn chat_returns_error_when_api_key_missing()
fn spinner_cycles_through_pi_digits()

// Bad: Vague
fn test_safe_resolve()
fn test_error()
fn test_spinner()
```

### What to Test

| Category | Examples |
|----------|----------|
| Happy path | Valid input produces expected output |
| Edge cases | Empty string, very long input, unicode, special chars |
| Boundaries | First/last item, zero, max values |
| Errors | Invalid input, missing files, network failures |
| Security | Path traversal, injection attempts, sandbox escapes |

### Test Commands

```bash
cargo test                    # Run all tests
cargo test --no-fail-fast     # Run all even if some fail
cargo test -- --nocapture     # Show println! output
cargo test safe_resolve       # Run tests matching name
```

### Test Discipline

- **Never skip failing tests** — Fix them or delete them
- **No `#[ignore]` without a tracking issue**
- **Tests must be deterministic** — No flaky tests
- **Fast tests** — Unit tests should run in milliseconds

## Dependencies

### Preferred Crates

| Purpose | Crate | Notes |
|---------|-------|-------|
| CLI parsing | clap | With derive feature |
| Telegram | teloxide | With macros feature |
| HTTP client | reqwest | With json feature |
| Async runtime | tokio | With rt-multi-thread, macros |
| Serialization | serde + serde_json | |
| Config | toml | |
| Errors | anyhow | For applications |
| Logging | tracing | With tracing-subscriber |
| CLI UI | console, dialoguer, indicatif | See STYLE.md |
| File walking | walkdir, ignore | For search |
| Regex | regex | For search patterns |

### Adding Dependencies

- Check crate quality: downloads, maintenance, security advisories
- Prefer well-maintained crates with good documentation
- Run `cargo audit` after adding new dependencies

## Git Workflow

See `CONTRIBUTING.md` for full details.

### Branch Naming

```
feature/conversation-memory
fix/empty-vault-crash
docs/install-guide
```

### Commit Format

```
feat: add conversation memory
fix: handle empty vault directory
docs: update install instructions
```

### Pre-Push Checks

cargo-husky runs automatically:
1. `cargo fmt --check`
2. `cargo clippy -- -D warnings`
3. `cargo test`

**All checks must pass before push.**

## CLI Output

See `STYLE.md` for complete formatting guidelines.

Quick reference:
- Spinner: `[31415]` sliding through pi digits at 200ms
- Success: `[•ok]` in green
- Error: `[•!!]` in red
- Pending: `[•--]` in dim
- Prompts: `π` prefix

## Common Tasks

### Adding a New Tool

1. Create `src/tools/new_tool.rs`
2. Implement `definition()` returning `Tool` struct
3. Implement `execute()` with sandbox checks
4. Register in `src/tools/mod.rs`
5. Add tests

### Adding a CLI Command

1. Add variant to `Command` enum in `src/cli.rs`
2. Add doc comment for help text
3. Implement handler function
4. Wire up in `main.rs` match
5. Update README

## Resources

- [Rust Book](https://doc.rust-lang.org/book/)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [teloxide Documentation](https://docs.rs/teloxide)
- [Claude API Reference](https://docs.anthropic.com/claude/reference)
