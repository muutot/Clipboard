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

`logging::init` runs first in the setup hook (before config load) with an initial `Info` level: on Windows it redirects the process stderr handle (`STD_ERROR_HANDLE`, the Win32 `(DWORD)-12` value, not positive 12) into `<project>/logs/clipboard.log` (renamed to `clipboard.log.old` past 512 KiB at startup), so every diagnostic survives GUI-subsystem builds. After the config loads, `logging::set_level(config.log_level())` applies the persisted threshold, and `set_general_settings` re-applies it live. During runtime, `log_line` drops lines below the threshold, samples the log size at most every 30 s, and truncates an oversized log in place — the append handle keeps writing at end-of-file, so renaming would detach it from the path. All backend modules emit via the leveled `crate::log_error!`, `crate::log_warn!`, `crate::log_info!`, and `crate::log_debug!` macros (the historical `crate::log_event!` is an `Info` alias); each line is tagged `[LEVEL]`. Do not reintroduce bare `eprintln!` in library code (main.rs CLI paths keep raw output on purpose).

## Managed runtime state

The app manages `ConfigStore`, `StoragePaths`, `Database`, `SearchIndex`, `PerformanceTracker`, privacy/capture/self-trigger state, clipboard monitor, keyboard/hotkey managers, OCR and thumbnail workers, cleanup worker, local API server, and the optional single-instance guard. The local API server starts during setup on an ephemeral loopback port and writes the bound port to `conf/api.port` so external scripts can discover it; the start/stop/status commands remain available for explicit control. It requires every request to present the bearer token persisted at `conf/api.token` (created on first start by `commands/api.rs::load_or_create_api_token`), a loopback `Host` header, and rejects requests carrying an `Origin` header — see `cli/api/http.rs::authorize`; `/health` is the only endpoint exempt from the token. On Unix the token file is tightened to owner-only (`0600`) on every read/create, and on Windows it gets a best-effort owner-only ACL via `icacls`, so another local account cannot read it. `conf/api.token` deliberately holds the credential **in the clear** and must keep doing so: it is a documented integration point (`cli/api/routes.rs` names this exact path in its 401 body, and the file exists so external scripts can read a stable credential), so routing it through `platform::secret_store` would break both the hint and the scripts the token exists for. Confidentiality therefore rests entirely on the file mode, which is why `commands/api.rs::write_api_token` creates the file with `OpenOptions::mode(0o600)` instead of writing and then chmod-ing — a plain `fs::write` leaves a window in which the credential is world-readable, and on a volume without POSIX modes (exFAT, some network shares) the later narrowing never takes effect. Both `restrict_api_token_permissions` branches log on failure rather than discarding it with `let _ =`, because a silently-skipped narrowing is the one case the user needs to hear about. An empty or whitespace-only token file is treated as absent and regenerated, so the API can never authorize against an empty string. Connections run one thread each with a 2 s per-read timeout, a 10 s total request-read budget (`MAX_READ_DURATION`, so a drip-feed client cannot pin a thread without ever presenting a token), and a 30 s write timeout; connection slots are reserved atomically and beyond 32 concurrent connection threads new connections get an immediate 503 instead of spawning more threads.

The single-instance guard treats a failed liveness probe as _inconclusive, and therefore as running_. On Windows, `classify_open_process_failure` maps only `ERROR_INVALID_PARAMETER` (87) to `OwnerLiveness::Dead`; `ERROR_ACCESS_DENIED` (5) and every other failure map to `Running`. `OpenProcess` returns access-denied when the owner is elevated or runs as a different user, and reading that as a dead owner made a second launch delete the live instance's `instance.lock` and start alongside it — two clipboard monitors, two capture loops, two OCR workers, and two hotkey registrations on one database. Only an error that positively identifies a nonexistent pid may authorize deleting the lock file. The lock file is also published before its PID is written, so an empty/unparsable lock younger than the two-second write grace is retried instead of being treated as stale.
`platform/local_wake.rs` supplies non-Windows wake IPC: the lock owner binds loopback TCP, publishes a random token in owner-only `instance-wake.json`, accepts only a bounded authenticated wake request, and joins its listener on drop. Late teardown removes metadata only when its token still matches. Wake callbacks post window work to the main thread to avoid a join/UI deadlock. Wayland activation can still be declined by the compositor.

Use `Mutex`/`Arc` according to existing ownership. Never hold a config or ingestion lock across slow filesystem, network, or UI work unless the operation explicitly requires atomicity.

## Database and repositories

`src-tauri/src/storage/` owns SQLite schema, forward-only migrations, repositories, recovery, and derived-data coordination. `Database::from_connection` enables foreign keys, WAL, normal synchronous mode, memory temp storage, cache/mmap settings, initializes or migrates the current schema, and persists one UUID device identity. Schema v1 is the one-time clean baseline; later versions use the adjacent transactional chain in `storage/migrations.rs`, while newer-than-supported databases are rejected untouched. `crates/clipboard-sync/src/v1/repository.rs` owns the provider-neutral replication DTOs and `SyncRepository` contract; `storage/sync_repository.rs` adapts `Database` to that contract and owns point-in-time SQLite snapshot creation. `storage/sync_state/` remains the compact SQLite implementation: it preserves clipboard rows as first-snapshot input, assigns deterministic `(modified_at_ms, sync_writer_device_id)` versions, maintains the outbox/tombstone/publication/cursor/checkpoint/resource-reference tables, restores remote keys only for scope-aware publication, and applies streamed snapshots, segments, or a full checkpoint atomically with echo suppression. Remote image/file/icon keys never masquerade as local paths in `clipboard_items`; their role/ordinal references commit with the row and cursor so ordinary pull has zero blob downloads. The v1 command orchestrator calls `Database::initialize_sync` through the provider-neutral engine; initialization does not read or convert obsolete sync data.

`storage/recovery.rs` validates SQLite integrity, rotates current/previous backups, quarantines damaged files, restores the first valid backup, and causes the derived search index to be quarantined/rebuilt after recovery. A deliberate schema reset deletes obsolete backup generations before writing a fresh v1 backup, so discarded rows cannot return through recovery. Persistence changes must preserve atomic config writes, recovery, backup refresh, and rebuildability of derived data.

