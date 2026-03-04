//! Security tests for claudefs-transport crate.
//!
//! This module validates security properties of the transport layer including:
//! - Certificate authentication and validation
//! - Zero-copy memory pool security
//! - Flow control and backpressure

#[cfg(test)]
mod tests {
    use claudefs_transport::conn_auth::{
        AuthConfig, AuthLevel, AuthResult, CertificateInfo, ConnectionAuthenticator,
    };
    use claudefs_transport::flowcontrol::{FlowControlConfig, FlowControlState, FlowController};
    use claudefs_transport::zerocopy::{RegionPool, ZeroCopyConfig};

    fn make_cert(
        subject: &str,
        issuer: &str,
        serial: &str,
        fingerprint: &str,
        not_before_ms: u64,
        not_after_ms: u64,
        is_ca: bool,
    ) -> CertificateInfo {
        CertificateInfo {
            subject: subject.to_string(),
            issuer: issuer.to_string(),
            serial: serial.to_string(),
            fingerprint_sha256: fingerprint.to_string(),
            not_before_ms,
            not_after_ms,
            is_ca,
        }
    }

    // ============================================================================
    // Category 1: Certificate Authentication (8 tests)
    // ============================================================================

    #[test]
    fn test_expired_cert_with_unset_time() {
        let config = AuthConfig::default();
        let mut auth = ConnectionAuthenticator::new(config);

        // Create cert that expired in the past
        let expired_ms = 1000u64;
        let cert = make_cert("server1", "ClusterCA", "01", "abc123", 0, expired_ms, false);

        // Do NOT call set_time() - time defaults to 0
        let result = auth.authenticate(&cert);

        // FINDING-TRANS-01: Time validation bypass
        // Document if expired cert is accepted when time is unset
        if matches!(result, AuthResult::Allowed { .. }) {
            eprintln!("FINDING-TRANS-01: Expired cert accepted with unset time (time=0)");
        }
    }

    #[test]
    fn test_expired_cert_with_correct_time() {
        let config = AuthConfig::default();
        let mut auth = ConnectionAuthenticator::new(config);

        let expired_ms = 1000u64;
        let cert = make_cert("server1", "ClusterCA", "01", "abc123", 0, expired_ms, false);

        // Set time past expiry
        auth.set_time(expired_ms + 1);
        let result = auth.authenticate(&cert);

        // Should be rejected - certificate expired
        assert!(matches!(
            result,
            AuthResult::CertificateExpired { subject, .. }
            if subject == "server1"
        ));
    }

    #[test]
    fn test_not_yet_valid_cert() {
        let config = AuthConfig::default();
        let mut auth = ConnectionAuthenticator::new(config);

        let not_before_ms = 10000u64;
        let not_after_ms = not_before_ms + 86400000 * 365;
        let cert = make_cert(
            "server1",
            "ClusterCA",
            "01",
            "abc123",
            not_before_ms,
            not_after_ms,
            false,
        );

        // Set time before cert is valid
        auth.set_time(5000);
        let result = auth.authenticate(&cert);

        // Should be rejected - not yet valid
        assert!(matches!(
            result,
            AuthResult::Denied { reason }
            if reason.contains("not yet valid")
        ));
    }

    #[test]
    fn test_revoked_serial_rejected() {
        let config = AuthConfig::default();
        let mut auth = ConnectionAuthenticator::new(config);

        // Revoke a serial number
        auth.revoke_serial("01".to_string());

        // Create cert with that serial
        let cert = make_cert(
            "server1",
            "ClusterCA",
            "01",
            "abc123",
            1000,
            86400000 * 365 * 1000 + 1000,
            false,
        );

        auth.set_time(5000);
        let result = auth.authenticate(&cert);

        // Should be rejected - certificate revoked
        assert!(matches!(
            result,
            AuthResult::CertificateRevoked { subject, serial }
            if subject == "server1" && serial == "01"
        ));
    }

