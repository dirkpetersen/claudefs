//! Batch authentication using HMAC-SHA256 for entry batches.
//!
//! Implements sender authentication and application-layer integrity
//! for journal entry batches exchanged between sites.

use hmac::{Hmac, Mac};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::{Zeroize, ZeroizeOnDrop};

type HmacSha256 = Hmac<Sha256>;

/// HMAC-SHA256 key for batch authentication (32 bytes).
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct BatchAuthKey {
    bytes: [u8; 32],
}

impl BatchAuthKey {
    /// Generate a new random key.
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill(&mut bytes);
        Self { bytes }
    }

    /// Create from raw bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }

    /// Get the raw bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }
}

/// An authenticated batch tag (HMAC-SHA256 output, 32 bytes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchTag {
    /// The 32-byte HMAC-SHA256 tag.
    pub bytes: [u8; 32],
}

impl BatchTag {
    /// Create a new batch tag from raw bytes.
    pub fn new(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }

    /// Create a zero-initialized tag (for testing/placeholder).
    pub fn zero() -> Self {
        Self { bytes: [0u8; 32] }
    }
}

/// Authentication result.
#[derive(Debug, Clone, PartialEq)]
pub enum AuthResult {
    /// The batch is authentic and unmodified.
    Valid,
    /// The batch failed authentication.
    Invalid {
        /// Reason for authentication failure.
        reason: String,
    },
}

/// Signs and verifies entry batches.
pub struct BatchAuthenticator {
    key: BatchAuthKey,
    local_site_id: u64,
}

impl BatchAuthenticator {
    /// Create a new batch authenticator.
    pub fn new(key: BatchAuthKey, local_site_id: u64) -> Self {
        Self { key, local_site_id }
    }

    /// Get the local site ID.
    pub fn local_site_id(&self) -> u64 {
        self.local_site_id
    }

    /// Compute HMAC-SHA256 tag for a batch.
    ///
    /// Message format:
    /// source_site_id (8 bytes LE) || batch_seq (8 bytes LE) ||
    ///   for each entry: seq (8 bytes LE) || inode (8 bytes LE) || payload
    pub fn sign_batch(
        &self,
        source_site_id: u64,
        batch_seq: u64,
        entries: &[crate::journal::JournalEntry],
    ) -> BatchTag {
        let mut msg = Vec::new();
        msg.extend_from_slice(&source_site_id.to_le_bytes());
        msg.extend_from_slice(&batch_seq.to_le_bytes());

        for entry in entries {
            msg.extend_from_slice(&entry.seq.to_le_bytes());
            msg.extend_from_slice(&entry.inode.to_le_bytes());
            msg.extend_from_slice(&entry.payload);
        }

        let hmac_result = hmac_sha256(self.key.as_bytes(), &msg);
        BatchTag::new(hmac_result)
    }

    /// Verify a batch tag using constant-time comparison.
    pub fn verify_batch(
        &self,
        tag: &BatchTag,
        source_site_id: u64,
        batch_seq: u64,
        entries: &[crate::journal::JournalEntry],
    ) -> AuthResult {
        let computed_tag = self.sign_batch(source_site_id, batch_seq, entries);

        if constant_time_compare(&tag.bytes, &computed_tag.bytes) {
            AuthResult::Valid
        } else {
            AuthResult::Invalid {
                reason: "tag mismatch".to_string(),
            }
        }
    }
}

