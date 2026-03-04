//! Security tests for claudefs-fuse crate.
//!
//! This module validates security properties of the FUSE client including:
//! - Client authentication and certificate management
//! - Path resolution and validation
//! - Mount options security
//! - Passthrough file descriptor management

#[cfg(test)]
mod tests {
    use claudefs_fuse::client_auth::{AuthState, CertRecord, ClientAuthManager};
    use claudefs_fuse::inode::InodeId;
    use claudefs_fuse::mount::{parse_mount_options, MountError, MountOptions};
    use claudefs_fuse::passthrough::{PassthroughConfig, PassthroughState};
    use claudefs_fuse::path_resolver::{PathResolveError, PathResolver, PathResolverConfig};
    use std::time::{Duration, Instant};

    // ============================================================================
    // Category 1: Client Authentication (8 tests)
    // ============================================================================

    #[test]
    fn test_enrollment_empty_token() {
        let mut mgr = ClientAuthManager::new("/tmp/cfs");
        let result = mgr.begin_enrollment("", 1000);

        // FINDING-FUSE-01: No token validation
        // Document if empty token is accepted (security gap)
        if result.is_ok() {
            eprintln!("FINDING-FUSE-01: Empty token accepted by begin_enrollment");
        }
    }

    #[test]
    fn test_enrollment_trivial_token() {
        let mut mgr = ClientAuthManager::new("/tmp/cfs");
        let result = mgr.begin_enrollment("a", 1000);

        // FINDING-FUSE-01: Trivial token acceptance
        // Document if 1-character token is accepted
        if result.is_ok() {
            eprintln!("FINDING-FUSE-01: Trivial 1-char token 'a' accepted");
        }
    }

    #[test]
    fn test_enrollment_while_enrolled() {
        let mut mgr = ClientAuthManager::new("/tmp/cfs");

        // First enroll
        mgr.begin_enrollment("token123", 1000).unwrap();
        mgr.complete_enrollment(
            "-----BEGIN CERTIFICATE-----\n/CN=cfs-client-uuid\n-----END CERTIFICATE-----",
            "-----BEGIN PRIVATE KEY-----",
            1000,
        )
        .unwrap();

        // Try to enroll again while already enrolled
        let result = mgr.begin_enrollment("token456", 2000);

        // FINDING-FUSE-02: State machine bypass
        // Should be rejected (AlreadyEnrolled error)
        if result.is_ok() {
            eprintln!("FINDING-FUSE-02: begin_enrollment accepted while already enrolled");
        }
        assert!(result.is_err());
    }

    #[test]
    fn test_double_enrollment_complete() {
        let mut mgr = ClientAuthManager::new("/tmp/cfs");

        // First enrollment
        mgr.begin_enrollment("token123", 1000).unwrap();
        mgr.complete_enrollment(
            "-----BEGIN CERTIFICATE-----\n/CN=cfs-client-1\n-----END CERTIFICATE-----",
            "-----BEGIN PRIVATE KEY-----",
            1000,
        )
        .unwrap();

        // Try to complete enrollment again (should fail - not in Enrolling state)
        let result = mgr.complete_enrollment(
            "-----BEGIN CERTIFICATE-----\n/CN=cfs-client-2\n-----END CERTIFICATE-----",
            "-----BEGIN PRIVATE KEY-----",
            2000,
        );

        // State machine should reject this
        assert!(result.is_err());
    }

    #[test]
    fn test_revoked_then_re_enroll() {
        let mut mgr = ClientAuthManager::new("/tmp/cfs");

        // Enroll and get certificate
        mgr.begin_enrollment("token123", 1000).unwrap();
        mgr.complete_enrollment(
            "-----BEGIN CERTIFICATE-----\n/CN=cfs-client-uuid\n-----END CERTIFICATE-----",
            "-----BEGIN PRIVATE KEY-----",
            1000,
        )
        .unwrap();

        // Revoke the certificate
        mgr.revoke("compromised", 2000);

        // Check state is Revoked
        assert!(matches!(mgr.state(), AuthState::Revoked { .. }));

        // Try to enroll again
        let result = mgr.begin_enrollment("new_token", 3000);

        // FINDING-FUSE-03: Post-revocation re-enrollment
        // Document if re-enrollment is allowed after revocation
        if result.is_ok() {
            eprintln!("FINDING-FUSE-03: Re-enrollment allowed after revocation");
        }
    }

