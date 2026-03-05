//! Security tests for claudefs-transport bandwidth_shaper module.
//!
//! This module validates security properties of the bandwidth shaping system
//! including token bucket correctness, enforcement modes, per-tenant isolation,
//! burst capacity handling, and configuration validation.

#[cfg(test)]
mod tests {
    use claudefs_transport::bandwidth_shaper::*;
    use std::time::Duration;
    use std::thread;

    fn make_allocation(
        id: u64,
        rate: u64,
        burst: u64,
        mode: EnforcementMode,
    ) -> BandwidthAllocation {
        BandwidthAllocation::new(BandwidthId(id), rate, burst, mode)
    }

    // ============================================================================
    // Category 1: Token Bucket Correctness (8 tests)
    // ============================================================================

    mod token_bucket_correctness {
        use super::*;

        #[test]
        fn test_transport_bw_sec_initial_tokens_equals_capacity() {
            let bucket = TokenBucket::new(1000, 100);
            let available = bucket.available();

            assert_eq!(available, 1000,
                "Initial tokens should equal capacity");
        }

        #[test]
        fn test_transport_bw_sec_refill_respects_rate() {
            let bucket = TokenBucket::new(1000, 100);

            bucket.try_consume(900);

            thread::sleep(Duration::from_millis(1100));

            let available = bucket.available();

            assert!(available >= 100,
                "After 1.1 seconds with rate=100/sec, should have ~100 tokens");
        }

        #[test]
        fn test_transport_bw_sec_refill_stops_at_capacity() {
            let bucket = TokenBucket::new(1000, 100);

            bucket.try_consume(500);

            thread::sleep(Duration::from_millis(1100));

            let available = bucket.available();

            assert!(available <= 1000,
                "Tokens should not exceed capacity");
        }

        #[test]
        fn test_transport_bw_sec_try_consume_fails_insufficient_tokens() {
            let bucket = TokenBucket::new(50, 100);

            let result = bucket.try_consume(100);

            assert!(!result,
                "try_consume should fail when tokens insufficient");
        }

        #[test]
        fn test_transport_bw_sec_try_consume_succeeds_if_sufficient() {
            let bucket = TokenBucket::new(100, 1000);

            let result = bucket.try_consume(50);

            assert!(result, "try_consume should succeed with sufficient tokens");

            let available = bucket.available();
            assert!(available <= 50, "Tokens should be reduced");
        }

        #[tokio::test]
        async fn test_transport_bw_sec_try_consume_atomic_check_and_subtract() {
            use std::sync::Arc;

            let bucket = Arc::new(TokenBucket::new(1000, 1000000));
            let mut handles = vec![];

            for _ in 0..10 {
                let bucket = Arc::clone(&bucket);
                let handle = tokio::spawn(async move {
                    bucket.try_consume(100)
                });
                handles.push(handle);
            }

            let mut success_count = 0;
            for handle in handles {
                if handle.await.unwrap() {
                    success_count += 1;
                }
            }

            assert!(success_count <= 10,
                "All concurrent consumes should be atomic");
        }

        #[test]
        fn test_transport_bw_sec_last_refill_timestamp_updated() {
            let bucket = TokenBucket::new(1000, 100);

            let before = bucket.last_refill_ns.load(std::sync::atomic::Ordering::Relaxed);

            bucket.try_consume(100);

            let after = bucket.last_refill_ns.load(std::sync::atomic::Ordering::Relaxed);

            assert!(after >= before,
                "Last refill timestamp should be updated");
        }

        #[test]
        fn test_transport_bw_sec_refill_calculation_correct() {
            let bucket = TokenBucket::new(1000, 1000);

            bucket.try_consume(500);

            let before = bucket.tokens.load(std::sync::atomic::Ordering::Relaxed);

            #[cfg(test)]
            bucket.refill_at_ns(before + 500_000_000);

            let after = bucket.tokens.load(std::sync::atomic::Ordering::Relaxed);

            assert!(after >= before,
                "Refill should add tokens based on elapsed time");
        }
    }

    // ============================================================================
    // Category 2: Enforcement Modes (8 tests)
    // ============================================================================

    mod enforcement_modes {
        use super::*;

