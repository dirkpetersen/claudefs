//! AES-256-GCM and ChaCha20-Poly1305 AEAD encryption with HKDF key derivation

use crate::error::ReduceError;
use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit};
use chacha20poly1305::ChaCha20Poly1305;
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// 256-bit (32-byte) encryption key
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct EncryptionKey(pub [u8; 32]);

impl std::fmt::Debug for EncryptionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EncryptionKey([REDACTED])")
    }
}

/// 96-bit (12-byte) nonce for AEAD ciphers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Nonce(pub [u8; 12]);

/// AEAD cipher selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EncryptionAlgorithm {
    /// AES-256-GCM — hardware accelerated on x86 with AES-NI
    #[default]
    AesGcm256,
    /// ChaCha20-Poly1305 — constant-time, fast on non-AES hardware
    ChaCha20Poly1305,
}

/// Encrypted chunk: ciphertext (with 16-byte auth tag), nonce, algorithm
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedChunk {
    /// Ciphertext with 16-byte AEAD authentication tag appended
    pub ciphertext: Vec<u8>,
    /// Nonce used for this encryption operation
    pub nonce: Nonce,
    /// Algorithm (needed for decryption)
    pub algo: EncryptionAlgorithm,
}

/// Derive a per-chunk key from master key + chunk hash using HKDF-SHA256
pub fn derive_chunk_key(master_key: &EncryptionKey, chunk_hash: &[u8; 32]) -> EncryptionKey {
    let hk = Hkdf::<Sha256>::new(None, &master_key.0);
    let mut okm = [0u8; 32];
    let mut info = Vec::with_capacity(18 + 32);
    info.extend_from_slice(b"claudefs-chunk-key");
    info.extend_from_slice(chunk_hash);
    hk.expand(&info, &mut okm).expect("HKDF expand failed");
    EncryptionKey(okm)
}

/// Generate a cryptographically random 12-byte nonce
pub fn random_nonce() -> Nonce {
    use rand::RngCore;
    let mut bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut bytes);
    Nonce(bytes)
}

/// Encrypt plaintext. A random nonce is generated and stored in the result.
pub fn encrypt(
    plaintext: &[u8],
    key: &EncryptionKey,
    algo: EncryptionAlgorithm,
) -> Result<EncryptedChunk, ReduceError> {
    let nonce = random_nonce();
    let ciphertext = match algo {
        EncryptionAlgorithm::AesGcm256 => {
            let cipher = Aes256Gcm::new_from_slice(&key.0)
                .map_err(|e| ReduceError::EncryptionFailed(e.to_string()))?;
            let n = aes_gcm::Nonce::from_slice(&nonce.0);
            cipher
                .encrypt(n, plaintext)
                .map_err(|e| ReduceError::EncryptionFailed(e.to_string()))?
        }
        EncryptionAlgorithm::ChaCha20Poly1305 => {
            use chacha20poly1305::aead::Aead as _;
            use chacha20poly1305::KeyInit as _;
            let cipher = ChaCha20Poly1305::new_from_slice(&key.0)
                .map_err(|e| ReduceError::EncryptionFailed(e.to_string()))?;
            let n = chacha20poly1305::Nonce::from_slice(&nonce.0);
            cipher
                .encrypt(n, plaintext)
                .map_err(|e| ReduceError::EncryptionFailed(e.to_string()))?
        }
    };
    Ok(EncryptedChunk {
        ciphertext,
        nonce,
        algo,
    })
}

/// Decrypt an EncryptedChunk. Returns DecryptionAuthFailed if tampered/corrupted.
pub fn decrypt(chunk: &EncryptedChunk, key: &EncryptionKey) -> Result<Vec<u8>, ReduceError> {
    match chunk.algo {
        EncryptionAlgorithm::AesGcm256 => {
            let cipher = Aes256Gcm::new_from_slice(&key.0)
                .map_err(|e| ReduceError::EncryptionFailed(e.to_string()))?;
            let n = aes_gcm::Nonce::from_slice(&chunk.nonce.0);
            cipher
                .decrypt(n, chunk.ciphertext.as_ref())
                .map_err(|_| ReduceError::DecryptionAuthFailed)
        }
        EncryptionAlgorithm::ChaCha20Poly1305 => {
            use chacha20poly1305::aead::Aead as _;
            use chacha20poly1305::KeyInit as _;
            let cipher = ChaCha20Poly1305::new_from_slice(&key.0)
                .map_err(|e| ReduceError::EncryptionFailed(e.to_string()))?;
            let n = chacha20poly1305::Nonce::from_slice(&chunk.nonce.0);
            cipher
                .decrypt(n, chunk.ciphertext.as_ref())
                .map_err(|_| ReduceError::DecryptionAuthFailed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn test_key() -> EncryptionKey {
        EncryptionKey([42u8; 32])
    }

    proptest! {
        #[test]
        fn prop_aesgcm_roundtrip(data in prop::collection::vec(0u8..=255, 0..65_536)) {
            let key = test_key();
            let enc = encrypt(&data, &key, EncryptionAlgorithm::AesGcm256).unwrap();
            let dec = decrypt(&enc, &key).unwrap();
            prop_assert_eq!(dec, data);
        }
        #[test]
        fn prop_chacha_roundtrip(data in prop::collection::vec(0u8..=255, 0..65_536)) {
            let key = test_key();
            let enc = encrypt(&data, &key, EncryptionAlgorithm::ChaCha20Poly1305).unwrap();
            let dec = decrypt(&enc, &key).unwrap();
            prop_assert_eq!(dec, data);
        }
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let key = test_key();
        let mut enc = encrypt(b"secret", &key, EncryptionAlgorithm::AesGcm256).unwrap();
        enc.ciphertext[0] ^= 0xff;
        assert!(matches!(
            decrypt(&enc, &key),
            Err(ReduceError::DecryptionAuthFailed)
        ));
    }

    #[test]
    fn wrong_key_fails() {
        let key = test_key();
        let enc = encrypt(b"secret", &key, EncryptionAlgorithm::AesGcm256).unwrap();
        let wrong = EncryptionKey([99u8; 32]);
        assert!(matches!(
            decrypt(&enc, &wrong),
            Err(ReduceError::DecryptionAuthFailed)
        ));
    }

    #[test]
    fn hkdf_is_deterministic() {
        let master = test_key();
        let hash = [1u8; 32];
        assert_eq!(
            derive_chunk_key(&master, &hash).0,
            derive_chunk_key(&master, &hash).0
        );
    }

    #[test]
    fn different_chunks_get_different_keys() {
        let master = test_key();
        let k1 = derive_chunk_key(&master, &[1u8; 32]);
        let k2 = derive_chunk_key(&master, &[2u8; 32]);
        assert_ne!(k1.0, k2.0);
    }
}
