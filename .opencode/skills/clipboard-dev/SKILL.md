---
name: clipboard-dev
description: Develop and maintain the Clipboard Desktop repository. Use for repository orientation, TODO audits, Svelte/Tauri feature work, frontend-backend contract changes, settings and theme UI work, storage/search/OCR/platform changes, verification, and minimal Git commits. Enforces direct-evidence completion, the approved main-page style, settings style gates, reference routing, lifecycle safety, and a mandatory skill/reference currency check before every commit.
---

# Clipboard Desktop Development

## Start every task

1. Read [references/maintenance-workflow.md](references/maintenance-workflow.md) completely before changing the repository.
2. Inspect `git status --short --branch`, recent commits, `TODO.md`, and the exact source and tests in scope. Preserve unrelated user changes.
3. Select the relevant references from the routing table below and read each selected file completely before editing its subsystem.
4. Treat current source, tests, configuration, and rendered/runtime behavior as authoritative. References are navigation aids and must be corrected when evidence disagrees.
5. Define one independently verifiable feature, fix, audit, style pass, or documentation update for the next commit.

## Non-negotiable principles

- Check a TODO only when direct implementation evidence and proportionate verification cover its full wording. A stub, setting, UI shell, similarly named symbol, or untested platform file is not completion.
- Preserve the approved main-page visual language. Do not redesign it unless the task explicitly requests a main-page redesign.
- Apply the settings style gate before any settings CSS or markup change. Reuse project-wide semantic variables and shared settings primitives.
- Protect local data first. Be conservative around migrations, cleanup, custom resource roots, self-trigger suppression, favorites, recycle-bin records, OCR, and derived search data.
- Keep parallel ownership non-overlapping. The primary agent owns shared integration files and final verification.
- Commit one verified minimal unit at a time; never mix unrelated cleanup into the same commit.

## Mandatory documentation currency gate — every commit

Before **every** commit, inspect the staged diff and decide whether `SKILL.md` or any file under `references/` must change. This gate is required even when the answer is “no documentation update needed.”

Update the matching reference in the same commit when the change affects any of these:

- routes, components, services, utilities, backend modules, or ownership boundaries;
- Tauri commands/events, TypeScript/Rust types, database/config schemas, defaults, or serialization;
- lifecycle, data-safety, search-cache, OCR, platform, or verification invariants;
- theme variables, project-wide styles, settings primitives, layout hierarchy, or an approved pattern;
- a recurring pitfall, workflow rule, or stable project preference.

Keep stable workflow and routing rules in `SKILL.md`. Put module-specific facts, signatures, examples, and style details in the relevant reference. Do not edit documentation merely to create churn, but never commit a known-stale skill or reference.

## Reference router

| Task                                                                    | Read before editing                                                                                             |
| ----------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| Any repository change, TODO audit, verification, or commit              | [maintenance-workflow.md](references/maintenance-workflow.md)                                                   |
| Repository orientation, routes, directories, runtime surfaces           | [project-structure.md](references/project-structure.md)                                                         |
| Svelte component behavior or component ownership                        | [components.md](references/components.md)                                                                       |
| Frontend stores, invoke wrappers, mapping, or settings bootstrap        | [services.md](references/services.md)                                                                           |
| Database/config/type/IPC/event contract changes                         | [data-contracts.md](references/data-contracts.md)                                                               |
| Rust startup, workers, storage, search, OCR, privacy, platform, CLI/API | [backend-architecture.md](references/backend-architecture.md)                                                   |
| Any UI or CSS change                                                    | [css-theming.md](references/css-theming.md)                                                                     |
| Settings shell, panel markup, controls, feedback, or settings CSS       | [settings-panels.md](references/settings-panels.md) and [css-theming.md](references/css-theming.md)             |
| General setting fields, defaults, normalization, persistence            | [settings-reference.md](references/settings-reference.md) and [data-contracts.md](references/data-contracts.md) |
| Search pagination or cache behavior                                     | [search-cache-strategy.md](references/search-cache-strategy.md)                                                 |
| Reviewing a local/niche UI exception                                    | [niche_ui_style.md](references/niche_ui_style.md); treat it as a review queue, never as an approved pattern     |

Also read the focused project document when relevant: `docs/PITFALLS.md`, `docs/SEARCH.md`, `docs/OCR.md`, or `docs/DEFAULTS_AND_PRIVACY.md`.

## Cross-cutting change gates

