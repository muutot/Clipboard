# Sync v2: scoping

Status: scoping only. Nothing in this document is implemented, scheduled or version-committed.
Each item below must leave this file and become an independently verifiable unit with its own
commits and evidence.

Sibling document: [`SYNC_V1.md`](SYNC_V1.md) defines the only supported sync protocol, and it
stays supported. A `v2/` wire namespace with no v1 reader is a separate decision that needs its
own motivation and is **not** assumed by this scope.

## Why a v2 track exists

v1 shipped as an S3-first replication protocol with immutable, content-addressed packs and one
per-scope encryption binding. Three items accumulated around it:

1. **Encryption password / mode rotation is impossible today.** Editing the password of an
   initialized scope fails hard on the next run (`cannot authenticate the existing sync v1
namespace`) and publishes nothing, so the user stays on the old password or loses remote
   access. `SYNC_V1.md` names a future destructive rotation workflow and the current settings
   surface deliberately rejects in-place changes. Tests:
   `sync/v1/engine_tests/encryption.rs::{wrong_password_is_rejected_before_a_new_device_publishes,
encryption_mode_change_is_rejected_before_a_new_device_publishes}`.
2. **Real-server CI coverage was deferred to this track.** The real-S3 smokes need a disposable
   S3-compatible server; only `scripts/sync-s3-test.ps1` (Windows, pinned rustfs) provides one
   locally, and the TODO 1.5.3 decision deferred the CI matrix to the v2 surgery. CI
   (`.github/workflows/ci.yml`) runs three OSes with no S3 job, so the 8 S3-dependent ignored
   tests never run in CI.
3. **Integration evidence gaps are explicit, not fixed.** Real WAN interruption timing and
   native process-exit timing remain checks outside the deterministic suites
   (`SYNC_V1.md`, "Cooperative cancellation" closing notes).

## Candidate items (none committed)

### V2-1. Encryption password / mode rotation workflow

User-visible goal: replace the sync password (or toggle the encryption mode) of an already
initialized scope without losing remote history or stranding other devices.

The four steps are pre-described by `SYNC_V1.md` ("a future dedicated, destructive rotation
workflow"): materialize every referenced resource under the old key, delete the old namespace's
v1 objects, clear scope-local sync state (heads, cursors, checkpoint baseline, cached namespace
binding), and republish the complete local state under the new key as a fresh bootstrap.

Open design decisions that must be answered before any implementation unit:

- **Crash window.** "Delete, then republish" leaves a crash point with no remote state. Either
  accept it with an explicit recovery story, or stage the new namespace first and delete the
  old one only after the new head is durable (double storage during rotation).
- **Multi-device semantics.** Peers keep syncing against the old namespace until it is deleted;
  afterwards every stale-password peer fails authentication and diverges. Rotation needs an
  explicit story: who rotates first, how peers discover the new namespace, and what a stale
  peer experiences.
- **Plaintext ↔ encrypted toggling.** The mode check is the same rejection family as the
  password check; decide whether one flow covers both.
- **Cost bounds.** Materializing a large scope is a full download (the documented workload is
  100,000 records per device). Define bounds, progress and cancellation on the same model as
  the v1.8 long-task surface (tags, backups, sync).

Acceptance candidates (intended gates; no test exists yet):

- rustfs smoke: two devices, rotate the scope, both converge on the new namespace, no mixed
  ciphertext, old namespace fully deleted;
- failure injection at every step boundary (cancel mid-materialize, mid-delete,
  mid-republish) leaves a documented, recoverable state;
- a wrong or old password during rotation is a hard error before any local state is cleared;
- the destructive step requires explicit user confirmation (frontend unit, after the engine).

### V2-2. Real S3 CI matrix

Status: wired, evidence pending. `.github/workflows/sync-s3.yml` runs the six S3-dependent
smokes on pushes and pull requests touching the sync paths, weekly, and on dispatch. A first
attempt used a pinned MinIO docker container and never passed the readiness probe; the job now
provisions the pinned rustfs release the local `scripts/sync-s3-test.ps1` path already uses,
verified against the upstream SHA256SUMS, and starts it in the same step as the tests (no
cross-step background-process assumptions, no docker daemon). Pre-verified locally: the
SHA256SUMS entry for the pinned asset and the checksum-extraction logic. Not pre-verifiable in
the authoring environment (throttled network): the 188 MB archive contents and the rustfs
1.0.1 CLI flags, which mirror the ps1 recipe and fail fast with the server log on startup
errors. Only ubuntu runs (pinned linux-x86_64-musl asset). The first green run on the final
branch is still owed as evidence.

Goal: the S3-dependent ignored tests run in CI so their behavior cannot rot between manual runs
(retires part of the roadmap's Rust ignored-test debt item).

S3-dependent ignored tests today (8):

- `sync::v1::s3_smoke::*` × 5 and `clipboard_sync::s3::tests` × 1 (correctness smokes);
- `sync::v1::scale_bench::s3_*` × 2 (also needs a release-mode runtime).

Decision: rustfs, not MinIO. The same server family that produced the documented smoke
evidence, no docker daemon on the runner, pin plus checksum verification, and one step for
server and tests. The MinIO attempt above is recorded so it is not repeated blindly.

Acceptance candidates: matrix green on the trigger scope; the suites still skip cleanly when
`CLIPBOARD_S3_TEST_ENDPOINT` is unset; the scale benchmarks stay opt-in and out of the default
gate.

### V2-3. Integration evidence for WAN interruption and native exit

Not a protocol change. Run the existing smoke and cancellation suites against a remote endpoint
with induced latency/loss (for example a loopback tunnel), and time native exit during an active
transfer, then record results in `SYNC_V1.md`. The scope may shrink to "document the procedure
and run it once" if full automation is not justified.

## Explicit non-goals (unless separately motivated)

- A `v2/` wire namespace with no v1 reader. The precedent (v1 replacing baseline/oplog with no
  conversion, `SYNC_V1.md` "Obsolete-state deletion boundary") permits it, but no motivating
  requirement exists yet; the 256 MiB in-memory object ceiling and multi-bucket support would
  each need their own case.
- Restoring WebDAV or other non-S3 backends. `SYNC_V1.md` only records that the old IPC
  surfaces are not registered; there is no decision to bring them back.
- Any change to v1 acceptance criteria or immutable-pack invariants while v1 remains the
  supported protocol.

## Suggested unit order

1. V2-2 (CI matrix) first — independent of the protocol and unblocks real-server evidence for
   everything after it.
2. V2-1 engine level plus rustfs smoke, once the open design decisions are answered.
3. V2-1 frontend: dedicated destructive flow with progress, cancellation and confirmation.
4. V2-3 as a scheduled evidence run.

Cross-references: `TODO.md` open item C ("同步 v2"), `docs/SYNC_V1.md`, and the roadmap's Rust
ignored-test debt item (V2-2 retires part of it).