    #[test]
    fn test_revoked_fingerprint_rejected() {
        let config = AuthConfig::default();
        let mut auth = ConnectionAuthenticator::new(config);

        // Revoke a fingerprint
        auth.revoke_fingerprint("abc123".to_string());

        // Create cert with that fingerprint
        let cert = make_cert(
            "server1",
            "ClusterCA",
            "01",
            "abc123",
            1000,
            86400000 * 365 * 1000 + 1000,
            false,
        );

        auth.set_time(5000);
        let result = auth.authenticate(&cert);

        // Should be rejected - certificate revoked
        assert!(matches!(
            result,
            AuthResult::CertificateRevoked { subject, .. }
            if subject == "server1"
        ));
    }

    #[test]
    fn test_ca_fingerprint_substring_match() {
        let config = AuthConfig {
            level: AuthLevel::MutualTls,
            allowed_subjects: vec![],
            allowed_fingerprints: vec![],
            max_cert_age_days: 365,
            require_cluster_ca: true,
            cluster_ca_fingerprint: Some("CA".to_string()),
        };
        let mut auth = ConnectionAuthenticator::new(config);

        // Create cert with issuer containing the substring
        let cert = make_cert(
            "server1",
            "MyCAInfoxyz",
            "01",
            "abc123",
            1000,
            86400000 * 365 * 1000 + 1000,
            false,
        );

        auth.set_time(5000);
        let result = auth.authenticate(&cert);

        // FINDING-TRANS-02: Weak CA validation
        // substring match - "CA" matches "MyCAInfoxyz"
        if matches!(result, AuthResult::Allowed { .. }) {
            eprintln!("FINDING-TRANS-02: CA fingerprint uses substring match (weak validation)");
        }
    }

    #[test]
    fn test_is_ca_field_ignored() {
        let config = AuthConfig::default();
        let mut auth = ConnectionAuthenticator::new(config);

        // Create cert with is_ca = true
        let cert = make_cert(
            "CA-Server",
            "RootCA",
            "01",
            "ca123",
            1000,
            86400000 * 365 * 1000 + 1000,
            true, // is_ca = true
        );

        auth.set_time(5000);
        let result = auth.authenticate(&cert);

        // FINDING-TRANS-03: is_ca not checked
        // Document if is_ca flag has any effect
        if matches!(result, AuthResult::Allowed { .. }) {
            eprintln!("FINDING-TRANS-03: is_ca field has no effect on auth decision");
        }
    }

    #[test]
    fn test_strict_mode_empty_allowed() {
        let config = AuthConfig {
            level: AuthLevel::MutualTlsStrict,
            allowed_subjects: vec![],
            allowed_fingerprints: vec![],
            max_cert_age_days: 365,
            require_cluster_ca: false,
            cluster_ca_fingerprint: None,
        };
        let mut auth = ConnectionAuthenticator::new(config);

        let cert = make_cert(
            "any-server",
            "AnyCA",
            "01",
            "anyfp",
            1000,
            86400000 * 365 * 1000 + 1000,
            false,
        );

        auth.set_time(5000);
        let result = auth.authenticate(&cert);

        // With empty allowed lists in strict mode, behavior depends on implementation
        // Document the behavior
        eprintln!(
            "FINDING-TRANS-04: MutualTlsStrict with empty allowed lists: {:?}",
            result
        );
    }

    // ============================================================================
    // Category 2: Zero-Copy Pool Security (6 tests)
    // ============================================================================

    #[test]
    fn test_pool_exhaustion_returns_none() {
        let config = ZeroCopyConfig {
            region_size: 1024,
            max_regions: 2,
            alignment: 4096,
            preregister: 2,
        };
        let pool = RegionPool::new(config);

        // Acquire all regions
        let r1 = pool.acquire();
        let r2 = pool.acquire();

        assert!(r1.is_some());
        assert!(r2.is_some());

        // Next acquire should return None (DoS protection)
        let r3 = pool.acquire();
        assert!(r3.is_none());
    }

