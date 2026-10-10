//! Single-resource transfer: fingerprint, upload, download and verification.

use super::*;

/// Fingerprints one managed resource with a bounded streaming read. The
/// canonical source must be a regular non-symlink file below `managed_root`.
pub fn fingerprint_resource(
    managed_root: &Path,
    source_path: &Path,
    category: ResourceCategory,
    max_bytes: u64,
    session_key: Option<&SessionKey>,
) -> Result<ResourceDescriptor, String> {
    let canonical_root = fs::canonicalize(managed_root)
        .map_err(|error| format!("failed to resolve managed resource root: {error}"))?;
    if !canonical_root.is_dir() {
        return Err("managed resource root is not a directory".to_string());
    }
    let link_metadata = fs::symlink_metadata(source_path)
        .map_err(|error| format!("failed to inspect managed resource: {error}"))?;
    if link_metadata.file_type().is_symlink() {
        return Err("managed resource cannot be a symlink".to_string());
    }
    let canonical_source = fs::canonicalize(source_path)
        .map_err(|error| format!("failed to resolve managed resource: {error}"))?;
    canonical_source
        .strip_prefix(&canonical_root)
        .map_err(|_| "managed resource is outside its configured root".to_string())?;

    let (plaintext_sha256, size_bytes) = hash_regular_file(&canonical_source, max_bytes)?;
    let extension = portable_extension(&canonical_source);
    let object_digest = resource_object_digest(&plaintext_sha256, session_key)?;
    let object_key = resource_object_key(category, &object_digest, &extension)?;
    Ok(ResourceDescriptor {
        category,
        source_path: canonical_source,
        object_key,
        plaintext_sha256,
        extension,
        size_bytes,
    })
}

/// Avoids uploading an already-present content-addressed object. The HEAD +
/// create-only PUT pair minimizes traffic while remaining safe under races.
pub fn ensure_resource_uploaded(
    store: &impl ObjectStore,
    resource: &ResourceDescriptor,
    session_key: Option<&SessionKey>,
) -> Result<ResourceUploadResult, String> {
    let stored_size_bytes = resource_stored_size(resource.size_bytes, session_key.is_some())?;
    if let Some(metadata) = store.head(&resource.object_key)? {
        validate_remote_size(&resource.object_key, stored_size_bytes, &metadata)?;
        return Ok(ResourceUploadResult {
            object_key: resource.object_key.clone(),
            size_bytes: stored_size_bytes,
            uploaded: false,
        });
    }

    let encrypted_upload = session_key
        .map(|key| encrypt_resource_to_temp(resource, key))
        .transpose()?;
    let (upload_path, upload_sha256, upload_size) = encrypted_upload.as_ref().map_or_else(
        || {
            (
                resource.source_path.as_path(),
                resource.plaintext_sha256.as_str(),
                resource.size_bytes,
            )
        },
        |upload| {
            (
                upload.path.as_path(),
                upload.sha256.as_str(),
                upload.size_bytes,
            )
        },
    );
    let uploaded = match store.put_file(
        &resource.object_key,
        upload_path,
        upload_sha256,
        upload_size,
        PutCondition::IfAbsent,
    )? {
        PutOutcome::Stored { .. } => true,
        PutOutcome::PreconditionFailed => {
            let metadata = store.head(&resource.object_key)?.ok_or_else(|| {
                format!(
                    "resource create-only write lost its race but {:?} is still absent",
                    resource.object_key
                )
            })?;
            validate_remote_size(&resource.object_key, stored_size_bytes, &metadata)?;
            false
        }
    };

    Ok(ResourceUploadResult {
        object_key: resource.object_key.clone(),
        size_bytes: stored_size_bytes,
        uploaded,
    })
}

