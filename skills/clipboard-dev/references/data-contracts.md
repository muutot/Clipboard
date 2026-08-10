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

Rust payload structs sent to the frontend use `#[serde(rename_all = "camelCase")]`. `ClipboardKind` serializes as `text`, `link`, `image`, or `file`. Text records may also carry optional rich-text fragments for paste-by-format: `html_content`/`htmlContent` (the CF_HTML fragment on Windows or `public.html` on macOS) and `rtf_content`/`rtfContent` (the registered `Rich Text Format` payload on Windows; not yet captured on macOS/Linux). Both are capped at the configurable `maxTextCaptureBytes` (default 500_000; see `settings-reference.md`) and `#[serde(default)]` so older records and imports stay compatible. `writeClipboardHtml(html, plainText?, rtf?)` writes `text/html` plus optional `text/plain` and `text/rtf`, so office suites that prefer RTF keep formatted paste. When fields change, update all four layers plus imports/exports and tests.

### Active-history listing filters

`list_clipboard_items` paginates active history with an optional `filter` argument (camelCase `HistoryFilterArgs` in `commands/clipboard/types.rs`, mirroring `HistoryFilter` in the storage layer): `kind` (`text`/`link`/`image`/`file`), `favorite`, `tag`, `sourceApp`, `dateFromMs`, `dateToMs`. All fields are optional; an omitted or empty payload returns unfiltered pages. Each filter is applied in the `list_recent` SQL `WHERE` clause (`json_each(metadata_json, '$.tags')` matches tags, tolerating `NULL` metadata) so every page returns the latest matching records rather than filtering a loaded set. The main route builds this payload from the active kind tab, tag/source/date dropdowns and resets history pagination when any filter changes; `filteredItems` still re-applies the same predicates client-side.

`icon_path` currently carries an icon file key in the intended frontend path: `ClipboardCard` joins it with `iconsDir`. Do not reintroduce arbitrary absolute icon paths without re-auditing import validation, migration, cleanup, and `convertFileSrc` use.

### Import summary and truncation warning

`ImportSummary` (used by `import_from_file` and `import_clipboard_items`) carries `importedCount`, `skippedCount`, `errors`, plus `pendingTruncation` and `maxItems` (both `#[serde(default)]`; default 0). The file import command `import_from_file` takes `config` and `paths` state, reads `max_items`, and reports `pendingTruncation = active_count.saturating_sub(max_items)` after importing so the settings UI can warn that the oldest non-favorite items will be removed by the next scheduled capacity cleanup. Capacity is enforced by the history-cleanup worker (`enforce_capacity_limit`), not by the import itself; the import only reports the risk.

### PPaste backup import

`import_from_file` routes `.Pastebackup` files (a ZIP) to `export::ppaste::import_from_ppaste_backup` (see `src-tauri/src/export/ppaste.rs`). It reads the embedded `PPaste2.db3` plus `PasteData/*.png`, maps `PPaste_Main` rows into `ClipboardItem` (Text/UnicodeText/HtmlText/RtfText/Json/Color → `text`; Links → `link`; Image → `image` with PNG bytes written to the managed images dir and inline image `metadata_json`), and persists via `ClipboardRepository::save_item` so the `(kind, content_hash)` dedup and search outbox triggers apply. Non-portable `Files` rows (absolute-path references to the source machine) are skipped and counted in `skipped_count`, with a single friendly error note explaining that their content is not included in the backup. Timestamps parse PPaste's local-time string as UTC, preserving relative order. The `zip` crate is a new dependency. The import dialog accepts the `.pastebackup` extension; `BACKUP_EXTENSION` is exported from `export::mod`.

