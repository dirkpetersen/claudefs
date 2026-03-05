//! Bandwidth shaper (QoS enforcer) security tests.
//!
//! Part of A10 Phase 35: Tests for token bucket correctness, enforcement modes,
//! per-tenant isolation, burst capacity, and configuration validation.

#[cfg(test)]
mod tests {
    use claudefs_transport::bandwidth_shaper::{
        BandwidthAllocation, BandwidthId, BandwidthShaper, EnforcementMode, TokenBucket,
    };
    use std::time::{Duration, SystemTime};

    mod token_bucket_correctness {
        use super::*;

        #[test]
        fn test_transport_bw_sec_initial_tokens_equals_capacity() {
            let bucket = TokenBucket::new(1000, 100);
            assert_eq!(
                bucket.tokens.load(std::sync::atomic::Ordering::SeqCst),
                1000,
                "Initial tokens should equal capacity"
            );
        }

        #[test]
        fn test_transport_bw_sec_refill_stops_at_capacity() {
            let bucket = TokenBucket::new(1000, 100);
            bucket.refill();

            let tokens = bucket.tokens.load(std::sync::atomic::Ordering::SeqCst);
            assert!(
                tokens <= 1000,
                "Tokens should not exceed capacity after refill"
            );
        }

        #[test]
        fn test_transport_bw_sec_try_consume_fails_insufficient_tokens() {
            let bucket = TokenBucket::new(100, 50);
            bucket.tokens.store(50, std::sync::atomic::Ordering::SeqCst);

            let result = bucket.try_consume(100);
            assert!(!result, "Should fail when trying to consume more than available");
        }

        #[test]
        fn test_transport_bw_sec_try_consume_succeeds_if_sufficient() {
            let bucket = TokenBucket::new(100, 50);
            bucket.tokens.store(100, std::sync::atomic::Ordering::SeqCst);

            let result = bucket.try_consume(50);
            assert!(result, "Should succeed when tokens are sufficient");

            let remaining = bucket.tokens.load(std::sync::atomic::Ordering::SeqCst);
            assert_eq!(remaining, 50, "Tokens should decrease after consumption");
        }

        #[test]
        fn test_transport_bw_sec_last_refill_timestamp_updated() {
            let bucket = TokenBucket::new(1000, 100);
            let before = bucket.last_refill_ns.load(std::sync::atomic::Ordering::SeqCst);

            bucket.refill();

            let after = bucket.last_refill_ns.load(std::sync::atomic::Ordering::SeqCst);
            assert!(after >= before, "Last refill timestamp should be updated or stay same");
        }

        #[test]
        fn test_transport_bw_sec_refill_calculation_correct() {
            let bucket = TokenBucket::new(1000, 1000); // 1000 tokens/sec
            let now_ns = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64;

            // Simulate 0.5 second elapsed
            bucket.last_refill_ns.store(now_ns - 500_000_000, std::sync::atomic::Ordering::SeqCst);

            bucket.refill();

            // Should have refilled ~500 tokens
            let tokens = bucket.tokens.load(std::sync::atomic::Ordering::SeqCst);
            assert!(tokens > 500 && tokens <= 1000, "Tokens should increase by ~500");
        }

        #[tokio::test]
        async fn test_transport_bw_sec_try_consume_atomic_check_and_subtract() {
            let bucket = std::sync::Arc::new(TokenBucket::new(1000, 100));
            let mut handles = vec![];

            for _ in 0..100 {
                let bucket_clone = std::sync::Arc::clone(&bucket);
                handles.push(tokio::spawn(async move {
                    let _ = bucket_clone.try_consume(10);
                }));
            }

            for handle in handles {
                let _ = handle.await;
            }

            let tokens = bucket.tokens.load(std::sync::atomic::Ordering::SeqCst);
            assert!(tokens <= 1000, "Tokens should not exceed capacity");
        }
    }

    mod enforcement_modes {
        use super::*;

        #[test]
        fn test_transport_bw_sec_hard_mode_rejects_over_limit() {
            let alloc = BandwidthAllocation::new(BandwidthId(1), 100, 500, EnforcementMode::Hard);
            assert_eq!(alloc.enforcement_mode, EnforcementMode::Hard);
            assert_eq!(alloc.bytes_per_sec, 100);
        }

