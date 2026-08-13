use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Cursor, ErrorKind, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use bincode::{Decode, Encode};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAGIC: &[u8; 8] = b"CLPSYNC1";
const FORMAT_VERSION: u16 = 1;
const HEADER_LEN: usize = 20;
const FLAG_ENCRYPTED: u8 = 0x01;
const KNOWN_FLAGS: u8 = FLAG_ENCRYPTED;
const NONCE_LEN: usize = 12;
const AUTH_TAG_LEN: usize = 16;
const KEY_LEN: usize = 32;
const PBKDF2_ITERATIONS: u32 = 310_000;
const ZSTD_LEVEL: i32 = 3;
const MAX_UNCOMPRESSED_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_STORED_BYTES: usize = 1024 * 1024 * 1024;
const BINCODE_LIMIT_BYTES: usize = MAX_UNCOMPRESSED_BYTES as usize;
const PACK_FLAG_CHUNKED: u8 = 0x02;
const PACK_KNOWN_FLAGS: u8 = FLAG_ENCRYPTED | PACK_FLAG_CHUNKED;
const PACK_CHUNK_HEADER_LEN: usize = 24;
const PACK_CHUNK_MAX_UNCOMPRESSED_BYTES: usize = 16 * 1024 * 1024;
const PACK_CHUNK_MAX_ENTRIES: usize = 4096;
const PACK_MAX_CHUNKS: u64 = 1_000_000;

#[doc(hidden)]
pub const RESOURCE_HEADER_LEN: usize = HEADER_LEN;
#[doc(hidden)]
pub const RESOURCE_AUTH_TAG_LEN: usize = AUTH_TAG_LEN;

/// Protocol-owned clipboard category. Variant order is part of the v1 bincode contract.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Encode, Decode,
)]
#[serde(rename_all = "camelCase")]
pub enum SyncItemKind {
    Text,
    Link,
    Image,
    File,
}

/// Protocol-owned record DTO. Field order is frozen for the v1 bincode layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase")]
pub struct SyncItem {
    pub id: String,
    pub kind: SyncItemKind,
    pub title: String,
    pub text_content: Option<String>,
    #[serde(default)]
    pub html_content: Option<String>,
    #[serde(default)]
    pub rtf_content: Option<String>,
    pub resource_path: Option<String>,
    pub preview_path: Option<String>,
    pub content_hash: String,
    pub source_app: Option<String>,
    pub icon_path: Option<String>,
    pub size_bytes: u64,
    pub created_at_ms: i64,
    pub last_used_at_ms: Option<i64>,
    pub is_favorite: bool,
    pub metadata_json: Option<String>,
}

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

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct SnapshotPackHeader {
    pub device_id: String,
    pub epoch: String,
    pub through_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct CheckpointPackHeader {
    pub generation: u64,
    pub vector: Vec<DeviceCursor>,
}

#[derive(Debug)]
pub struct EncodedFile {
    path: PathBuf,
    pub sha256: String,
    pub stored_size_bytes: u64,
    pub uncompressed_size_bytes: u64,
    pub record_count: u64,
}

impl EncodedFile {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for EncodedFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum ObjectKind {
    DeviceHead = 1,
    Snapshot = 2,
    Segment = 3,
    Checkpoint = 4,
    CheckpointHead = 5,
    Resource = 6,
}

impl TryFrom<u8> for ObjectKind {
    type Error = String;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::DeviceHead),
            2 => Ok(Self::Snapshot),
            3 => Ok(Self::Segment),
            4 => Ok(Self::Checkpoint),
            5 => Ok(Self::CheckpointHead),
            6 => Ok(Self::Resource),
            _ => Err(format!("unknown sync v1 object kind {value}")),
        }
    }
}

pub struct SessionKey {
    key: [u8; KEY_LEN],
}

impl std::fmt::Debug for SessionKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SessionKey([redacted])")
    }
}

impl Drop for SessionKey {
    fn drop(&mut self) {
        self.key.fill(0);
    }
}

