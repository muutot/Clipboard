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

1. load `ConfigStore` beside the executable/project (`resolve_project_directory`: executable directory while writable, platform data dir otherwise — AppImages and read-only install dirs fall back so startup never panics on the read-only mount);
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
`platform/local_wake.rs` supplies non-Windows wake IPC: the lock owner binds loopback TCP, publishes a random token in owner-only `instance-wake.json`, accepts only a bounded authenticated wake request, and joins its listener on drop. Late teardown removes metadata only when its token still matches. Wake callbacks post window work to the main thread to avoid a join/UI deadlock. Wayland activation can still be declined by the compositor.

Use `Mutex`/`Arc` according to existing ownership. Never hold a config or ingestion lock across slow filesystem, network, or UI work unless the operation explicitly requires atomicity.

## Database and repositories

`src-tauri/src/storage/` owns SQLite schema, forward-only migrations, repositories, recovery, and derived-data coordination. `Database::from_connection` enables foreign keys, WAL, normal synchronous mode, memory temp storage, cache/mmap settings, initializes or migrates the current schema, and persists one UUID device identity. Schema v1 is the one-time clean baseline; later versions use the adjacent transactional chain in `storage/migrations.rs`, while newer-than-supported databases are rejected untouched. `crates/clipboard-sync/src/v1/repository.rs` owns the provider-neutral replication DTOs and `SyncRepository` contract; `storage/sync_repository.rs` adapts `Database` to that contract and owns point-in-time SQLite snapshot creation. `storage/sync_state.rs` remains the compact SQLite implementation: it preserves clipboard rows as first-snapshot input, assigns deterministic `(modified_at_ms, sync_writer_device_id)` versions, maintains the outbox/tombstone/publication/cursor/checkpoint/resource-reference tables, restores remote keys only for scope-aware publication, and applies streamed snapshots, segments, or a full checkpoint atomically with echo suppression. Remote image/file/icon keys never masquerade as local paths in `clipboard_items`; their role/ordinal references commit with the row and cursor so ordinary pull has zero blob downloads. The v1 command orchestrator calls `Database::initialize_sync` through the provider-neutral engine; initialization does not read or convert obsolete sync data.

`storage/recovery.rs` validates SQLite integrity, rotates current/previous backups, quarantines damaged files, restores the first valid backup, and causes the derived search index to be quarantined/rebuilt after recovery. A deliberate schema reset deletes obsolete backup generations before writing a fresh v1 backup, so discarded rows cannot return through recovery. Persistence changes must preserve atomic config writes, recovery, backup refresh, and rebuildability of derived data.

Corrupt configuration is moved to a UUID-suffixed quarantine before defaults are
loaded. A failed move returns an error with the original untouched; it must not
delete the only recovery copy or claim that default settings were persisted.

`export::stream` writes JSON/CSV/plain text incrementally from `Database::visit_active_items`, a single read transaction with one materialized row. GUI and CLI file exports use a buffered temporary file, fsync and atomic replacement; write failure preserves the destination. IPC/stdout compatibility wrappers still return a complete String. The visitor holds the database mutex and must not reenter it. A real WAL regression mutates recency on a second connection while exporting and verifies complete unique IDs and original snapshot values.

Plain-text import recognizes a `---` delimiter line with LF or CRLF endings, while preserving line endings inside each record. Delimiters at the file boundary create empty chunks that are ignored; they never create clipboard records. The CRLF regression imports two records and verifies the first record's internal CRLF bytes.

Historical auto tags use `Database::with_task_connection`: an existing file is reopened
without schema initialization, with a small cache and file-backed temporary storage. The
in-memory test path retains its single connection. Matching reads one WAL snapshot and
stores only candidate identities/tags in a temporary table. The subsequent immediate
transaction verifies matching inputs and rereads manual tags before additive writes;
changed inputs or a progress callback error roll back every update. Cancellation is
checked at phase/row boundaries and immediately before commit. A single regex or SQLite
busy wait is not preempted. The temporary plan is removed on success/failure.

## Synchronization

Synchronization is split between the Tauri-independent `src-tauri/crates/clipboard-sync/` crate
and the desktop integration in `src-tauri/src/sync/`:

