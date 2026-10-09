# v1.8 follow-up implementation

The original 14 commits were rebased onto v1.7.5 and fast-forwarded into it at
`cbfc3c9`. Follow-up changes belong to the existing v1.8.0 worktree/branch.
No version bump, tag, push or release is implied.

| Item                      | Necessity and scope                                                                                 | Status / evidence                                                                    |
| ------------------------- | --------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| 1. Native copy            | Remove redundant full-body IPC; retain media materialization and formatted paste hydration.         | Complete: 353 frontend tests with two workers, Svelte check and frontend build pass. |
| 2. History auto tags      | Avoid sharing the GUI database connection during a full scan; retain atomic writes and manual tags. | Pending implementation.                                                              |
| 3. Long tasks             | Expose bounded progress/cancellation for tags, backups and the current sync run.                    | Pending contract review.                                                             |
| 4. Native desktop         | Exercise clipboard, multiple windows and visual combinations using isolated data.                   | Pending environment verification.                                                    |
| 5. Platforms and network  | Check matching platform CI and real S3 interruption/exit evidence.                                  | Pending available runtime/CI evidence.                                               |
| 6. Large-data performance | Measure before selecting further memory or throughput changes.                                      | Pending synthetic/native measurements.                                               |
| 7. Local gates            | Make Vitest worker limits reproducible without omitting tests.                                      | Pending configuration.                                                               |

Checks describe only the exercised environment. Missing native, remote or visual evidence
must remain explicit; a passing unit test does not complete those rows.