/// Materializes a canonical resource object into a local content-addressed
/// cache. Downloads are written to a unique temporary file, hash-checked, and
/// renamed only after complete verification.
pub fn materialize_resource(
    store: &impl ObjectStore,
    object_key: &str,
    destination_root: &Path,
    max_bytes: u64,
    session_key: Option<&SessionKey>,
) -> Result<MaterializedResource, String> {
    let parsed = parse_resource_key(object_key)?;
    let category_root = prepare_destination_root(destination_root)?;
    let file_name = format!("sha256-{}.{}", parsed.sha256, parsed.extension);
    let final_path = category_root.join(file_name);

    if let Some(metadata) = symlink_metadata_if_exists(&final_path)? {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("cached resource path is not a regular file".to_string());
        }
        if metadata.len() <= max_bytes {
            let (sha256, size_bytes) = hash_regular_file(&final_path, max_bytes)?;
            if resource_object_digest(&sha256, session_key)? == parsed.sha256 {
                return Ok(MaterializedResource {
                    path: final_path,
                    size_bytes,
                    transferred_bytes: 0,
                    reused_local_file: true,
                });
            }
        }
    }

    let download_path = category_root.join(format!(
        ".download-{}-{:016x}.tmp",
        std::process::id(),
        rand::random::<u64>()
    ));
    let stored_limit = resource_stored_size(max_bytes, session_key.is_some())?;
    let download = match store.get_to_file(object_key, &download_path, stored_limit) {
        Ok(Some(download)) => download,
        Ok(None) => {
            let _ = fs::remove_file(&download_path);
            return Err(format!("remote resource {object_key:?} does not exist"));
        }
        Err(error) => {
            let _ = fs::remove_file(&download_path);
            return Err(error);
        }
    };
    let (verified_path, plaintext_size) = if let Some(key) = session_key {
        let plaintext_path = category_root.join(format!(
            ".plaintext-{}-{:016x}.tmp",
            std::process::id(),
            rand::random::<u64>()
        ));
        match decrypt_resource_to_file(&download_path, &plaintext_path, object_key, key, max_bytes)
        {
            Ok(size_bytes) => {
                let _ = fs::remove_file(&download_path);
                (plaintext_path, size_bytes)
            }
            Err(error) => {
                let _ = fs::remove_file(&download_path);
                let _ = fs::remove_file(&plaintext_path);
                return Err(error);
            }
        }
    } else {
        if download.sha256 != parsed.sha256 {
            let _ = fs::remove_file(&download_path);
            return Err(format!(
                "remote resource digest mismatch for {object_key:?}: expected {}, got {}",
                parsed.sha256, download.sha256
            ));
        }
        if download.size_bytes > max_bytes {
            let _ = fs::remove_file(&download_path);
            return Err(format!(
                "remote resource {object_key:?} exceeds the {max_bytes}-byte limit"
            ));
        }
        (download_path, download.size_bytes)
    };

    if let Err(error) = publish_verified_file(&verified_path, &final_path) {
        let _ = fs::remove_file(&verified_path);
        return Err(error);
    }

    Ok(MaterializedResource {
        path: final_path,
        size_bytes: plaintext_size,
        transferred_bytes: download.size_bytes,
        reused_local_file: false,
    })
}

