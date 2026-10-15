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

## Work items

### V2-1. Encryption password / mode rotation workflow

Progress (2026-10-15): D1/D2 decided, D3/D4 recommended. Engine level landed:

- `commands/sync::refuse_if_prefix_rotated` — every sync run reads `{prefix}/rotated-to`
  before namespace resolution and refuses with an actionable message, so a stale peer can
  never repair the old binding from cache and fork the namespace (pure unit tests plus the
  real-S3 smoke `real_s3_rotation_pointer_blocks_a_stale_peer`; both CI workflows green, smoke
  run 38553188977 / main CI 38553189131 at sha 23d1cc3a).
- `rotate_sync_namespace` (Tauri command) over `perform_namespace_rotation` — materializes
  every referenced resource of the old scope, claims a fresh derived prefix
  (`{old}-rotated-{unix_ms}`) with the new key, republishes the complete local state, writes
  the tombstone with the new key check, deletes the old engine namespace, and only then
  rewrites the local configuration. A `MemoryStore`-style integration test covers the whole
  sequence including the double-rotation refusal, and the real-S3 smoke
  `real_s3_namespace_rotation_moves_state_and_tombstones` proves the same flow over the wire
  (materialize-all, encrypted republish, tombstone contents, old-namespace deletion,
  byte-identical body, second-rotation refusal) — smoke run 38662706953 / main CI green on
  sha e9d1163b. Failure injection at every step boundary (cancel mid-materialize,
  mid-publish, after the pointer, mid-delete, mid-repoint) is still owed; the destructive
  cancellation semantics follow the v1.8 long-task model.

Remaining: the rustfs failure-injection smoke and the frontend flow.

User-visible goal: replace the sync password (or toggle the encryption mode) of an already
initialized scope without losing remote history or stranding other devices.