PPaste backups made on Windows store zip entry names with backslashes (e.g. `PasteData\1745.png`); `read_zip_entry` tries both `/` and `\` separators so images are found regardless of the archive's separator convention. Each entry is read with a decompressed-size cap (`MAX_PPASTE_ENTRY_BYTES`) so a crafted archive cannot force unbounded allocation. PPaste datetimes are parsed as UTC with range validation (fields and a sane year range) and saturating arithmetic, so malformed or adversarial timestamps are rejected or clamped instead of wrapping.

Duplicate imports are not double-counted: before persisting each row, `import_rows` calls `ClipboardRepository::content_exists(kind, content_hash)` (matches the `UNIQUE(kind, content_hash)` upsert key, includes soft-deleted rows) and counts an existing row as skipped instead of re-upserting. `content_exists` is only used by the PPaste import path; `save_item` still returns the record id on both insert and upsert for other callers.

## SQLite schema

`src-tauri/src/storage/migrations.rs::create_schema` is authoritative.

- `clipboard_items`: ID, kind, title/text/html/rtf/resource/preview, `content_hash`, source/icon, size/time, favorite, soft-delete fields, and metadata JSON. `(kind, content_hash)` is unique. The `html_content` and `rtf_content` columns are added idempotently via `ensure_column` for databases created before their introduction.
- `ocr_results`: one row per item, status/engine/model/language/text/blocks/image hash/timestamps/error, with cascade deletion.
- `search_outbox`: ordered `upsert`/`delete` operations populated by clipboard and OCR triggers. Its `sequence` is an `INTEGER PRIMARY KEY` (implicit index); the historical redundant `search_outbox_sequence_idx` is dropped via `DROP INDEX IF EXISTS` on every open.
- `sync_metadata`: key/value store. `device_id` is a random v4 UUID created by `Database::from_connection` and persisted for the lifetime of the local database; a pre-UUID hostname value is migrated once to a new UUID and retained as `legacy_device_id` solely for recognizing already-uploaded local oplogs. Empty/`unknown`/`unknown-device` fallbacks are never retained as aliases. The store also carries the `sync_suppress_changelog` flag used to silence the sync triggers. `sync_changelog` is populated by the three `clipboard_items_sync_*` triggers; each is guarded with `WHEN NOT EXISTS (SELECT 1 FROM sync_metadata WHERE key='sync_suppress_changelog' AND value='1')`. The three sync triggers are dropped and recreated on every open (so older databases gain the guard). Any code path writing remote data locally (`apply_remote_oplog`, `import_baseline_items`) must set the flag inside the same transaction and clear it before commit via `Database::set_changelog_suppressed`; otherwise received entries echo back into the changelog and are re-broadcast. The search triggers do not read the flag, so received items still enter `search_outbox`.
- `sync_remote_state`: provider/endpoint/path/account-scoped key/value state. The `initialized` flag determines whether that specific remote needs baseline bootstrap; `remote_oplog_count` is observation only, while `remote_baseline_modified_ms` drives the missing/stale snapshot-refresh suggestion. Never use global `lastSyncMs` as a remote progress cursor.
- `sync_applied_oplogs`: one row per successfully processed immutable remote oplog object and remote scope (`remote_scope`, `object_name`, `revision`, `applied_at_ms`). Legacy timestamp-named oplogs are intentionally not skipped from metadata because older clients may overwrite them.

Core invariants:

- Deduplication returns/reuses the database record ID; event producers must emit that saved ID.
- Re-capturing content that already exists as a soft-deleted row resurrects that row (`deleted=0`, `deleted_at_ms=NULL`) so the copied item reappears in the active list instead of silently staying hidden.
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

`searchPlaceholder` is a flattened-only frontend string (default `""`, trimmed to 80 chars). It is not an explicit `GeneralConfig` member: the main search box uses `searchPlaceholder.trim() || localized app.searchPlaceholder` as its `placeholder`/`aria-label`, so an empty value falls back to the language-aware default. No backend change is needed for it.

`colorIcons` is a flattened-only frontend boolean (default `false`) consumed solely by `AppIcon.svelte` (renders per-icon colors when enabled). `iconColors` is a flattened-only frontend `IconColors` map (`Partial<Record<IconName, string>>`, default `{}`) holding optional per-icon hex overrides; `AppIcon.svelte` resolves `iconColors[name] ?? DEFAULT_ICON_COLORS[name]` (the built-in palette from `src/lib/types/clipboard.ts`) while `colorIcons` is enabled, otherwise it falls back to `currentColor`. Neither key has an explicit Rust member or backend behavior; no Rust change is needed for them.

`search_index_sync_mode` is an explicit `GeneralConfig` member (values `"lazy"`/`"background"`, default `"lazy"`) with a typed `SearchIndexSyncMode` enum in `config/types.rs`; unknown values parse as `Lazy`. It selects between lazy outbox draining inside `search_clipboard_items` and the startup-created `SearchSyncWorker`, and takes effect on restart.

`update_source` is an explicit `GeneralConfig` member (values `"github"`/`"gitcode"`, default `"gitcode"`) with a typed `UpdateSource` enum in `config/types.rs`; unknown values parse as `Gitcode`. The About-panel update check (`check_for_update` in `commands/update.rs`) reads `ConfigStore::update_source()` at call time to pick the latest-release API and release page URL, so the dropdown takes effect without a restart.

`max_text_capture_bytes` is an explicit `GeneralConfig` member (`u64` bytes, default `500_000`); `ConfigStore::max_text_capture_bytes()` clamps to 10000–10000000. The capture loop reads the value per iteration from `CaptureState` (an `Arc<AtomicU64>` seeded at startup and pushed by `set_general_settings`), so it caps plain-text, HTML, and RTF captures live without a restart.

### Keyboard settings

`src-tauri/src/keyboard/config.rs` and `KeyboardManager` own `<project>/conf/keyboard.json`. Each action maps to an array of shortcut strings. Do not merge keyboard configuration into `GeneralSettings` or `conf.json`.

### Sync settings

`SyncConfig` in `config/types.rs` is a member of `AppConfig.sync` (`#[serde(default, rename_all = "camelCase")]`), with `SyncProvider` enum (`off`/`webdav`/`s3`, serialized lowercase). It holds provider, endpoint, remote path, username/password, the S3 fields (`s3_region`, `s3_bucket`, `s3_access_key`, `s3_secret_key`), the optional encryption `sync_password`, timestamps/status, and the sync policy numbers (auto-sync interval, max remote oplog files, rollover limits, max image/file bytes). Secrets are write-only: `get_sync_config` returns `hasS3SecretKey`, `hasSyncPassword` (and `hasPassword`), never the raw values.

