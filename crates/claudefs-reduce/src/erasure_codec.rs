//! Reed-Solomon erasure coding for segment durability.
//!
//! Implements 4+2 and 2+1 stripe configurations per architecture decision D1.

use crate::error::ReduceError;
use reed_solomon_erasure::galois_8::ReedSolomon;
use serde::{Deserialize, Serialize};

/// Erasure coding stripe configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EcStripe {
    /// Number of data shards.
    pub data_shards: usize,
    /// Number of parity shards.
    pub parity_shards: usize,
}

impl EcStripe {
    /// 4 data + 2 parity (default for clusters >= 6 nodes).
    pub const FOUR_TWO: Self = EcStripe {
        data_shards: 4,
        parity_shards: 2,
    };
    /// 2 data + 1 parity (for clusters 3-5 nodes).
    pub const TWO_ONE: Self = EcStripe {
        data_shards: 2,
        parity_shards: 1,
    };

    /// Total number of shards (data + parity).
    pub fn total_shards(&self) -> usize {
        self.data_shards + self.parity_shards
    }
}

/// The output of encoding one segment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncodedSegment {
    /// Segment ID from the original segment.
    pub segment_id: u64,
    /// Stripe configuration used.
    pub stripe: EcStripe,
    /// Bytes per shard (all shards are the same size).
    pub shard_size: usize,
    /// Original payload length before padding.
    pub original_len: usize,
    /// All shards (data + parity), each of size `shard_size`.
    pub shards: Vec<Vec<u8>>,
}

/// Codec that encodes/decodes segment data into Reed-Solomon shards.
pub struct ErasureCodec {
    stripe: EcStripe,
    rs: ReedSolomon,
}

impl ErasureCodec {
    /// Create a new codec with the given stripe configuration.
    pub fn new(stripe: EcStripe) -> Self {
        let rs = ReedSolomon::new(stripe.data_shards, stripe.parity_shards)
            .expect("valid stripe configuration");
        Self { stripe, rs }
    }

    /// Encode `payload` bytes into `stripe.total_shards()` shards.
    ///
    /// `payload` is padded to a multiple of `data_shards` bytes if needed.
    /// Returns `EncodedSegment` with all data+parity shards.
    pub fn encode(&self, segment_id: u64, payload: &[u8]) -> Result<EncodedSegment, ReduceError> {
        let original_len = payload.len();

        let data_shards = self.stripe.data_shards;

        let padded_len = if original_len == 0 {
            data_shards
        } else {
            original_len.div_ceil(data_shards) * data_shards
        };

        let mut padded = payload.to_vec();
        padded.resize(padded_len, 0);

        let shard_size = padded_len / data_shards;

        let mut shards: Vec<Vec<u8>> = padded.chunks(shard_size).map(|c| c.to_vec()).collect();

        for _ in shards.len()..self.stripe.total_shards() {
            shards.push(vec![0u8; shard_size]);
        }

        self.rs
            .encode(&mut shards)
            .map_err(|e| ReduceError::RecoveryFailed(format!("encode failed: {}", e)))?;

        Ok(EncodedSegment {
            segment_id,
            stripe: self.stripe,
            shard_size,
            original_len,
            shards,
        })
    }

    /// Decode back to the original payload from an `EncodedSegment`.
    ///
    /// All shards must be present (no erasures). Verifies parity.
    pub fn decode(&self, encoded: &EncodedSegment) -> Result<Vec<u8>, ReduceError> {
        if encoded.shards.len() != self.stripe.total_shards() {
            return Err(ReduceError::ShardCountMismatch {
                expected: self.stripe.total_shards(),
                got: encoded.shards.len(),
            });
        }

        let shards = encoded.shards.clone();

        if !self
            .rs
            .verify(&shards)
            .map_err(|e| ReduceError::RecoveryFailed(format!("verify failed: {}", e)))?
        {
            return Err(ReduceError::RecoveryFailed(
                "parity verification failed".to_string(),
            ));
        }

        let mut payload = Vec::with_capacity(encoded.original_len);
        for shard in &shards[..self.stripe.data_shards] {
            payload.extend_from_slice(shard);
        }
        payload.truncate(encoded.original_len);
        Ok(payload)
    }

