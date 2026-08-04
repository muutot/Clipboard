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

## Pre-release Format Check

Before any version bump or release (including `--regenerate`), run format checks and commit any formatting changes **as a separate, prior commit**:

1. Run `npm run format:prettier:check` and `npm run format:rust:check`
2. If there are formatting diffs, run `npm run format:prettier` / `npm run format:rust` to apply them
3. Commit all formatting-only changes in a single `🎨 style[...]: apply formatting` commit
4. Then proceed with the version bump / release commit
