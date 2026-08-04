# Data, Configuration, IPC, and Event Contracts

Use this reference whenever a value crosses TypeScript, Tauri, Rust, SQLite, JSON config, the filesystem, or a WebviewWindow boundary.

## Clipboard record contract

| Layer              | Source                                                                   | Role                                                            |
| ------------------ | ------------------------------------------------------------------------ | --------------------------------------------------------------- |
| Rust domain        | `src-tauri/src/domain/clipboard_item.rs`                                 | persisted/backend `ClipboardItem` and lowercase `ClipboardKind` |
| SQLite mapping     | `src-tauri/src/storage/repository.rs`                                    | row conversion, CRUD, dedup, favorite/delete semantics          |
| Frontend raw type  | `PersistedClipboardItem` in `src/lib/types/clipboard.ts`                 | camelCase Tauri payload                                         |
| Frontend view type | `ClipboardItem` in the same file                                         | display-enriched item used by cards/routes                      |
| Mapping            | `toClipboardItem` and `parseResourceMetadata` in `services/clipboard.ts` | raw payload → view state and metadata                           |

Rust payload structs sent to the frontend use `#[serde(rename_all = "camelCase")]`. `ClipboardKind` serializes as `text`, `link`, `image`, or `file`. Text records may also carry optional HTML content (`html_content`/`htmlContent`) for paste-by-format: the CF_HTML fragment on Windows or `public.html` on macOS, capped at 500_000 bytes, `#[serde(default)]` so older records and imports stay compatible. When fields change, update all four layers plus imports/exports and tests.

`icon_path` currently carries an icon file key in the intended frontend path: `ClipboardCard` joins it with `iconsDir`. Do not reintroduce arbitrary absolute icon paths without re-auditing import validation, migration, cleanup, and `convertFileSrc` use.

## SQLite schema

`src-tauri/src/storage/migrations.rs::create_schema` is authoritative.

- `clipboard_items`: ID, kind, title/text/html/resource/preview, `content_hash`, source/icon, size/time, favorite, soft-delete fields, and metadata JSON. `(kind, content_hash)` is unique. The `html_content` column is added idempotently via `ensure_column` for databases created before its introduction.
- `ocr_results`: one row per item, status/engine/model/language/text/blocks/image hash/timestamps/error, with cascade deletion.
- `search_outbox`: ordered `upsert`/`delete` operations populated by clipboard and OCR triggers.

Core invariants:

- Deduplication returns/reuses the database record ID; event producers must emit that saved ID.
- Favorites are protected from normal deletion and history cleanup until explicitly unfavorited.
- Soft-delete and permanent-delete paths must keep OCR, search outbox/index, resource references, and frontend invalidation consistent.
- Binary image/file content lives in managed files; SQLite stores paths and metadata, not blobs.
- Tantivy is derived and rebuildable; SQLite plus owned resource files are the primary data.

Schema changes require schema/migration logic, row mapping, repository tests, recovery/backup consideration, derived-data behavior, and an update to this reference.

## Configuration contracts

### `conf/conf.json`

`src-tauri/src/config.rs` owns `AppConfig` groups: storage, history, privacy, permissions, window, general, export, and OCR. Config structs use `#[serde(default, rename_all = "camelCase")]`; several also flatten unknown fields so newer frontend settings survive round-trips.

`ConfigStore` loads and saves `<project>/conf/conf.json`. Preserve the responsibility boundary: changing the data directory must not move `conf/`.

### General settings

The frontend `GeneralSettings` type/defaults/normalizer are richer than the explicit Rust `GeneralConfig` fields. Extra frontend keys are preserved through Rust's flattened map. Therefore a setting change must be checked in both places even when it appears to survive through `extra`: explicit Rust fields provide typed/defaulted backend behavior; flattened-only keys remain frontend-defined.

`search_index_sync_mode` is an explicit `GeneralConfig` member (values `"lazy"`/`"background"`, default `"lazy"`) with a typed `SearchIndexSyncMode` enum in `config/types.rs`; unknown values parse as `Lazy`. It selects between lazy outbox draining inside `search_clipboard_items` and the startup-created `SearchSyncWorker`, and takes effect on restart.

### Keyboard settings

`src-tauri/src/keyboard/config.rs` and `KeyboardManager` own `<project>/conf/keyboard.json`. Each action maps to an array of shortcut strings. Do not merge keyboard configuration into `GeneralSettings` or `conf.json`.

## Tauri command contract

Commands are registered in `src-tauri/src/lib.rs` with `tauri::generate_handler!`. Frontend wrappers live primarily under `src/lib/services/`.

For each command change:

1. preserve or intentionally update the registered name;
2. align Rust parameter names/types with the frontend invoke object;
3. keep serialized field casing consistent;
4. use `Result<T, String>` for fallible commands so frontend failures are explicit;
5. update wrapper/caller error and null semantics;
6. add or adjust focused tests;
7. update the focused reference rather than copying a full command list here.

A direct `invoke` in a component is still a public cross-layer contract and receives the same audit.

## Event contract

| Event                           | Producer                                                             | Consumer/purpose                                                   |
| ------------------------------- | -------------------------------------------------------------------- | ------------------------------------------------------------------ |
| `clipboard-item-added`          | capture/write backend                                                | main route inserts or replaces the saved record                    |
| `clipboard-history-invalidated` | destructive storage-kind operation; file import (`import_from_file`) | main route removes IDs and resets affected pagination/search state |
| `general-settings-changed`      | authoritative config save                                            | settings stores in other WebviewWindows                            |
| `settings-font-changed`         | font panel                                                           | main route live font/display synchronization                       |
| `tray-open-settings`            | tray backend                                                         | main route opens settings                                          |
| `viewer:open`                   | detail panel                                                         | dedicated viewer window                                            |
| `ppocr-download-progress`       | OCR installer                                                        | settings UI download progress                                      |

Event payloads also use camelCase where Rust structs are serialized. Register listeners before fetching state when an update could occur during hydration, and always retain/unregister the returned unlisten function.

## Resource metadata contract

`content/resource_metadata.rs` and frontend `ResourceMetadata`/`ResourceFileMetadata` carry schema version, MIME/extension, size, storage/original paths, hashes, image dimensions, and per-file entries.

- Treat metadata JSON as untrusted/optional at the frontend boundary.
- Preserve external original paths separately from managed storage paths.
- Rewrite only managed paths during data-directory migration.
- Include every path in multi-file reference accounting so cleanup cannot delete active files.

## Change checklist

- Trace create/read/update/delete/import/export/migrate/cleanup behavior, not just the happy-path UI.
- Search both snake_case storage/Rust names and camelCase payload/config names.
- Verify old configuration/data defaults and unknown-field preservation.
- Keep self-trigger hash registration and capture-side comparison on the same canonical hashing rules.
- Update `settings-reference.md`, `services.md`, `backend-architecture.md`, or `search-cache-strategy.md` when their contract changes.
