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
    io::{BufReader, BufWriter, Cursor, ErrorKind, Read, Seek, SeekFrom, Write},
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
// Bincode reserves Vec/String capacity before the slice reader can reject a
// truncated length claim. Tie that reservation budget to verified raw bytes,
// allowing for Rust DTOs' larger in-memory footprint and varint accounting.
const BINCODE_DECODE_EXPANSION: usize = 32;
const BINCODE_MIN_DECODE_BUDGET: usize = 64 * 1024;
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
// Binary resources stream in fixed chunks instead of being decoded as one
// bincode/zstd value. Keep their cap aligned with the desktop's 2 GiB limit.
const MAX_RESOURCE_PLAINTEXT_BYTES: u64 = 2 * 1024 * 1024 * 1024;

mod crypto;
mod packs;
mod types;

fn decode_bounded_bincode<T: Decode<()>>(
    bytes: &[u8],
    fixed_int_encoding: bool,
) -> Result<(T, usize), bincode::error::DecodeError> {
    let budget = bytes
        .len()
        .saturating_mul(BINCODE_DECODE_EXPANSION)
        .clamp(BINCODE_MIN_DECODE_BUDGET, BINCODE_LIMIT_BYTES);
    // Bincode's configured limit is const-generic. Select the next power-of-two
    // budget, bounded by the existing protocol ceiling; never base it on an
    // untrusted container length or compressed frame size.
    macro_rules! decode {
        ($limit:expr) => {
            if fixed_int_encoding {
                bincode::decode_from_slice(
                    bytes,
                    bincode::config::standard()
                        .with_fixed_int_encoding()
                        .with_limit::<$limit>(),
                )
            } else {
                bincode::decode_from_slice(
                    bytes,
                    bincode::config::standard().with_limit::<$limit>(),
                )
            }
        };
    }
    match budget {
        0..=65_536 => decode!(65_536),
        65_537..=131_072 => decode!(131_072),
        131_073..=262_144 => decode!(262_144),
        262_145..=524_288 => decode!(524_288),
        524_289..=1_048_576 => decode!(1_048_576),
        1_048_577..=2_097_152 => decode!(2_097_152),
        2_097_153..=4_194_304 => decode!(4_194_304),
        4_194_305..=8_388_608 => decode!(8_388_608),
        8_388_609..=16_777_216 => decode!(16_777_216),
        16_777_217..=33_554_432 => decode!(33_554_432),
        33_554_433..=67_108_864 => decode!(67_108_864),
        67_108_865..=134_217_728 => decode!(134_217_728),
        134_217_729..=268_435_456 => decode!(268_435_456),
        268_435_457..=536_870_912 => decode!(536_870_912),
        _ => decode!(BINCODE_LIMIT_BYTES),
    }
}

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