Remote payloads are encrypted with `sync::crypto` when `syncPassword` is set: AES-256-GCM with a PBKDF2-HMAC-SHA256 key (fixed salt, 100k iterations), nonce prefixed to the ciphertext (`encrypt`/`decrypt`). `encrypt_if_configured`/`decrypt_if_configured` in `commands/sync/mod.rs` apply the password only when configured; on decryption failure the payload falls back to the raw bytes (so files uploaded before a password was set remain readable) **and** logs an explicit `[sync] warning: ...` line so a changed/mismatched password is visible instead of silently skipping the payload.

`set_sync_config` accepts `provider`, `endpoint`, `remotePath`, `username`, `password`, `autoSync`, `autoSyncIntervalSecs`, `maxRemoteOplogFiles`, `oplogRolloverEntries`, `oplogRolloverSizeBytes`, `maxSyncImageBytes`, `maxSyncFileBytes`, `s3Region`, `s3Bucket`, `s3AccessKey`, `s3SecretKey`, and `syncPassword` (all optional where applicable); the three write-only secrets (`password`, `s3SecretKey`, `syncPassword`) fall back to their previously stored values when omitted/null so the UI never needs to send secrets back. `get_sync_config` returns the full effective sync policy plus `hasPassword` / `hasS3SecretKey` / `hasSyncPassword`, never the raw secrets. Its remote maintenance observations are read from the current SHA-256 remote scope in `sync_remote_state`, not from global timestamps. The compatibility field `compactionSuggested` now means “snapshot refresh suggested”: it is true only for an initialized remote whose newest baseline timestamp is missing or at least 30 days old; oplog count does not trigger it. `maxRemoteOplogFiles` is retained in config but not enforced until a per-device acknowledgement protocol can prove deletion safe. `sync_list_remote_backups` returns a directly serialized array of camelCase entries (`name`, `isDirectory`, `sizeBytes`, `modifiedMs`), not a JSON string containing that array. The compatibility-named `sync_compact_remote` command performs a full sync and appends a fresh baseline without deleting any existing baseline or oplog; the frontend exposes it as `syncRefreshRemoteSnapshot`. `test_sync_connection` takes the same provider/endpoint/remote-path/credential inputs and returns a serialized `WebDavTestResult`/`S3TestResult` JSON string. New remote oplogs are immutable objects named `oplog-{device_id}-s{first_sequence}-e{last_sequence}-{sha256}` (no extension) and hold `Oplog` envelopes serialized via `sync::wire::serialize_oplog_with_resources` (bincode v2); `device_id` is the database-persisted UUID, while an optional `legacy_device_id` is consulted only to skip objects uploaded by this database before the hostname-to-UUID migration. Older timestamp-named objects remain readable and are reprocessed each run rather than trusted as immutable. Baseline ZIPs embed `baseline.bin` plus a human-readable `manifest.json`; downloaded archive bytes are unpacked by `read_baseline_archive_bytes`/`merge_baseline_archives` before the contained bincode payload reaches `deserialize_baseline_with_resources`. Pool manifests are stored per remote scope, so `sync::pool::prepare_pool_refs` only downgrades bytes confirmed on that same remote; `materialize_resources` fetches pool references through an optional `Pool` callback; `merge_baseline_contents` prefers inline bytes over pool references so merged payloads stay self-sufficient. `deserialize_baseline_with_resources`/`deserialize_oplog_with_resources` decode the single canonical wire envelope each (there is no V1/V2/V3 fallback). Image and file ids are content-derived (`img_{hash}`, `file_{hash}`, `files_{group_hash}`), but text ids are currently timestamp-suffixed (`{text_hash}_{ms}`), so independently captured identical text does not yet converge to one entity. `apply_remote_oplog` resolves conflicts last-write-wins on `modified_at_ms` when ids match (update/delete only apply when the remote entry is at least as new as the local row; insert is `INSERT OR IGNORE`). `import_baseline_items` also guards its `ON CONFLICT(id) DO UPDATE` with `WHERE excluded.modified_at_ms >= clipboard_items.modified_at_ms`, so a stale baseline snapshot never clobbers a newer local edit (the `clipboard_items_set_modified` trigger bumps `modified_at_ms` on any update). See `commands/sync/mod.rs` and the `sync/` module in `backend-architecture.md` for transport behavior.

