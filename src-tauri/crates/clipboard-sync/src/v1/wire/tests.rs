//! Sync v1 wire codec tests, moved verbatim with the module.

use super::*;

fn sample_item() -> SyncItem {
    SyncItem {
        id: "text-device-1".to_string(),
        kind: SyncItemKind::Text,
        title: "repetitive title ".repeat(10),
        text_content: Some("repetitive clipboard content ".repeat(50)),
        html_content: None,
        rtf_content: None,
        resource_path: None,
        preview_path: None,
        content_hash: "content-hash".to_string(),
        source_app: Some("test".to_string()),
        icon_path: None,
        size_bytes: 1024,
        created_at_ms: 100,
        last_used_at_ms: None,
        is_favorite: false,
        metadata_json: Some("{}".to_string()),
    }
}

fn sample_version() -> RecordVersion {
    RecordVersion {
        modified_at_ms: 200,
        writer_device_id: "c527a31e-7f42-43cf-bf73-6e5fbed4be18".to_string(),
    }
}

fn sample_segment() -> Segment {
    Segment {
        device_id: "c527a31e-7f42-43cf-bf73-6e5fbed4be18".to_string(),
        epoch: "e04623ec-6109-4275-a748-8743f3076b7d".to_string(),
        first_sequence: 1,
        last_sequence: 1,
        mutations: MutationBatch {
            upserts: vec![ReplicatedItem {
                item: sample_item(),
                version: sample_version(),
            }],
            tombstones: Vec::new(),
        },
    }
}

#[test]
fn plaintext_segment_round_trips_and_is_compressed() {
    let segment = sample_segment();
    let raw = bincode::encode_to_vec(&segment, bincode::config::standard()).unwrap();
    let encoded = encode_segment(&segment, None).unwrap();
    assert_eq!(&encoded.bytes[..MAGIC.len()], MAGIC);
    assert!(encoded.bytes.len() < raw.len());
    assert_eq!(encoded.sha256, hex::encode(Sha256::digest(&encoded.bytes)));
    assert_eq!(decode_segment(&encoded.bytes, None).unwrap(), segment);
}

#[test]
fn encrypted_segment_retries_are_content_addressed() {
    let key = SessionKey::derive("correct horse battery staple", "remote-a").unwrap();
    let segment = sample_segment();
    let first = encode_segment(&segment, Some(&key)).unwrap();
    let second = encode_segment(&segment, Some(&key)).unwrap();
    assert_eq!(first.bytes, second.bytes);
    assert_eq!(first.sha256, second.sha256);
    assert_eq!(decode_segment(&first.bytes, Some(&key)).unwrap(), segment);
    assert_eq!(decode_segment(&second.bytes, Some(&key)).unwrap(), segment);
}

#[test]
fn wrong_password_and_corruption_fail_explicitly() {
    let right = SessionKey::derive("right", "remote-a").unwrap();
    let wrong = SessionKey::derive("wrong", "remote-a").unwrap();
    let mut encoded = encode_segment(&sample_segment(), Some(&right)).unwrap();
    assert!(decode_segment(&encoded.bytes, Some(&wrong))
        .unwrap_err()
        .contains("wrong password or corrupted data"));
    let last = encoded.bytes.len() - 1;
    encoded.bytes[last] ^= 0x01;
    assert!(decode_segment(&encoded.bytes, Some(&right))
        .unwrap_err()
        .contains("wrong password or corrupted data"));
}

#[test]
fn decoder_rejects_kind_mismatch_and_oversized_claims() {
    let encoded = encode_segment(&sample_segment(), None).unwrap();
    assert!(
        decode_value::<MutationBatch>(ObjectKind::Snapshot, &encoded.bytes, None)
            .unwrap_err()
            .contains("kind mismatch")
    );

    let mut oversized = encoded.bytes;
    oversized[12..20].copy_from_slice(&(MAX_UNCOMPRESSED_BYTES + 1).to_le_bytes());
    assert!(decode_segment(&oversized, None)
        .unwrap_err()
        .contains("uncompressed size limit"));
}