Corrupt configuration is moved to a UUID-suffixed quarantine before defaults are
loaded. A failed move returns an error with the original untouched; it must not
delete the only recovery copy or claim that default settings were persisted.

`export::stream` writes JSON/CSV/plain text incrementally from `Database::visit_active_items`, a single read transaction with one materialized row. GUI and CLI file exports use a buffered temporary file, fsync and atomic replacement; write failure preserves the destination. IPC/stdout compatibility wrappers still return a complete String. The visitor holds the database mutex and must not reenter it. A real WAL regression mutates recency on a second connection while exporting and verifies complete unique IDs and original snapshot values.

Plain-text import recognizes a `---` delimiter line with LF or CRLF endings, while preserving line endings inside each record. Delimiters at the file boundary create empty chunks that are ignored; they never create clipboard records. The CRLF regression imports two records and verifies the first record's internal CRLF bytes. Export escapes a content line that is exactly `---` as `\---` and doubles a leading backslash on any content line; import reverses that escaping, so a record containing a delimiter-looking line round-trips as one record.

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

- `crates/clipboard-sync/src/s3/` (`signing.rs`, `client.rs`, `objects.rs`, `listing.rs`, `mod.rs`, `tests.rs`) owns AWS SigV4 request signing, paginated object listing (including decoded listing ETags, buffered under a hard object-count cap so a runaway namespace fails loudly instead of exhausting memory), conditional writes, streaming file upload/download, ETag handling, and S3-compatible connection testing. `S3ObjectStore::with_metrics` may attach shared atomic diagnostics that count actual HTTP PUT/GET/HEAD/LIST/DELETE requests (including LIST continuation pages), payload bytes, and per-method elapsed time; normal production construction leaves metrics disabled. Its v1 modules own the isolated namespace, provider-neutral replication engine (`v1/engine/` split into `mod.rs` entry points plus `transfer.rs`, `checkpoint.rs`, `publish.rs`, and `pull.rs`), object-store and repository contracts, frozen protocol DTOs (`v1/wire/types.rs`), encryption/compression/pack codecs (`v1/wire/crypto.rs`, `v1/wire/packs.rs`), and content-addressed image/file/icon resource processing (`v1/resources/store.rs`, `v1/resources/mutations.rs`) without depending on Tauri or application storage. Checkpoint CAS publication must read the written pointer back before GC or local generation advancement; a missing, changed, or regressing pointer is a hard failure. The engine receives only `SyncRepository`, `ObjectStore`, and `SyncEnginePaths { temporary_directory, resource_roots }`; it cannot see the desktop project, database, search index, or cleanup layout. It owns metadata-only bootstrap/incremental pull, checkpoint recovery, CAS compaction, and vector-bounded garbage collection described in `docs/SYNC_V1.md`.
- `src/sync/v1/` owns only explicit desktop-domain DTO/path adapters and SQLite-backed integration tests for the crate engine; `storage/sync_repository.rs` supplies the production SQLite implementation. Snapshot/checkpoint publication first makes a point-in-time SQLite copy, releases the live database lock, exports deterministic bounded batches into one temporary chunked pack, then performs one streaming S3 PUT. That copy lives in the data directory; startup removes stale `.sync-database-snapshot-*` files older than an hour so a crash cannot leak a full plaintext database forever; pull performs one streaming GET and applies every decoded chunk plus cursor/checkpoint state in one rollback-safe SQLite transaction. Segments stay on the compact single-envelope path, so normal daily sync request counts are unchanged. Preview images remain device-local derived data and are stripped at the storage export boundary. With a sync password, resources use a keyed plaintext identity and fixed 1 MiB AES-256-GCM chunks, adding a 20-byte header plus 16 bytes per non-empty chunk; upload/download remain file-streamed and bounded to one chunk of encryption memory. Pulling a pack validates and transactionally records remote resource references but does not download blobs; scope-aware republishing reuses those references even before materialization. Each run performs one paginated heads listing. Once a head has been validated/applied, a disposable `sync_metadata` cache may skip its body GET only when listing ETag/size and local publication state or peer cursor all still match; missing ETags, malformed cache, recovery changes and mismatches conservatively GET/decode the head. Fresh devices authenticate existing canonical pointers before their first publication, then apply the global checkpoint before peer heads; wrong/missing/changed passwords and encryption-mode mismatches fail without writing a new head. In-place password rotation is unsupported; key rotation moves the scope to a fresh prefix via `rotate_sync_namespace` (below). Unavailable snapshots or non-contiguous segment chains force checkpoint recovery and one retry, with fallback to the retained previous checkpoint. A local checkpoint-vector baseline prevents checkpoint pointer/body reads on idle runs; compaction runs after 50,000 aggregate new-history units, a known-device removal, or an existing-device epoch change/regression. A peer that failed to pull does not disable the checkpoint for the namespace — the checkpoint is the only recovery artifact for pruned history, so one permanently broken peer would otherwise leave every peer without a way back; only the collection of the history the previous checkpoint covered waits for a healthy pull, so a lagging peer keeps its segment-chain recovery path. A newly observed device contributes the larger of its trusted bootstrap record count and published sequence, so an empty peer does not immediately rewrite the full checkpoint. The CAS winner deletes history covered by the previous checkpoint vector, retains current plus previous checkpoints, preserves same/newer-generation candidates against delayed cleanup, and records its local baseline after GC so interruption remains retryable. Before publishing, the engine reconciles the local device's remote head through the same cache-or-GET path; a remote head that outruns the local publication state within the same epoch and snapshot is adopted in place (the gap segments were encoded from the local outbox, so one publication commit closes the crash window without rotating the epoch), and the orphan-segment scan lists the device prefix only while the outbox still has pending rows, because an orphan exists only until its encoding rows are committed, and an orphan is adopted only when the still-pending range matches its complete mutations, versions and canonical resource identities (an old backup can reuse the same sequence numbers and item IDs for different edits; an ID-only acknowledgement would drop the new local work); a restored/divergent local publication state still re-applies remote history, rotates epoch, and republishes a complete snapshot instead of overwriting a newer head. Remote-head failures are isolated per device: healthy peers continue, the result reports `failedPeers`, and only head-namespace discovery failure aborts the whole pull pass.

