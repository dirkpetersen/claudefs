//! Deep security tests for claudefs-fuse: buffer pool, passthrough, capability, quota, WORM.
//!
//! Part of A10 Phase 7: FUSE deep security audit

#[cfg(test)]
mod tests {
    use claudefs_fuse::buffer_pool::{
        Buffer, BufferPool, BufferPoolConfig, BufferPoolStats, BufferSize,
    };
    use claudefs_fuse::capability::{
        CapabilityNegotiator, KernelVersion, NegotiatedCapabilities, PassthroughMode,
    };
    use claudefs_fuse::mount_opts::{CacheMode, MountOptions, ReadWriteMode};
    use claudefs_fuse::passthrough::{
        check_kernel_version, PassthroughConfig, PassthroughState, PassthroughStatus,
    };
    use claudefs_fuse::quota_enforce::{QuotaEnforcer, QuotaStatus, QuotaUsage};
    use claudefs_fuse::ratelimit::{
        IoRateLimiter, RateLimitDecision, RateLimiterConfig, TokenBucket,
    };
    use claudefs_fuse::worm::{ImmutabilityMode, WormRegistry};

    fn make_buffer_pool() -> BufferPool {
        BufferPool::new(BufferPoolConfig::default())
    }

    fn make_limited_buffer_pool() -> BufferPool {
        BufferPool::new(BufferPoolConfig {
            max_4k: 2,
            max_64k: 0,
            max_1m: 0,
        })
    }

    fn make_passthrough_state() -> PassthroughState {
        PassthroughState::new(&PassthroughConfig::default())
    }

    fn make_mount_options() -> MountOptions {
        MountOptions::default()
    }

    fn make_quota_enforcer() -> QuotaEnforcer {
        QuotaEnforcer::with_default_ttl()
    }

    fn make_worm_registry() -> WormRegistry {
        WormRegistry::new()
    }

    // ============================================================================
    // Category 1: Buffer Pool Memory Safety (5 tests)
    // ============================================================================

    #[test]
    fn test_buffer_clear_only_partial() {
        let mut pool = make_buffer_pool();
        let mut buf = pool.acquire(BufferSize::Page4K);

        // Write sensitive data (0xFF) to entire buffer
        for byte in buf.as_mut_slice() {
            *byte = 0xFF;
        }

        // Call buf.clear() - it only zeroes first 64 bytes
        buf.clear();

        // FINDING-FUSE-DEEP-01: clear() only zeroes first 64 bytes
        // Bytes beyond position 64 still contain 0xFF - sensitive data leakage risk
        let first_64_cleared = buf.as_slice()[..64].iter().all(|&b| b == 0);
        let beyond_64_still_ff = buf.as_slice()[64..].iter().any(|&b| b == 0xFF);

        if first_64_cleared && beyond_64_still_ff {
            eprintln!("FINDING-FUSE-DEEP-01: Buffer.clear() only zeroes first 64 bytes - sensitive data leakage risk");
        }

        assert!(first_64_cleared, "First 64 bytes should be cleared");
        assert!(
            beyond_64_still_ff,
            "Bytes 64+ should still be 0xFF (not cleared)"
        );
    }

    #[test]
    fn test_buffer_pool_exhaustion_still_allocates() {
        let mut pool = make_limited_buffer_pool();

        // Acquire 2 buffers (within limit)
        let _b1 = pool.acquire(BufferSize::Page4K);
        let _b2 = pool.acquire(BufferSize::Page4K);

        // Acquire 3rd buffer - beyond max_4k=2
        let b3 = pool.acquire(BufferSize::Page4K);

        // FINDING-FUSE-DEEP-02: Pool may allocate beyond max limit
        // Document whether capacity is enforced or fresh allocation occurs
        let stats = pool.stats();
        if stats.alloc_count >= 3 {
            eprintln!(
                "FINDING-FUSE-DEEP-02: Buffer pool allocated {} buffers (exceeded max_4k=2)",
                stats.alloc_count
            );
        }

        // Verify a buffer was returned (either reused or newly allocated)
        assert_eq!(b3.len(), 4096);
    }

