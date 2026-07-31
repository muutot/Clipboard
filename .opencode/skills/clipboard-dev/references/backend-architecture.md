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

`storage/pool.rs` contains a tested `DatabasePool` with one write connection and round-robin read connections, but the GUI runtime still manages `Database` directly and opens additional `Database` instances for workers. Do not claim the pool is integrated until `lib.rs` state and runtime read/write paths actually use it. The unchecked TODO remains evidence of that boundary.

### Recovery and backups

`storage/recovery.rs` validates with SQLite integrity checks, rotates current/previous backups, quarantines damaged files, restores the first valid backup, and causes the derived search index to be quarantined/rebuilt after recovery.

A persistence change must preserve atomic config writes, SQLite recovery, backup refresh, and the ability to rebuild derived data.

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

Tantivy uses the schema/query modules and a CJK-friendly n-gram tokenizer. SQLite triggers append `search_outbox` operations; `SearchSynchronizer` drains them and applies index changes. Full rebuild begins by clearing/recreating derived index state and repopulating from SQLite.

Read `search-cache-strategy.md` before changing query pagination or caching. Index writes/rebuilds must invalidate backend cached IDs.

## OCR

`OcrEngine` allows PP-OCR, Tesseract, and no-op implementations. `OcrWorkerManager` owns a replaceable worker so engine/model/threshold changes can restart OCR without restarting the app. OCR rows are recoverable jobs, share image-hash results, and feed search through the outbox.

Keep these paths aligned when changing OCR: config, model install/status, engine selection/fallback, worker lifecycle, database status transitions, search synchronization, settings feedback/progress, and shutdown.

## Capture, content, and self-trigger suppression

The clipboard monitor produces change notifications; a named capture thread reads platform content, applies privacy and self-trigger checks, stores resources/metadata, saves through the repository, queues OCR/thumbnails, and emits the saved record. Text capture also reads an optional HTML fragment (`platform::read_clipboard_html`: `HTML Format`/CF_HTML on Windows, `public.html` on macOS, `None` on Linux), capped at 500_000 bytes, which is stored on the text record for later paste-by-format.

`content/` responsibilities:

| File                         | Responsibility                                              |
| ---------------------------- | ----------------------------------------------------------- |
| `detector.rs` / `actions.rs` | content kind/marker detection and quick actions             |
| `hash.rs`                    | canonical content/media/file hashes plus `SelfTriggerGuard` |
| `file_store.rs`              | managed copy decisions and verification                     |
| `resource_metadata.rs`       | metadata schema/MIME/file details                           |
| `thumbnail.rs`               | background preview generation and worker lifecycle          |
| `transform.rs`               | text transforms                                             |

Write-back registration and capture-side checks must use the same canonical hashing rules for text, links, images, and files. A mismatch creates duplicate history records or suppresses legitimate captures.

## Privacy and cleanup

Privacy pause, ignored applications, and sensitive-source checks must happen before persistence, file writes, OCR, or index work. Cleanup reads current config periodically, protects favorites, respects recycle-bin policy, removes database rows first through repository rules, and cleans only positively owned orphan resources.

## Platform adapters

`platform/` contains shared traits/managers plus Windows, macOS, X11, and Wayland sources. Windows is the primary wired runtime. The presence or size of macOS/Linux files is not proof of native clipboard, hotkey, source-app, double-modifier, tray, or quick-paste completion. Use `TODO.md`, runtime wiring, target-specific tests, and real-platform evidence.

Keep shared degradation capabilities accurate in `RuntimeInfo`; never expose an enabled UI path when the platform implementation is a scaffold.

## CLI and loopback API

`main.rs` parses GUI versus process CLI execution. `cli/mod.rs` implements commands over the database. `cli/api.rs` starts only on explicit command, binds to `127.0.0.1`, accepts configured limits, and must retain a stoppable listener/thread lifecycle.

A CLI/API action that writes to the system clipboard must follow the same self-trigger and metadata-preservation rules as the GUI.

`export/` owns `write_export_file` (creates parent directories and writes export output). Both the CLI and the file-backed Tauri commands in `commands/export.rs` (`export_to_file`, `import_from_file`) reuse it; the GUI import emits `clipboard-history-invalidated` after a successful import so the main window refreshes.

## Update checks

`commands/update.rs` exposes `check_for_update`: a stateless GitHub Releases check against the owning repository using the existing `reqwest` dependency (10s timeout, `clipboard-desktop` UA). It parses the latest non-draft/non-prerelease tag, compares it to the current app version with numeric three-segment ordering (ignoring a `v` prefix and non-numeric segments), and returns `UpdateInfo` with camelCase fields for the settings About panel. No install/apply path exists; the result links to the release page. Test coverage lives in the module's unit tests. A release page URL should remain the only external navigation surface until a signed installer pipeline exists.

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
