# Fix false_positive_rate_reasonable test in dedup_bloom.rs

## Working directory
`/home/cfs/claudefs`

## Problem

In `crates/claudefs-reduce/src/dedup_bloom.rs`, the test `false_positive_rate_reasonable` is failing.

The bug is that `[i as u8; 32]` causes hash collisions when `i >= 256` because the cast wraps:
- `256 as u8 = 0`, `257 as u8 = 1`, etc.
- So 10000 hashes only have 256 unique values
- The "not added" hashes (10000..11000) map to the same byte values as the added ones

## Fix

Please read the file `crates/claudefs-reduce/src/dedup_bloom.rs` and find the test
`false_positive_rate_reasonable`. Fix it to use unique 32-byte hashes that don't collide.

Replace the test with this corrected version:

```rust
    #[test]
    fn false_positive_rate_reasonable() {
        let config = BloomConfig {
            expected_items: 1_000_000,
            false_positive_rate: 0.01,
        };
        let mut bloom = DedupBloom::new(config);
        let added_hashes: Vec<[u8; 32]> = (0u64..10000u64)
            .map(|i| {
                let mut h = [0u8; 32];
                h[0..8].copy_from_slice(&i.to_le_bytes());
                h
            })
            .collect();
        for hash in &added_hashes {
            bloom.add(hash);
        }
        let not_added_hashes: Vec<[u8; 32]> = (100000u64..101000u64)
            .map(|i| {
                let mut h = [0u8; 32];
                h[0..8].copy_from_slice(&i.to_le_bytes());
                h
            })
            .collect();
        let mut false_positives = 0;
        for hash in &not_added_hashes {
            if bloom.may_contain(hash) {
                false_positives += 1;
            }
        }
        let fpr = false_positives as f64 / not_added_hashes.len() as f64;
        assert!(fpr < 0.1);
    }
```

Also fix the `estimated_fill_ratio_increases` test to use unique hashes:

```rust
    #[test]
    fn estimated_fill_ratio_increases() {
        let config = BloomConfig {
            expected_items: 1000,
            false_positive_rate: 0.01,
        };
        let mut bloom = DedupBloom::new(config);
        let initial_ratio = bloom.estimated_fill_ratio();
        for i in 0u64..50u64 {
            let mut hash = [0u8; 32];
            hash[0..8].copy_from_slice(&i.to_le_bytes());
            bloom.add(&hash);
        }
        let final_ratio = bloom.estimated_fill_ratio();
        assert!(final_ratio > initial_ratio);
    }
```

## Steps
1. Read `crates/claudefs-reduce/src/dedup_bloom.rs`
2. Fix the two tests as shown above
3. Run `cd /home/cfs/claudefs && cargo test -p claudefs-reduce 2>&1 | tail -5`
4. Ensure all tests pass
