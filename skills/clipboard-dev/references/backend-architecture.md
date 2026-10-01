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

`logging::init` runs first in the setup hook (before config load) with an initial `Info` level: on Windows it redirects the process stderr handle into `<project>/logs/clipboard.log` (renamed to `clipboard.log.old` past 512 KiB at startup), so every diagnostic survives GUI-subsystem builds. After the config loads, `logging::set_level(config.log_level())` applies the persisted threshold, and `set_general_settings` re-applies it live. During runtime, `log_line` drops lines below the threshold, samples the log size at most every 30 s, and truncates an oversized log in place — the append handle keeps writing at end-of-file, so renaming would detach it from the path. All backend modules emit via the leveled `crate::log_error!`, `crate::log_warn!`, `crate::log_info!`, and `crate::log_debug!` macros (the historical `crate::log_event!` is an `Info` alias); each line is tagged `[LEVEL]`. Do not reintroduce bare `eprintln!` in library code (main.rs CLI paths keep raw output on purpose).

## Managed runtime state

The app manages `ConfigStore`, `StoragePaths`, `Database`, `SearchIndex`, `PerformanceTracker`, privacy/capture/self-trigger state, clipboard monitor, keyboard/hotkey managers, OCR and thumbnail workers, cleanup worker, local API server, and the optional single-instance guard. The local API server requires every request to present the bearer token persisted at `conf/api.token` (created on first start by `commands/api.rs::load_or_create_api_token`), a loopback `Host` header, and rejects requests carrying an `Origin` header — see `cli/api.rs::authorize`; `/health` is the only endpoint exempt from the token. On Unix the token file is tightened to owner-only (`0600`) on every read/create, and on Windows it gets a best-effort owner-only ACL via `icacls`, so another local account cannot read it. `conf/api.token` deliberately holds the credential **in the clear** and must keep doing so: it is a documented integration point (`cli/api.rs` names this exact path in its 401 body, and the file exists so external scripts can read a stable credential), so routing it through `platform::secret_store` would break both the hint and the scripts the token exists for. Confidentiality therefore rests entirely on the file mode, which is why `commands/api.rs::write_api_token` creates the file with `OpenOptions::mode(0o600)` instead of writing and then chmod-ing — a plain `fs::write` leaves a window in which the credential is world-readable, and on a volume without POSIX modes (exFAT, some network shares) the later narrowing never takes effect. Both `restrict_api_token_permissions` branches log on failure rather than discarding it with `let _ =`, because a silently-skipped narrowing is the one case the user needs to hear about. An empty or whitespace-only token file is treated as absent and regenerated, so the API can never authorize against an empty string. Connections run one thread each with a 2 s per-read timeout, a 10 s total request-read budget (`MAX_READ_DURATION`, so a drip-feed client cannot pin a thread without ever presenting a token), and a 30 s write timeout; connection slots are reserved atomically and beyond 32 concurrent connection threads new connections get an immediate 503 instead of spawning more threads.

The single-instance guard treats a failed liveness probe as _inconclusive, and therefore as running_. On Windows, `classify_open_process_failure` maps only `ERROR_INVALID_PARAMETER` (87) to `OwnerLiveness::Dead`; `ERROR_ACCESS_DENIED` (5) and every other failure map to `Running`. `OpenProcess` returns access-denied when the owner is elevated or runs as a different user, and reading that as a dead owner made a second launch delete the live instance's `instance.lock` and start alongside it — two clipboard monitors, two capture loops, two OCR workers, and two hotkey registrations on one database. Only an error that positively identifies a nonexistent pid may authorize deleting the lock file.
Use `Mutex`/`Arc` according to existing ownership. Never hold a config or ingestion lock across slow filesystem, network, or UI work unless the operation explicitly requires atomicity.

## Database and repositories