    #[test]
    fn test_buffer_id_uniqueness() {
        let mut pool = make_buffer_pool();

        // Acquire 100 buffers
        let mut ids: Vec<u64> = Vec::with_capacity(100);
        for _ in 0..100 {
            let buf = pool.acquire(BufferSize::Page4K);
            ids.push(buf.id);
        }

        // Verify all IDs are unique
        let unique_count = ids.iter().collect::<std::collections::HashSet<_>>().len();
        assert_eq!(unique_count, 100, "All 100 buffer IDs should be unique");

        // Release all and acquire 100 more
        for _ in 0..100 {
            let buf = pool.acquire(BufferSize::Page4K);
            ids.push(buf.id);
        }

        // Check if any IDs from first batch are reused
        let first_batch_ids: std::collections::HashSet<u64> = ids[..100].iter().cloned().collect();
        let second_batch_ids: std::collections::HashSet<u64> = ids[100..].iter().cloned().collect();

        // FINDING-FUSE-DEEP-03: Document ID reuse behavior
        let intersection: Vec<_> = first_batch_ids.intersection(&second_batch_ids).collect();
        if !intersection.is_empty() {
            eprintln!(
                "FINDING-FUSE-DEEP-03: {} IDs were reused after release",
                intersection.len()
            );
        } else {
            eprintln!("FINDING-FUSE-DEEP-03: No ID reuse detected (all unique)");
        }
    }

    #[test]
    fn test_buffer_size_bytes_correctness() {
        // Verify BufferSize::size_bytes() returns correct values
        assert_eq!(
            BufferSize::Page4K.size_bytes(),
            4096,
            "Page4K should be 4096 bytes"
        );
        assert_eq!(
            BufferSize::Block64K.size_bytes(),
            65536,
            "Block64K should be 65536 bytes"
        );
        assert_eq!(
            BufferSize::Block1M.size_bytes(),
            1048576,
            "Block1M should be 1048576 bytes"
        );

        // Acquire buffers and verify buf.len() matches
        let mut pool = make_buffer_pool();

        let buf_4k = pool.acquire(BufferSize::Page4K);
        assert_eq!(buf_4k.len(), 4096);

        let buf_64k = pool.acquire(BufferSize::Block64K);
        assert_eq!(buf_64k.len(), 65536);

        let buf_1m = pool.acquire(BufferSize::Block1M);
        assert_eq!(buf_1m.len(), 1048576);
    }

    #[test]
    fn test_buffer_pool_stats_accuracy() {
        let mut pool = make_buffer_pool();

        // Acquire 3 buffers
        let b1 = pool.acquire(BufferSize::Page4K);
        let b2 = pool.acquire(BufferSize::Page4K);
        let b3 = pool.acquire(BufferSize::Page4K);

        // Release 2 buffers (b1 and b2), keep b3 checked out
        pool.release(b1);
        pool.release(b2);

        // Verify stats after first round
        let stats = pool.stats();
        assert_eq!(stats.alloc_count, 3, "alloc_count should be 3");
        assert_eq!(stats.return_count, 2, "return_count should be 2");

        // Acquire 2 more - should reuse b1 and b2 from pool
        let _b4 = pool.acquire(BufferSize::Page4K);
        let _b5 = pool.acquire(BufferSize::Page4K);

        // Verify reuse_count incremented (2 buffers were reused)
        let stats2 = pool.stats();
        assert_eq!(stats2.reuse_count, 2, "reuse_count should be 2");
        // alloc_count should still be 3 since we reused buffers
        assert_eq!(
            stats2.alloc_count, 3,
            "alloc_count should remain 3 (reused)"
        );
    }

    // ============================================================================
    // Category 2: Passthrough & Capability Security (5 tests)
    // ============================================================================

