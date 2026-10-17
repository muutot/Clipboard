# v1.8 performance evidence

Synthetic benchmark on Windows, 2026-10-08, Node/Vitest in the development worktree.
Run `src/lib/utils/layout/history-performance.test.ts` explicitly:

```powershell
$env:CLIPBOARD_BENCHMARK = '1'
npm test -- history-performance
Remove-Item Env:CLIPBOARD_BENCHMARK
```

1,000 distinct synthetic records, approximately 32 KiB text plus 32 KiB HTML each.
Each operation has 3 warmups and 9 measured samples; results below are medians.
The checksum sink prevents discarding the work. No timing threshold runs in CI.

| Operation                                                    |     Before |     After |
| ------------------------------------------------------------ | ---------: | --------: |
| Repeated full-text line scan vs warm WeakMap identity tokens |   30.60 ms |  0.025 ms |
| JSON serialization, full bodies vs 2,048-character summaries |   51.29 ms |   1.97 ms |
| Serialized payload, UTF-8 bytes                              | 65,599,621 | 2,199,781 |

The token comparison isolates the removed normalization/splitting cost. It does not
measure canvas layout, DOM painting, cold cache, or the remaining O(n) geometry scan.
The payload comparison models active text summaries; metadata and file lists remain
complete, and full bodies are still loaded for detail, copying and editing. It does
not measure SQLite query time, native IPC overhead, or resident process memory.
Actual benefit depends on record size and cache reuse. This is not an end-to-end
desktop speedup claim. No real clipboard data was used.

Search interaction metrics in Settings → Statistics → Performance measure input
through debounce, query and two animation frames after accepted results. They report
mean/P95/P99 for the latest 1,000 completed visible text searches, with durations only
in memory. Empty queries, stale/hidden results, errors and disposal do not add samples.
This approximation includes the intentional 300 ms debounce, unlike backend query
latency. General native WebView interaction latency remains unmeasured; the
hidden-window resident-memory probe is recorded below.

File export and capture improvements are additionally covered by behavioral tests:
one SQLite read transaction under a concurrent WAL writer, atomic destination
publication on write failure, and single-pass file copy/hash with bounded staging.
No storage-throughput or network-throughput percentage is claimed.

## Hidden WebView2 memory (2026-10-17)

Windows 11, local debug build with the development Cargo profile and an isolated
WebView2 data directory. The process group contained the Rust host plus six WebView2
processes (browser, GPU, network/storage utilities, crashpad and renderer). The main
window was hidden through the normal close-to-tray path, which also set the
`ICoreWebView2Controller` invisible and requested `TrySuspend`. The app process itself
stayed alive with its DOM and JavaScript state intact.

| State                     | Private working set | Working set total | Private commit |
| ------------------------- | ------------------: | ----------------: | -------------: |
| Main window visible       |           205.2 MiB |         536.1 MiB |      288.4 MiB |
| Hidden 10 s               |            38.7 MiB |         111.0 MiB |      273.8 MiB |
| Hidden 40 s               |            48.9 MiB |         128.2 MiB |      271.9 MiB |
| Restored after first wake |           148.5 MiB |         325.2 MiB |      316.5 MiB |

Working set total sums shared pages across the six WebView2 processes and therefore
double-counts resources relative to the OS; it is diagnostic only. Private working set
is the resident physical-footprint measure. `TrySuspend` is best effort: it trims
resident pages but does not immediately release the renderer's committed address space,
which is why private commit falls much less than resident memory. A second hide reached
45.7 MiB after about 40 seconds, and a fresh debug process after the final race fix
reached 17.4 MiB after the same wait.

After suspension, launching the same debug executable was handled by the
single-instance guard and restored the main window. Native screenshots after the first
and second wake showed the complete 73-record list, images and thumbnails rather than a
blank webview. Escape hiding and close-to-tray hiding both exercised the same lifecycle.
These are single observations from one Windows machine, not statistical P50/P95 values,
and they do not claim the same result for release, macOS or Linux builds. The record
count was not varied in this probe because the dominant footprint is the WebView2
process group rather than the 41/73-row SQLite payload.

## Portable-backup preview memory (2026-10-09)

The old validator retained every decoded clipboard body even for a read-only preview.
An opt-in Rust benchmark first measured that behavior, then measured the same fixture
after preview switched to per-row validation and release. Restore still retains rows
for its existing atomic transaction. Preview retains identity sets and resource metadata;
its memory is not constant in record count.

`src-tauri/src/export/backup/scale_bench.rs` generates a ZIP one row at a time with
2,048 repeated text characters plus a unique prefix and small rich-text/metadata fields.
There are no binary resources. It runs the actual preview against an empty temporary
file-backed SQLite database. Fixture creation is excluded from timing; each measurement
uses a separate test process and a 20 ms RSS sampler. The JSONL size is 2,435 bytes/row.
The Windows local Cargo test profile is unoptimized. No release overrides were used.

| Records | Extra peak RSS before | Extra peak RSS after | Preview time before | Preview time after |
| ------: | --------------------: | -------------------: | ------------------: | -----------------: |
|   1,000 |              2.99 MiB |             2.12 MiB |              110 ms |             123 ms |
|  10,000 |             28.30 MiB |             4.00 MiB |            1,034 ms |           1,110 ms |
| 100,000 |            267.37 MiB |            18.55 MiB |           20,533 ms |          23,394 ms |

These are single observations at each size, not statistical latency estimates. Extra RSS
subtracts the process baseline after fixture creation (approximately 15 MiB). At 100,000
rows the observed extra peak falls by about 93%; elapsed time did not improve in this run.
This change targets memory, with no throughput claim. OS scheduling, filesystem cache and
antivirus can affect the times. Restore memory, resource-heavy archives, populated target
databases, native WebView P95/P99 and whole-application RSS remain separate measurements.

Reproduce without changing production data or the normal CI timing budget:

```powershell
$env:CLIPBOARD_BACKUP_BENCH_ITEMS = '100000' # also 1000 or 10000
cargo test --manifest-path src-tauri/Cargo.toml --lib export::backup::scale_bench::preview_large_backup -- --ignored --nocapture --test-threads=1
Remove-Item Env:CLIPBOARD_BACKUP_BENCH_ITEMS
```

The benchmark asserts accurate counts and no database writes. Backup regressions also
cover validation of the final already-present row, EOF cancellation/retry, unchanged
checksums and resource validation, and transactional restore rollback.
