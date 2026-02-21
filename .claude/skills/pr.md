---
name: pr
description: Create a pull request following project conventions
---

# /pr - Create Pull Request

Open a PR with a Conventional Commit title, targeting the correct branch.

## Merge Targets

| Branch prefix | Target | Use case |
|---------------|--------|----------|
| `feature/*` | `develop` | New functionality |
| `fix/*` | `develop` | Bug found during development |
| `hotfix/*` | `production` | Critical fix for live |
| `docs/*` | `develop` | Documentation |
| `refactor/*` | `develop` | Code restructuring |
| `chore/*` | `develop` | Maintenance |

**Hotfix note:** After merging to `production`, the hotfix should also be merged or cherry-picked to `develop`.

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

1. Detect current branch name
2. Determine target branch from prefix:
   - `hotfix/*` → `production`
   - Everything else → `develop`
3. Verify not on `develop` or `production`
4. Run pre-push checks:
   ```bash
   cargo fmt --check
   cargo clippy -- -D warnings
   cargo test
   ```
5. If checks fail, fix issues first
6. Ask user to summarize the changes
7. Suggest a PR title following Conventional Commits
8. Confirm with user
9. Push branch and create PR:
   ```bash
   git push -u origin HEAD
   gh pr create --base <target> --title "<title>" --body "<body>"
   ```
10. Return the PR URL
11. If hotfix, remind user to also merge to `develop` after

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

- Hotfixes target `production`, everything else targets `develop`
- Title must be valid Conventional Commit format
- All checks must pass before creating PR
- Squash merge is used, so PR title becomes the commit message
