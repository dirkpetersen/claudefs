//! At-rest encryption for storage blocks.

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::error::StorageError;
use crate::StorageResult;

/// Supported encryption algorithms for data at rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EncryptionAlgorithm {
    /// AES-256 in GCM mode (12-byte nonce, 16-byte tag)
    Aes256Gcm,
    /// ChaCha20-Poly1305 stream cipher
    ChaCha20Poly1305,
    /// No encryption (plaintext passthrough)
    None,
}

impl fmt::Display for EncryptionAlgorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EncryptionAlgorithm::Aes256Gcm => write!(f, "AES-256-GCM"),
            EncryptionAlgorithm::ChaCha20Poly1305 => write!(f, "ChaCha20-Poly1305"),
            EncryptionAlgorithm::None => write!(f, "None"),
        }
    }
}

/// Encryption key with metadata.
#[derive(Debug, Clone)]
pub struct EncryptionKey {
    /// Key identifier (UUID or external key reference)
    id: String,
    /// Encryption algorithm
    algorithm: EncryptionAlgorithm,
    /// Key material (32 bytes for AES-256)
    key_bytes: Vec<u8>,
    /// Creation timestamp (seconds since epoch)
    created_at_secs: u64,
    /// Previous key ID if this is a rotation
    rotated_from: Option<String>,
}

impl EncryptionKey {
    /// Creates a new encryption key with validation.
    pub fn new(
        id: String,
        algorithm: EncryptionAlgorithm,
        key_bytes: Vec<u8>,
    ) -> StorageResult<Self> {
        let required_len = match algorithm {
            EncryptionAlgorithm::Aes256Gcm | EncryptionAlgorithm::ChaCha20Poly1305 => 32,
            EncryptionAlgorithm::None => 0,
        };

        if algorithm != EncryptionAlgorithm::None && key_bytes.len() != required_len {
            return Err(StorageError::SerializationError {
                reason: format!(
                    "Invalid key length for {:?}: expected {} bytes, got {}",
                    algorithm,
                    required_len,
                    key_bytes.len()
                ),
            });
        }

        Ok(Self {
            id,
            algorithm,
            key_bytes,
            created_at_secs: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            rotated_from: None,
        })
    }

    /// Generates a mock key for testing.
    pub fn generate_mock(algorithm: EncryptionAlgorithm) -> Self {
        let key_bytes = match algorithm {
            EncryptionAlgorithm::Aes256Gcm | EncryptionAlgorithm::ChaCha20Poly1305 => {
                (0..32).map(|i| (i as u8) ^ 0x5A).collect()
            }
            EncryptionAlgorithm::None => vec![],
        };

        let id = format!(
            "mock-key-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );

        Self {
            id,
            algorithm,
            key_bytes,
            created_at_secs: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            rotated_from: None,
        }
    }

    /// Returns the key length in bytes.
    pub fn key_len(&self) -> usize {
        self.key_bytes.len()
    }

    /// Returns the key ID.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the algorithm.
    pub fn algorithm(&self) -> EncryptionAlgorithm {
        self.algorithm
    }

    /// Returns the key bytes (for internal encryption use).
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.key_bytes
    }
}

/// Encrypted block data with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedBlock {
    /// Encrypted data
    ciphertext: Vec<u8>,
    /// Per-block nonce (12 bytes for GCM)
    nonce: Vec<u8>,
    /// Authentication tag (16 bytes for GCM)
    tag: Vec<u8>,
    /// Which key was used
    key_id: String,
    /// Algorithm used
    algorithm: EncryptionAlgorithm,
    /// Size before encryption
    original_size: u64,
}

impl EncryptedBlock {
    /// Returns the ciphertext.
    pub fn ciphertext(&self) -> &[u8] {
        &self.ciphertext
    }

    /// Returns the nonce.
    pub fn nonce(&self) -> &[u8] {
        &self.nonce
    }

    /// Returns the authentication tag.
    pub fn tag(&self) -> &[u8] {
        &self.tag
    }

    /// Returns the key ID.
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    /// Returns the algorithm.
    pub fn algorithm(&self) -> EncryptionAlgorithm {
        self.algorithm
    }

    /// Returns the original size before encryption.
    pub fn original_size(&self) -> u64 {
        self.original_size
    }
}

/// Configuration for encryption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionConfig {
    /// Default encryption algorithm
    algorithm: EncryptionAlgorithm,
    /// Whether encryption is active
    enabled: bool,
    /// How often to rotate keys (in hours)
    key_rotation_interval_hours: u64,
}