#[test]
fn encrypted_objects_require_a_key_without_plaintext_fallback() {
    let key = SessionKey::derive("password", "remote-a").unwrap();
    let encoded = encode_segment(&sample_segment(), Some(&key)).unwrap();
    assert!(decode_segment(&encoded.bytes, None)
        .unwrap_err()
        .contains("requires an encryption password"));
}

#[test]
fn encrypted_scope_rejects_plaintext_objects() {
    let key = SessionKey::derive("password", "remote-a").unwrap();
    let encoded = encode_segment(&sample_segment(), None).unwrap();
    assert!(decode_segment(&encoded.bytes, Some(&key))
        .unwrap_err()
        .contains("requires encryption"));
}

#[test]
fn chunked_snapshot_pack_round_trips_without_a_whole_pack_buffer() {
    let directory = std::env::temp_dir().join(format!(
        "clipboard-sync-pack-test-{}-{:016x}",
        std::process::id(),
        rand::random::<u64>()
    ));
    let key = SessionKey::derive("password", "remote-a").unwrap();
    let header = SnapshotPackHeader {
        device_id: "c527a31e-7f42-43cf-bf73-6e5fbed4be18".to_string(),
        epoch: "e04623ec-6109-4275-a748-8743f3076b7d".to_string(),
        through_sequence: 42,
    };
    let first = sample_segment().mutations;
    let second = MutationBatch {
        upserts: Vec::new(),
        tombstones: vec![Tombstone {
            item_id: "deleted-item".to_string(),
            kind: SyncItemKind::Text,
            content_hash: "deleted-hash".to_string(),
            deleted_at_ms: 300,
            version: sample_version(),
        }],
    };
    let encoded = encode_snapshot_pack(
        &directory,
        &header,
        [first.clone(), second.clone()],
        Some(&key),
    )
    .unwrap();
    let mut reader = open_snapshot_pack(encoded.path(), Some(&key)).unwrap();
    assert_eq!(reader.header, header);
    assert_eq!(reader.next().unwrap().unwrap(), first);
    assert_eq!(reader.next().unwrap().unwrap(), second);
    assert!(reader.next().is_none());
    assert!(reader.is_complete());
    assert_eq!(reader.record_count(), 2);
    drop(reader);
    drop(encoded);
    let _ = fs::remove_dir(&directory);
}

/// Writes a byte-exact unencrypted snapshot pack by hand so a test can lie
/// about the chunk sizes the real writer would never emit.
///
/// `declared_chunk` is written verbatim as the 24-byte chunk header, and
/// `payload` supplies the bytes that follow it.
fn write_crafted_snapshot_pack(
    path: &Path,
    uncompressed_size: u64,
    declared_chunk: [u8; PACK_CHUNK_HEADER_LEN],
    payload: &[u8],
) {
    let header_bytes = encode_pack_header(&SnapshotPackHeader {
        device_id: "c527a31e-7f42-43cf-bf73-6e5fbed4be18".to_string(),
        epoch: "e04623ec-6109-4275-a748-8743f3076b7d".to_string(),
        through_sequence: 1,
    })
    .unwrap();

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&header(
        ObjectKind::Snapshot,
        PACK_FLAG_CHUNKED,
        uncompressed_size,
    ));
    bytes.extend_from_slice(&(header_bytes.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&header_bytes);
    bytes.extend_from_slice(&declared_chunk);
    bytes.extend_from_slice(payload);
    fs::write(path, bytes).unwrap();
}

