# Sync v1: S3-first replication protocol

Status: first and only supported sync protocol. The implementation may land in small verified
commits, but it must not preserve, read or convert the baseline/oplog wire format.

## Workload and priorities

The protocol is sized for at least three devices, each beginning with roughly 100,000 distinct
clipboard records and adding about 200 records per day.

Priorities, in order:

1. keep normal local capture and reads independent of network latency;
2. minimize local and remote storage amplification;
3. upload and download only new metadata and previously unseen binary resources;
4. avoid remote work proportional to the complete history during an ordinary sync;
5. remain crash-safe and convergent when devices sync concurrently or return after being offline.

Sync latency is not traded for mutable remote log files. Every published data pack is immutable;
the small per-device head is the only routinely overwritten object.

## Runtime entry points

The desktop runtime supports only S3-compatible storage. `get_sync_config`, `set_sync_config`,
typed `test_sync_connection`, and `sync_now` are the complete Tauri surface; automatic sync calls
the same internal `run_sync` function. The old remote-backup list/download/verification,
compaction, WebDAV, baseline and oplog IPC surfaces are not registered.

Each run snapshots configuration once, releases the config lock, derives at most one optional
remote-scoped encryption key, creates one prefix-scoped S3 object store, and enters the v1 engine.
The frontend exposes `segmentMaxEntries` plus image/file resource byte limits; it has no mutable-log
rollover or remote-file-retention settings.

## Remote namespace

All v1 objects live below the configured remote prefix in an isolated `v1/` namespace:

```text
v1/
├─ heads/{device_id}.bin
├─ checkpoint.bin
├─ checkpoints/{generation:020}-{sha256}.pack
├─ snapshots/{device_id}/{epoch}/{sha256}.pack
├─ segments/{device_id}/{epoch}/{first:020}-{last:020}-{sha256}.pack
└─ resources/{image|file|icon}/sha256-{digest}.{ext}
```

- `device_id` is the persisted UUID belonging to one local database.
- `epoch` changes only when that device must publish a replacement bootstrap snapshot.
- A device writes only its own head, snapshot and segment namespace.
- Snapshot, segment and checkpoint names are content addressed and immutable.
- Resource names are content addressed over the raw bytes. Packs contain references, never a
  second inline copy of an uploaded resource.
- Preview images are device-local derived data: packs clear `preview_path` and `previewPath`, never
  create `resources/preview/` objects, and receivers can display the original image until a local
  preview is rebuilt.
- Fixed-width sequence numbers preserve lexical and numeric order for S3 `start-after` listing.

Normal discovery lists `v1/heads/`, which is O(device count). Segment listing starts after the
last applied object key for that device and is O(new segment count), including when a namespace
contains more than S3's 1,000-object page size.

## Wire envelopes

Every binary object begins with an explicit magic, format version and object kind. The logical
payload is bincode encoded, zstd compressed and then optionally AES-256-GCM encrypted:

```text
magic | version | kind | flags | nonce? | compressed-or-encrypted payload
```

- One sync run derives the encryption key once and reuses that key for all object envelopes.
- Immutable encrypted objects derive their nonce from the authenticated header and compressed
  plaintext. The same logical retry therefore produces the same ciphertext and content-addressed
  key, while distinct plaintexts receive distinct nonces with SHA-256 collision resistance.
  Equality is already visible from the immutable object key.
- A password mismatch or authentication failure is a hard error. Ciphertext is never retried as
  plaintext.
- SHA-256 in an object name is computed over the final stored bytes, allowing corruption checks
  before decoding.
- Decoders enforce size, entry-count and decompression limits before allocating untrusted sizes.

## Replicated record version

Every upsert and tombstone carries a deterministic version:

```text
(modified_at_ms, writer_device_id)
```

Versions compare lexicographically. The writer UUID is the stable tie-break when two clocks yield
the same millisecond. Applying the same or an older version is a no-op, making snapshot and segment
replay idempotent.

