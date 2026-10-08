# v1.8 performance evidence

Synthetic benchmark on Windows, 2026-10-08, Node/Vitest in the development worktree.
Run `src/lib/utils/history-performance.test.ts` explicitly:

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
latency. Native WebView interaction and process-memory profiling remain unmeasured.

File export and capture improvements are additionally covered by behavioral tests:
one SQLite read transaction under a concurrent WAL writer, atomic destination
publication on write failure, and single-pass file copy/hash with bounded staging.
No storage-throughput or network-throughput percentage is claimed.
