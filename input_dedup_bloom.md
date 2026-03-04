# Implement dedup_bloom.rs for ClaudeFS Reduction Crate

## Working Directory
`/home/cfs/claudefs/crates/claudefs-reduce/src/`

## Context
You are implementing Phase 18 of the A3 (Data Reduction) agent for ClaudeFS.
The reduction crate currently has 1303 tests across 64 modules. Goal: ~1390 tests.

## Task
Create NEW FILE: `/home/cfs/claudefs/crates/claudefs-reduce/src/dedup_bloom.rs`

Implement a Bloom filter for fast dedup negative lookups.

Before a full CAS lookup, a Bloom filter can quickly reject chunks that are definitely
not in the index, avoiding expensive hash table lookups.

## Requirements

### Types

1. **BloomConfig struct**:
```rust
pub struct BloomConfig {
    pub expected_items: usize,      // default 1_000_000
    pub false_positive_rate: f64,   // default 0.01 = 1%
}
```
- Derive: Debug, Clone, Serialize, Deserialize
- impl Default
- `fn bit_count(&self) -> usize` — approximate formula:
  ```
  ceil(expected_items * -1.44 * log2(false_positive_rate))
  ```
  Use: `(expected_items as f64 * -1.44 * (false_positive_rate as f64).ln() / (2.0f64).ln()).ceil() as usize`
  Actually simpler: `-(n * ln(p)) / (ln(2)^2)` = `ceil(n * (-1.44 * log2(p)))`
  In Rust: `(self.expected_items as f64 * (-1.44) * (self.false_positive_rate as f64).ln() / std::f64::consts::LN_2 / std::f64::consts::LN_2).ceil() as usize`
- `fn hash_count(&self) -> usize` — `ceil(-log2(p))` = `((-(self.false_positive_rate as f64).ln() / std::f64::consts::LN_2).ceil() as usize).max(1)`

2. **BloomStats struct**:
```rust
pub struct BloomStats {
    pub items_added: u64,
    pub queries: u64,
    pub definitely_absent: u64,
    pub possibly_present: u64,
}
```
- Derive: Debug, Clone, Default
- `fn false_negative_rate(&self) -> f64` → 0.0 (Bloom filters have no false negatives)

3. **DedupBloom struct**:
- Internal: `bits: Vec<bool>`, `config: BloomConfig`, `stats: BloomStats`
- `fn new(config: BloomConfig) -> Self` — allocate bit vector of config.bit_count() bits
- `fn add(&mut self, hash: &[u8; 32])` — set bits:
  - Use first `hash_count` u64 values from hash as hash functions
  - Interpret bytes 0..8, 8..16, 16..24, etc. as u64 LE, modulo bit_count
  - For each hash function k: bits[(hash_k % bit_count)] = true
- `fn may_contain(&self, hash: &[u8; 32]) -> bool` — true if all bits set; false if any bit clear
- `fn definitely_absent(&self, hash: &[u8; 32]) -> bool` → !may_contain(hash)
- `fn stats(&self) -> &BloomStats`
- `fn estimated_fill_ratio(&self) -> f64` → bits set / total bits

## Required Tests (at least 15)

1. `bloom_config_default` — verify defaults
2. `bloom_bit_count_calculation` — for 1M items, 1% FPR: bit_count > 0
3. `bloom_hash_count_calculation` — for 1% FPR: hash_count should be reasonable (around 7)
4. `add_and_may_contain_true` — added hash is found (may_contain returns true)
5. `definitely_absent_for_not_added` — hash not added is definitely absent
6. `add_multiple_hashes` — add several hashes
7. `may_contain_all_added` — all added hashes are found
8. `stats_items_added` — items_added increments
9. `stats_queries_after_check` — queries increments on may_contain/definitely_absent
10. `stats_definitely_absent` — definitely_absent counter increments
11. `stats_possibly_present` — possibly_present counter increments
12. `estimated_fill_ratio_increases` — add items, fill ratio grows
13. `bloom_no_false_negatives` — added items never return definitely_absent
14. `empty_bloom_all_absent` — empty filter: all hashes are definitely_absent
15. `false_positive_rate_reasonable` — with 1M items and 1% FPR, false positive rate stays low

## Hash Function Implementation Details

To extract k hash values from the 32-byte BLAKE3 hash:
```rust
fn hash_values(hash: &[u8; 32], count: usize) -> Vec<usize> {
    (0..count)
        .map(|i| {
            let start = (i * 8) % 24; // Use first 24 bytes for up to 3 hash functions
            let bytes: [u8; 8] = hash[start..start+8].try_into().unwrap();
            u64::from_le_bytes(bytes) as usize
        })
        .collect()
}
```

Actually, for simplicity, use more bytes:
```rust
fn hash_values(hash: &[u8; 32], count: usize, bit_count: usize) -> Vec<usize> {
    (0..count)
        .map(|i| {
            let start = (i * 8) % 24;
            let bytes: [u8; 8] = hash[start..start+8].try_into().unwrap();
            (u64::from_le_bytes(bytes) as usize) % bit_count
        })
        .collect()
}
```

But a better approach is to use different parts of the hash for each hash function:
```rust
fn hash_values(hash: &[u8; 32], count: usize, bit_count: usize) -> Vec<usize> {
    (0..count)
        .map(|i| {
            let offset = i * 8;
            if offset + 8 <= 32 {
                let bytes: [u8; 8] = hash[offset..offset+8].try_into().unwrap();
                (u64::from_le_bytes(bytes) as usize) % bit_count
            } else {
                // Wrap around for more hash functions than 4
                let bytes: [u8; 8] = [
                    hash[offset % 32],
                    hash[(offset + 1) % 32],
                    hash[(offset + 2) % 32],
                    hash[(offset + 3) % 32],
                    hash[(offset + 4) % 32],
                    hash[(offset + 5) % 32],
                    hash[(offset + 6) % 32],
                    hash[(offset + 7) % 32],
                ];
                (u64::from_le_bytes(bytes) as usize) % bit_count
            }
        })
        .collect()
}
```

## Style
- Follow existing crate patterns
- Use `use serde::{Deserialize, Serialize};`
- NO COMMENTS in code
- Use `#[cfg(test)] mod tests { ... }` pattern

## Validation
After writing the file, verify it compiles:
```bash
cd /home/cfs/claudefs && cargo check -p claudefs-reduce
```