### Sync wire-format evolution rules

The sync payloads are **bincode v2**, which is a positionally-encoded format with no field names or tags. This constrains how the on-disk/remote format may evolve:

- **Append-only fields.** New fields must be added at the **end** of the struct and typed `Option<T>` (or carry a bincode default) so data written without them still decodes. Never insert or reorder fields in the middle; never change an existing field's type (`i64`→`u64`, `String`→`Vec<u8>` break decoding).
- **Single canonical layout; no version fallback.** There is exactly one `Baseline` and one `Oplog` wire layout, and `deserialize_baseline_with_resources`/`deserialize_oplog_with_resources` decode only that canonical envelope. The per-version `BaselineV1/V2` and `OplogV2/V3` structs and their newest-first fallback chains were removed; do not reintroduce parallel version structs. `Baseline` carries a `format_version: u32` tag so an incompatible payload is detected before decoding proceeds.
- **Coordinated upgrades.** Because bincode readers cannot skip unknown fields, a protocol change requires all syncing clients to upgrade together. There is no cross-version coexistence; if old + new clients must read the same remote data simultaneously, switch the wire format to protobuf (unknown-field skipping) instead of extending bincode.
- **Wire-format authority is `sync/wire.rs`.** The wire formats live in `src-tauri/src/sync/wire.rs` and its tests; there is no `.proto` file. Keep the structs append-only and re-run the bincode round-trip tests when they change.

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

| Event                           | Producer                                                                                                               | Consumer/purpose                                                                                                                       |
| ------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `clipboard-item-added`          | capture/write backend                                                                                                  | main route inserts or replaces the saved record; settings window refreshes storage stats                                               |
| `clipboard-history-invalidated` | destructive storage-kind operation; file import (`import_from_file`); sync apply of remote data (`sync_upload_backup`) | main route removes IDs and resets affected pagination/search state; settings window refreshes storage stats                            |
| `general-settings-changed`      | authoritative config save                                                                                              | settings stores in other WebviewWindows                                                                                                |
| `settings-font-changed`         | font panel                                                                                                             | main route live font/display synchronization                                                                                           |
| `tags-changed`                  | tag management panel, or main-window `TagEditDialog` (rename/delete/color)                                             | main route refreshes tag colors and rewrites item tags/filter; settings tag panel refreshes its list (skipping self-originated events) |
| `tray-open-settings`            | tray backend                                                                                                           | main route opens settings                                                                                                              |
| `viewer:open`                   | detail panel                                                                                                           | dedicated viewer window                                                                                                                |
| `ppocr-download-progress`       | OCR installer                                                                                                          | settings UI download progress                                                                                                          |
| `privacy-pause-changed`         | tray pause toggle, or `toggle_privacy_pause` command                                                                   | settings `GeneralSettingsPanel` and tray menu item refresh recording pause state                                                       |

