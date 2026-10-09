# v1.8 follow-up implementation

The original 14 commits were rebased onto v1.7.5 and fast-forwarded into it at
`cbfc3c9`. Follow-up changes belong to the existing v1.8.0 worktree/branch.
No version bump, tag, push or release is implied.

| Item                      | Necessity and scope                                                                                 | Status / evidence                                                                                                                                                                                          |
| ------------------------- | --------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1. Native copy            | Remove redundant full-body IPC; retain media materialization and formatted paste hydration.         | Complete: 353 frontend tests with two workers, Svelte check and frontend build pass.                                                                                                                       |
| 2. History auto tags      | Avoid sharing the GUI database connection during a full scan; retain atomic writes and manual tags. | Implemented: separate WAL snapshot and on-disk plan; concurrent capture/manual tags, changed-input rollback and final-row cancellation tests pass. UI progress/cancel follows in item 3.                   |
| 3. Long tasks             | Expose bounded progress/cancellation for tags, backups and the current sync run.                    | Implemented: 356 frontend and 846 Rust tests pass; type check, frontend build and Clippy pass. Browser fixtures cover 18 panel/theme/width combinations; native validation remains separate.               |
| 4. Native desktop         | Exercise clipboard, multiple windows and visual combinations using isolated data.                   | Necessary, unverified: a user app is running and the native CF_HDROP test overwrites the system clipboard. Keep the native matrix below open.                                                              |
| 5. Platforms and network  | Check matching platform CI and real S3 interruption/exit evidence.                                  | Partial: all 4 real-S3 smoke tests pass, including snapshot/segment publication cancellation and retry. Existing CI covers three OSes; final-branch remote results and native exit timing are unavailable. |
| 6. Large-data performance | Measure before selecting further memory or throughput changes.                                      | Preview memory optimized after measuring 1k/10k/100k synthetic rows: extra RSS at 100k falls from 267.37 to 18.55 MiB. Seven backup regressions pass. See PERFORMANCE_V1_8.md for timing and scope limits. |
| 7. Local gates            | Make Vitest worker limits reproducible without omitting tests.                                      | Complete: Vitest defaults to two workers; plain npm test passes all 356 tests with isolation retained. CLI worker overrides remain available.                                                              |

Checks describe only the exercised environment. Missing native, remote or visual evidence
must remain explicit; a passing unit test does not complete those rows.

Long-operation verification includes stale-ID cancellation, shutdown waiting, final-row
transaction rollback, resource cleanup and retry. Eight existing ignored Rust checks and the
native CF_HDROP test are excluded from the ordinary Rust result: that native test replaces
the system clipboard without restoring it. The browser fixtures use synthetic data and a
mock invoke bridge; they establish layout and cancel-button behavior, not native IPC.

## Native verification still required

Use a disposable desktop session and temporary application data for these checks.
An isolated database alone does not isolate the OS clipboard or global shortcuts.

- [ ] Native text, HTML, files and remote-media copy/paste preserves full content.
- [ ] Two real windows observe settings edits, task status/cancellation and item invalidation.
- [ ] Dark/light/custom themes at normal/narrow sizes, including Windows text transparency
      combined with background/window effects, remain readable.
- [ ] Exit during tags, restore and a slow S3 transfer joins workers and leaves retryable data.
- [ ] The final branch receives successful Windows/macOS/Linux CI results.

## Real S3 evidence (2026-10-09)

`scripts/sync-s3-test.ps1 -Port 19280 -Test smoke` ran against the pinned local rustfs
server with synthetic credentials, temporary SQLite databases and random remote prefixes.
All 4 tests passed in 27.63 seconds. The added regression interrupts after a real snapshot
or segment PUT completes, confirms the published head does not advance and pending work
survives, then retries and checks exact two-device convergence and idle no-op behavior.
This proves publication-boundary recovery over S3; it does not measure WAN interruption
latency or native application exit. Stalled HTTP transport cancellation has separate tests.
The helper stopped its own server after completion.

## Final local integration evidence

- Plain `npm test`: 356 passed, one opt-in frontend benchmark skipped.
- Rust workspace: 849 passed, 10 ignored opt-in checks, one native CF_HDROP check
  explicitly excluded. Command:
  `npm run test:rust -- -- --skip platform::windows_clipboard::tests::read_clipboard_file_paths_reads_a_cf_hdrop`.
- Svelte check: zero errors/warnings. Frontend build, workspace Clippy and format pass.
- Real S3 smoke: 4 passed separately; backup preview benchmarks ran at 1k/10k/100k rows.

The full Rust run exposed an existing Windows API connection race: accepted sockets
could retain nonblocking mode and reject delayed request bytes with `WouldBlock`.
A regression failed before the fix and passed after accepted sockets explicitly switched
to bounded blocking I/O. All 14 API tests and the full workspace then passed.

The necessary implementation work is committed on v1.8.0. Native verification and final
remote CI remain open as listed above; version metadata, tags and remote branches have
not been changed by this follow-up.
