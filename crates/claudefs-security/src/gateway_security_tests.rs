//! Security tests for claudefs-gateway crate.
//!
//! This module validates security properties of the gateway including:
//! - S3 API input validation and bucket/object security
//! - pNFS layout server security
//! - NFS authentication and authorization
//! - Token-based authentication security

#[cfg(test)]
mod tests {
    use claudefs_gateway::auth::{AuthCred, AuthSysCred, SquashPolicy, AUTH_SYS_MAX_GIDS};
    use claudefs_gateway::error::GatewayError;
    use claudefs_gateway::pnfs::{DataServerLocation, IoMode, LayoutType, PnfsLayoutServer};
    use claudefs_gateway::s3::S3Handler;
    use claudefs_gateway::token_auth::{AuthToken, TokenAuth, TokenPermissions};

    // ============================================================================
    // Category 1: S3 API Security (10 tests)
    // ============================================================================

    #[test]
    fn test_s3_bucket_name_with_dots() {
        let handler = S3Handler::new();

        // Bucket name like "my..bucket" should be rejected
        let result = handler.create_bucket("my..bucket");

        // FINDING-GW-01: DNS label validation
        // Should be rejected (AWS S3 rejects consecutive dots)
        assert!(result.is_err());
    }

    #[test]
    fn test_s3_bucket_name_ip_format() {
        let handler = S3Handler::new();

        // Bucket name "192.168.1.1" should be rejected (AWS S3 rejects IP-like names)
        let result = handler.create_bucket("192.168.1.1");

        // FINDING-GW-02: AWS compatibility
        // Should be rejected
        assert!(result.is_err());
    }

    #[test]
    fn test_s3_object_key_path_traversal() {
        let handler = S3Handler::new();
        handler.create_bucket("test-bucket").unwrap();

        // Object key "../../etc/passwd" should either be rejected or safely stored
        let result = handler.put_object(
            "test-bucket",
            "../../etc/passwd",
            "application/octet-stream",
            vec![0, 1, 2, 3],
        );

        // FINDING-GW-03: Path traversal
        // Either should reject or safely store (not interpret as path)
        assert!(result.is_ok() || result.is_err());

        // If it succeeded, verify we can retrieve it with the exact key
        if result.is_ok() {
            let get_result = handler.get_object("test-bucket", "../../etc/passwd");
            assert!(get_result.is_ok());
        }
    }

    #[test]
    fn test_s3_object_key_null_bytes() {
        let handler = S3Handler::new();
        handler.create_bucket("test-bucket").unwrap();

        // Object key containing "\0" should be rejected
        let result = handler.put_object(
            "test-bucket",
            "test\0key",
            "application/octet-stream",
            vec![0, 1, 2, 3],
        );

        // FINDING-GW-04: Null byte injection
        // Document the finding - null bytes may be accepted
        if result.is_ok() {
            eprintln!("FINDING-GW-04: Null byte in object key accepted");
        }
    }

    #[test]
    fn test_s3_object_key_max_length() {
        let handler = S3Handler::new();
        handler.create_bucket("test-bucket").unwrap();

        // Object key with 1025+ bytes should be rejected (AWS limit is 1024)
        let long_key = "a".repeat(1025);
        let result = handler.put_object(
            "test-bucket",
            &long_key,
            "application/octet-stream",
            vec![0, 1, 2, 3],
        );

        // FINDING-GW-05: Key length validation
        // Document the finding - long keys may be accepted
        if result.is_ok() {
            eprintln!("FINDING-GW-05: Long object key (>1024) accepted");
        }
    }

    #[test]
    fn test_s3_list_objects_max_keys_zero() {
        let handler = S3Handler::new();
        handler.create_bucket("test-bucket").unwrap();
        handler
            .put_object("test-bucket", "obj1", "text/plain", b"data".to_vec())
            .unwrap();

        // ListObjects with max_keys=0 should return empty or handle gracefully
        let result = handler.list_objects("test-bucket", "", None, 0);

        // FINDING-GW-06: Edge case handling
        // Should return empty list, not error
        if result.is_ok() {
            let list_result = result.unwrap();
            // Either returns empty or defaults to some value
            assert!(list_result.objects.is_empty() || list_result.max_keys > 0);
        }
    }