/// Verifies an already materialized local file against a canonical resource
/// key without touching the object store. This lets records captured locally
/// (or materialized during an earlier request) reuse their existing path while
/// retaining the remote reference as the stable cache identity.
pub fn verify_local_resource(
    path: &Path,
    object_key: &str,
    max_bytes: u64,
    session_key: Option<&SessionKey>,
) -> Result<bool, String> {
    let parsed = parse_resource_key(object_key)?;
    let metadata = match symlink_metadata_if_exists(path)? {
        Some(metadata) => metadata,
        None => return Ok(false),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > max_bytes {
        return Ok(false);
    }
    let (sha256, _) = hash_regular_file(path, max_bytes)?;
    Ok(resource_object_digest(&sha256, session_key)? == parsed.sha256)
}

fn validate_remote_size(
    object_key: &str,
    expected_size: u64,
    metadata: &ObjectMetadata,
) -> Result<(), String> {
    if metadata
        .size_bytes
        .is_some_and(|actual_size| actual_size != expected_size)
    {
        return Err(format!(
            "remote resource size mismatch for {object_key:?}: expected {expected_size}, got {}",
            metadata.size_bytes.unwrap_or_default()
        ));
    }
    Ok(())
}

fn resource_object_digest(
    plaintext_sha256: &str,
    session_key: Option<&SessionKey>,
) -> Result<String, String> {
    let digest = hex::decode(plaintext_sha256)
        .map_err(|error| format!("resource SHA-256 is not hexadecimal: {error}"))?;
    let digest: [u8; 32] = digest
        .try_into()
        .map_err(|_| "resource SHA-256 must contain 32 bytes".to_string())?;
    Ok(session_key.map_or_else(
        || plaintext_sha256.to_string(),
        |key| key.resource_digest(&digest),
    ))
}

fn resource_stored_size(plaintext_size: u64, encrypted: bool) -> Result<u64, String> {
    if !encrypted {
        return Ok(plaintext_size);
    }
    let chunk_size = RESOURCE_CHUNK_BYTES as u64;
    let chunks = plaintext_size
        .checked_add(chunk_size.saturating_sub(1))
        .ok_or_else(|| "resource chunk count overflowed".to_string())?
        / chunk_size;
    (RESOURCE_HEADER_LEN as u64)
        .checked_add(plaintext_size)
        .and_then(|size| size.checked_add(chunks.saturating_mul(RESOURCE_AUTH_TAG_LEN as u64)))
        .ok_or_else(|| "encrypted resource size overflowed".to_string())
}

fn encrypt_resource_to_temp(
    resource: &ResourceDescriptor,
    session_key: &SessionKey,
) -> Result<EncryptedResourceTemp, String> {
    let temp_path = std::env::temp_dir().join(format!(
        ".sync-encrypted-{}-{:016x}.tmp",
        std::process::id(),
        rand::random::<u64>()
    ));
    let result = (|| {
        let mut source = File::open(&resource.source_path)
            .map_err(|error| format!("failed to open resource for encryption: {error}"))?;
        let metadata = source
            .metadata()
            .map_err(|error| format!("failed to inspect resource for encryption: {error}"))?;
        if !metadata.is_file() || metadata.len() != resource.size_bytes {
            return Err("resource changed after fingerprinting".to_string());
        }
        let mut destination = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .map_err(|error| format!("failed to create encrypted resource: {error}"))?;
        let header = resource_header(resource.size_bytes)?;
        destination
            .write_all(&header)
            .map_err(|error| format!("failed to write encrypted resource header: {error}"))?;
        let mut stored_hasher = Sha256::new();
        stored_hasher.update(header);
        let mut plaintext_hasher = Sha256::new();
        let mut plaintext_size = 0u64;
        let mut chunk_index = 0u64;
        let mut buffer = vec![0u8; RESOURCE_CHUNK_BYTES];
        loop {
            crate::cancellation::check()?;
            let read = source
                .read(&mut buffer)
                .map_err(|error| format!("failed to read resource for encryption: {error}"))?;
            if read == 0 {
                break;
            }
            plaintext_size = plaintext_size
                .checked_add(read as u64)
                .ok_or_else(|| "resource plaintext size overflowed".to_string())?;
            if plaintext_size > resource.size_bytes {
                return Err("resource grew while it was encrypted".to_string());
            }
            plaintext_hasher.update(&buffer[..read]);
            let encrypted = session_key.encrypt_resource_chunk(
                &header,
                &resource.object_key,
                chunk_index,
                &buffer[..read],
            )?;
            destination
                .write_all(&encrypted)
                .map_err(|error| format!("failed to write encrypted resource: {error}"))?;
            stored_hasher.update(&encrypted);
            chunk_index = chunk_index
                .checked_add(1)
                .ok_or_else(|| "resource chunk index overflowed".to_string())?;
        }
        destination
            .flush()
            .map_err(|error| format!("failed to flush encrypted resource: {error}"))?;
        if plaintext_size != resource.size_bytes
            || hex::encode(plaintext_hasher.finalize()) != resource.plaintext_sha256
        {
            return Err("resource changed while it was encrypted".to_string());
        }
        let size_bytes = resource_stored_size(resource.size_bytes, true)?;
        let actual_size = destination
            .metadata()
            .map_err(|error| format!("failed to inspect encrypted resource: {error}"))?
            .len();
        if actual_size != size_bytes {
            return Err("encrypted resource size does not match its format".to_string());
        }
        Ok(EncryptedResourceTemp {
            path: temp_path.clone(),
            sha256: hex::encode(stored_hasher.finalize()),
            size_bytes,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

fn decrypt_resource_to_file(
    encrypted_path: &Path,
    plaintext_path: &Path,
    object_key: &str,
    session_key: &SessionKey,
    max_bytes: u64,
) -> Result<u64, String> {
    let parsed = parse_resource_key(object_key)?;
    let mut source = File::open(encrypted_path)
        .map_err(|error| format!("failed to open encrypted resource: {error}"))?;
    let source_size = source
        .metadata()
        .map_err(|error| format!("failed to inspect encrypted resource: {error}"))?
        .len();
    let mut header = [0u8; RESOURCE_HEADER_LEN];
    source
        .read_exact(&mut header)
        .map_err(|error| format!("failed to read encrypted resource header: {error}"))?;
    let plaintext_size = validate_resource_header(&header)?;
    if plaintext_size > max_bytes {
        return Err(format!(
            "remote resource {object_key:?} exceeds the {max_bytes}-byte limit"
        ));
    }
    if source_size != resource_stored_size(plaintext_size, true)? {
        return Err("encrypted resource size does not match its header".to_string());
    }

    let mut destination = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(plaintext_path)
        .map_err(|error| format!("failed to create decrypted resource: {error}"))?;
    let mut plaintext_hasher = Sha256::new();
    let mut remaining = plaintext_size;
    let mut chunk_index = 0u64;
    while remaining > 0 {
        crate::cancellation::check()?;
        let chunk_plaintext_size = remaining.min(RESOURCE_CHUNK_BYTES as u64) as usize;
        let mut ciphertext = vec![0u8; chunk_plaintext_size + RESOURCE_AUTH_TAG_LEN];
        source
            .read_exact(&mut ciphertext)
            .map_err(|error| format!("failed to read encrypted resource chunk: {error}"))?;
        let plaintext =
            session_key.decrypt_resource_chunk(&header, object_key, chunk_index, &ciphertext)?;
        if plaintext.len() != chunk_plaintext_size {
            return Err("decrypted resource chunk has an invalid size".to_string());
        }
        destination
            .write_all(&plaintext)
            .map_err(|error| format!("failed to write decrypted resource: {error}"))?;
        plaintext_hasher.update(&plaintext);
        remaining -= chunk_plaintext_size as u64;
        chunk_index = chunk_index
            .checked_add(1)
            .ok_or_else(|| "resource chunk index overflowed".to_string())?;
    }
    destination
        .flush()
        .map_err(|error| format!("failed to flush decrypted resource: {error}"))?;
    let plaintext_sha256 = hex::encode(plaintext_hasher.finalize());
    if resource_object_digest(&plaintext_sha256, Some(session_key))? != parsed.sha256 {
        return Err("decrypted resource digest does not match its object key".to_string());
    }
    Ok(plaintext_size)
}

fn symlink_metadata_if_exists(path: &Path) -> Result<Option<fs::Metadata>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("failed to inspect cached resource: {error}")),
    }
}

fn publish_verified_file(temp_path: &Path, final_path: &Path) -> Result<(), String> {
    crate::cancellation::check()?;
    if let Some(metadata) = symlink_metadata_if_exists(final_path)? {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("cached resource path changed to a non-regular file".to_string());
        }
        fs::remove_file(final_path)
            .map_err(|error| format!("failed to replace corrupted cached resource: {error}"))?;
    }
    fs::rename(temp_path, final_path)
        .map_err(|error| format!("failed to publish cached resource: {error}"))
}

fn portable_extension(path: &Path) -> String {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|extension| {
            !extension.is_empty()
                && extension.len() <= 16
                && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
        .unwrap_or_else(|| "bin".to_string())
}

fn hash_regular_file(path: &Path, max_bytes: u64) -> Result<(String, u64), String> {
    let mut file = File::open(path).map_err(|error| format!("failed to open resource: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("failed to inspect resource: {error}"))?;
    if !metadata.is_file() {
        return Err("resource is not a regular file".to_string());
    }
    if metadata.len() > max_bytes {
        return Err(format!("resource exceeds the {max_bytes}-byte limit"));
    }

    let mut hasher = Sha256::new();
    let mut size_bytes = 0u64;
    let mut buffer = vec![0u8; HASH_BUFFER_BYTES];
    loop {
        crate::cancellation::check()?;
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("failed to hash resource: {error}"))?;
        if read == 0 {
            break;
        }
        size_bytes = size_bytes
            .checked_add(read as u64)
            .ok_or_else(|| "resource size overflowed".to_string())?;
        if size_bytes > max_bytes {
            return Err(format!("resource exceeds the {max_bytes}-byte limit"));
        }
        hasher.update(&buffer[..read]);
    }
    if size_bytes != metadata.len() {
        return Err(format!(
            "resource size changed while hashing: expected {}, got {size_bytes}",
            metadata.len()
        ));
    }
    Ok((hex::encode(hasher.finalize()), size_bytes))
}

fn prepare_destination_root(destination_root: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(destination_root)
        .map_err(|error| format!("failed to create resource cache root: {error}"))?;
    let canonical_root = fs::canonicalize(destination_root)
        .map_err(|error| format!("failed to resolve resource cache root: {error}"))?;
    if !canonical_root.is_dir() {
        return Err("resource cache root is not a directory".to_string());
    }
    let metadata = fs::symlink_metadata(&canonical_root)
        .map_err(|error| format!("failed to inspect resource cache root: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("resource cache root is not a regular directory".to_string());
    }
    Ok(canonical_root)
}