- **Tauri contract:** keep command name, Rust arguments/result, frontend `invoke`, serde casing, error handling, and tests aligned.
- **Settings contract:** update TypeScript type, defaults, normalization/ranges, Rust config serde/defaults, UI, persistence, cross-window application, i18n, tests, and references as applicable.
- **Database contract:** update schema/migration, row mapping, repository behavior, derived search/OCR cleanup, recovery expectations, and tests.
- **i18n contract:** update `src/lib/i18n/locales/en.ts`, `zh-CN.ts`, and `src/lib/i18n/types.ts` together unless the string is intentionally non-localized and documented.
- **Worker/listener contract:** retain stop signals, join handles, unlisten functions, and the unified shutdown path.
- **Platform contract:** separate shared behavior from per-platform implementation and degradation. Do not infer macOS/Linux completion from a compiled adapter or documentation scaffold.
- **Visual contract:** type/build checks do not prove appearance. Use structural comparison and, when available, rendered/runtime inspection at the target window size and theme.

## Verification commands

```powershell
npm run check
npm run build
npm run test:rust
npm run lint:rust
npm run format:check
npm run verify
```

Run the narrowest relevant checks during implementation. Run `npm run verify` at integration milestones or before a commit whose scope crosses frontend and backend. If an environment prevents a required runtime, visual, platform, packaging, or performance check, report the missing evidence and leave the corresponding TODO unverified.

## Commit message format

Follow the gitmoji convention established by the earliest ~70 commits of this repository:

```
<gitmoji> <type>[<scope>]: <message>
```

- **gitmoji**: single emoji indicating the change category (see table below).
- **type**: lowercase change type matching the emoji (e.g., `feat`, `fix`, `docs`, `refactor`).
- **scope**: optional, lowercase, in **square brackets** (e.g., `[storage]`, `[settings]`, `[search]`).
- **message**: concise imperative description, Chinese or English.

### Gitmoji mapping

| Emoji | Type     | Use when                                                |
| ----- | -------- | ------------------------------------------------------- |
| ✨    | feat     | new feature or capability                               |
| 🐛    | fix      | bug fix or correction                                   |
| 📝    | docs     | documentation, roadmap, skill, or reference update      |
| ♻️    | refactor | code restructuring without behavior change              |
| 🎨    | style    | formatting, CSS, visual polish                          |
| 🚀    | perf     | performance improvement                                 |
| ✅    | test     | adding or updating tests                                |
| 🔧    | chore    | tooling, dependencies, build scripts, CI                |
| 🎉    | chore    | initial commit / project bootstrap                      |
| 🗃️    | feat     | database / schema changes                               |
| 🔒    | feat     | privacy, security, permissions, defaults enforcement    |
| 🔍    | feat     | search / index functionality                            |
| 🔌    | feat     | exposing commands / connecting frontend to backend      |
| 🔄    | feat     | synchronization, reload, outbox                         |
| ⌨️    | feat     | keyboard / shortcuts                                    |
| ⚙️    | feat     | configuration / settings                                |
| 💾    | feat     | storage / persistence                                   |
| 📦    | feat     | packaging / dependencies                                |
| 🔎    | feat     | search / query / listing                                |
| 🏷️    | feat     | versioning / identification                             |
| 💬    | fix      | messaging / empty-state / user-facing text              |
| 🙈    | fix      | hiding / ignoring / suppressing                         |
| 📁    | feat     | file / directory organization                           |
| 📂    | feat     | directory / path support                                |
| 🔡    | fix      | text normalization / case handling                      |
| ✏️    | fix      | minor text / copy corrections                           |
| ⚡    | chore    | build / dependency addition                             |
| 🛠️    | feat     | tooling / utility commands                              |

### Examples

```
📝 docs[skill]: document commit message format convention
✨ feat[search]: add backend SearchResultCache and DB-level pagination truncation
🐛 fix[viewer]: finalize fullscreen refactor, a11y fix, cleanup old viewer route
♻️ refactor[css]: remove !important from actions-hidden and transfer-column
🎨 style[settings]: format code and minor cleanup across settings modules
🚀 perf[frontend]: cache virtual scroll heights, updateItem helper, named card handlers
```

## Commit discipline

1. Review `git diff` and `git diff --cached`; stage only the intended unit.
2. Run the relevant verification and the mandatory documentation currency gate.
3. Update matching TODO evidence in the same commit only when completion is directly proven.
4. Write the commit message in the gitmoji format described above.
5. Recheck `git status` after committing and report verification plus any evidence gaps.