- `crates/clipboard-sync/src/s3.rs` owns AWS SigV4 request signing, paginated object listing (including decoded listing ETags, buffered under a hard object-count cap so a runaway namespace fails loudly instead of exhausting memory), conditional writes, streaming file upload/download, ETag handling, and S3-compatible connection testing. `S3ObjectStore::with_metrics` may attach shared atomic diagnostics that count actual HTTP PUT/GET/HEAD/LIST/DELETE requests (including LIST continuation pages), payload bytes, and per-method elapsed time; normal production construction leaves metrics disabled. Its v1 modules own the isolated namespace, provider-neutral replication engine, object-store and repository contracts, frozen protocol DTOs, encryption/compression/pack codecs, and content-addressed image/file/icon resource processing without depending on Tauri or application storage. Checkpoint CAS publication must read the written pointer back before GC or local generation advancement; a missing, changed, or regressing pointer is a hard failure. The engine receives only `SyncRepository`, `ObjectStore`, and `SyncEnginePaths { temporary_directory, resource_roots }`; it cannot see the desktop project, database, search index, or cleanup layout. It owns metadata-only bootstrap/incremental pull, checkpoint recovery, CAS compaction, and vector-bounded garbage collection described in `docs/SYNC_V1.md`.
- `src/sync/v1/` owns only explicit desktop-domain DTO/path adapters and SQLite-backed integration tests for the crate engine; `storage/sync_repository.rs` supplies the production SQLite implementation. Snapshot/checkpoint publication first makes a point-in-time SQLite copy, releases the live database lock, exports deterministic bounded batches into one temporary chunked pack, then performs one streaming S3 PUT; pull performs one streaming GET and applies every decoded chunk plus cursor/checkpoint state in one rollback-safe SQLite transaction. Segments stay on the compact single-envelope path, so normal daily sync request counts are unchanged. Preview images remain device-local derived data and are stripped at the storage export boundary. With a sync password, resources use a keyed plaintext identity and fixed 1 MiB AES-256-GCM chunks, adding a 20-byte header plus 16 bytes per non-empty chunk; upload/download remain file-streamed and bounded to one chunk of encryption memory. Pulling a pack validates and transactionally records remote resource references but does not download blobs; scope-aware republishing reuses those references even before materialization. Each run performs one paginated heads listing. Once a head has been validated/applied, a disposable `sync_metadata` cache may skip its body GET only when listing ETag/size and local publication state or peer cursor all still match; missing ETags, malformed cache, recovery changes and mismatches conservatively GET/decode the head. Fresh devices authenticate existing canonical pointers before their first publication, then apply the global checkpoint before peer heads; wrong/missing/changed passwords and encryption-mode mismatches fail without writing a new head. In-place password rotation is currently unsupported and requires a future dedicated destructive materialize/delete/reset/republish workflow. Unavailable snapshots or non-contiguous segment chains force checkpoint recovery and one retry, with fallback to the retained previous checkpoint. A local checkpoint-vector baseline prevents checkpoint pointer/body reads on idle runs; compaction runs after 50,000 aggregate new-history units, a known-device removal, or an existing-device epoch change/regression, only when all peers pulled successfully. A newly observed device contributes the larger of its trusted bootstrap record count and published sequence, so an empty peer does not immediately rewrite the full checkpoint. The CAS winner deletes history covered by the previous checkpoint vector, retains current plus previous checkpoints, preserves same/newer-generation candidates against delayed cleanup, and records its local baseline after GC so interruption remains retryable. Before publishing, the engine reconciles the local device's remote head through the same cache-or-GET path; a restored/divergent local publication state first re-applies remote history, rotates epoch, and republishes a complete snapshot instead of overwriting a newer head. Remote-head failures are isolated per device: healthy peers continue, the result reports `failedPeers`, and only head-namespace discovery failure aborts the whole pull pass.
- `commands/sync/mod.rs` exposes `get_sync_config`, `set_sync_config`, typed S3 connection testing, `sync_now`, and the UI-facing `materialize_clipboard_item`. Sync runs snapshot config before I/O, derive one optional remote-scoped `SessionKey`, construct one scoped `S3ObjectStore`, and scope the current operation token around `sync_database_with_progress` for manual and automatic runs. On-demand materialization reuses verified local files, deduplicates concurrent downloads with weak process-local locks, writes all item paths atomically without replication/version changes, and queues a local thumbnail rebuild for images.
- `commands/sync/auto.rs` owns the stoppable background loop. Manual and automatic runs share a process-wide try-lock; a successful remote apply emits `clipboard-history-invalidated`, and last-run status persistence is best-effort but logs a failure via `log_event!` instead of discarding the error silently. `SyncCancellation` owns a root token; manual/automatic runs receive child tokens and
  the current operation scopes another child around `sync_database_with_progress`.
  Cancelling it leaves the root and automatic worker usable for the next run. Engine phases, resource
  hash/encryption chunks and rollback-safe batch iterators check cancellation. The S3 facade
  uses async reqwest on a shared two-thread Tokio runtime; selecting cancellation drops the
  request or body future, and retry waits are cancellable. Upload reads stay at 64 KiB per poll.
  No per-request blocking thread is detached. Shutdown signals the root before any database
  observer/worker join; auto-stop also cancels its child. The 30 s join bound remains only as a
  last-resort guard for non-preemptible local filesystem/SQLite/OS work. A completed atomic
  commit or remote PUT can win a cancellation race; the next run reconciles/adopts it using
  unchanged v1 publication rules. Both success and error invalidate desktop history because
  an earlier peer may have committed before a later cancellation. Cancelled runs persist a
  localized `cancelled` status. Custom ObjectStore implementations must supply their own
  interruptible transport if they block internally.

