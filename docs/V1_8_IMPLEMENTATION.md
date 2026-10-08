# v1.8 implementation checklist

Base: `v1.7.5` at `b8659bed6f3cbfaaee2a80b83788cedbdeab4bc5`.
Branch: `v1.8.0`. Each unit is verified and committed separately.
This tracks implementation, not a version bump or release.

- [x] Include frontend tests in both local verification gates (320 Vitest tests pass).
- [x] Search all history for single characters and dates; apply combined filters before candidate limits and pagination (322 frontend tests, 816 Rust tests and Clippy pass; frontend check/build and formatting pass).
- [x] Save named searches with complete filter state and relative dates (329 frontend tests; check/build/format pass; dark-theme browser interaction and 520px layout inspected).
- [x] Return bounded active-list text summaries and hydrate full content for detail/copy/edit (336 frontend and 818 Rust tests in verify; extra mounted editor regression passes).
- [x] Select/copy OCR blocks and highlight search matches on images (342 frontend tests; check/build/format pass; synthetic image geometry inspected in light and dark/narrow views).
- [x] Extend automatic tags with source/type conditions, preview and history application (344 frontend tests, Rust suite and Clippy, check/build/format pass; dark settings layout inspected).
- [x] Export/import a self-contained resource backup with validation and restore preview (346 frontend tests; check/build/format/Clippy pass; Rust suite passes with the unavailable native OpenClipboard test excluded; dark and 520px settings inspected).
- [x] Extract search/history/bulk/window controllers with one item-store owner (350 frontend tests, check/build/format pass; browser main page inspected; stale requests, disposal and rollback covered).
- [x] Share item-operation behavior across GUI, CLI and local API (353 frontend tests with two workers; Rust workspace tests except unavailable native clipboard test, Clippy, check/build/format pass).
- [x] Cancel synchronization cooperatively through engine and transport (stalled HTTP and retry interruption, temporary-file cleanup, publication retry, snapshot/checkpoint rollback tests; full Rust workspace and Clippy pass with the unavailable native clipboard test excluded).
- [x] Stream file exports from one read transaction with atomic publication (Rust export/WAL/concurrent-write/failure regressions and full Rust suite, Clippy pass).
- [x] Hash file bytes during bounded staging without a second full read (stream/digest/growth/write-failure tests, Rust suite except unavailable native clipboard test, Clippy and format pass).
- [x] Cache content-dependent card measurements and reuse unchanged geometry arrays (352 frontend tests, check/build/format pass; no CSS changes; synthetic timings tracked below).
- [x] Measure search input-to-paint latency and validate performance candidates with synthetic data (tracker lifecycle tests, bounded Rust metrics; see `PERFORMANCE_V1_8.md` for measurements and limits).

## Verification

Use synthetic data and temporary directories for destructive, import and backup tests.
Run focused tests during each unit and the integration gate after cross-layer work.
Preserve single-character recall, global sorting, mutation invalidation, original clipboard
payloads, resource ownership, atomic publication, and cross-window consistency.
Record native-platform and visual checks separately from static/unit results.

Saved searches reuse the existing frontend-owned config fields and settings patch transport.
Native cross-window behavior is covered through the mocked persistence bridge; native WebView
and light/custom theme inspection remain integration follow-ups.

Active lists project 2048 text characters in SQLite and omit rich-text bodies. Metadata and
file lists stay complete; recycle-bin bodies stay complete to preserve its local keyword search.
Backend and spare frontend caches enforce a 16 MiB string-payload budget. Native clipboard
integration and process-memory measurements remain separate from the passing unit/bridge tests.

Shared native default copy now uses the backend operation service. GUI media materializes
remote content before copy; CLI/API require existing local resources. External SQLite writes
invalidate desktop views within a 500 ms polling interval plus any database-lock wait.
The unrestricted Vitest fork pool exhausted available process/memory resources during the
combined gate; rerunning the complete suite with `--maxWorkers=2` passed. No test was omitted
by this concurrency adjustment. The opt-in performance benchmark is skipped in normal tests.

## Final integration gate (2026-10-09)

- Frontend: 353 tests passed with `--maxWorkers=2`; one opt-in synthetic benchmark skipped
  by the normal suite (its separate measured run is in `PERFORMANCE_V1_8.md`).
- Rust workspace: 838 tests passed, 8 pre-existing ignored tests, and one explicitly
  excluded native CF_HDROP test because `OpenClipboard` is unavailable in this session.
- Svelte check: zero errors/warnings. Frontend build, workspace Clippy and formatting pass.
- No version metadata bump, tag, release build, push or merge was performed.

The implementation checkboxes describe the exercised code and test scope. Native desktop
copy/paste, app exit timing, cross-platform CI, real S3 interruption, process-memory profiling,
and remaining light/custom-theme settings inspection are integration evidence gaps. Settings
layout checks completed during the individual units are recorded above. The new performance
cards reuse existing primitives but have not been inspected in native WebView.

Sync cancellation preserves completed commits, rolls back an incomplete batch apply, removes
partial downloads, and permits orphan publication recovery. Uninterruptible local OS/disk calls
still have the existing 30-second fallback. GUI remote media downloads before copying; CLI/API
copy operates on available local resources and reports a missing-resource error otherwise.
