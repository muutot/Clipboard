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

`src-tauri/src/storage/schema.rs::initialize` is authoritative. Schema v1 is the clean baseline:
`PRAGMA user_version = 0`/pre-v1 input is transactionally reset only for this redesign. From v1
forward, every schema bump must register exactly one adjacent migration in
`storage/migrations.rs`; the entire chain runs in one transaction, advances `user_version` after
each step, validates foreign keys, and rolls back all steps on failure. A database newer than the
binary's current schema is rejected without modification rather than downgraded or reset.

- `clipboard_items`: ID, kind, title/text/html/rtf/resource/preview, `content_hash`, source/icon, size/time, favorite, soft-delete fields, and metadata JSON. `(kind, content_hash)` is unique. Sync v1 stores `modified_at_ms` plus `sync_writer_device_id`; together they form the deterministic `(timestamp, writer UUID)` conflict version. `last_used_at_ms` and `preview_path` remain device-local: changing either does not create a sync outbox row or advance the replicated record version; wire export clears them as applicable, a remote insert initializes `last_used_at_ms` from `created_at_ms`, and a remote update preserves the receiver's local values. The current schema is created with all columns present; there is no historical column migration.
- `ocr_results`: one row per item, status/engine/model/language/text/blocks/image hash/timestamps/error, with cascade deletion.
- `search_outbox`: ordered `upsert`/`delete` operations populated by clipboard and OCR triggers. Its `sequence` is an `INTEGER PRIMARY KEY` (implicit index), so no redundant sequence index is created.
- `sync_metadata`: key/value store. `device_id` is a random v4 UUID created by `Database::from_connection` and persisted for the lifetime of the local database; any missing or non-UUID value is replaced directly and no historical identity alias is retained. `sync_enabled` gates v1 outbox triggers, while `sync_suppress_changelog` is a transaction-local echo guard. Search triggers do not read the guard, so received items still enter `search_outbox`. Keys below `sync_head_cache:{remote_scope}:{device_id}` are disposable JSON hints containing the last validated head's listing ETag/size/time and logical epoch/snapshot/sequence. They are cleared by first v1 initialization, ignored when malformed, and are never authoritative: skipping a head GET additionally requires an exact match with current `sync_publication_state` or `sync_cursors`.
- `sync_item_aliases`: local convergence map from a remote text or link `alias_id` to the retained local `clipboard_items.id` (foreign key with cascade delete). When an exact id is absent, snapshot/segment apply uses `(kind, content_hash)` once to discover an independently captured equivalent row and persists the alias; subsequent updates/deletes resolve by alias, so text edits remain attached to the same entity even after their content hash changes. Image/file rows do not use this fallback because their capture ids are already content-derived.
- `sync_outbox`: compact local mutation queue containing only sequence, item id, upsert/delete operation, kind/hash and version metadata; it never duplicates text payloads or resource bytes. `get_sync_outbox_batch` reads a bounded sequence range, coalesces repeated ids, and batch-loads their current rows/tombstones. `acknowledge_sync_outbox` deletes only a successfully published prefix while SQLite's AUTOINCREMENT high-water remains the snapshot sequence.
- `sync_tombstones`: one compact winning delete version per id. A soft delete creates/updates it and enqueues the replicated delete; restore removes it. Permanently purging an already-deleted row retains the tombstone without another sync outbox row, preventing local or remote recycle-bin cleanup from echoing the same delete. A direct hard delete of an active row still creates and queues a tombstone. Snapshot export returns all active versioned rows plus these tombstones in deterministic id order.
- `sync_publication_state`, `sync_cursors`, `sync_checkpoint_state`, `sync_checkpoint_cursors`, `sync_resource_scopes`, and `sync_item_resources`: provider-scope-local publication state, per-device applied cursors, the last atomically applied/published global checkpoint generation and digest, its compact baseline vector, and compact per-item remote resource references (`image`, ordered `file`, or `icon`). The scope table stores each remote scope once; the item table stores a 32-byte SHA-256 digest, extension, slot, and file ordinal without duplicating the full S3 key on every row. Incoming packs strip remote keys from the local clipboard row and commit these references in the same transaction as the winning mutation and cursor/checkpoint; normal pull therefore performs no resource download. Scope-aware snapshot/outbox export restores the canonical keys so an unmaterialized item can be forwarded or compacted without a local file. Local resource-path/content changes and deletion clear stale references, while preview-only changes do not. Applying a checkpoint validates unique canonical cursors, applies every winning mutation, replaces both the scope cursor vector and checkpoint baseline, and records generation/digest in one transaction; mutation replay is version-idempotent, while a forced same-generation recovery may deliberately re-establish the checkpoint cursor vector before retrying missing increments. Equal generation with a different digest is rejected, and any mutation failure rolls the whole operation back. The engine uses the baseline vector to skip checkpoint pointer/body I/O on idle runs and schedules compaction for a missing baseline, 50,000 aggregate new-history units, a known-device removal, or an existing-device epoch change/regression. A newly observed device contributes the larger of its trusted bootstrap record count and published sequence; an empty peer therefore does not immediately rewrite the full checkpoint. Each run lists heads once; an unchanged listing ETag/size plus a cache/state-or-cursor match skips the corresponding body GET, while missing listing identity or any mismatch falls back to GET. A local epoch is stable until `reset_sync_remote_state`; bootstrap/segment publication commits the snapshot/segment pointer and acknowledges only the covered outbox prefix in the same transaction. Before either publication path, the engine reconciles the same device's remote head through that safe cache-or-GET path. Missing or divergent state is reconciled into SQLite, then reset to a fresh epoch and republished as a complete snapshot, so a restored database cannot regress the remote head. Bootstrap publication is rejected until the configured remote has been prepared by exact deletion of obsolete objects, and neither publication path may advance past the local AUTOINCREMENT high-water.

