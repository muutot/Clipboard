//! Chunked sync v1 snapshot and checkpoint pack reader/writer.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LargePackKind {
    Snapshot,
    Checkpoint,
}

impl LargePackKind {
    fn object_kind(self) -> ObjectKind {
        match self {
            Self::Snapshot => ObjectKind::Snapshot,
            Self::Checkpoint => ObjectKind::Checkpoint,
        }
    }
}

fn temporary_pack_path(directory: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(directory)
        .map_err(|error| format!("failed to create sync pack directory: {error}"))?;
    for _ in 0..16 {
        let path = directory.join(format!(
            ".sync-pack-{}-{:016x}.tmp",
            std::process::id(),
            rand::random::<u64>()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => return Ok(path),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("failed to create sync pack file: {error}")),
        }
    }
    Err("failed to allocate a unique sync pack file".to_string())
}

fn encode_bincode<T: Encode>(value: &T, label: &str) -> Result<Vec<u8>, String> {
    bincode::encode_to_vec(
        value,
        bincode::config::standard().with_limit::<BINCODE_LIMIT_BYTES>(),
    )
    .map_err(|error| format!("failed to encode sync v1 {label}: {error}"))
}

fn decode_bincode<T: Decode<()>>(bytes: &[u8], label: &str) -> Result<T, String> {
    let (value, consumed): (T, usize) = bincode::decode_from_slice(
        bytes,
        bincode::config::standard().with_limit::<BINCODE_LIMIT_BYTES>(),
    )
    .map_err(|error| format!("failed to decode sync v1 {label}: {error}"))?;
    if consumed != bytes.len() {
        return Err(format!("sync v1 {label} has trailing decoded bytes"));
    }
    Ok(value)
}

pub(super) fn encode_pack_header<T: Encode>(value: &T) -> Result<Vec<u8>, String> {
    bincode::encode_to_vec(
        value,
        bincode::config::standard()
            .with_fixed_int_encoding()
            .with_limit::<BINCODE_LIMIT_BYTES>(),
    )
    .map_err(|error| format!("failed to encode sync v1 pack header: {error}"))
}

fn decode_pack_header<T: Decode<()>>(bytes: &[u8]) -> Result<T, String> {
    let (value, consumed): (T, usize) = bincode::decode_from_slice(
        bytes,
        bincode::config::standard()
            .with_fixed_int_encoding()
            .with_limit::<BINCODE_LIMIT_BYTES>(),
    )
    .map_err(|error| format!("failed to decode sync v1 pack header: {error}"))?;
    if consumed != bytes.len() {
        return Err("sync v1 pack header has trailing decoded bytes".to_string());
    }
    Ok(value)
}

fn protect_pack_header(
    kind: ObjectKind,
    header_bytes: &[u8],
    key: Option<&SessionKey>,
) -> Result<Vec<u8>, String> {
    let Some(key) = key else {
        return Ok(header_bytes.to_vec());
    };
    let mut nonce_hasher = Sha256::new();
    nonce_hasher.update(b"clipboard-sync-v1-pack-header-nonce\0");
    nonce_hasher.update([kind as u8]);
    nonce_hasher.update(header_bytes);
    let nonce_digest = nonce_hasher.finalize();
    let nonce = &nonce_digest[..NONCE_LEN];
    let aad = [b"clipboard-sync-v1-pack-header\0".as_slice(), &[kind as u8]].concat();
    let ciphertext = key
        .cipher()?
        .encrypt(
            &Nonce::try_from(nonce).expect("derived nonce length"),
            Payload {
                msg: header_bytes,
                aad: &aad,
            },
        )
        .map_err(|_| "failed to encrypt sync v1 pack header".to_string())?;
    let mut protected = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    protected.extend_from_slice(nonce);
    protected.extend_from_slice(&ciphertext);
    Ok(protected)
}