    /// Reconstruct missing shards.
    ///
    /// `shards` is a Vec of `Option<Vec<u8>>` — `None` means that shard is missing/corrupt.
    /// Modifies `shards` in-place, filling in the missing shards.
    ///
    /// Returns `Ok(())` if reconstruction succeeded.
    /// Fails if too many shards are missing (more than `parity_shards`).
    pub fn reconstruct(
        &self,
        shards: &mut [Option<Vec<u8>>],
        shard_size: usize,
    ) -> Result<(), ReduceError> {
        if shards.len() != self.stripe.total_shards() {
            return Err(ReduceError::ShardCountMismatch {
                expected: self.stripe.total_shards(),
                got: shards.len(),
            });
        }

        let missing_count = shards.iter().filter(|s| s.is_none()).count();
        if missing_count > self.stripe.parity_shards {
            return Err(ReduceError::RecoveryFailed(format!(
                "too many missing shards: {} > {}",
                missing_count, self.stripe.parity_shards
            )));
        }

        self.rs
            .reconstruct(shards)
            .map_err(|e| ReduceError::RecoveryFailed(format!("reconstruct failed: {}", e)))?;

        for s in shards.iter().flatten() {
            if s.len() != shard_size {
                return Err(ReduceError::InvalidInput(format!(
                    "shard size mismatch: expected {}, got {}",
                    shard_size,
                    s.len()
                )));
            }
        }

        Ok(())
    }

    /// Extract original payload bytes from a reconstructed full shard set.
    ///
    /// `shards` must all be `Some(_)` after calling `reconstruct`.
    /// `original_len` is the byte count before padding was applied.
    pub fn extract_payload(
        &self,
        shards: &[Option<Vec<u8>>],
        original_len: usize,
    ) -> Result<Vec<u8>, ReduceError> {
        if shards.len() != self.stripe.total_shards() {
            return Err(ReduceError::ShardCountMismatch {
                expected: self.stripe.total_shards(),
                got: shards.len(),
            });
        }

        let mut payload = Vec::with_capacity(original_len);
        for shard in &shards[..self.stripe.data_shards] {
            match shard {
                Some(data) => payload.extend_from_slice(data),
                None => {
                    return Err(ReduceError::RecoveryFailed(
                        "missing data shard after reconstruction".to_string(),
                    ))
                }
            }
        }
        payload.truncate(original_len);
        Ok(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_codec_four_two() {
        let codec = ErasureCodec::new(EcStripe::FOUR_TWO);
        assert_eq!(codec.stripe, EcStripe::FOUR_TWO);
    }

    #[test]
    fn test_new_codec_two_one() {
        let codec = ErasureCodec::new(EcStripe::TWO_ONE);
        assert_eq!(codec.stripe, EcStripe::TWO_ONE);
    }

    #[test]
    fn test_encode_decode_roundtrip_4_2() {
        let codec = ErasureCodec::new(EcStripe::FOUR_TWO);
        let payload = b"hello world, this is a test payload for 4+2 erasure coding";

        let encoded = codec.encode(42, payload).expect("encode should succeed");
        let decoded = codec.decode(&encoded).expect("decode should succeed");

        assert_eq!(decoded.as_slice(), payload);
    }

    #[test]
    fn test_encode_decode_roundtrip_2_1() {
        let codec = ErasureCodec::new(EcStripe::TWO_ONE);
        let payload = b"hello world, this is a test payload for 2+1 erasure coding";

        let encoded = codec.encode(42, payload).expect("encode should succeed");
        let decoded = codec.decode(&encoded).expect("decode should succeed");

        assert_eq!(decoded.as_slice(), payload);
    }

    #[test]
    fn test_encode_empty_payload() {
        let codec = ErasureCodec::new(EcStripe::FOUR_TWO);
        let encoded = codec.encode(1, b"").expect("encode empty should work");
        assert_eq!(encoded.original_len, 0);
        assert_eq!(encoded.shards.len(), 6);
    }

    #[test]
    fn test_encode_large_payload() {
        let codec = ErasureCodec::new(EcStripe::FOUR_TWO);
        let payload: Vec<u8> = (0..4 * 1024 * 1024).map(|i| (i % 256) as u8).collect();

        let encoded = codec.encode(1, &payload).expect("encode large should work");
        let decoded = codec.decode(&encoded).expect("decode large should work");

        assert_eq!(decoded.as_slice(), payload.as_slice());
    }

    #[test]
    fn test_encode_odd_size_payload() {
        let codec = ErasureCodec::new(EcStripe::FOUR_TWO);
        let payload: Vec<u8> = (0..17).map(|i| (i % 256) as u8).collect();

        let encoded = codec.encode(1, &payload).expect("encode odd should work");
        assert!(encoded.shard_size * encoded.stripe.data_shards >= payload.len());

        let decoded = codec.decode(&encoded).expect("decode should work");
        assert_eq!(decoded.as_slice(), payload.as_slice());
    }

    #[test]
    fn test_shard_count() {
        let codec = ErasureCodec::new(EcStripe::FOUR_TWO);
        let encoded = codec.encode(1, b"test").expect("encode should work");
        assert_eq!(encoded.shards.len(), 6);
    }

    #[test]
    fn test_shard_sizes_equal() {
        let codec = ErasureCodec::new(EcStripe::FOUR_TWO);
        let payload: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();

        let encoded = codec.encode(1, &payload).expect("encode should work");
        let shard_size = encoded.shard_size;
        for shard in &encoded.shards {
            assert_eq!(shard.len(), shard_size);
        }
    }

    #[test]
    fn test_reconstruct_one_missing_data_shard() {
        let codec = ErasureCodec::new(EcStripe::FOUR_TWO);
        let payload = b"test data for reconstructing one missing shard";

        let encoded = codec.encode(1, payload).expect("encode should work");

        let mut shards: Vec<Option<Vec<u8>>> =
            encoded.shards.iter().map(|s| Some(s.clone())).collect();
        shards[1] = None;

        codec
            .reconstruct(&mut shards, encoded.shard_size)
            .expect("reconstruct should work");

        assert!(shards[1].is_some());

        let extracted = codec
            .extract_payload(&shards, encoded.original_len)
            .expect("extract should work");
        assert_eq!(extracted.as_slice(), payload);
    }

    #[test]
    fn test_reconstruct_one_missing_parity_shard() {
        let codec = ErasureCodec::new(EcStripe::FOUR_TWO);
        let payload = b"test data for reconstructing missing parity";

        let encoded = codec.encode(1, payload).expect("encode should work");

        let mut shards: Vec<Option<Vec<u8>>> =
            encoded.shards.iter().map(|s| Some(s.clone())).collect();
        shards[5] = None;

        codec
            .reconstruct(&mut shards, encoded.shard_size)
            .expect("reconstruct should work");

        assert!(shards[5].is_some());

        let extracted = codec
            .extract_payload(&shards, encoded.original_len)
            .expect("extract should work");
        assert_eq!(extracted.as_slice(), payload);
    }

    #[test]
    fn test_reconstruct_two_missing_shards_4_2() {
        let codec = ErasureCodec::new(EcStripe::FOUR_TWO);
        let payload = b"test data for reconstructing two missing shards";

        let encoded = codec.encode(1, payload).expect("encode should work");

        let mut shards: Vec<Option<Vec<u8>>> =
            encoded.shards.iter().map(|s| Some(s.clone())).collect();
        shards[1] = None;
        shards[4] = None;

        codec
            .reconstruct(&mut shards, encoded.shard_size)
            .expect("reconstruct should work");

        assert!(shards[1].is_some());
        assert!(shards[4].is_some());

        let extracted = codec
            .extract_payload(&shards, encoded.original_len)
            .expect("extract should work");
        assert_eq!(extracted.as_slice(), payload);
    }

    #[test]
    fn test_reconstruct_too_many_missing() {
        let codec = ErasureCodec::new(EcStripe::FOUR_TWO);
        let payload = b"test data for too many missing shards";

        let encoded = codec.encode(1, payload).expect("encode should work");

        let mut shards: Vec<Option<Vec<u8>>> =
            encoded.shards.iter().map(|s| Some(s.clone())).collect();
        shards[0] = None;
        shards[1] = None;
        shards[2] = None;

        let result = codec.reconstruct(&mut shards, encoded.shard_size);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ReduceError::RecoveryFailed(_)
        ));
    }