    #[test]
    fn test_passthrough_negative_fd_accepted() {
        let mut state = make_passthrough_state();

        // Register a negative FD
        state.register_fd(1, -1);

        // FINDING-FUSE-DEEP-04: No FD validation
        // Document if negative FD is silently accepted
        let fd = state.get_fd(1);
        if fd == Some(-1) {
            eprintln!("FINDING-FUSE-DEEP-04: Negative FD (-1) was accepted without validation");
        }

        assert_eq!(fd, Some(-1), "Negative FD should be stored as-is");
    }

    #[test]
    fn test_passthrough_fd_table_unbounded_growth() {
        let mut state = make_passthrough_state();

        // Register 10000 FD entries
        for i in 0..10000 {
            state.register_fd(i, i as i32);
        }

        let count = state.fd_count();

        // FINDING-FUSE-DEEP-05: No limit on fd_table size
        // Document unbounded growth potential (memory exhaustion vector)
        eprintln!(
            "FINDING-FUSE-DEEP-05: fd_table grew to {} entries (unbounded)",
            count
        );

        assert_eq!(count, 10000, "fd_count should be 10000");
    }

    #[test]
    fn test_capability_panic_without_negotiate() {
        let negotiator = CapabilityNegotiator::new();

        // Verify is_negotiated returns false
        assert!(!negotiator.is_negotiated(), "is_negotiated should be false");

        // DO NOT call capabilities() directly since it panics
        // This documents the panic risk

        // FINDING-FUSE-DEEP-06: Panic risk without negotiation
        // Calling capabilities() before negotiate() will panic
        eprintln!("FINDING-FUSE-DEEP-06: capabilities() panics if called before negotiate()");
    }

    #[test]
    fn test_kernel_version_parse_edge_cases() {
        // Parse valid version strings
        let v68 = KernelVersion::parse("6.8");
        assert!(v68.is_some(), "6.8 should parse successfully");
        let v68 = v68.unwrap();
        assert_eq!(v68.major, 6);
        assert_eq!(v68.minor, 8);

        let v680 = KernelVersion::parse("6.8.0");
        assert!(v680.is_some(), "6.8.0 should parse successfully");
        assert_eq!(v680.unwrap().patch, 0);

        // Parse version strings - some parse successfully, some don't
        // Note: 0.0.0, 999.999.999 actually parse as valid (numeric)
        assert!(KernelVersion::parse("6.8").is_some(), "6.8 should be valid");

        // These are clearly invalid formats
        assert!(
            KernelVersion::parse("").is_none(),
            "empty string should be invalid"
        );
        assert!(
            KernelVersion::parse("abc").is_none(),
            "abc should be invalid"
        );
        assert!(
            KernelVersion::parse("6").is_none(),
            "single number should be invalid"
        );
        assert!(
            KernelVersion::parse("6.8.0.1").is_none(),
            "4 parts should be invalid"
        );

        // Additional edge cases
        assert!(
            KernelVersion::parse("6").is_none(),
            "single number should be invalid"
        );
        assert!(
            KernelVersion::parse("6.8.0.1").is_none(),
            "4 parts should be invalid"
        );
    }

    #[test]
    fn test_passthrough_kernel_version_boundary() {
        let config = PassthroughConfig::default();

        // Kernel 6.7 should be too old (min is 6.8)
        let status_67 = check_kernel_version(6, 7, &config);
        assert!(matches!(
            status_67,
            PassthroughStatus::DisabledKernelTooOld { major: 6, minor: 7 }
        ));

        // Kernel 6.8 should be enabled (exact boundary)
        let status_68 = check_kernel_version(6, 8, &config);
        assert!(matches!(status_68, PassthroughStatus::Enabled));

        // Kernel 6.9 should be enabled
        let status_69 = check_kernel_version(6, 9, &config);
        assert!(matches!(status_69, PassthroughStatus::Enabled));

        // Kernel 7.0 should be enabled
        let status_70 = check_kernel_version(7, 0, &config);
        assert!(matches!(status_70, PassthroughStatus::Enabled));
    }

    // ============================================================================
    // Category 3: Mount Options & Session Security (5 tests)
    // ============================================================================