`crates/clipboard-sync/src/v1/repository.rs` owns the provider-neutral sync persistence DTOs and `SyncRepository` transaction contract. `storage/sync_repository.rs` is the SQLite adapter: it creates the point-in-time database copy used by streamed export and maps crate-level string errors to `StorageError` only at the database boundary. `Database::initialize_sync` initializes the first and only supported sync protocol. It preserves every `clipboard_items` row as first-snapshot input, assigns missing versions to the current database UUID, rebuilds recycle-bin tombstones, clears only current v1 transient state, and sets `sync_enabled=1`. It does not read or convert baseline/oplog state, and repeated calls are idempotent. Both manual and automatic S3 runs enter it through `sync::v1::sync_database`.

`apply_sync_snapshot` may replace a device epoch; `apply_sync_segment` requires the already-applied epoch and a strictly advancing sequence/key pair. Both validate canonical UUID versions, suppress sync echo, apply the entire upsert/tombstone batch, maintain `sync_item_aliases` and the derived `item_tags` mirror, and advance the per-device cursor in one transaction. Conflict order is lexicographic `(modified_at_ms, writer_device_id)`; equal/older mutations are no-ops, replaying the same segment key returns zero, and any SQL/constraint failure rolls back rows, tombstones, suppression state and cursor together.

Core invariants:

- Deduplication returns/reuses the database record ID; event producers must emit that saved ID.
- Re-capturing content that already exists as a soft-deleted row resurrects that row (`deleted=0`, `deleted_at_ms=NULL`) so the copied item reappears in the active list instead of silently staying hidden.
- Favorites are protected from normal deletion and history cleanup until explicitly unfavorited.
- Soft-delete and permanent-delete paths must keep OCR, search outbox/index, resource references, and frontend invalidation consistent.
- Binary image/file content lives in managed files; SQLite stores paths and metadata, not blobs.
- Tantivy is derived and rebuildable; SQLite plus owned resource files are the primary data.

Schema changes require a deliberate new schema marker, one registered adjacent migration, row mapping, migration and repository tests, recovery/backup consideration, derived-data behavior, and an update to this reference. Pre-v1 historical readers remain intentionally out of scope; that one-time reset boundary must not be reused for changes after v1.

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

Defaults include positional group-switch actions `switchFilter1`..`switchFilter7` bound to `Alt+1`..`Alt+7`. The main route (`+page.svelte`) reads `get_keyboard_config` via `services/keyboard.ts` on mount and on window focus, and resolves each `switchFilter<N>` action to `filters[N-1]` (all, text, link, image, file, favorite, optional deleted); an absent action falls back to `Alt+<N>`, while an action explicitly configured to empty disables that group's shortcut. The shortcut string format is the Rust canonical form (modifier order Ctrl/Alt/Shift/Meta, then key: single-char keys upper-cased, multi-char keys like `Arrowright`/`Space` first-upper rest-lower); `src/lib/utils/keyboard.ts::shortcutMatchesEvent` matches a canonical binding against a keydown event with an exact modifier set. Only the `toggleWindow` action drives OS global hotkey registration.

### Sync settings

`SyncConfig` in `config/types.rs` is a member of `AppConfig.sync` (`#[serde(default, rename_all = "camelCase")]`). `SyncProvider` has only `off` and `s3`. The current fields are endpoint, object prefix (`remote_path`), S3 region/bucket/access key/secret key, optional `sync_password`, last-run status, auto-sync interval, `segment_max_entries`, and image/file byte limits. The default prefix is `clipboard-sync`, the default region is `us-east-1`, and the default segment limit is 512. Removed provider and oplog policy fields are not flattened back into the current config.