Event payloads also use camelCase where Rust structs are serialized. Register listeners before fetching state when an update could occur during hydration, and always retain/unregister the returned unlisten function.

## Resource metadata contract

`content/resource_metadata.rs` and frontend `ResourceMetadata`/`ResourceFileMetadata` carry schema version, MIME/extension, size, storage/original paths, hashes, image dimensions, and per-file entries.

- Treat metadata JSON as untrusted/optional at the frontend boundary.
- Preserve external original paths separately from managed storage paths.
- Rewrite only managed paths during data-directory migration.
- Include every path in multi-file reference accounting so cleanup cannot delete active files.
- Resource files are content-hash-named and may be shared by several records (dedup, duplicates). `rename_item` renames the physical file only when the record is its sole owner (`resource_reference_count(path, id) == 0`); a shared file keeps its hash name and only the display title changes, so renaming one record never breaks another.

## Tags

Tags are stored as an array of strings under `metadata_json.tags` (no dedicated column or schema migration; the `metadata_json` column already exists and defaults to `{}`). `ClipboardRepository::set_tags` reads the current object, trims each tag, deduplicates (preserving order), writes the array, or removes the `tags` key entirely when the resulting list is empty; a missing record is a no-op returning `false`.

The command `set_clipboard_item_tags` (`id: String, tags: Vec<String>`) persists a full replacement of the item's tag list and is registered in `lib.rs`. Because tags are folded into the search document content (`search_repository.rs::SearchDocument::from_row` reads `metadata_json.get("tags")`), setting tags invalidates the derived search content for that item. The frontend view type exposes `ClipboardItem.tags?: string[]` (parsed in `toClipboardItem`), and the `persistTags` wrapper in `services/clipboard.ts` invokes the command. A tag click on a card chip or a detail-panel edit calls `persistTags`; the list routes apply an optional `tagFilter` to `filteredItems`.

Tag management (settings) is backed by a `tags` registry table (`name TEXT PRIMARY KEY, color TEXT NOT NULL DEFAULT ''`) created in `migrations.rs`. Item membership remains strings under `metadata_json.tags`; the table only carries global presentation metadata. `ClipboardRepository::list_all_tags` scans active records' `tags` arrays for distinct names with usage counts and joins each with its registry color. `rename_tag(old, new)` rewrites every active record's tag array (de-duplicating, preserving order) and migrates the registry color; `delete_tag(name)` removes it from records and deletes the registry row; `set_tag_color(name, color)` upserts the registry, accepting only empty or `#RRGGBB` hex. Each metadata rewrite is one transaction and is picked up by the `clipboard_items` search update trigger. Commands `list_all_tags`, `rename_tag`, `delete_tag`, and `set_tag_color` are registered in `lib.rs`; `TagInfo { name, count, color }` is serde-serialized for IPC. `rename_tag`/`delete_tag` return the number of records changed; `set_tag_color`/`set_clipboard_item_tags` return a boolean.

Tag management runs in the separate settings WebviewWindow, so the tag panel emits the `tags-changed` event (payload `TagsChangedPayload` in `types/clipboard.ts`: `{ renamed?: { old, new }; deleted?: string }`, empty for color-only changes) after every successful rename/delete/color operation. The main-window `TagEditDialog` (opened by right-clicking a tag chip on a card) uses the same commands and emits the same `tags-changed` payload, so both producers share the same reconciliation path. The main route listens and refreshes `tagColors`, rewrites the in-memory `tags` arrays of `items`/`indexedItems`/`detailItem` (deduplicating on rename), and re-points or clears an active `tagFilter` accordingly.

## Change checklist

- Trace create/read/update/delete/import/export/migrate/cleanup behavior, not just the happy-path UI.
- Search both snake_case storage/Rust names and camelCase payload/config names.
- Verify old configuration/data defaults and unknown-field preservation.
- Keep self-trigger hash registration and capture-side comparison on the same canonical hashing rules.
- Update `settings-reference.md`, `services.md`, `backend-architecture.md`, or `search-cache-strategy.md` when their contract changes.