`commands/sync/policy.rs` registers active manual, automatic, connection-test and
materialization tokens while the configuration mutex is held. Local-only mode
rejects sync/test entry and returns the local record for materialization without
network access. Saving local-only=true cancels all registered runs before releasing
that same mutex, closing the snapshot/register race. The background sync operation
is a child of that registered token; its scope covers namespace resolution and the
engine so either privacy changes or task cancellation also interrupts S3 requests.
It does not cancel the auto worker or root, so later opt-in enables fresh runs.
A completed request/commit can
win the cancellation race; cancellation cannot retract already delivered bytes.
Connection testing uses the blocking pool because the synchronous S3 facade must
not block Tauri's async executor. Tests include a stalled loopback HTTP response
interrupted by the privacy policy.

There is no WebDAV transport, baseline/oplog archive implementation, legacy resource pool, fallback encryption path, or historical wire reader in the source tree.

Desktop sync resolves `clipboard_sync::v1::namespace` before engine work. New
identities exclude S3 credentials; existing v1 scopes are authenticated and bound
without rewriting data. A conditional, read-back-verified `v1/namespace.json`
stores the public scope plus a domain-separated HMAC password check. SQLite caches
that descriptor via `storage/sync_namespace.rs` for offline materialization and
rejects replacement of a known binding. All resolution I/O stays inside the same
cancellation scope. See `docs/SYNC_V1.md` for upgrading before credential rotation.

`sync/v1/s3_smoke.rs` includes an opt-in real-server cancellation regression: a thin
ObjectStore decorator cancels after a successful immutable snapshot/segment PUT.
The real head must remain unchanged and retry must preserve pending rows and converge.
Run through `scripts/sync-s3-test.ps1 -Test smoke`; it does not prove native exit timing.

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

Changing data directories is a migration workflow, not a path-string edit. Keep managed/external path rewriting, database backup, search-index derivation, and concurrent ingestion/worker coordination in scope. The webview asset protocol's static scope is empty; at startup the app grants the managed storage root plus any configured image/file roots with `asset_protocol_scope().allow_directory`, so previews and thumbnails render even when a custom resource directory lives outside `storage`, and the install directory — where `$EXE`/`$RESOURCE` both resolve on Windows and where `conf/api.token`, `conf/conf.json`, and the database live — is never exposed. Migration must be retryable: `VACUUM INTO` refuses an existing destination, so `migrate_storage_data` first moves the existing database and its `-wal`/`-shm` sidecars into an exclusively created `clipboard.sqlite3.pre-migrate-<uuid>` directory. A partial quarantine failure restores earlier moves; a snapshot, open, or path-rewrite failure preserves the failed database bundle in a separate quarantine before restoring the originals. Rollback refuses conflicting destinations and reports the remaining backup paths on failure. Successful migration retains the old bundle for recovery, and rapid retries cannot overwrite earlier backups. Directory copies and size walks track canonical ancestor paths (`copy_dir_contents`/`dir_size` in `commands/config/storage.rs`) so a junction/reparse cycle cannot recurse until the stack overflows.

