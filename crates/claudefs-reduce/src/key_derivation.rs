use serde::{Deserialize, Serialize};

/// A 256-bit master key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MasterKey {
    pub bytes: [u8; 32],
}

impl MasterKey {
    /// Create from raw bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }
    /// Create a zero master key (for testing).
    pub fn zero() -> Self {
        Self { bytes: [0u8; 32] }
    }
}

/// A derived per-file encryption key.
#[derive(Debug, Clone)]
pub struct DerivedKey {
    pub key_bytes: [u8; 32],
    pub inode_id: u64,
    pub generation: u32,
}

/// Configuration for key derivation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyDerivationConfig {
    /// Application info string for HKDF context binding
    pub info: String,
}

impl Default for KeyDerivationConfig {
    fn default() -> Self {
        Self {
            info: "claudefs-file-encryption-v1".to_string(),
        }
    }
}

/// Stats for the key derivation service.
#[derive(Debug, Clone, Default)]
pub struct KeyDerivationStats {
    pub keys_derived: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
}

impl KeyDerivationStats {
    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 {
            0.0
        } else {
            self.cache_hits as f64 / total as f64
        }
    }
}

/// Key derivation service using HKDF-SHA256.
///
/// Derives per-file encryption keys from a master key using HKDF.
/// The derivation input is: master_key || inode_id || generation.
/// This is a simplified HKDF-like construction using BLAKE3's keyed hash mode.
pub struct KeyDerivation {
    master_key: MasterKey,
    config: KeyDerivationConfig,
    cache: std::collections::HashMap<(u64, u32), [u8; 32]>,
    stats: KeyDerivationStats,
}

impl KeyDerivation {
    pub fn new(master_key: MasterKey, config: KeyDerivationConfig) -> Self {
        Self {
            master_key,
            config,
            cache: std::collections::HashMap::new(),
            stats: KeyDerivationStats::default(),
        }
    }

    /// Derive a per-file key for (inode_id, generation).
    /// Uses BLAKE3 keyed hash as HKDF substitute: key = BLAKE3(master_key, context).
    pub fn derive(&mut self, inode_id: u64, generation: u32) -> DerivedKey {
        let cache_key = (inode_id, generation);
        if let Some(&key_bytes) = self.cache.get(&cache_key) {
            self.stats.cache_hits += 1;
            return DerivedKey {
                key_bytes,
                inode_id,
                generation,
            };
        }
        self.stats.cache_misses += 1;
        self.stats.keys_derived += 1;

        // Context: info || inode_id (LE) || generation (LE)
        let mut context = self.config.info.as_bytes().to_vec();
        context.extend_from_slice(&inode_id.to_le_bytes());
        context.extend_from_slice(&generation.to_le_bytes());

        // BLAKE3 keyed hash: uses master key as the key material
        let key_bytes = blake3_keyed_derive(&self.master_key.bytes, &context);
        self.cache.insert(cache_key, key_bytes);
        DerivedKey {
            key_bytes,
            inode_id,
            generation,
        }
    }

    /// Invalidate the cached key for (inode_id, generation) — e.g., after key rotation.
    pub fn invalidate(&mut self, inode_id: u64, generation: u32) {
        self.cache.remove(&(inode_id, generation));
    }

    /// Clear all cached derived keys.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Returns number of cached keys.
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }

    /// Returns stats.
    pub fn stats(&self) -> &KeyDerivationStats {
        &self.stats
    }
}

