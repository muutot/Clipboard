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

## 🚨 HARD RULE — the tag points at the release commit and nothing else

The version tag `vx.x.x` may **ONLY** ever be bound to the release commit
`🔖 chore[release]: bump version to x.x.x`. **Never bind it to any other commit** —
not a fix, not a chore, not a merge. The tag is what triggers the GitHub Actions
release build, so a tag on a non-release commit ships an unversioned, unverified
tree and can publish broken artifacts.

**Every release, every re-tag, every regenerate — this rule is non-negotiable:**

1. The release script creates the tag at HEAD right after the release commit, and
   its hard-rule guard **refuses to tag** if HEAD's subject is not exactly
   `🔖 chore[release]: bump version to x.x.x`.
2. When reordering, rebase-rebuilding, or manually moving a tag: **rewrite the
   history first so the release commit is the tip**, then tag _that_ commit.
3. Before pushing, verify the binding: `git rev-parse <tag>^{}` must resolve to a
   commit whose subject is `🔖 chore[release]: bump version to x.x.x`
   (`git log -1 --pretty=%s <tag>^{}`).
4. Push the branch first, the tag last.

## 🚨 HARD RULE — the release commit contains only the six release files

The release commit must contain **exactly** the six release files and nothing else:
`package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`,
`CHANGELOG.md`, `RELEASE.md`. Never fold a code fix, test, script, or skill change into it —
**not even when re-running a failed release**. Enforced twice:

1. **Locally** — `release.mjs` refuses to run while any file outside the six is dirty or
   staged. **Always create the release commit through the script.** Hand-rolling
   `git reset --soft` + `git add` + `git commit` bypasses this guard, which is exactly how
   mixed release commits happen.
2. **Remotely** — `scripts/validate-release-version.mjs` (release.yml Resolve job) fails the
   build when the tagged commit touches any file outside the six.

### Post-push CI failure recovery

When remote CI fails on a pushed release commit or tag, the remedy is a code fix, and a code
fix is always its own commit (one verified minimal fix at a time):

1. If a GitHub release was already published, delete it (with its assets) first, then the
   remote tag (`git push origin :refs/tags/vx.x.x`) and the local tag (`git tag -d vx.x.x`).
2. Commit the fix standalone: `🐛 fix[scope]: ...`.
3. Re-run the normal two-pass flow — `release.mjs x.x.x` is idempotent and regenerates the
   changelog with the fix included — so Pass 2 creates a **fresh** release commit on top.
4. Re-tag, verify the binding (`git rev-parse <tag>^{}` must equal the release commit),
   push the branch first, the tag last.

See "Regenerate mode" for dropping an existing release commit from history.

## Release workflow (single script, two passes for RELEASE.md)

### Pre-release Check Gate

Before any version bump or release (including `--regenerate`), apply the release gate **to the final tree**, and commit any formatting-only changes **as a separate, prior `🎨 style` commit**.

The release flow does **not** build anything locally — building is performed remotely by the GitHub Actions `release.yml` workflow when the `v*` tag is pushed. The local gate therefore runs `format` + `check` + `lint` (no build):

1. `npm run format:check` — prettier (`format:prettier:check`) + rustfmt (`format:rust:check`)
2. `npm run check` — svelte-check type checking
3. `npm run lint:rust` — cargo clippy `-D warnings`

> ⚠️ **The extreme-release build (fat LTO, opt-level 3, codegen-units 1) is GitHub Actions-only and must NEVER be run locally.** It is enabled solely by the environment variables `CARGO_PROFILE_RELEASE_LTO`, `CARGO_PROFILE_RELEASE_OPT_LEVEL`, and `CARGO_PROFILE_RELEASE_CODEGEN_UNITS` set in `.github/workflows/release.yml` when the `v*` tag is pushed. The local `[profile.release]` in `src-tauri/Cargo.toml` is intentionally fast and unoptimized (opt-level 0, codegen-units 256) — do not override it locally, and never run the extreme build by hand.

If any diffs appear, apply them (`npm run format:prettier` / `npm run format:rust`), then re-run the gate. Commit all formatting-only changes in a **single** `🎨 style[...]: apply formatting` commit **before** the release commit.

