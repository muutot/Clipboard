//! Executable format prototype. No application command uses this format yet.
use aes_gcm::{
    aead::{Aead, Payload},
    Aes256Gcm, KeyInit, Nonce,
};
use hmac::Hmac;
use rand::TryRng;
use sha2::{Digest, Sha256};
use std::io::{self, Read, Write};
use zeroize::Zeroizing;

const MAGIC: &[u8; 8] = b"CLIPENC2";
const HEADER: usize = 49; // magic + version + salt + nonce prefix
const CHUNK: usize = 1024 * 1024;
const TOTAL: u64 = 18 * 1024 * 1024 * 1024;
const MAX_FRAMES: u32 = 1_000_000;
const ITERATIONS: u32 = 600_000;
const FAILURE: &str = "wrong password or damaged backup";

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, FAILURE)
}

fn cipher(password: &str, header: &[u8; HEADER]) -> io::Result<Aes256Gcm> {
    if password.is_empty() || password.len() > 1024 || &header[..8] != MAGIC || header[8] != 2 {
        return Err(invalid());
    }
    let mut salt = b"clipboard-portable-backup-v2\0".to_vec();
    salt.extend_from_slice(&header[9..41]);
    let mut key = Zeroizing::new([0u8; 32]);
    pbkdf2::pbkdf2::<Hmac<Sha256>>(password.as_bytes(), &salt, ITERATIONS, key.as_mut())
        .map_err(|_| invalid())?;
    Ok(Aes256Gcm::new((&*key).into()))
}

fn nonce(header: &[u8; HEADER], index: u32) -> [u8; 12] {
    let mut value = [0u8; 12];
    value[..8].copy_from_slice(&header[41..]);
    value[8..].copy_from_slice(&index.to_be_bytes());
    value
}
fn aad(header: &[u8; HEADER], index: u32, size: u32) -> Vec<u8> {
    let mut value = header.to_vec();
    value.extend_from_slice(&index.to_be_bytes());
    value.extend_from_slice(&size.to_be_bytes());
    value
}

struct Encoder<W> {
    writer: W,
    header: [u8; HEADER],
    cipher: Aes256Gcm,
    index: u32,
    bytes: u64,
    hash: Sha256,
    failed: bool,
}
impl<W: Write> Encoder<W> {
    fn new(mut writer: W, password: &str) -> io::Result<Self> {
        let mut header = [0u8; HEADER];
        header[..8].copy_from_slice(MAGIC);
        header[8] = 2;
        rand::rngs::SysRng
            .try_fill_bytes(&mut header[9..])
            .map_err(|_| io::Error::other("secure random generation failed"))?;
        let cipher = cipher(password, &header)?;
        writer.write_all(&header)?;
        Ok(Self {
            writer,
            header,
            cipher,
            index: 0,
            bytes: 0,
            hash: Sha256::new(),
            failed: false,
        })
    }
    fn frame(&mut self, plaintext: &[u8]) -> io::Result<()> {
        if self.failed || self.index >= MAX_FRAMES || plaintext.len() > CHUNK + 1 {
            self.failed = true;
            return Err(invalid());
        }
        // Any encryption/write failure poisons the encoder, preventing nonce reuse.
        self.failed = true;
        let size = u32::try_from(plaintext.len()).map_err(|_| invalid())?;
        let ciphertext = self
            .cipher
            .encrypt(
                &Nonce::from(nonce(&self.header, self.index)),
                Payload {
                    msg: plaintext,
                    aad: &aad(&self.header, self.index, size),
                },
            )
            .map_err(|_| invalid())?;
        self.writer.write_all(&size.to_be_bytes())?;
        self.writer.write_all(&ciphertext)?;
        self.index += 1;
        self.failed = false;
        Ok(())
    }
    fn data(&mut self, data: &[u8]) -> io::Result<()> {
        if data.is_empty() || data.len() > CHUNK || self.bytes + data.len() as u64 > TOTAL {
            self.failed = true;
            return Err(invalid());
        }
        let mut payload = Zeroizing::new(Vec::with_capacity(data.len() + 1));
        payload.push(1);
        payload.extend_from_slice(data);
        self.frame(&payload)?;
        self.bytes += data.len() as u64;
        self.hash.update(data);
        Ok(())
    }
    fn finish(mut self) -> io::Result<W> {
        let mut payload = Zeroizing::new(vec![0]);
        payload.extend_from_slice(&self.bytes.to_be_bytes());
        payload.extend_from_slice(&self.index.to_be_bytes());
        payload.extend_from_slice(&self.hash.clone().finalize());
        self.frame(&payload)?;
        self.writer.flush()?;
        Ok(self.writer)
    }
}

