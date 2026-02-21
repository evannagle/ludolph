# Ludolph CLI Style Guide

> **Status:** Implemented
> **Crates:** `console`, `dialoguer`, `indicatif`

## Design Principles

- **Minimalist** — No clutter, every element earns its place
- **Consistent** — Same patterns throughout
- **Informative** — User always knows what's happening
- **Branded** — Pi (π) theming where appropriate

---

## Colors

| Element | Color | Usage |
|---------|-------|-------|
| Success | Green | `[•ok]`, completed steps |
| Error | Red | `[•!!]`, failures |
| Pending | Dim/Gray | `[•--]`, not yet checked |
| Headers | Bold | Step titles |
| Values | Cyan | User data, paths, URLs |
| Prompts | Default | Input prompts |

---

## Step Headers

Bold header with inline pi-spinner, consistent newlines:

```
                                        ← blank line before
Checking system [31415]                 ← bold header + spinner
                                        ← blank line after header
  [•ok] Network connected
  [•ok] 8GB free space

Checking system ✓                       ← spinner replaced with ✓
                                        ← blank line after section
```

---

## Status Indicators

No space after dot. Colored by status:

```
  [•ok] Network connected               ← green
  [•!!] Token missing                   ← red
  [•--] Vault sync                      ← dim (not yet checked)
```

Alignment: 2-space indent, consistent column for descriptions.

---

## Pi Spinner

Sliding window through pi digits (3.14159265358979...).
Starts with zeros, pi digits shift in from right.
**Speed: 200ms per frame.**

```
[00000]
[00003]
[00031]
[00314]
[03141]
[31415]
[14159]
[41592]
[15926]
[59265]
...repeats from [31415]
```

Implementation: Store first ~50 digits of pi, cycle through with 5-char window.

---

## Data Tables

Minimal lines, colored status values:

```
Service         Status    Uptime
───────────────────────────────────
Telegram Bot    running   2d 4h       ← "running" green
Vault Sync      idle      -           ← "idle" dim
```

- Header row: bold or default
- Separator: single thin line (─)
- Columns: left-aligned, consistent spacing
- No outer borders

---

## Prompts

π symbol prefix, description on separate line if needed:

```
π Telegram bot token: _
```

With help text:

```
π Telegram bot token
  Get one from @BotFather on Telegram
  : _
```

---

## Error Messages

Red prefix, clear message, optional help:

```
[•!!] Could not connect to Telegram API

  Check your bot token and try again.
  Docs: https://ludolph.dev/setup/telegram
```

---

## Success/Completion

```
                                        ← blank line
Setup complete ✓                        ← bold, green ✓

  Ludolph is running. Message your bot on Telegram!

  Commands:
    lu status     Check service status
    lu logs       View recent logs
    lu config     Edit configuration
```

---

## Wizard Flow Example

```

Welcome to Ludolph                      ← bold

A real brain for your second brain.
Talk to your vault, from anywhere, anytime.


Checking system [31415]

  [•ok] Raspberry Pi 4B detected
  [•ok] Network connected
  [•ok] 12GB free space

Checking system ✓


π Telegram bot token
  Create one at @BotFather on Telegram
  : 1234567890:ABCdef...

π Claude API key
  Get one at console.anthropic.com
  : sk-ant-...


Configuring Ludolph [14159]

  [•ok] Config written
  [•ok] Service installed

Configuring Ludolph ✓


Setup complete ✓

  Next: Sync your vault to ~/ludolph/vault/
  Then message your Telegram bot!

```

---

## Usage

### Spinner

```rust
use crate::ui::PiSpinner;

let spinner = PiSpinner::new("Checking system");
// ... do work ...
spinner.finish(); // Shows "Checking system ✓"
```

### Status Lines

```rust
use crate::ui::StatusLine;

StatusLine::ok("Network connected").print();
StatusLine::error("Token missing").print();
StatusLine::pending("Vault sync").print();
```

### Prompts

```rust
use crate::ui::{prompt, prompt_with_help};

let token = prompt("Telegram bot token")?;

let api_key = prompt_with_help(
    "Claude API key",
    "Get one at console.anthropic.com"
)?;
```

### Tables

```rust
use crate::ui::Table;

let mut table = Table::new(&["Service", "Status", "Uptime"]);
table.add_row(&["Telegram Bot", "running", "2d 4h"]);
table.add_row(&["Vault Sync", "idle", "-"]);
table.print();
```