/// The declared chunk sizes are attacker-controlled, and the size checks only
/// compared them against the protocol limit. A single-record chunk may declare
/// up to 1 GiB, so a ~100-byte unencrypted pack passed every check and then
/// made the reader zero-fill a gigabyte before `read_exact` noticed the file
/// was truncated. Nothing authenticates an unencrypted pack header, so this is
/// reachable from any object an attacker can publish under a `*.pack` key.
#[test]
fn crafts_a_tiny_pack_that_declares_a_gigabyte() {
    let directory = std::env::temp_dir().join(format!(
        "clipboard-sync-pack-oversized-{}-{:016x}",
        std::process::id(),
        rand::random::<u64>()
    ));
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("crafted.pack");

    // A lone record may declare up to the global uncompressed limit, and a
    // stored payload may add the nonce and auth tag on top of that.
    let raw_size = (PACK_CHUNK_SINGLE_RECORD_MAX_UNCOMPRESSED_BYTES - 1) as u32;
    let stored_size =
        (PACK_CHUNK_SINGLE_RECORD_MAX_UNCOMPRESSED_BYTES + NONCE_LEN + AUTH_TAG_LEN) as u32;
    let mut chunk = [0u8; PACK_CHUNK_HEADER_LEN];
    chunk[0..8].copy_from_slice(&0u64.to_le_bytes());
    chunk[8..12].copy_from_slice(&1u32.to_le_bytes());
    chunk[12..16].copy_from_slice(&raw_size.to_le_bytes());
    chunk[16..20].copy_from_slice(&stored_size.to_le_bytes());
    // Trailing 16 bytes keep this well inside the other size checks so the
    // remaining-bytes check is the one that has to fire.
    write_crafted_snapshot_pack(&path, raw_size as u64, chunk, &[0u8; 16]);

    assert!(
        fs::metadata(&path).unwrap().len() < 1024,
        "the crafted pack must stay tiny so the test proves a small input cannot \
         demand a large allocation"
    );

    let mut reader = open_snapshot_pack(&path, None).expect("the header itself is well formed");
    let message = reader
        .next()
        .expect("the chunk header is present, so the pack is not empty")
        .expect_err("a chunk larger than the remaining file must be rejected");
    // The message names the remaining byte count, which is what proves the
    // rejection happened before the allocation rather than after the failed
    // read.
    assert!(
        message.contains("remain"),
        "expected a remaining-bytes rejection, got {message:?}"
    );
    assert!(
        !message.contains("truncated"),
        "the payload is not truncated, it never fit: {message:?}"
    );

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn near_limit_high_entropy_chunk_round_trips() {
    let directory = std::env::temp_dir().join(format!(
        "clipboard-sync-pack-size-test-{}-{:016x}",
        std::process::id(),
        rand::random::<u64>()
    ));
    // Printable ASCII maximizes per-byte entropy for a UTF-8 String field.
    // If even this near-limit chunk survives, zstd's framing overhead
    // cannot push a real (String-only) chunk past the decoder's stored-size
    // bound, so the encoder/decoder size limits agree in practice.
    let target = large_pack_chunk_limit_bytes() - 8192;
    let mut text = String::with_capacity(target);
    for _ in 0..target {
        let byte = 0x20 + (rand::random::<u8>() % 95);
        text.push(byte as char);
    }
    let mut item = sample_item();
    item.text_content = Some(text);
    let batch = MutationBatch {
        upserts: vec![ReplicatedItem {
            item,
            version: sample_version(),
        }],
        tombstones: Vec::new(),
    };
    let raw_size = mutation_batch_encoded_size(&batch).unwrap();
    assert!(
        raw_size <= large_pack_chunk_limit_bytes(),
        "raw batch size {raw_size} must fit the chunk limit"
    );

    let header = SnapshotPackHeader {
        device_id: "c527a31e-7f42-43cf-bf73-6e5fbed4be18".to_string(),
        epoch: "e04623ec-6109-4275-a748-8743f3076b7d".to_string(),
        through_sequence: 1,
    };
    let encoded = encode_snapshot_pack(&directory, &header, [batch.clone()], None).unwrap();
    let mut reader = open_snapshot_pack(encoded.path(), None).unwrap();
    assert_eq!(reader.next().unwrap().unwrap(), batch);
    assert!(reader.next().is_none());
    drop(reader);
    drop(encoded);
    let _ = fs::remove_dir(&directory);
}

/// The zstd framing headroom the split threshold reserves must actually
/// cover the worst case, otherwise the "safe" routing budget still admits a
/// chunk the reader rejects.
///
/// This measures it: compress high-entropy data at the packing target and
/// assert the frame overhead stays inside the reserved headroom. Without
/// this, `large_pack_chunk_raw_budget_bytes` is an unverified constant and
/// the encoder/reader disagreement is back.
#[test]
fn framing_headroom_covers_the_worst_case_at_the_packing_target() {
    let limit = large_pack_chunk_limit_bytes();
    assert_eq!(
        large_pack_chunk_raw_budget_bytes() + PACK_CHUNK_FRAMING_HEADROOM_BYTES,
        limit,
        "the raw routing budget plus its headroom must reconstruct the packing target"
    );

    let mut state: u32 = 0x9e37_79b9;
    let mut raw = Vec::with_capacity(limit);
    while raw.len() < limit {
        // Full-byte-range LCG output: zstd cannot find structure in this.
        for _ in 0..4 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            raw.push((state >> 16) as u8);
        }
    }
    raw.truncate(limit);
    let compressed =
        zstd::stream::encode_all(Cursor::new(&raw), ZSTD_LEVEL).expect("compress probe");
    let overhead = compressed.len().saturating_sub(raw.len());
    assert!(
        overhead <= PACK_CHUNK_FRAMING_HEADROOM_BYTES,
        "zstd framing overhead at the {limit}-byte target was {overhead} bytes, above the \
         reserved {PACK_CHUNK_FRAMING_HEADROOM_BYTES}-byte headroom"
    );
}