    #[test]
    fn test_decode_verifies_parity() {
        let codec = ErasureCodec::new(EcStripe::FOUR_TWO);
        let payload = b"test parity verification";

        let encoded = codec.encode(1, payload).expect("encode should work");

        let mut corrupted = encoded.clone();
        if corrupted.shards[4].len() > 0 {
            corrupted.shards[4][0] ^= 0xFF;
        }

        let result = codec.decode(&corrupted);
        assert!(result.is_err());
    }

    #[test]
    fn test_segment_id_preserved() {
        let codec = ErasureCodec::new(EcStripe::FOUR_TWO);
        let encoded = codec.encode(12345, b"test").expect("encode should work");
        assert_eq!(encoded.segment_id, 12345);

        let decoded = codec.decode(&encoded).expect("decode should work");
        assert_eq!(decoded.as_slice(), b"test");
    }

    #[test]
    fn test_ec_stripe_constants() {
        assert_eq!(EcStripe::FOUR_TWO.data_shards, 4);
        assert_eq!(EcStripe::FOUR_TWO.parity_shards, 2);
        assert_eq!(EcStripe::TWO_ONE.data_shards, 2);
        assert_eq!(EcStripe::TWO_ONE.parity_shards, 1);
    }

    #[test]
    fn test_ec_stripe_total_shards() {
        assert_eq!(EcStripe::FOUR_TWO.total_shards(), 6);
        assert_eq!(EcStripe::TWO_ONE.total_shards(), 3);
    }
}
