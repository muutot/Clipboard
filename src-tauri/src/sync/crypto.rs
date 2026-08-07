use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use hmac::Hmac;
use sha2::Sha256;

const SALT: &[u8] = b"clipboard-sync-salt-v1";
const NONCE_SIZE: usize = 12;
const TAG_SIZE: usize = 16;
const KEY_SIZE: usize = 32;
const PBKDF2_ITERATIONS: u32 = 100_000;

fn derive_key(password: &str) -> [u8; KEY_SIZE] {
    let mut key = [0u8; KEY_SIZE];
    pbkdf2::pbkdf2::<Hmac<Sha256>>(password.as_bytes(), SALT, PBKDF2_ITERATIONS, &mut key);
    key
}

pub fn encrypt(plaintext: &[u8], password: &str) -> Result<Vec<u8>, String> {
    if password.is_empty() {
        return Err("encryption password is empty".to_string());
    }

    let key = derive_key(password);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));

    let nonce_bytes: [u8; NONCE_SIZE] = rand::random();
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| format!("encryption failed: {e}"))?;

    let mut result = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);

    Ok(result)
}

pub fn decrypt(data: &[u8], password: &str) -> Result<Vec<u8>, String> {
    if password.is_empty() {
        return Err("decryption password is empty".to_string());
    }

    if data.len() < NONCE_SIZE + TAG_SIZE {
        return Err("encrypted data too short".to_string());
    }

    let key = derive_key(password);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));

    let nonce = Nonce::from_slice(&data[..NONCE_SIZE]);
    let ciphertext = &data[NONCE_SIZE..];

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| "decryption failed: wrong password or corrupted data".to_string())
}
