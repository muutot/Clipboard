---
name: version-release
description: Automated version bumping and release workflow for Clipboard Desktop. Use when the user asks to bump the version, release a new version, or regenerate a release. Supports semantic version bumping (patch/minor/major), specific version targets, and regenerate mode for re-releasing current content.
---

# Version Release

## Trigger patterns

Start this skill when the user says any of:

- "升级版本到 x.x.x" / "bump version to x.x.x"
- "发布版本 x.x.x" / "release version x.x.x"
- "重新发布版本 x.x.x" / "regenerate release x.x.x"
- "升级 patch/minor/major 版本"
- "release" combined with a version number or bump type

## Prerequisites

Before running any release command:

1. Check `git status --short --branch` — working directory must be clean.
2. Run `npm run verify` — all checks must pass.
3. If verification fails, report the failures and stop.

## Release workflow

### Normal release (bump to new version)

```
node scripts/release.mjs <version>
```

This runs the full pipeline:

1. Pre-flight checks (clean working directory)
2. `npm run verify`
3. Version bump in `package.json`, `tauri.conf.json`, `Cargo.toml`
4. Changelog generation from commits since last tag
5. `${version}` / `${date}` placeholder substitution in `RELEASE.md`
6. Git commit (version files + CHANGELOG.md + RELEASE.md) + annotated tag
7. Tauri production build

### Semantic bump

```
node scripts/release.mjs patch    # 0.1.0 → 0.1.1
node scripts/release.mjs minor    # 0.1.0 → 0.2.0
node scripts/release.mjs major    # 0.1.0 → 1.0.0
```

### Regenerate mode (re-release current version)

```
node scripts/release.mjs --regenerate <version>
```

This mode:

**No existing tag:** Keeps current version, regenerates changelog, commits and tags.

**Existing tag detected:** Uses `git rebase --onto` to **drop** the old release commit from history entirely, delete the old tag, bump to target version, regenerate changelog, and create a fresh commit + tag.

Useful when re-releasing with updated content.

### Dry run

```
node scripts/release.mjs --dry-run <version>
```

Previews the process without committing or building.

## Standalone tools

These can be run independently:

```sh
node scripts/version.mjs <version>        # bump version only
node scripts/version.mjs patch|minor|major  # semantic bump
node scripts/version.mjs --current         # show current version

node scripts/changelog.mjs                # generate changelog since last tag
node scripts/changelog.mjs --all           # full history changelog
node scripts/changelog.mjs --from v0.1.0   # from specific tag
node scripts/changelog.mjs --preview       # preview without writing
```

## Release body (`RELEASE.md`)

`RELEASE.md` is the canonical release body template for GitHub Releases. The release script (`release.mjs`) automatically substitutes `${version}` and `${date}` placeholders and includes the file in the release commit.

Before each release, update `RELEASE.md` with a curated summary of the current changelog:

1. Group related commits into feature areas (see existing sections for reference)
2. Attach commit hash links using the format:

   ```
   - **Feature description** — detail | [`hash`](https://github.com/muutot/Clipboard/commit/hash) [`hash2`](https://github.com/muutot/Clipboard/commit/hash2)
   ```

   Rules:
   - One point can reference multiple hashes; multiple points can reuse the same hash
   - Use the full 7+ char shortened hash visible in `CHANGELOG.md`
   - Group related commits under a single bullet with shared links

3. When creating a GitHub Release from the tag, copy the body from `RELEASE.md` (placeholders already substituted in the committed file)

## Post-release

After a successful release, report:

1. New version number
2. Tag created (`vx.x.x`)
3. Bundle location: `src-tauri/target/release/bundle/`
4. Push commands:
   ```
   git push origin core
   git push origin v<version>
   ```

## CI/CD

Pushing a `v*` tag triggers `.github/workflows/release.yml` which:

- Builds for Windows (x64), macOS (x64 + arm64), Linux (x64)
- Creates a draft GitHub Release with artifacts using `RELEASE.md` as the release body

## Version source files

| File                        | Key        |
| :-------------------------- | :--------- |
| `package.json`              | `.version` |
| `src-tauri/tauri.conf.json` | `.version` |
| `src-tauri/Cargo.toml`      | `version`  |

All three are updated atomically by `scripts/version.mjs`.

## Error recovery

If the release script fails mid-way:

- If version was already bumped: run `git checkout -- .` to revert config files
- If commit was created but tag failed: `git reset --soft HEAD~1` then re-run
- If build fails: fix the issue, then re-run with `--regenerate` mode

## Commit message format

Release commits use the gitmoji convention:

```
🔖 chore[release]: bump version to x.x.x
```
