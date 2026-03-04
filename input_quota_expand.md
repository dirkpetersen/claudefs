# Expand Tests in quota_tracker.rs for ClaudeFS Reduction Crate

## Working Directory
`/home/cfs/claudefs/crates/claudefs-reduce/src/quota_tracker.rs`

## Context
The file currently has 27 tests. Add 6 more tests for edge cases.

## Current Tests (for reference)
1. test_new_tracker_empty
2. test_set_get_quota
3. test_default_quota_unlimited
4. test_record_write_increments_logical
5. test_record_write_increments_physical
6. test_record_write_increments_chunk_count
7. test_record_dedup_hit
8. test_record_delete_decrements
9. test_record_delete_no_underflow
10. test_check_write_within_limits
11. test_check_write_logical_exceeded
12. test_check_write_physical_exceeded
13. test_check_write_unlimited_quota
14. test_remove_namespace
15. test_total_usage
16. test_reset_usage
17. test_reduction_ratio_normal
18. test_reduction_ratio_zero_physical
19. test_namespaces_list
20. test_quota_tracker_multiple_namespaces_isolated
21. test_quota_tracker_near_limit
22. test_quota_usage_percentage
23. test_quota_violation_details
24. test_namespace_id_equality
25. test_quota_config_default_values
26. test_quota_tracker_reset_usage
27. test_quota_usage_clone
28. test_quota_config_clone
29. test_quota_tracker_default

## Add These 6 New Tests

1. `test_record_delete_to_empty_namespace` — delete from namespace with no prior writes (saturating_sub works)
2. `test_record_many_dedup_hits` — multiple dedup hits accumulate correctly
3. `test_check_write_exact_limit` — write that reaches exactly the limit succeeds
4. `test_get_quota_missing_namespace` — get_quota for unknown namespace returns None
5. `test_usage_missing_namespace` — usage for unknown namespace returns default (zeros)
6. `test_record_write_zero_bytes` — record_write with 0 bytes is valid, increments chunk_count

## Style
- Follow existing test patterns in the file
- NO COMMENTS in test code

## Validation
After editing, verify:
```bash
cd /home/cfs/claudefs && cargo test -p claudefs-reduce quota_tracker::
```