impl SessionKey {
    /// Derives one remote-scope-specific key for an entire sync run.
    pub fn derive(password: &str, remote_scope: &str) -> Result<Self, String> {
        if password.is_empty() {
            return Err("sync encryption password is empty".to_string());
        }
        if remote_scope.is_empty() {
            return Err("sync remote scope is empty".to_string());
        }
        let mut salt_input = b"clipboard-sync-v1\0".to_vec();
        salt_input.extend_from_slice(remote_scope.as_bytes());
        let salt = Sha256::digest(salt_input);
        let mut key = [0u8; KEY_LEN];
        pbkdf2::pbkdf2::<Hmac<Sha256>>(password.as_bytes(), &salt, PBKDF2_ITERATIONS, &mut key);
        Ok(Self { key })
    }

    fn cipher(&self) -> Result<Aes256Gcm, String> {
        Aes256Gcm::new_from_slice(&self.key)
            .map_err(|_| "failed to initialize sync encryption".to_string())
    }

    #[doc(hidden)]
    pub fn resource_digest(&self, plaintext_sha256: &[u8; 32]) -> String {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.key)
            .expect("HMAC-SHA256 accepts a 256-bit key");
        mac.update(b"clipboard-sync-v1-resource-digest\0");
        mac.update(plaintext_sha256);
        hex::encode(mac.finalize().into_bytes())
    }

    fn resource_nonce(&self, object_key: &str, chunk_index: u64) -> [u8; NONCE_LEN] {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.key)
            .expect("HMAC-SHA256 accepts a 256-bit key");
        mac.update(b"clipboard-sync-v1-resource-nonce\0");
        mac.update(object_key.as_bytes());
        mac.update(&chunk_index.to_le_bytes());
        let digest = mac.finalize().into_bytes();
        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(&digest[..NONCE_LEN]);
        nonce
    }

    fn resource_aad(header: &[u8], object_key: &str, chunk_index: u64) -> Vec<u8> {
        let mut aad = Vec::with_capacity(header.len() + object_key.len() + 9);
        aad.extend_from_slice(header);
        aad.push(0);
        aad.extend_from_slice(object_key.as_bytes());
        aad.extend_from_slice(&chunk_index.to_le_bytes());
        aad
    }

    #[doc(hidden)]
    pub fn encrypt_resource_chunk(
        &self,
        header: &[u8],
        object_key: &str,
        chunk_index: u64,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, String> {
        let nonce = self.resource_nonce(object_key, chunk_index);
        let aad = Self::resource_aad(header, object_key, chunk_index);
        self.cipher()?
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| "failed to encrypt sync v1 resource chunk".to_string())
    }

    #[doc(hidden)]
    pub fn decrypt_resource_chunk(
        &self,
        header: &[u8],
        object_key: &str,
        chunk_index: u64,
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, String> {
        let nonce = self.resource_nonce(object_key, chunk_index);
        let aad = Self::resource_aad(header, object_key, chunk_index);
        self.cipher()?
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| {
                "failed to decrypt sync v1 resource: wrong password or corrupted data".to_string()
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedObject {
    pub bytes: Vec<u8>,
    pub sha256: String,
    pub uncompressed_size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Encode, Decode)]
pub struct RecordVersion {
    pub modified_at_ms: i64,
    pub writer_device_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct ReplicatedItem {
    pub item: SyncItem,
    pub version: RecordVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Tombstone {
    pub item_id: String,
    pub kind: SyncItemKind,
    pub content_hash: String,
    pub deleted_at_ms: i64,
    pub version: RecordVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct MutationBatch {
    pub upserts: Vec<ReplicatedItem>,
    pub tombstones: Vec<Tombstone>,
}

impl MutationBatch {
    pub fn len(&self) -> usize {
        self.upserts.len() + self.tombstones.len()
    }

    pub fn is_empty(&self) -> bool {
        self.upserts.is_empty() && self.tombstones.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct ObjectRef {
    pub key: String,
    pub sha256: String,
    pub stored_size_bytes: u64,
    pub record_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct DeviceHead {
    pub device_id: String,
    pub epoch: String,
    pub snapshot: ObjectRef,
    pub published_sequence: u64,
    pub last_segment_key: Option<String>,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct DeviceCursor {
    pub device_id: String,
    pub epoch: String,
    pub sequence: u64,
    pub last_segment_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Segment {
    pub device_id: String,
    pub epoch: String,
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub mutations: MutationBatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct CheckpointHead {
    pub generation: u64,
    pub checkpoint: ObjectRef,
    pub vector: Vec<DeviceCursor>,
    pub previous_checkpoint: Option<ObjectRef>,
    pub updated_at_ms: i64,
}

fn header(kind: ObjectKind, flags: u8, uncompressed_size: u64) -> [u8; HEADER_LEN] {
    let mut header = [0u8; HEADER_LEN];
    header[..MAGIC.len()].copy_from_slice(MAGIC);
    header[8..10].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    header[10] = kind as u8;
    header[11] = flags;
    header[12..20].copy_from_slice(&uncompressed_size.to_le_bytes());
    header
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

fn encode_pack_header<T: Encode>(value: &T) -> Result<Vec<u8>, String> {
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
            Nonce::from_slice(nonce),
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
            Nonce::from_slice(nonce),
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
) -> Result<(Vec<u8>, u64), String> {
    if batch.is_empty() || batch.len() > PACK_CHUNK_MAX_ENTRIES {
        return Err("sync v1 pack chunk has an invalid record count".to_string());
    }
    let raw = encode_bincode(batch, "pack chunk")?;
    if raw.len() > PACK_CHUNK_MAX_UNCOMPRESSED_BYTES {
        return Err("sync v1 pack chunk exceeds the uncompressed size limit".to_string());
    }
    let compressed = zstd::stream::encode_all(Cursor::new(&raw), ZSTD_LEVEL)
        .map_err(|error| format!("failed to compress sync v1 pack chunk: {error}"))?;
    let stored_size = compressed
        .len()
        .checked_add(key.map(|_| NONCE_LEN + AUTH_TAG_LEN).unwrap_or(0))
        .ok_or_else(|| "sync v1 pack chunk stored size overflowed".to_string())?;
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
                Nonce::from_slice(&nonce),
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
        if batch.is_empty() {
            return Ok(());
        }
        let (chunk, raw_size) = encode_pack_chunk(self.kind, self.chunk_index, batch, self.key)?;
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
        let result = (|| {
            let chunk_index = u64::from_le_bytes(chunk_header[0..8].try_into().unwrap());
            if chunk_index != self.next_chunk_index || chunk_index >= PACK_MAX_CHUNKS {
                return Err("sync v1 pack chunk index is invalid".to_string());
            }
            let record_count = u32::from_le_bytes(chunk_header[8..12].try_into().unwrap()) as usize;
            let raw_size = u32::from_le_bytes(chunk_header[12..16].try_into().unwrap()) as usize;
            let stored_size = u32::from_le_bytes(chunk_header[16..20].try_into().unwrap()) as usize;
            if record_count == 0
                || record_count > PACK_CHUNK_MAX_ENTRIES
                || raw_size == 0
                || raw_size > PACK_CHUNK_MAX_UNCOMPRESSED_BYTES
                || stored_size == 0
                || stored_size > PACK_CHUNK_MAX_UNCOMPRESSED_BYTES + NONCE_LEN + AUTH_TAG_LEN
            {
                return Err("sync v1 pack chunk declares invalid sizes".to_string());
            }
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
                        Nonce::from_slice(nonce),
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
            let mut raw = Vec::with_capacity(raw_size);
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

#[doc(hidden)]
pub fn resource_header(plaintext_size: u64) -> Result<[u8; HEADER_LEN], String> {
    if plaintext_size > MAX_UNCOMPRESSED_BYTES {
        return Err("sync v1 resource exceeds the plaintext size limit".to_string());
    }
    Ok(header(ObjectKind::Resource, FLAG_ENCRYPTED, plaintext_size))
}

#[doc(hidden)]
pub fn validate_resource_header(data: &[u8]) -> Result<u64, String> {
    if data.len() != HEADER_LEN || &data[..MAGIC.len()] != MAGIC {
        return Err("sync v1 resource header is invalid".to_string());
    }
    let version = u16::from_le_bytes([data[8], data[9]]);
    if version != FORMAT_VERSION {
        return Err(format!("unsupported sync v1 format version {version}"));
    }
    if ObjectKind::try_from(data[10])? != ObjectKind::Resource {
        return Err("sync v1 resource object kind does not match".to_string());
    }
    if data[11] != FLAG_ENCRYPTED {
        return Err("sync v1 resource must use authenticated encryption".to_string());
    }
    let plaintext_size = u64::from_le_bytes(
        data[12..20]
            .try_into()
            .map_err(|_| "sync v1 resource header is truncated".to_string())?,
    );
    if plaintext_size > MAX_UNCOMPRESSED_BYTES {
        return Err("sync v1 resource exceeds the plaintext size limit".to_string());
    }
    Ok(plaintext_size)
}

#[doc(hidden)]
pub fn envelope_is_encrypted(data: &[u8]) -> Result<bool, String> {
    if data.len() < HEADER_LEN || data.len() > MAX_STORED_BYTES {
        return Err("sync v1 object has an invalid stored size".to_string());
    }
    if &data[..MAGIC.len()] != MAGIC {
        return Err("sync v1 object magic does not match".to_string());
    }
    let version = u16::from_le_bytes([data[8], data[9]]);
    if version != FORMAT_VERSION {
        return Err(format!("unsupported sync v1 format version {version}"));
    }
    ObjectKind::try_from(data[10])?;
    let flags = data[11];
    if flags & !KNOWN_FLAGS != 0 {
        return Err("sync v1 object contains unknown flags".to_string());
    }
    Ok(flags & FLAG_ENCRYPTED != 0)
}

fn encode_value<T: Encode>(
    kind: ObjectKind,
    value: &T,
    key: Option<&SessionKey>,
) -> Result<EncodedObject, String> {
    let raw = bincode::encode_to_vec(
        value,
        bincode::config::standard().with_limit::<BINCODE_LIMIT_BYTES>(),
    )
    .map_err(|error| format!("failed to encode sync v1 object: {error}"))?;
    let uncompressed_size_bytes = raw.len() as u64;
    if uncompressed_size_bytes > MAX_UNCOMPRESSED_BYTES {
        return Err("sync v1 object exceeds the uncompressed size limit".to_string());
    }
    let compressed = zstd::stream::encode_all(Cursor::new(raw), ZSTD_LEVEL)
        .map_err(|error| format!("failed to compress sync v1 object: {error}"))?;

    let flags = if key.is_some() { FLAG_ENCRYPTED } else { 0 };
    let header = header(kind, flags, uncompressed_size_bytes);
    let mut bytes = Vec::with_capacity(
        HEADER_LEN + compressed.len() + key.map(|_| NONCE_LEN + AUTH_TAG_LEN).unwrap_or(0),
    );
    bytes.extend_from_slice(&header);
    if let Some(key) = key {
        // Immutable v1 objects are content addressed. Deriving the nonce from
        // the authenticated header and compressed plaintext makes a retry
        // reproduce the exact same ciphertext/object key while still giving
        // distinct plaintexts distinct nonces with SHA-256 collision
        // resistance. Equality is already observable through object names.
        let mut nonce_hasher = Sha256::new();
        nonce_hasher.update(b"clipboard-sync-v1-nonce\0");
        nonce_hasher.update(header);
        nonce_hasher.update(&compressed);
        let nonce_digest = nonce_hasher.finalize();
        let mut nonce_bytes = [0u8; NONCE_LEN];
        nonce_bytes.copy_from_slice(&nonce_digest[..NONCE_LEN]);
        let ciphertext = key
            .cipher()?
            .encrypt(
                Nonce::from_slice(&nonce_bytes),
                Payload {
                    msg: &compressed,
                    aad: &header,
                },
            )
            .map_err(|_| "failed to encrypt sync v1 object".to_string())?;
        bytes.extend_from_slice(&nonce_bytes);
        bytes.extend_from_slice(&ciphertext);
    } else {
        bytes.extend_from_slice(&compressed);
    }
    if bytes.len() > MAX_STORED_BYTES {
        return Err("sync v1 object exceeds the stored size limit".to_string());
    }
    let sha256 = hex::encode(Sha256::digest(&bytes));
    Ok(EncodedObject {
        bytes,
        sha256,
        uncompressed_size_bytes,
    })
}

fn decode_value<T: Decode<()>>(
    expected_kind: ObjectKind,
    data: &[u8],
    key: Option<&SessionKey>,
) -> Result<T, String> {
    if data.len() < HEADER_LEN || data.len() > MAX_STORED_BYTES {
        return Err("sync v1 object has an invalid stored size".to_string());
    }
    if &data[..MAGIC.len()] != MAGIC {
        return Err("sync v1 object magic does not match".to_string());
    }
    let version = u16::from_le_bytes([data[8], data[9]]);
    if version != FORMAT_VERSION {
        return Err(format!("unsupported sync v1 format version {version}"));
    }
    let actual_kind = ObjectKind::try_from(data[10])?;
    if actual_kind != expected_kind {
        return Err(format!(
            "sync v1 object kind mismatch: expected {}, got {}",
            expected_kind as u8, actual_kind as u8
        ));
    }
    let flags = data[11];
    if flags & !KNOWN_FLAGS != 0 {
        return Err("sync v1 object contains unknown flags".to_string());
    }
    let uncompressed_size = u64::from_le_bytes(
        data[12..20]
            .try_into()
            .map_err(|_| "sync v1 object header is truncated".to_string())?,
    );
    if uncompressed_size > MAX_UNCOMPRESSED_BYTES {
        return Err("sync v1 object exceeds the uncompressed size limit".to_string());
    }
    let header = &data[..HEADER_LEN];
    let payload = &data[HEADER_LEN..];
    let encrypted = flags & FLAG_ENCRYPTED != 0;
    let compressed = match (encrypted, key) {
        (true, None) => {
            return Err("sync v1 object requires an encryption password".to_string());
        }
        (false, Some(_)) => {
            return Err(
                "sync v1 object is unencrypted but this remote scope requires encryption"
                    .to_string(),
            );
        }
        (true, Some(key)) => {
            if payload.len() < NONCE_LEN + AUTH_TAG_LEN {
                return Err("encrypted sync v1 object is truncated".to_string());
            }
            key.cipher()?
                .decrypt(
                    Nonce::from_slice(&payload[..NONCE_LEN]),
                    Payload {
                        msg: &payload[NONCE_LEN..],
                        aad: header,
                    },
                )
                .map_err(|_| {
                    "failed to decrypt sync v1 object: wrong password or corrupted data".to_string()
                })?
        }
        (false, None) => payload.to_vec(),
    };

    let decoder = zstd::stream::read::Decoder::new(Cursor::new(compressed))
        .map_err(|error| format!("failed to open sync v1 zstd payload: {error}"))?;
    let mut raw = Vec::with_capacity(uncompressed_size.min(16 * 1024 * 1024) as usize);
    decoder
        .take(uncompressed_size.saturating_add(1))
        .read_to_end(&mut raw)
        .map_err(|error| format!("failed to decompress sync v1 object: {error}"))?;
    if raw.len() as u64 != uncompressed_size {
        return Err("sync v1 uncompressed size does not match its header".to_string());
    }
    let (value, consumed): (T, usize) = bincode::decode_from_slice(
        &raw,
        bincode::config::standard().with_limit::<BINCODE_LIMIT_BYTES>(),
    )
    .map_err(|error| format!("failed to decode sync v1 object: {error}"))?;
    if consumed != raw.len() {
        return Err("sync v1 object has trailing decoded bytes".to_string());
    }
    Ok(value)
}

macro_rules! wire_functions {
    ($encode:ident, $decode:ident, $kind:ident, $type:ty) => {
        pub fn $encode(value: &$type, key: Option<&SessionKey>) -> Result<EncodedObject, String> {
            encode_value(ObjectKind::$kind, value, key)
        }

        pub fn $decode(data: &[u8], key: Option<&SessionKey>) -> Result<$type, String> {
            decode_value(ObjectKind::$kind, data, key)
        }
    };
}

wire_functions!(
    encode_device_head,
    decode_device_head,
    DeviceHead,
    DeviceHead
);
wire_functions!(encode_segment, decode_segment, Segment, Segment);
wire_functions!(
    encode_checkpoint_head,
    decode_checkpoint_head,
    CheckpointHead,
    CheckpointHead
);

#[cfg(test)]
mod tests {
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
}