struct Decoder<R> {
    reader: R,
    header: [u8; HEADER],
    cipher: Aes256Gcm,
    index: u32,
    bytes: u64,
    hash: Sha256,
    finished: bool,
    failed: bool,
}
impl<R: Read> Decoder<R> {
    fn new(mut reader: R, password: &str) -> io::Result<Self> {
        let mut header = [0u8; HEADER];
        reader.read_exact(&mut header).map_err(|_| invalid())?;
        let cipher = cipher(password, &header)?;
        Ok(Self {
            reader,
            header,
            cipher,
            index: 0,
            bytes: 0,
            hash: Sha256::new(),
            finished: false,
            failed: false,
        })
    }
    /// Returned data is provisional until the authenticated final frame and EOF.
    /// Callers must not mutate live data before consuming the whole stream.
    fn next(&mut self) -> io::Result<Option<Zeroizing<Vec<u8>>>> {
        if self.failed {
            return Err(invalid());
        }
        if self.finished {
            return Ok(None);
        }
        self.failed = true;
        if self.index >= MAX_FRAMES {
            return Err(invalid());
        }
        let mut length = [0u8; 4];
        self.reader.read_exact(&mut length).map_err(|_| invalid())?;
        let size = u32::from_be_bytes(length);
        if size == 0 || size as usize > CHUNK + 1 {
            return Err(invalid());
        }
        let mut ciphertext = vec![0u8; size as usize + 16];
        self.reader
            .read_exact(&mut ciphertext)
            .map_err(|_| invalid())?;
        let mut plaintext = Zeroizing::new(
            self.cipher
                .decrypt(
                    &Nonce::from(nonce(&self.header, self.index)),
                    Payload {
                        msg: &ciphertext,
                        aad: &aad(&self.header, self.index, size),
                    },
                )
                .map_err(|_| invalid())?,
        );
        match plaintext[0] {
            0 => {
                if plaintext.len() != 45
                    || plaintext[1..9] != self.bytes.to_be_bytes()
                    || plaintext[9..13] != self.index.to_be_bytes()
                    || plaintext[13..] != self.hash.clone().finalize()[..]
                {
                    return Err(invalid());
                }
                let mut tail = [0u8; 1];
                if self.reader.read(&mut tail).map_err(|_| invalid())? != 0 {
                    return Err(invalid());
                }
                self.finished = true;
                self.failed = false;
                Ok(None)
            }
            1 if plaintext.len() > 1 => {
                let count = (plaintext.len() - 1) as u64;
                if self.bytes + count > TOTAL {
                    return Err(invalid());
                }
                self.bytes += count;
                self.index += 1;
                plaintext.remove(0);
                self.hash.update(&plaintext);
                self.failed = false;
                Ok(Some(plaintext))
            }
            _ => Err(invalid()),
        }
    }
}

fn encode(data: &[u8]) -> Vec<u8> {
    let mut encoder = Encoder::new(Vec::new(), "synthetic strong password").unwrap();
    for chunk in data.chunks(CHUNK) {
        encoder.data(chunk).unwrap();
    }
    encoder.finish().unwrap()
}
fn decode(bytes: &[u8], password: &str) -> io::Result<Vec<u8>> {
    let mut decoder = Decoder::new(bytes, password)?;
    let mut value = Vec::new();
    while let Some(data) = decoder.next()? {
        value.extend_from_slice(&data);
    }
    Ok(value)
}

