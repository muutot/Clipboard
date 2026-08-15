# Clipboard repository instructions

Before changing this repository, read `skills/clipboard-dev/SKILL.md` completely and follow its linked maintenance workflow.

In particular:

- Audit TODO completion from direct evidence before checking an item.
- Preserve the approved main-page style and apply the settings style gate before CSS changes.
- Keep parallel agent ownership non-overlapping.
- Commit one verified minimal feature or fix at a time.
- **Commit message must follow gitmoji format:** `<emoji> <type>[<scope>]: <message>` (e.g., `✨ feat[search]: add pagination`, `🐛 fix[viewer]: correct fullscreen crash`, `📝 docs[release]: ...`). See full type/emoji table in `skills/clipboard-dev/SKILL.md:96-135`.

## Release

Use `skills/version-release/SKILL.md` for version bumping, changelog generation, and release. Trigger with "升级版本到 x.x.x" or "release version x.x.x". Supports `--regenerate` mode for re-releasing current content.

## Pre-release Check Gate

Before any version bump or release (including `--regenerate`), apply the release gate **to the final tree**, and commit any formatting-only changes **as a separate, prior `🎨 style` commit**.

The release flow does **not** build anything locally — building is performed remotely by the GitHub Actions `release.yml` workflow when the `v*` tag is pushed. The local gate therefore runs `format` + `check` + `lint` (no build):

1. `npm run format:check` — prettier (`format:prettier:check`) + rustfmt (`format:rust:check`)
2. `npm run check` — svelte-check type checking
3. `npm run lint:rust` — cargo clippy `-D warnings`

> ⚠️ **The extreme-release build (fat LTO, opt-level 3, codegen-units 1) is GitHub Actions-only and must NEVER be run locally.** It is enabled solely by the environment variables `CARGO_PROFILE_RELEASE_LTO`, `CARGO_PROFILE_RELEASE_OPT_LEVEL`, and `CARGO_PROFILE_RELEASE_CODEGEN_UNITS` set in `.github/workflows/release.yml` when the `v*` tag is pushed. The local `[profile.release]` in `src-tauri/Cargo.toml` is intentionally fast and unoptimized (opt-level 0, codegen-units 256) — do not override it locally, and never run the extreme build by hand.

If any diffs appear, apply them (`npm run format:prettier` / `npm run format:rust`), then re-run the gate. Commit all formatting-only changes in a **single** `🎨 style[...]: apply formatting` commit **before** the release commit.

Note: `RELEASE.md` and `CHANGELOG.md` are generated **after** this gate runs. Before committing the release (Pass 2), re-run `npm run format:prettier:check` so the freshly curated `RELEASE.md` is also prettier-clean.