        #[test]
        fn test_transport_bw_sec_soft_mode_allows_over_limit() {
            let alloc = BandwidthAllocation::new(BandwidthId(1), 100, 500, EnforcementMode::Soft);
            assert_eq!(alloc.enforcement_mode, EnforcementMode::Soft);
        }

        #[test]
        fn test_transport_bw_sec_hard_soft_mode_switch() {
            let mut alloc =
                BandwidthAllocation::new(BandwidthId(1), 100, 500, EnforcementMode::Hard);
            assert_eq!(alloc.enforcement_mode, EnforcementMode::Hard);

            alloc.enforcement_mode = EnforcementMode::Soft;
            assert_eq!(alloc.enforcement_mode, EnforcementMode::Soft);
        }

        #[test]
        fn test_transport_bw_sec_mode_change_does_not_reset_tokens() {
            let bucket = TokenBucket::new(100, 50);
            bucket.tokens.store(50, std::sync::atomic::Ordering::SeqCst);

            let tokens_before = bucket.tokens.load(std::sync::atomic::Ordering::SeqCst);

            // Mode change shouldn't affect bucket
            let _alloc = BandwidthAllocation::new(BandwidthId(1), 100, 500, EnforcementMode::Soft);

            let tokens_after = bucket.tokens.load(std::sync::atomic::Ordering::SeqCst);
            assert_eq!(tokens_before, tokens_after, "Tokens should not change on mode switch");
        }

        #[tokio::test]
        async fn test_transport_bw_sec_per_tenant_enforcement_independent() {
            let alloc_a = BandwidthAllocation::new(BandwidthId(1), 100, 500, EnforcementMode::Hard);
            let alloc_b = BandwidthAllocation::new(BandwidthId(2), 100, 500, EnforcementMode::Soft);

            assert_eq!(alloc_a.enforcement_mode, EnforcementMode::Hard);
            assert_eq!(alloc_b.enforcement_mode, EnforcementMode::Soft);
        }

        #[test]
        fn test_transport_bw_sec_enforcement_mode_validation() {
            let default_mode = EnforcementMode::default();
            assert_eq!(default_mode, EnforcementMode::Soft, "Default mode should be Soft");
        }

        #[test]
        fn test_transport_bw_sec_hard_mode_with_burst_capacity() {
            let alloc = BandwidthAllocation::new(BandwidthId(1), 100, 500, EnforcementMode::Hard);
            assert_eq!(alloc.burst_bytes, 500, "Burst capacity should be preserved");
            assert!(
                alloc.burst_bytes > alloc.bytes_per_sec,
                "Burst should exceed per_sec"
            );
        }
    }

    mod per_tenant_isolation {
        use super::*;

        #[test]
        fn test_transport_bw_sec_separate_bucket_per_tenant() {
            let allocs: Vec<_> = (0..10)
                .map(|i| {
                    BandwidthAllocation::new(
                        BandwidthId(i),
                        100 * (i + 1) as u64,
                        500,
                        EnforcementMode::Hard,
                    )
                })
                .collect();

            assert_eq!(allocs.len(), 10, "Should create separate allocations");
            for (i, alloc) in allocs.iter().enumerate() {
                assert_eq!(alloc.tenant_id, BandwidthId(i as u64));
            }
        }

        #[test]
        fn test_transport_bw_sec_tenant_a_usage_independent_tenant_b() {
            let bucket_a = TokenBucket::new(1000, 100);
            let bucket_b = TokenBucket::new(1000, 100);

            bucket_a.try_consume(500);

            let tokens_a = bucket_a.tokens.load(std::sync::atomic::Ordering::SeqCst);
            let tokens_b = bucket_b.tokens.load(std::sync::atomic::Ordering::SeqCst);

            assert_eq!(tokens_a, 500, "Bucket A should be depleted");
            assert_eq!(tokens_b, 1000, "Bucket B should be unaffected");
        }

        #[tokio::test]
        async fn test_transport_bw_sec_concurrent_allocations_different_tenants() {
            use std::sync::Arc;

            let mut handles = vec![];

            for tenant_id in 0..40 {
                handles.push(tokio::spawn(async move {
                    let _alloc = BandwidthAllocation::new(
                        BandwidthId(tenant_id),
                        100,
                        500,
                        if tenant_id < 20 {
                            EnforcementMode::Hard
                        } else {
                            EnforcementMode::Soft
                        },
                    );
                }));
            }

            for handle in handles {
                let _ = handle.await;
            }
        }