The local database stores the current writer beside each clipboard row and stores compact
tombstones separately. A remote apply runs in one transaction with changelog suppression enabled,
updates the search outbox normally, and never echoes the received mutation back to S3.

`last_used_at_ms` is device-local usage state, not replicated record content. Local-only updates to
that field neither advance the record version nor enter the sync outbox. Exported snapshots and
segments clear it; a remotely inserted row initializes it from `created_at_ms`, while later remote
content updates preserve the receiving device's current value.

Soft deletion is the replicated mutation boundary. Its trigger writes the winning tombstone and one
outbox delete; later permanent removal of an already-deleted row only reclaims local SQLite/resource
state and does not enqueue the same delete again. A direct hard delete of an active row remains a
replicated delete so non-recycle-bin cleanup paths cannot silently lose convergence.

## Bootstrap

Bootstrap order is intentionally upload-first so an existing local collection is not confused
with records downloaded during the same first sync:

1. acquire the process-wide sync lock and take a consistent SQLite view;
2. assign the current device epoch and export active local records plus known tombstones;
3. upload all missing content-addressed resources;
4. upload one immutable bootstrap snapshot;
5. publish the device head only after the snapshot and resources are durable;
6. download the current global checkpoint, if any;
7. list every device head and apply snapshots/segments not covered by the checkpoint or local
   cursors;
8. persist cursors only in the same transaction that successfully applies a pack.

The snapshot pack contains resource references only. A failed resource upload aborts publication;
the head can never point to a snapshot with dangling resources.

Before any bootstrap or incremental publication, a device reads and validates its own remote head.
If the local publication state was restored, lost, or otherwise differs from that head, the client
first applies the remote device history back into SQLite, rotates to a new epoch, and publishes one
replacement bootstrap snapshot. It never overwrites a newer or divergent remote head with an older
local sequence. If the local state claims initialization but the remote head is missing, the same
epoch-rotation/bootstrap path recreates it from the complete local materialized state.

## Incremental push

Local mutation triggers append compact outbox rows containing sequence, item id, operation and
version metadata. They do not copy binary resource bytes into SQLite.

For each push:

1. read a bounded sequence range and materialize the latest record state for affected ids;
2. coalesce repeated mutations of the same id within the selected range;
3. upload previously unseen resources;
4. encode, compress and upload an immutable segment;
5. overwrite only the local device head with the new published sequence and segment key;
6. advance the local published cursor and purge acknowledged outbox rows in one transaction.

If a crash occurs before step 5, the orphan immutable segment is unreachable and harmless. If it
occurs after step 5 but before step 6, deterministic immutable encoding republishes the same
logical range under the same content-addressed name; receivers remain idempotent.

## Incremental pull

For each remote head:

1. reject malformed ids, epochs, keys, hashes or sequence regressions;
2. if the epoch changed or the local cursor predates the advertised snapshot, apply that snapshot;
3. list segment keys strictly after the saved key and at or below the head's published sequence;
4. verify object-name hash, decrypt, decompress and decode with bounded limits;
5. fetch and validate missing resources before changing database paths;
6. apply all mutations and advance that device cursor in one SQLite transaction.

Segment ranges must be continuous: the next object begins at exactly `cursor.sequence + 1`.
Missing/overlapping ranges, an unavailable bootstrap snapshot, or another incomplete chain triggers
a forced global-checkpoint apply and one pull retry. A database with no peer cursors applies the
current checkpoint before scanning heads, so a fresh fourth device does not need compacted peer
history. Checkpoint mutation state, the complete cursor vector, generation and digest commit in one
SQLite transaction. If the current immutable checkpoint is corrupt or missing, the retained
previous checkpoint is tried; ordinary devices with cursors do not download checkpoint bodies on
idle runs.

Objects newer than the downloaded head are ignored until a later run. This makes the head the
publication barrier and prevents observing a partially uploaded batch.

Head parsing, download, validation and application errors are isolated per remote device. A bad
peer increments the run's `failedPeers` count and is reported as a partial run, while remaining
device heads continue in lexical order. Failure to list the head namespace remains a whole-run
error because discovery cannot proceed safely.