    #[test]
    fn test_crl_growth_unbounded() {
        let mut mgr = ClientAuthManager::new("/tmp/cfs");

        // Add many entries to CRL (1000+)
        let initial_len = mgr.crl_len();
        for i in 0..1500 {
            let mut fp = [0u8; 32];
            fp[0] = (i & 0xFF) as u8;
            fp[1] = ((i >> 8) & 0xFF) as u8;
            mgr.add_to_crl(fp, "test-reason", 1000 + i as u64);
        }

        let final_len = mgr.crl_len();

        // FINDING-FUSE-04: CRL unbounded growth
        // Document that CRL can grow without explicit limit
        eprintln!(
            "FINDING-FUSE-04: CRL grew from {} to {} entries (unbounded)",
            initial_len, final_len
        );
        assert!(final_len > 1000);
    }

    #[test]
    fn test_crl_compact_removes_old() {
        let mut mgr = ClientAuthManager::new("/tmp/cfs");

        // Add old and new entries
        let old_fp: [u8; 32] = [0x11; 32];
        let new_fp: [u8; 32] = [0x22; 32];

        mgr.add_to_crl(old_fp, "old", 100);
        mgr.add_to_crl(new_fp, "new", 500);

        // Compact with max_age_secs = 300 (should remove entry at 100)
        let removed = mgr.compact_crl(500, 300);

        // Verify old entry was removed
        assert_eq!(removed, 1);
        assert_eq!(mgr.crl_len(), 1);
        assert!(!mgr.is_revoked(&old_fp));
        assert!(mgr.is_revoked(&new_fp));
    }

    #[test]
    fn test_fingerprint_collision_weakness() {
        let mut mgr = ClientAuthManager::new("/tmp/cfs");

        // Create two PEMs that differ only slightly
        let pem1 = "-----BEGIN CERTIFICATE-----\n/CN=client1\n-----END CERTIFICATE-----";
        let pem2 = "-----BEGIN CERTIFICATE-----\n/CN=client2\n-----END CERTIFICATE-----";

        // Complete enrollment with first
        mgr.begin_enrollment("token1", 1000).unwrap();
        mgr.complete_enrollment(pem1, "key1", 1000).unwrap();

        let fp1 = mgr.cert().unwrap().fingerprint;

        // Now create a second manager and try different PEM
        let mut mgr2 = ClientAuthManager::new("/tmp/cfs2");
        mgr2.begin_enrollment("token2", 1000).unwrap();
        mgr2.complete_enrollment(pem2, "key2", 1000).unwrap();

        let fp2 = mgr2.cert().unwrap().fingerprint;

        // FINDING-FUSE-05: Weak fingerprint hash
        // The fingerprint uses a simple additive hash (mod 32) - potential for collisions
        if fp1 == fp2 {
            eprintln!("FINDING-FUSE-05: Fingerprint collision detected!");
        }
    }

    // ============================================================================
    // Category 2: Path Resolution Security (6 tests)
    // ============================================================================

    #[test]
    fn test_validate_path_dotdot() {
        // Test path with ".." component
        let result = PathResolver::validate_path("a/../b");

        // FINDING-FUSE-06: Path traversal
        // Should reject paths with ".."
        if result.is_ok() {
            eprintln!("FINDING-FUSE-06: Path traversal 'a/../b' was accepted");
        } else {
            assert!(matches!(
                result.unwrap_err(),
                PathResolveError::InvalidPath { reason }
                if reason.contains("..")
            ));
        }
    }

    #[test]
    fn test_validate_path_empty() {
        let result = PathResolver::validate_path("");

        // Should reject empty path
        if result.is_err() {
            assert!(matches!(
                result.unwrap_err(),
                PathResolveError::InvalidPath { reason }
                if reason.contains("empty")
            ));
        } else {
            eprintln!("FINDING-FUSE-06b: Empty path was accepted");
        }
    }

    #[test]
    fn test_validate_path_absolute() {
        let result = PathResolver::validate_path("/absolute/path");

        // FINDING-FUSE-07: Absolute path injection
        // Should reject absolute paths
        if result.is_ok() {
            eprintln!("FINDING-FUSE-07: Absolute path '/absolute/path' was accepted");
        } else {
            assert!(matches!(
                result.unwrap_err(),
                PathResolveError::InvalidPath { reason }
                if reason.contains("absolute")
            ));
        }
    }

    #[test]
    fn test_validate_path_deeply_nested() {
        // Create a path with 200+ components
        let deep_path = "a/".repeat(250);
        let result = PathResolver::validate_path(&deep_path);

        // PathResolver default max_depth is 64, so this should fail
        // But we need to check validate_path respects max_depth
        // Note: validate_path doesn't check depth, only component parsing
        if result.is_ok() {
            let segments = result.unwrap();
            eprintln!(
                "FINDING-FUSE-07b: Deep path with {} segments accepted",
                segments.len()
            );
        }
    }

