use std::io::{Cursor, Read};

use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use bincode::{Decode, Encode};
use hmac::Hmac;
use sha2::{Digest, Sha256};

use crate::domain::{ClipboardItem, ClipboardKind};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum ObjectKind {
    DeviceHead = 1,
    Snapshot = 2,
    Segment = 3,
    Checkpoint = 4,
    CheckpointHead = 5,
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
    pub item: ClipboardItem,
    pub version: RecordVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Tombstone {
    pub item_id: String,
    pub kind: ClipboardKind,
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
pub struct Snapshot {
    pub device_id: String,
    pub epoch: String,
    pub through_sequence: u64,
    pub mutations: MutationBatch,
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
pub struct Checkpoint {
    pub generation: u64,
    pub vector: Vec<DeviceCursor>,
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
        let nonce_bytes: [u8; NONCE_LEN] = rand::random();
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
    let compressed = if flags & FLAG_ENCRYPTED != 0 {
        if payload.len() < NONCE_LEN + AUTH_TAG_LEN {
            return Err("encrypted sync v1 object is truncated".to_string());
        }
        let key =
            key.ok_or_else(|| "sync v1 object requires an encryption password".to_string())?;
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
    } else {
        payload.to_vec()
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
wire_functions!(encode_snapshot, decode_snapshot, Snapshot, Snapshot);
wire_functions!(encode_segment, decode_segment, Segment, Segment);
wire_functions!(encode_checkpoint, decode_checkpoint, Checkpoint, Checkpoint);
wire_functions!(
    encode_checkpoint_head,
    decode_checkpoint_head,
    CheckpointHead,
    CheckpointHead
);

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_item() -> ClipboardItem {
        ClipboardItem {
            id: "text-device-1".to_string(),
            kind: ClipboardKind::Text,
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
    fn encrypted_segment_uses_one_derived_session_key() {
        let key = SessionKey::derive("correct horse battery staple", "remote-a").unwrap();
        let segment = sample_segment();
        let first = encode_segment(&segment, Some(&key)).unwrap();
        let second = encode_segment(&segment, Some(&key)).unwrap();
        assert_ne!(
            first.bytes, second.bytes,
            "every object needs a fresh nonce"
        );
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
        assert!(decode_snapshot(&encoded.bytes, None)
            .unwrap_err()
            .contains("kind mismatch"));

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
}