fn unprotect_pack_header(
    kind: ObjectKind,
    protected: &[u8],
    key: Option<&SessionKey>,
) -> Result<Vec<u8>, String> {
    let Some(key) = key else {
        return Ok(protected.to_vec());
    };
    if protected.len() < NONCE_LEN + AUTH_TAG_LEN {
        return Err("encrypted sync v1 pack header is truncated".to_string());
    }
    let nonce = &protected[..NONCE_LEN];
    let aad = [b"clipboard-sync-v1-pack-header\0".as_slice(), &[kind as u8]].concat();
    key.cipher()?
        .decrypt(
            &Nonce::try_from(nonce).expect("derived nonce length"),
            Payload {
                msg: &protected[NONCE_LEN..],
                aad: &aad,
            },
        )
        .map_err(|_| {
            "failed to decrypt sync v1 pack header: wrong password or corrupted data".to_string()
        })
}

fn encode_pack_chunk(
    kind: ObjectKind,
    chunk_index: u64,
    batch: &MutationBatch,
    key: Option<&SessionKey>,
    raw_limit: usize,
) -> Result<(Vec<u8>, u64), String> {
    if batch.is_empty() || batch.len() > PACK_CHUNK_MAX_ENTRIES {
        return Err("sync v1 pack chunk has an invalid record count".to_string());
    }
    let raw = encode_bincode(batch, "pack chunk")?;
    if raw.len() > raw_limit {
        return Err("sync v1 pack chunk exceeds the uncompressed size limit".to_string());
    }
    let compressed = zstd::stream::encode_all(Cursor::new(&raw), ZSTD_LEVEL)
        .map_err(|error| format!("failed to compress sync v1 pack chunk: {error}"))?;
    let stored_size = compressed
        .len()
        .checked_add(key.map(|_| NONCE_LEN + AUTH_TAG_LEN).unwrap_or(0))
        .ok_or_else(|| "sync v1 pack chunk stored size overflowed".to_string())?;
    // Compression is not a bound: zstd framing adds a frame header plus a block
    // header per block, so an incompressible chunk at the raw limit encodes
    // slightly *above* it. The reader rejects
    // `stored_size > raw_limit + NONCE_LEN + AUTH_TAG_LEN`, so enforcing the
    // same budget here is what stops us from publishing a chunk this protocol
    // version can never read back. The dangerous window is the last few hundred
    // bytes of the raw range, which is exactly where an incompressible
    // multi-record batch lands.
    let stored_limit = raw_limit
        .checked_add(NONCE_LEN + AUTH_TAG_LEN)
        .ok_or_else(|| "sync v1 pack chunk stored size overflowed".to_string())?;
    if stored_size > stored_limit {
        return Err(format!(
            "sync v1 pack chunk of {stored_size} stored bytes exceeds the {stored_limit}-byte \
             budget for a {raw_limit}-byte uncompressed chunk; the batch is incompressible and \
             must be split into smaller chunks"
        ));
    }
    let raw_size = u32::try_from(raw.len())
        .map_err(|_| "sync v1 pack chunk raw size overflowed".to_string())?;
    let stored_size = u32::try_from(stored_size)
        .map_err(|_| "sync v1 pack chunk stored size overflowed".to_string())?;
    let record_count = u32::try_from(batch.len())
        .map_err(|_| "sync v1 pack chunk record count overflowed".to_string())?;
    let mut chunk_header = [0u8; PACK_CHUNK_HEADER_LEN];
    chunk_header[0..8].copy_from_slice(&chunk_index.to_le_bytes());
    chunk_header[8..12].copy_from_slice(&record_count.to_le_bytes());
    chunk_header[12..16].copy_from_slice(&raw_size.to_le_bytes());
    chunk_header[16..20].copy_from_slice(&stored_size.to_le_bytes());

    let payload = if let Some(key) = key {
        let mut nonce_hasher = Sha256::new();
        nonce_hasher.update(b"clipboard-sync-v1-pack-chunk-nonce\0");
        nonce_hasher.update([kind as u8]);
        nonce_hasher.update(chunk_header);
        nonce_hasher.update(&compressed);
        let nonce_digest = nonce_hasher.finalize();
        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(&nonce_digest[..NONCE_LEN]);
        let mut aad = Vec::with_capacity(1 + chunk_header.len() + nonce.len());
        aad.push(kind as u8);
        aad.extend_from_slice(&chunk_header);
        aad.extend_from_slice(&nonce);
        let ciphertext = key
            .cipher()?
            .encrypt(
                &Nonce::from(nonce),
                Payload {
                    msg: &compressed,
                    aad: &aad,
                },
            )
            .map_err(|_| "failed to encrypt sync v1 pack chunk".to_string())?;
        let mut payload = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        payload.extend_from_slice(&nonce);
        payload.extend_from_slice(&ciphertext);
        payload
    } else {
        compressed
    };
    let mut chunk = Vec::with_capacity(PACK_CHUNK_HEADER_LEN + payload.len());
    chunk.extend_from_slice(&chunk_header);
    chunk.extend_from_slice(&payload);
    Ok((chunk, raw.len() as u64))
}

