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
typed `test_sync_connection`, `sync_now`, and on-demand `materialize_clipboard_item` are the complete
sync Tauri surface; automatic sync calls the same internal `run_sync` function. The old remote-backup list/download/verification,
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
- Resource names are content addressed over the raw bytes when encryption is disabled. With a
  sync password, the digest is `HMAC-SHA256(session_key, plaintext_sha256)`, so devices using the
  same remote scope/password still deduplicate while the bucket does not expose the raw content
  hash and a different password cannot collide with the old resource namespace. Packs contain
  references, never a second inline copy of an uploaded resource.
- Preview images are device-local derived data: packs clear `preview_path` and `previewPath`, never
  create `resources/preview/` objects, and receivers can display the original image until a local
  preview is rebuilt.
- Fixed-width sequence numbers preserve lexical and numeric order for S3 `start-after` listing.

Normal discovery performs one paginated `v1/heads/` listing, which is O(device count). The client
retains a disposable per-scope/device head cache only after that head has been fully validated and
its advertised state has been published or applied. A later listing can skip the head body GET only
when its ETag and size match the cache and the cached epoch/sequence/segment still exactly match the
authoritative local publication state or peer cursor. `LastModified` is an additional mismatch
signal when both values are available, never the sole content identity. A missing ETag/size,
corrupt cache, changed listing metadata, restored SQLite state, or cursor mismatch falls back to a
normal GET/decode/validation. Segment listing starts after the last applied object key for that
device and is O(new segment count), including when a namespace contains more than S3's 1,000-object
page size.

## Wire envelopes

Every metadata object begins with an explicit magic, format version and object kind. The logical
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
- Encrypted and unencrypted metadata objects are mutually exclusive for one configured scope: a
  client with a password rejects plaintext envelopes, and a client without one rejects encrypted
  envelopes. Before an uninitialized device publishes any bootstrap object, it authenticates an
  existing canonical device/checkpoint pointer when the namespace is non-empty. A wrong, removed,
  or changed password therefore fails before the new device writes a head.
- SHA-256 in an object name is computed over the final stored bytes, allowing corruption checks
  before decoding.
- Decoders enforce size, entry-count and decompression limits before allocating untrusted sizes.

Snapshots and checkpoints keep one immutable S3 object each, but their v1 payload is an internal
chunk stream rather than one monolithic bincode value. The small identity header is authenticated,
then deterministic batches of at most 2,048 records are independently bincode encoded, zstd
compressed and (when enabled) AES-256-GCM authenticated. A batch that exceeds the 16 MiB decoded
chunk cap is bisected deterministically; one record larger than that cap is rejected. Publication
first creates a point-in-time SQLite copy, releases the live database lock, exports that copy in
bounded batches to a temporary pack file, and performs one streaming S3 PUT. Pull performs one
streaming GET to a temporary file and applies decoded batches inside one SQLite transaction; any
late chunk, authentication, count or cursor failure rolls the entire snapshot/checkpoint back.
Temporary files are removed on every success and failure path. Segments and small pointer objects
retain the compact single-envelope encoding, so the ordinary 200-record daily path adds no S3
requests and no chunk-container overhead.

Binary resources use the same `CLPSYNC1` format version with an internal resource object kind, but
are not bincode encoded or compressed. With a password they are streamed through fixed 1 MiB
AES-256-GCM chunks: a 20-byte authenticated-format header records the plaintext size, and each
non-empty chunk adds a 16-byte authentication tag. The nonce is deterministically derived from the
session key, canonical resource object key and chunk index; AAD binds the header, object key and
chunk index. This keeps retries byte-identical without loading the whole resource into memory.
Upload encryption writes a ciphertext temporary file and then uses the streaming S3 file PUT;
download writes a bounded ciphertext temporary file, authenticates/decrypts one chunk at a time to
a plaintext temporary file, verifies the keyed content identity, and only then atomically publishes
the local cache. Wrong passwords, truncation, header/size disagreement and modified chunks are hard
failures with no plaintext fallback. Reported resource traffic counts actual stored ciphertext
bytes, including header and tags.

