# Sync v3: S3-first replication protocol

Status: approved design for the incompatible sync rewrite. The implementation may land in
small verified commits, but it must not preserve or read the baseline/oplog wire format.

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

## Remote namespace

All v3 objects live below the configured remote prefix in an isolated `v3/` namespace:

```text
v3/
├─ heads/{device_id}.bin
├─ checkpoint.bin
├─ checkpoints/{generation:020}-{sha256}.pack
├─ snapshots/{device_id}/{epoch}/{sha256}.pack
├─ segments/{device_id}/{epoch}/{first:020}-{last:020}-{sha256}.pack
└─ resources/{category}/sha256-{digest}.{ext}
```

- `device_id` is the persisted UUID belonging to one local database.
- `epoch` changes only when that device must publish a replacement bootstrap snapshot.
- A device writes only its own head, snapshot and segment namespace.
- Snapshot, segment and checkpoint names are content addressed and immutable.
- Resource names are content addressed over the raw bytes. Packs contain references, never a
  second inline copy of an uploaded resource.
- Fixed-width sequence numbers preserve lexical and numeric order for S3 `start-after` listing.

Normal discovery lists `v3/heads/`, which is O(device count). Segment listing starts after the
last applied object key for that device and is O(new segment count), including when a namespace
contains more than S3's 1,000-object page size.

## Wire envelopes

Every binary object begins with an explicit magic, format version and object kind. The logical
payload is bincode encoded, zstd compressed and then optionally AES-256-GCM encrypted:

```text
magic | version | kind | flags | nonce? | compressed-or-encrypted payload
```

- One sync run derives the encryption key once and reuses that key for all object envelopes.
- Every encrypted object still uses a fresh random nonce.
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
occurs after step 5 but before step 6, the same logical range may be republished under the same
content-addressed name; receivers remain idempotent.

## Incremental pull

For each remote head:

1. reject malformed ids, epochs, keys, hashes or sequence regressions;
2. if the epoch changed or the local cursor predates the advertised snapshot, apply that snapshot;
3. list segment keys strictly after the saved key and at or below the head's published sequence;
4. verify object-name hash, decrypt, decompress and decode with bounded limits;
5. fetch and validate missing resources before changing database paths;
6. apply all mutations and advance that device cursor in one SQLite transaction.

Objects newer than the downloaded head are ignored until a later run. This makes the head the
publication barrier and prevents observing a partially uploaded batch.

## Global checkpoint and safe garbage collection

`checkpoint.bin` points to one immutable global checkpoint and carries an ETag-protected generation
plus a vector clock of `{device_id, epoch, sequence}` entries.

A compactor:

1. reads the current checkpoint pointer and all device heads;
2. materializes the previous checkpoint plus every segment up to a frozen vector;
3. writes a checkpoint containing the winning record version for every id, including tombstones;
4. conditionally replaces `checkpoint.bin` with `If-Match` (or `If-None-Match: *` initially);
5. only after the conditional publish succeeds, deletes snapshots and segments fully covered by
   the published vector;
6. retains the immediately previous checkpoint until the new pointer and cleanup are revalidated.

Concurrent compactors are safe: only the conditional pointer update winner may perform garbage
collection. A segment crossing a checkpoint boundary is retained. Tombstones are never discarded
merely because the corresponding live row is absent; dropping them would allow an offline device
to resurrect deleted records.

## Legacy reset and deletion boundary

There is no reader, migration or fallback for the old baseline/oplog format. During the first v3
initialization of a configured remote scope, the client may directly delete the old remote data:

- objects whose basename starts with `baseline-`;
- objects whose basename starts with `oplog-`;
- every object strictly below the old `resources/` prefix;
- the local legacy pool-manifest file for that exact remote scope.

Deletion rules:

1. list the exact configured prefix with full pagination;
2. normalize and validate every candidate key before deleting it;
3. never delete the configured prefix itself, a parent prefix, or any object below `v3/`;
4. mark legacy cleanup complete only after every selected delete succeeds;
5. publish no v3 head until cleanup and the bootstrap snapshot both succeed.

Local clipboard records remain intact. Only legacy sync tables/state, old remote objects and the
old local pool manifest are reset.

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
