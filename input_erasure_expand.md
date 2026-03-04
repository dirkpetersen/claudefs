# Expand Tests in erasure_codec.rs for ClaudeFS Reduction Crate

## Working Directory
`/home/cfs/claudefs/crates/claudefs-reduce/src/erasure_codec.rs`

## Context
The file currently has 17 tests. Add 8 more tests for ErasureCodec edge cases.

## Current Tests (for reference)
1. test_new_codec_four_two
2. test_new_codec_two_one
3. test_encode_decode_roundtrip_4_2
4. test_encode_decode_roundtrip_2_1
5. test_encode_empty_payload
6. test_encode_large_payload
7. test_encode_odd_size_payload
8. test_shard_count
9. test_shard_sizes_equal
10. test_reconstruct_one_missing_data_shard
11. test_reconstruct_one_missing_parity_shard
12. test_reconstruct_two_missing_shards_4_2
13. test_reconstruct_too_many_missing
14. test_decode_verifies_parity
15. test_segment_id_preserved
16. test_ec_stripe_constants
17. test_ec_stripe_total_shards

## Add These 8 New Tests

1. `test_encode_single_byte` — payload of length 1
2. `test_reconstruct_all_missing_parity` — 4+2 with both parity shards missing (should work)
3. `test_reconstruct_all_missing_data_fails` — 4+2 with all 4 data shards missing (fails, need at least 4 shards total)
4. `test_decode_after_reconstruct` — full workflow: encode → corrupt → reconstruct → decode
5. `test_encoded_segment_shard_size` — verify shard_size calculation
6. `test_multiple_segments_same_id` — encoding same segment_id twice produces same shard structure (deterministic)
7. `test_reconstruct_with_wrong_shard_size` — reconstruct fails with inconsistent shard sizes
8. `test_2_1_reconstruct_one_missing` — 2+1 stripe with one missing shard

## Style
- Follow existing test patterns in the file
- NO COMMENTS in test code

## Validation
After editing, verify:
```bash
cd /home/cfs/claudefs && cargo test -p claudefs-reduce erasure_codec::
```