impl Default for EncryptionConfig {
    fn default() -> Self {
        Self {
            algorithm: EncryptionAlgorithm::Aes256Gcm,
            enabled: false,
            key_rotation_interval_hours: 720, // 30 days
        }
    }
}

impl EncryptionConfig {
    /// Returns the algorithm.
    pub fn algorithm(&self) -> EncryptionAlgorithm {
        self.algorithm
    }

    /// Returns whether encryption is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Returns the key rotation interval in hours.
    pub fn key_rotation_interval_hours(&self) -> u64 {
        self.key_rotation_interval_hours
    }
}

/// Statistics for encryption operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EncryptionStats {
    /// Number of blocks encrypted
    blocks_encrypted: u64,
    /// Number of blocks decrypted
    blocks_decrypted: u64,
    /// Total bytes encrypted
    bytes_encrypted: u64,
    /// Total bytes decrypted
    bytes_decrypted: u64,
    /// Number of key rotations
    key_rotations: u64,
    /// Number of encryption errors
    encryption_errors: u64,
}

impl EncryptionStats {
    /// Returns the number of blocks encrypted.
    pub fn blocks_encrypted(&self) -> u64 {
        self.blocks_encrypted
    }

    /// Returns the number of blocks decrypted.
    pub fn blocks_decrypted(&self) -> u64 {
        self.blocks_decrypted
    }

    /// Returns the total bytes encrypted.
    pub fn bytes_encrypted(&self) -> u64 {
        self.bytes_encrypted
    }

    /// Returns the total bytes decrypted.
    pub fn bytes_decrypted(&self) -> u64 {
        self.bytes_decrypted
    }

    /// Returns the number of key rotations.
    pub fn key_rotations(&self) -> u64 {
        self.key_rotations
    }

    /// Returns the number of encryption errors.
    pub fn encryption_errors(&self) -> u64 {
        self.encryption_errors
    }
}

/// Encryption engine for at-rest data protection.
pub struct EncryptionEngine {
    /// Configuration
    config: EncryptionConfig,
    /// Active keys by ID
    keys: HashMap<String, EncryptionKey>,
    /// Current active key
    current_key_id: Option<String>,
    /// Statistics
    stats: EncryptionStats,
}

impl EncryptionEngine {
    /// Creates a new encryption engine with the given configuration.
    pub fn new(config: EncryptionConfig) -> Self {
        debug!(
            "Creating new encryption engine: enabled={}",
            config.is_enabled()
        );
        Self {
            config,
            keys: HashMap::new(),
            current_key_id: None,
            stats: EncryptionStats::default(),
        }
    }

    /// Registers an encryption key.
    pub fn add_key(&mut self, key: EncryptionKey) {
        let id = key.id().to_string();
        debug!("Adding encryption key: id={}", id);
        self.keys.insert(id, key);
    }

    /// Sets the current active encryption key.
    pub fn set_current_key(&mut self, key_id: &str) -> StorageResult<()> {
        if !self.keys.contains_key(key_id) {
            warn!("Attempted to set nonexistent key as current: {}", key_id);
            return Err(StorageError::SerializationError {
                reason: format!("Key not found: {}", key_id),
            });
        }
        debug!("Setting current encryption key: {}", key_id);
        self.current_key_id = Some(key_id.to_string());
        Ok(())
    }

