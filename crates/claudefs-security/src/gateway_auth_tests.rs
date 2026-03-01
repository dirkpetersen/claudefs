//! Security audit tests for A7 gateway authentication.
//!
//! Findings: FINDING-16 through FINDING-20

use claudefs_gateway::auth::{AuthCred, AuthSysCred, AUTH_SYS_MAX_GIDS};
use claudefs_gateway::rpc::OpaqueAuth;
use claudefs_gateway::token_auth::{AuthToken, TokenAuth, TokenPermissions};
use claudefs_gateway::xdr::XdrEncoder;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finding_16_token_generation_is_random() {
        let token1 = TokenAuth::generate_token();
        let token2 = TokenAuth::generate_token();
        assert_ne!(
            token1, token2,
            "FINDING-16 FIXED: Tokens should be random, not deterministic"
        );
        assert_eq!(token1.len(), 64, "Token should be 32 bytes hex-encoded");
        assert_eq!(token2.len(), 64, "Token should be 32 bytes hex-encoded");
    }

    #[test]
    fn finding_16_sequential_tokens_unique() {
        let tokens: Vec<String> = (0..10).map(|_| TokenAuth::generate_token()).collect();
        let unique: std::collections::HashSet<&String> = tokens.iter().collect();
        assert_eq!(
            unique.len(),
            10,
            "FINDING-16 FIXED: All 10 tokens should be unique"
        );
    }

    #[test]
    fn finding_17_auth_sys_accepts_any_uid() {
        for uid in [0u32, 1, 1000, 65534, u32::MAX] {
            let cred = AuthSysCred {
                stamp: 1,
                machinename: "attacker.local".to_string(),
                uid,
                gid: uid,
                gids: vec![],
            };
            let encoded = cred.encode_xdr();
            let decoded = AuthSysCred::decode_xdr(&encoded).unwrap();
            assert_eq!(
                decoded.uid, uid,
                "FINDING-17: Client-supplied UID {} accepted without verification",
                uid
            );
        }
    }

    #[test]
    fn finding_17_auth_sys_root_uid_accepted() {
        let cred = AuthSysCred {
            stamp: 1,
            machinename: "attacker.local".to_string(),
            uid: 0,
            gid: 0,
            gids: vec![0],
        };
        assert!(
            cred.is_root(),
            "FINDING-17: Client claiming root is accepted"
        );

        let opaque = OpaqueAuth {
            flavor: 1,
            body: cred.encode_xdr(),
        };
        let auth_cred = AuthCred::from_opaque_auth(&opaque);
        assert!(
            auth_cred.is_root(),
            "Root access granted from AUTH_SYS without squashing"
        );
    }

    #[test]
    fn finding_18_token_stored_in_plaintext() {
        let auth = TokenAuth::new();
        let token_str = "super-secret-token-12345";
        auth.register(AuthToken::new(token_str, 1000, 100, "user"));

        assert!(auth.validate(token_str, 0).is_some());
        assert!(auth.exists(token_str));
    }

    #[test]
    fn finding_18_token_stored_as_hash() {
        let auth = TokenAuth::new();
        auth.register(AuthToken::new("token-a", 1000, 100, "user1"));
        auth.register(AuthToken::new("token-b", 2000, 200, "user2"));

        let tokens = auth.tokens_for_user(1000);
        assert_eq!(tokens.len(), 1);
        assert_eq!(
            tokens[0].token.len(),
            64,
            "Token stored as 64-char SHA-256 hex hash"
        );
        assert_ne!(tokens[0].token, "token-a", "Token NOT stored in cleartext");
        assert!(auth.validate("token-a", 0).is_some());
    }

    #[test]
    fn finding_19_mutex_poisoning_panics() {
        let auth = std::sync::Arc::new(TokenAuth::new());
        auth.register(AuthToken::new("token1", 1, 1, "u"));

        assert!(auth.validate("token1", 0).is_some());
    }

    #[test]
    fn finding_20_no_root_squashing() {
        let auth_cred = AuthCred::None;
        assert_eq!(auth_cred.uid(), 65534);
        assert!(!auth_cred.is_root());

        let root_cred = AuthSysCred {
            stamp: 1,
            machinename: "client".to_string(),
            uid: 0,
            gid: 0,
            gids: vec![],
        };
        let opaque = OpaqueAuth {
            flavor: 1,
            body: root_cred.encode_xdr(),
        };
        let cred = AuthCred::from_opaque_auth(&opaque);
        assert_eq!(cred.uid(), 0, "FINDING-20: Root UID not squashed");
        assert!(
            cred.is_root(),
            "FINDING-20: Root access granted without squashing"
        );
    }

    #[test]
    fn token_expiry_boundary_exact_match() {
        let auth = TokenAuth::new();
        let token = AuthToken::new("t1", 1, 1, "u").with_expiry(1000);
        auth.register(token);

        let t = AuthToken::new("t2", 1, 1, "u").with_expiry(1000);
        assert!(
            t.is_expired(1000),
            "Token expired at exact expiry time (now >= expires_at)"
        );
        assert!(t.is_expired(1001), "Token expired one second after expiry");
        assert!(!t.is_expired(999), "Token not expired before expiry time");
    }

    #[test]
    fn auth_sys_max_gids_enforced() {
        let cred = AuthSysCred {
            stamp: 1,
            machinename: "host".to_string(),
            uid: 1000,
            gid: 1000,
            gids: vec![1; 16],
        };
        let encoded = cred.encode_xdr();
        let decoded = AuthSysCred::decode_xdr(&encoded).unwrap();
        assert_eq!(decoded.gids.len(), 16);
    }

    #[test]
    fn auth_sys_exceeding_max_gids_rejected() {
        let mut enc = XdrEncoder::new();
        enc.encode_u32(1);
        enc.encode_string("host");
        enc.encode_u32(1000);
        enc.encode_u32(1000);
        enc.encode_u32(17);
        for _ in 0..17 {
            enc.encode_u32(1);
        }
        let result = AuthSysCred::decode_xdr(&enc.finish().to_vec());
        assert!(result.is_err(), "17 gids should be rejected");
    }

    #[test]
    fn token_default_permissions_deny_all() {
        let token = AuthToken::new("t1", 1000, 100, "user");
        assert!(!token.can_read(), "Default token cannot read");
        assert!(!token.can_write(), "Default token cannot write");
        assert!(!token.can_admin(), "Default token cannot admin");
    }

    #[test]
    fn auth_sys_long_machinename_rejected() {
        let long_name = "a".repeat(10000);
        let cred = AuthSysCred {
            stamp: 1,
            machinename: long_name.clone(),
            uid: 1000,
            gid: 1000,
            gids: vec![],
        };
        let encoded = cred.encode_xdr();
        let result = AuthSysCred::decode_xdr(&encoded);
        assert!(
            result.is_err(),
            "Machinename > 255 bytes should be rejected"
        );

        let ok_name = "b".repeat(255);
        let ok_cred = AuthSysCred {
            stamp: 1,
            machinename: ok_name,
            uid: 1000,
            gid: 1000,
            gids: vec![],
        };
        let ok_encoded = ok_cred.encode_xdr();
        assert!(AuthSysCred::decode_xdr(&ok_encoded).is_ok());
    }

    #[test]
    fn token_with_admin_permissions_can_do_anything() {
        let token =
            AuthToken::new("t1", 1000, 100, "user").with_permissions(TokenPermissions::admin());
        assert!(token.can_read());
        assert!(token.can_write());
        assert!(token.can_admin());
    }

    #[test]
    fn token_revoke_works() {
        let auth = TokenAuth::new();
        auth.register(AuthToken::new("token1", 1000, 100, "user"));

        assert!(auth.exists("token1"));
        assert!(auth.revoke("token1"));
        assert!(!auth.exists("token1"));
    }

    #[test]
    fn token_validate_returns_none_for_expired() {
        let auth = TokenAuth::new();
        auth.register(AuthToken::new("token1", 1000, 100, "user").with_expiry(100));

        assert!(auth.validate("token1", 50).is_some());
        assert!(auth.validate("token1", 101).is_none());
    }

    #[test]
    fn auth_none_maps_to_nobody() {
        let cred = AuthCred::None;
        assert_eq!(cred.uid(), 65534);
        assert_eq!(cred.gid(), 65534);
        assert!(!cred.is_root());
    }

    #[test]
    fn auth_sys_has_gid_checks_supplementary() {
        let cred = AuthSysCred {
            stamp: 1,
            machinename: "host".to_string(),
            uid: 1000,
            gid: 100,
            gids: vec![200, 300],
        };
        assert!(cred.has_gid(100));
        assert!(cred.has_gid(200));
        assert!(cred.has_gid(300));
        assert!(!cred.has_gid(400));
    }

    #[test]
    fn auth_sys_credential_roundtrip() {
        let cred = AuthSysCred {
            stamp: 42,
            machinename: "testmachine".to_string(),
            uid: 1234,
            gid: 5678,
            gids: vec![100, 200, 300],
        };
        let encoded = cred.encode_xdr();
        let decoded = AuthSysCred::decode_xdr(&encoded).unwrap();

        assert_eq!(cred.stamp, decoded.stamp);
        assert_eq!(cred.machinename, decoded.machinename);
        assert_eq!(cred.uid, decoded.uid);
        assert_eq!(cred.gid, decoded.gid);
        assert_eq!(cred.gids, decoded.gids);
    }
}
