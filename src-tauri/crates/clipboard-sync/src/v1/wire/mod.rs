//! Frozen sync protocol v1 codecs: DTOs, envelope crypto, and large packs.
//!
//! The wire format is split by concern. `types.rs` holds the protocol-owned
//! DTOs with their frozen field order, `crypto.rs` owns the per-object envelope
//! (bincode + zstd + AES-256-GCM) and the resource-chunk helpers, `packs.rs`
//! owns the chunked snapshot/checkpoint pack reader and writer, and this module
//! keeps the shared imports and format constants so every submodule can reach
//! them through `use super::*`.

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
use zeroize::Zeroize;

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
/// A lone record cannot be split across chunks, so a chunk holding exactly one
/// record may exceed the 16 MiB packing target up to the global uncompressed
/// limit. Without this an individual item larger than `PACK_CHUNK_MAX_*` could
/// never be published in a snapshot or checkpoint and wedged sync forever.
const PACK_CHUNK_SINGLE_RECORD_MAX_UNCOMPRESSED_BYTES: usize = MAX_UNCOMPRESSED_BYTES as usize;

pub(crate) const RESOURCE_HEADER_LEN: usize = HEADER_LEN;
pub(crate) const RESOURCE_AUTH_TAG_LEN: usize = AUTH_TAG_LEN;

mod crypto;
mod packs;
mod types;

#[cfg(test)]
mod tests;

use crypto::*;
#[cfg(test)]
use packs::*;

pub use crypto::{
    decode_checkpoint_head, decode_device_head, decode_segment, encode_checkpoint_head,
    encode_device_head, encode_segment, envelope_is_encrypted, EncodedObject, SessionKey,
};
pub(crate) use crypto::{resource_header, validate_resource_header};
pub use packs::{
    encode_checkpoint_pack, encode_snapshot_pack, large_pack_chunk_limit_bytes,
    large_pack_chunk_raw_budget_bytes, mutation_batch_encoded_size, open_checkpoint_pack,
    open_snapshot_pack, CheckpointPackReader, LargePackKind, LargePackWriter, SnapshotPackReader,
    PACK_CHUNK_FRAMING_HEADROOM_BYTES,
};
pub use types::{
    CheckpointHead, CheckpointPackHeader, DeviceCursor, DeviceHead, EncodedFile, MutationBatch,
    ObjectRef, RecordVersion, ReplicatedItem, Segment, SnapshotPackHeader, SyncItem, SyncItemKind,
    Tombstone,
};
