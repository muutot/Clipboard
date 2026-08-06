# Project Structure and Runtime Surfaces

This reference is a source map, not a substitute for reading the current files. Avoid brittle line counts and module counts; update this map when ownership or entry points change.

## Runtime surfaces

| Surface                   | Entry point                                                         | Responsibility                                                                                          |
| ------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| Main desktop window       | `src/routes/+page.svelte`                                           | Search, filtering, pagination, virtual list, card actions, bulk actions, detail opening, runtime events |
| Settings window           | `src/routes/settings/+page.svelte` → `StorageSettingsDialog.svelte` | Standalone settings WebviewWindow, theme/font application, settings navigation and panels               |
| Desktop fullscreen viewer | `src/routes/+page.svelte::openDesktopViewer`                        | Fullscreen image viewer via Fullscreen API with zoom, pan, drag; created in `+page.svelte`              |
| GUI backend               | `src-tauri/src/lib.rs::run`                                         | Tauri setup, managed state, commands, workers, tray, hotkeys, shutdown                                  |
| Process CLI               | `src-tauri/src/main.rs` → `cli/mod.rs`                              | `list`, `search`, `copy`, `paste`, `delete`, `export`, and `stats` without launching the GUI            |
| Loopback API              | `src-tauri/src/cli/api.rs`                                          | Optional local HTTP automation server bound to `127.0.0.1`                                              |

SvelteKit runs as a static SPA: `src/routes/+layout.ts` disables SSR and awaits `generalSettings.initialize()` before route load. `+layout.svelte` imports global CSS and applies settings to the document.

## Frontend ownership

| Path                                 | Ownership                                                                                                  |
| ------------------------------------ | ---------------------------------------------------------------------------------------------------------- |
| `src/app.css`                        | Global reset, theme defaults, font variables, focus/accessibility media rules; imports shared settings CSS |
| `src/lib/styles/settings-shared.css` | Reusable settings-panel primitives                                                                         |
| `src/lib/components/`                | Cards, detail/view helpers, settings panels, icons, context menu, toast                                    |
| `src/lib/services/`                  | Tauri invoke wrappers, stores, persistence, record mapping, runtime detection                              |
| `src/lib/types/`                     | Frontend contracts for clipboard, settings, runtime, and memory diagnostics                                |
| `src/lib/utils/`                     | Date query, keyboard target checks, theme application, time formatting, virtual-scroll calculations        |
| `src/lib/i18n/`                      | Locale store, typed message shape, English and Simplified Chinese dictionaries                             |
| `src/lib/settings-search.ts`         | Search index/metadata for settings items and navigation targets                                            |
| `src/lib/data/demo-items.ts`         | Browser-preview fallback data; not proof of desktop persistence behavior                                   |

Read `components.md` and `services.md` for ownership and change gates inside those directories.

## Backend ownership

| Path                   | Ownership                                                                                          |
| ---------------------- | -------------------------------------------------------------------------------------------------- |
| `src-tauri/src/lib.rs` | Runtime composition and Tauri boundary; keep business logic in focused modules when practical      |
| `config.rs`            | `conf/conf.json` schema, defaults, validation, and atomic persistence                              |
| `domain/`              | Shared Rust clipboard and OCR domain types                                                         |
| `storage/`             | SQLite connection/repositories, schema, storage paths, recovery/backups, integrity repair helpers  |
| `search/`              | Tantivy schema/query/index, manifest, outbox synchronization                                       |
| `ocr/`                 | Engine trait, PP-OCR/Tesseract/no-op engines, models, restartable worker manager                   |
| `content/`             | Detection/actions, hashing/self-trigger rules, file storage, metadata, thumbnails, text transforms |
| `keyboard/`            | Shortcut config in `conf/keyboard.json`, binding parse/match, manager                              |
| `platform/`            | Clipboard monitor, hotkey, tray, single-instance and platform-specific adapters                    |
| `privacy/`             | Pause/ignore/sensitive-source policy helpers                                                       |
| `performance/`         | Startup/search/performance snapshots and monitoring                                                |
| `export/`              | JSON, CSV, plain-text import/export, and PPaste `.Pastebackup` import (`ppaste.rs`)                |
| `commands/`            | Tauri command modules (clipboard, config, update, system, files, OCR, etc.)                        |
| `cli/`                 | Process CLI execution and loopback API server                                                      |

Platform source files may contain detailed scaffolding or documented intended flows. Verify runtime wiring and tests before claiming platform completion.

## Persistent layout

```text
<project>/
├─ conf/
│  ├─ conf.json                 # ConfigStore; remains beside the executable/project
│  └─ keyboard.json             # KeyboardManager; separate contract
└─ storage/                     # default data root, or under a configured data directory
   ├─ image/
   │  └─ previews/
   ├─ files/
   ├─ icons/
   └─ database/
      ├─ clipboard.sqlite3
      └─ search-index/
```

Custom image and file roots can replace their default resource directories. Cleanup eligibility is separate from usability and depends on validated ownership markers. See `backend-architecture.md` and `data-contracts.md` before changing paths or migration behavior.

## High-coupling files

Treat these as integration points and avoid concurrent edits:

- `src/routes/+page.svelte`
- `src/lib/components/StorageSettingsDialog.svelte`
- `src/lib/types/clipboard.ts`
- `src/lib/services/settings.ts`
- locale files plus `src/lib/i18n/types.ts`
- `src-tauri/src/lib.rs`
- `src-tauri/src/config.rs`
- `src-tauri/src/storage/migrations.rs`
- `TODO.md`
- `SKILL.md` and shared references

## Structure update rule

When adding, removing, renaming, or moving a route/module/component/service, update this map and the focused reference that describes its contract. Describe stable ownership; do not add transient implementation notes or raw file listings that will immediately go stale.
