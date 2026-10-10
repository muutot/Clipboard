//! Object-store primitives shared by every engine phase.

use super::*;

pub(super) fn write_large_pack_batch(
    writer: &mut LargePackWriter<'_>,
    mutations: MutationBatch,
) -> Result<(), String> {
    cancellation::check()?;
    if mutations.is_empty() {
        return Ok(());
    }
    let encoded_size = mutation_batch_encoded_size(&mutations)?;
    // Route on the *raw* budget, not the packing target: a batch that only just
    // fits uncompressed can compress to slightly more than the reader accepts
    // when its content is incompressible, and such a chunk would be written but
    // could never be read back. Splitting here keeps the export progressing.
    if encoded_size <= large_pack_chunk_raw_budget_bytes() {
        return writer.write_batch(&mutations);
    }
    if mutations.len() == 1 {
        // A lone record cannot be split, so emit it as one oversized chunk
        // (bounded by the global uncompressed limit) instead of failing every
        // snapshot/checkpoint publication of the whole database.
        return writer.write_single_oversized_batch(&mutations);
    }

    // Split upserts by halves first. A single upsert is emitted on its own so
    // the recursion always makes progress even when it is paired with
    // tombstones (both `len()/2 == 0` previously recursed on identical input).
    if mutations.upserts.len() > 1 {
        let middle = mutations.upserts.len() / 2;
        let mut left = mutations;
        let right = left.upserts.split_off(middle);
        write_large_pack_batch(writer, left)?;
        return write_large_pack_batch(
            writer,
            MutationBatch {
                upserts: right,
                tombstones: Vec::new(),
            },
        );
    }
    if mutations.upserts.len() == 1 {
        let mut remaining = mutations;
        let tombstones = std::mem::take(&mut remaining.tombstones);
        writer.write_single_oversized_batch(&remaining)?;
        return write_large_pack_batch(
            writer,
            MutationBatch {
                upserts: Vec::new(),
                tombstones,
            },
        );
    }

    if mutations.tombstones.len() == 1 {
        return writer.write_single_oversized_batch(&mutations);
    }
    let middle = mutations.tombstones.len() / 2;
    let mut left = mutations;
    let right = left.tombstones.split_off(middle);
    write_large_pack_batch(writer, left)?;
    write_large_pack_batch(
        writer,
        MutationBatch {
            upserts: Vec::new(),
            tombstones: right,
        },
    )
}

pub(super) fn validate_head(key: &str, device_id: &str, head: &DeviceHead) -> Result<(), String> {
    cancellation::check()?;
    if head.device_id != device_id {
        return Err(format!("head payload device does not match key {key:?}"));
    }
    if head.snapshot.key
        != snapshot_object_key(&head.device_id, &head.epoch, &head.snapshot.sha256)?
    {
        return Err(format!(
            "head {key:?} contains a noncanonical snapshot reference"
        ));
    }
    match &head.last_segment_key {
        None => Ok(()),
        Some(segment_key) => {
            let parsed = parse_segment_key(segment_key)?;
            if parsed.device_id != head.device_id
                || parsed.epoch != head.epoch
                || parsed.last_sequence != head.published_sequence
            {
                return Err(format!(
                    "head {key:?} has an invalid last segment reference"
                ));
            }
            Ok(())
        }
    }
}

pub(super) struct PublishedHead {
    pub(super) etag: Option<String>,
    pub(super) stored_size_bytes: u64,
}

pub(super) fn publish_head(
    store: &impl ObjectStore,
    head: &DeviceHead,
    session_key: Option<&SessionKey>,
    result: &mut SyncEngineResult,
) -> Result<PublishedHead, String> {
    cancellation::check()?;
    let key = head_object_key(&head.device_id)?;
    let encoded = encode_device_head(head, session_key)?;
    let stored_size = encoded.stored_size_bytes();
    match store.put(&key, encoded.bytes, PutCondition::Unconditional)? {
        PutOutcome::Stored { etag } => {
            result.bytes_uploaded =
                checked_add(result.bytes_uploaded, stored_size, "uploaded byte count")?;
            Ok(PublishedHead {
                etag,
                stored_size_bytes: stored_size,
            })
        }
        PutOutcome::PreconditionFailed => {
            Err("unconditional device-head write failed its precondition".to_string())
        }
    }
}