    #[test]
    fn test_released_region_data_zeroed() {
        let config = ZeroCopyConfig {
            region_size: 64,
            max_regions: 10,
            alignment: 4096,
            preregister: 2,
        };
        let pool = RegionPool::new(config);

        // Acquire region and write data
        let mut region = pool.acquire().unwrap();
        region.as_mut_slice()[0] = 0xAB;
        region.as_mut_slice()[1] = 0xCD;

        // Release the region
        pool.release(region);

        // Acquire again - should be zeroed
        let new_region = pool.acquire().unwrap();

        // FINDING-TRANS-05: Info leak prevention
        // Verify data is zeroed (security feature)
        assert_eq!(new_region.as_slice()[0], 0);
        assert_eq!(new_region.as_slice()[1], 0);
    }

    #[test]
    fn test_pool_grow_within_limits() {
        let config = ZeroCopyConfig {
            region_size: 1024,
            max_regions: 5,
            alignment: 4096,
            preregister: 2,
        };
        let pool = RegionPool::new(config);

        // Try to grow beyond max
        let grown = pool.grow(10);

        // Should not exceed max_regions
        let total = pool.total();
        assert!(total <= 5);
        assert_eq!(grown, 3); // 2 + 3 = 5
    }

    #[test]
    fn test_pool_shrink_safety() {
        let config = ZeroCopyConfig {
            region_size: 1024,
            max_regions: 10,
            alignment: 4096,
            preregister: 5,
        };
        let pool = RegionPool::new(config);

        // Initial state: 5 pre-registered, 5 available
        assert_eq!(pool.total(), 5);
        assert_eq!(pool.available(), 5);

        // Acquire one (will be in use)
        let _region = pool.acquire().unwrap();

        // After acquire: 5 total, 4 available, 1 in use
        assert_eq!(pool.total(), 5);
        assert_eq!(pool.available(), 4);
        assert_eq!(pool.in_use(), 1);

        // Shrink should only affect available (idle) regions
        let shrunk = pool.shrink(3);

        // Should shrink 3 idle regions
        // Available: 4 - 3 = 1, Total: 5 - 3 = 2
        assert_eq!(shrunk, 3);
        assert_eq!(pool.available(), 1);
        assert_eq!(pool.in_use(), 1);

        // Can still acquire from remaining pool
        let remaining = pool.acquire();
        assert!(remaining.is_some());
    }