`src-tauri/src/storage/` owns SQLite schema, forward-only migrations, repositories, recovery, and derived-data coordination. `Database::from_connection` enables foreign keys, WAL, normal synchronous mode, memory temp storage, cache/mmap settings, initializes or migrates the current schema, and persists one UUID device identity. Schema v1 is the one-time clean baseline; later versions use the adjacent transactional chain in `storage/migrations.rs`, while newer-than-supported databases are rejected untouched. `crates/clipboard-sync/src/v1/repository.rs` owns the provider-neutral replication DTOs and `SyncRepository` contract; `storage/sync_repository.rs` adapts `Database` to that contract and owns point-in-time SQLite snapshot creation. `storage/sync_state.rs` remains the compact SQLite implementation: it preserves clipboard rows as first-snapshot input, assigns deterministic `(modified_at_ms, sync_writer_device_id)` versions, maintains the outbox/tombstone/publication/cursor/checkpoint/resource-reference tables, restores remote keys only for scope-aware publication, and applies streamed snapshots, segments, or a full checkpoint atomically with echo suppression. Remote image/file/icon keys never masquerade as local paths in `clipboard_items`; their role/ordinal references commit with the row and cursor so ordinary pull has zero blob downloads. The v1 command orchestrator calls `Database::initialize_sync` through the provider-neutral engine; initialization does not read or convert obsolete sync data.

`storage/recovery.rs` validates SQLite integrity, rotates current/previous backups, quarantines damaged files, restores the first valid backup, and causes the derived search index to be quarantined/rebuilt after recovery. A deliberate schema reset deletes obsolete backup generations before writing a fresh v1 backup, so discarded rows cannot return through recovery. Persistence changes must preserve atomic config writes, recovery, backup refresh, and rebuildability of derived data.

## Synchronization

Synchronization is split between the Tauri-independent `src-tauri/crates/clipboard-sync/` crate
and the desktop integration in `src-tauri/src/sync/`:

- `crates/clipboard-sync/src/s3.rs` owns AWS SigV4 request signing, paginated object listing (including decoded listing ETags, buffered under a hard object-count cap so a runaway namespace fails loudly instead of exhausting memory), conditional writes, streaming file upload/download, ETag handling, and S3-compatible connection testing. `S3ObjectStore::with_metrics` may attach shared atomic diagnostics that count actual HTTP PUT/GET/HEAD/LIST/DELETE requests (including LIST continuation pages), payload bytes, and per-method elapsed time; normal production construction leaves metrics disabled. Its v1 modules own the isolated namespace, provider-neutral replication engine, object-store and repository contracts, frozen protocol DTOs, encryption/compression/pack codecs, and content-addressed image/file/icon resource processing without depending on Tauri or application storage. Checkpoint CAS publication must read the written pointer back before GC or local generation advancement; a missing, changed, or regressing pointer is a hard failure. The engine receives only `SyncRepository`, `ObjectStore`, and `SyncEnginePaths { temporary_directory, resource_roots }`; it cannot see the desktop project, database, search index, or cleanup layout. It owns metadata-only bootstrap/incremental pull, checkpoint recovery, CAS compaction, and vector-bounded garbage collection described in `docs/SYNC_V1.md`.
- `src/sync/v1/` owns only explicit desktop-domain DTO/path adapters and SQLite-backed integration tests for the crate engine; `storage/sync_repository.rs` supplies the production SQLite implementation. Snapshot/checkpoint publication first makes a point-in-time SQLite copy, releases the live database lock, exports deterministic bounded batches into one temporary chunked pack, then performs one streaming S3 PUT; pull performs one streaming GET and applies every decoded chunk plus cursor/checkpoint state in one rollback-safe SQLite transaction. Segments stay on the compact single-envelope path, so normal daily sync request counts are unchanged. Preview images remain device-local derived data and are stripped at the storage export boundary. With a sync password, resources use a keyed plaintext identity and fixed 1 MiB AES-256-GCM chunks, adding a 20-byte header plus 16 bytes per non-empty chunk; upload/download remain file-streamed and bounded to one chunk of encryption memory. Pulling a pack validates and transactionally records remote resource references but does not download blobs; scope-aware republishing reuses those references even before materialization. Each run performs one paginated heads listing. Once a head has been validated/applied, a disposable `sync_metadata` cache may skip its body GET only when listing ETag/size and local publication state or peer cursor all still match; missing ETags, malformed cache, recovery changes and mismatches conservatively GET/decode the head. Fresh devices authenticate existing canonical pointers before their first publication, then apply the global checkpoint before peer heads; wrong/missing/changed passwords and encryption-mode mismatches fail without writing a new head. In-place password rotation is currently unsupported and requires a future dedicated destructive materialize/delete/reset/republish workflow. Unavailable snapshots or non-contiguous segment chains force checkpoint recovery and one retry, with fallback to the retained previous checkpoint. A local checkpoint-vector baseline prevents checkpoint pointer/body reads on idle runs; compaction runs after 50,000 aggregate new-history units, a known-device removal, or an existing-device epoch change/regression, only when all peers pulled successfully. A newly observed device contributes the larger of its trusted bootstrap record count and published sequence, so an empty peer does not immediately rewrite the full checkpoint. The CAS winner deletes history covered by the previous checkpoint vector, retains current plus previous checkpoints, preserves same/newer-generation candidates against delayed cleanup, and records its local baseline after GC so interruption remains retryable. Before publishing, the engine reconciles the local device's remote head through the same cache-or-GET path; a restored/divergent local publication state first re-applies remote history, rotates epoch, and republishes a complete snapshot instead of overwriting a newer head. Remote-head failures are isolated per device: healthy peers continue, the result reports `failedPeers`, and only head-namespace discovery failure aborts the whole pull pass.
- `commands/sync/mod.rs` exposes `get_sync_config`, `set_sync_config`, typed S3 connection testing, `sync_now`, and the UI-facing `materialize_clipboard_item`. Sync runs snapshot config before I/O, derive one optional remote-scoped `SessionKey`, construct one scoped `S3ObjectStore`, and call `v1::sync_database` for manual and automatic runs. On-demand materialization reuses verified local files, deduplicates concurrent downloads with weak process-local locks, writes all item paths atomically without replication/version changes, and queues a local thumbnail rebuild for images.
- `commands/sync/auto.rs` owns the stoppable background loop. Manual and automatic runs share a process-wide try-lock; a successful remote apply emits `clipboard-history-invalidated`, and last-run status persistence is best-effort but logs a failure via `log_event!` instead of discarding the error silently. `AutoSyncWorker::stop` joins the worker through a helper with a 30 s bound: an in-flight `run_sync` has no cancellation checkpoints, so a network-slow run is logged and leaked (flag set, thread reclaimed at process exit) instead of hanging exit/restart.

