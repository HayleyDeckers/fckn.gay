# Issue Implementation Workflow

A repeatable process for picking up and implementing GitHub issues.

## 1. Find & Plan

- Browse open issues with `gh issue list` or via browser
- Pick a self-contained issue that fits your time/skill
- Create a plan document outlining approach, key files, and implementation details
- consider browsing crates.io for relevant crates where appropriate. If you find multiple crates which could be appropriate, always ask which one to use.

## 2. Create Feature Branch

- Switch to the main branch (whose name is the unicode character U+202E, right-to-left overide)
- git pull to make sure we're up to date
- make a new feature branch: `git checkout -b <issue-number>-<short-description>`


## 3. Implement

- Make changes following repo code style
- Add tests for new functionality
- Update example configs/docs as needed

## 4. Local Validation

```bash
cargo fmt --all -- --check                                # Rust formatting
cargo clippy --all-targets --all-features -- -D warnings  # lints
cargo test --workspace                                    # tests
taplo format --check                                      # TOML formatting
```

## 5. Commit & Push

```bash
git add <files>               # NEVER use `git add .`
git commit -m "🐛/⭐/📎 scope: description"
git push -u origin <branch-name>
```

### Commit Emoji Conventions

| Emoji | Use           |
| ----- | ------------- |
| ⭐     | Features      |
| 🐛     | Bug fixes     |
| 📎     | Clippy fixes  |
| 📖     | Documentation |

## 6. Create PR

```bash
gh pr create --title "⭐ scope: description" --body "Closes #<issue>" --base <main-branch>
```

Or use GitHub web UI if branch names are funky.

## 7. Monitor CI

- Check PR checks page for CI status
- Fix any failures (fmt, clippy, tests, taplo, etc.)
- Amend commit and force push fixes:

```bash
git add <fixed-files>
git commit --amend --no-edit
git push --force-with-lease
```

## 8. Review & Merge

- Wait for CI green ✅
- If CI fails, iterate and ammend the original MR





