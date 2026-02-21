# Ludolph v0.1 Design

> Date: 2026-02-21
> Status: Approved

## Overview

Ludolph is a sandboxed Telegram bot that gives Claude read-only access to an Obsidian vault. It runs on a Raspberry Pi or any always-on machine.

**Tagline:** "A real brain for your second brain. Talk to your vault, from anywhere, anytime."

## Goals

1. **One-line install** — `curl | bash` gets you running
2. **Read-only safety** — Claude can read the vault, nothing else
3. **Single binary** — No Python, no dependencies
4. **User's own API key** — No proxy service, user controls costs

## Non-Goals (v0.1)

- Write capabilities
- Plugin/extension architecture
- MCP support
- Conversation memory/history
- Multiple users

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    User's Phone                          │
│                    (Telegram)                            │
└─────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────┐
│                   Raspberry Pi                           │
│  ┌───────────────────────────────────────────────────┐  │
│  │                    Ludolph                         │  │
│  │  ┌─────────┐  ┌─────────┐  ┌─────────────────┐   │  │
│  │  │ Telegram│──│  Claude │──│     Tools       │   │  │
│  │  │   Bot   │  │  Client │  │ (read_file, etc)│   │  │
│  │  └─────────┘  └─────────┘  └────────┬────────┘   │  │
│  └─────────────────────────────────────┼────────────┘  │
│                                        │               │
│  ┌─────────────────────────────────────▼────────────┐  │
│  │              ~/ludolph/vault/                     │  │
│  │           (synced Obsidian vault)                 │  │
│  └───────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

## Directory Structure

```
~/ludolph/
├── config.toml      # Bot token, API key, settings
├── vault/           # User's Obsidian vault (synced in)
├── cache/           # Working state (future use)
└── logs/            # Log files
```

## Technical Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Language | Rust | Single binary, cross-compile for ARM, performance on Pi |
| Telegram | teloxide | Mature, async, well-documented |
| Claude API | reqwest + native tool_use | Simple, no MCP complexity |
| Config format | TOML | Rust convention for app config |
| Vault format | Obsidian conventions | YAML frontmatter, wikilinks preserved |

## Claude Tools (v0.1)

| Tool | Parameters | Description |
|------|------------|-------------|
| `read_file` | `path: String` | Read file contents from vault |
| `list_dir` | `path: String` | List files/folders in directory |
| `search` | `query: String, path?: String` | Search file contents (grep-like) |

All paths are relative to `~/ludolph/vault/`. Absolute paths or `..` traversal are rejected.

## Sandbox Enforcement

1. All file operations resolve paths relative to vault root
2. Paths containing `..` are rejected
3. Symlinks pointing outside vault are not followed
4. Only `read_file`, `list_dir`, `search` tools available — no write tools

## Install Flow

```bash
curl -sSL https://ludolph.dev/install | bash
```

1. Detect architecture (x86_64, aarch64)
2. Download appropriate binary to `~/.ludolph/bin/lu`
3. Add to PATH (append to .bashrc/.zshrc)
4. Create `~/ludolph/` directory structure
5. Interactive prompts:
   - Telegram bot token (link to BotFather instructions)
   - Claude API key (link to Anthropic console)
6. Write `~/ludolph/config.toml`
7. Install systemd user service (Linux) or launchd (macOS)
8. Print success message with next steps

## CLI Commands

| Command | Description |
|---------|-------------|
| `lu status` | Show service status |
| `lu logs` | Tail recent logs |
| `lu restart` | Restart service |
| `lu update` | Download latest binary, restart |
| `lu uninstall` | Stop service, remove files (prompts for confirmation) |
| `lu config` | Open config.toml in $EDITOR |

## Future Considerations

Deferred to later versions:

- **Write capabilities** — Modify vault files (append to daily note, etc.)
- **Extensibility** — Plugin architecture for custom tools
- **MCP support** — Act as MCP client for external tool servers
- **Memory** — Conversation history, observations
- **Multi-user** — Support multiple Telegram users with different vaults