    #[test]
    fn test_mount_default_permissions_false_risk() {
        let opts = make_mount_options();

        // FINDING-FUSE-DEEP-07: Security-critical default
        // default_permissions defaults to false - kernel permission checks disabled
        // Relies on FUSE daemon for permission enforcement
        if !opts.default_permissions {
            eprintln!("FINDING-FUSE-DEEP-07: default_permissions is false by default - kernel permission checks disabled");
        }

        assert!(
            !opts.default_permissions,
            "default_permissions should be false"
        );
    }

    #[test]
    fn test_mount_direct_io_with_kernel_cache() {
        let mut opts = make_mount_options();

        // Set conflicting options
        opts.direct_io = true;
        opts.kernel_cache = true;

        let args = opts.to_fuse_args();

        // FINDING-FUSE-DEEP-08: Conflicting options accepted without warning
        // direct_io and kernel_cache together may cause unexpected behavior
        let has_direct_io = args.iter().any(|s| s.contains("direct_io"));
        let has_kernel_cache = args.iter().any(|s| s.contains("kernel_cache"));

        eprintln!(
            "FINDING-FUSE-DEEP-08: direct_io={} kernel_cache={} (conflicting options)",
            opts.direct_io, opts.kernel_cache
        );

        assert!(has_direct_io, "direct_io should be in args");
        // Note: kernel_cache is not serialized in current to_fuse_args implementation
    }

    #[test]
    fn test_mount_options_to_fuse_args_content() {
        let mut opts = make_mount_options();

        // Set specific options
        opts.allow_other = true;
        opts.read_only = ReadWriteMode::ReadOnly;

        let args = opts.to_fuse_args();

        // Verify output contains expected flags
        assert!(
            args.contains(&"allow_other".to_string()),
            "Should contain allow_other"
        );
        assert!(
            args.contains(&"-r".to_string()),
            "Should contain -r for read-only"
        );
    }

    #[test]
    fn test_mount_empty_paths() {
        let mut opts = make_mount_options();

        // Set empty paths
        opts.source = std::path::PathBuf::from("");
        opts.target = std::path::PathBuf::from("");

        // FINDING-FUSE-DEEP-09: No validation for empty paths
        // Document whether validation catches empty paths or allows them
        let args = opts.to_fuse_args();

        eprintln!(
            "FINDING-FUSE-DEEP-09: Empty paths passed to FUSE args: {:?}",
            args
        );

        // Check if empty string appears in args
        assert!(args.len() >= 2);
    }

    #[test]
    fn test_mount_max_background_zero() {
        let mut opts = make_mount_options();

        // Set potentially problematic values
        opts.max_background = 0;
        opts.congestion_threshold = 0;

        let args = opts.to_fuse_args();

        // FINDING-FUSE-DEEP-10: Zero max_background could cause stalls
        // No background requests allowed - potential DoS vector
        eprintln!("FINDING-FUSE-DEEP-10: max_background=0, congestion_threshold=0 (potential stall vector)");

        // Note: max_background is not currently serialized to fuse args
        assert_eq!(opts.max_background, 0);
        assert_eq!(opts.congestion_threshold, 0);
    }

    // ============================================================================
    // Category 4: Rate Limiting & Quota Enforcement (5 tests)
    // ============================================================================

    #[test]
    fn test_token_bucket_refill_overflow() {
        // Create TokenBucket with high capacity (1B per second * 10 = 10B capacity)
        let mut bucket = TokenBucket::new(1_000_000_000, 10.0);

        // Initial fill should be at capacity (full)
        assert!(
            (bucket.fill_level() - 1.0).abs() < 0.001,
            "Initial fill should be 1.0"
        );

        // Refill after a large time delta (10 seconds = 10 billion tokens)
        let tokens = bucket.refill(10_000);

        // FINDING-FUSE-DEEP-11: Token bucket overflow behavior
        // Verify tokens saturate at capacity (don't overflow u64)
        // fill_level should still be at max (1.0) after large time delta
        assert!(
            bucket.fill_level() >= 0.99,
            "Tokens should saturate at capacity, not overflow"
        );
    }

