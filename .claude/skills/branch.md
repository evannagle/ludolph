---
name: branch
description: Create a new branch following project conventions
---

# /branch - Create Feature Branch

Create a properly named branch based off `develop`.

## Branch Naming Convention

```
<type>/<short-description>
```

**Types:**
- `feature/` — New functionality
- `fix/` — Bug fix
- `docs/` — Documentation only
- `refactor/` — Code restructuring
- `chore/` — Maintenance, deps, CI

**Examples:**
- `feature/conversation-memory`
- `fix/empty-vault-crash`
- `docs/install-guide`

## Process

1. Ask the user what they're working on
2. Suggest a branch name following the convention
3. Confirm with user
4. Run:
   ```bash
   git fetch origin
   git checkout develop
   git pull origin develop
   git checkout -b <branch-name>
   ```
5. Confirm branch created and ready

## Rules

- Always branch from `develop` (never from `production`)
- Use lowercase, hyphens for spaces
- Keep descriptions short (2-4 words)
- No issue numbers in branch name (reference in PR instead)