#[test]
fn authenticated_codec_roundtrip_and_empty_stream() {
    for data in [
        vec![],
        b"synthetic secret never in a file".to_vec(),
        vec![b'x'; CHUNK + 1],
    ] {
        let encoded = encode(&data);
        assert_eq!(decode(&encoded, "synthetic strong password").unwrap(), data);
        if !data.is_empty() {
            assert!(!encoded.windows(data.len()).any(|window| window == data));
        }
    }
}

#[test]
fn authentication_rejects_password_tampering_truncation_and_append() {
    let encoded = encode(b"synthetic payload");
    assert!(decode(&encoded, "wrong password").is_err());
    for index in [0, 8, 9, 41, HEADER, HEADER + 4, encoded.len() - 1] {
        let mut damaged = encoded.clone();
        damaged[index] ^= 1;
        assert!(
            decode(&damaged, "synthetic strong password").is_err(),
            "byte {index}"
        );
    }
    for length in [
        0,
        HEADER - 1,
        HEADER,
        encoded.len() - 1,
        HEADER + 4 + 18 + 16,
    ] {
        assert!(decode(&encoded[..length], "synthetic strong password").is_err());
    }
    let mut appended = encoded;
    appended.push(0);
    assert!(decode(&appended, "synthetic strong password").is_err());
}

#[test]
fn independent_exports_are_randomized_and_oversized_lengths_fail_before_allocation() {
    let first = encode(b"same synthetic data");
    let second = encode(b"same synthetic data");
    assert_ne!(first, second);
    let mut damaged = first;
    damaged[HEADER..HEADER + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(decode(&damaged, "synthetic strong password").is_err());
}

#[test]
fn encoder_rejects_empty_password_and_poisoned_stream() {
    assert!(Encoder::new(Vec::new(), "").is_err());
    let mut encoder = Encoder::new(Vec::new(), "synthetic strong password").unwrap();
    assert!(encoder.data(&[]).is_err());
    assert!(encoder.data(b"later").is_err());
    assert!(encoder.finish().is_err());
}

#[test]
fn frame_reordering_duplication_and_write_failures_are_rejected() {
    let mut encoder = Encoder::new(Vec::new(), "synthetic strong password").unwrap();
    encoder.data(b"first").unwrap();
    encoder.data(b"other").unwrap();
    let encoded = encoder.finish().unwrap();
    let frame_size = 4 + 1 + 5 + 16;
    let mut reordered = encoded.clone();
    reordered[HEADER..HEADER + frame_size]
        .copy_from_slice(&encoded[HEADER + frame_size..HEADER + 2 * frame_size]);
    reordered[HEADER + frame_size..HEADER + 2 * frame_size]
        .copy_from_slice(&encoded[HEADER..HEADER + frame_size]);
    assert!(decode(&reordered, "synthetic strong password").is_err());
    let mut duplicated = encoded.clone();
    duplicated[HEADER + frame_size..HEADER + 2 * frame_size]
        .copy_from_slice(&encoded[HEADER..HEADER + frame_size]);
    assert!(decode(&duplicated, "synthetic strong password").is_err());

    struct FailingWriter {
        remaining: usize,
    }
    impl Write for FailingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.remaining == 0 {
                return Err(io::Error::other("synthetic disk full"));
            }
            let count = bytes.len().min(self.remaining);
            self.remaining -= count;
            Ok(count)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut encoder = Encoder::new(
        FailingWriter {
            remaining: HEADER + 5,
        },
        "synthetic strong password",
    )
    .unwrap();
    assert!(encoder.data(b"content").is_err());
    assert!(encoder.data(b"retry").is_err());
    assert!(encoder.finish().is_err());
}