`get_sync_config` returns camelCase current settings plus `pendingEntries`, `hasS3SecretKey`, and `hasSyncPassword`; it never returns either secret. `set_sync_config` accepts the same current fields, preserves an already-stored secret when its optional input is omitted, clamps the interval to 10–86400 seconds and the segment limit to 16–10000, and has no historical field aliases. `test_sync_connection` tests the persisted S3 settings and returns a typed camelCase `S3TestResult`. `sync_now` is the only manual run command; its typed `SyncRunResult` includes `failedPeers`. Zero failed peers records `lastSyncStatus = "success"`; one or more isolated remote-head failures records `"partial"` while returning counters for work completed against healthy peers. Remote listing/download/verification/compaction commands are not registered.

`commands/sync/mod.rs` snapshots the config once, releases its mutex, computes a SHA-256 remote scope from the normalized S3 endpoint/region/bucket/prefix/account, derives at most one `SessionKey`, builds one scoped `S3ObjectStore`, and calls `sync::v1::sync_database`. Automatic sync uses the same `run_sync` entry point and global run lock. A successful remote apply emits `clipboard-history-invalidated`; last-run status is persisted after success or failure.

Optional encryption is owned by `crates/clipboard-sync/src/v1/wire.rs`: PBKDF2-HMAC-SHA256 derives a remote-scope-specific AES-256-GCM key once per run. Immutable metadata-object nonces are derived from the authenticated header plus compressed plaintext, making retries byte-identical. Encrypted and plaintext metadata envelopes are mutually exclusive for the configured scope. Before an uninitialized database publishes, the engine authenticates existing canonical device/checkpoint pointers when the v1 namespace is non-empty, so adding, removing, or changing a password cannot publish a mixed namespace. Password rotation is not an in-place setting edit and is currently unsupported: a future dedicated destructive workflow must materialize every referenced resource, delete the old v1 namespace, clear scope-local state, and completely republish. A missing/wrong password, authentication failure, or corruption is a hard error; there is no raw-plaintext fallback.

Sync resource paths are untrusted canonical keys below `v1/resources/{image|file|icon}/`. `crates/clipboard-sync/src/v1/resources.rs` receives only explicit image/file/icon roots from the desktop adapter; it does not know the project, database, search-index, or cleanup layout. Uploads canonicalize regular files below those managed roots, enforce byte limits, hash with streaming reads, and clear unavailable local references rather than publishing machine paths. Without a password the object identity is the plaintext SHA-256. With a password it is `HMAC-SHA256(session_key, plaintext_sha256)`, preserving same-scope/password dedup without exposing the raw hash or reusing keys across password changes. Encrypted resources use the `CLPSYNC1` v1 resource kind without compression: a 20-byte header plus one 16-byte AES-256-GCM tag per non-empty fixed 1 MiB plaintext chunk. Encryption/decryption uses temporary files and one-chunk memory; nonce/AAD bind the canonical key, header and chunk index, and traffic counters use stored ciphertext bytes. Preview images are derived local state: storage export and v1 packs clear `preview_path` and every `previewPath` metadata key and never upload a preview object. Normal snapshot/segment/checkpoint pull only registers remote references and leaves local resource fields absent; no resource `GET` occurs until `materialize_clipboard_item` is invoked for an operation that needs a local path. Materialization verifies a matching existing local path first, coalesces concurrent downloads by remote scope/object key, streams to a bounded ciphertext temporary file, authenticates/decrypts to a plaintext temporary file when needed, verifies the canonical keyed digest, and atomically renames only verified content. It writes all paths for one record back in one changelog-suppressed SQLite transaction while retaining the canonical references. A single-file record keeps `text_content = NULL`; ordered path JSON is only used for multi-file records. Image completion is queued to the local thumbnail worker so previews are rebuilt without sync traffic.

### Sync wire-format evolution rules

The only wire namespace is `v1/`, and every object has the `CLPSYNC1` magic plus format version `1`. `crates/clipboard-sync/src/v1/wire.rs` is authoritative for the protocol-owned `SyncItem`/`SyncItemKind` DTOs, `DeviceHead`, `Segment`, checkpoint records, mutation batches, the compressed envelope, size limits, and exact bincode consumption. `src/sync/v1/mod.rs` is the application adapter between those DTOs and the desktop `ClipboardItem`/`ClipboardKind`; a byte-for-byte compatibility test locks the original v1 positional layout.