pub fn mutation_batch_encoded_size(batch: &MutationBatch) -> Result<usize, String> {
    let config = bincode::config::standard().with_limit::<BINCODE_LIMIT_BYTES>();
    let mut encoder =
        bincode::enc::EncoderImpl::new(bincode::enc::write::SizeWriter::default(), config);
    batch
        .encode(&mut encoder)
        .map_err(|error| format!("failed to size sync v1 mutation batch: {error}"))?;
    Ok(encoder.into_writer().bytes_written)
}

pub fn large_pack_chunk_limit_bytes() -> usize {
    PACK_CHUNK_MAX_UNCOMPRESSED_BYTES
}

/// Worst-case zstd framing overhead for one chunk at the 16 MiB packing
/// target: a frame header plus a 3-byte block header per 128 KiB block plus the
/// end-of-frame marker, rounded up generously. A batch is only routed into
/// `write_batch` when its uncompressed size fits under this budget, so an
/// incompressible batch near the limit is split by the caller instead of
/// compressing to a chunk the reader would reject. 1 KiB is ~0.006% of the
/// chunk budget.
pub const PACK_CHUNK_FRAMING_HEADROOM_BYTES: usize = 1024;

/// Largest uncompressed batch that is safe to hand to `write_batch` without
/// knowing how well it compresses. The encoder still enforces the exact budget
/// as a backstop; this only keeps the split decision on the cheap side of it.
pub fn large_pack_chunk_raw_budget_bytes() -> usize {
    PACK_CHUNK_MAX_UNCOMPRESSED_BYTES - PACK_CHUNK_FRAMING_HEADROOM_BYTES
}

pub struct LargePackWriter<'a> {
    path: PathBuf,
    file: Option<BufWriter<File>>,
    kind: ObjectKind,
    key: Option<&'a SessionKey>,
    chunk_index: u64,
    record_count: u64,
    uncompressed_size_bytes: u64,
    pack_header_plaintext_size: usize,
    pack_header_stored_size: usize,
}

