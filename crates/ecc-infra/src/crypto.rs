use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::Rng;
use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("encryption failed")]
    Encrypt,
    #[error("base64 decode failed: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("invalid ciphertext length")]
    InvalidLength,
    #[error("utf-8 decode failed: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
}

impl From<aes_gcm::Error> for CryptoError {
    fn from(_: aes_gcm::Error) -> Self {
        CryptoError::Encrypt
    }
}

/// Derive a 32-byte AES key from an arbitrary seed string.
pub fn derive_key(seed: &str) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(seed.as_bytes());
    hasher.finalize().to_vec()
}

/// Encrypt with random nonce, prepend nonce to ciphertext, base64 encode.
pub fn encrypt(key: &[u8], plaintext: &str) -> Result<String, CryptoError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::Encrypt)?;
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher.encrypt(nonce, plaintext.as_bytes())?;
    let mut combined = nonce_bytes.to_vec();
    combined.extend(&ciphertext);
    Ok(B64.encode(&combined))
}

/// Decrypt: base64 decode -> split nonce[12] + ciphertext -> AES-GCM decrypt.
pub fn decrypt(key: &[u8], encoded: &str) -> Result<String, CryptoError> {
    let data = B64.decode(encoded)?;
    if data.len() < 12 + 16 {
        return Err(CryptoError::InvalidLength);
    }
    let (nonce_bytes, ciphertext) = data.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::Encrypt)?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher.decrypt(nonce, ciphertext)?;
    Ok(String::from_utf8(plaintext)?)
}