## Global checkpoint and safe garbage collection

`checkpoint.bin` points to one immutable global checkpoint and carries an ETag-protected generation
plus a vector clock of `{device_id, epoch, sequence}` entries. Each database stores the vector of
the last checkpoint it successfully applied or published in `sync_checkpoint_cursors`; that compact
local baseline lets an idle run decide that compaction is not due without reading `checkpoint.bin`.

Compaction is due when no local checkpoint baseline exists, when the device/epoch set changes, or
when the sum of sequence advances since that baseline reaches 50,000. A run with any failed peer
does not compact because its materialized view is not known to cover every discoverable head. The
compactor also skips publication while local outbox state is not fully represented by the published
local sequence.

A compactor:

1. freezes the locally published device state plus every successfully applied peer cursor;
2. reads and validates the current checkpoint pointer/body only when the local baseline says work
   is due, rejecting a candidate that drops a known device or regresses a sequence in the same epoch;
3. exports the complete local materialized state, including tombstones, and uploads missing
   content-addressed resources;
4. writes one immutable checkpoint containing that complete winning state and frozen vector;
5. conditionally replaces `checkpoint.bin` with `If-Match` (or `If-None-Match: *` initially);
6. only after the conditional publish succeeds, deletes snapshots and segments covered by the
   immediately previous checkpoint vector;
7. retains the current and immediately previous checkpoints and atomically records the new local
   baseline only after cleanup completes.

Concurrent compactors are safe: only the conditional pointer update winner may perform garbage
collection. A delayed winner never prunes a checkpoint pack from its own or a higher generation;
same-generation losing candidates are therefore harmless and become eligible only after a later
generation advances. Cleanup is restartable: if the pointer was published but the process stopped
before GC/local-baseline recording, a later run with the same vector revalidates the checkpoint and
finishes cleanup. A segment crossing a checkpoint boundary is retained. Tombstones are never
discarded merely because the corresponding live row is absent; dropping them would allow an offline
device to resurrect deleted records.

## Obsolete-state deletion boundary

There is no reader, migration or fallback for the old baseline/oplog format. During first v1
initialization of a configured remote scope, the client directly deletes the obsolete remote data:

- top-level objects whose name starts with `baseline-`;
- top-level objects whose name starts with `oplog-`;
- every object strictly below the old `resources/` prefix;
- the obsolete local pool-manifest file for that exact remote scope.

Deletion rules:

1. list the exact configured prefix with full pagination;
2. normalize and validate every candidate key before deleting it;
3. never delete the configured prefix itself, a parent prefix, or any object below `v1/`;
4. mark remote preparation complete only after every selected delete succeeds;
5. publish no v1 head until cleanup and the bootstrap snapshot both succeed.

Local clipboard records remain intact. All discarded sync-only tables/state, old remote objects
and the obsolete local pool manifest are deleted, not converted.

## Performance acceptance criteria

Synthetic verification uses three independent 100,000-record databases followed by 200 new
records per device. It records both raw and compressed sizes and counts every simulated request.

The implementation is accepted only when:

- an idle sync reads O(device count) heads and transfers no snapshot/segment/resource bodies;
- one device's incremental sync transfers only its new segment, changed head and new resources;
- another device downloads only that segment and resources it does not already have;
- first convergence produces the 300,000-record union without echo-generated outbox rows;
- replaying any snapshot or segment changes zero already-current rows;
- listing and pulling remain correct beyond 1,000 segment objects;
- a checkpointed namespace can delete only vector-covered history and still bootstrap a fresh
  fourth database to the same materialized state;
- encrypted corruption and wrong-password cases fail explicitly without applying partial data.

Benchmarks must report elapsed encode/decode/apply time, peak pack size, SQLite file growth, S3 PUT/
GET/LIST/DELETE counts and total uploaded/downloaded bytes. Correctness gates precede performance
claims.