Checkpoint publication can check resource references, gated by `SyncEngineOptions.resource_gc_mode` (`off`/`referenceScan`/`manifest`). It runs after checkpoint pruning and pointer read-back verification. `referenceScan` decodes every surviving checkpoint, snapshot, and segment and validates their resource keys. `manifest` reads `v1/resource-refs/<generation>-<sha>.json` sidecars written when that mode publishes new checkpoints and falls back to a pack decode when a sidecar is missing or invalid. **v1 never deletes resource bodies:** a peer can HEAD-reuse an old content-addressed object and publish a new reference after the scan, and neither a timestamp grace nor a second scan fences its publication against DELETE. Safe body reclamation requires a publication/deletion protocol. Only sidecars for absent checkpoints strictly below the verified head generation are removed; current/newer-generation sidecars survive delayed collectors. Both modes and the settings note explicitly retain unused bodies. The real-engine regression publishes a peer snapshot after the last scan listing and verifies that the reused old resource is still downloadable in both modes.

- `commands/sync/mod.rs` exposes `get_sync_config`, `set_sync_config`, typed S3 connection testing, `sync_now`, `rotate_sync_namespace`, and the UI-facing `materialize_clipboard_item`. Sync runs snapshot config before I/O, derive one optional remote-scoped `SessionKey`, construct one scoped `S3ObjectStore`, and scope the current operation token around `sync_database_with_progress` for manual and automatic runs. On-demand materialization reuses verified local files, deduplicates concurrent downloads with weak process-local locks, writes all item paths atomically without replication/version changes, and queues a local thumbnail rebuild for images.
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

`sync/v1/s3_smoke.rs` holds opt-in real-server regressions that `MemoryStore` cannot prove:
a thin ObjectStore decorator cancels after a successful immutable snapshot/segment PUT (the
real head must stay unchanged; retry preserves pending rows and converges), an encrypted
image resource must upload as multi-chunk ciphertext, defer its body during an ordinary pull,
and materialize byte-identical on the peer, a rotation tombstone at the prefix root must
make a stale peer refuse, the full rotation flow (materialize-all → publish at the new
prefix → tombstone → delete the old namespace → byte-identical body) must hold over the wire
(`real_s3_namespace_rotation_moves_state_and_tombstones`), the opt-in GC modes must publish
and prune checkpoint sidecars, fall back to pack decoding, and retain fresh orphans through
the server-`LastModified` grace window (`real_s3_resource_gc_reads_sidecars_and_retains_fresh_resources`),
the typed connection probe must answer success / 403 wrong-secret / 404 missing-bucket
(`real_s3_connection_probe_reports_success_and_failure_modes`), the streamed resource
download must refuse a body above the caller limit from its declared `Content-Length`
without leaving a partial file (`real_s3_streamed_download_enforces_the_limit_and_leaves_no_partial_file`),
and PUT/GET/LIST/HEAD must agree on the raw quoted ETag text
(`real_s3_conditional_writes_and_etags_match_the_transport_contract`). CI runs the smokes through
`.github/workflows/sync-s3.yml` (ubuntu + checksum-verified pinned rustfs); locally use
`scripts/sync-s3-test.ps1 -Test smoke`, which exports the password. None of it proves native
exit timing.

Every sync run first reads `{configured_prefix}/rotated-to`
(`commands/sync::refuse_if_prefix_rotated`) and refuses before namespace resolution when the
tombstone exists: repairing the old binding from cache would bootstrap a second namespace
under the old password and fork the scope (design: `docs/SYNC_V2.md` D2). The check runs for
manual and automatic runs alike and must stay ahead of binding resolution.

`rotate_sync_namespace` replaces the scope's key (password change or plaintext toggle) by
rotating to a fresh derived prefix: materialize every referenced resource of the old scope,
claim `{old}-rotated-{unix_ms}` with the new key, republish the complete local state, write
the create-only tombstone (new prefix + key check + device id) at the old prefix root,
rewrite the local configuration **before** deleting the old `v1/` objects (the tombstone
makes every run at the old prefix refuse, so an interrupted deletion must not strand the
device), and leave any leftover old objects as bounded garbage. Any failure earlier leaves
the old prefix and password configured; a tombstone that already exists refuses the run.
The engine stays unaware of rotation — the command owns the two stores and the deletion
boundary.

Sync interruption recovery uses
`SyncRepository::sync_outbox_range_matches_mutations(remote_scope, after, through, mutations)`.
The SQLite adapter reads pending IDs and their current mutations in one WAL snapshot,
with the ID query capped to the expected count plus one. It compares full payloads,
versions and tombstones after deferred-resource normalization, and separately compares
canonical resource references. Preview/cache paths do not create false differences;
resource identity does. An old backup can reuse the same sequence and item IDs for
different edits. Unmatched orphan segments are discarded and live work republished;
unmatched published heads use pull/reset/bootstrap recovery. A later edit of the same
item may conservatively decline adoption and increase traffic. No IPC, schema or v1
wire change is required. Real backup/restore UTs cover both interruption windows,
with plaintext/encryption and genuine media-orphan retry preserving canonical refs.

`storage/sync_run.rs` owns each database's complete sync lease. The engine acquires
`SyncRepository::acquire_sync_run` before initialization or remote I/O and holds it
through publication, pull and compaction. File DBs use the canonical DB sibling
`.sync-run.lock` with an exclusive nonblocking OS lock; in-memory DBs use their
own mutex. Different connections/processes of the same DB fail fast on overlap;
different databases remain independent. Failure/drop/crash releases ownership and
the persistent lock file is never removed by a run. Coordination errors refuse
the run before remote effects. `begin_storage_migration` acquires the same file
lock before its SQLite writer reservation, retaining both until restart on success
or releasing both on failure. The process-wide command mutex still supplies UI
serialization; it is insufficient alone for independent app processes. Real-engine
UTs reproduce an old head overwriting a successful newer head; native child UTs
cover cross-process exclusion and abrupt exit. This is per-database coordination;
independent copies with the same remote device identity are outside this guarantee.