    /// Encrypts data with the current key (mock: XOR cipher for testing).
    pub fn encrypt(&mut self, plaintext: &[u8]) -> StorageResult<EncryptedBlock> {
        let current_key = match &self.current_key_id {
            Some(id) => self.keys.get(id),
            None => {
                warn!("Attempted encryption without a current key");
                self.stats.encryption_errors += 1;
                return Err(StorageError::SerializationError {
                    reason: "No current encryption key set".to_string(),
                });
            }
        };

        let key = match current_key {
            Some(k) => k,
            None => {
                self.stats.encryption_errors += 1;
                return Err(StorageError::SerializationError {
                    reason: "Current key not found".to_string(),
                });
            }
        };

        if key.algorithm() == EncryptionAlgorithm::None {
            return Ok(EncryptedBlock {
                ciphertext: plaintext.to_vec(),
                nonce: vec![],
                tag: vec![],
                key_id: key.id().to_string(),
                algorithm: EncryptionAlgorithm::None,
                original_size: plaintext.len() as u64,
            });
        }

        // Generate mock nonce (12 bytes) and tag (16 bytes) using deterministic pattern
        // Based on plaintext content for reproducibility in tests
        let mut nonce = vec![0u8; 12];
        for (i, byte) in plaintext.iter().take(12).enumerate() {
            nonce[i] = *byte;
        }
        if nonce.iter().all(|&b| b == 0) {
            nonce = (0..12)
                .map(|i| (i as u8).wrapping_add(plaintext.len() as u8))
                .collect();
        }

        let mut tag = vec![0u8; 16];
        for (i, byte) in plaintext.iter().skip(12).take(16).enumerate() {
            tag[i] = *byte;
        }
        if tag.iter().all(|&b| b == 0) {
            tag = (0..16)
                .map(|i| {
                    (i as u8)
                        .wrapping_add(plaintext.len() as u8)
                        .wrapping_add(0xAA)
                })
                .collect();
        }

        // XOR encryption (mock)
        let ciphertext: Vec<u8> = plaintext
            .iter()
            .enumerate()
            .map(|(i, &b)| b ^ key.as_bytes()[i % key.as_bytes().len()])
            .collect();

        self.stats.blocks_encrypted += 1;
        self.stats.bytes_encrypted += plaintext.len() as u64;

        debug!("Encrypted {} bytes with key {}", plaintext.len(), key.id());

        Ok(EncryptedBlock {
            ciphertext,
            nonce,
            tag,
            key_id: key.id().to_string(),
            algorithm: key.algorithm(),
            original_size: plaintext.len() as u64,
        })
    }

    /// Decrypts data (mock: XOR to reverse).
    pub fn decrypt(&mut self, block: &EncryptedBlock) -> StorageResult<Vec<u8>> {
        if block.algorithm() == EncryptionAlgorithm::None {
            self.stats.blocks_decrypted += 1;
            self.stats.bytes_decrypted += block.original_size;
            return Ok(block.ciphertext().to_vec());
        }

        let key = match self.keys.get(block.key_id()) {
            Some(k) => k,
            None => {
                warn!("Decryption key not found: {}", block.key_id());
                self.stats.encryption_errors += 1;
                return Err(StorageError::SerializationError {
                    reason: format!("Key not found: {}", block.key_id()),
                });
            }
        };

        // XOR decryption (same as encryption for XOR cipher)
        let plaintext: Vec<u8> = block
            .ciphertext()
            .iter()
            .enumerate()
            .map(|(i, &b)| b ^ key.as_bytes()[i % key.as_bytes().len()])
            .collect();

        self.stats.blocks_decrypted += 1;
        self.stats.bytes_decrypted += block.original_size;

        debug!(
            "Decrypted {} bytes with key {}",
            block.original_size,
            block.key_id()
        );

        Ok(plaintext)
    }

    /// Rotates to a new key, returning the old key ID.
    pub fn rotate_key(&mut self, new_key: EncryptionKey) -> StorageResult<String> {
        let old_key_id = self.current_key_id.clone();

        let new_id = new_key.id().to_string();
        debug!(
            "Rotating encryption key from {:?} to {}",
            old_key_id, new_id
        );

        // Key rotation tracking - for future use with rotated_from field
        let _ = old_key_id.as_ref();

        self.keys.insert(new_id.clone(), new_key);
        self.current_key_id = Some(new_id);
        self.stats.key_rotations += 1;

        Ok(old_key_id.unwrap_or_else(|| "none".to_string()))
    }

    /// Returns the number of registered keys.
    pub fn key_count(&self) -> usize {
        self.keys.len()
    }

    /// Returns the current key, if set.
    pub fn current_key(&self) -> Option<&EncryptionKey> {
        self.current_key_id
            .as_ref()
            .and_then(|id| self.keys.get(id))
    }

    /// Returns a reference to the stats.
    pub fn stats(&self) -> &EncryptionStats {
        &self.stats
    }

    /// Returns whether encryption is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.is_enabled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        let key = EncryptionKey::generate_mock(EncryptionAlgorithm::Aes256Gcm);
        engine.add_key(key.clone());
        engine.set_current_key(key.id()).unwrap();

        let plaintext = b"Hello, ClaudeFS encryption!";
        let encrypted = engine.encrypt(plaintext).unwrap();
        let decrypted = engine.decrypt(&encrypted).unwrap();