/// A multi-record batch the engine routes into a single chunk must stay
/// readable.
///
/// `write_large_pack_batch` emits a multi-record chunk whenever the batch's
/// uncompressed size is within `large_pack_chunk_raw_budget_bytes()`. That
/// is the exact case the pre-fix code got wrong: the encoder's raw check
/// passed and the reader then rejected the stored size. Building the batch
/// just under the routing budget and reading it back is the regression this
/// test exists for.
#[test]
fn a_routed_multi_record_chunk_at_the_threshold_stays_readable() {
    let directory = std::env::temp_dir().join(format!(
        "clipboard-sync-pack-routed-test-{}-{:016x}",
        std::process::id(),
        rand::random::<u64>()
    ));
    let routing_budget = large_pack_chunk_raw_budget_bytes();
    let mut state: u32 = 0x9e37_79b9;
    let mut incompressible = |bytes: usize| -> String {
        let text: String = (0..bytes / 2)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                char::from((0x80u32 + (state >> 24) % 0x60) as u8)
            })
            .collect();
        text
    };

    // Shrink-to-fit so the batch lands just under the routing budget rather
    // than guessing at the per-record bincode overhead.
    let mut text_bytes = routing_budget;
    let batch = loop {
        let mut big = sample_item();
        big.text_content = Some(incompressible(text_bytes));
        let candidate = MutationBatch {
            upserts: vec![
                ReplicatedItem {
                    item: big,
                    version: sample_version(),
                },
                ReplicatedItem {
                    item: sample_item(),
                    version: sample_version(),
                },
            ],
            tombstones: Vec::new(),
        };
        let size = mutation_batch_encoded_size(&candidate).unwrap();
        if size <= routing_budget {
            break candidate;
        }
        text_bytes -= size - routing_budget;
    };
    let raw_size = mutation_batch_encoded_size(&batch).unwrap();
    assert!(
        raw_size > routing_budget - PACK_CHUNK_FRAMING_HEADROOM_BYTES * 2,
        "the batch must sit at the routing threshold, got {raw_size} vs {routing_budget}"
    );

    let header = SnapshotPackHeader {
        device_id: "c527a31e-7f42-43cf-bf73-6e5fbed4be18".to_string(),
        epoch: "e04623ec-6109-4275-a748-8743f3076b7d".to_string(),
        through_sequence: 1,
    };
    let encoded = encode_snapshot_pack(&directory, &header, [batch.clone()], None)
        .expect("a routed multi-record chunk must encode");
    let mut reader = open_snapshot_pack(encoded.path(), None).expect("pack must open");
    let decoded = reader
        .next()
        .transpose()
        .expect("a routed multi-record chunk must decode")
        .expect("the chunk must be present");
    assert_eq!(decoded, batch);
    assert!(reader.next().is_none(), "no extra chunk expected");
    drop(reader);
    drop(encoded);
    let _ = fs::remove_dir(&directory);
}