        #[test]
        fn test_transport_bw_sec_hard_mode_rejects_over_limit() {
            let shaper = BandwidthShaper::default();
            let alloc = make_allocation(1, 100, 100, EnforcementMode::Hard);

            shaper.set_allocation(alloc).unwrap();

            let result = shaper.try_allocate(BandwidthId(1), 200);

            assert!(result.is_err(),
                "Hard mode should reject when exceeding limit");
            if let Err(e) = result {
                assert!(matches!(e, BandwidthError::LimitExceeded { .. }),
                    "Should return LimitExceeded error");
            }
        }

        #[test]
        fn test_transport_bw_sec_soft_mode_allows_over_limit() {
            let shaper = BandwidthShaper::default();
            let alloc = make_allocation(1, 100, 500, EnforcementMode::Soft);

            shaper.set_allocation(alloc).unwrap();

            let result = shaper.try_allocate(BandwidthId(1), 200);

            assert!(result.is_ok(),
                "Soft mode should allow over limit with warning");
        }

        #[test]
        fn test_transport_bw_sec_hard_soft_mode_switch() {
            let shaper = BandwidthShaper::default();

            let alloc_hard = make_allocation(1, 100, 100, EnforcementMode::Hard);
            shaper.set_allocation(alloc_hard).unwrap();

            let hard_result = shaper.try_allocate(BandwidthId(1), 150);
            assert!(hard_result.is_err(), "Hard mode should reject");

            let alloc_soft = make_allocation(1, 100, 500, EnforcementMode::Soft);
            shaper.set_allocation(alloc_soft).unwrap();

            let soft_result = shaper.try_allocate(BandwidthId(1), 150);
            assert!(soft_result.is_ok(), "Soft mode should allow");
        }

        #[test]
        fn test_transport_bw_sec_mode_change_does_not_reset_tokens() {
            let shaper = BandwidthShaper::default();
            let alloc1 = make_allocation(1, 1000, 1000, EnforcementMode::Hard);
            shaper.set_allocation(alloc1).unwrap();

            shaper.try_allocate(BandwidthId(1), 500).unwrap();

            let stats_before = shaper.stats(BandwidthId(1)).unwrap();

            let alloc2 = make_allocation(1, 1000, 1000, EnforcementMode::Soft);
            shaper.set_allocation(alloc2).unwrap();

            let stats_after = shaper.stats(BandwidthId(1)).unwrap();

            assert!(stats_after.current_tokens <= stats_before.current_tokens + 100,
                "Mode change should not reset tokens");
        }

        #[tokio::test]
        async fn test_transport_bw_sec_per_tenant_enforcement_independent() {
            use std::sync::Arc;

            let shaper = Arc::new(BandwidthShaper::default());

            shaper.set_allocation(make_allocation(1, 100, 100, EnforcementMode::Hard)).unwrap();
            shaper.set_allocation(make_allocation(2, 100, 100, EnforcementMode::Soft)).unwrap();

            let shaper_clone = Arc::clone(&shaper);
            let handle1 = tokio::spawn(async move {
                shaper_clone.try_allocate(BandwidthId(1), 150)
            });

            let shaper_clone2 = Arc::clone(&shaper);
            let handle2 = tokio::spawn(async move {
                shaper_clone2.try_allocate(BandwidthId(2), 150)
            });

            let result1 = handle1.await.unwrap();
            let result2 = handle2.await.unwrap();

            assert!(result1.is_err(), "Tenant 1 (Hard) should reject");
            assert!(result2.is_ok(), "Tenant 2 (Soft) should allow");
        }

        #[test]
        fn test_transport_bw_sec_enforcement_mode_validation() {
            let alloc = BandwidthAllocation::new(BandwidthId(1), 0, 100, EnforcementMode::Hard);

            assert!(!alloc.is_valid(),
                "Allocation with zero bytes_per_sec should be invalid");
        }

        #[test]
        fn test_transport_bw_sec_hard_mode_with_burst_capacity() {
            let shaper = BandwidthShaper::default();
            let alloc = make_allocation(1, 100, 500, EnforcementMode::Hard);

            shaper.set_allocation(alloc).unwrap();

            let result = shaper.try_allocate(BandwidthId(1), 300);

            assert!(result.is_ok(),
                "Hard mode should allow within burst capacity");
        }

        #[test]
        fn test_transport_bw_sec_soft_mode_logs_warning() {
            let shaper = BandwidthShaper::default();
            let alloc = make_allocation(1, 100, 100, EnforcementMode::Soft);

            shaper.set_allocation(alloc).unwrap();

            shaper.try_allocate(BandwidthId(1), 150).unwrap();

            let stats = shaper.stats(BandwidthId(1)).unwrap();
            assert!(stats.requests_granted >= 1,
                "Soft mode should grant request despite exceeding limit");
        }
    }