    #[test]
    fn test_cache_invalidation_prefix() {
        let config = PathResolverConfig {
            max_depth: 64,
            cache_capacity: 100,
            ttl: Duration::from_secs(30),
        };
        let mut resolver = PathResolver::new(config);

        // Insert entries
        resolver.insert(
            "a/b",
            claudefs_fuse::path_resolver::ResolvedPath {
                path: "a/b".to_string(),
                components: vec![],
                final_ino: 2u64,
                resolved_at: Instant::now(),
            },
        );
        resolver.insert(
            "a/c",
            claudefs_fuse::path_resolver::ResolvedPath {
                path: "a/c".to_string(),
                components: vec![],
                final_ino: 3u64,
                resolved_at: Instant::now(),
            },
        );
        resolver.insert(
            "x/y",
            claudefs_fuse::path_resolver::ResolvedPath {
                path: "x/y".to_string(),
                components: vec![],
                final_ino: 4u64,
                resolved_at: Instant::now(),
            },
        );

        // Invalidate prefix "a"
        resolver.invalidate_prefix("a");

        // Verify lookups return None for invalidated entries
        assert!(resolver.lookup("a/b").is_none());
        assert!(resolver.lookup("a/c").is_none());
        // x/y should still be accessible
        assert!(resolver.lookup("x/y").is_some());
    }

    #[test]
    fn test_generation_tracking_bump() {
        let config = PathResolverConfig::default();
        let mut resolver = PathResolver::new(config);

        let ino: InodeId = 100;

        // Insert a path with generation 1
        resolver.insert(
            "test",
            claudefs_fuse::path_resolver::ResolvedPath {
                path: "test".to_string(),
                components: vec![claudefs_fuse::path_resolver::ResolvedComponent {
                    name: "test".to_string(),
                    ino,
                    parent_ino: 1u64,
                    generation: 1,
                }],
                final_ino: ino,
                resolved_at: Instant::now(),
            },
        );

        // Check is_generation_current with matching generation
        assert!(resolver.is_generation_current(ino, 1));

        // Bump generation
        resolver.bump_generation(ino);

        // Now old generation should be stale
        assert!(!resolver.is_generation_current(ino, 1));
        // New generation should match
        assert!(resolver.is_generation_current(ino, 2));
    }

    // ============================================================================
    // Category 3: Mount Options Security (3 tests)
    // ============================================================================

    #[test]
    fn test_mount_allow_other_default() {
        let opts = MountOptions::default();

        // FINDING-FUSE-08: Security-critical defaults
        // allow_other defaults to false (secure)
        if opts.allow_other {
            eprintln!("FINDING-FUSE-08: allow_other is true by default!");
        }
        assert!(!opts.allow_other);
    }

    #[test]
    fn test_mount_parse_invalid_option() {
        let result = parse_mount_options("foobar");

        // Should return InvalidOption error
        assert!(matches!(result, Err(MountError::InvalidOption(_))));
    }

    #[test]
    fn test_mount_default_permissions() {
        let opts = MountOptions::default();

        // FINDING-FUSE-09: default_permissions security
        // This defaults to false, which could be a security concern
        if !opts.default_permissions {
            eprintln!(
                "FINDING-FUSE-09: default_permissions is false by default (security consideration)"
            );
        }
        assert!(!opts.default_permissions);
    }

    // ============================================================================
    // Category 4: Passthrough FD Security (3 tests)
    // ============================================================================

    #[test]
    fn test_passthrough_fd_overwrite() {
        let config = PassthroughConfig::default();
        let mut state = PassthroughState::new(&config);

        // Register same fh twice with different fds
        state.register_fd(1, 10);
        state.register_fd(1, 20);

        // FINDING-FUSE-10: FD leak
        // Document that old fd is silently overwritten (potential fd leak)
        let fd = state.get_fd(1);
        if fd == Some(20) {
            eprintln!("FINDING-FUSE-10: FD silently overwritten (old fd 10 leaked)");
        }
        assert_eq!(fd, Some(20));
    }

    #[test]
    fn test_passthrough_get_nonexistent() {
        let config = PassthroughConfig::default();
        let state = PassthroughState::new(&config);

        // Get fd for unregistered fh
        let fd = state.get_fd(999);

        // Should return None
        assert_eq!(fd, None);
    }

    #[test]
    fn test_passthrough_unregister_twice() {
        let config = PassthroughConfig::default();
        let mut state = PassthroughState::new(&config);

        // Register and unregister
        state.register_fd(1, 10);
        let fd1 = state.unregister_fd(1);
        assert_eq!(fd1, Some(10));

        // Unregister again
        let fd2 = state.unregister_fd(1);
        assert_eq!(fd2, None);
    }
}