The four steps are pre-described by `SYNC_V1.md` ("a future dedicated, destructive rotation
workflow"): materialize every referenced resource under the old key, delete the old namespace's
v1 objects, clear scope-local sync state (heads, cursors, checkpoint baseline, cached namespace
binding), and republish the complete local state under the new key as a fresh bootstrap.

The design questions are worked out with options and trade-offs in
[V2-1 design decisions](#v2-1-design-decisions-2026-10-14-drafted-pending-decision) below.

## V2-1 design decisions (2026-10-14, drafted, pending decision)

| #   | Decision                                                                                     | Status                                       |
| --- | -------------------------------------------------------------------------------------------- | -------------------------------------------- |
| D1  | Stage a new remote prefix, then delete the old namespace (option A)                          | Implied by D2; pending explicit confirmation |
| D2  | Pointer warning, then delete the old namespace immediately; peers hard-fail until re-pointed | **Decided 2026-10-14**                       |
| D3  | One flow covers plaintext ↔ encrypted toggling                                               | Recommended, pending objection               |
| D4  | Preview + progress + cancellation, no hard size cap                                          | Recommended, pending objection               |

### Constraints proven from the current tree

1. `v1/namespace.json` is create-only: a second PUT returns `PreconditionFailed`
   (smoke `namespace_binding_is_durable_and_cannot_be_overwritten`). The binding can never be
   edited in place, which is why any rotation is destructive.
2. The remote scope identity excludes the password (`SmokeScope::new` hashes
   endpoint/region/bucket/prefix/access key). Changing the password changes only the derived
   session key and the binding's HMAC key check — the same scope keeps its identity.
3. On publish, a canonical resource reference is passed through **without uploading its body**
   (`resources/mutations.rs::rewrite_outgoing_path`); a local path is fingerprinted and
   uploaded; a missing local file is counted as skipped and the reference is dropped.
   Therefore a rotation that republishes from local state **must first materialize every
   referenced resource**, or the new namespace ships dangling references. This holds for every
   D1 option below.
4. A deleted `namespace.json` can be recreated by a peer from its cached identity
   (`docs/SYNC_V1.md`: "a missing binding may be repaired from that cached identity after
   authenticating existing pointers"). With same-prefix deletion, a stale-password peer can
   write the old key check back into the namespace — the mixed-ciphertext state the design
   forbids.

### D1 — Crash window

- **A. Stage a new remote prefix, then delete the old one (recommended).** Publish the
  complete local state under a new prefix with the new key, verify locally, delete the old
  prefix, and only then rewrite the configured prefix. Any crash leaves the old namespace
  intact until the new head is durable; clipboard data is never at risk; recovery is
  "resume or abandon". Costs: the prefix becomes a user-visible, mutated setting (needs the
  same explicit-confirmation treatment as the rotation itself), double storage during the
  window, and an orphan old prefix that needs a cleanup path. Side effect: every device
  re-bootstraps from the new scope once (fresh devices bootstrap in ~51 s per 300k records in
  the documented benchmark).
- **B. Delete the same prefix, then republish (`docs/SYNC_V1.md`'s current recipe).** One
  prefix, no config change. Crash inside the window leaves the remote empty; recovery is a
  fresh bootstrap from local data, but until it completes the scope has **no remote backup at
  all** — the window equals a full re-upload (38.6 s encode for 300k records in the benchmark,
  plus every resource; a large resource set makes it minutes to hours). Constraint 4 above
  also applies while the namespace is missing.
- **C. Generation sub-namespace inside the same prefix.** Requires a layout/protocol change
  (the engine owns `v1/` as a fixed root); fold into the separate `v2/` namespace question,
  do not build it here.

**Decision (implied by D2, pending explicit confirmation): A.** B's window is a real
backup-safety regression; A's extra costs are one-time and collectible. The decided D2
protocol needs the old namespace alive as the pointer's home, which only A provides.

### D2 — Multi-device semantics (decided: pointer warning, then delete immediately)

**Decision (2026-10-14):** keep the pointer notification, but do **not** wait for every known
device to switch. Once the pointer is durable, the old namespace is deleted right away;
stale peers hard-fail with an actionable error until a human re-points them. This trades
"peers keep working on the old namespace during a handshake" (rejected: the split-brain
window stays open as long as the old namespace accepts writes, and per-device switch
bookkeeping is pure overhead) for a shorter window and a much smaller engine change. Manual
runbook (rejected: a forgotten device silently keeps its own namespace) and single-device-only
(rejected: the documented workload is three devices) remain rejected.

Concrete protocol:

1. The rotating device publishes the complete state at the new prefix `P2` with the new key
   (D1-A) and verifies it locally.
2. It writes one small owner-only pointer object at the **old configured prefix root**,
   outside the engine's `v1/` namespace: `{P1}/rotated-to` = new prefix + new key check +
   timestamp + rotating device id. The key check lets a peer verify a typed password locally
   before any network work.
3. It deletes every object under `{P1}/v1/` immediately. The pointer at the prefix root
   survives (the deletion scope is the engine namespace, never the configured prefix
   itself, per the existing obsolete-state deletion boundary). Crash between 2 and 3 is
   idempotent (retry deletion); a crash after 3 leaves the pointer alone, which is exactly
   the discovery artifact peers need. A later rotation leaves its own pointer; stale pointers
   are tiny and documented as retained.
4. It rewrites its own configured prefix to `P2` (destructive setting change, explicit
   confirmation) — **before** the deletion starts, because the tombstone makes every run at
   `{P1}` refuse: an interrupted deletion must not leave the device pointed at a prefix that
   answers "rotated". Leftover old objects after an interruption are bounded garbage; an
   unsyncable device is not (regression:
   `rotation_commits_the_configuration_before_deleting_the_old_namespace`).
5. It rewrites its own configured prefix to `P2` (destructive setting change, explicit
   confirmation).
6. A stale peer's next run checks `{P1}/rotated-to` **before** namespace resolution or binding
   repair. Without this check, constraint 4 applies: the peer would recreate `namespace.json`
   from its cached identity with the old key check and bootstrap a second namespace — the
   split brain this design exists to prevent. With the check, the peer refuses to run and
   reports "namespace rotated on <date>; obtain the new prefix and password".
7. The user re-points each peer (new prefix + new password); the pointer's key check gives
   immediate local feedback on a wrong password. Re-pointed peers bootstrap from `P2` as
   fresh devices and purge the old scope's local state (heads, cursors, outbox, cached
   binding) the way the rotation recipe already describes.

Engine-level consequence: the pointer check belongs in the desktop sync entry
(`commands/sync`) before the engine runs, because the pointer lives outside the engine's
prefix scope; the engine itself stays unaware of rotation. Deletion reuses the existing
bounded list-and-delete discipline (full pagination, validated keys, never the prefix).

### D3 — Plaintext ↔ encrypted toggling

One flow. The mode is only `session_key: None` vs `Some`, and both rejections
(`wrong_password_is_rejected_before_a_new_device_publishes`,
`encryption_mode_change_is_rejected_before_a_new_device_publishes`) are the same family.
A separate path would double the UI and the failure injection for no behavioral gain.

### D4 — Cost bounds

Materialize-everything is mandatory (constraint 3): download unknown resources first, then
republish. The cost is one-time but invisible to the user, so the flow must show it before
starting and survive abandonment:

- confirm dialog with the pending resource count and byte total (both are already countable
  from the scope-local resource reference tables);
- progress and cancellation on the v1.8 long-task model (tags/backups/sync), with the same
  "cancelled commits stay committed" semantics;
- no hard size cap; a rotation event is rare and a cap would block legitimate large scopes.

### After the decisions

1. engine-level rotation flow (materialize-all → publish at `P2` → write `{P1}/rotated-to` →
   delete `{P1}/v1/`) plus rustfs smoke with failure injection at every step boundary;
2. the peer-side pointer check in the desktop sync entry with its own smoke case;
3. frontend: dedicated destructive flow with the confirmation, progress and cancellation.

Progress: step 2 is done (2026-10-14, `commands/sync::refuse_if_prefix_rotated`); step 1's
engine flow is done (`perform_namespace_rotation` behind the `rotate_sync_namespace` command,
with `MemoryStore` and real-server smokes); step 3 is done (2026-10-15,
`SyncPanel` rotation section — destructive `<dialog>` confirm with a new-password field,
progress/cancel through the shared Sync-kind operation monitor, config re-read on success,
`rotateSyncNamespace` service wrapper, bilingual keys; type check and the frontend suite pass).
What remains for V2-1 is only the rustfs failure-injection smoke at every step boundary.

Acceptance candidates (intended gates; no test exists yet):

- rustfs smoke: two devices, rotate the scope, both converge on the new namespace, no mixed
  ciphertext, old namespace deleted while the pointer survives at the prefix root;
- failure injection at every step boundary (cancel mid-materialize, mid-publish, after the
  pointer, mid-delete, mid-repoint) leaves a documented, recoverable state;
- a stale peer that has not seen the pointer check refuses with the actionable message and
  writes nothing — in particular it must not recreate `namespace.json` from its cached
  binding;
- a wrong or old password during rotation is a hard error before any local state is cleared;
- the destructive step requires explicit user confirmation (frontend unit, after the engine).

### V2-2. Real S3 CI matrix

Status: wired and green. `.github/workflows/sync-s3.yml` runs the eight S3-dependent smokes on
pushes and pull requests touching the sync paths, weekly, and on dispatch. A first attempt
used a pinned MinIO docker container and never passed the readiness probe; the job now
provisions the pinned rustfs release the local `scripts/sync-s3-test.ps1` path already uses,
verified against the upstream SHA256SUMS, and starts it in the same step as the tests (no
cross-step background-process assumptions, no docker daemon). First green run 2026-10-13
(run 38382124322, v1.8.1 at 0d3860b, six smokes): checksum verified, server healthy, both
smoke invocations executed; the green step also proves the in-step guards accepted the run,
so no silent skip or empty filter occurred. The encrypted binary-resource smoke and the
rotation-pointer smoke passed in run 38506261099 (sha a4c30ed), with the main CI green on the
same tree. Only ubuntu runs (pinned linux-x86_64-musl asset).

Goal: the S3-dependent ignored tests run in CI so their behavior cannot rot between manual runs
(retires part of the roadmap's Rust ignored-test debt item).

S3-dependent ignored tests today (8):

- `sync::v1::s3_smoke::*` × 7 and `clipboard_sync::s3::tests` × 1 (correctness smokes);
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
   everything after it. **Done** (see V2-2).
2. V2-1 engine level plus rustfs smoke. D1/D2 are decided and D3/D4 stand unless objected to,
   so this unit is unblocked; build the rotation flow, then the peer pointer check.
3. V2-1 frontend: dedicated destructive flow with progress, cancellation and confirmation.
4. V2-3 as a scheduled evidence run.

Cross-references: `TODO.md` open item C ("同步 v2"), `docs/SYNC_V1.md`, and the roadmap's Rust
ignored-test debt item (V2-2 retires part of it).
