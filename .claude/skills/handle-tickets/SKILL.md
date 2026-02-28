---
name: handle-tickets
description: Workflow for picking up and implementing GitHub issues in this repo. Use when the user asks to work on an issue, pick up a ticket, implement a feature from GitHub, or start work on a bug/feature branch.
argument-hint: [issue-number]
allowed-tools: Bash(git checkout*), Bash(git add*), Bash(git commit*), Bash(git pull*), Bash(git log*), Bash(git status*), Bash(git diff*), Bash(gh issue*), Bash(gh pr create*), Bash(gh pr view*), Bash(gh pr checks*), Bash(cargo *), Bash(taplo *)
---

# Issue Implementation Workflow

## 1. Find & Plan

If an issue number was provided (`$ARGUMENTS`), read it immediately:
```bash
gh issue view $ARGUMENTS
```

Otherwise, browse open issues and pick a self-contained one:
```bash
gh issue list
```

- In the conversation, outline your approach: which files you'll touch, what the implementation plan is, and any risks or open questions.
- Browse crates.io for relevant crates where appropriate. If multiple crates could work, always ask which one to use.

## 2. Create Feature Branch

- Switch to the main branch (whose name is the unicode character U+202E, right-to-left override)
- `git pull` to make sure we're up to date
- `git checkout -b <issue-number>-<short-description>`

## 3. Implement

- Make changes following repo code style
- Add tests for new functionality
- Update example configs/docs as needed

## 4. Local Validation

Run checks; if formatting fails, auto-fix first then re-check:

```bash
cargo fmt --all                                           # auto-fix formatting
taplo format                                              # auto-fix TOML formatting
cargo fmt --all -- --check                                # verify formatting
cargo clippy --all-targets --all-features -- -D warnings  # lints (fix, don't allow)
cargo test --workspace                                    # tests
taplo format --check                                      # verify TOML
```

## 5. Commit & Push

```bash
git add <files>   # NEVER use `git add .`
git commit -m "🐛/⭐/📎 one-line description"
git push -u origin <branch-name>  # requires human approval
```

### Commit Emoji Conventions

| Emoji | Use               |
| ----- | ----------------- |
| ⭐     | Features          |
| 🐛     | Bug fixes         |
| 📎     | Clippy fixes      |
| 📖     | Documentation     |
| 🧹     | Clean-up/chore    |
| 🔒     | Update lockfile   |
| 🎨     | Visual work (FE)  |
| 🤖     | Agent set-up work |

## 6. Create PR

```bash
gh pr create --title "⭐ one-line description" --body "Closes #<issue>" --base <main-branch>
```

Or use the GitHub web UI if branch names are funky.

## 7. Monitor CI

- Check the PR checks page for CI status
- Fix any failures (fmt, clippy, tests, taplo, etc.)
- Amend and force-push fixes:

```bash
git add <fixed-files>
git commit --amend --no-edit
git push --force-with-lease
```

## 8. Review & Merge

- Wait for CI green ✅
- If CI fails, iterate and amend the original PR
- **Merging requires human approval** — do not run `gh pr merge` or equivalent automatically
