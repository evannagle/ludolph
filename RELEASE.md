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
