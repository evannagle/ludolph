---
name: commit
description: Create a commit following Conventional Commits format
---

# /commit - Create Commit

Create a commit with a properly formatted Conventional Commit message.

## Commit Format

```
<type>: <short description>

[optional body]
```

**Types:**

| Type | When to use | Version bump |
|------|-------------|--------------|
| `feat` | New feature | Minor |
| `fix` | Bug fix | Patch |
| `docs` | Documentation only | None |
| `refactor` | Code restructuring | None |
| `test` | Adding or updating tests | None |
| `chore` | Maintenance, deps, CI | None |
| `ci` | CI/CD changes | None |

**Breaking changes:** Add `!` after type: `feat!: change config format`

## Process

1. Run `git status` to see staged and unstaged changes
2. Run `git diff --staged` to review what will be committed
3. If nothing staged, ask user what to stage or suggest `git add` commands
4. Analyze the changes and determine the commit type
5. Draft a concise commit message (single line preferred)
6. Present the message and ask for confirmation
7. Run the commit:
   ```bash
   git commit -m "<message>"
   ```
8. Confirm success with `git log -1 --oneline`

## Rules

- **Concise messages** — Single line, under 72 characters when possible
- **Imperative mood** — "add feature" not "added feature"
- **No period** — Don't end the subject line with a period
- **No Claude bylines** — Don't add Co-Authored-By
- **Lowercase type** — Use `feat:` not `Feat:`
- **Body is optional** — Only add if genuinely needed for context

## Examples

```
feat: add conversation memory
fix: handle empty vault directory
docs: update install instructions
refactor: simplify tool execution
chore: update dependencies
feat!: require config.toml instead of env vars
```

## Multi-file Changes

When changes span multiple concerns, prefer one commit per logical change. If they're all part of one feature, summarize at the feature level:

```
# Good - single feature
feat: add CLI style guide with Pi-themed UI

# Avoid - too granular
feat: add spinner component
feat: add status component
feat: add prompt component
```
