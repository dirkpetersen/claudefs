use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use tracing::{debug, info};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EcProfile {
    pub data_shards: u8,
    pub parity_shards: u8,
}

impl EcProfile {
    pub fn ec_4_2() -> Self {
        Self {
            data_shards: 4,
            parity_shards: 2,
        }
    }

    pub fn ec_2_1() -> Self {
        Self {
            data_shards: 2,
            parity_shards: 1,
        }
    }

    pub fn total_shards(&self) -> u8 {
        self.data_shards + self.parity_shards
    }

    pub fn storage_overhead(&self) -> f64 {
        let total = self.data_shards as f64 + self.parity_shards as f64;
        total / self.data_shards as f64
    }

    pub fn can_tolerate_failures(&self) -> u8 {
        self.parity_shards
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EcShard {
    pub shard_index: u8,
    pub is_parity: bool,
    pub data: Vec<u8>,
    pub checksum: u64,
    pub segment_id: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StripeState {
    Encoding,
    Distributed,
    Degraded { missing_shards: Vec<u8> },
    Reconstructing,
    Failed { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EcStripe {
    pub segment_id: u64,
    pub profile: EcProfile,
    pub shards: Vec<Option<EcShard>>,
    pub state: StripeState,
    pub created_at: u64,
    pub shard_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EcConfig {
    pub default_profile: EcProfile,
    pub segment_size: usize,
    pub verify_on_read: bool,
    pub background_verify_interval_secs: u64,
    pub max_concurrent_reconstructions: u32,
}

impl Default for EcConfig {
    fn default() -> Self {
        Self {
            default_profile: EcProfile::ec_4_2(),
            segment_size: 2 * 1024 * 1024,
            verify_on_read: true,
            background_verify_interval_secs: 3600,
            max_concurrent_reconstructions: 4,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EcStats {
    pub stripes_encoded: u64,
    pub stripes_decoded: u64,
    pub shards_created: u64,
    pub reconstructions: u64,
    pub reconstruction_failures: u64,
    pub bytes_encoded: u64,
    pub bytes_decoded: u64,
    pub verify_successes: u64,
    pub verify_failures: u64,
}

#[derive(Debug, Clone, Error)]
pub enum EcError {
    #[error("Invalid data size: expected {expected}, got {actual}")]
    InvalidDataSize { expected: usize, actual: usize },
    #[error("Too many missing shards: need {needed}, have {available}")]
    TooManyMissing { needed: u8, available: u8 },
    #[error("Shard index out of range: {index} >= {total}")]
    ShardIndexOutOfRange { index: u8, total: u8 },
    #[error("Stripe not found for segment {0}")]
    StripeNotFound(u64),
    #[error("Checksum mismatch on shard {shard_index}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        shard_index: u8,
        expected: u64,
        actual: u64,
    },
    #[error("Encoding failed: {0}")]
    EncodingFailed(String),
}

fn simple_checksum(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in data {
        hash = hash.wrapping_mul(0x100000001b3).wrapping_add(byte as u64);
    }
    hash
}

pub struct ErasureCodingEngine {
    config: EcConfig,
    stripes: HashMap<u64, EcStripe>,
    stats: EcStats,
}

impl ErasureCodingEngine {
    pub fn new(config: EcConfig) -> Self {
        info!(
            "Initializing EC engine with profile: {}",
            config.default_profile.data_shards + config.default_profile.parity_shards
        );
        Self {
            config,
            stripes: HashMap::new(),
            stats: EcStats::default(),
        }
    }

    pub fn encode_segment(&mut self, segment_id: u64, data: &[u8]) -> Result<EcStripe, EcError> {
        let profile = self.config.default_profile;
        let total_shards = profile.total_shards() as usize;

        if data.is_empty() {
            return Err(EcError::EncodingFailed(
                "Empty data not allowed".to_string(),
            ));
        }

        let shard_size =
            (data.len() + profile.data_shards as usize - 1) / profile.data_shards as usize;
        let padded_size = shard_size * profile.data_shards as usize;

        let mut padded_data = data.to_vec();
        if padded_data.len() < padded_size {
            padded_data.resize(padded_size, 0);
        }

        let mut data_shards: Vec<Vec<u8>> = Vec::with_capacity(profile.data_shards as usize);
        for i in 0..profile.data_shards as usize {
            let start = i * shard_size;
            let end = start + shard_size;
            data_shards.push(padded_data[start..end].to_vec());
        }

        let mut parity_shards: Vec<Vec<u8>> = Vec::with_capacity(profile.parity_shards as usize);
        for p in 0..profile.parity_shards as usize {
            let mut parity = vec![0u8; shard_size];
            for i in 0..shard_size {
                let mut byte: u8 = 0;
                for d in 0..profile.data_shards as usize {
                    let rotate = if p == 0 { 0u32 } else { d as u32 };
                    let src_byte = data_shards[d].get(i).copied().unwrap_or(0);
                    byte ^= src_byte.rotate_right(rotate);
                }
                parity[i] = byte;
            }
            parity_shards.push(parity);
        }

        let mut shards: Vec<Option<EcShard>> = Vec::with_capacity(total_shards);

        for i in 0..profile.data_shards as usize {
            let checksum = simple_checksum(&data_shards[i]);
            shards.push(Some(EcShard {
                shard_index: i as u8,
                is_parity: false,
                data: data_shards[i].clone(),
                checksum,
                segment_id,
            }));
            self.stats.shards_created += 1;
        }

        for i in 0..profile.parity_shards as usize {
            let idx = profile.data_shards as usize + i;
            let checksum = simple_checksum(&parity_shards[i]);
            shards.push(Some(EcShard {
                shard_index: idx as u8,
                is_parity: true,
                data: parity_shards[i].clone(),
                checksum,
                segment_id,
            }));
            self.stats.shards_created += 1;
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let stripe = EcStripe {
            segment_id,
            profile,
            shards,
            state: StripeState::Distributed,
            created_at: now,
            shard_size,
        };

        self.stats.stripes_encoded += 1;
        self.stats.bytes_encoded += data.len() as u64;

        debug!(
            "Encoded segment {} into {} shards",
            segment_id,
            profile.total_shards()
        );

        Ok(stripe)
    }

    pub fn decode_stripe(&mut self, stripe: &EcStripe) -> Result<Vec<u8>, EcError> {
        let profile = stripe.profile;
        let mut available_count = 0usize;

        for (i, shard_opt) in stripe.shards.iter().enumerate() {
            if let Some(shard) = shard_opt {
                if !shard.is_parity && i < profile.data_shards as usize {
                    available_count += 1;
                }
            }
        }

        if available_count < profile.data_shards as usize {
            return Err(EcError::TooManyMissing {
                needed: profile.data_shards,
                available: available_count as u8,
            });
        }

        let shard_size = stripe.shard_size;
        let mut reconstructed: Vec<Vec<u8>> = Vec::with_capacity(profile.data_shards as usize);

        for i in 0..profile.data_shards as usize {
            if let Some(shard) = stripe.shards[i].as_ref() {
                reconstructed.push(shard.data.clone());
            } else {
                let mut recovered = vec![0u8; shard_size];
                let mut found_parity = false;

                for p in 0..profile.parity_shards as usize {
                    let parity_idx = profile.data_shards as usize + p;
                    if stripe.shards[parity_idx].is_some() {
                        if p == 0 {
                            for byte_idx in 0..shard_size {
                                let mut byte: u8 = 0;
                                for d in 0..profile.data_shards as usize {
                                    if let Some(ds) = stripe.shards[d].as_ref() {
                                        byte ^= ds.data.get(byte_idx).copied().unwrap_or(0);
                                    }
                                }
                                recovered[byte_idx] = byte;
                            }
                            found_parity = true;
                            break;
                        }
                    }
                }

                if !found_parity {
                    return Err(EcError::TooManyMissing {
                        needed: 1,
                        available: 0,
                    });
                }

                reconstructed.push(recovered);
            }
        }

        let mut result: Vec<u8> = Vec::new();
        for shard_data in reconstructed {
            result.extend(shard_data);
        }

        self.stats.stripes_decoded += 1;
        self.stats.bytes_decoded += result.len() as u64;

        debug!("Decoded stripe for segment {}", stripe.segment_id);
        Ok(result)
    }

    pub fn reconstruct_shard(
        &mut self,
        stripe: &mut EcStripe,
        missing_index: u8,
    ) -> Result<EcShard, EcError> {
        let profile = stripe.profile;

        if missing_index >= profile.total_shards() {
            return Err(EcError::ShardIndexOutOfRange {
                index: missing_index,
                total: profile.total_shards(),
            });
        }

        if stripe.shards[missing_index as usize].is_some() {
            return Err(EcError::EncodingFailed(format!(
                "Shard {} is not missing",
                missing_index
            )));
        }

        let is_parity = missing_index >= profile.data_shards;
        let shard_size = stripe.shard_size;
        let mut recovered_data = vec![0u8; shard_size];

        if is_parity {
            let parity_idx = missing_index - profile.data_shards;
            for byte_idx in 0..shard_size {
                let mut byte: u8 = 0;
                for d in 0..profile.data_shards as usize {
                    if let Some(ds) = stripe.shards[d].as_ref() {
                        let rotate = if parity_idx == 0 { 0u32 } else { d as u32 };
                        byte ^= ds
                            .data
                            .get(byte_idx)
                            .copied()
                            .unwrap_or(0)
                            .rotate_right(rotate);
                    }
                }
                recovered_data[byte_idx] = byte;
            }
        } else {
            for byte_idx in 0..shard_size {
                let mut byte: u8 = 0;
                for d in 0..profile.data_shards as usize {
                    if d != missing_index as usize {
                        if let Some(ds) = stripe.shards[d].as_ref() {
                            byte ^= ds.data.get(byte_idx).copied().unwrap_or(0);
                        }
                    }
                }
                for p in 0..profile.parity_shards as usize {
                    let parity_idx = profile.data_shards as usize + p;
                    if p == 0 {
                        if let Some(ps) = stripe.shards[parity_idx].as_ref() {
                            byte ^= ps.data.get(byte_idx).copied().unwrap_or(0);
                        }
                    }
                }
                recovered_data[byte_idx] = byte;
            }
        }

        let checksum = simple_checksum(&recovered_data);
        let shard = EcShard {
            shard_index: missing_index,
            is_parity,
            data: recovered_data,
            checksum,
            segment_id: stripe.segment_id,
        };

        stripe.shards[missing_index as usize] = Some(shard.clone());

        if let StripeState::Degraded { missing_shards } = &mut stripe.state {
            missing_shards.retain(|&x| x != missing_index);
            if missing_shards.is_empty() {
                stripe.state = StripeState::Distributed;
            }
        }

        self.stats.reconstructions += 1;
        debug!(
            "Reconstructed shard {} for segment {}",
            missing_index, stripe.segment_id
        );

        Ok(shard)
    }

    pub fn verify_stripe(&mut self, stripe: &EcStripe) -> Result<bool, EcError> {
        for (i, shard_opt) in stripe.shards.iter().enumerate() {
            if let Some(shard) = shard_opt {
                let computed = simple_checksum(&shard.data);
                if computed != shard.checksum {
                    self.stats.verify_failures += 1;
                    return Err(EcError::ChecksumMismatch {
                        shard_index: i as u8,
                        expected: shard.checksum,
                        actual: computed,
                    });
                }
            }
        }

        self.stats.verify_successes += 1;
        debug!("Stripe {} verified successfully", stripe.segment_id);
        Ok(true)
    }

    pub fn register_stripe(&mut self, stripe: EcStripe) {
        self.stripes.insert(stripe.segment_id, stripe);
    }

    pub fn get_stripe(&self, segment_id: u64) -> Option<&EcStripe> {
        self.stripes.get(&segment_id)
    }

    pub fn get_stripe_mut(&mut self, segment_id: u64) -> Option<&mut EcStripe> {
        self.stripes.get_mut(&segment_id)
    }

    pub fn mark_shard_missing(&mut self, segment_id: u64, shard_index: u8) -> Result<(), EcError> {
        let stripe = self
            .stripes
            .get_mut(&segment_id)
            .ok_or(EcError::StripeNotFound(segment_id))?;

        let profile = stripe.profile;
        if shard_index >= profile.total_shards() {
            return Err(EcError::ShardIndexOutOfRange {
                index: shard_index,
                total: profile.total_shards(),
            });
        }

        if stripe.shards[shard_index as usize].is_none() {
            return Err(EcError::EncodingFailed(format!(
                "Shard {} already missing",
                shard_index
            )));
        }

        stripe.shards[shard_index as usize] = None;

        if let StripeState::Degraded { missing_shards } = &mut stripe.state {
            missing_shards.push(shard_index);
        } else {
            stripe.state = StripeState::Degraded {
                missing_shards: vec![shard_index],
            };
        }

        debug!(
            "Marked shard {} missing for segment {}",
            shard_index, segment_id
        );
        Ok(())
    }

    pub fn remove_stripe(&mut self, segment_id: u64) -> Option<EcStripe> {
        self.stripes.remove(&segment_id)
    }

    pub fn degraded_stripes(&self) -> Vec<u64> {
        self.stripes
            .iter()
            .filter(|(_, stripe)| matches!(stripe.state, StripeState::Degraded { .. }))
            .map(|(&segment_id, _)| segment_id)
            .collect()
    }

    pub fn stats(&self) -> &EcStats {
        &self.stats
    }

    pub fn stripe_count(&self) -> usize {
        self.stripes.len()
    }

    pub fn reconstruct_shard_by_id(
        &mut self,
        segment_id: u64,
        missing_index: u8,
    ) -> Result<EcShard, EcError> {
        let mut stripe = self
            .stripes
            .remove(&segment_id)
            .ok_or(EcError::StripeNotFound(segment_id))?;
        let result = self.reconstruct_shard(&mut stripe, missing_index);
        self.stripes.insert(segment_id, stripe);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ec_profile_4_2() {
        let profile = EcProfile::ec_4_2();
        assert_eq!(profile.data_shards, 4);
        assert_eq!(profile.parity_shards, 2);
    }

    #[test]
    fn test_ec_profile_2_1() {
        let profile = EcProfile::ec_2_1();
        assert_eq!(profile.data_shards, 2);
        assert_eq!(profile.parity_shards, 1);
    }

    #[test]
    fn test_ec_profile_total_shards() {
        let profile_4_2 = EcProfile::ec_4_2();
        assert_eq!(profile_4_2.total_shards(), 6);
        let profile_2_1 = EcProfile::ec_2_1();
        assert_eq!(profile_2_1.total_shards(), 3);
    }

    #[test]
    fn test_ec_profile_storage_overhead() {
        let profile_4_2 = EcProfile::ec_4_2();
        assert!((profile_4_2.storage_overhead() - 1.5).abs() < 0.001);
        let profile_2_1 = EcProfile::ec_2_1();
        assert!((profile_2_1.storage_overhead() - 1.5).abs() < 0.001);
    }

    #[test]
    fn test_ec_profile_can_tolerate_failures() {
        let profile_4_2 = EcProfile::ec_4_2();
        assert_eq!(profile_4_2.can_tolerate_failures(), 2);
        let profile_2_1 = EcProfile::ec_2_1();
        assert_eq!(profile_2_1.can_tolerate_failures(), 1);
    }

    #[test]
    fn test_encode_segment_basic() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);
        let data = vec![1u8; 1024];
        let stripe = engine.encode_segment(1, &data).unwrap();
        assert_eq!(stripe.segment_id, 1);
        assert_eq!(stripe.shards.len(), 6);
    }

    #[test]
    fn test_encode_segment_4_2_shard_count() {
        let config = EcConfig {
            default_profile: EcProfile::ec_4_2(),
            ..Default::default()
        };
        let mut engine = ErasureCodingEngine::new(config);
        let data = vec![0u8; 8000];
        let stripe = engine.encode_segment(1, &data).unwrap();
        assert_eq!(stripe.shards.len(), 6);
        let data_shards: Vec<_> = stripe
            .shards
            .iter()
            .filter_map(|s| s.as_ref())
            .filter(|s| !s.is_parity)
            .collect();
        assert_eq!(data_shards.len(), 4);
    }

    #[test]
    fn test_encode_segment_2_1_shard_count() {
        let config = EcConfig {
            default_profile: EcProfile::ec_2_1(),
            ..Default::default()
        };
        let mut engine = ErasureCodingEngine::new(config);
        let data = vec![0u8; 1024];
        let stripe = engine.encode_segment(1, &data).unwrap();
        assert_eq!(stripe.shards.len(), 3);
    }

    #[test]
    fn test_encode_produces_parity() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);
        let data = vec![0xFFu8; 100];
        let stripe = engine.encode_segment(1, &data).unwrap();
        let parity_shards: Vec<_> = stripe
            .shards
            .iter()
            .filter_map(|s| s.as_ref())
            .filter(|s| s.is_parity)
            .collect();
        assert_eq!(parity_shards.len(), 2);
    }

    #[test]
    fn test_decode_stripe_all_shards_present() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);
        let data = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        let stripe = engine.encode_segment(1, &data).unwrap();
        let decoded = engine.decode_stripe(&stripe).unwrap();
        let decoded_trimmed = &decoded[..data.len()];
        assert_eq!(decoded_trimmed, data.as_slice());
    }

    #[test]
    fn test_decode_stripe_data_matches_original() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);
        let original: Vec<u8> = (0..100).map(|i| i as u8).collect();
        let stripe = engine.encode_segment(1, &original).unwrap();
        let decoded = engine.decode_stripe(&stripe).unwrap();
        assert_eq!(&decoded[..original.len()], original.as_slice());
    }

    #[test]
    fn test_decode_with_missing_parity_succeeds() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);
        let data = vec![1u8; 100];
        let stripe = engine.encode_segment(1, &data).unwrap();
        let mut stripe_missing = stripe;
        stripe_missing.shards[4] = None;
        let decoded = engine.decode_stripe(&stripe_missing);
        assert!(decoded.is_ok());
    }