> The release script _bumps the version first_ (Pass 1), so run the gate on the code **before** starting, then re-check `RELEASE.md` after curating it (see Pass 2 note below).

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
node skills/version-release/scripts/release.mjs <version>
```

The script does the following:

| Step | What                                                                                       |
| ---- | ------------------------------------------------------------------------------------------ |
| 1    | Bump version in `package.json`, `tauri.conf.json`, `Cargo.toml`                            |
| 2    | Generate `CHANGELOG.md` from commits since last tag                                        |
| 3    | Check `RELEASE.md` — if stale, prints instructions and exits cleanly                       |
| 4    | Commit version files + CHANGELOG.md + RELEASE.md                                           |
| 5    | Create git tag `vx.x.x`                                                                    |
| 6    | **Local-only: no remote push.** Print the `git push` commands for the user to run manually |

The script is **idempotent**: re-running with the same version skips already-done steps, and
`changelog.mjs` replaces this version's existing `## <version> (` section instead of prepending a
second copy of it, so running both passes leaves exactly one section. It also accepts both
`[scope]` and `(scope)` commit subjects and warns on a subject or type it cannot map, rather
than dropping the commit silently.

### Pass 1 — Script bumps + generates changelog

```
node skills/version-release/scripts/release.mjs <version>
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
node skills/version-release/scripts/release.mjs <version>
```

Steps 3–6 run: check, commit, tag. Remote push is the user's manual step (the script never pushes).

**Re-check formatting after curation.** `RELEASE.md` is written _after_ the pre-release gate. Before Pass 2, run `npm run format:prettier:check` (or `npm run format:prettier -- RELEASE.md` to fix) so the freshly curated `RELEASE.md` is prettier-clean. Fix any diff (`npx prettier --write RELEASE.md`), then commit it either as a separate `🎨 style[release]` commit or folded into the release commit — do **not** push a release whose `RELEASE.md` fails the format gate.

### Semantic bump

```
node skills/version-release/scripts/release.mjs patch    # 0.1.0 → 0.1.1
node skills/version-release/scripts/release.mjs minor    # 0.1.0 → 0.2.0
node skills/version-release/scripts/release.mjs major    # 0.1.0 → 1.0.0
```

Same two-pass flow applies.

### Regenerate mode

Re-releases the current version. **The first step must delete the old release commit + tag for that version** before re-running the normal flow:

```
node skills/version-release/scripts/release.mjs --regenerate <version>
```

The script locates the old release commit by the local tag **or** by scanning history (including remote-tracking refs) for `bump version to <version>`, then:

- if it is an ancestor of `HEAD`, drops it with the standard single-commit removal `git rebase --committer-date-is-author-date --onto <parent> <commit> <branch>`, which replays later commits onto the old release commit's parent while `--committer-date-is-author-date` preserves every surviving commit's original timestamp. Before rewriting, it requires a clean working tree, backs the branch tip up to `refs/backup/pre-release-delete-<sha>`, warns when a replayed commit's author date differs from its committer date, and on failure aborts the rebase and restores the previous tip;
- otherwise (old release commit exists only on a remote ref), notes that you must force-push / delete the remote tag manually to drop it;
- deletes the old tag locally (you must also delete the remote tag manually), and records that history was rewritten so the printed push instructions include `--force-with-lease`.

The normal flow then creates a fresh changelog, commit, and tag. Verify the deletion actually happened before Pass 1: `git log --oneline <branch> | findstr "bump version to <version>"` should show nothing, and the old tag should be gone. The pre-delete tip is kept as a recovery backup at `refs/backup/pre-release-delete-<sha>` (so the old release commit may still appear under `git log --all`); the script's own history scan excludes `refs/backup/*`, which is why re-running `--regenerate` for Pass 2 reports no old release commit.

### Dry run

```
node skills/version-release/scripts/release.mjs --dry-run <version>
```

Previews the process without committing, tagging, or pushing (the script never pushes anyway).

## Standalone tools

These can be run independently:

```sh
node skills/version-release/scripts/version.mjs <version>        # bump version only
node skills/version-release/scripts/version.mjs patch|minor|major  # semantic bump
node skills/version-release/scripts/version.mjs --current         # show current version

node skills/version-release/scripts/changelog.mjs                # generate changelog since last tag
node skills/version-release/scripts/changelog.mjs --all           # full history changelog
node skills/version-release/scripts/changelog.mjs --from v0.1.0   # from specific tag
node skills/version-release/scripts/changelog.mjs --preview       # preview without writing
```

### Delete a commit