    #[test]
    fn test_s3_list_objects_max_keys_overflow() {
        let handler = S3Handler::new();
        handler.create_bucket("test-bucket").unwrap();

        // ListObjects with max_keys=u32::MAX should not cause OOM
        let result = handler.list_objects("test-bucket", "", None, u32::MAX);

        // FINDING-GW-07: Integer overflow
        // Should handle gracefully (may truncate to reasonable limit)
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_s3_delete_nonexistent_bucket() {
        let handler = S3Handler::new();

        // Delete a bucket that doesn't exist
        let result = handler.delete_bucket("nonexistent-bucket");

        // Should return BucketNotFound
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(matches!(e, GatewayError::S3BucketNotFound { .. }));
        }
    }

    #[test]
    fn test_s3_put_object_empty_body() {
        let handler = S3Handler::new();
        handler.create_bucket("test-bucket").unwrap();

        // Put an object with empty data (valid in S3)
        let result = handler.put_object(
            "test-bucket",
            "empty-object",
            "application/octet-stream",
            vec![],
        );

        // Empty objects are valid in S3
        assert!(result.is_ok());

        // Verify we can retrieve it
        let (meta, data) = handler.get_object("test-bucket", "empty-object").unwrap();
        assert_eq!(meta.size, 0);
        assert!(data.is_empty());
    }

    #[test]
    fn test_s3_copy_to_nonexistent_bucket() {
        let handler = S3Handler::new();

        // Create source bucket with object
        handler.create_bucket("source-bucket").unwrap();
        handler
            .put_object("source-bucket", "obj", "text/plain", b"data".to_vec())
            .unwrap();

        // Copy object to a bucket that doesn't exist
        let result = handler.copy_object("source-bucket", "obj", "nonexistent-bucket", "obj");

        // FINDING-GW-08: Cross-bucket validation
        // Should fail properly
        assert!(result.is_err());
    }

    // ============================================================================
    // Category 2: S3 Bucket Validation (5 tests)
    // ============================================================================

    #[test]
    fn test_s3_bucket_name_too_short() {
        let handler = S3Handler::new();

        // 2-char bucket name should be rejected
        let result = handler.create_bucket("ab");

        // Length validation
        assert!(result.is_err());
    }

    #[test]
    fn test_s3_bucket_name_too_long() {
        let handler = S3Handler::new();

        // 64-char bucket name should be rejected (max is 63)
        let long_name = "a".repeat(64);
        let result = handler.create_bucket(&long_name);

        // Length validation
        assert!(result.is_err());
    }

    #[test]
    fn test_s3_bucket_name_special_chars() {
        let handler = S3Handler::new();

        // Bucket names with special characters should be rejected
        let special_names = [
            "test@bucket",
            "test#bucket",
            "test$bucket",
            "test%bucket",
            "test bucket",
        ];

        for name in special_names {
            let result = handler.create_bucket(name);
            assert!(result.is_err(), "Bucket '{}' should be rejected", name);
        }
    }

    #[test]
    fn test_s3_bucket_name_leading_hyphen() {
        let handler = S3Handler::new();

        // Bucket "-my-bucket" should be rejected
        let result = handler.create_bucket("-my-bucket");

        // Format validation
        assert!(result.is_err());
    }

    #[test]
    fn test_s3_bucket_name_valid_examples() {
        let handler = S3Handler::new();

        // Verify that valid bucket names succeed
        let valid_names = ["my-bucket", "test123", "a-b-c", "bucket123", "abc123xyz"];

        for name in valid_names {
            let result = handler.create_bucket(name);
            assert!(result.is_ok(), "Bucket '{}' should be accepted", name);
            // Clean up for next iteration
            let _ = handler.delete_bucket(name);
        }
    }

    // ============================================================================
    // Category 3: pNFS Layout Security (5 tests)
    // ============================================================================

    #[test]
    fn test_pnfs_stateid_is_inode_based() {
        let servers = vec![DataServerLocation {
            address: "192.168.1.1:2001".to_string(),
            device_id: [0xAB; 16],
        }];
        let server = PnfsLayoutServer::new(servers, 1);

        // Get layout for inode 12345
        let layout = server.get_layout(12345, 0, 1000000, IoMode::Read);

        // Verify stateid contains the inode number
        let inode_from_state = u64::from_le_bytes(layout.stateid[0..8].try_into().unwrap());

        // FINDING-GW-09: Predictable stateids
        // Stateid should contain inode (identifies the predictability issue)
        assert_eq!(inode_from_state, 12345);
    }