    #[test]
    fn test_decode_too_many_missing_fails() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);
        let data = vec![1u8; 100];
        let stripe = engine.encode_segment(1, &data).unwrap();
        let mut stripe_missing = stripe;
        stripe_missing.shards[0] = None;
        stripe_missing.shards[1] = None;
        stripe_missing.shards[4] = None;
        stripe_missing.shards[5] = None;
        let result = engine.decode_stripe(&stripe_missing);
        assert!(matches!(result, Err(EcError::TooManyMissing { .. })));
    }

    #[test]
    fn test_reconstruct_missing_data_shard() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);
        let data: Vec<u8> = (0..100).map(|i| i as u8).collect();
        let stripe = engine.encode_segment(1, &data).unwrap();
        let mut stripe_missing = stripe;
        stripe_missing.shards[0] = None;
        stripe_missing.shards[4] = None;
        stripe_missing.shards[5] = None;

        if let StripeState::Degraded { missing_shards } = &mut stripe_missing.state {
            missing_shards.push(0);
            missing_shards.push(4);
            missing_shards.push(5);
        } else {
            stripe_missing.state = StripeState::Degraded {
                missing_shards: vec![0, 4, 5],
            };
        }

        engine.register_stripe(stripe_missing);

        let result = engine.reconstruct_shard_by_id(1, 0);
        assert!(result.is_ok());

        let stripe_after = engine.get_stripe(1).unwrap();
        assert!(stripe_after.shards[0].is_some());
    }

    #[test]
    fn test_reconstruct_missing_parity_shard() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);
        let data: Vec<u8> = (0..100).map(|i| i as u8).collect();
        let stripe = engine.encode_segment(1, &data).unwrap();
        let mut stripe_missing = stripe;
        stripe_missing.shards[4] = None;

        if let StripeState::Degraded { missing_shards } = &mut stripe_missing.state {
            missing_shards.push(4);
        } else {
            stripe_missing.state = StripeState::Degraded {
                missing_shards: vec![4],
            };
        }

        engine.register_stripe(stripe_missing);

        let result = engine.reconstruct_shard_by_id(1, 4);
        assert!(result.is_ok());

        let stripe_after = engine.get_stripe(1).unwrap();
        assert!(stripe_after.shards[4].is_some());
    }

    #[test]
    fn test_reconstruct_nonexistent_fails() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);
        let data = vec![1u8; 100];
        let stripe = engine.encode_segment(1, &data).unwrap();
        engine.register_stripe(stripe);

        let result = engine.reconstruct_shard_by_id(1, 5);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_stripe_valid() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);
        let data = vec![1u8; 100];
        let stripe = engine.encode_segment(1, &data).unwrap();
        let result = engine.verify_stripe(&stripe);
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_verify_stripe_corrupted_checksum() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);
        let data = vec![1u8; 100];
        let mut stripe = engine.encode_segment(1, &data).unwrap();

        if let Some(shard) = stripe.shards[0].as_mut() {
            shard.data[0] = 0xFF;
        }

        let result = engine.verify_stripe(&stripe);
        assert!(matches!(result, Err(EcError::ChecksumMismatch { .. })));
    }

    #[test]
    fn test_register_and_get_stripe() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);
        let data = vec![1u8; 100];
        let stripe = engine.encode_segment(1, &data).unwrap();
        engine.register_stripe(stripe);

        let retrieved = engine.get_stripe(1);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().segment_id, 1);
    }

    #[test]
    fn test_remove_stripe() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);
        let data = vec![1u8; 100];
        let stripe = engine.encode_segment(1, &data).unwrap();
        engine.register_stripe(stripe);

        let removed = engine.remove_stripe(1);
        assert!(removed.is_some());

        let retrieved = engine.get_stripe(1);
        assert!(retrieved.is_none());
    }

    #[test]
    fn test_mark_shard_missing() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);
        let data = vec![1u8; 100];
        let stripe = engine.encode_segment(1, &data).unwrap();
        engine.register_stripe(stripe);

        engine.mark_shard_missing(1, 0).unwrap();

        let stripe = engine.get_stripe(1).unwrap();
        assert!(stripe.shards[0].is_none());
    }

    #[test]
    fn test_mark_shard_missing_updates_state() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);
        let data = vec![1u8; 100];
        let stripe = engine.encode_segment(1, &data).unwrap();
        engine.register_stripe(stripe);

        engine.mark_shard_missing(1, 0).unwrap();

        let stripe = engine.get_stripe(1).unwrap();
        assert!(matches!(stripe.state, StripeState::Degraded { .. }));
    }

    #[test]
    fn test_degraded_stripes_list() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);

        let data = vec![1u8; 100];
        let stripe = engine.encode_segment(1, &data).unwrap();
        engine.register_stripe(stripe);

        let data = vec![2u8; 100];
        let stripe = engine.encode_segment(2, &data).unwrap();
        engine.register_stripe(stripe);

        let data = vec![3u8; 100];
        let stripe = engine.encode_segment(3, &data).unwrap();
        engine.register_stripe(stripe);

        engine.mark_shard_missing(1, 0).unwrap();
        engine.mark_shard_missing(3, 1).unwrap();

        let degraded = engine.degraded_stripes();
        assert_eq!(degraded.len(), 2);
        assert!(degraded.contains(&1));
        assert!(degraded.contains(&3));
    }

    #[test]
    fn test_stats_after_encode() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);

        let data = vec![1u8; 100];
        engine.encode_segment(1, &data).unwrap();

        let stats = engine.stats();
        assert_eq!(stats.stripes_encoded, 1);
        assert_eq!(stats.bytes_encoded, 100);
    }

    #[test]
    fn test_stats_after_decode() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);

        let data = vec![1u8; 100];
        let stripe = engine.encode_segment(1, &data).unwrap();
        engine.decode_stripe(&stripe).unwrap();

        let stats = engine.stats();
        assert_eq!(stats.stripes_decoded, 1);
    }

    #[test]
    fn test_stats_after_reconstruct() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);

        let data = vec![1u8; 100];
        let stripe = engine.encode_segment(1, &data).unwrap();
        let mut stripe_missing = stripe;
        stripe_missing.shards[0] = None;

        if let StripeState::Degraded { missing_shards } = &mut stripe_missing.state {
            missing_shards.push(0);
        } else {
            stripe_missing.state = StripeState::Degraded {
                missing_shards: vec![0],
            };
        }

        engine.register_stripe(stripe_missing);
        engine.reconstruct_shard_by_id(1, 0).unwrap();

        let stats = engine.stats();
        assert_eq!(stats.reconstructions, 1);
    }

    #[test]
    fn test_encode_empty_data_fails() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);

        let result = engine.encode_segment(1, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_encode_small_data_padded() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);

        let data = vec![1u8; 5];
        let stripe = engine.encode_segment(1, &data).unwrap();

        assert_eq!(stripe.shard_size, 2);

        let decoded = engine.decode_stripe(&stripe).unwrap();
        assert_eq!(&decoded[..5], data.as_slice());
    }

    #[test]
    fn test_ec_config_default() {
        let config = EcConfig::default();
        assert_eq!(config.default_profile.data_shards, 4);
        assert_eq!(config.default_profile.parity_shards, 2);
        assert_eq!(config.segment_size, 2 * 1024 * 1024);
    }

    #[test]
    fn test_stripe_count() {
        let config = EcConfig::default();
        let mut engine = ErasureCodingEngine::new(config);

        assert_eq!(engine.stripe_count(), 0);

        let data = vec![1u8; 100];
        let stripe = engine.encode_segment(1, &data).unwrap();
        engine.register_stripe(stripe);

        assert_eq!(engine.stripe_count(), 1);

        let data = vec![1u8; 100];
        let stripe = engine.encode_segment(2, &data).unwrap();
        engine.register_stripe(stripe);

        assert_eq!(engine.stripe_count(), 2);
    }
}