pub(super) fn put_immutable(
    store: &impl ObjectStore,
    key: &str,
    encoded: &EncodedObject,
    result: &mut SyncEngineResult,
) -> Result<(), String> {
    cancellation::check()?;
    match store.put(key, encoded.bytes.clone(), PutCondition::IfAbsent)? {
        PutOutcome::Stored { .. } => {
            result.bytes_uploaded = checked_add(
                result.bytes_uploaded,
                encoded.stored_size_bytes(),
                "uploaded byte count",
            )?;
            Ok(())
        }
        PutOutcome::PreconditionFailed => {
            let metadata = store
                .head(key)?
                .ok_or_else(|| format!("immutable object {key:?} disappeared after a retry"))?;
            if metadata
                .size_bytes
                .is_some_and(|size| size != encoded.stored_size_bytes())
            {
                return Err(format!("immutable object {key:?} has an unexpected size"));
            }
            Ok(())
        }
    }
}

pub(super) fn get_verified_object(
    store: &impl ObjectStore,
    key: &str,
    expected_sha256: &str,
    expected_size: Option<u64>,
    result: &mut SyncEngineResult,
) -> Result<Vec<u8>, String> {
    cancellation::check()?;
    let downloaded = store
        .get(key)?
        .ok_or_else(|| format!("remote object {key:?} does not exist"))?;
    let actual_size = downloaded.bytes.len() as u64;
    if expected_size.is_some_and(|size| size != actual_size) {
        return Err(format!(
            "remote object {key:?} size does not match its reference"
        ));
    }
    let actual_sha256 = hex::encode(Sha256::digest(&downloaded.bytes));
    if actual_sha256 != expected_sha256 {
        return Err(format!(
            "remote object {key:?} digest does not match its reference"
        ));
    }
    result.bytes_downloaded = checked_add(
        result.bytes_downloaded,
        actual_size,
        "downloaded byte count",
    )?;
    Ok(downloaded.bytes)
}

pub(super) fn get_verified_object_to_file(
    store: &impl ObjectStore,
    key: &str,
    expected_sha256: &str,
    expected_size: Option<u64>,
    directory: &std::path::Path,
    result: &mut SyncEngineResult,
) -> Result<TemporarySyncFile, String> {
    cancellation::check()?;
    let temporary = TemporarySyncFile::new(directory, "download")?;
    fs::remove_file(&temporary.path)
        .map_err(|error| format!("failed to prepare sync download file: {error}"))?;
    let max_bytes = expected_size.unwrap_or(MAX_LARGE_PACK_STORED_BYTES);
    if max_bytes > MAX_LARGE_PACK_STORED_BYTES {
        return Err(format!(
            "remote object {key:?} exceeds the sync pack size limit"
        ));
    }
    let downloaded = store
        .get_to_file(key, &temporary.path, max_bytes)?
        .ok_or_else(|| format!("remote object {key:?} does not exist"))?;
    if expected_size.is_some_and(|size| size != downloaded.size_bytes) {
        return Err(format!(
            "remote object {key:?} size does not match its reference"
        ));
    }
    if downloaded.sha256 != expected_sha256 {
        return Err(format!(
            "remote object {key:?} digest does not match its reference"
        ));
    }
    result.bytes_downloaded = checked_add(
        result.bytes_downloaded,
        downloaded.size_bytes,
        "downloaded byte count",
    )?;
    Ok(temporary)
}

pub(super) fn put_immutable_file(
    store: &impl ObjectStore,
    key: &str,
    encoded: &EncodedFile,
    result: &mut SyncEngineResult,
) -> Result<(), String> {
    cancellation::check()?;
    match store.put_file(
        key,
        encoded.path(),
        &encoded.sha256,
        encoded.stored_size_bytes,
        PutCondition::IfAbsent,
    )? {
        PutOutcome::Stored { .. } => {
            result.bytes_uploaded = checked_add(
                result.bytes_uploaded,
                encoded.stored_size_bytes,
                "uploaded byte count",
            )?;
            Ok(())
        }
        PutOutcome::PreconditionFailed => {
            let metadata = store
                .head(key)?
                .ok_or_else(|| format!("immutable object {key:?} disappeared after a retry"))?;
            if metadata
                .size_bytes
                .is_some_and(|size| size != encoded.stored_size_bytes)
            {
                return Err(format!("immutable object {key:?} has an unexpected size"));
            }
            Ok(())
        }
    }
}