    #[test]
    fn test_pool_concurrent_acquire_release() {
        use std::sync::Arc;
        use std::thread;

        let config = ZeroCopyConfig {
            region_size: 1024,
            max_regions: 20,
            alignment: 4096,
            preregister: 10,
        };
        let pool = Arc::new(RegionPool::new(config));

        // Multiple threads acquiring and releasing
        let handles: Vec<_> = (0..4)
            .map(|i| {
                let pool = pool.clone();
                thread::spawn(move || {
                    let mut acquired = Vec::new();
                    for _ in 0..50 {
                        if let Some(mut region) = pool.acquire() {
                            // Write something
                            let slice = region.as_mut_slice();
                            slice[0] = i as u8;
                            acquired.push(region);
                        }
                    }
                    // Release
                    for region in acquired {
                        pool.release(region);
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        // Verify consistent state
        let stats = pool.stats();
        assert_eq!(
            stats.available_regions + stats.in_use_regions,
            stats.total_regions
        );
        assert_eq!(stats.in_use_regions, 0);
    }

    #[test]
    fn test_pool_stats_accurate() {
        let config = ZeroCopyConfig {
            region_size: 1024,
            max_regions: 10,
            alignment: 4096,
            preregister: 3,
        };
        let pool = RegionPool::new(config);

        // Initial stats
        let stats = pool.stats();
        assert_eq!(stats.total_regions, 3);
        assert_eq!(stats.available_regions, 3);
        assert_eq!(stats.in_use_regions, 0);

        // Acquire one
        let _r1 = pool.acquire().unwrap();
        let stats = pool.stats();
        assert_eq!(stats.in_use_regions, 1);
        assert_eq!(stats.available_regions, 2);

        // Acquire another
        let _r2 = pool.acquire().unwrap();
        let stats = pool.stats();
        assert_eq!(stats.in_use_regions, 2);

        // Release one
        // Note: we'd need to store the region to release it
        // This is just checking stats update during acquire
    }

    // ============================================================================
    // Category 3: Flow Control Security (6 tests)
    // ============================================================================

    #[test]
    fn test_flow_control_blocks_over_limit() {
        let config = FlowControlConfig {
            max_inflight_requests: 2,
            max_inflight_bytes: 10000,
            window_size: 256,
            high_watermark_pct: 80,
            low_watermark_pct: 50,
        };
        let controller = FlowController::new(config);

        // Acquire up to limit
        let p1 = controller.try_acquire(1000);
        let p2 = controller.try_acquire(1000);

        assert!(p1.is_some());
        assert!(p2.is_some());
        assert_eq!(controller.inflight_requests(), 2);

        // Should be blocked now
        let p3 = controller.try_acquire(1);
        assert!(p3.is_none());
    }

    #[test]
    fn test_flow_control_byte_limit() {
        let config = FlowControlConfig {
            max_inflight_requests: 100,
            max_inflight_bytes: 2000,
            window_size: 256,
            high_watermark_pct: 80,
            low_watermark_pct: 50,
        };
        let controller = FlowController::new(config);

        // Acquire up to byte limit
        let p1 = controller.try_acquire(1500);
        let p2 = controller.try_acquire(1000); // Would exceed

        assert!(p1.is_some());
        assert!(p2.is_none());
    }

    #[test]
    fn test_flow_control_release_restores() {
        let config = FlowControlConfig {
            max_inflight_requests: 2,
            max_inflight_bytes: 10000,
            window_size: 256,
            high_watermark_pct: 80,
            low_watermark_pct: 50,
        };
        let controller = FlowController::new(config);

        // Acquire
        let p1 = controller.try_acquire(1000).unwrap();
        let p2 = controller.try_acquire(1000).unwrap();

        assert_eq!(controller.inflight_requests(), 2);

        // Release one manually
        controller.release(1000);
        assert_eq!(controller.inflight_requests(), 1);
        assert_eq!(controller.inflight_bytes(), 1000);

        // Should be able to acquire again
        let p3 = controller.try_acquire(1000);
        assert!(p3.is_some());

        drop(p1);
        drop(p2);
        drop(p3);
    }

    #[test]
    fn test_flow_permit_drop_releases() {
        let config = FlowControlConfig::default();
        let controller = FlowController::new(config);

        {
            let _permit = controller.try_acquire(5000).unwrap();
            assert_eq!(controller.inflight_requests(), 1);
            assert_eq!(controller.inflight_bytes(), 5000);
        } // permit dropped here - RAII releases

        assert_eq!(controller.inflight_requests(), 0);
        assert_eq!(controller.inflight_bytes(), 0);
    }

    #[test]
    fn test_flow_control_state_transitions() {
        let config = FlowControlConfig {
            max_inflight_requests: 10,
            max_inflight_bytes: 1000,
            window_size: 256,
            high_watermark_pct: 80,
            low_watermark_pct: 50,
        };
        let controller = FlowController::new(config);

        // Initially Open
        assert_eq!(controller.state(), FlowControlState::Open);

        let mut permits = Vec::new();

        // Fill to 50% - should still be Open
        for _ in 0..5 {
            permits.push(controller.try_acquire(100).unwrap());
        }
        assert_eq!(controller.state(), FlowControlState::Open);

        // Fill to 80% - should be Throttled
        for _ in 0..3 {
            permits.push(controller.try_acquire(100).unwrap());
        }
        assert_eq!(controller.state(), FlowControlState::Throttled);

        // Fill to 100% - should be Blocked
        for _ in 0..2 {
            permits.push(controller.try_acquire(100).unwrap());
        }
        assert_eq!(controller.state(), FlowControlState::Blocked);
    }

    #[test]
    fn test_flow_control_zero_config() {
        let config = FlowControlConfig {
            max_inflight_requests: 0,
            max_inflight_bytes: 0,
            window_size: 256,
            high_watermark_pct: 80,
            low_watermark_pct: 50,
        };
        let controller = FlowController::new(config);

        // With zero limits, should block all requests
        let permit = controller.try_acquire(1);

        // FINDING-TRANS-06: Zero config edge case
        // Document behavior with zero limits
        if permit.is_none() {
            eprintln!("FINDING-TRANS-06: Zero config blocks all requests (correct)");
        }
        assert!(permit.is_none());
    }
}