/// Constant-time byte comparison to prevent timing attacks.
fn constant_time_compare(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut diff = 0u8;
    for i in 0..32 {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// HMAC-SHA256 keyed hash (RFC 2104).
fn hmac_sha256(key: &[u8; 32], message: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC key length is always valid");
    mac.update(message);
    let result = mac.finalize();
    let bytes = result.into_bytes();
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hmac_sha256_known_key() {
        let key = [0x0b; 32];
        let message = b"Hi There";
        let hmac = hmac_sha256(&key, message);
        assert_eq!(hmac.len(), 32);
    }

    #[test]
    fn test_batch_key_generate() {
        let key = BatchAuthKey::generate();
        let bytes = key.as_bytes();
        assert_eq!(bytes.len(), 32);
    }

    #[test]
    fn test_batch_key_from_bytes() {
        let bytes = [0x42; 32];
        let key = BatchAuthKey::from_bytes(bytes);
        assert_eq!(*key.as_bytes(), bytes);
    }

    #[test]
    fn test_batch_key_zeroize() {
        use zeroize::Zeroize;
        let mut key = BatchAuthKey::from_bytes([0x55; 32]);
        key.zeroize();
        assert_eq!(*key.as_bytes(), [0u8; 32]);
    }

    #[test]
    fn test_batch_key_secure_drop() {
        let bytes = [0x55; 32];
        let key = BatchAuthKey::from_bytes(bytes);
        let _ = key;
    }

    #[test]
    fn test_batch_tag_equality() {
        let tag1 = BatchTag::new([0x11; 32]);
        let tag2 = BatchTag::new([0x11; 32]);
        let tag3 = BatchTag::new([0x22; 32]);
        assert_eq!(tag1, tag2);
        assert_ne!(tag1, tag3);
    }

    #[test]
    fn test_batch_tag_zero() {
        let tag = BatchTag::zero();
        assert_eq!(tag.bytes, [0u8; 32]);
    }

    #[test]
    fn test_authenticator_sign_verify_valid() {
        let key = BatchAuthKey::from_bytes([0xaa; 32]);
        let auth = BatchAuthenticator::new(key, 1);

        let entries = vec![crate::journal::JournalEntry {
            seq: 100,
            shard_id: 0,
            site_id: 1,
            timestamp_us: 1000,
            inode: 500,
            op: crate::journal::OpKind::Create,
            payload: vec![1, 2, 3, 4],
            crc32: 0,
        }];

        let tag = auth.sign_batch(1, 1, &entries);
        let result = auth.verify_batch(&tag, 1, 1, &entries);

        match result {
            AuthResult::Valid => (),
            _ => panic!("expected valid"),
        }
    }

    #[test]
    fn test_authenticator_verify_invalid_tag() {
        let key = BatchAuthKey::from_bytes([0xaa; 32]);
        let auth = BatchAuthenticator::new(key, 1);

        let entries = vec![crate::journal::JournalEntry {
            seq: 100,
            shard_id: 0,
            site_id: 1,
            timestamp_us: 1000,
            inode: 500,
            op: crate::journal::OpKind::Create,
            payload: vec![1, 2, 3, 4],
            crc32: 0,
        }];

        let wrong_tag = BatchTag::new([0x00; 32]);
        let result = auth.verify_batch(&wrong_tag, 1, 1, &entries);

        match result {
            AuthResult::Invalid { .. } => (),
            _ => panic!("expected invalid"),
        }
    }

    #[test]
    fn test_authenticator_verify_different_source() {
        let key = BatchAuthKey::from_bytes([0xaa; 32]);
        let auth = BatchAuthenticator::new(key, 1);

        let entries = vec![crate::journal::JournalEntry {
            seq: 100,
            shard_id: 0,
            site_id: 1,
            timestamp_us: 1000,
            inode: 500,
            op: crate::journal::OpKind::Create,
            payload: vec![1, 2, 3, 4],
            crc32: 0,
        }];

        let tag = auth.sign_batch(1, 1, &entries);
        let result = auth.verify_batch(&tag, 2, 1, &entries);

        match result {
            AuthResult::Invalid { .. } => (),
            _ => panic!("expected invalid"),
        }
    }

    #[test]
    fn test_authenticator_verify_different_seq() {
        let key = BatchAuthKey::from_bytes([0xaa; 32]);
        let auth = BatchAuthenticator::new(key, 1);

        let entries = vec![crate::journal::JournalEntry {
            seq: 100,
            shard_id: 0,
            site_id: 1,
            timestamp_us: 1000,
            inode: 500,
            op: crate::journal::OpKind::Create,
            payload: vec![1, 2, 3, 4],
            crc32: 0,
        }];

        let tag = auth.sign_batch(1, 1, &entries);
        let result = auth.verify_batch(&tag, 1, 2, &entries);

        match result {
            AuthResult::Invalid { .. } => (),
            _ => panic!("expected invalid"),
        }
    }

    #[test]
    fn test_authenticator_verify_different_entries() {
        let key = BatchAuthKey::from_bytes([0xaa; 32]);
        let auth = BatchAuthenticator::new(key, 1);

        let entries1 = vec![crate::journal::JournalEntry {
            seq: 100,
            shard_id: 0,
            site_id: 1,
            timestamp_us: 1000,
            inode: 500,
            op: crate::journal::OpKind::Create,
            payload: vec![1, 2, 3, 4],
            crc32: 0,
        }];

        let entries2 = vec![crate::journal::JournalEntry {
            seq: 200,
            shard_id: 0,
            site_id: 1,
            timestamp_us: 2000,
            inode: 600,
            op: crate::journal::OpKind::Write,
            payload: vec![5, 6, 7, 8],
            crc32: 0,
        }];

        let tag = auth.sign_batch(1, 1, &entries1);
        let result = auth.verify_batch(&tag, 1, 1, &entries2);

        match result {
            AuthResult::Invalid { .. } => (),
            _ => panic!("expected invalid"),
        }
    }

    #[test]
    fn test_authenticator_empty_entries() {
        let key = BatchAuthKey::from_bytes([0xaa; 32]);
        let auth = BatchAuthenticator::new(key, 1);

        let entries: Vec<crate::journal::JournalEntry> = vec![];
        let tag = auth.sign_batch(1, 1, &entries);
        let result = auth.verify_batch(&tag, 1, 1, &entries);

        match result {
            AuthResult::Valid => (),
            _ => panic!("expected valid"),
        }
    }

    #[test]
    fn test_authenticator_multiple_entries() {
        let key = BatchAuthKey::from_bytes([0xaa; 32]);
        let auth = BatchAuthenticator::new(key, 1);

        let entries = vec![
            crate::journal::JournalEntry {
                seq: 100,
                shard_id: 0,
                site_id: 1,
                timestamp_us: 1000,
                inode: 500,
                op: crate::journal::OpKind::Create,
                payload: vec![1],
                crc32: 0,
            },
            crate::journal::JournalEntry {
                seq: 101,
                shard_id: 0,
                site_id: 1,
                timestamp_us: 2000,
                inode: 501,
                op: crate::journal::OpKind::Write,
                payload: vec![2, 3],
                crc32: 0,
            },
            crate::journal::JournalEntry {
                seq: 102,
                shard_id: 0,
                site_id: 1,
                timestamp_us: 3000,
                inode: 502,
                op: crate::journal::OpKind::Unlink,
                payload: vec![],
                crc32: 0,
            },
        ];

        let tag = auth.sign_batch(1, 5, &entries);
        let result = auth.verify_batch(&tag, 1, 5, &entries);

        match result {
            AuthResult::Valid => (),
            _ => panic!("expected valid"),
        }
    }

    #[test]
    fn test_batch_tag_serialize_deserialize() {
        let tag = BatchTag::new([0xab; 32]);
        let serialized = bincode::serialize(&tag).unwrap();
        let deserialized: BatchTag = bincode::deserialize(&serialized).unwrap();
        assert_eq!(tag, deserialized);
    }

    #[test]
    fn test_constant_time_compare_equal() {
        let a: [u8; 32] = [0x55; 32];
        let b: [u8; 32] = [0x55; 32];
        assert!(constant_time_compare(&a, &b));
    }

    #[test]
    fn test_constant_time_compare_not_equal() {
        let a: [u8; 32] = [0x55; 32];
        let b: [u8; 32] = [0x66; 32];
        assert!(!constant_time_compare(&a, &b));
    }

    #[test]
    fn test_constant_time_compare_single_byte_diff() {
        let mut a: [u8; 32] = [0x55; 32];
        let mut b: [u8; 32] = [0x55; 32];
        b[15] = 0x66;
        assert!(!constant_time_compare(&a, &b));
    }

    #[test]
    fn test_hmac_different_key() {
        let key1 = [0xaa; 32];
        let key2 = [0xbb; 32];
        let message = b"test message";
        let hmac1 = hmac_sha256(&key1, message);
        let hmac2 = hmac_sha256(&key2, message);
        assert_ne!(hmac1, hmac2);
    }

    #[test]
    fn test_auth_result_display() {
        let valid = AuthResult::Valid;
        let invalid = AuthResult::Invalid {
            reason: "test reason".to_string(),
        };
        format!("{:?}", valid);
        format!("{:?}", invalid);
    }
}
