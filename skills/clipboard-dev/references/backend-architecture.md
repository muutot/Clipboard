# Backend Architecture

## Contents

- [Runtime composition](#runtime-composition)
- [Managed runtime state](#managed-runtime-state)
- [Database and repositories](#database-and-repositories)
- [Storage paths and ownership](#storage-paths-and-ownership)
- [Search](#search)
- [OCR](#ocr)
- [Capture, content, and self-trigger suppression](#capture-content-and-self-trigger-suppression)
- [Privacy and cleanup](#privacy-and-cleanup)
- [Platform adapters](#platform-adapters)
- [CLI and loopback API](#cli-and-loopback-api)
- [Unified shutdown](#unified-shutdown)
- [Backend change checklist](#backend-change-checklist)

## Runtime composition

`src-tauri/src/lib.rs::run` is the GUI composition root. The current startup order is safety-sensitive:

1. load `ConfigStore` beside the executable/project;
2. acquire the optional single-instance guard and wake listener;
3. load keyboard configuration;
4. resolve `StoragePaths` without auto-claiming arbitrary custom roots;
5. recover/validate the database, quarantine a stale search index after recovery, refresh backups, and requeue interrupted OCR;
6. open/validate/synchronize Tantivy;
7. record startup metrics;
8. choose PP-OCR, Tesseract, fallback Tesseract, or no-op OCR and start the restartable worker;
9. start thumbnail, privacy/capture, clipboard-monitor, capture-ingestion, cleanup, tray, and hotkey services;
10. manage state, apply startup window configuration, register commands, and enter the Tauri event loop.

Do not reorder recovery, search initialization, worker startup, or managed-state installation casually. A startup change needs failure-path and shutdown review.

## Managed runtime state

The app manages focused state objects including `ConfigStore`, `StoragePaths`, `Database`, `SearchIndex`, `PerformanceTracker`, privacy/capture/self-trigger state, clipboard monitor, shortcut/keyboard/hotkey managers, OCR and thumbnail workers, cleanup worker, local API server, and the single-instance guard when enabled.

Use `Mutex`/`Arc` according to existing ownership. Avoid holding a config or ingestion lock across slow filesystem, network, or UI operations unless atomicity requires it.

## Database and repositories

The implementation lives under `src-tauri/src/storage/`.

### Active runtime path

`Database` wraps a rusqlite connection configured for foreign keys, WAL, normal synchronous mode, memory temp storage, cache, and mmap. Repository traits/implementations cover clipboard, OCR, and search-outbox behavior.

### Pool status

`storage/pool.rs` no longer contains a connection pool: the unused `DatabasePool`/`PooledConnection` types and the read-only/estimate helpers were removed as dead code. The file now holds only the live `Database::repair`/`RepairResult` integrity check and `Database::set_preview_path`. The GUI runtime and workers each manage `Database` directly; a shared pool is not part of the runtime path.

### Recovery and backups

`storage/recovery.rs` validates with SQLite integrity checks, rotates current/previous backups, quarantines damaged files, restores the first valid backup, and causes the derived search index to be quarantined/rebuilt after recovery.

A persistence change must preserve atomic config writes, SQLite recovery, backup refresh, and the ability to rebuild derived data.

## Sync and remote backup

`src-tauri/src/sync/` owns cross-device synchronization and remote backup:

- `backup.rs` builds a baseline ZIP (`baseline-{device_id}-{timestamp}.zip`) from the clipboard database via `create_baseline_backup`, reads it back with `read_baseline_items`/`read_baseline_with_resources`, and produces a `BackupManifest` through `read_manifest_from_backup`. `create_baseline_backup` takes an optional `PoolStorage`; when supplied it is passed to `pool::prepare_pool_refs` so resources already confirmed in the remote pool are written as references (`bytes: None`) instead of inline copies (see the pool bullet below). `BackupManifest::total_resource_bytes` counts only inline bytes, so pooled references don't skew the stat. `create_oplog_backup`/`mark_oplog_synced`/`purge_oplog`/`count_unsynced` manage the outbox so only unsynced changes are uploaded next run. Since item ids are content-derived (`img_{hash}`, `file_{hash}`, `files_{group_hash}`, `{text_hash}_{ms}`), `import_baseline_items` uses last-write-wins (`WHERE excluded.modified_at_ms >= clipboard_items.modified_at_ms`) so a stale baseline never overwrites a newer local edit; subsequent conflicts converge through `apply_remote_oplog`'s LMW.
- `wire.rs` wraps the wire formats: `Baseline` (format_version, timestamps, device id, items plus `OplogResource`s whose `bytes` may be `Some` inline or `None` for a pool reference) and `Oplog` (entries plus the same `OplogResource`s) are bincode v2-encoded via `serialize_baseline_with_resources`/`serialize_oplog_with_resources`; there is exactly one canonical layout per envelope (no V1/V2/V3 fallback). `merge_baselines` unions several disjoint baseline payloads (items by id keeping the newer `created_at_ms`, resources by wire path) — multiple baselines arise only from concurrent first syncs and have no common root, so they are merged into a superset rather than dropped; when payloads disagree on a resource, inline bytes are preferred over a pool reference so consolidated payloads stay self-sufficient. `write_baseline_zip` writes a baseline ZIP from already-serialized wire-form items + resources (used by the consolidation path). Despite the old module name, the on-wire format is bincode, not protobuf; there is no `.proto` file. Zip entries are named `baseline.bin`/`oplog.bin`; there is no JSON serialization of oplog payloads. Rollover merges parse existing remote oplogs with `deserialize_oplog_with_resources`.
- `resources.rs` rewrites entry and item paths between local and portable wire form (`category/relative`: `image/`, `file/`, `preview/`, `icon/`) and collects the referenced file bytes into `OplogResource`s. `collect_entry_resources` (oplog upload side) and `collect_item_resources` (baseline side) read bytes and rewrite to wire paths; `rewrite_to_local`/`rewrite_item_paths_to_local` (download side) restore absolute paths before `apply_remote_oplog`/`import_baseline_items`. `materialize_resources` (download side) writes inline bytes into the receiver's storage subdirectories and, for `bytes: None` pool references, fetches the object from the pool through an optional `Pool` closure when the file is missing locally (skipping it when no pool is available). Files that cannot be read are cleared from the entry; resources dedupe by wire path (an image whose preview equals its resource is transferred once).
- `webdav.rs` and `s3.rs` are the two remote transports, exposed through `sync::upload_to_webdav`, `download_from_webdav`, `list_webdav_files`, `delete_from_webdav`, `test_webdav_connection` (and the S3 analogues). Both reuse a shared `OnceLock` reqwest client (timeout only); WebDAV attaches Basic auth per request via `basic_auth`, S3 signs every request with AWS SigV4 (`SigV4` in `s3.rs`, verified against the official test-suite vectors). For S3 the endpoint string is parsed into scheme/host (accepts bare host, `http://`, `https://`; defaults to https, custom ports kept in the `host` header), and path-style URLs are built as `{scheme}://{host}/{bucket}/{key}`. Requests are described by the `S3Request` struct and signed/sent through `signed_request`, with listings using `list-type=2` and `prefix` percent-encoded. This is compatible with S3-compatible object stores (e.g. MinIO); the region/bucket/access key/secret key come from `ConfigStore` (`s3_region()` defaults to `us-east-1`).
- `pool.rs` is the remote resource pool. Resource files are stored as standalone objects at `resources/<rel_path>` in the same remote layout instead of always being embedded inside payloads, so a compacted baseline can reference already-uploaded files (`bytes: None`) instead of re-transferring every file. `PoolStorage` abstracts the transport (`upload`/`download`); `WebDavPool`/`S3Pool` in `commands/sync/mod.rs` implement it on top of the two providers, applying the sync password encryption per object. A per-device on-disk manifest (`sync-pool-manifest.json`, `load_pool_manifest`/`save_pool_manifest`) records which `rel_path`s this device knows exist in the pool. `ensure_pool_uploads` uploads not-yet-confirmed inline bytes and records them; `mark_pool_references` downgrades confirmed resources to `None`; `prepare_pool_refs` combines both (upload new, downgrade already-known) and is used by `create_baseline_backup` and the multi-baseline consolidation path. `absorb_pool_paths` learns `bytes: None` references from downloaded payloads so this device's later compaction reuses them. The manifest is only an optimization: payloads keep inline `bytes` for every resource not confirmed in the pool, so a downgrade never loses data and pool upload failures are logged and skipped (the payload keeps its inline copy).
- `crypto.rs` provides optional AES-256-GCM encryption of remote payloads (`encrypt`/`decrypt`) keyed by PBKDF2-HMAC-SHA256 over `syncPassword` (fixed salt `clipboard-sync-salt-v1`, 100k iterations, 12-byte random nonce prefixed to the ciphertext). `commands/sync/mod.rs` gates it through `encrypt_if_configured`/`decrypt_if_configured`, which are no-ops without a configured password and fall back to the raw bytes on decryption failure — logging an explicit warning so a changed/mismatched password is visible instead of silently dropping the payload.
- `commands/sync/mod.rs` orchestrates: on the very first sync it uploads a fresh baseline when the remote has none, or when the remote already has baselines it downloads **every** remote baseline, merges them via `merge_baselines`, absorbs their pool references (`pool::absorb_pool_paths`), materializes them with a pool fetch closure, and writes a consolidated superset baseline (self-heal: the multiple disjoint baselines are replaced by one for future devices; the consolidated resources run through `pool::prepare_pool_refs` first so already-pooled files are referenced instead of written inline again), then imports the merged items; afterwards it merges/rolls over remote oplogs (collecting resource bytes for new local entries and re-serializing with the combined resource set), materializes downloaded resources (pool-aware) and applies remote entries via `database.apply_remote_oplog`, and prunes old remote oplog files. An applied-oplog mtime watermark (`sync_metadata` key `sync_applied_oplog_watermark_ms`, `Database::get/set_sync_applied_oplog_watermark`) records the highest `modified_ms` among successfully-applied remote oplogs; later syncs skip remote oplogs whose mtime is at or below the watermark, so previously-applied files are not downloaded/parsed again. Sync commands snapshot all needed config once into a `SyncSettings` struct under the `ConfigStore` lock and release the lock before any network I/O (re-acquiring only to record `update_sync_status`), so a slow remote never blocks other config access. `sync_upload_backup` guards the non-reentrant merge/apply/cleanup with a module-level `SYNC_RUN_LOCK: Mutex<()>` (`try_lock`, fail-fast "sync already in progress"), serializing manual syncs and the auto-sync worker. `sync_download_backup` fetches a remote backup file for local restore; `verify_backup_file` validates its manifest. Manual compaction via `sync_compact_remote` first runs a full `run_sync` (so the local DB is a superset of every remote device), then re-acquires `SYNC_RUN_LOCK`, writes a fresh full baseline through the shared baseline-with-resources writer, uploads it, and deletes every older baseline plus the older oplogs that are fully covered by the fresh snapshot (oplogs trimmed to the `max_remote_oplog_files` retention ceiling). After each sync the observed remote layout (`record_remote_stats`) is persisted in `sync_metadata` keys `sync_remote_oplog_count` and `sync_remote_baseline_modified_ms`; `get_sync_config` derives `compaction_suggested` from them (oplog count at/above the retention ceiling, or baseline older than 30 days) so the UI can offer the manual compact button without an extra network round-trip.
- `commands/sync/auto.rs` is the auto-sync background worker (`AutoSyncWorker`), started in `lib.rs` setup and managed as `Mutex<Option<AutoSyncWorker>>`. It re-reads `auto_sync()` and `auto_sync_interval_secs()` from the managed `ConfigStore` every second, so toggling the setting takes effect without a restart, and runs `run_sync` (the shared sync entry point also used by `sync_upload_backup`) when auto-sync is enabled and the interval has elapsed since the last attempt. `run_sync` resolves the managed `Mutex<ConfigStore>`/`Database`/`StoragePaths` states from the `AppHandle`, so the Tauri command and the worker share one path; the worker's syncs are serialized against manual ones by `SYNC_RUN_LOCK`.

## Storage paths and ownership

`src-tauri/src/storage/paths.rs` owns resource-root resolution and validation.

Resource-root safety is mandatory:

- Default roots are application-owned; custom roots require an explicit `.clipboard-resource-root` marker before orphan cleanup is enabled.
- Runtime startup may use an unmarked custom root but must not auto-claim it.
- Explicit settings configuration may claim only a safe empty root or validate an existing matching marker.
- Image and file roots must not overlap each other or reserved project/data/database/index/icon paths.
- Cleanup must skip the ownership marker and run only when the corresponding cleanup flag is true.

Changing data directories is a migration workflow, not a path-string edit. Keep managed/external path rewriting, database backup, search-index derivation, and concurrent ingestion/worker coordination in scope.

## Search

Tantivy uses the schema/query modules and a CJK-friendly n-gram tokenizer. SQLite triggers append `search_outbox` operations; `SearchSynchronizer` drains them and applies index changes. Outbox draining runs lazily inside `search_clipboard_items` by default, or in a `SearchSyncWorker` background thread when `GeneralConfig.search_index_sync_mode` is `"background"` (see `search-cache-strategy.md`). Full rebuild begins by clearing/recreating derived index state and repopulating from SQLite.

Read `search-cache-strategy.md` before changing query pagination or caching. Index writes/rebuilds must invalidate backend cached IDs.

## OCR

`OcrEngine` allows PP-OCR, Tesseract, and no-op implementations. `OcrWorkerManager` owns a replaceable worker so engine/model/threshold changes can restart OCR without restarting the app. OCR rows are recoverable jobs, share image-hash results, and feed search through the outbox.

Keep these paths aligned when changing OCR: config, model install/status, engine selection/fallback, worker lifecycle, database status transitions, search synchronization, settings feedback/progress, and shutdown.

## Capture, content, and self-trigger suppression

The clipboard monitor produces change notifications; a named capture thread reads platform content, applies privacy and self-trigger checks, stores resources/metadata, saves through the repository, queues OCR/thumbnails, and emits the saved record. Platform access goes through the `PlatformClipboard` trait implemented per OS in `platform/` (`platform::platform()` returns the active adapter; Linux picks X11 or Wayland at runtime). Text capture also reads optional rich-text fragments for later paste-by-format: an HTML fragment (`PlatformClipboard::read_clipboard_html`: `HTML Format`/CF_HTML on Windows, `public.html` on macOS, `None` on Linux) and an RTF payload (`PlatformClipboard::read_clipboard_rtf`: the registered `Rich Text Format` on Windows, `None` on macOS/Linux). Plain text, the HTML fragment, and the RTF payload are each capped at the configurable `maxTextCaptureBytes` limit (default 500_000 bytes; an `Arc<AtomicU64>` in `CaptureState` seeded from `GeneralConfig` and updated live by `set_general_settings`); over-limit content is skipped and stored records carry `html_content`/`rtf_content` only when under the limit.

`content/` responsibilities:

| File                         | Responsibility                                                                                                         |
| ---------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `detector.rs` / `actions.rs` | content kind/marker detection and quick actions                                                                        |
| `hash.rs`                    | canonical content/media/file hashes plus `SelfTriggerGuard`                                                            |
| `file_store.rs`              | managed copy decisions and verification                                                                                |
| `resource_metadata.rs`       | metadata schema/MIME/file details                                                                                      |
| `thumbnail.rs`               | background preview generation and worker lifecycle                                                                     |
| `transform.rs`               | text transforms plus the paste-cleaning pipeline (`clean_paste`: trim, collapse whitespace, strip URL tracking params) |

Write-back registration and capture-side checks must use the same canonical hashing rules for text, links, images, and files. A mismatch creates duplicate history records or suppresses legitimate captures.

## Privacy and cleanup

Privacy pause, ignored applications, and sensitive-source checks must happen before persistence, file writes, OCR, or index work. Cleanup reads current config periodically, protects favorites, respects recycle-bin policy, removes database rows first through repository rules, and cleans only positively owned orphan resources.

## Platform adapters

`platform/` contains shared traits/managers plus Windows, macOS, X11, and Wayland sources. Windows is the primary wired runtime. The presence or size of macOS/Linux files is not proof of native clipboard, hotkey, source-app, double-modifier, tray, or quick-paste completion. Use `TODO.md`, runtime wiring, target-specific tests, and real-platform evidence.

Keep shared degradation capabilities accurate in `RuntimeInfo`; never expose an enabled UI path when the platform implementation is a scaffold.

`RuntimeInfo` (built by `runtime_info()` in `platform/platform_info.rs`, returned by the `get_runtime_info` command) carries `app_version`, `operating_system`, `architecture`, `executable_path` (from `std::env::current_exe()`, empty string when unavailable), and `capabilities`. The frontend `RuntimeInfo` type in `src/lib/types/clipboard.ts` mirrors it with camelCase names (`executablePath`). The About settings panel reads it to display the running program's location.

## CLI and loopback API

`main.rs` parses GUI versus process CLI execution. `cli/mod.rs` implements commands over the database. The process CLI resolves the database path from the configured storage/data directory through the same `StoragePaths` used by the GUI, so a custom storage directory stays consistent between CLI and desktop. `cli/api.rs` starts only on explicit command, binds to `127.0.0.1`, accepts configured limits, and must retain a stoppable listener/thread lifecycle.

The loopback API does not send a wildcard CORS header, so browser-based cross-origin requests cannot read clipboard data; automation clients that do not rely on CORS are unaffected.

A CLI/API action that writes to the system clipboard must follow the same self-trigger and metadata-preservation rules as the GUI.

`export/` owns `write_export_file` (creates parent directories and writes export output). Both the CLI and the file-backed Tauri commands in `commands/export.rs` (`export_to_file`, `import_from_file`) reuse it; the GUI import emits `clipboard-history-invalidated` after a successful import so the main window refreshes.

## Update checks

`commands/update.rs` exposes `check_for_update`: a stateless-per-call release check driven by the persisted general setting `updateSource` (an explicit `GeneralConfig` field; `ConfigStore::update_source()` returns an `UpdateSource` enum with `Github` and `Gitcode` variants). It reads the configured `UpdateSource` from `Mutex<ConfigStore>`, then queries the matching latest-release API (`api.github.com/repos/muutot/Clipboard` or `api.gitcode.com/api/v5/repos/m2u/Clipboard/releases/latest`) using the existing `reqwest` dependency (10s timeout, `clipboard-desktop` UA). It compares the latest tag to the current app version with numeric three-segment ordering (ignoring a `v` prefix and non-numeric segments), and returns `UpdateInfo` (camelCase) whose `release_url` falls back to the source's release page when the payload has no `html_url` (GitCode). No install/apply path exists; the result links to the release page. Test coverage lives in the module's unit tests plus `UpdateSource` parsing/persistence tests in `config/tests.rs`. A release page URL should remain the only external navigation surface until a signed installer pipeline exists.

## Unified shutdown

`stop_runtime_services()` stops cleanup, clipboard monitor, capture, OCR, thumbnail, hotkey, and local API services. The Tauri `ExitRequested` path invokes it; ordinary window close may hide to tray instead.

Every new worker/listener/server requires:

- a stop signal;
- a retained join/unlisten handle;
- idempotent stop behavior;
- integration with normal exit, tray exit, interrupt, and restart behavior as applicable;
- tests for drop/stop or a documented runtime verification gap.

## Backend change checklist

- Keep business logic in focused modules; keep `lib.rs` as boundary/composition where practical.
- Return explicit errors through Tauri and avoid partial cross-resource updates.
- Add focused Rust tests for repository, migration, hash, cache, worker, and path invariants.
- Run `cargo fmt`, focused tests, full Rust tests, and Clippy.
- Update this reference and `data-contracts.md` when runtime ownership or cross-layer contracts change.