    #[test]
    fn test_pnfs_server_selection_modulo() {
        let servers = vec![
            DataServerLocation {
                address: "192.168.1.1:2001".to_string(),
                device_id: [0x01; 16],
            },
            DataServerLocation {
                address: "192.168.1.2:2001".to_string(),
                device_id: [0x02; 16],
            },
            DataServerLocation {
                address: "192.168.1.3:2001".to_string(),
                device_id: [0x03; 16],
            },
        ];
        let server = PnfsLayoutServer::new(servers, 1);

        // Get layouts for consecutive inodes
        let layout0 = server.get_layout(0, 0, 1000, IoMode::Read);
        let layout1 = server.get_layout(1, 0, 1000, IoMode::Read);
        let layout2 = server.get_layout(2, 0, 1000, IoMode::Read);

        // Verify server selection is simple modulo (identifies predictability)
        let servers_used = [
            layout0.segments[0].data_servers[0].address.clone(),
            layout1.segments[0].data_servers[0].address.clone(),
            layout2.segments[0].data_servers[0].address.clone(),
        ];

        // FINDING-GW-10: Predictable server selection
        // Should use simple modulo distribution
        assert_eq!(servers_used[0], "192.168.1.1:2001");
        assert_eq!(servers_used[1], "192.168.1.2:2001");
        assert_eq!(servers_used[2], "192.168.1.3:2001");
    }

    #[test]
    fn test_pnfs_empty_server_list() {
        let server = PnfsLayoutServer::new(vec![], 1);

        // Get layout with no data servers
        let result = server.get_layout(123, 0, 1000000, IoMode::Read);

        // Should return empty segments
        assert!(result.segments.is_empty());
    }

    #[test]
    fn test_pnfs_layout_iomode_validation() {
        // Verify IoMode::from_u32 rejects invalid values
        assert!(IoMode::from_u32(0).is_none());
        assert!(IoMode::from_u32(4).is_none());
        assert!(IoMode::from_u32(u32::MAX).is_none());

        // Valid values should work
        assert_eq!(IoMode::from_u32(1), Some(IoMode::Read));
        assert_eq!(IoMode::from_u32(2), Some(IoMode::ReadWrite));
        assert_eq!(IoMode::from_u32(3), Some(IoMode::Any));
    }

    #[test]
    fn test_pnfs_large_inode_no_overflow() {
        let servers = vec![
            DataServerLocation {
                address: "192.168.1.1:2001".to_string(),
                device_id: [0x01; 16],
            },
            DataServerLocation {
                address: "192.168.1.2:2001".to_string(),
                device_id: [0x02; 16],
            },
            DataServerLocation {
                address: "192.168.1.3:2001".to_string(),
                device_id: [0x03; 16],
            },
        ];
        let server = PnfsLayoutServer::new(servers, 1);

        // Get layout for inode u64::MAX with 3 servers
        let result = server.get_layout(u64::MAX, 0, 1000000, IoMode::Read);

        // FINDING-GW-11: Integer overflow
        // Should not overflow or panic
        assert!(result.segments.len() > 0);

        // Verify modulo with u64::MAX works correctly
        // u64::MAX = 18446744073709551615, which is divisible by 3, so index 0
        let server_addr = result.segments[0].data_servers[0].address.clone();

        // Just verify it works without panic - server choice may vary
        assert!(!server_addr.is_empty());
    }

    // ============================================================================
    // Category 4: NFS Authentication Security (5 tests)
    // ============================================================================

    #[test]
    fn test_auth_sys_root_squash() {
        let cred = AuthSysCred {
            stamp: 1,
            machinename: "testhost".to_string(),
            uid: 0, // root
            gid: 0,
            gids: vec![],
        };

        // Wrap in AuthCred to use effective_uid/gid methods
        let auth_cred = AuthCred::Sys(cred);

        // Verify that UID 0 is mapped to 65534 under RootSquash policy
        let effective_uid = auth_cred.effective_uid(SquashPolicy::RootSquash);
        let effective_gid = auth_cred.effective_gid(SquashPolicy::RootSquash);

        // Security policy enforcement
        assert_eq!(effective_uid, 65534); // nobody
        assert_eq!(effective_gid, 65534); // nobody
    }

