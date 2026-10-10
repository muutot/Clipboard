# Clipboard audit profile

Read this profile when auditing Clipboard. Paths below are relative to the repository root; verify them against the current tree. If this skill is packaged for another project, use that project's instructions instead.

## Repository workflow

Read `AGENTS.md`, `skills/clipboard-dev/SKILL.md` and its maintenance workflow before repository changes. Route focused inspection through the development skill's references; do not load all subsystem references for every audit.

- Inspect Git status and preserve unrelated work, including nested worktrees.
- TODO completion requires direct implementation evidence plus proportionate verification. An audit report alone does not authorize checking roadmap items.
- Preserve the approved main-page style. For requested UI remediation, read `css-theming.md`; settings changes also require `settings-panels.md` and its style gate.
- For authorized fixes, use one verified minimal commit at a time, inspect documentation currency, and use `<emoji> <type>[<scope>]: <English imperative message>`.

## Risk-driven source map

| Concern                  | Starting points and checks                                                                                                                                                                          |
| ------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| UI and state             | `src/routes/`, `src/lib/services/`, `src/lib/utils/store/item-store.ts`: initialization gates, cross-window events, out-of-order results, subscription cleanup, focus and pagination                |
| IPC and local API        | `src-tauri/src/lib.rs`, `commands/`, `cli/`: registered commands, invoke names, argument/result casing, permissions, token/Host/Origin checks, validation before effects                            |
| Persistence and deletion | `src-tauri/src/storage/`, `content/`, `export/`, `windows/installer.nsi`: atomicity, migrations, custom roots, ownership markers, favorites/recycle-bin preservation, import and uninstall behavior |
| Search, OCR and sync     | `src-tauri/src/search/`, `ocr/`, `sync/`, `src-tauri/crates/clipboard-sync/`: derived-data invalidation, resource bounds, cancellation, writer shutdown, retries and convergence                    |
| Privacy                  | `src-tauri/src/privacy/`, config and logging paths: local-only behavior, sensitive-source filtering, clipboard/OCR content in logs, credential storage and export boundaries                        |
| Native platform          | `src-tauri/src/platform/`: capture sequence consistency, self-trigger suppression, hotkeys, paste targeting, FFI lifetimes, platform degradation and CI coverage                                    |
| Release                  | `.github/workflows/`, lockfiles, installer and `skills/version-release/SKILL.md`: pinned actions, permissions, artifact verification and final-tree checks                                          |

Documentation is a navigation aid. Re-check a historical finding against current code before reporting it again. OCR models and installed coding-agent skills do not alone establish an application LLM attack surface.

## Verification limits

Use configured commands and record their actual results:

- Frontend: `npm run check`, focused `npm test -- <test-file>`.
- Rust: focused Cargo tests, `npm run test:rust`, `npm run lint:rust`.
- Before a fix commit: `npm run format:check` and the applicable maintenance gates.
- Release readiness: `npm run format:check`, `npm run check`, `npm run lint:rust`. Release building occurs in GitHub Actions on tag push. Never run or locally enable the extreme-release profile overrides.

Windows checks do not compile macOS/Linux `cfg` branches. Inspect matching CI or runtime evidence before claiming those platforms verified. Browser preview and static checks do not prove native clipboard, OS hotkeys, multi-window behavior or visual correctness. Record unavailable evidence as a limit, not an automatic defect or a passing result.

Audit tests use synthetic inputs and temporary directories. Do not run migration, cleanup, installer or clipboard-mutating experiments against the user's real data.