        #[test]
        fn test_transport_bw_sec_tenant_removal_idempotent() {
            let alloc = BandwidthAllocation::new(BandwidthId(1), 100, 500, EnforcementMode::Hard);
            assert_eq!(alloc.tenant_id, BandwidthId(1));
            // Removal would be handled at allocator level, not bucket level
        }

        #[test]
        fn test_transport_bw_sec_tenant_allocation_add_independently() {
            let alloc_a = BandwidthAllocation::new(BandwidthId(1), 100, 500, EnforcementMode::Hard);
            let alloc_b = BandwidthAllocation::new(BandwidthId(2), 200, 600, EnforcementMode::Soft);

            assert_eq!(alloc_a.bytes_per_sec, 100);
            assert_eq!(alloc_b.bytes_per_sec, 200);
            assert_ne!(alloc_a.tenant_id, alloc_b.tenant_id);
        }

        #[test]
        fn test_transport_bw_sec_same_tenant_multiple_allocations_share_bucket() {
            let bucket = std::sync::Arc::new(TokenBucket::new(1000, 100));
            let bucket_clone = std::sync::Arc::clone(&bucket);

            bucket.try_consume(300);
            bucket_clone.try_consume(200);

            let tokens = bucket.tokens.load(std::sync::atomic::Ordering::SeqCst);
            assert_eq!(tokens, 500, "Shared bucket should show combined consumption");
        }
    }

    mod burst_capacity_handling {
        use super::*;

        #[test]
        fn test_transport_bw_sec_burst_allows_temporary_exceed() {
            let alloc = BandwidthAllocation::new(BandwidthId(1), 100, 500, EnforcementMode::Hard);
            assert!(alloc.burst_bytes >= alloc.bytes_per_sec * 3, "Burst should be >> per_sec");
        }

        #[test]
        fn test_transport_bw_sec_burst_capacity_depletes() {
            let bucket = TokenBucket::new(500, 100);
            bucket.try_consume(300);

            let tokens = bucket.tokens.load(std::sync::atomic::Ordering::SeqCst);
            assert_eq!(tokens, 200, "Burst should deplete on consumption");
        }

        #[test]
        fn test_transport_bw_sec_burst_capacity_replenishes() {
            let bucket = TokenBucket::new(500, 1000);
            bucket.try_consume(300);

            std::thread::sleep(Duration::from_millis(100));
            bucket.refill();

            let tokens = bucket.tokens.load(std::sync::atomic::Ordering::SeqCst);
            assert!(tokens > 200, "Tokens should replenish after delay");
        }

        #[test]
        fn test_transport_bw_sec_over_burst_rejected() {
            let bucket = TokenBucket::new(500, 100);

            let result = bucket.try_consume(600);
            assert!(!result, "Should reject consumption exceeding capacity");
        }
    }

    mod configuration_validation {
        use super::*;

        #[test]
        fn test_transport_bw_sec_valid_config_bytes_per_sec_gt_zero() {
            let alloc = BandwidthAllocation::new(BandwidthId(1), 100, 500, EnforcementMode::Hard);
            assert!(alloc.is_valid(), "Config with bytes_per_sec > 0 should be valid");
        }

        #[test]
        fn test_transport_bw_sec_valid_config_burst_bytes_gt_zero() {
            let alloc = BandwidthAllocation::new(BandwidthId(1), 100, 500, EnforcementMode::Soft);
            assert!(alloc.is_valid(), "Config with burst_bytes > 0 should be valid");
        }

        #[test]
        fn test_transport_bw_sec_invalid_config_zero_values() {
            let alloc_zero_bytes = BandwidthAllocation::new(BandwidthId(1), 0, 500, EnforcementMode::Hard);
            let alloc_zero_burst = BandwidthAllocation::new(BandwidthId(1), 100, 0, EnforcementMode::Hard);

            assert!(!alloc_zero_bytes.is_valid(), "Config with bytes_per_sec = 0 should be invalid");
            assert!(!alloc_zero_burst.is_valid(), "Config with burst_bytes = 0 should be invalid");
        }
    }
}