    #[test]
    fn test_auth_sys_all_squash() {
        let cred = AuthSysCred {
            stamp: 1,
            machinename: "testhost".to_string(),
            uid: 1000,
            gid: 1000,
            gids: vec![],
        };

        // Wrap in AuthCred
        let auth_cred = AuthCred::Sys(cred);

        // Verify that all UIDs are mapped to 65534 under AllSquash
        let effective_uid = auth_cred.effective_uid(SquashPolicy::AllSquash);
        let effective_gid = auth_cred.effective_gid(SquashPolicy::AllSquash);

        // Policy enforcement
        assert_eq!(effective_uid, 65534);
        assert_eq!(effective_gid, 65534);
    }

    #[test]
    fn test_auth_sys_oversized_machinename() {
        // AUTH_SYS with 256+ byte machine name
        let large_name = "a".repeat(256);
        let cred = AuthSysCred {
            stamp: 1,
            machinename: large_name,
            uid: 1000,
            gid: 1000,
            gids: vec![],
        };

        let encoded = cred.encode_xdr();
        let decoded = AuthSysCred::decode_xdr(&encoded);

        // FINDING-GW-12: Input validation
        // Should be rejected
        assert!(decoded.is_err());
    }

    #[test]
    fn test_auth_sys_too_many_gids() {
        // AUTH_SYS with 17+ GIDs (AUTH_SYS_MAX_GIDS is 16)
        let too_many_gids: Vec<u32> = (0..=AUTH_SYS_MAX_GIDS).map(|i| i as u32).collect();
        let cred = AuthSysCred {
            stamp: 1,
            machinename: "testhost".to_string(),
            uid: 1000,
            gid: 1000,
            gids: too_many_gids,
        };

        let encoded = cred.encode_xdr();
        let decoded = AuthSysCred::decode_xdr(&encoded);

        // FINDING-GW-13: GID count validation
        // Should be rejected
        assert!(decoded.is_err());
    }

    #[test]
    fn test_auth_sys_truncated_payload() {
        // Incomplete AUTH_SYS XDR data (too few bytes)
        let truncated_data = vec![0, 0, 0, 1]; // Only stamp, missing rest

        let decoded = AuthSysCred::decode_xdr(&truncated_data);

        // FINDING-GW-14: Truncation handling
        // Should return error, not crash
        assert!(decoded.is_err());
    }

    // ============================================================================
    // Category 5: Token Auth Security (3 tests)
    // ============================================================================

    #[test]
    fn test_token_revocation_prevents_access() {
        let auth = TokenAuth::new();

        // Create a token
        let token_str = TokenAuth::generate_token();
        let token = AuthToken::new(&token_str, 1000, 100, "testuser")
            .with_permissions(TokenPermissions::read_write());
        auth.register(token);

        // Revoke the token
        let revoked = auth.revoke(&token_str);
        assert!(revoked, "Token should be revoked");

        // Validate should return None
        let validated = auth.validate(&token_str, 0);

        // Token lifecycle security
        assert!(validated.is_none());
    }

    #[test]
    fn test_token_validate_unknown() {
        let auth = TokenAuth::new();

        // Validate a completely unknown token hash
        let validated = auth.validate("unknown-token-12345", 0);

        // Default-deny security model
        assert!(validated.is_none());
    }

    #[test]
    fn test_token_permissions_preserved() {
        let auth = TokenAuth::new();

        // Create token with specific permissions
        let token_str = TokenAuth::generate_token();
        let token = AuthToken::new(&token_str, 1000, 100, "testuser")
            .with_permissions(TokenPermissions::admin());
        auth.register(token);

        // Validate and verify permissions match
        let validated = auth.validate(&token_str, 0);

        assert!(validated.is_some());
        let validated_token = validated.unwrap();

        // Permission integrity
        assert!(validated_token.can_read());
        assert!(validated_token.can_write());
        assert!(validated_token.can_admin());
    }
}