There is no WebDAV transport, baseline/oplog archive implementation, legacy resource pool, fallback encryption path, or historical wire reader in the source tree.

## Storage paths and ownership

`src-tauri/src/storage/paths.rs` owns resource-root resolution and validation.

Resource-root safety is mandatory:

- default roots are application-owned;
- custom roots require an explicit `.clipboard-resource-root` marker before orphan cleanup is enabled;
- startup may use an unmarked custom root but must not auto-claim it;
- explicit settings may claim only a safe empty root or validate a matching marker;
- the `set_resource_ownership(owned)` command is the explicit recovery action for an already-used unmarked root: it writes or removes only the marker (never deleting or touching file contents), skips roots that do not require a marker (default project data directories), and reports `restartRequired` only when the resulting ownership differs from the cleanup flags captured at startup (an off-and-on round trip in one session needs no restart);
- image and file roots must not overlap each other or reserved project/data/database/index/icon paths;
- cleanup skips the ownership marker and runs only when the corresponding cleanup flag is true;
- every orphan-cleanup entry point (scheduled `CleanupWorker`, `enforceHistoryCleanup`, `cleanupStorageFiles`) and the `renameItem` command hold `CaptureState.storage_maintenance_lock` for the run: a rename persists new paths before moving the file, so an unserialized cleanup could delete the not-yet-moved old file and leave the rolled-back record dangling. This lock is deliberately separate from `ingestion_guard` so a long scan never stalls the capture loop.

Changing data directories is a migration workflow, not a path-string edit. Keep managed/external path rewriting, database backup, search-index derivation, and concurrent ingestion/worker coordination in scope. The webview asset protocol's static scope is empty; at startup the app grants the managed storage root plus any configured image/file roots with `asset_protocol_scope().allow_directory`, so previews and thumbnails render even when a custom resource directory lives outside `storage`, and the install directory — where `$EXE`/`$RESOURCE` both resolve on Windows and where `conf/api.token`, `conf/conf.json`, and the database live — is never exposed. Migration must be retryable: `VACUUM INTO` refuses an existing destination, so `migrate_storage_data` quarantines any existing target database and its `-wal`/`-shm` sidecars to `*.pre-migrate-<stamp>` first (restoring them if the vacuum fails), which both lets a retry after a partial failure proceed and avoids destroying a pre-existing database at the chosen directory. Directory copies and size walks track canonical ancestor paths (`copy_dir_contents`/`dir_size` in `commands/config/storage.rs`) so a junction/reparse cycle cannot recurse until the stack overflows.

## Search

