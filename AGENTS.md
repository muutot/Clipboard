# Clipboard repository instructions

Before changing this repository, read `.agents/skills/clipboard-dev/SKILL.md` completely and follow its linked maintenance workflow.

In particular:

- Audit TODO completion from direct evidence before checking an item.
- Preserve the approved main-page style and apply the settings style gate before CSS changes.
- Keep parallel agent ownership non-overlapping.
- Commit one verified minimal feature or fix at a time.
- **Commit message must follow gitmoji format:** `<emoji> <type>[<scope>]: <message>` (e.g., `✨ feat[search]: add pagination`, `🐛 fix[viewer]: correct fullscreen crash`, `📝 docs[release]: ...`). See the gitmoji mapping table in `.agents/skills/clipboard-dev/SKILL.md` (Commit message format); the `commit-msg` hook in `.githooks/` checks it, so run `npm run hooks:install` once per clone.

## Repository audits

For requested codebase audits or structured health reports, use the repository copy at
`.agents/skills/code-auditor/SKILL.md` and its Clipboard profile. Skill maintenance does not itself
start an application audit. Keep this copy independent of the user's global skill installation.

## Release

Use `.agents/skills/version-release/SKILL.md` for version bumping, changelog generation, and release. Trigger with "升级版本到 x.x.x" or "release version x.x.x". Supports `--regenerate` mode for re-releasing current content.

## Pre-release Check Gate

Before any version bump or release (including `--regenerate`), apply the release gate **to the final tree**, and commit any formatting-only changes **as a separate, prior `🎨 style` commit**.

The release flow does **not** build anything locally — building is performed remotely by the GitHub Actions `release.yml` workflow when the `v*` tag is pushed. The local gate therefore runs `format` + `check` + `lint` (no build):

1. `npm run format:check` — prettier (`format:prettier:check`) + rustfmt (`format:rust:check`)
2. `npm run check` — svelte-check type checking
3. `npm run lint:rust` — cargo clippy `-D warnings`

> ⚠️ **The extreme-release build (fat LTO, opt-level 3, codegen-units 1) is GitHub Actions-only and must NEVER be run locally.** It is enabled solely by the environment variables `CARGO_PROFILE_RELEASE_LTO`, `CARGO_PROFILE_RELEASE_OPT_LEVEL`, and `CARGO_PROFILE_RELEASE_CODEGEN_UNITS` set in `.github/workflows/release.yml` when the `v*` tag is pushed. The local `[profile.release]` in `src-tauri/Cargo.toml` is intentionally fast and unoptimized (opt-level 0, codegen-units 256) — do not override it locally, and never run the extreme build by hand.

If any diffs appear, apply them (`npm run format:prettier` / `npm run format:rust`), then re-run the gate. Commit all formatting-only changes in a **single** `🎨 style[...]: apply formatting` commit **before** the release commit.

Note: `RELEASE.md` and `CHANGELOG.md` are generated **after** this gate runs. Before committing the release (Pass 2), re-run `npm run format:prettier:check` so the freshly curated `RELEASE.md` is also prettier-clean.

## Agent skills

All agent skills live under `.agents/skills/`. DSH discovers this directory
automatically: each `<name>/SKILL.md` with valid frontmatter appears in the session
skill catalog and is loaded with the `skill` tool. The first-party skills are
`clipboard-dev`, `code-auditor`, `version-release`, and `delete-remote-release`.
A small, curated set of [mattpocock/skills](https://github.com/mattpocock/skills) is
also vendored here (`tdd`, `diagnosing-bugs`, `codebase-design`, `code-review`,
`domain-modeling`, `grilling`); no plugin or skills.sh installer is wired into this
repo. Load the relevant skill when a task matches its description, and follow its
linked files.

**This file and `.agents/skills/clipboard-dev/SKILL.md` override the vendored skills whenever they
disagree.** In particular: commits still follow the gitmoji format and one verified minimal unit per
commit, the documentation currency gate still applies before every commit, and planning evidence
still comes from `TODO.md` and the routed references. The upstream `implement`, `implement-spec`,
`triage`, `to-tickets`, and setup wizard are deliberately not vendored.

### Issue tracker

GitHub issues in `muutot/Clipboard` via `gh`; `TODO.md` is the primary planning surface. See
`docs/agents/issue-tracker.md`.

### Domain docs

No `GLOSSARY.md` or `docs/adr/` exists yet; the maintained equivalents are
`.agents/skills/clipboard-dev/references/` and the live docs under `docs/`. See `docs/agents/domain.md`.

### Triage labels

Not applicable — `triage` is not vendored here.