`configure_storage_directory` first takes the same process-wide try-lock as manual/automatic S3 sync, refusing an active run before stopping services. After shutdown it drains `storage_maintenance_lock` and `ingestion_guard`. `migrate_and_save_storage` then reserves the source SQLite writer with a separate `BEGIN IMMEDIATE` connection before copying resources; this waits for an existing transaction and blocks later writes from independent connections, including CLI/API requests that escaped worker shutdown. The snapshot uses `snapshot_into` without a checkpoint (which would conflict with the reservation). Only a successful snapshot plus config save retains the empty transaction inside the managed `Database` until restart; copy/snapshot/save errors release it. Reads remain possible, while post-migration writes fail instead of being silently left in the old database. Real multi-connection tests cover drain-before-snapshot, writes during config save and after success, and copy/save failure release. The shutdown timeout alone must never be treated as proof that migration writers have stopped.

Migration resource copies never overwrite an existing destination. `copy_migration_file` reserves each new file with `create_new`, streams and flushes it, and cleans up only that newly created file on failure. An existing byte-identical file is a valid retry; differing content fails with the destination path so a pre-existing database's resources are preserved. Comparisons use bounded buffers. `copy_dir_contents` compares canonical roots before traversal: aliases of the same root are no-ops, while ancestor/descendant roots are rejected to prevent copying the newly created destination into itself.

## Search

Tantivy uses the schema/query modules and a CJK-friendly n-gram tokenizer. SQLite search triggers append `search_outbox` operations; `SearchSynchronizer` drains them. Outbox draining runs lazily inside `search_clipboard_items` by default or in a live-switchable `SearchSyncWorker` when `GeneralConfig.search_index_sync_mode` is `background`; the worker also retries a required full rebuild (`SearchIndex::requires_full_rebuild`) each tick. The outbox is mutated only by the synchronizer: history cleanup must never prune it, because deleting the `delete` events for hard-deleted rows would leave their Tantivy documents indexed forever. Full rebuild clears/recreates derived index state and repopulates from SQLite. ``SearchIndex::open`retries a failed open once after a short delay before falling back to delete-and-recreate, so transient interference (antivirus locks, permission hiccups) does not trigger a minutes-long full re-index.`SearchIndexLayout::prepare`applies the same distinction to the manifest:`read_manifest`returns`Err`for "present but unreadable" and`Ok(None)`for "absent", and only a manifest that could actually be read (or an index directory that is already gone) may authorize`remove_dir_all`. An unreadable manifest with a healthy index is logged and deferred instead of deleting the index. Read `search-cache-strategy.md` before changing pagination or query caching.

## OCR

Worker stop revokes a per-worker publication mutex before the bounded join.
Claims, reused results, successful inference and terminal failure writes share
that mutex; inference itself runs outside it. A timed-out old engine may finish
computing, but cannot write after revocation or overwrite a replacement worker.
The running flag records actual thread lifetime, while `is_running` also checks
publication permission. Concurrent stop callers wait for actual completion up
to the same bound instead of treating the stop request as thread completion.

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
`OcrImageOverlay` used by `DetailImagePreview` and `DetailOcrTab` consumes
these original-image coordinates for search highlighting and block selection;
a missing rescale misaligns those overlays on downscaled OCR inputs.

## Capture, content, and self-trigger suppression

Currency quick actions retain the complete digit/separator sequence from the
source, including ungrouped amounts beyond three digits and decimal precision.
Detection extracts text without interpreting locale-dependent numeric values;
tests assert the actual copy payload rather than only a `has_currency` flag.

URL tracking cleanup splits off `#fragment` before looking for a query string.
Fragments, including SPA routes with their own `?` parameters, remain byte-for-byte
unchanged in both the standalone transform and clean-paste pipeline.