Tantivy uses the schema/query modules and a CJK-friendly n-gram tokenizer. SQLite search triggers append `search_outbox` operations; `SearchSynchronizer` drains them. Outbox draining runs lazily inside `search_clipboard_items` by default or in a startup `SearchSyncWorker` when `GeneralConfig.search_index_sync_mode` is `background`; the worker also retries a required full rebuild (`SearchIndex::requires_full_rebuild`) each tick. The outbox is mutated only by the synchronizer: history cleanup must never prune it, because deleting the `delete` events for hard-deleted rows would leave their Tantivy documents indexed forever. Full rebuild clears/recreates derived index state and repopulates from SQLite. ``SearchIndex::open`retries a failed open once after a short delay before falling back to delete-and-recreate, so transient interference (antivirus locks, permission hiccups) does not trigger a minutes-long full re-index.`SearchIndexLayout::prepare`applies the same distinction to the manifest:`read_manifest`returns`Err`for "present but unreadable" and`Ok(None)`for "absent", and only a manifest that could actually be read (or an index directory that is already gone) may authorize`remove_dir_all`. An unreadable manifest with a healthy index is logged and deferred instead of deleting the index. Read `search-cache-strategy.md` before changing pagination or query caching.

## OCR

`OcrEngine` supports PP-OCR, Tesseract, and no-op implementations. `OcrWorkerManager` owns a replaceable worker so engine/model/threshold changes can restart OCR without restarting the app. `OcrWorker::start` and `OcrWorkerManager::start`/`restart` return `Result`, so a thread-spawn failure degrades to unavailable OCR instead of panicking the app. OCR rows are recoverable jobs, share image-hash results, and feed search through the outbox. Model downloads verify size and the pinned upstream SHA-256 (each `PpOcrModelFile.sha256`, taken from the GitHub release asset digest) before activating the file, and only then record the digest to `<model>.sha256` (`ocr/models.rs::record_model_digest`) so a later install redownloads when the on-disk file stops matching. Digest checks are memoized by file length/mtime (`ocr/models.rs::digest_cache`), so the periodic OCR status and memory-diagnostics polling does not re-hash unchanged ONNX weights. A replaced asset, intercepted TLS stream, or corruption is rejected instead of reaching the ONNX runtime; updating a model means updating the pinned size/digest in `ocr/models.rs` together with the release tag. The Tesseract engine runs the CLI with a fixed 120 s per-image timeout; its stdout/stderr must be drained on dedicated threads while waiting (`run_tesseract_with_timeout` → `wait_for_child_with_drain`), because polling `try_wait` without reading the pipes deadlocks the child as soon as the recognized output exceeds the OS pipe buffer. Keep config, model installation/status, fallback selection, worker lifecycle, database transitions, search synchronization, settings progress, and shutdown aligned.

Recognition failures are retried, but the retry queue lives in the worker
(`ocr/worker.rs::PendingRetry`), not in the database, and both the first
attempt and every retry go through `handle_recognition_failure` so the
budget is consulted once per failure. Two constraints make that shape
mandatory rather than stylistic:

- Requeueing through `retry_ocr` would set the row back to `pending`, and
  `claim_next_ocr` orders by `created_at_ms` — the same row would be
  claimed again on the next 500 ms poll while every other pending image
  waited out the full backoff.
- Leaving the row in `processing` for the whole retry window keeps the
  detail panel honest, and startup `requeue_interrupted_ocr` already
  turns `processing` rows back into `pending`, so a crash mid-retry is
  covered without any new state.

`MAX_RECOGNITION_ATTEMPTS` bounds the automatic passes; the manual
`regenerate_clipboard_item_ocr` path stays unlimited. Only the last
failure is persisted, so a stored `error_message` is always the real
reason the final attempt failed. The attempt-count test exists to catch
the budget being checked on one path only, which silently made the
constant mean one more pass than it says.

`PpOcrEngine::recognize` decodes through
`content::hash::decode_image_file`, not oar-ocr's `load_image`, so the
OCR path inherits the same `MAX_DECODE_DIMENSION` and allocation budget
as thumbnails instead of allocating whatever the file declares. It then
clamps the long side to `MAX_OCR_SIDE` (4000, oar-ocr's own
`DEFAULT_MAX_SIDE_LIMIT`, which the detector discards beyond anyway)
before `predict`. Two rules follow from that clamp and must not be
dropped: `ocr_input_geometry` is a pure function so the mapping is
testable without ONNX weights, and the stored block geometry must be
multiplied by the returned `coordinate_scale`, because the detector
reports boxes in the coordinate space of the image it was handed. The
OCR-coordinate highlight feature is still unimplemented, so nothing
consumes those boxes yet — a missing rescale would only surface when
that lands.

## Capture, content, and self-trigger suppression

The clipboard monitor produces change notifications; a capture thread reads platform content, applies privacy and self-trigger checks, stores resources/metadata, saves through the repository, queues OCR/thumbnails, and emits the saved record. Platform access goes through `PlatformClipboard` adapters. Text capture may include HTML/RTF fragments, each capped by `maxTextCaptureBytes`. The monitor's `start` detects a dead monitor thread (the capture worker's receiver was dropped after a worker panic or spawn failure) via `JoinHandle::is_finished` and resets its flag so a restart is not permanently blocked by "already running".

`content/` owns detection/actions, canonical hashes and `SelfTriggerGuard`, managed file copies, resource metadata, thumbnails, and text transforms. Write-back registration and capture-side checks must use identical canonical hashing rules for text, links, images, and files. Untrusted clipboard/thumbnail images are decoded through `content/hash.rs::decode_image_bytes`/`decode_image_file`, which enforce a strict `MAX_DECODE_DIMENSION` plus the `image` crate's allocation budget, so a crafted decompression bomb cannot exhaust memory.

## Privacy and cleanup

Privacy pause, ignored applications, and sensitive-source checks happen before persistence, file writes, OCR, or index work. Cleanup reads current config periodically, protects favorites, respects recycle-bin policy, removes database rows through repository rules, and removes only positively owned orphan resources. The retention inputs are snapshotted into a `CleanupPolicy` value (`retention_days`, `max_items`, `recycle_bin_days`) before any work starts: the cleanup walks every resource root and canonicalizes every referenced path, so holding `Mutex<ConfigStore>` across it would stall every other config-reading command and the auto-sync worker's per-tick read. `enforce_history_cleanup_with_policy` therefore takes no `ConfigStore` at all, and the scheduled worker keeps loading its own store per tick.

## Platform adapters

`platform/` contains shared traits/managers plus Windows, macOS, X11, and Wayland sources. Windows is the primary wired runtime. File presence is not proof of native clipboard, hotkey, source-app, tray, or quick-paste completion; verify runtime wiring, target-specific tests, and real-platform evidence. Keep degradation capabilities accurate in `RuntimeInfo`.

## CLI and loopback API

`main.rs` separates GUI and process CLI execution. `cli/mod.rs` implements list/search/copy/paste/delete/export/stats over the same configured database path as the GUI. `cli/api.rs` starts only on explicit command, binds to `127.0.0.1`, enforces configured limits, and retains a stoppable listener/thread lifecycle. A CLI/API write to the system clipboard follows the same self-trigger and metadata-preservation rules as the GUI.

## Unified shutdown

`stop_runtime_services()` stops the auto-sync worker first (it is a background SQLite/S3 writer that must not outlive a storage snapshot), then cleanup, clipboard monitor, capture, OCR, thumbnail, hotkey, local API, and search-sync services. A poisoned managed lock never skips a stop: the guard is recovered with `into_inner()` (matching `state.rs`) so background writers still stop through teardown. The Tauri `ExitRequested` path invokes it; ordinary window close may hide to tray. Every new worker/listener/server needs a stop signal, retained join/unlisten handle, idempotent stop behavior, integration with normal exit/tray exit/interrupt/restart as applicable, and drop/stop tests or a documented verification gap. Background threads must never call a Tauri API that blocks on the main thread while the main thread may be joining them: `refresh_tray_recent_menu` posts its `tray.set_menu` work through `run_on_main_thread` instead of calling it directly, because `set_menu` blocks on the main thread and the capture worker is joined there during shutdown. If `ctrlc::set_handler` fails (a foreign handler installed first), the failure is logged so the missing unified interrupt path is diagnosable.

## Backend change checklist

- keep business logic in focused modules and `lib.rs` as the boundary;
- return explicit errors through Tauri and avoid partial cross-resource updates;
- add focused Rust tests for repository, schema, hash, cache, worker, and path invariants;
- run `cargo fmt`, focused tests, full Rust tests, and Clippy;
- update this reference and `data-contracts.md` when runtime ownership or cross-layer contracts change.