A successful data-directory migration creates an independent SQLite database and
retains the source for older instances/recovery. After path rewriting, the target
calls `fork_sync_identity_for_migration`: one transaction assigns a fresh device
UUID and clears snapshot/sequence/epoch publication state for every scope, keeping
remote preparation, sync_enabled, row versions, outbox, cursors and resource refs.
The next target run bootstraps a complete snapshot under its own device namespace;
the source retains its UUID. This prevents a still-running source and the migrated
copy from treating one another's in-flight segment as an unrelated orphan and
publishing an unreachable head. The target UUID persists across normal restarts.
Identity/reset failure rolls back and the existing migration quarantine handles
the failed bundle. Migration increases first-run metadata upload and leaves the old
remote device namespace available for recovery; it never remotely deletes that peer.
This fixes migration-created copies; arbitrary externally copied active DBs remain
outside the per-path sync lease guarantee. UTs cover real migration/restart with
interleaved old/new sync, record/version/outbox/resource preservation and reset failure.

## Storage paths and ownership

`src-tauri/src/storage/paths/` owns resource-root resolution/validation and metadata; `databases.rs` owns bounded additive database claims.

Resource-root safety is mandatory:

- default roots are application-owned;
- uninstall only deletes image/file roots resolved as owned: unconfigured defaults
  under the project root are application-owned, while defaults under a custom data
  directory and explicit resource paths require a matching project/role marker;
  an explicit path equal to a project default still requires that marker. Database,
  icons and models retain their separate options. Unmarked/foreign resource folders
  are preserved even when the user selects data deletion;
- custom roots require an explicit `.clipboard-resource-root` marker before orphan cleanup is enabled;
- startup may use an unmarked custom root but must not auto-claim it; registering database coordination metadata does not grant ownership;
- the Windows uninstaller resolves the storage root plus custom image/file roots, and deletes a custom root only when its marker matches the project key and role; an invalid/missing marker is retained even when "delete clipboard data" is checked;
- explicit settings may claim only a safe empty root or validate a matching marker;
- the `set_resource_ownership(owned)` command is the explicit recovery action for an already-used unmarked root: it writes or removes the marker under the persistent root coordination lock (never deleting or touching resource contents), skips roots that do not require a marker (default project data directories), and reports `restartRequired` only when the resulting ownership differs from the cleanup flags captured at startup (an off-and-on round trip in one session needs no restart);
- image and file roots must not overlap each other or reserved project/data/database/index/icon paths;
- cleanup skips the ownership marker, `.clipboard-resource-root.lock` and `.clipboard-resource-root.databases`; each image/file/preview unlink requires a single registered database under the root lock, and custom roots also require current marker validation;
- every orphan-cleanup entry point (scheduled `CleanupWorker`, `enforceHistoryCleanup`, `cleanupStorageFiles`), the `renameItem` command, and `permanentlyDeleteStorageKind` (whose in-run orphan cleanup uses zero grace) hold `CaptureState.storage_maintenance_lock` for the run: a rename copies a new body before saving its paths, so cleanup must not reclaim that temporarily unreferenced copy before commit. Commands that pair it with `ingestion_guard` must acquire maintenance first (the storage-root migration order). This lock is deliberately separate from `ingestion_guard` so a long scan never stalls the capture loop.

Changing data directories is a migration workflow, not a path-string edit. Keep managed/external path rewriting, database backup, search-index derivation, and concurrent ingestion/worker coordination in scope. The webview asset protocol's static scope is empty; at startup the app grants the managed storage root plus any configured image/file roots with `asset_protocol_scope().allow_directory`, so previews and thumbnails render even when a custom resource directory lives outside `storage`, and the install directory — where `$EXE`/`$RESOURCE` both resolve on Windows and where `conf/api.token`, `conf/conf.json`, and the database live — is never exposed. Migration must be retryable: `VACUUM INTO` refuses an existing destination, so `migrate_storage_data` first moves the existing database and its `-wal`/`-shm` sidecars into an exclusively created `clipboard.sqlite3.pre-migrate-<uuid>` directory. A partial quarantine failure restores earlier moves; a snapshot, open, or path-rewrite failure preserves the failed database bundle in a separate quarantine before restoring the originals. Rollback refuses conflicting destinations and reports the remaining backup paths on failure. Successful migration retains the old bundle for recovery, and rapid retries cannot overwrite earlier backups. Directory copies and size walks track canonical ancestor paths (`copy_dir_contents`/`dir_size` in `commands/config/storage/migration.rs`) so a junction/reparse cycle cannot recurse until the stack overflows.

`configure_storage_directory` first takes the same process-wide try-lock as manual/automatic S3 sync, refusing an active run before stopping services. After shutdown it drains `storage_maintenance_lock` and `ingestion_guard`. `migrate_and_save_storage` then reserves the source SQLite writer with a separate `BEGIN IMMEDIATE` connection before copying resources; this waits for an existing transaction and blocks later writes from independent connections, including CLI/API requests that escaped worker shutdown. The snapshot uses `snapshot_into` without a checkpoint (which would conflict with the reservation). Only a successful snapshot plus config save retains the empty transaction inside the managed `Database` until restart; copy/snapshot/save errors release it. Reads remain possible, while post-migration writes fail instead of being silently left in the old database. Real multi-connection tests cover drain-before-snapshot, writes during config save and after success, and copy/save failure release. The shutdown timeout alone must never be treated as proof that migration writers have stopped.

Migration resource copies never overwrite an existing destination. `copy_migration_file` reserves each new file with `create_new`, streams and flushes it, and cleans up only that newly created file on failure. An existing byte-identical file is a valid retry; differing content fails with the destination path so a pre-existing database's resources are preserved. Comparisons use bounded buffers. `copy_dir_contents` compares canonical roots before traversal: aliases of the same root are no-ops, while ancestor/descendant roots are rejected to prevent copying the newly created destination into itself.

Storage migration additionally holds `Database::begin_resource_write_at` for the
destination before copying any image/file/preview/icon body and until the snapshot's
rewritten references commit. The source SQLite writer reservation does not fence
another instance's destination cleaner. Destination publication initializes its
existing-parent path coordination without creating a placeholder SQLite database;
later opens reuse the same coordinator and invalidate pre-copy cleanup snapshots.
Failure releases the destination guard; success releases it after reference commit.
UTs interleave an independent destination cleaner between copying and DB replacement,
verify both file/icon references, and confirm ordinary orphan removal resumes.

