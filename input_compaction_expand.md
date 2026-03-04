# Expand Tests in compaction.rs for ClaudeFS Reduction Crate

## Working Directory
`/home/cfs/claudefs/crates/claudefs-reduce/src/compaction.rs`

## Context
The file currently has 16 tests. Add 8 more tests for CompactionEngine edge cases.

## Current Tests (for reference)
1. test_default_config
2. test_live_ratio_all_live
3. test_live_ratio_all_dead
4. test_live_ratio_half
5. test_live_ratio_empty_segment
6. test_select_candidates_none
7. test_select_candidates_some
8. test_select_candidates_all
9. test_compact_empty_candidates
10. test_compact_fully_dead_segment
11. test_compact_keeps_live_chunks
12. test_compact_dead_chunks_dropped
13. test_compact_bytes_reclaimed
14. test_compact_multiple_segments_into_one
15. test_compact_segment_id_increments
16. test_compact_updates_result_stats

## Add These 8 New Tests

1. `test_compact_single_live_chunk` — segment with one live chunk, one dead
2. `test_compact_preserves_order` — live chunks appear in same order in output
3. `test_compact_empty_segment_with_live_hashes` — empty segment, no live hashes → ratio is 1.0, not selected
4. `test_compact_threshold_boundary` — segment exactly at threshold (e.g., 0.7) is NOT selected
5. `test_compact_multiple_output_segments` — many live chunks exceed target_segment_bytes, produce multiple segments
6. `test_compact_duplicate_hashes` — same hash appears in multiple input segments, both are repacked
7. `test_compact_zero_byte_payload` — handle empty chunk gracefully
8. `test_live_ratio_with_partial_overlap` — some hashes in segment are live, some not

## Style
- Follow existing test patterns in the file
- Use helper functions `make_segment` and `hashes_from_data` already defined
- NO COMMENTS in test code

## Validation
After editing, verify:
```bash
cd /home/cfs/claudefs && cargo test -p claudefs-reduce compaction::
```
