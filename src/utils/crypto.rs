use aes_gcm::aead::{rand_core::RngCore, Aead, KeyInit, OsRng, Payload};
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

pub fn build_aad(user_id: i64, wallet_address: &str) -> Vec<u8> {
    format!("{}:{}", user_id, wallet_address.to_lowercase()).into_bytes()
}

pub fn encrypt(key: EncryptionKey, plaintext: &[u8], aad: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let cipher = Aes256Gcm::new_from_slice(key.as_bytes()).expect("key length is 32 bytes");
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let payload = Payload { msg: plaintext, aad };
    let ciphertext = cipher
        .encrypt(nonce, payload)
        .map_err(|_| eyre!("encrypt failed"))?;
    Ok((ciphertext, nonce_bytes.to_vec()))
}

pub fn decrypt(key: EncryptionKey, nonce: &[u8], ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    if nonce.len() != 12 {
        return Err(eyre!("invalid encryption nonce"));
    }

    let cipher = Aes256Gcm::new_from_slice(key.as_bytes()).expect("key length is 32 bytes");
    let nonce = Nonce::from_slice(nonce);
    let payload = Payload { msg: ciphertext, aad };
    let plaintext = cipher
        .decrypt(nonce, payload)
        .map_err(|_| eyre!("decrypt failed"))?;
    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY_HEX: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn encryption_key_from_valid_hex() {
        let key = EncryptionKey::from_hex(TEST_KEY_HEX);
        assert!(key.is_ok());
        assert_eq!(key.unwrap().as_bytes().len(), 32);
    }

    #[test]
    fn encryption_key_from_hex_with_0x_prefix() {
        let key = EncryptionKey::from_hex(&format!("0x{}", TEST_KEY_HEX));
        assert!(key.is_ok());
    }

    #[test]
    fn encryption_key_from_hex_with_whitespace() {
        let key = EncryptionKey::from_hex(&format!("  {}  ", TEST_KEY_HEX));
        assert!(key.is_ok());
    }

    #[test]
    fn encryption_key_from_short_hex_fails() {
        let result = EncryptionKey::from_hex("0123456789abcdef");
        assert!(result.is_err());
    }

    #[test]
    fn encryption_key_from_invalid_hex_fails() {
        let result = EncryptionKey::from_hex("gg23456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef");
        assert!(result.is_err());
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = EncryptionKey::from_hex(TEST_KEY_HEX).unwrap();
        let plaintext = b"hello world";
        let aad = b"1:0xabc123";

        let (ciphertext, nonce) = encrypt(key, plaintext, aad).unwrap();
        let decrypted = decrypt(key, &nonce, &ciphertext, aad).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn encrypt_produces_different_ciphertext_each_time() {
        let key = EncryptionKey::from_hex(TEST_KEY_HEX).unwrap();
        let plaintext = b"hello world";
        let aad = b"1:0xabc123";

        let (ciphertext1, _) = encrypt(key, plaintext, aad).unwrap();
        let (ciphertext2, _) = encrypt(key, plaintext, aad).unwrap();

        assert_ne!(ciphertext1, ciphertext2);
    }

    #[test]
    fn decrypt_with_wrong_key_fails() {
        let key1 = EncryptionKey::from_hex(TEST_KEY_HEX).unwrap();
        let key2 = EncryptionKey::from_hex(
            "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210",
        )
        .unwrap();
        let plaintext = b"secret data";
        let aad = b"1:0xabc123";

        let (ciphertext, nonce) = encrypt(key1, plaintext, aad).unwrap();
        let result = decrypt(key2, &nonce, &ciphertext, aad);

        assert!(result.is_err());
    }

    #[test]
    fn decrypt_with_wrong_nonce_fails() {
        let key = EncryptionKey::from_hex(TEST_KEY_HEX).unwrap();
        let plaintext = b"secret data";
        let aad = b"1:0xabc123";

        let (ciphertext, _) = encrypt(key, plaintext, aad).unwrap();
        let wrong_nonce = [0u8; 12];
        let result = decrypt(key, &wrong_nonce, &ciphertext, aad);

        assert!(result.is_err());
    }

    #[test]
    fn decrypt_with_wrong_aad_fails() {
        let key = EncryptionKey::from_hex(TEST_KEY_HEX).unwrap();
        let plaintext = b"secret data";
        let aad = b"1:0xabc123";
        let wrong_aad = b"2:0xdef456";

        let (ciphertext, nonce) = encrypt(key, plaintext, aad).unwrap();
        let result = decrypt(key, &nonce, &ciphertext, wrong_aad);

        assert!(result.is_err());
    }

    #[test]
    fn decrypt_with_invalid_nonce_length_fails() {
        let key = EncryptionKey::from_hex(TEST_KEY_HEX).unwrap();
        let aad = b"1:0xabc123";
        let (ciphertext, _) = encrypt(key, b"data", aad).unwrap();

        let result = decrypt(key, &[0u8; 8], &ciphertext, aad);
        assert!(result.is_err());
    }

    #[test]
    fn encrypt_empty_plaintext() {
        let key = EncryptionKey::from_hex(TEST_KEY_HEX).unwrap();
        let aad = b"1:0xabc123";
        let (ciphertext, nonce) = encrypt(key, b"", aad).unwrap();
        let decrypted = decrypt(key, &nonce, &ciphertext, aad).unwrap();
        assert!(decrypted.is_empty());
    }

    #[test]
    fn encrypt_large_plaintext() {
        let key = EncryptionKey::from_hex(TEST_KEY_HEX).unwrap();
        let plaintext = vec![0xabu8; 10_000];
        let aad = b"1:0xabc123";

        let (ciphertext, nonce) = encrypt(key, &plaintext, aad).unwrap();
        let decrypted = decrypt(key, &nonce, &ciphertext, aad).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn build_aad_formats_correctly() {
        let aad = build_aad(123, "0xAbC123");
        assert_eq!(aad, b"123:0xabc123");
    }
}
