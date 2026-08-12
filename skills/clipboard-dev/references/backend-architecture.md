# Backend Architecture

## Contents

- [Runtime composition](#runtime-composition)
- [Managed runtime state](#managed-runtime-state)
- [Database and repositories](#database-and-repositories)
- [Synchronization](#synchronization)
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

`src-tauri/src/lib.rs::run` is the GUI composition root. The safety-sensitive startup order is:

1. load `ConfigStore` beside the executable/project;
2. acquire the optional single-instance guard and wake listener;
3. load keyboard configuration;
4. resolve `StoragePaths` without auto-claiming arbitrary custom roots;
5. recover/validate SQLite, quarantine a stale search index after recovery, refresh backups, and requeue interrupted OCR;
6. open/validate Tantivy;
7. record startup metrics and start the selected OCR worker;
8. start thumbnail, privacy/capture, clipboard-monitor, ingestion, cleanup, tray, and hotkey services;
9. manage state, apply startup window configuration, register commands, and enter the Tauri event loop.

Do not reorder recovery, search initialization, worker startup, or managed-state installation without reviewing failure paths and shutdown.

## Managed runtime state

The app manages `ConfigStore`, `StoragePaths`, `Database`, `SearchIndex`, `PerformanceTracker`, privacy/capture/self-trigger state, clipboard monitor, keyboard/hotkey managers, OCR and thumbnail workers, cleanup worker, local API server, and the optional single-instance guard.

Use `Mutex`/`Arc` according to existing ownership. Never hold a config or ingestion lock across slow filesystem, network, or UI work unless the operation explicitly requires atomicity.

## Database and repositories

`src-tauri/src/storage/` owns SQLite schema, forward-only migrations, repositories, recovery, and derived-data coordination. `Database::from_connection` enables foreign keys, WAL, normal synchronous mode, memory temp storage, cache/mmap settings, initializes or migrates the current schema, and persists one UUID device identity. Schema v1 is the one-time clean baseline; later versions use the adjacent transactional chain in `storage/migrations.rs`, while newer-than-supported databases are rejected untouched. `storage/sync_state.rs` is the compact v1 replication repository: it preserves clipboard rows as first-snapshot input, assigns deterministic `(modified_at_ms, sync_writer_device_id)` versions, maintains the outbox/tombstone/publication/cursor/checkpoint/resource tables, and applies snapshots, segments, or a full checkpoint atomically with echo suppression. The v1 command orchestrator calls `Database::initialize_sync` through `sync::v1::sync_database`; initialization does not read or convert obsolete sync data.

`storage/recovery.rs` validates SQLite integrity, rotates current/previous backups, quarantines damaged files, restores the first valid backup, and causes the derived search index to be quarantined/rebuilt after recovery. A deliberate schema reset deletes obsolete backup generations before writing a fresh v1 backup, so discarded rows cannot return through recovery. Persistence changes must preserve atomic config writes, recovery, backup refresh, and rebuildability of derived data.

## Synchronization

`src-tauri/src/sync/` contains only the S3 transport and the first/only replication protocol:

- `s3.rs` owns AWS SigV4 request signing, paginated object listing, conditional writes, streaming file upload/download, ETag handling, and S3-compatible connection testing.
- `v1/` owns the isolated namespace, strict wire envelopes, content-addressed resources, the scoped object-store adapter, bootstrap/incremental replication, checkpoint recovery, CAS compaction, and vector-bounded garbage collection described in `docs/SYNC_V1.md`. Fresh devices apply the global checkpoint before peer heads; unavailable snapshots or non-contiguous segment chains force checkpoint recovery and one retry, with fallback to the retained previous checkpoint. A local checkpoint-vector baseline prevents checkpoint pointer/body reads on idle runs; compaction runs after 50,000 aggregate sequences or a device/epoch-set change, only when all peers pulled successfully. The CAS winner deletes history covered by the previous checkpoint vector, retains current plus previous checkpoints, preserves same/newer-generation candidates against delayed cleanup, and records its local baseline after GC so interruption remains retryable. Before publishing, the engine reconciles the local device's remote head; a restored/divergent local publication state first re-applies remote history, rotates epoch, and republishes a complete snapshot instead of overwriting a newer head. Remote-head failures are isolated per device: healthy peers continue, the result reports `failedPeers`, and only head-namespace discovery failure aborts the whole pull pass.
- `commands/sync/mod.rs` exposes only `get_sync_config`, `set_sync_config`, typed S3 connection testing, and `sync_now`. It snapshots config before I/O, derives one optional remote-scoped `SessionKey`, constructs one scoped `S3ObjectStore`, and calls `v1::sync_database` for manual and automatic runs.
- `commands/sync/auto.rs` owns the stoppable background loop. Manual and automatic runs share a process-wide try-lock; a successful remote apply emits `clipboard-history-invalidated`, and last-run status persistence is best-effort.

There is no WebDAV transport, baseline/oplog archive implementation, legacy resource pool, fallback encryption path, or historical wire reader in the source tree.

## Storage paths and ownership

`src-tauri/src/storage/paths.rs` owns resource-root resolution and validation.

Resource-root safety is mandatory:

- default roots are application-owned;
- custom roots require an explicit `.clipboard-resource-root` marker before orphan cleanup is enabled;
- startup may use an unmarked custom root but must not auto-claim it;
- explicit settings may claim only a safe empty root or validate a matching marker;
- image and file roots must not overlap each other or reserved project/data/database/index/icon paths;
- cleanup skips the ownership marker and runs only when the corresponding cleanup flag is true.

Changing data directories is a migration workflow, not a path-string edit. Keep managed/external path rewriting, database backup, search-index derivation, and concurrent ingestion/worker coordination in scope.

## Search

Tantivy uses the schema/query modules and a CJK-friendly n-gram tokenizer. SQLite search triggers append `search_outbox` operations; `SearchSynchronizer` drains them. Outbox draining runs lazily inside `search_clipboard_items` by default or in a startup `SearchSyncWorker` when `GeneralConfig.search_index_sync_mode` is `background`. Full rebuild clears/recreates derived index state and repopulates from SQLite. Read `search-cache-strategy.md` before changing pagination or query caching.

## OCR

`OcrEngine` supports PP-OCR, Tesseract, and no-op implementations. `OcrWorkerManager` owns a replaceable worker so engine/model/threshold changes can restart OCR without restarting the app. OCR rows are recoverable jobs, share image-hash results, and feed search through the outbox. Keep config, model installation/status, fallback selection, worker lifecycle, database transitions, search synchronization, settings progress, and shutdown aligned.

## Capture, content, and self-trigger suppression

The clipboard monitor produces change notifications; a capture thread reads platform content, applies privacy and self-trigger checks, stores resources/metadata, saves through the repository, queues OCR/thumbnails, and emits the saved record. Platform access goes through `PlatformClipboard` adapters. Text capture may include HTML/RTF fragments, each capped by `maxTextCaptureBytes`.

`content/` owns detection/actions, canonical hashes and `SelfTriggerGuard`, managed file copies, resource metadata, thumbnails, and text transforms. Write-back registration and capture-side checks must use identical canonical hashing rules for text, links, images, and files.

## Privacy and cleanup

Privacy pause, ignored applications, and sensitive-source checks happen before persistence, file writes, OCR, or index work. Cleanup reads current config periodically, protects favorites, respects recycle-bin policy, removes database rows through repository rules, and removes only positively owned orphan resources.

## Platform adapters

`platform/` contains shared traits/managers plus Windows, macOS, X11, and Wayland sources. Windows is the primary wired runtime. File presence is not proof of native clipboard, hotkey, source-app, tray, or quick-paste completion; verify runtime wiring, target-specific tests, and real-platform evidence. Keep degradation capabilities accurate in `RuntimeInfo`.

## CLI and loopback API

`main.rs` separates GUI and process CLI execution. `cli/mod.rs` implements list/search/copy/paste/delete/export/stats over the same configured database path as the GUI. `cli/api.rs` starts only on explicit command, binds to `127.0.0.1`, enforces configured limits, and retains a stoppable listener/thread lifecycle. A CLI/API write to the system clipboard follows the same self-trigger and metadata-preservation rules as the GUI.

## Unified shutdown

`stop_runtime_services()` stops cleanup, clipboard monitor, capture, OCR, thumbnail, hotkey, and local API services. The Tauri `ExitRequested` path invokes it; ordinary window close may hide to tray. Every new worker/listener/server needs a stop signal, retained join/unlisten handle, idempotent stop behavior, integration with normal exit/tray exit/interrupt/restart as applicable, and drop/stop tests or a documented verification gap.

## Backend change checklist

- keep business logic in focused modules and `lib.rs` as the boundary;
- return explicit errors through Tauri and avoid partial cross-resource updates;
- add focused Rust tests for repository, schema, hash, cache, worker, and path invariants;
- run `cargo fmt`, focused tests, full Rust tests, and Clippy;
- update this reference and `data-contracts.md` when runtime ownership or cross-layer contracts change.
