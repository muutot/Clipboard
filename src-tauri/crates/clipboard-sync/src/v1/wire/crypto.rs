//! Per-object sync v1 envelope crypto and resource-chunk helpers.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum ObjectKind {
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
        // `zeroize` issues a volatile write so the optimizer cannot elide the
        // scrub of the derived AES key.
        self.key.zeroize();
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
        pbkdf2::pbkdf2::<Hmac<Sha256>>(password.as_bytes(), &salt, PBKDF2_ITERATIONS, &mut key)
            .expect("PBKDF2 output length matches the derived key size");
        Ok(Self { key })
    }

    pub(super) fn cipher(&self) -> Result<Aes256Gcm, String> {
        Ok(Aes256Gcm::new(&self.key.into()))
    }

    pub(crate) fn namespace_check(&self, scope: &str) -> String {
        let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(&self.key).expect("HMAC key size");
        mac.update(b"clipboard-sync-v1-namespace-check\0");
        mac.update(scope.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    pub(crate) fn verify_namespace_check(&self, scope: &str, expected: &str) -> Result<(), String> {
        let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(&self.key).expect("HMAC key size");
        mac.update(b"clipboard-sync-v1-namespace-check\0");
        mac.update(scope.as_bytes());
        let bytes = hex::decode(expected).map_err(|_| "invalid namespace key check")?;
        mac.verify_slice(&bytes).map_err(|_| {
            "sync namespace password differs; in-place password changes are unsupported".into()
        })
    }

    pub(crate) fn resource_digest(&self, plaintext_sha256: &[u8; 32]) -> String {
        let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(&self.key)
            .expect("HMAC-SHA256 accepts a 256-bit key");
        mac.update(b"clipboard-sync-v1-resource-digest\0");
        mac.update(plaintext_sha256);
        hex::encode(mac.finalize().into_bytes())
    }

    fn resource_nonce(&self, object_key: &str, chunk_index: u64) -> [u8; NONCE_LEN] {
        let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(&self.key)
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

    pub(crate) fn encrypt_resource_chunk(
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
                &Nonce::from(nonce),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| "failed to encrypt sync v1 resource chunk".to_string())
    }

    pub(crate) fn decrypt_resource_chunk(
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
                &Nonce::from(nonce),
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

pub(super) fn header(kind: ObjectKind, flags: u8, uncompressed_size: u64) -> [u8; HEADER_LEN] {
    let mut header = [0u8; HEADER_LEN];
    header[..MAGIC.len()].copy_from_slice(MAGIC);
    header[8..10].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    header[10] = kind as u8;
    header[11] = flags;
    header[12..20].copy_from_slice(&uncompressed_size.to_le_bytes());
    header
}

pub(crate) fn resource_header(plaintext_size: u64) -> Result<[u8; HEADER_LEN], String> {
    if plaintext_size > MAX_UNCOMPRESSED_BYTES {
        return Err("sync v1 resource exceeds the plaintext size limit".to_string());
    }
    Ok(header(ObjectKind::Resource, FLAG_ENCRYPTED, plaintext_size))
}

pub(crate) fn validate_resource_header(data: &[u8]) -> Result<u64, String> {
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

pub(super) fn encode_value<T: Encode>(
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
                &Nonce::from(nonce_bytes),
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

pub(super) fn decode_value<T: Decode<()>>(
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
                    &Nonce::try_from(&payload[..NONCE_LEN])
                        .expect("nonce slice length is checked above"),
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