- There is one canonical layout per object kind and no reader fallback chain.
- The decoder rejects unknown flags, wrong kinds/versions, trailing bytes, oversized stored or expanded payloads, authentication failure, and malformed identifiers.
- Because bincode is positional, an incompatible protocol DTO change requires an intentional new protocol namespace; application-domain fields may evolve independently only through explicit adapter logic that leaves the v1 DTO layout unchanged. Do not silently reinterpret v1 objects or add historical-layout decoders.
- Snapshot/checkpoint v1 objects use one authenticated internal chunk stream: a fixed-int identity header plus deterministic mutation batches (target 2,048 records, hard 16 MiB decoded chunk cap), each independently bincode+zstd encoded and optionally AES-256-GCM authenticated. They are written/read through temporary files with one S3 PUT/GET, while segment and pointer objects retain the small single-envelope layout. Applying all chunks and the final cursor/checkpoint marker remains one SQLite transaction, so any late failure rolls back every earlier chunk.
- Re-run the v1 wire round-trip, corruption, wrong-password, retry-determinism, and size-bound tests for every wire change.

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

`materialize_clipboard_item(id)` returns the refreshed camelCase `PersistedClipboardItem`. It is a local-cache mutation: it must not advance `(modified_at_ms, sync_writer_device_id)` or create a sync outbox row, and any stale/missing reference causes the whole path write-back to remain unchanged.

A direct `invoke` in a component is still a public cross-layer contract and receives the same audit.

## Event contract

| Event                           | Producer                                                                                           | Consumer/purpose                                                                                                                       |
| ------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `clipboard-item-added`          | capture/write backend                                                                              | main route inserts or replaces the saved record; settings window refreshes storage stats                                               |
| `clipboard-history-invalidated` | destructive storage-kind operation; file import (`import_from_file`); v1 remote apply (`sync_now`) | main route removes IDs and resets affected pagination/search state; settings window refreshes storage stats                            |
| `general-settings-changed`      | authoritative config save                                                                          | settings stores in other WebviewWindows                                                                                                |
| `settings-font-changed`         | font panel                                                                                         | main route live font/display synchronization                                                                                           |
| `tags-changed`                  | tag management panel, or main-window `TagEditDialog` (rename/delete/color)                         | main route refreshes tag colors and rewrites item tags/filter; settings tag panel refreshes its list (skipping self-originated events) |
| `tray-open-settings`            | tray backend                                                                                       | main route opens settings                                                                                                              |
| `viewer:open`                   | detail panel                                                                                       | dedicated viewer window                                                                                                                |
| `ppocr-download-progress`       | OCR installer                                                                                      | settings UI download progress                                                                                                          |
| `privacy-pause-changed`         | tray pause toggle, or `toggle_privacy_pause` command                                               | settings `GeneralSettingsPanel` and tray menu item refresh recording pause state                                                       |

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

Tag management (settings) is backed by a `tags` registry table (`name TEXT PRIMARY KEY, color TEXT NOT NULL DEFAULT ''`) defined in `schema.rs`. Item membership remains strings under `metadata_json.tags`; the table only carries global presentation metadata. `ClipboardRepository::list_all_tags` scans active records' `tags` arrays for distinct names with usage counts and joins each with its registry color. `rename_tag(old, new)` rewrites every active record's tag array (de-duplicating, preserving order) and migrates the registry color; `delete_tag(name)` removes it from records and deletes the registry row; `set_tag_color(name, color)` upserts the registry, accepting only empty or `#RRGGBB` hex. Each metadata rewrite is one transaction and is picked up by the `clipboard_items` search update trigger. Commands `list_all_tags`, `rename_tag`, `delete_tag`, and `set_tag_color` are registered in `lib.rs`; `TagInfo { name, count, color }` is serde-serialized for IPC. `rename_tag`/`delete_tag` return the number of records changed; `set_tag_color`/`set_clipboard_item_tags` return a boolean.

Tag management runs in the separate settings WebviewWindow, so the tag panel emits the `tags-changed` event (payload `TagsChangedPayload` in `types/clipboard.ts`: `{ renamed?: { old, new }; deleted?: string }`, empty for color-only changes) after every successful rename/delete/color operation. The main-window `TagEditDialog` (opened by right-clicking a tag chip on a card) uses the same commands and emits the same `tags-changed` payload, so both producers share the same reconciliation path. The main route listens and refreshes `tagColors`, rewrites the in-memory `tags` arrays of `items`/`indexedItems`/`detailItem` (deduplicating on rename), and re-points or clears an active `tagFilter` accordingly.

## Change checklist

- Trace create/read/update/delete/import/export/migrate/cleanup behavior, not just the happy-path UI.
- Search both snake_case storage/Rust names and camelCase payload/config names.
- Verify old configuration/data defaults and unknown-field preservation.
- Keep self-trigger hash registration and capture-side comparison on the same canonical hashing rules.
- Update `settings-reference.md`, `services.md`, `backend-architecture.md`, or `search-cache-strategy.md` when their contract changes.