## Search

Relative-week search subtracts calendar dates before resolving local midnight; it must not subtract 24-hour durations from the current instant. `week_start_date` is tested against New York, Berlin and Lord Howe DST boundaries with the test-only `chrono-tz` dependency, without changing the host timezone.

`search_clipboard_items` and `rebuild_search_index` are async commands whose SQLite/Tantivy work runs inside `tauri::async_runtime::spawn_blocking`; neither may block the main event loop.

Tantivy uses the schema/query modules and a CJK-friendly n-gram tokenizer. SQLite search triggers append `search_outbox` operations; `SearchSynchronizer` drains them. Outbox draining runs lazily inside `search_clipboard_items` by default or in a live-switchable `SearchSyncWorker` when `GeneralConfig.search_index_sync_mode` is `background`; the worker also retries a required full rebuild (`SearchIndex::requires_full_rebuild`) each tick. The outbox is mutated only by the synchronizer: history cleanup must never prune it, because deleting the `delete` events for hard-deleted rows would leave their Tantivy documents indexed forever. Full rebuild clears/recreates derived index state and repopulates from SQLite. ``SearchIndex::open`retries a failed open once after a short delay before falling back to delete-and-recreate, so transient interference (antivirus locks, permission hiccups) does not trigger a minutes-long full re-index.`SearchIndexLayout::prepare`applies the same distinction to the manifest:`read_manifest`returns`Err`for "present but unreadable" and`Ok(None)`for "absent", and only a manifest that could actually be read (or an index directory that is already gone) may authorize`remove_dir_all`. An unreadable manifest with a healthy index is logged and deferred instead of deleting the index. Read `search-cache-strategy.md` before changing pagination or query caching.

## OCR

`get_ocr_status`, `check_ppocr_status`, `set_ocr_config` and `restart_ocr_engine` run their process probes, digest checks and worker joins on `spawn_blocking`, not the main thread.

Worker stop revokes a per-worker publication mutex before the bounded join.
Claims, reused results, successful inference and terminal failure writes share
that mutex; inference itself runs outside it. A timed-out old engine may finish
computing, but cannot write after revocation or overwrite a replacement worker.
The running flag records actual thread lifetime, while `is_running` also checks
publication permission. Concurrent stop callers wait for actual completion up
to the same bound instead of treating the stop request as thread completion.

`OcrEngine` supports PP-OCR, Tesseract, and no-op implementations. `OcrWorkerManager` owns a replaceable worker so engine/model/threshold changes can restart OCR without restarting the app. `OcrWorker::start` and `OcrWorkerManager::start`/`restart` return `Result`, so a thread-spawn failure degrades to unavailable OCR instead of panicking the app. OCR rows are recoverable jobs, share image-hash results, and feed search through the outbox. Model downloads verify size and the pinned upstream SHA-256 (each `PpOcrModelFile.sha256`, taken from the GitHub release asset digest) before activating the file, and only then record the digest to `<model>.sha256` (`ocr/models.rs::record_model_digest`) so a later install redownloads when the on-disk file stops matching. Digest checks are memoized by file length/mtime (`ocr/models.rs::digest_cache`), so the periodic OCR status and memory-diagnostics polling does not re-hash unchanged ONNX weights. A replaced asset, intercepted TLS stream, or corruption is rejected instead of reaching the ONNX runtime; updating a model means updating the pinned size/digest in `ocr/models.rs` together with the release tag. The Tesseract engine runs the CLI with a fixed 120 s per-image timeout; its stdout/stderr must be drained on dedicated threads while waiting (`run_tesseract_with_timeout` → `wait_for_child_with_drain`), because polling `try_wait` without reading the pipes deadlocks the child as soon as the recognized output exceeds the OS pipe buffer. Keep config, model installation/status, fallback selection, worker lifecycle, database transitions, search synchronization, settings progress, and shutdown aligned.

`OcrWorkerManager` also owns a separate settings-operation mutex shared by clones. `commands/ocr.rs` holds it across resolution, engine preparation, persistence and restart, while releasing the global config lock before probes and joins. Settings, explicit restart and post-install activation use the same apply path. A controlled post-save delay tests competing applies and the resulting worker's model version; save failures release the gate without replacing the worker. Existing stop signals, publication revocation and join handling remain in `OcrWorker`.

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

`commands/config/misc.rs::set_source_mapping_rules` broadcasts a null
`source-mapping-rules-changed` invalidation only after a successful config save,
with the mutex released. Receivers read the current rules rather than trusting
a delayed config snapshot. The shared save helper tests disk/memory ordering
and the no-notification rollback path; native multi-WebView delivery remains a
separate runtime check.

The content-dedup upsert in `storage/repository/helpers.rs::insert_item_row` treats text/link `web` metadata as one value owned by the latest capture: it removes the old blob before merging incoming metadata, or clears it when none arrives. This prevents non-browser recaptures and pages without SourceURL from inheriting an old URL/domain while retaining user tags, custom titles and unrelated root metadata. The saved record and derived tag mirror commit together; image/file metadata retains its existing merge policy.

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

Windows native reads validate HGLOBAL sizes before copying: 64 MiB for text/HTML/RTF, 4 KiB for the private self-trigger marker and 512 MiB for DIB input. Every Windows clipboard reader, including the private self-trigger marker that decides whether a change is the application's own write, opens the clipboard with the same bounded retry (`platform/windows/access.rs::open_clipboard_with_retry`): a single failed `OpenClipboard` would turn a clipboard that another application holds for a moment into a false negative, so the application's own copy would be captured as a new history entry. DIB and HBITMAP dimensions share the 16,384-axis and 512 MiB decoded RGBA ceiling; only declared pixel rows are converted. HBITMAP reads require all requested rows and use one DIB allocation. These are native safety ceilings; configured capture/persistence limits still apply afterwards. Synthetic DIB/header tests exercise the limits without touching the system clipboard; the single Windows test that writes the real clipboard (`read_clipboard_file_paths_reads_a_cf_hdrop`) runs only when `CLIPBOARD_ALLOW_REAL_CLIPBOARD_TEST` is set, which CI's Windows runner does and local `npm run verify` / `ci:local` runs do not, so a developer's clipboard is never overwritten.

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

`commands/config/misc.rs::apply_privacy_settings` validates patterns first, then
holds the privacy mutex across persistence and both runtime-policy mirrors.
Its lock order is privacy -> config, matching pause and getters. A poisoned
privacy manager rejects the update before writing; concurrent saves wait for
the previous capture-policy application. Disk failures preserve the old patterns.
Tests exercise the real ConfigStore, PrivacyManager and CaptureState with a
blocked policy write, poisoned runtime and failed config save.

`apply_auto_tag_rules` retains the config mutex from its normalized save through
the capture-policy update. Concurrent callers cannot publish a newer config ahead
of an older runtime application. Validation and save failures retain the old
running rules. Its regression blocks the real policy write and observes only
config-file size during concurrent saves, avoiding Windows reader sharing that
would itself prevent replacement.

Tray pause toggles hold the privacy-manager lock through the capture update and config save. A failed save keeps the requested session state, emits `privacy-pause-changed`, and opens a non-blocking native warning that explains the restart risk even with the main window hidden. Settings-command toggles use the same lock ordering but retain the previous state on save failure. The tray warning's English/Chinese strings intentionally live in Rust rather than the WebView catalog; they follow `general.language` and the OS locale for `system`. Native dialog appearance still needs runtime verification.

`storage/resource_publication.rs` coordinates this application's connections and processes keyed by the canonical database path. Capture (including icons), JSON/PPaste/portable-backup import, record duplication, sync/materialization and thumbnail jobs hold a publication guard from before resource lookup/reuse through the DB commit; repository save/preview methods also register publication. File databases use two siblings: `.resource-writers.lock` carries a shared OS lock for each active writer, and `.resource-generation` holds an eight-byte little-endian counter incremented under its own exclusive lock. Cleanup briefly tries an exclusive writer lock to snapshot/check that counter, including before each unlink; directory scans and reference resolution hold no file lock or publication mutex. Active writers or any intervening publication defer the remaining cleanup, including a writer that finished in another process during the scan. Nested/parallel writes use independently opened handles; close or abrupt process exit releases locks. Coordination I/O failures reject publication and defer cleanup; malformed counters are never replaced with a default. In-memory databases retain the local count/generation only. Counters need process visibility, not per-record fsync, because no cleanup snapshot survives reboot. The ten-minute mtime grace retains fresh/interrupted files but is not the concurrency guarantee. Temporary sync snapshots remove their coordination siblings on drop; data migration creates independent coordination at the destination instead of copying the source locks. The standard-library file-lock APIs require Rust 1.89 or newer. Windows UTs cover actual child-process publication and abrupt exit; matching macOS/Linux CI remains required. Independent tools that bypass the protocol are outside this guarantee. The separate resource-root database registry (below) disables deletion when different databases share a root.

File renames hold the database's publication guard from before reading the row
until a completed new-body copy and its path/title commit. Known shared files
receive a display-title change only. A currently unshared body is copied with
`create_new` and flushed before saving any new path; the old file stays intact
for duplicates/captures that already read it but have not committed a reference.
Only reference-scanned orphan cleanup may reclaim that old file. Copy failures
remove only the newly created partial destination and keep the row unchanged;
save failure leaves a complete unreferenced copy for cleanup. This temporarily
uses another body-sized amount of storage and adds copy I/O. The maintenance
mutex orders same-instance operations; publication fences independent cleaners.
Real independent-Database UTs interleave duplicate's read/save with rename,
exercise cleanup during copy and after commit, reject target conflicts, and
inject copy/save failures without changing the source paths.

Resource-root ownership actions and per-file orphan deletion share an exclusive
`.clipboard-resource-root.lock` inside that root. Cleanup tries the lock without
waiting and revalidates required project/role ownership and database exclusivity through one
unlink; revoked, transferred, busy or unreadable ownership defers that root.
The preview directory uses its image root's lock. Claim/revoke wait for a current
unlink; a stale revoke does not remove a foreign marker. Enabling a previously
ineligible root still requires startup, while revocation now prevents subsequent
custom-root deletion immediately. Configuration checks empty roots again under
the lock; the coordination file is excluded from that check and orphan/rename
operations. Marker bytes remain v1, so uninstall validation is unchanged. Database claims below additionally protect roots shared by different databases, including the same project/role marker after a data migration. UTs use independent projects/databases, a held unlink
guard, stale revoke and busy/invalid coordination; Unix CI remains a separate gate.

Every image/file resource root retains `.clipboard-resource-root.databases`
(version 1 JSON: `version`, sorted `databases`). `paths/databases.rs` registers the
canonical database path under the same root lock before any initializer returns,
including default and unmarked roots; this is coordination metadata, not ownership.
A root used by multiple database paths cannot be scanned as one database's orphan
space. Its startup cleanup flag is false, and every unlink rechecks the sole-path
claim under lock so a second initializer invalidates an earlier snapshot's permission.
Previews use their image root. Aliases normalize to one database identity.
Registration is additive and is never reset by claim/revoke or pruned by database
age/existence. Inactive databases and migration attempts may therefore keep a shared
root's orphan deletion disabled; use distinct roots when reclamation is required.
Read validation is bounded to 64 KiB and 64 identities; malformed/unknown/oversized
metadata rejects initialization, and missing/unreadable/invalid metadata defers an
already-running cleanup. Metadata writes use exclusive UUID staging and atomic
replacement. Migration copies skip database claims and root locks, because target
roots own their coordination; reference bodies and v1 marker bytes are unchanged.
Real UTs cover migration/restart plus an older instance's late write, a late join
of a default root, invalid/missing claims, aliases and independent-root migration.
These guarantees require participating Clipboard versions and intact coordination;
external tools that replace/remove the registry bypass the protocol.

Ownership marker publication uses an exclusively created UUID staging file,
flushes it, then atomically replaces the marker's directory entry under the root
lock. It never writes through an existing marker link: a hard-link alias of an
unrelated external file keeps its original bytes. Failed writes/replacements
retain the old entry and remove only owned staging. Both explicit claim and
configuration-time empty-root claims use this helper. Marker v1 bytes and IPC
remain unchanged. Real hard-link and conflicting-directory UTs cover alias
preservation, failed publication and staging cleanup.

`commands/files/replace.rs::replace_icon_file_at` validates the picked image and target name/symlink, then copies into a temporary sibling and uses `content/file_store.rs::store_atomically` to flush and replace it. Selecting the cached target itself or a hard-link alias succeeds without copying onto the source inode; a failed replacement preserves the existing destination and removes staging. The Tauri command and frontend arguments are unchanged. Windows UTs exercise self/alias replacement and a conflicting destination directory; matching Unix CI remains necessary.

`FileStore::save_file` verifies an existing body against the immutable staged source before reusing it; `save_screenshot` and `save_bytes_atomically` compare with the supplied bytes. Equal length is only a fast rejection check, never proof of identity. Reads use bounded buffers and stop at the expected stream's end, so growing existing files cannot force unbounded comparison. Missing/changed bodies are atomically replaced, identical cache hits retain their mtime, and comparison failures propagate while removing staging. UTs cover same-length corruption and byte differences across buffer boundaries. Reclamation/publication guards are still the caller's responsibility.

`ConfigStore` saves use optimistic file revisions and an exclusive `conf.json.lock` OS lock shared by Clipboard instances. Startup creation/quarantine uses the same lock; the scheduled `load_existing` reader creates nothing. A stale complete-config save is rejected before atomic replacement, so a later display/geometry change cannot overwrite another instance's privacy, retention or storage policy. Setter rollback keeps local/runtime state unchanged and the error requests a restart before retrying; cross-instance runtime policy synchronization is not implied. The revision advances only after a successful replacement. Concurrent-store UTs verify one winner per shared revision and preservation of another store's sensitive-application rules across repeated rejected saves and explicit reload.

Privacy pause, ignored applications, and sensitive-source checks happen before persistence, file writes, OCR, or index work. Cleanup reads current config periodically, protects favorites, respects recycle-bin policy, removes database rows through repository rules, and removes only positively owned orphan resources. The retention inputs are snapshotted into a `CleanupPolicy` value (`retention_days`, `max_items`, `recycle_bin_days`) before any work starts: the cleanup walks every resource root and canonicalizes every referenced path, so holding `Mutex<ConfigStore>` across it would stall every other config-reading command and the auto-sync worker's per-tick read. `enforce_history_cleanup_with_policy` therefore takes no `ConfigStore` at all. The scheduled worker uses `ConfigStore::load_existing` per tick: missing, invalid or unreadable config logs a failure and skips the run without creating, quarantining or replacing anything. Only startup uses the recovering loader. This prevents a failed policy read from changing a user's retention/capacity to defaults and deleting records. A later valid policy is applied on the next tick; real-worker UTs cover both invalid/missing policies and resumption.

## Platform adapters

Manual text-copy adapters (`pbcopy`, `xclip`, `wl-copy`) call `bounded_command::wait_for_success` after writing stdin. Spawn/write/wait failures and non-zero or signal exits propagate to the copy caller; a completed failing helper cannot report successful copying. The shared fixture consumes stdin and exits 0/7 without touching a native clipboard. This status check adds no timeout, and macOS/Linux native compilation/runtime remain platform gates.

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

`platform/` contains shared traits/managers plus Windows, macOS, X11, and Wayland sources. `ForegroundApp` carries `name`, `exe_path` and `window_title`; only the Windows adapter fills `window_title` (raw foreground window title for web-source attribution) — other platforms keep it empty and source attribution degrades to app name only. The `PlatformClipboard::read_clipboard_source_url` trait method (default `None`) extracts the Chromium `SourceURL:` CF_HTML header line on Windows via `windows/html.rs::parse_cf_html_source_url`, whose scan is bounded to the header region so copied markup mentioning "SourceURL:" cannot shadow the real header; `ClipboardSnapshot` carries it as `source_url`. The capture worker writes web-source attribution into text/link `metadata_json` via `commands/capture/web_source.rs::web_source_metadata` (pure, cross-platform): only when the foreground exe stem is a known browser does it emit `{"schemaVersion":2,"web":{"title","url","domain"}}`, where `title` is the raw window title minus the browser display-name suffix and `url`/`domain` come from `SourceURL`; other apps keep `metadata_json: None`. `platform/os_locale.rs` (`get_os_locale`) reads the OS UI locale for the `system` language setting — Windows `GetUserDefaultLocaleName`, macOS CoreFoundation (`platform/macos/locale.rs`), Linux `LC_ALL`/`LC_MESSAGES`/`LANG` — because the webview language follows the Chromium accepted-language list rather than the display language. macOS native declarations live in `platform/macos/ffi.rs`: one `ApplicationServices` block (CoreGraphics/Carbon/Accessibility/NSStatusBar) and one `CoreFoundation` block (`CFRelease`, `CFLocaleCopyCurrent`, `CFLocaleGetIdentifier`, `CFStringGetCString`) — CoreFoundation is not re-exported by ApplicationServices, and `locale.rs`/`quick_paste.rs` call through `platform::macos::ffi` instead of declaring their own. A wrong FFI symbol name only surfaces as `ld: symbol(s) not found` on the matching platform's CI (`docs/PITFALLS.md`). Windows is the primary wired runtime. File presence is not proof of native clipboard, hotkey, source-app, tray, or quick-paste completion; verify runtime wiring, target-specific tests, and real-platform evidence. Keep degradation capabilities accurate in `RuntimeInfo` and in `PlatformInfo.platform_notes`. The Wayland profiles are classified from session environment variables (`WaylandCapabilities::detect` / `classify`) rather than a live protocol handshake, so keep them aligned with the `linux/wayland/mod.rs` compatibility matrix and treat them as reporting, not as proof that the clipboard or shortcut paths work. On macOS the accessibility note is driven by a real `AXIsProcessTrusted` probe, while asking the system to grant the permission is still unwired. Non-Windows hotkey dispatch is implemented by the historical `windows/hotkey_stub.rs` facade, `native_hotkeys.rs` (macOS/X11 main-thread registrations), `modifier_input.rs` (10 ms modifier samples), and `linux/wayland/hotkeys.rs` (authorized GlobalShortcuts portal sessions). Each loop has a stop signal and join; queued UI operations never block the loop during shutdown. On Windows the dispatch loop is joined after the message loop, float-panel toggles run on a blocking worker, and a dispatcher whose message loop had to be leaked is not joined. `quick_paste.rs` restores macOS/X11 targets by native id plus pid, checks focus and held modifiers/keys before injecting paste, and rejects stale owners. The Windows quick-paste path polls `GetAsyncKeyState` and waits up to 400 ms for Ctrl/Shift/Alt/Win to be released before sending `Ctrl+V`. `linux/wayland/paste.rs` supports Sway/Hyprland with wtype through bounded subprocess calls. A retained 150 ms foreground tracker runs independently of configured hotkeys. Modifier sampling uses x11rb so disconnects return errors instead of entering Xlib process-wide fatal handlers; stale native hotkey events are drained before replacement registrations. Portal `Session.Closed` carries a details map and clears availability. Unix instance liveness treats EPERM/inconclusive failures as live and only ESRCH as dead.

Windows global hotkeys register with `MOD_NOREPEAT` so holding a chord cannot toggle the window repeatedly. Windows image capture copies the DIB bytes while the clipboard is locked and decodes/encodes PNG only after `CloseClipboard`. X11 native text reads stop at a 64 MiB ceiling, and the X11/poll monitors exit when their capture receiver disappears instead of blocking a restart. Tray/main-window show paths do not record one of the application's own windows as the quick-paste target. The 500 ms poll fallback reads `read_clipboard_sequence()` first when the platform provides it and skips all text/file/image reads on an unchanged sequence, avoiding macOS helper-subprocess churn.

On Windows, every main-window hide/show path goes through `platform::hide_main_window` and `platform::restore_main_window` (or the Tauri commands wrapping them). Hiding calls only `Window::hide()`: WebView2 does not render for a hidden HWND — `requestAnimationFrame` and frame production stall, so no "wait until the page presents a frame, then show" protocol can work — and the hidden window keeps its last painted surface, so restoring `show`s that frame immediately. Do not reintroduce `ICoreWebView2Controller::IsVisible(false)` / `ICoreWebView2_3::TrySuspend` here: the suspension saved hidden memory, but the `Resume` that a show needs re-creates the composition surface, and the window blinked to the backdrop a few frames after it appeared (visible as a content flash in the light theme). `platform::enable_child_clipping` sets `WS_CLIPCHILDREN` on the main window at creation so tao's `WM_ERASEBKGND` brush (which fills the whole client rectangle) cannot cover the WebView2 child for a frame. `restore_main_window` only calls `unminimize` when the window is actually minimized, because `ShowWindow(SW_RESTORE)` on a restored window repaints it right after it became visible. Every main-window hide/show path (close-to-tray, tray restore, toggle hotkey, Escape, paste-to-previous-app, float-panel detail, onboarding and single-instance wake) must use these helpers.

The float panel is a second, on-demand WebView. `commands::float::start_float_idle_reaper` destroys it after ten minutes continuously hidden (60 s poll; any visible observation resets the clock), releasing its renderer process; every show path already rebuilds on demand. Before closing, the reaper captures the outer position/size into a process-local slot, and `position_float_panel` restores that geometry (inset-corrected, clamped into the work area by `WorkArea::clamp_point`) instead of the configured corner, so an idle rebuild does not jump the panel across the screen. All toggle/hotkey/tray hide paths stay untouched, and this does not license suspending the main window (see above).

Window opacity/effects are posted to the UI thread. `apply_webview_transparency` uses Win32 alpha, macOS NSWindow alpha, or Linux GTK opacity. Tauri >=2.12.1 exposes transparency without the obsolete `macos-private-api` opt-in; do not reintroduce that target-only dependency feature or `macOSPrivateApi` config overlay, which older manifest validation compares against only the main dependency declaration. macOS maps acrylic/mica to native Vibrancy materials. `linux/x11/effect.rs` sends/clears the KWin-compatible blur hint; Wayland blur requests return an explicit unsupported error while opacity remains available. Disabling whole-window opacity restores native alpha to 100% before CSS supplies background-only translucency.

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

`main.rs` separates GUI and process CLI execution. `cli/mod.rs` implements list/search/copy/paste/delete/export/stats over the same configured database path as the GUI. `cli/api/` splits the loopback server into `mod.rs` (lifecycle), `http.rs` (transport, auth, and responses), and `routes.rs` (endpoints); it starts during setup on an ephemeral `127.0.0.1` port (with explicit start/stop/status commands still available), writes the bound port to `conf/api.port`, enforces configured limits, and retains a stoppable listener/thread lifecycle. `item_operations.rs` owns `CopyContext`, complete-record copy/usage ordering and single-item
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
- follow the maintenance workflow: focused UTs per fix, then the full verification gate after all fixes;
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

`export::backup` implements version-1 `.clipbackup` ZIP bundles of active records and original resource bytes. Before archiving, creation drops path references that are not regular files (directory-valued file captures, missing/imported placeholders) while preserving tags/other metadata; a valid history row must not abort the whole bundle. Export uses the snapshot visitor, streams binaries and spools JSONL to disk. Derived previews/icons and configuration are excluded. Restore first copies the archive to private scratch space, validates entry names/counts, SHA-256, expanded sizes and record constraints, then remaps every known resource reference to newly published flat files under the matching managed image/file root. A strict SQLite transaction skips duplicate content and rolls back on any other failure; rollback removes the newly published files. The Tauri restore command holds storage_maintenance_lock against cleanup, clears the search result cache and invalidates history/tags after success. It reports retention limits without truncating during restore. The snapshot visitor holds the DB mutex during export; very large exports may delay capture.

`FileStore::save_file` now hashes while streaming source bytes into an exclusively created staging file (64 KiB buffer), avoiding a second full read. Hash and size describe exactly the bytes written when the file is published. If a live source grows past the copy cap, staging stops at/below that cap and the remaining source is hashed for the existing pass-through fallback; the partial stage is removed. Flush/fsync and atomic publication remain unchanged. Read/write failures remove only the owned stage. This bounds staging writes, not the lifetime or total reads of an unbounded growing source.
