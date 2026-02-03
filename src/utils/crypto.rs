use aes_gcm::aead::{rand_core::RngCore, Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use color_eyre::eyre::{eyre, Result};

#[derive(Clone, Copy)]
pub struct EncryptionKey([u8; 32]);

impl EncryptionKey {
    pub fn from_hex(raw: &str) -> Result<Self> {
        let trimmed = raw.trim();
        let trimmed = trimmed.strip_prefix("0x").unwrap_or(trimmed);
        if trimmed.len() != 64 {
            return Err(eyre!("ENCRYPTION_KEY must be 32 bytes (64 hex characters)"));
        }

        let mut bytes = [0u8; 32];
        for (idx, chunk) in trimmed.as_bytes().chunks(2).enumerate() {
            let pair = std::str::from_utf8(chunk)
                .map_err(|_| eyre!("ENCRYPTION_KEY contains invalid hex characters"))?;
            let value = u8::from_str_radix(pair, 16)
                .map_err(|_| eyre!("ENCRYPTION_KEY contains invalid hex characters"))?;
            bytes[idx] = value;
        }

        Ok(Self(bytes))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

pub fn encrypt(key: EncryptionKey, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let cipher = Aes256Gcm::new_from_slice(key.as_bytes()).expect("key length is 32 bytes");
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| eyre!("encrypt failed"))?;
    Ok((ciphertext, nonce_bytes.to_vec()))
}

pub fn decrypt(key: EncryptionKey, nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>> {
    if nonce.len() != 12 {
        return Err(eyre!("invalid encryption nonce"));
    }

    let cipher = Aes256Gcm::new_from_slice(key.as_bytes()).expect("key length is 32 bytes");
    let nonce = Nonce::from_slice(nonce);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| eyre!("decrypt failed"))?;
    Ok(plaintext)
}