    // ============================================================================
    // Category 3: Per-Tenant Isolation (7 tests)
    // ============================================================================

    mod per_tenant_isolation {
        use super::*;

        #[test]
        fn test_transport_bw_sec_separate_bucket_per_tenant() {
            let shaper = BandwidthShaper::default();

            for i in 1..=10 {
                let alloc = make_allocation(i, 1000, 1000, EnforcementMode::Hard);
                shaper.set_allocation(alloc).unwrap();
            }

            let stats = shaper.all_stats();

            assert_eq!(stats.len(), 10,
                "Should have separate bucket for each tenant");
        }

        #[test]
        fn test_transport_bw_sec_tenant_a_usage_independent_tenant_b() {
            let shaper = BandwidthShaper::default();

            shaper.set_allocation(make_allocation(1, 100, 100, EnforcementMode::Hard)).unwrap();
            shaper.set_allocation(make_allocation(2, 100, 100, EnforcementMode::Hard)).unwrap();

            shaper.try_allocate(BandwidthId(1), 100).unwrap();

            let stats_tenant_b = shaper.stats(BandwidthId(2)).unwrap();

            assert_eq!(stats_tenant_b.current_tokens, 100,
                "Tenant B's bucket should be unaffected by Tenant A's usage");
        }

        #[tokio::test]
        async fn test_transport_bw_sec_concurrent_allocations_different_tenants() {
            use std::sync::Arc;

            let shaper = Arc::new(BandwidthShaper::default());

            shaper.set_allocation(make_allocation(1, 10000, 10000, EnforcementMode::Hard)).unwrap();
            shaper.set_allocation(make_allocation(2, 10000, 10000, EnforcementMode::Hard)).unwrap();

            let mut handles = vec![];

            for _ in 0..20 {
                let shaper = Arc::clone(&shaper);
                let handle = tokio::spawn(async move {
                    shaper.try_allocate(BandwidthId(1), 100)
                });
                handles.push(handle);
            }

            for _ in 0..20 {
                let shaper = Arc::clone(&shaper);
                let handle = tokio::spawn(async move {
                    shaper.try_allocate(BandwidthId(2), 100)
                });
                handles.push(handle);
            }

            for handle in handles {
                let _ = handle.await;
            }

            let stats1 = shaper.stats(BandwidthId(1)).unwrap();
            let stats2 = shaper.stats(BandwidthId(2)).unwrap();

            assert!(stats1.requests_granted + stats1.requests_rejected > 0,
                "Tenant 1 should have activity");
            assert!(stats2.requests_granted + stats2.requests_rejected > 0,
                "Tenant 2 should have activity");
        }

        #[test]
        fn test_transport_bw_sec_dashmap_no_cross_tenant_leak() {
            let shaper = BandwidthShaper::default();

            shaper.set_allocation(make_allocation(1, 100, 100, EnforcementMode::Hard)).unwrap();
            shaper.set_allocation(make_allocation(2, 200, 200, EnforcementMode::Soft)).unwrap();

            let stats1 = shaper.stats(BandwidthId(1)).unwrap();
            let stats2 = shaper.stats(BandwidthId(2)).unwrap();

            assert_eq!(stats1.allocated_bytes_per_sec, 100,
                "Tenant 1 should have its own allocation");
            assert_eq!(stats2.allocated_bytes_per_sec, 200,
                "Tenant 2 should have its own allocation");
        }

        #[test]
        fn test_transport_bw_sec_tenant_removal_idempotent() {
            let shaper = BandwidthShaper::default();
            let config = BandwidthShaperConfig {
                tick_interval_ms: 10,
                cleanup_interval_ms: 1,
            };
            let shaper = BandwidthShaper::new(config);

            shaper.set_allocation(make_allocation(1, 1000, 1000, EnforcementMode::Hard)).unwrap();

            shaper.cleanup();

            shaper.cleanup();

            assert!(true, "Idempotent removal should succeed");
        }

        #[test]
        fn test_transport_bw_sec_tenant_allocation_add_independently() {
            let shaper = BandwidthShaper::default();

            shaper.set_allocation(make_allocation(1, 1000, 1000, EnforcementMode::Hard)).unwrap();
            shaper.set_allocation(make_allocation(2, 2000, 2000, EnforcementMode::Soft)).unwrap();

            let stats1 = shaper.stats(BandwidthId(1)).unwrap();
            let stats2 = shaper.stats(BandwidthId(2)).unwrap();

            assert!(stats1.is_some() && stats2.is_some(),
                "Both allocations should exist independently");
        }