Thumbnail shutdown uses an independent atomic stop flag checked before each queued job. Stop wakes an idle receiver and joins the current job, then discards the backlog; enqueue handles reject new work after stop. A currently running decoder is not interrupted. Skipped jobs retain their original-image preview fallback. The controlled-job test covers a blocked current job plus 100 queued jobs without decoding large fixtures.

`PlatformClipboard::read_clipboard_image` carries `ClipboardImageData::Png` (Windows) or `Rgba` (Linux/macOS), plus dimensions. Capture normalizes only explicitly raw pixels and rejects malformed raw dimensions/lengths. Never infer the representation from byte count: a valid encoded PNG can have exactly width × height × 4 bytes. Snapshot comparison and polling retain this representation tag. Native adapters still require their matching platform gate.

The clipboard monitor produces change notifications; a capture thread reads platform content, applies privacy and self-trigger checks, stores resources/metadata, saves through the repository, queues OCR/thumbnails, and emits the saved record. Platform access goes through `PlatformClipboard` adapters. Text capture may include HTML/RTF fragments, each capped by `maxTextCaptureBytes`. `platform/clipboard_snapshot.rs` retries multi-format reads up to three times: Windows uses the system sequence and macOS uses `NSPasteboard.changeCount`; without a native revision (Linux), two complete equal samples provide best-effort stability. An event-thread counter is not a server revision and must not be substituted. Repeated samples cannot detect ABA replacements. Exhaustion drops the unstable capture; immediate retries avoid depending on another monitor notification. The monitor's `start` detects a dead monitor thread (the capture worker's receiver was dropped after a worker panic or spawn failure) via `JoinHandle::is_finished` and resets its flag so a restart is not permanently blocked by "already running".

`content/` owns detection/actions, canonical hashes and `SelfTriggerGuard`, managed file copies, resource metadata, thumbnails, and text transforms. Write-back registration and capture-side checks must use identical canonical hashing rules for text, links, images, and files. Untrusted clipboard/thumbnail images are decoded through `content/hash.rs::decode_image_bytes`/`decode_image_file`, which enforce a strict `MAX_DECODE_DIMENSION` plus the `image` crate's allocation budget, so a crafted decompression bomb cannot exhaust memory.

macOS RTF capture reads `public.rtf` as NSData (16 MiB allocation ceiling), preserves non-ASCII code-page bytes as RTF hex escapes, and rejects raw `\\bin` runs because the stored RTF contract is a UTF-8 string. HTML/plain-text capture remains available when RTF is rejected. Native pasteboard behavior requires macOS verification.

Windows native reads validate HGLOBAL sizes before copying: 64 MiB for text/HTML/RTF, 4 KiB for the private self-trigger marker and 512 MiB for DIB input. DIB and HBITMAP dimensions share the 16,384-axis and 512 MiB decoded RGBA ceiling; only declared pixel rows are converted. HBITMAP reads require all requested rows and use one DIB allocation. These are native safety ceilings; configured capture/persistence limits still apply afterwards. Synthetic DIB/header tests exercise the limits without touching the system clipboard.

Linux `wl-paste`, `xclip` and foreground helpers use `platform/bounded_command.rs`:
600 ms per subprocess and a 64 MiB stdout ceiling. Capture supplies a shared 3 s
budget and stop flag across foreground sampling, formats and consistency retries;
timeout/overflow discards the capture instead of saving a partial format set.
Wayland quick-paste helpers use the same runner with a 1 MiB ceiling. Unix pipes
are nonblocking and each helper gets its own process group, killed/reaped on exit;
no reader thread waits indefinitely for inherited stdout. Windows fixture tests
use `PeekNamedPipe`; actual Linux adapters and descendant cleanup need Linux CI.
These bounds do not make unrelated native display/filesystem calls preemptible.

## Privacy and cleanup

