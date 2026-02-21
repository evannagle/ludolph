---
name: pr
description: Create a pull request following project conventions
---

# /pr - Create Pull Request

Open a PR targeting `develop` with a Conventional Commit title.

## PR Title Format

```
<type>: <short description>
```

**Types:**
- `feat` — New feature (bumps minor version)
- `fix` — Bug fix (bumps patch version)
- `docs` — Documentation only
- `refactor` — Code change, no behavior change
- `test` — Adding or updating tests
- `chore` — Maintenance, deps, CI
- `ci` — CI/CD changes

**Breaking changes:** Add `!` after type: `feat!: change config format`

## Process

1. Verify on a feature branch (not `develop` or `production`)
2. Run pre-push checks:
   ```bash
   cargo fmt --check
   cargo clippy -- -D warnings
   cargo test
   ```
3. If checks fail, fix issues first
4. Ask user to summarize the changes
5. Suggest a PR title following Conventional Commits
6. Confirm with user
7. Push branch and create PR:
   ```bash
   git push -u origin HEAD
   gh pr create --base develop --title "<title>" --body "<body>"
   ```
8. Return the PR URL

## PR Body Template

```markdown
## Summary

<One sentence describing what this PR does>

## Changes

- <Key change 1>
- <Key change 2>

## Testing

- [ ] `cargo test` passes
- [ ] Manual testing: <describe>

## Related

Closes #<issue> (if applicable)
```

## Rules

- PRs always target `develop`
- Title must be valid Conventional Commit format
- All checks must pass before creating PR
- Squash merge is used, so PR title becomes the commit message
