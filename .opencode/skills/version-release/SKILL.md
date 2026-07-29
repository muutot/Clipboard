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

The release happens in **two passes** because `RELEASE.md` must be curated manually by the LLM.

#### Pass 1 — Script runs steps 1–4 (verify → bump → changelog)

```
node scripts/release.mjs <version>
```

The pipeline runs:

1. Pre-flight checks (clean working directory)
2. `npm run verify`
3. Version bump in `package.json`, `tauri.conf.json`, `Cargo.toml`
4. Changelog generation from commits since last tag (`CHANGELOG.md`)
5. Check `RELEASE.md` — if stale, prints instructions and exits cleanly

At this point the script **exits** with a message telling you to update `RELEASE.md`.

#### Between passes — LLM generates RELEASE.md

Read `CHANGELOG.md` and use `.opencode/skills/version-release/release_template.md` as a format reference:

1. Group related commits into feature areas (see template sections for reference)
2. Attach commit hash links using the format:

   ```
   - **Feature description** — detail | [`hash`](https://github.com/muutot/Clipboard/commit/hash) [`hash2`](https://github.com/muutot/Clipboard/commit/hash2)
   ```

   Rules:
   - One point can reference multiple hashes; multiple points can reuse the same hash
   - Use the full 7+ char shortened hash visible in `CHANGELOG.md`
   - Group related commits under a single bullet with shared links

3. Write the curated body to `RELEASE.md`
4. Commit it:

   ```
   git add RELEASE.md
   git commit -m "📝 docs[release]: update RELEASE.md for v<version>"
   ```

#### Pass 2 — Script runs steps 5–7 (verify RELEASE.md → commit → tag → build)

Re-run the **same** command — already-bumped steps are idempotent:

```
node scripts/release.mjs <version>
```

The pipeline continues:

5. Verify `RELEASE.md` matches the target version ✓
6. Commit (version files + CHANGELOG.md + RELEASE.md) + annotated tag
7. Tauri production build (MSI + NSIS)

### Semantic bump

```
node scripts/release.mjs patch    # 0.1.0 → 0.1.1
node scripts/release.mjs minor    # 0.1.0 → 0.2.0
node scripts/release.mjs major    # 0.1.0 → 1.0.0
```

Same two-pass flow applies.

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

`RELEASE.md` is the canonical release body for GitHub Releases. It is **manually curated** by the LLM during each release, following the format in `.opencode/skills/version-release/release_template.md`.

The release script no longer performs placeholder substitution — it only verifies that `RELEASE.md` references the correct version before proceeding to commit.

Pushing the tag triggers CI/CD which reads `RELEASE.md` automatically as the GitHub Release body.

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