`storage/resource_publication.rs` shares a process-local writer count and generation across `Database` connections keyed by the canonical database path. Capture (including icons), JSON/PPaste/portable-backup import, record duplication, sync/materialization and thumbnail jobs hold a publication guard from before resource lookup/reuse through the DB commit; repository save/preview methods also register publication. Orphan cleanup snapshots the generation before resolving references, holds no publication mutex during scans, and checks it under a short mutex for each unlink. Active writers or any intervening publication defer the remaining cleanup. The ten-minute mtime grace still retains fresh/interrupted files but is not the concurrency guarantee. This mechanism coordinates this application's threads; it is not an interprocess lock for independent applications sharing a data directory.

Privacy pause, ignored applications, and sensitive-source checks happen before persistence, file writes, OCR, or index work. Cleanup reads current config periodically, protects favorites, respects recycle-bin policy, removes database rows through repository rules, and removes only positively owned orphan resources. The retention inputs are snapshotted into a `CleanupPolicy` value (`retention_days`, `max_items`, `recycle_bin_days`) before any work starts: the cleanup walks every resource root and canonicalizes every referenced path, so holding `Mutex<ConfigStore>` across it would stall every other config-reading command and the auto-sync worker's per-tick read. `enforce_history_cleanup_with_policy` therefore takes no `ConfigStore` at all, and the scheduled worker keeps loading its own store per tick.

## Platform adapters

macOS capture tools (`pngpaste`, `imgpaste`, `osascript`, `sips`, and `plutil`)
use `bounded_command`: 600 ms per subprocess, a shared 3 s capture budget, output
caps and cancellation. Snapshot publication rejects an aborted budget.
`platform::macos_image` owns image fallback and icon conversion. Image bytes are
capped at 64 MiB before decoding; TIFF/PNG scratch files live in a unique owner-only
directory and are removed on success/failure. Converted icons are validated before
atomic publication. File size limits bound reads, not the helper's temporary disk
writes before its deadline. Portable fixture tests cover orchestration/cleanup;
native macOS tools and process-group behavior still require macOS CI/runtime.

Linux icon caching also stages source copies with `store_atomically`; failed
copies cannot leave a partial final cache file for later lookups to reuse.
Existing icon-lookup fixtures and shared partial-write regressions cover this path's
portable contracts. Native theme lookup remains a Linux runtime check.

The Linux ONNX compatibility shim's `_M_replace_cold` exports must accept the
implicit C++ `this` pointer before their five explicit arguments, for both char
and wchar_t. `scripts/test-glibc-compat.sh` exercises actual C++ member-call ABI
and overlapping replacements against copied-string expectations. A dedicated
Ubuntu 22.04 CI job runs it; Windows Rust checks cannot verify this shim.

The X11 quick-paste backend is also compiled by non-Linux unit-test builds for
type checking. Its unused-function allowance applies only to those test builds,
including macOS; Linux production code keeps normal dead-code diagnostics.

Linux monitor stop pipes use `pipe2(O_NONBLOCK | O_CLOEXEC)` so both flags are
set atomically and no partial initialization or child exec can leak a pipe end.
Wayland monitor setup bounds each protocol handshake to two seconds. Its event
loop uses prepare/read/dispatch with stop-pipe polling instead of blocking
roundtrips, and exits on compositor disconnect. A synthetic Wayland socket test
covers callback delivery, a stalled peer, stop wakeup, and disconnect.

CI rust-cache must point to `src-tauri -> target`; `cache-targets` is a boolean,
not a profile-directory list. Otherwise cache discovery looks for the absent
root Cargo.toml and dependency artifacts are rebuilt on each CI run.

Wayland source attribution shares the Sway `get_tree` / Hyprland `activewindow -j`
PID parser with quick paste. Capture includes the application's own PID; paste
filters it out. A compositor window ID must never be used as a `/proc` PID.