Changing, adding or removing the password of an already initialized remote scope is not an in-place
operation. The existing namespace remains bound to its original encryption mode/key. The current
settings surface intentionally rejects such a change before publication; a future dedicated,
destructive rotation workflow must first materialize every referenced resource, delete the old v1
namespace, clear scope-local sync state, and republish under the new key. Merely editing the setting
never creates a mixed or partly unreadable namespace.

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

Bootstrap order is intentionally upload-first after remote access has been authenticated, so an
existing local collection is not confused with records downloaded during the same first sync:

1. acquire the process-wide sync lock and authenticate an existing canonical pointer when the v1
   namespace is non-empty;
2. create a point-in-time SQLite copy, release the live database lock, assign the current device
   epoch and export active local records plus known tombstones in bounded batches;
3. upload all missing content-addressed resources;
4. upload one immutable bootstrap snapshot;
5. publish the device head only after the snapshot and resources are durable;
6. download the current global checkpoint, if any;
7. list every device head and apply snapshots/segments not covered by the checkpoint or local
   cursors;
8. persist cursors only in the same transaction that successfully applies a pack.

The snapshot pack contains resource references only. A failed resource upload aborts publication;
the head can never point to a snapshot with dangling resources.

Before any bootstrap or incremental publication, a device reconciles its own remote head. An exact
LIST ETag/size plus cached logical-head/local-publication-state match can prove it unchanged without
another body GET; otherwise the client reads and validates it. If the local publication state was
restored, lost, or otherwise differs from that head, the client first applies the remote device
history back into SQLite, rotates to a new epoch, and publishes one replacement bootstrap snapshot.
It never overwrites a newer or divergent remote head with an older local sequence. If the local
state claims initialization but the remote head is missing, the same epoch-rotation/bootstrap path
recreates it from the complete local materialized state.

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

For each listed remote head:

1. skip the body GET only when listing ETag/size, cached logical state and the applied cursor all
   match; otherwise GET/decode the head and reject malformed ids, epochs, keys, hashes or sequence
   regressions;
2. if the epoch changed or the local cursor predates the advertised snapshot, apply that snapshot;
3. list segment keys strictly after the saved key and at or below the head's published sequence;
4. verify object-name hash, decrypt, decompress and decode with bounded limits;
5. validate canonical resource keys, strip them from device-local path fields, and stage compact
   per-item resource references without downloading blob bodies;
6. apply all mutations, resource references and the device cursor in one SQLite transaction, then
   best-effort persist the new disposable head cache.

An unmaterialized item can still be published in a later segment or checkpoint: scope-aware export
restores its stored canonical keys. Local image/file paths remain either usable paths on this device
or absent; an S3 object key is never written into a field consumed as a local filesystem path.

Copy, double-click paste, detail, fullscreen, save-as, or an explicit first drag attempt may request
materialization. The command verifies an existing local path by digest before network I/O, shares one
in-flight download per remote scope/object key, streams into a bounded temporary file, verifies the
canonical SHA-256 and atomically renames it. Every resource path for the item is then written in one
SQLite transaction while retaining the remote references; this local cache update neither advances
the replicated version nor enters the sync outbox. A failed or stale reference leaves the whole path
set unchanged. Images are sent to the local thumbnail worker after success; hovering/listing never
downloads body resources, and a drag with no path is cancelled until the user retries after download.

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
3. creates a point-in-time SQLite copy, exports the complete materialized state including
   tombstones in bounded batches, and uploads missing content-addressed resources without holding
   the live database lock across network I/O;
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

- after cache warmup, an idle sync performs one O(device count) head listing, zero head body GETs,
  and transfers no snapshot/segment/resource bodies; stores without listing ETags safely fall back
  to head GETs;
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