#[test]
fn oversized_single_record_chunk_round_trips() {
    let directory = std::env::temp_dir().join(format!(
        "clipboard-sync-pack-oversize-test-{}-{:016x}",
        std::process::id(),
        rand::random::<u64>()
    ));
    // One record above the 16 MiB packing target must still be storable: a
    // lone record cannot be split, and failing here previously wedged every
    // snapshot/checkpoint publication for the whole database.
    let mut item = sample_item();
    item.text_content = Some("a".repeat(large_pack_chunk_limit_bytes() + 4096));
    let batch = MutationBatch {
        upserts: vec![ReplicatedItem {
            item,
            version: sample_version(),
        }],
        tombstones: Vec::new(),
    };
    assert!(
        mutation_batch_encoded_size(&batch).unwrap() > large_pack_chunk_limit_bytes(),
        "test batch must exceed the normal chunk limit"
    );

    let header = SnapshotPackHeader {
        device_id: "c527a31e-7f42-43cf-bf73-6e5fbed4be18".to_string(),
        epoch: "e04623ec-6109-4275-a748-8743f3076b7d".to_string(),
        through_sequence: 1,
    };
    // The normal chunk writer rejects it; the single-record writer stores it.
    let mut rejected =
        LargePackWriter::new(&directory, LargePackKind::Snapshot, &header, None).unwrap();
    assert!(rejected.write_batch(&batch).is_err());

    let mut writer =
        LargePackWriter::new(&directory, LargePackKind::Snapshot, &header, None).unwrap();
    writer.write_single_oversized_batch(&batch).unwrap();
    let encoded = writer.finish().unwrap();
    let mut reader = open_snapshot_pack(encoded.path(), None).unwrap();
    assert_eq!(reader.next().unwrap().unwrap(), batch);
    assert!(reader.next().is_none());
    assert!(reader.is_complete());
    drop(reader);
    drop(encoded);
    let _ = fs::remove_dir(&directory);
}

#[test]
fn chunked_pack_rejects_wrong_password_and_tampering() {
    let directory = std::env::temp_dir().join(format!(
        "clipboard-sync-pack-corruption-test-{}-{:016x}",
        std::process::id(),
        rand::random::<u64>()
    ));
    let right = SessionKey::derive("right", "remote-a").unwrap();
    let wrong = SessionKey::derive("wrong", "remote-a").unwrap();
    let header = CheckpointPackHeader {
        generation: 1,
        vector: vec![DeviceCursor {
            device_id: "c527a31e-7f42-43cf-bf73-6e5fbed4be18".to_string(),
            epoch: "e04623ec-6109-4275-a748-8743f3076b7d".to_string(),
            sequence: 1,
            last_segment_key: None,
        }],
    };
    let encoded = encode_checkpoint_pack(
        &directory,
        &header,
        [sample_segment().mutations],
        Some(&right),
    )
    .unwrap();
    assert!(open_checkpoint_pack(encoded.path(), Some(&wrong))
        .unwrap_err()
        .contains("wrong password or corrupted data"));

    let mut bytes = fs::read(encoded.path()).unwrap();
    *bytes.last_mut().unwrap() ^= 0x01;
    fs::write(encoded.path(), bytes).unwrap();
    let mut reader = open_checkpoint_pack(encoded.path(), Some(&right)).unwrap();
    assert!(reader
        .next()
        .unwrap()
        .unwrap_err()
        .contains("wrong password or corrupted data"));
    drop(reader);
    drop(encoded);
    let _ = fs::remove_dir(&directory);
}