        assert_eq!(plaintext.to_vec(), decrypted);
    }

    #[test]
    fn test_encrypt_with_none_algorithm() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        let key =
            EncryptionKey::new("test-key".to_string(), EncryptionAlgorithm::None, vec![]).unwrap();
        engine.add_key(key.clone());
        engine.set_current_key(key.id()).unwrap();

        let plaintext = b"Plaintext data";
        let encrypted = engine.encrypt(plaintext).unwrap();

        assert_eq!(encrypted.algorithm(), EncryptionAlgorithm::None);
        assert_eq!(encrypted.ciphertext(), plaintext);
    }

    #[test]
    fn test_key_generation() {
        let key = EncryptionKey::generate_mock(EncryptionAlgorithm::Aes256Gcm);
        assert_eq!(key.key_len(), 32);
        assert_eq!(key.algorithm(), EncryptionAlgorithm::Aes256Gcm);
    }

    #[test]
    fn test_key_length_validation() {
        let result = EncryptionKey::new(
            "test-key".to_string(),
            EncryptionAlgorithm::Aes256Gcm,
            vec![0u8; 16], // Wrong size
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_key_rotation() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        let key1 = EncryptionKey::new(
            "key-1".to_string(),
            EncryptionAlgorithm::Aes256Gcm,
            vec![0x11; 32],
        )
        .unwrap();
        engine.add_key(key1.clone());
        engine.set_current_key(key1.id()).unwrap();

        let key2 = EncryptionKey::new(
            "key-2".to_string(),
            EncryptionAlgorithm::Aes256Gcm,
            vec![0x22; 32],
        )
        .unwrap();
        let old_id = engine.rotate_key(key2).unwrap();

        assert_eq!(old_id, "key-1");
        assert_eq!(engine.stats().key_rotations(), 1);
    }

    #[test]
    fn test_stats_tracking() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        let key = EncryptionKey::generate_mock(EncryptionAlgorithm::Aes256Gcm);
        engine.add_key(key.clone());
        engine.set_current_key(key.id()).unwrap();

        let plaintext = b"Test data for stats";
        let encrypted = engine.encrypt(plaintext).unwrap();
        engine.decrypt(&encrypted).unwrap();

        let stats = engine.stats();
        assert_eq!(stats.blocks_encrypted(), 1);
        assert_eq!(stats.blocks_decrypted(), 1);
        assert_eq!(stats.bytes_encrypted(), plaintext.len() as u64);
        assert_eq!(stats.bytes_decrypted(), plaintext.len() as u64);
    }

    #[test]
    fn test_config_defaults() {
        let config = EncryptionConfig::default();
        assert!(!config.is_enabled());
        assert_eq!(config.algorithm(), EncryptionAlgorithm::Aes256Gcm);
        assert_eq!(config.key_rotation_interval_hours(), 720);
    }

    #[test]
    fn test_encrypt_without_current_key() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        let result = engine.encrypt(b"test");
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_with_wrong_key() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());

        // First key
        let key1 = EncryptionKey::new(
            "key-1".to_string(),
            EncryptionAlgorithm::Aes256Gcm,
            vec![0x11; 32],
        )
        .unwrap();
        engine.add_key(key1.clone());
        engine.set_current_key(key1.id()).unwrap();

        // Encrypt with key1
        let plaintext = b"Secret data";
        let encrypted = engine.encrypt(plaintext).unwrap();

        // Create engine2 with different key (but same ID to simulate wrong key)
        let mut engine2 = EncryptionEngine::new(EncryptionConfig::default());
        let key2 = EncryptionKey::new(
            "key-1".to_string(),
            EncryptionAlgorithm::Aes256Gcm,
            vec![0x22; 32], // Different key material but same ID
        )
        .unwrap();
        engine2.add_key(key2.clone());
        engine2.set_current_key(key2.id()).unwrap();

        // Try to decrypt with different key - should get different data
        let decrypted = engine2.decrypt(&encrypted).unwrap();
        assert_ne!(decrypted, plaintext.to_vec());
    }

    #[test]
    fn test_multiple_encrypt_operations() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        let key = EncryptionKey::generate_mock(EncryptionAlgorithm::Aes256Gcm);
        engine.add_key(key.clone());
        engine.set_current_key(key.id()).unwrap();

        for i in 0..5 {
            let plaintext = format!("Test message {}", i);
            let encrypted = engine.encrypt(plaintext.as_bytes()).unwrap();
            let decrypted = engine.decrypt(&encrypted).unwrap();
            assert_eq!(plaintext.as_bytes(), decrypted.as_slice());
        }

        assert_eq!(engine.stats().blocks_encrypted(), 5);
    }

    #[test]
    fn test_algorithm_display() {
        assert_eq!(format!("{}", EncryptionAlgorithm::Aes256Gcm), "AES-256-GCM");
        assert_eq!(
            format!("{}", EncryptionAlgorithm::ChaCha20Poly1305),
            "ChaCha20-Poly1305"
        );
        assert_eq!(format!("{}", EncryptionAlgorithm::None), "None");
    }

    #[test]
    fn test_encrypted_block_fields() {
        let block = EncryptedBlock {
            ciphertext: vec![1, 2, 3],
            nonce: vec![4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
            tag: vec![
                16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31,
            ],
            key_id: "test-key".to_string(),
            algorithm: EncryptionAlgorithm::Aes256Gcm,
            original_size: 100,
        };

        assert_eq!(block.ciphertext(), &[1, 2, 3]);
        assert_eq!(block.nonce().len(), 12);
        assert_eq!(block.tag().len(), 16);
        assert_eq!(block.key_id(), "test-key");
        assert_eq!(block.original_size(), 100);
    }

    #[test]
    fn test_key_rotation_tracks_old_key_id() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        let key1 = EncryptionKey::new(
            "key-1".to_string(),
            EncryptionAlgorithm::Aes256Gcm,
            vec![0x11; 32],
        )
        .unwrap();
        engine.add_key(key1.clone());
        engine.set_current_key(key1.id()).unwrap();

        let key2 = EncryptionKey::new(
            "key-2".to_string(),
            EncryptionAlgorithm::Aes256Gcm,
            vec![0x22; 32],
        )
        .unwrap();
        let old_id = engine.rotate_key(key2).unwrap();
        assert_eq!(old_id, "key-1");
    }

    #[test]
    fn test_enabled_disabled_check() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        assert!(!engine.is_enabled());

        let mut engine_enabled = EncryptionEngine::new(EncryptionConfig {
            enabled: true,
            ..Default::default()
        });
        assert!(engine_enabled.is_enabled());
    }

    #[test]
    fn test_add_and_retrieve_keys() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        assert_eq!(engine.key_count(), 0);

        let key = EncryptionKey::generate_mock(EncryptionAlgorithm::Aes256Gcm);
        let key_id = key.id().to_string();
        engine.add_key(key);

        assert_eq!(engine.key_count(), 1);
        assert!(engine.current_key().is_none());

        engine.set_current_key(&key_id).unwrap();
        assert!(engine.current_key().is_some());
        assert_eq!(engine.current_key().unwrap().id(), key_id);
    }

    #[test]
    fn test_set_current_key_to_nonexistent() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        let result = engine.set_current_key("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_mock_key_correct_length() {
        let key_aes = EncryptionKey::generate_mock(EncryptionAlgorithm::Aes256Gcm);
        assert_eq!(key_aes.key_len(), 32);

        let key_chacha = EncryptionKey::generate_mock(EncryptionAlgorithm::ChaCha20Poly1305);
        assert_eq!(key_chacha.key_len(), 32);

        let key_none = EncryptionKey::generate_mock(EncryptionAlgorithm::None);
        assert_eq!(key_none.key_len(), 0);
    }

    #[test]
    fn test_encryption_engine_new_is_empty() {
        let engine = EncryptionEngine::new(EncryptionConfig::default());
        assert_eq!(engine.key_count(), 0);
        assert!(engine.current_key().is_none());
    }

    #[test]
    fn test_chacha20poly1305_mock_roundtrip() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        let key = EncryptionKey::generate_mock(EncryptionAlgorithm::ChaCha20Poly1305);
        engine.add_key(key.clone());
        engine.set_current_key(key.id()).unwrap();

        let plaintext = b"ChaCha20-Poly1305 test data here";
        let encrypted = engine.encrypt(plaintext).unwrap();
        let decrypted = engine.decrypt(&encrypted).unwrap();

        assert_eq!(encrypted.algorithm(), EncryptionAlgorithm::ChaCha20Poly1305);
        assert_eq!(plaintext.to_vec(), decrypted);
    }

    #[test]
    fn test_encryption_stats_default() {
        let stats = EncryptionStats::default();
        assert_eq!(stats.blocks_encrypted(), 0);
        assert_eq!(stats.blocks_decrypted(), 0);
        assert_eq!(stats.bytes_encrypted(), 0);
        assert_eq!(stats.bytes_decrypted(), 0);
        assert_eq!(stats.key_rotations(), 0);
        assert_eq!(stats.encryption_errors(), 0);
    }

    #[test]
    fn test_encryption_error_increments_stats() {
        let mut engine = EncryptionEngine::new(EncryptionConfig::default());
        // Try to encrypt without a key
        let _ = engine.encrypt(b"test");
        assert_eq!(engine.stats().encryption_errors(), 1);
    }
}