impl<'a> LargePackWriter<'a> {
    pub fn new<T: Encode>(
        directory: &Path,
        kind: LargePackKind,
        pack_header: &T,
        key: Option<&'a SessionKey>,
    ) -> Result<Self, String> {
        let path = temporary_pack_path(directory)?;
        let mut file = BufWriter::new(
            OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&path)
                .map_err(|error| format!("failed to open sync pack file: {error}"))?,
        );
        let header_bytes = encode_pack_header(pack_header)?;
        let protected_header = protect_pack_header(kind.object_kind(), &header_bytes, key)?;
        let header_size = u32::try_from(protected_header.len())
            .map_err(|_| "sync v1 pack header is too large".to_string())?;
        let flags = PACK_FLAG_CHUNKED | if key.is_some() { FLAG_ENCRYPTED } else { 0 };
        file.write_all(&header(kind.object_kind(), flags, 0))
            .and_then(|_| file.write_all(&header_size.to_le_bytes()))
            .and_then(|_| file.write_all(&protected_header))
            .map_err(|error| format!("failed to write sync pack header: {error}"))?;
        Ok(Self {
            path,
            file: Some(file),
            kind: kind.object_kind(),
            key,
            chunk_index: 0,
            record_count: 0,
            uncompressed_size_bytes: header_bytes.len() as u64,
            pack_header_plaintext_size: header_bytes.len(),
            pack_header_stored_size: protected_header.len(),
        })
    }

    pub fn rewrite_header<T: Encode>(&mut self, pack_header: &T) -> Result<(), String> {
        let header_bytes = encode_pack_header(pack_header)?;
        let protected_header = protect_pack_header(self.kind, &header_bytes, self.key)?;
        if header_bytes.len() != self.pack_header_plaintext_size
            || protected_header.len() != self.pack_header_stored_size
        {
            return Err("sync v1 pack header size changed during export".to_string());
        }
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| "sync pack writer is already finished".to_string())?;
        file.flush()
            .and_then(|_| file.seek(SeekFrom::Start((HEADER_LEN + 4) as u64)))
            .and_then(|_| file.write_all(&protected_header))
            .and_then(|_| file.seek(SeekFrom::End(0)).map(|_| ()))
            .map_err(|error| format!("failed to rewrite sync pack header: {error}"))
    }

    pub fn write_batch(&mut self, batch: &MutationBatch) -> Result<(), String> {
        self.write_encoded_chunk(batch, PACK_CHUNK_MAX_UNCOMPRESSED_BYTES)
    }

    /// Writes a batch holding exactly one record larger than the normal chunk
    /// limit. A lone record cannot be split, so it is emitted as one chunk up
    /// to the global uncompressed limit instead of failing the whole pack.
    pub fn write_single_oversized_batch(&mut self, batch: &MutationBatch) -> Result<(), String> {
        if batch.len() != 1 {
            return Err("oversized sync pack chunk must hold exactly one record".to_string());
        }
        self.write_encoded_chunk(batch, PACK_CHUNK_SINGLE_RECORD_MAX_UNCOMPRESSED_BYTES)
    }

    fn write_encoded_chunk(
        &mut self,
        batch: &MutationBatch,
        raw_limit: usize,
    ) -> Result<(), String> {
        if batch.is_empty() {
            return Ok(());
        }
        let (chunk, raw_size) =
            encode_pack_chunk(self.kind, self.chunk_index, batch, self.key, raw_limit)?;
        self.file
            .as_mut()
            .ok_or_else(|| "sync pack writer is already finished".to_string())?
            .write_all(&chunk)
            .map_err(|error| format!("failed to write sync pack chunk: {error}"))?;
        self.chunk_index = self
            .chunk_index
            .checked_add(1)
            .ok_or_else(|| "sync v1 pack chunk count overflowed".to_string())?;
        self.record_count = self
            .record_count
            .checked_add(batch.len() as u64)
            .ok_or_else(|| "sync v1 pack record count overflowed".to_string())?;
        self.uncompressed_size_bytes = self
            .uncompressed_size_bytes
            .checked_add(raw_size)
            .ok_or_else(|| "sync v1 pack uncompressed size overflowed".to_string())?;
        if self.uncompressed_size_bytes > MAX_UNCOMPRESSED_BYTES {
            return Err("sync v1 pack exceeds the uncompressed size limit".to_string());
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<EncodedFile, String> {
        let mut file = self
            .file
            .take()
            .ok_or_else(|| "sync pack writer is already finished".to_string())?;
        file.flush()
            .map_err(|error| format!("failed to flush sync pack file: {error}"))?;
        let mut file = file
            .into_inner()
            .map_err(|error| format!("failed to finish sync pack file: {}", error.error()))?;
        file.seek(SeekFrom::Start(12))
            .and_then(|_| file.write_all(&self.uncompressed_size_bytes.to_le_bytes()))
            .and_then(|_| file.flush())
            .map_err(|error| format!("failed to finalize sync pack header: {error}"))?;
        drop(file);
        let metadata = fs::metadata(&self.path)
            .map_err(|error| format!("failed to inspect sync pack file: {error}"))?;
        if metadata.len() > MAX_STORED_BYTES as u64 {
            return Err("sync v1 pack exceeds the stored size limit".to_string());
        }
        let sha256 = hash_file(&self.path)?;
        let path = std::mem::take(&mut self.path);
        Ok(EncodedFile {
            path,
            sha256,
            stored_size_bytes: metadata.len(),
            uncompressed_size_bytes: self.uncompressed_size_bytes,
            record_count: self.record_count,
        })
    }
}

impl Drop for LargePackWriter<'_> {
    fn drop(&mut self) {
        let _ = self.file.take();
        if !self.path.as_os_str().is_empty() {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn hash_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path)
        .map_err(|error| format!("failed to open sync pack for hashing: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 256 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("failed to hash sync pack: {error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[derive(Debug)]
pub struct LargePackReader<'a, T> {
    reader: BufReader<File>,
    kind: ObjectKind,
    key: Option<&'a SessionKey>,
    pub header: T,
    expected_uncompressed_size_bytes: u64,
    decoded_uncompressed_size_bytes: u64,
    decoded_record_count: u64,
    next_chunk_index: u64,
    finished: bool,
    /// Total on-disk size of the pack, captured once in `open`.
    total_stored_bytes: u64,
    /// Bytes of the pack already read, so `next` can require a declared chunk
    /// size to fit in what actually remains instead of trusting the header.
    consumed_bytes: u64,
}

impl<'a, T: Decode<()>> LargePackReader<'a, T> {
    pub fn open(
        path: &Path,
        expected_kind: LargePackKind,
        key: Option<&'a SessionKey>,
    ) -> Result<Self, String> {
        let metadata = fs::metadata(path)
            .map_err(|error| format!("failed to inspect downloaded sync pack: {error}"))?;
        if metadata.len() < (HEADER_LEN + 4) as u64 || metadata.len() > MAX_STORED_BYTES as u64 {
            return Err("sync v1 pack has an invalid stored size".to_string());
        }
        let total_stored_bytes = metadata.len();
        let mut reader = BufReader::new(
            File::open(path).map_err(|error| format!("failed to open sync pack: {error}"))?,
        );
        let mut envelope_header = [0u8; HEADER_LEN];
        reader
            .read_exact(&mut envelope_header)
            .map_err(|error| format!("failed to read sync pack header: {error}"))?;
        if &envelope_header[..MAGIC.len()] != MAGIC {
            return Err("sync v1 object magic does not match".to_string());
        }
        let version = u16::from_le_bytes([envelope_header[8], envelope_header[9]]);
        if version != FORMAT_VERSION {
            return Err(format!("unsupported sync v1 format version {version}"));
        }
        let kind = ObjectKind::try_from(envelope_header[10])?;
        if kind != expected_kind.object_kind() {
            return Err(format!(
                "sync v1 object kind mismatch: expected {}, got {}",
                expected_kind.object_kind() as u8,
                kind as u8
            ));
        }
        let flags = envelope_header[11];
        if flags & !PACK_KNOWN_FLAGS != 0 || flags & PACK_FLAG_CHUNKED == 0 {
            return Err("sync v1 pack contains invalid flags".to_string());
        }
        let encrypted = flags & FLAG_ENCRYPTED != 0;
        match (encrypted, key) {
            (true, None) => {
                return Err("sync v1 object requires an encryption password".to_string())
            }
            (false, Some(_)) => {
                return Err(
                    "sync v1 object is unencrypted but this remote scope requires encryption"
                        .to_string(),
                )
            }
            _ => {}
        }
        let expected_uncompressed_size_bytes = u64::from_le_bytes(
            envelope_header[12..20]
                .try_into()
                .map_err(|_| "sync v1 pack header is truncated".to_string())?,
        );
        if expected_uncompressed_size_bytes > MAX_UNCOMPRESSED_BYTES {
            return Err("sync v1 pack exceeds the uncompressed size limit".to_string());
        }
        let mut header_size = [0u8; 4];
        reader
            .read_exact(&mut header_size)
            .map_err(|error| format!("failed to read sync pack metadata size: {error}"))?;
        let header_size = u32::from_le_bytes(header_size) as usize;
        if header_size == 0 || header_size > PACK_CHUNK_MAX_UNCOMPRESSED_BYTES {
            return Err("sync v1 pack metadata size is invalid".to_string());
        }
        let mut protected_header = vec![0u8; header_size];
        reader
            .read_exact(&mut protected_header)
            .map_err(|error| format!("failed to read sync pack metadata: {error}"))?;
        let header_bytes = unprotect_pack_header(kind, &protected_header, key)?;
        let header = decode_pack_header(&header_bytes)?;
        Ok(Self {
            reader,
            kind,
            key,
            header,
            expected_uncompressed_size_bytes,
            decoded_uncompressed_size_bytes: header_bytes.len() as u64,
            decoded_record_count: 0,
            next_chunk_index: 0,
            finished: false,
            total_stored_bytes,
            // The envelope header, the metadata length prefix, and the protected
            // metadata itself are already off the wire.
            consumed_bytes: (HEADER_LEN + 4 + header_size) as u64,
        })
    }

    pub fn record_count(&self) -> u64 {
        self.decoded_record_count
    }

    pub fn declared_uncompressed_size_bytes(&self) -> u64 {
        self.expected_uncompressed_size_bytes
    }

    pub fn is_complete(&self) -> bool {
        self.finished
            && self.decoded_uncompressed_size_bytes == self.expected_uncompressed_size_bytes
    }

    pub fn finish(mut self) -> Result<u64, String> {
        while self.next().transpose()?.is_some() {}
        if self.decoded_uncompressed_size_bytes != self.expected_uncompressed_size_bytes {
            return Err("sync v1 pack uncompressed size does not match its header".to_string());
        }
        Ok(self.decoded_record_count)
    }
}

impl<T> Iterator for LargePackReader<'_, T> {
    type Item = Result<MutationBatch, String>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        let mut chunk_header = [0u8; PACK_CHUNK_HEADER_LEN];
        match self.reader.read_exact(&mut chunk_header) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::UnexpectedEof => {
                let read = self
                    .reader
                    .fill_buf()
                    .map(|buffer| buffer.len())
                    .unwrap_or(0);
                if read == 0 {
                    self.finished = true;
                    return None;
                }
                self.finished = true;
                return Some(Err("sync v1 pack chunk header is truncated".to_string()));
            }
            Err(error) => {
                self.finished = true;
                return Some(Err(format!(
                    "failed to read sync pack chunk header: {error}"
                )));
            }
        }
        self.consumed_bytes += PACK_CHUNK_HEADER_LEN as u64;
        let result = (|| {
            let chunk_index = u64::from_le_bytes(chunk_header[0..8].try_into().unwrap());
            if chunk_index != self.next_chunk_index || chunk_index >= PACK_MAX_CHUNKS {
                return Err("sync v1 pack chunk index is invalid".to_string());
            }
            let record_count = u32::from_le_bytes(chunk_header[8..12].try_into().unwrap()) as usize;
            let raw_size = u32::from_le_bytes(chunk_header[12..16].try_into().unwrap()) as usize;
            let stored_size = u32::from_le_bytes(chunk_header[16..20].try_into().unwrap()) as usize;
            // A single record may legitimately exceed the 16 MiB chunk target
            // (it cannot be split); accept it up to the global uncompressed
            // limit. Multi-record chunks keep the tighter packing bound.
            let raw_limit = if record_count == 1 {
                PACK_CHUNK_SINGLE_RECORD_MAX_UNCOMPRESSED_BYTES
            } else {
                PACK_CHUNK_MAX_UNCOMPRESSED_BYTES
            };
            if record_count == 0
                || record_count > PACK_CHUNK_MAX_ENTRIES
                || raw_size == 0
                || raw_size > raw_limit
                || stored_size == 0
                || stored_size > raw_limit + NONCE_LEN + AUTH_TAG_LEN
            {
                return Err("sync v1 pack chunk declares invalid sizes".to_string());
            }
            // Require the declared payload to fit in the bytes that actually
            // remain on disk, before anything is allocated. The size checks
            // above only compare the declared sizes against the protocol limit:
            // a ~100-byte unencrypted pack can declare a 1 GiB single-record
            // chunk, pass every one of them, and then make the reader
            // zero-fill a gigabyte before `read_exact` reports the truncation.
            let remaining = self.total_stored_bytes.saturating_sub(self.consumed_bytes);
            if stored_size as u64 > remaining {
                return Err(format!(
                    "sync v1 pack chunk declares {stored_size} bytes but only {remaining} remain"
                ));
            }
            self.consumed_bytes += stored_size as u64;
            let mut stored = vec![0u8; stored_size];
            self.reader
                .read_exact(&mut stored)
                .map_err(|_| "sync v1 pack chunk payload is truncated".to_string())?;
            let compressed = if let Some(key) = self.key {
                if stored.len() < NONCE_LEN + AUTH_TAG_LEN {
                    return Err("encrypted sync v1 pack chunk is truncated".to_string());
                }
                let nonce = &stored[..NONCE_LEN];
                let mut aad = Vec::with_capacity(1 + chunk_header.len() + nonce.len());
                aad.push(self.kind as u8);
                aad.extend_from_slice(&chunk_header);
                aad.extend_from_slice(nonce);
                key.cipher()?
                    .decrypt(
                        &Nonce::try_from(nonce).expect("derived nonce length"),
                        Payload {
                            msg: &stored[NONCE_LEN..],
                            aad: &aad,
                        },
                    )
                    .map_err(|_| {
                        "failed to decrypt sync v1 pack chunk: wrong password or corrupted data"
                            .to_string()
                    })?
            } else {
                stored
            };
            let decoder = zstd::stream::read::Decoder::new(Cursor::new(compressed))
                .map_err(|error| format!("failed to open sync v1 pack zstd payload: {error}"))?;
            // Reserve a packed chunk outright; a lone oversized record grows into
            // the rest. Reserving the declared `raw_size` up front would let a
            // crafted header demand a gigabyte of address space before a single
            // byte has been verified.
            let mut raw = Vec::with_capacity(raw_size.min(PACK_CHUNK_MAX_UNCOMPRESSED_BYTES));
            decoder
                .take(raw_size as u64 + 1)
                .read_to_end(&mut raw)
                .map_err(|error| format!("failed to decompress sync v1 pack chunk: {error}"))?;
            if raw.len() != raw_size {
                return Err("sync v1 pack chunk size does not match its header".to_string());
            }
            let batch: MutationBatch = decode_bincode(&raw, "pack chunk")?;
            if batch.len() != record_count || batch.is_empty() {
                return Err("sync v1 pack chunk record count does not match".to_string());
            }
            self.next_chunk_index += 1;
            self.decoded_record_count = self
                .decoded_record_count
                .checked_add(record_count as u64)
                .ok_or_else(|| "sync v1 pack record count overflowed".to_string())?;
            self.decoded_uncompressed_size_bytes = self
                .decoded_uncompressed_size_bytes
                .checked_add(raw_size as u64)
                .ok_or_else(|| "sync v1 pack uncompressed size overflowed".to_string())?;
            if self.decoded_uncompressed_size_bytes > self.expected_uncompressed_size_bytes {
                return Err("sync v1 pack exceeds its declared uncompressed size".to_string());
            }
            Ok(batch)
        })();
        if result.is_err() {
            self.finished = true;
        }
        Some(result)
    }
}

pub type SnapshotPackReader<'a> = LargePackReader<'a, SnapshotPackHeader>;
pub type CheckpointPackReader<'a> = LargePackReader<'a, CheckpointPackHeader>;

pub fn encode_snapshot_pack(
    directory: &Path,
    header: &SnapshotPackHeader,
    batches: impl IntoIterator<Item = MutationBatch>,
    key: Option<&SessionKey>,
) -> Result<EncodedFile, String> {
    let mut writer = LargePackWriter::new(directory, LargePackKind::Snapshot, header, key)?;
    for batch in batches {
        writer.write_batch(&batch)?;
    }
    writer.finish()
}

pub fn encode_checkpoint_pack(
    directory: &Path,
    header: &CheckpointPackHeader,
    batches: impl IntoIterator<Item = MutationBatch>,
    key: Option<&SessionKey>,
) -> Result<EncodedFile, String> {
    let mut writer = LargePackWriter::new(directory, LargePackKind::Checkpoint, header, key)?;
    for batch in batches {
        writer.write_batch(&batch)?;
    }
    writer.finish()
}

pub fn open_snapshot_pack<'a>(
    path: &Path,
    key: Option<&'a SessionKey>,
) -> Result<SnapshotPackReader<'a>, String> {
    LargePackReader::open(path, LargePackKind::Snapshot, key)
}

pub fn open_checkpoint_pack<'a>(
    path: &Path,
    key: Option<&'a SessionKey>,
) -> Result<CheckpointPackReader<'a>, String> {
    LargePackReader::open(path, LargePackKind::Checkpoint, key)
}