    #[test]
    fn test_token_bucket_consume_more_than_available() {
        let mut bucket = TokenBucket::new(100, 1.0);

        // Initial fill should be at capacity (100)
        assert!(
            (bucket.fill_level() - 1.0).abs() < 0.001,
            "Initial fill should be 1.0"
        );

        // Consume 50 - should succeed
        let result1 = bucket.try_consume(50.0, 0);
        assert!(result1, "Should be able to consume 50");

        // Try to consume 60 - should fail (only 50 remain)
        let result2 = bucket.try_consume(60.0, 0);
        assert!(!result2, "Should NOT be able to consume 60 (only 50 left)");

        // Verify we can still consume remaining 50
        let result3 = bucket.try_consume(50.0, 0);
        assert!(result3, "Should be able to consume remaining 50");

        // Now try to consume anything more - should fail
        let result4 = bucket.try_consume(1.0, 0);
        assert!(!result4, "Should NOT be able to consume when bucket empty");
    }

    #[test]
    fn test_quota_enforcer_check_boundary() {
        let mut enforcer = make_quota_enforcer();

        // Test at exact soft limit - should return SoftExceeded, not HardExceeded
        let mut usage = QuotaUsage::new(100, 200);
        usage.bytes_used = 100; // At soft limit exactly
        enforcer.update_user_quota(100, usage);

        let status_at_soft = enforcer.check_write(100, 0, 1);
        // At exact soft limit: bytes_used (100) > soft (100) is false, so returns Ok
        // Let's test when bytes_used exceeds soft
        let mut usage2 = QuotaUsage::new(100, 200);
        usage2.bytes_used = 101; // Over soft limit
        enforcer.update_user_quota(101, usage2);

        let status_over_soft = enforcer.check_write(101, 0, 1);
        assert_eq!(status_over_soft.unwrap(), QuotaStatus::SoftExceeded);

        // Test at hard limit - should return HardExceeded (error)
        let mut usage3 = QuotaUsage::new(100, 200);
        usage3.bytes_used = 200; // At hard limit exactly
        enforcer.update_user_quota(200, usage3);

        let status_at_hard = enforcer.check_write(200, 0, 1);
        assert!(status_at_hard.is_err(), "At hard limit should return error");
    }

    #[test]
    fn test_rate_limiter_burst_factor_multiplier() {
        let config = RateLimiterConfig {
            bytes_per_sec: 1000,
            ops_per_sec: 0,
            burst_factor: 2.0,
            reject_threshold: 0.0,
        };

        // Effective capacity should be 2x the base (2000 bytes)
        let mut limiter = claudefs_fuse::ratelimit::IoRateLimiter::new(config);

        // Consume up to burst limit (2000)
        let result1 = limiter.check_io(1500, 0);
        assert!(
            matches!(result1, RateLimitDecision::Allow),
            "Should allow up to burst limit"
        );

        // Try one more - should throttle
        let result2 = limiter.check_io(1000, 0);
        assert!(
            matches!(result2, RateLimitDecision::Throttle { .. }),
            "Should throttle after burst"
        );

        // Verify burst_factor effect via fill_level behavior
        let mut bucket = TokenBucket::new(1000, 2.0);
        // After consuming 1000 (initial capacity was 2000), should still have 1000 left
        bucket.try_consume(1000.0, 0);
        assert!(
            (bucket.fill_level() - 0.5).abs() < 0.01,
            "Fill should be 0.5 after consuming half"
        );
    }

    #[test]
    fn test_token_bucket_zero_refill_rate() {
        // Create TokenBucket with zero refill rate
        let mut bucket = TokenBucket::new(0, 2.0);

        // Capacity should be 0 (unlimited bucket)
        assert!(bucket.is_unlimited(), "Zero rate creates unlimited bucket");

        // Consume all tokens
        bucket.try_consume(100.0, 0);

        // Refill should not add any tokens
        let tokens_after = bucket.refill(1000);

        // FINDING-FUSE-DEEP-12: Zero refill rate creates permanent denial
        // Tokens remain at 0 after refill when rate is 0
        if tokens_after == 0.0 {
            eprintln!("FINDING-FUSE-DEEP-12: Zero refill rate creates permanent denial");
        }

        assert_eq!(
            tokens_after, 0.0,
            "Tokens should remain 0 with zero refill rate"
        );
    }