`platform/` contains shared traits/managers plus Windows, macOS, X11, and Wayland sources. Windows is the primary wired runtime. File presence is not proof of native clipboard, hotkey, source-app, tray, or quick-paste completion; verify runtime wiring, target-specific tests, and real-platform evidence. Keep degradation capabilities accurate in `RuntimeInfo`. Non-Windows hotkey dispatch is implemented by the historical `windows_hotkey_stub.rs` facade, `native_hotkeys.rs` (macOS/X11 main-thread registrations), `modifier_input.rs` (10 ms modifier samples), and `wayland_hotkeys.rs` (authorized GlobalShortcuts portal sessions). Each loop has a stop signal and join; queued UI operations never block the loop during shutdown. `quick_paste.rs` restores macOS/X11 targets by native id plus pid, checks focus and held modifiers/keys before injecting paste, and rejects stale owners. `wayland_paste.rs` supports Sway/Hyprland with wtype through bounded subprocess calls. A retained 150 ms foreground tracker runs independently of configured hotkeys. Modifier sampling uses x11rb so disconnects return errors instead of entering Xlib process-wide fatal handlers; stale native hotkey events are drained before replacement registrations. Portal `Session.Closed` carries a details map and clears availability. Unix instance liveness treats EPERM/inconclusive failures as live and only ESRCH as dead.

Window opacity/effects are posted to the UI thread. `apply_webview_transparency` uses Win32 alpha, macOS NSWindow alpha, or Linux GTK opacity. Tauri >=2.12.1 exposes transparency without the obsolete `macos-private-api` opt-in; do not reintroduce that target-only dependency feature or `macOSPrivateApi` config overlay, which older manifest validation compares against only the main dependency declaration. macOS maps acrylic/mica to native Vibrancy materials. `x11_effect.rs` sends/clears the KWin-compatible blur hint; Wayland blur requests return an explicit unsupported error while opacity remains available. Disabling whole-window opacity restores native alpha to 100% before CSS supplies background-only translucency.

## CLI and loopback API

The listener owns every accepted connection's socket clone and join handle.
Stop closes all retained sockets before joining handlers, interrupting delayed
request reads and response writes; an already executing database operation may
complete, but no handler outlives a successful stop. Finished handlers are reaped
during normal serving, and connection slots are released on unwinding as well.

Accepted API sockets explicitly switch to blocking mode before applying read/write timeouts.
Windows may inherit the listener's nonblocking flag; a timeout alone does not clear it, so
delayed request bytes would otherwise cause an immediate `WouldBlock` disconnect. The
delayed-request regression forces this socket mode on every platform.

`main.rs` separates GUI and process CLI execution. `cli/mod.rs` implements list/search/copy/paste/delete/export/stats over the same configured database path as the GUI. `cli/api.rs` starts only on explicit command, binds to `127.0.0.1`, enforces configured limits, and retains a stoppable listener/thread lifecycle. `item_operations.rs` owns `CopyContext`, complete-record copy/usage ordering and single-item
membership operations for native default GUI copy, CLI and API. Text/link copy writes plain
text; media resolves existing originals/managed resources through the same resolver. Missing
resources produce tagged errors, never title-text fallback. GUI remote media is materialized
before invoking copy; CLI/API require local resources. Formatted paste remains separate.
Successful OS writes stay successful if usage persistence fails (logged, no promotion).
In-process CLI/API use the shared self-trigger guard and invalidate desktop caches/events;
standalone Windows writes also set the private clipboard marker. Standalone macOS/Linux
cannot share the GUI memory guard, so cross-process self-trigger suppression remains limited.
`ExternalChangeWorker` compares `PRAGMA data_version` on the GUI's same connection every
500 ms, clears search cache and emits history invalidation after external connection commits,
including usage changes without search outbox entries. Detection is eventual and can wait
behind the database mutex. It is independent of lazy/background indexing and stopped/joined
through unified shutdown. Native WebView/OS behavior and macOS/Linux CI remain separate gates.

## Unified shutdown

`BackgroundOperations` owns at most one active tags, backup and sync slot. The
non-clone operation guard releases its slot even on unwinding; matching IDs protect
against stale cancellation. Before worker joins, shutdown rejects new operations,
cancels all slots and waits up to 30 seconds on a condition variable, logging timeout.
This registry wait is separate from the auto-sync worker's existing join fallback and
is not a bound on total application shutdown time.

