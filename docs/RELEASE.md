# Release Process

## Quick Reference

```sh
# Normal release (bump version, generate changelog, build)
node scripts/release.mjs 0.2.0

# Semantic version bump
node scripts/release.mjs patch       # 0.1.0 → 0.1.1
node scripts/release.mjs minor       # 0.1.0 → 0.2.0
node scripts/release.mjs major       # 0.1.0 → 1.0.0

# Dry run (preview without committing)
node scripts/release.mjs --dry-run 0.2.0

# Re-generate changelog from current content (re-release same version)
node scripts/release.mjs --regenerate 0.1.0
```

## Prerequisites

- Working directory must be clean (`git status` shows no changes)
- All verification passes (`npm run verify`)
- Git remote is configured for push

## Release Flow

### 1. Pre-flight

The release script checks:

- Working directory is clean
- No uncommitted changes

### 2. Verification

Runs the full verification suite:

```sh
npm run verify
# = format:check + check + build + test:rust + lint:rust
```

### 3. Version Bump

Updates version in three files atomically:

- `package.json`
- `src-tauri/tauri.conf.json`
- `src-tauri/Cargo.toml`

Can also be run standalone:

```sh
node scripts/version.mjs 0.2.0          # to specific version
node scripts/version.mjs patch          # semver bump
node scripts/version.mjs --current      # show current version
```

### 4. Changelog Generation

Generates `CHANGELOG.md` from gitmoji commit messages since the last git tag. Commits are grouped by type:

| Section          | Commit Type      |
| :--------------- | :--------------- |
| ✨ Features      | `feat`           |
| 🐛 Bug Fixes     | `fix`            |
| 🚀 Performance   | `perf`           |
| ♻️ Refactoring   | `refactor`       |
| 🎨 Styling       | `style`          |
| 📝 Documentation | `docs`           |
| ✅ Testing       | `test`           |
| 🔧 Chores        | `chore`, `build` |
| 🌐 i18n          | `i18n`           |

Standalone usage:

```sh
node scripts/changelog.mjs              # since last tag
node scripts/changelog.mjs --all        # full history
node scripts/changelog.mjs --from v0.1.0  # since specific tag
node scripts/changelog.mjs --preview    # preview only
```

### 5. Commit & Tag

Creates a release commit and annotated tag:

```sh
git commit -m "🔖 chore[release]: bump version to 0.2.0"
git tag -a v0.2.0 -m "Release v0.2.0"
```

### 6. Build

Runs Tauri production build for all platforms:

```sh
npm run tauri build
```

### 7. Publish

After successful build, push to remote:

```sh
git push origin core
git push origin v0.2.0
```

Pushing the tag triggers the CI/CD pipeline (`.github/workflows/release.yml`) which:

- Builds for Windows (x64), macOS (arm64), Linux (x64)
- Intel macOS (x86_64-apple-darwin) is excluded: ort-sys ships no prebuilt ONNX Runtime for it
- Creates a draft GitHub Release with artifacts attached

## Special: Regenerate Mode

`--regenerate` re-releases the specified version with updated content:

```sh
node scripts/release.mjs --regenerate 0.1.0
```

This is useful when:

- You've made changes and want to update the release artifacts
- The changelog needs to be regenerated from scratch
- You're iterating on a pre-release version

### No existing tag

If no tag for the version exists yet:

1. Keeps the current version (does not bump)
2. Re-generates `CHANGELOG.md` from the full commit history (`--all`)
3. Commits and tags normally

### Existing tag → auto drop from history

If a release commit and tag already exist for the version, the script automatically:

1. Detects the existing tag (e.g., `v1.0.0`)
2. Runs `git rebase --onto <parent> <tag-commit>` to **drop** the old release commit from history entirely
3. Deletes the local tag
4. Bumps to the target version, regenerates changelog from full history
5. Creates a fresh commit and tag

The old release commit is **removed from the Git DAG**, as if it never existed. All subsequent commits are rewired on top of the parent, preserving their timestamps and content. After pushing:

```sh
git push origin core --force-with-lease
git push origin v<version> --force
```

## Version Management

### Version Sources

| File                        | Key        | Format      |
| :-------------------------- | :--------- | :---------- |
| `package.json`              | `.version` | JSON string |
| `src-tauri/tauri.conf.json` | `.version` | JSON string |
| `src-tauri/Cargo.toml`      | `version`  | TOML string |

All three must stay in sync. The `scripts/version.mjs` script ensures atomic updates.

### Pre-release Versions

Semver pre-release tags are supported:

```sh
node scripts/release.mjs 0.2.0-beta.1
node scripts/release.mjs 0.2.0-rc.1
```

Pre-release tags are marked as draft releases in CI/CD.

## Rollback

If a release needs to be rolled back:

```sh
# Delete local tag
git tag -d v0.2.0

# Delete remote tag
git push --delete origin v0.2.0

# Revert to previous version
node scripts/version.mjs 0.1.0
git commit -m "⏪ revert[release]: rollback to v0.1.0"
```
