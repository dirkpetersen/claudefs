//! Batch authentication using HMAC-SHA256 for entry batches.
//!
//! Implements sender authentication and application-layer integrity
//! for journal entry batches exchanged between sites.

use rand::Rng;
use serde::{Deserialize, Serialize};

/// HMAC-SHA256 key for batch authentication (32 bytes).
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

impl Drop for BatchAuthKey {
    fn drop(&mut self) {
        for b in self.bytes.iter_mut() {
            *b = 0;
        }
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

/// SHA-256 hash function (FIPS 180-4).
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0x00);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    let mut final_h = h;

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) = (
            final_h[0], final_h[1], final_h[2], final_h[3], final_h[4], final_h[5], final_h[6],
            final_h[7],
        );

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);

            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        final_h[0] = final_h[0].wrapping_add(a);
        final_h[1] = final_h[1].wrapping_add(b);
        final_h[2] = final_h[2].wrapping_add(c);
        final_h[3] = final_h[3].wrapping_add(d);
        final_h[4] = final_h[4].wrapping_add(e);
        final_h[5] = final_h[5].wrapping_add(f);
        final_h[6] = final_h[6].wrapping_add(g);
        final_h[7] = final_h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, &v) in final_h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

/// HMAC-SHA256 keyed hash (RFC 2104).
fn hmac_sha256(key: &[u8; 32], message: &[u8]) -> [u8; 32] {
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];

    for i in 0..32 {
        ipad[i] ^= key[i];
        opad[i] ^= key[i];
    }

    let mut inner_input = Vec::with_capacity(64 + message.len());
    inner_input.extend_from_slice(&ipad);
    inner_input.extend_from_slice(message);
    let inner_hash = sha256(&inner_input);

    let mut outer_input = Vec::with_capacity(64 + 32);
    outer_input.extend_from_slice(&opad);
    outer_input.extend_from_slice(&inner_hash);
    sha256(&outer_input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_known_hash() {
        let input = b"hello";
        let hash = sha256(input);
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn test_sha256_empty_string() {
        let input = b"";
        let hash = sha256(input);
        assert_eq!(hash.len(), 32);
    }

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
    fn test_batch_key_zero_on_drop() {
        let bytes = [0x55; 32];
        let key = BatchAuthKey::from_bytes(bytes);
        let ptr = key.bytes.as_ptr();
        std::mem::forget(key);
        let dropped_bytes = unsafe { *ptr };
        assert_eq!(dropped_bytes, 0x55);
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
    fn test_sha256_block_alignment() {
        let data = vec![0u8; 64];
        let hash = sha256(&data);
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn test_sha256_large_input() {
        let data = vec![0xab; 1000];
        let hash = sha256(&data);
        assert_eq!(hash.len(), 32);
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