Portable backup commands report progress through the registry. Binary transfers check
cancellation every 64 KiB and at EOF; validation checks entries/rows; restore checks rows
and the final pre-commit boundary. Cancellation preserves an existing output archive,
rolls back the current restore transaction and removes newly published resources.
Local filesystem calls and a single SQLite busy wait remain non-preemptible.

`stop_runtime_services()` signals sync cancellation first, waits for background-operation cancellation, then stops the external-change observer and auto-sync worker (it is a background SQLite/S3 writer that must not outlive a storage snapshot), then cleanup, clipboard monitor, capture, OCR, thumbnail, hotkey, local API, and search-sync services. A poisoned managed lock never skips a stop: the guard is recovered with `into_inner()` (matching `state.rs`) so background writers still stop through teardown. The Tauri `ExitRequested` path invokes it; ordinary window close may hide to tray. Every new worker/listener/server needs a stop signal, retained join/unlisten handle, idempotent stop behavior, integration with normal exit/tray exit/interrupt/restart as applicable, and drop/stop tests or a documented verification gap. Background threads must never call a Tauri API that blocks on the main thread while the main thread may be joining them: `refresh_tray_recent_menu` posts its `tray.set_menu` work through `run_on_main_thread` instead of calling it directly, because `set_menu` blocks on the main thread and the capture worker is joined there during shutdown. If `ctrlc::set_handler` fails (a foreign handler installed first), the failure is logged so the missing unified interrupt path is diagnosable.

## Backend change checklist

- keep business logic in focused modules and `lib.rs` as the boundary;
- return explicit errors through Tauri and avoid partial cross-resource updates;
- add focused Rust tests for repository, schema, hash, cache, worker, and path invariants;
- run `cargo fmt`, focused tests, full Rust tests, and Clippy;
- update this reference and `data-contracts.md` when runtime ownership or cross-layer contracts change.

### Portable backups and file staging

`content::file_store::store_atomically` removes its owned temporary file when
population fails, including after a partial write, and leaves any existing target
untouched. Flush/publication failures use the same cleanup guarantee.

Restored originals are published into the record role's managed root: image records
use `paths.images`, file records use `paths.files`. A shared archive entry used by
both roles gets one copy per root; remapping is cached by role and entry name.
This preserves the sync uploader's root-containment contract. Publication is lazy
over referenced entries, and cancellation/transaction rollback removes new files
from both roots. Derived previews/icons are still discarded.

Portable-backup validation distinguishes preview from restore. Both validate every row,
including duplicates, and retain identity sets for cross-row uniqueness. Preview drops each
body after validation/counting; restore retains it for the atomic apply. Progress uses a
separate processed-row counter, so EOF cancellation and manifest counts do not depend on
the retained vector length. The opt-in `export/backup/scale_bench.rs` measures actual
preview RSS/time with synthetic archives; see `docs/PERFORMANCE_V1_8.md` for limits.

`export::backup` implements version-1 `.clipbackup` ZIP bundles of active records and original resource bytes. Export uses the snapshot visitor, streams binaries and spools JSONL to disk. Derived previews/icons and configuration are excluded. Restore first copies the archive to private scratch space, validates entry names/counts, SHA-256, expanded sizes and record constraints, then remaps every known resource reference to newly published flat files under the matching managed image/file root. A strict SQLite transaction skips duplicate content and rolls back on any other failure; rollback removes the newly published files. The Tauri restore command holds storage_maintenance_lock against cleanup, clears the search result cache and invalidates history/tags after success. It reports retention limits without truncating during restore. The snapshot visitor holds the DB mutex during export; very large exports may delay capture.

`FileStore::save_file` now hashes while streaming source bytes into an exclusively created staging file (64 KiB buffer), avoiding a second full read. Hash and size describe exactly the bytes written when the file is published. If a live source grows past the copy cap, staging stops at/below that cap and the remaining source is hashed for the existing pass-through fallback; the partial stage is removed. Flush/fsync and atomic publication remain unchanged. Read/write failures remove only the owned stage. This bounds staging writes, not the lifetime or total reads of an unbounded growing source.