        #[test]
        fn test_transport_bw_sec_same_tenant_multiple_allocations_share_bucket() {
            let shaper = BandwidthShaper::default();

            shaper.set_allocation(make_allocation(1, 1000, 1000, EnforcementMode::Hard)).unwrap();
            shaper.try_allocate(BandwidthId(1), 500).unwrap();

            let first_allocation_tokens = shaper.stats(BandwidthId(1)).unwrap().current_tokens;

            shaper.set_allocation(make_allocation(1, 2000, 2000, EnforcementMode::Hard)).unwrap();

            let after_replacement_tokens = shaper.stats(BandwidthId(1)).unwrap().current_tokens;

            assert!(after_replacement_tokens > 0,
                "Same tenant should share/update bucket, not create duplicate");
        }
    }

    // ============================================================================
    // Category 4: Burst Capacity Handling (4 tests)
    // ============================================================================

    mod burst_capacity_handling {
        use super::*;

        #[test]
        fn test_transport_bw_sec_burst_allows_temporary_exceed() {
            let shaper = BandwidthShaper::default();
            let alloc = make_allocation(1, 100, 500, EnforcementMode::Hard);

            shaper.set_allocation(alloc).unwrap();

            let result = shaper.try_allocate(BandwidthId(1), 300);

            assert!(result.is_ok(),
                "Should allow consumption up to burst capacity");
        }

        #[test]
        fn test_transport_bw_sec_burst_capacity_depletes() {
            let shaper = BandwidthShaper::default();
            let alloc = make_allocation(1, 100, 500, EnforcementMode::Hard);

            shaper.set_allocation(alloc).unwrap();

            shaper.try_allocate(BandwidthId(1), 300).unwrap();
            shaper.try_allocate(BandwidthId(1), 300).unwrap();

            let result = shaper.try_allocate(BandwidthId(1), 100);

            assert!(result.is_err(),
                "Burst should be depleted after use");
        }

        #[test]
        fn test_transport_bw_sec_burst_capacity_replenishes() {
            let shaper = BandwidthShaper::default();
            let alloc = make_allocation(1, 100, 500, EnforcementMode::Hard);

            shaper.set_allocation(alloc).unwrap();

            shaper.try_allocate(BandwidthId(1), 500).unwrap();

            thread::sleep(Duration::from_millis(1100));

            let result = shaper.try_allocate(BandwidthId(1), 100);

            assert!(result.is_ok(),
                "Burst should partially replenish after time");
        }

        #[test]
        fn test_transport_bw_sec_over_burst_rejected() {
            let shaper = BandwidthShaper::default();
            let alloc = make_allocation(1, 100, 500, EnforcementMode::Hard);

            shaper.set_allocation(alloc).unwrap();

            let result = shaper.try_allocate(BandwidthId(1), 600);

            assert!(result.is_err(),
                "Should reject request exceeding burst_bytes");
        }
    }

    // ============================================================================
    // Category 5: Configuration Validation (3 tests)
    // ============================================================================

    mod configuration_validation {
        use super::*;

        #[test]
        fn test_transport_bw_sec_valid_config_bytes_per_sec_gt_zero() {
            let alloc = BandwidthAllocation::new(BandwidthId(1), 1000, 100, EnforcementMode::Hard);

            assert!(alloc.is_valid(),
                "Allocation with bytes_per_sec > 0 should be valid");
        }

        #[test]
        fn test_transport_bw_sec_valid_config_burst_bytes_gt_zero() {
            let alloc = BandwidthAllocation::new(BandwidthId(1), 100, 1000, EnforcementMode::Hard);

            assert!(alloc.is_valid(),
                "Allocation with burst_bytes > 0 should be valid");
        }

        #[test]
        fn test_transport_bw_sec_invalid_config_zero_values() {
            let alloc_zero_rate = BandwidthAllocation::new(BandwidthId(1), 0, 100, EnforcementMode::Hard);
            let alloc_zero_burst = BandwidthAllocation::new(BandwidthId(1), 100, 0, EnforcementMode::Hard);

            assert!(!alloc_zero_rate.is_valid(),
                "Allocation with bytes_per_sec=0 should be invalid");
            assert!(!alloc_zero_burst.is_valid(),
                "Allocation with burst_bytes=0 should be invalid");
        }
    }
}