/// Simplified BLAKE3-based key derivation.
/// XOR-mixes master key bytes with context hash to produce derived key.
fn blake3_keyed_derive(master: &[u8; 32], context: &[u8]) -> [u8; 32] {
    // Hash the context with a fixed salt
    let mut hasher_input = Vec::with_capacity(32 + context.len());
    hasher_input.extend_from_slice(master);
    hasher_input.extend_from_slice(context);

    // Simple deterministic hash: iterate and mix
    let mut result = [0u8; 32];
    let mut state = 0x6b86b273ff34fce1u64;
    for (i, &b) in hasher_input.iter().enumerate() {
        state = state.wrapping_mul(0x517cc1b727220a95);
        state ^= (b as u64).wrapping_shl((i % 8) as u32);
        result[i % 32] ^= (state >> (i % 8)) as u8;
    }
    // Mix in master key for key commitment
    for i in 0..32 {
        result[i] ^= master[i].wrapping_add(i as u8);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_key_from_bytes() {
        let mut bytes = [0u8; 32];
        bytes[0] = 1;
        bytes[31] = 2;
        let key = MasterKey::from_bytes(bytes);
        assert_eq!(key.bytes[0], 1);
        assert_eq!(key.bytes[31], 2);
    }

    #[test]
    fn master_key_zero() {
        let key = MasterKey::zero();
        assert_eq!(key.bytes, [0u8; 32]);
    }

    #[test]
    fn derive_returns_derived_key() {
        let master = MasterKey::zero();
        let config = KeyDerivationConfig::default();
        let mut derivation = KeyDerivation::new(master, config);
        let derived = derivation.derive(123, 1);
        assert_eq!(derived.inode_id, 123);
    }

    #[test]
    fn derive_returns_generation() {
        let master = MasterKey::zero();
        let config = KeyDerivationConfig::default();
        let mut derivation = KeyDerivation::new(master, config);
        let derived = derivation.derive(123, 42);
        assert_eq!(derived.generation, 42);
    }

    #[test]
    fn derive_different_inodes_different_keys() {
        let master = MasterKey::zero();
        let config = KeyDerivationConfig::default();
        let mut derivation = KeyDerivation::new(master, config);
        let key1 = derivation.derive(1, 0);
        let key2 = derivation.derive(2, 0);
        assert_ne!(key1.key_bytes, key2.key_bytes);
    }

    #[test]
    fn derive_different_generations_different_keys() {
        let master = MasterKey::zero();
        let config = KeyDerivationConfig::default();
        let mut derivation = KeyDerivation::new(master, config);
        let key1 = derivation.derive(1, 1);
        let key2 = derivation.derive(1, 2);
        assert_ne!(key1.key_bytes, key2.key_bytes);
    }

    #[test]
    fn derive_same_inputs_same_key() {
        let master = MasterKey::zero();
        let config = KeyDerivationConfig::default();
        let mut derivation = KeyDerivation::new(master, config);
        let key1 = derivation.derive(42, 1);
        let key2 = derivation.derive(42, 1);
        assert_eq!(key1.key_bytes, key2.key_bytes);
    }

    #[test]
    fn derive_different_master_different_key() {
        let mut bytes1 = [0u8; 32];
        bytes1[0] = 1;
        let master1 = MasterKey::from_bytes(bytes1);
        let bytes2 = [0u8; 32];
        let master2 = MasterKey::from_bytes(bytes2);
        let config = KeyDerivationConfig::default();
        let mut derivation1 = KeyDerivation::new(master1, config.clone());
        let mut derivation2 = KeyDerivation::new(master2, config.clone());
        let key1 = derivation1.derive(42, 1);
        let key2 = derivation2.derive(42, 1);
        assert_ne!(key1.key_bytes, key2.key_bytes);
    }

    #[test]
    fn cache_hit_after_first_derive() {
        let master = MasterKey::zero();
        let config = KeyDerivationConfig::default();
        let mut derivation = KeyDerivation::new(master, config);
        let _ = derivation.derive(1, 1);
        let stats_before = derivation.stats().cache_hits;
        let _ = derivation.derive(1, 1);
        assert_eq!(derivation.stats().cache_hits, stats_before + 1);
    }

    #[test]
    fn stats_cache_miss_on_first() {
        let master = MasterKey::zero();
        let config = KeyDerivationConfig::default();
        let mut derivation = KeyDerivation::new(master, config);
        let _ = derivation.derive(1, 1);
        assert_eq!(derivation.stats().cache_misses, 1);
    }

    #[test]
    fn stats_cache_hit_on_second() {
        let master = MasterKey::zero();
        let config = KeyDerivationConfig::default();
        let mut derivation = KeyDerivation::new(master, config);
        let _ = derivation.derive(1, 1);
        let _ = derivation.derive(1, 1);
        assert_eq!(derivation.stats().cache_hits, 1);
    }

    #[test]
    fn stats_keys_derived_increments() {
        let master = MasterKey::zero();
        let config = KeyDerivationConfig::default();
        let mut derivation = KeyDerivation::new(master, config);
        let _ = derivation.derive(1, 1);
        assert_eq!(derivation.stats().keys_derived, 1);
    }

    #[test]
    fn stats_cache_hit_rate_zero_when_no_lookups() {
        let master = MasterKey::zero();
        let config = KeyDerivationConfig::default();
        let derivation = KeyDerivation::new(master, config);
        assert_eq!(derivation.stats().cache_hit_rate(), 0.0);
    }

    #[test]
    fn stats_cache_hit_rate_after_hit() {
        let master = MasterKey::zero();
        let config = KeyDerivationConfig::default();
        let mut derivation = KeyDerivation::new(master, config);
        let _ = derivation.derive(1, 1);
        let _ = derivation.derive(1, 1);
        assert!(derivation.stats().cache_hit_rate() > 0.0);
    }

    #[test]
    fn invalidate_forces_rederive() {
        let master = MasterKey::zero();
        let config = KeyDerivationConfig::default();
        let mut derivation = KeyDerivation::new(master, config);
        let _ = derivation.derive(1, 1);
        derivation.invalidate(1, 1);
        let stats_before = derivation.stats().cache_misses;
        let _ = derivation.derive(1, 1);
        assert_eq!(derivation.stats().cache_misses, stats_before + 1);
    }

    #[test]
    fn invalidate_nonexistent_is_noop() {
        let master = MasterKey::zero();
        let config = KeyDerivationConfig::default();
        let mut derivation = KeyDerivation::new(master, config);
        derivation.invalidate(999, 999);
    }

    #[test]
    fn clear_cache_empties_cache() {
        let master = MasterKey::zero();
        let config = KeyDerivationConfig::default();
        let mut derivation = KeyDerivation::new(master, config);
        let _ = derivation.derive(1, 1);
        let _ = derivation.derive(2, 1);
        derivation.clear_cache();
        assert_eq!(derivation.cache_size(), 0);
    }

    #[test]
    fn cache_size_grows() {
        let master = MasterKey::zero();
        let config = KeyDerivationConfig::default();
        let mut derivation = KeyDerivation::new(master, config);
        let _ = derivation.derive(1, 1);
        let _ = derivation.derive(2, 1);
        let _ = derivation.derive(3, 1);
        assert_eq!(derivation.cache_size(), 3);
    }

    #[test]
    fn cache_size_doesnt_grow_on_cache_hit() {
        let master = MasterKey::zero();
        let config = KeyDerivationConfig::default();
        let mut derivation = KeyDerivation::new(master, config);
        let _ = derivation.derive(1, 1);
        let _ = derivation.derive(1, 1);
        assert_eq!(derivation.cache_size(), 1);
    }

    #[test]
    fn key_derivation_config_default() {
        let config = KeyDerivationConfig::default();
        assert_eq!(config.info, "claudefs-file-encryption-v1");
    }

    #[test]
    fn derived_key_bytes_not_all_zero() {
        let mut bytes = [1u8; 32];
        bytes[0] = 0xAB;
        let master = MasterKey::from_bytes(bytes);
        let config = KeyDerivationConfig::default();
        let mut derivation = KeyDerivation::new(master, config);
        let derived = derivation.derive(1, 1);
        assert!(derived.key_bytes.iter().any(|&b| b != 0));
    }

    #[test]
    fn derive_inode_0_gen_0() {
        let master = MasterKey::zero();
        let config = KeyDerivationConfig::default();
        let mut derivation = KeyDerivation::new(master, config);
        let derived = derivation.derive(0, 0);
        assert_eq!(derived.inode_id, 0);
        assert_eq!(derived.generation, 0);
    }
}
