---
name: version-release
description: Automated version bumping and release workflow for Clipboard Desktop. Use when the user asks to bump the version, release a new version, or regenerate a release. Supports semantic version bumping (patch/minor/major) and specific version targets.
---

# Version Release

## Trigger patterns

Start this skill when the user says any of:

- "升级版本到 x.x.x" / "bump version to x.x.x"
- "发布版本 x.x.x" / "release version x.x.x"
- "重新发布版本 x.x.x" / "regenerate release x.x.x"
- "升级 patch/minor/major 版本"
- "release" combined with a version number or bump type

## Release workflow (single script, two passes for RELEASE.md)

### Prerequisite: only version files in the release commit

Before running the release script, ensure that **every other change** has already been committed separately.

The release commit (`🔖 chore[release]: bump version to x.x.x`) **must only contain**:

- `package.json`
- `src-tauri/tauri.conf.json`
- `src-tauri/Cargo.toml`
- `src-tauri/Cargo.lock`
- `CHANGELOG.md`
- `RELEASE.md`

Any change to scripts, skills, references, tests, or other source files **must be committed before** the release. The release script's `git diff --name-only` may pick up unrelated dirty files — verify the staged diff before allowing the commit.

Run:

```
node scripts/release.mjs <version>
```

The script does the following:

| Step | What                                                                 |
| ---- | -------------------------------------------------------------------- |
| 1    | Bump version in `package.json`, `tauri.conf.json`, `Cargo.toml`      |
| 2    | Generate `CHANGELOG.md` from commits since last tag                  |
| 3    | Check `RELEASE.md` — if stale, prints instructions and exits cleanly |
| 4    | Commit version files + CHANGELOG.md + RELEASE.md                     |
| 5    | Create git tag `vx.x.x`                                              |
| 6    | Push to origin (triggers GitHub Actions release workflow)            |

The script is **idempotent**: re-running with the same version skips already-done steps.

### Pass 1 — Script bumps + generates changelog

```
node scripts/release.mjs <version>
```

Steps 1–2 run, then Step 3 detects stale `RELEASE.md` and exits.

### Between passes — LLM generates RELEASE.md

Read `CHANGELOG.md` and use `skills/version-release/release_template.md` as a format reference:

1. Group related commits into feature areas
2. Attach commit hash links:
   ```
   - **Feature description** — detail | [`hash`](https://github.com/muutot/Clipboard/commit/hash)
   ```
3. Write the curated body to `RELEASE.md`

**Do NOT commit** — Pass 2 will include `RELEASE.md` in the release commit automatically.

### Pass 2 — Script commits + tags + pushes

Re-run the **same** command — already-bumped steps skip, `RELEASE.md` check passes:

```
node scripts/release.mjs <version>
```

Steps 3–6 run: check, commit, tag, push to origin.

### Semantic bump

```
node scripts/release.mjs patch    # 0.1.0 → 0.1.1
node scripts/release.mjs minor    # 0.1.0 → 0.2.0
node scripts/release.mjs major    # 0.1.0 → 1.0.0
```

Same two-pass flow applies.

### Regenerate mode

Re-releases the current version by dropping the old release commit + tag from history first, then re-running the normal flow:

```
node scripts/release.mjs --regenerate <version>
```

Uses `git rebase --onto` to surgically remove the previous release commit (preserving other commits' content and timestamps) and deletes the old tag. The normal flow then creates a fresh changelog, commit, and tag.

### Dry run

```
node scripts/release.mjs --dry-run <version>
```

Previews the process without committing, tagging, or pushing.

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

`RELEASE.md` is the canonical release body for GitHub Releases. It is **manually curated** by the LLM during each release, following the format in `skills/version-release/release_template.md`.

Pushing the tag triggers CI/CD which reads `RELEASE.md` automatically as the GitHub Release body.

## Post-release

After a successful release, report:

1. New version number
2. Tag created (`vx.x.x`)
3. Release has been pushed to origin (GitHub Actions will build artifacts automatically)

## CI/CD

Pushing a `v*` tag triggers `.github/workflows/release.yml` which:

- Builds for Windows (x64), macOS (arm64), Linux (x64)
- Intel macOS (x86_64-apple-darwin) is excluded: ort-sys ships no prebuilt ONNX Runtime for it
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

## Commit message format

Release commits use the gitmoji convention:

```
🔖 chore[release]: bump version to x.x.x
```