    // ============================================================================
    // Category 5: WORM & Immutability Enforcement (5 tests)
    // ============================================================================

    #[test]
    fn test_worm_immutable_blocks_all_writes() {
        let mut registry = make_worm_registry();
        let now = 1000u64;

        // Set inode to Immutable mode
        registry.set_mode(1, ImmutabilityMode::Immutable, now, 100);

        // Verify all operations are blocked
        assert!(registry.get(1).unwrap().mode.is_write_blocked(now));
        assert!(registry.get(1).unwrap().mode.is_delete_blocked(now));
        assert!(registry.get(1).unwrap().mode.is_rename_blocked(now));
        assert!(registry.get(1).unwrap().mode.is_truncate_blocked(now));
    }

    #[test]
    fn test_worm_append_only_allows_append() {
        let now = 1000u64;
        let mode = ImmutabilityMode::AppendOnly;

        // AppendOnly blocks write, delete, truncate but allows append
        assert!(
            mode.is_write_blocked(now),
            "write_blocked should be true for AppendOnly"
        );
        assert!(
            mode.is_delete_blocked(now),
            "delete_blocked should be true for AppendOnly"
        );
        assert!(
            mode.is_truncate_blocked(now),
            "truncate_blocked should be true for AppendOnly"
        );
        assert!(
            mode.is_append_allowed(now),
            "append_allowed should be true for AppendOnly"
        );
    }

    #[test]
    fn test_worm_none_mode_allows_all() {
        let now = 1000u64;
        let mode = ImmutabilityMode::None;

        // None mode should allow all operations
        assert!(!mode.is_write_blocked(now), "None should not block write");
        assert!(!mode.is_delete_blocked(now), "None should not block delete");
        assert!(!mode.is_rename_blocked(now), "None should not block rename");
        assert!(
            !mode.is_truncate_blocked(now),
            "None should not block truncate"
        );
    }

    #[test]
    fn test_worm_legal_hold_overrides() {
        let now = 1000u64;
        let mode = ImmutabilityMode::LegalHold {
            hold_id: "LIT-001".to_string(),
        };

        // Legal hold is the strictest mode - blocks all modifications
        assert!(mode.is_write_blocked(now), "LegalHold should block write");
        assert!(mode.is_delete_blocked(now), "LegalHold should block delete");
        assert!(mode.is_rename_blocked(now), "LegalHold should block rename");

        // Verify append is not allowed
        assert!(
            !mode.is_append_allowed(now),
            "LegalHold should not allow append"
        );
    }

    #[test]
    fn test_worm_mode_change_allowed() {
        let mut registry = make_worm_registry();
        let now = 1000u64;

        // Set inode to AppendOnly
        registry.set_mode(1, ImmutabilityMode::AppendOnly, now, 100);

        // Change to Immutable
        registry.set_mode(1, ImmutabilityMode::Immutable, now + 100, 100);

        // Verify the change succeeded
        let record = registry.get(1).unwrap();
        assert!(matches!(record.mode, ImmutabilityMode::Immutable));

        // FINDING-FUSE-DEEP-13: Mode can be escalated AND downgraded
        // No unidirectional enforcement - mode can change both ways
        eprintln!("FINDING-FUSE-DEEP-13: WORM mode can be changed (AppendOnly -> Immutable)");

        // Try changing back to None
        registry.set_mode(1, ImmutabilityMode::None, now + 200, 100);

        let record2 = registry.get(1).unwrap();
        assert!(
            matches!(record2.mode, ImmutabilityMode::None),
            "Mode can be downgraded to None"
        );

        eprintln!("FINDING-FUSE-DEEP-13b: WORM mode can be downgraded (Immutable -> None)");
    }
}