`skills/version-release/scripts/delete-commit.mjs` drops a single commit from the current branch by rewriting history locally (the same mechanism `--regenerate` uses internally). It is **local-only** — it never touches the remote.

```sh
node skills/version-release/scripts/delete-commit.mjs <commit>          # delete <commit> (rewrites history)
node skills/version-release/scripts/delete-commit.mjs --dry-run <commit> # preview replay range; no changes
node skills/version-release/scripts/delete-commit.mjs <commit> --branch <name>  # target a non-current branch
```

Behavior:

- Requires a clean working tree; exits safely if `<commit>` does not exist or is not an ancestor of the branch tip.
- Backs the branch tip up to `refs/backup/pre-delete-<sha>` before rewriting, so it can be restored with `git update-ref refs/heads/<branch> refs/backup/pre-delete-<sha>`.
- Replays commits after `<commit>` with `--committer-date-is-author-date` (timestamps preserved when author date == committer date; warns otherwise).
- On conflict it aborts the rebase and restores the previous tip automatically.
- Only use this when the branch has **not** been pushed (or you will force-push afterward) — like `--regenerate`, it changes SHAs.

## Release body (`RELEASE.md`)

`RELEASE.md` is the canonical release body for GitHub Releases. It is **manually curated** by the LLM during each release, following the format in `skills/version-release/release_template.md`.

Pushing the tag (done manually by the user) triggers CI/CD which reads `RELEASE.md` automatically as the GitHub Release body.

## Post-release

The release script is **local-only** — it performs no remote operations. After a successful run, report:

1. New version number
2. Tag created locally (`vx.x.x`)
3. Release commit + tag exist only locally; **the user pushes to origin manually** to trigger GitHub Actions (which builds artifacts automatically)

## CI/CD

Manual dispatch and tag pushes resolve an existing `v<version>` tag through
`scripts/validate-release-version.mjs --resolve`. Resolution reads the tagged tree,
checks version files and the release-only commit rules, then exports its SHA.
Verify and all build jobs check out that same SHA. Manual inputs accept either
`x.y.z` or `vx.y.z`; selecting a newer dispatch branch does not change release
contents. `node --test scripts/test-release-identity.mjs` verifies these rules in
temporary Git repositories without publishing or changing project tags.

When the release commit is pushed to the main branch (manually, by the user), CI still runs: `ci.yml` only ignores pushes that touch solely `CHANGELOG.md` / `RELEASE.md`; manifest and version files intentionally still trigger the full pipeline.

Pushing a `v*` tag triggers `.github/workflows/release.yml` which:

- Builds for Windows (x64), macOS (arm64), Linux (x64)
- Intel macOS (x86_64-apple-darwin) is excluded: ort-sys ships no prebuilt ONNX Runtime for it
- Publishes the GitHub Release as a draft (`releaseDraft: true`) and only flips it live in the final `publish` job once all artifacts are attached

The separate `.github/workflows/sync-gitcode.yml` workflow mirrors the published release body + assets to GitCode via `scripts/sync_release.py`. It is decoupled from `release.yml` and is triggered **manually** (`workflow_dispatch` with the release tag) after a release is published, so it can be run once all assets are attached. It requires repo secret `GITCODE_TOKEN` (rotate immediately if ever committed in plaintext); optional `GITCODE_OWNER` overrides the default owner. All actions are pinned to commit SHAs and its Python dependencies come from the pinned `scripts/requirements-sync.txt` (installed with `pip install -r`) rather than an unpinned `pip install`, because this job holds `GITCODE_TOKEN` and `GITHUB_TOKEN`.

## Version source files

| File                        | Key        |
| :-------------------------- | :--------- |
| `package.json`              | `.version` |
| `src-tauri/tauri.conf.json` | `.version` |
| `src-tauri/Cargo.toml`      | `version`  |

All three are updated atomically by `skills/version-release/scripts/version.mjs`.

## Error recovery

If the release script fails mid-way:

- If version was already bumped: run `git checkout -- .` to revert config files
- If commit was created but tag failed: `git reset --soft HEAD~1` then re-run

If remote CI failed **after** the release commit/tag was pushed, follow
"Post-push CI failure recovery" above — the fix lands as its own commit and the
release commit is re-created through the script, never hand-folded.

## Commit message format

Release commits use the gitmoji convention:

```
🔖 chore[release]: bump version to x.x.x
```
