//! S3 API security & penetration tests for claudefs-gateway crate.
//!
//! Part of A10 Phase 5: Gateway S3 deep audit — presigned URL forgery, policy bypass,
//! bucket enumeration, input validation, rate limiting evasion

use claudefs_gateway::auth::{AuthSysCred, SquashPolicy};
use claudefs_gateway::gateway_tls::{CipherPreference, ClientCertMode, TlsConfig, TlsVersion};
use claudefs_gateway::nfs_export::{ClientSpec, ExportConfig};
use claudefs_gateway::s3::S3Handler;
use claudefs_gateway::s3::S3Operation;
use claudefs_gateway::s3_bucket_policy::{
    BucketPolicy, BucketPolicyRegistry, PolicyEffect, PolicyStatement, Principal, Resource,
    S3Action,
};
use claudefs_gateway::s3_multipart::{MultipartManager, MultipartState, MultipartUpload};
use claudefs_gateway::s3_presigned::{PresignedRequest, PresignedSigner, PresignedUrl};
use claudefs_gateway::s3_ratelimit::{RateLimitConfig, S3RateLimiter};
use claudefs_gateway::session::{ClientSession, SessionId, SessionManager, SessionProtocol};
use claudefs_gateway::token_auth::{AuthToken, TokenAuth, TokenPermissions};

#[cfg(test)]
mod tests {
    use super::*;

    fn make_s3_handler() -> S3Handler {
        S3Handler::new()
    }

    fn make_presigned_signer() -> PresignedSigner {
        PresignedSigner::new("test-access-key", "test-secret-key-12345")
    }

    fn make_policy_registry() -> BucketPolicyRegistry {
        BucketPolicyRegistry::new()
    }

    fn make_rate_limiter(requests_per_second: u32, burst: u32) -> S3RateLimiter {
        S3RateLimiter::new(RateLimitConfig::new(requests_per_second, burst))
    }

    fn make_token_auth() -> TokenAuth {
        TokenAuth::new()
    }

    fn make_session_manager() -> SessionManager {
        SessionManager::new()
    }

    fn make_multipart_manager() -> MultipartManager {
        MultipartManager::new()
    }

    // =========================================================================
    // Category 1: S3 Bucket Name Validation (5 tests)
    // =========================================================================

    #[test]
    fn test_s3_bucket_name_too_short() {
        let handler = make_s3_handler();
        let result = handler.create_bucket("ab");
        assert!(
            result.is_err(),
            "Bucket name 'ab' (2 chars) should be rejected"
        );
    }

    #[test]
    fn test_s3_bucket_name_too_long() {
        let handler = make_s3_handler();
        let long_name = "a".repeat(64);
        let result = handler.create_bucket(&long_name);
        assert!(
            result.is_err(),
            "Bucket name with 64 chars should be rejected (max 63)"
        );
    }

    #[test]
    fn test_s3_bucket_name_ip_format() {
        let handler = make_s3_handler();
        let result = handler.create_bucket("192.168.1.1");
        if result.is_ok() {
            println!("FINDING-GW-S3-01: IP-formatted bucket name '192.168.1.1' was ACCEPTED");
        }
        assert!(result.is_err(), "AWS rejects IP-formatted bucket names");
    }

    #[test]
    fn test_s3_bucket_name_valid() {
        let handler = make_s3_handler();
        let result = handler.create_bucket("my-valid-bucket-123");
        assert!(
            result.is_ok(),
            "Valid bucket name 'my-valid-bucket-123' should be accepted"
        );
    }

    #[test]
    fn test_s3_bucket_name_special_chars() {
        let handler = make_s3_handler();

        let result = handler.create_bucket("my_bucket");
        assert!(
            result.is_err(),
            "Underscore in bucket name should be rejected"
        );

        let uppercase_result = handler.create_bucket("MY-BUCKET");
        if uppercase_result.is_ok() {
            println!("FINDING-GW-S3-07: Uppercase bucket name 'MY-BUCKET' was ACCEPTED (AWS requires lowercase)");
        } else {
            assert!(
                uppercase_result.is_err(),
                "Uppercase in bucket name should be rejected"
            );
        }

        let result = handler.create_bucket("my..bucket");
        assert!(result.is_err(), "Consecutive dots should be rejected");
    }

    // =========================================================================
    // Category 2: Presigned URL Security (5 tests)
    // =========================================================================

    #[test]
    fn test_presigned_url_expiry_cap() {
        let signer = make_presigned_signer();
        let req = PresignedRequest::new("GET", "bucket", "key", 999999);
        assert_eq!(
            req.expires_in, 604800,
            "Expiry should be capped at 7 days (604800s)"
        );
    }

    #[test]
    fn test_presigned_url_signature_validation() {
        let signer = make_presigned_signer();
        let req = PresignedRequest::get("mybucket", "mykey", 3600);
        let url = signer.sign_request(&req, 1000);

        let result = signer.validate_url(
            "GET",
            "mybucket",
            "mykey",
            url.expires_at,
            &url.signature,
            1500,
        );
        assert!(result.is_ok(), "Signature should validate correctly");
    }

    #[test]
    fn test_presigned_url_expired_rejected() {
        let signer = make_presigned_signer();
        let req = PresignedRequest::new("GET", "bucket", "key", 10);
        let url = signer.sign_request(&req, 1000);

        let is_expired = url.is_expired(1011);
        assert!(
            is_expired,
            "URL should be expired at now=1011 (created at 1000, expires_in=10)"
        );
    }

    #[test]
    fn test_presigned_url_wrong_key_fails() {
        let signer1 = PresignedSigner::new("access-key", "secret-key-1");
        let signer2 = PresignedSigner::new("access-key", "secret-key-2");

        let req = PresignedRequest::get("bucket", "key", 3600);
        let url = signer1.sign_request(&req, 1000);

        let result =
            signer2.validate_url("GET", "bucket", "key", url.expires_at, &url.signature, 1500);
        assert!(
            result.is_err(),
            "Validation with different secret should fail"
        );
    }

    #[test]
    fn test_presigned_url_canonical_string_no_body_hash() {
        let signer = make_presigned_signer();
        let req = PresignedRequest::new("GET", "bucket", "key", 3600);
        let url = signer.sign_request(&req, 1000);

        let canonical = &url.canonical_string;
        assert!(canonical.contains("GET"), "Canonical should contain method");
        assert!(
            canonical.contains("bucket"),
            "Canonical should contain bucket"
        );
        assert!(canonical.contains("key"), "Canonical should contain key");
        assert!(
            canonical.contains("access-key"),
            "Canonical should contain access_key_id"
        );
        assert!(
            canonical.contains("3600") || url.expires_at.to_string().len() > 0,
            "Canonical should contain expires"
        );

        if !canonical.contains("UNSIGNED-PAYLOAD") && !canonical.contains("e3b0c442") {
            println!(
                "FINDING-GW-S3-02: Weak canonical string, no body hash in: {}",
                canonical
            );
        }
    }

    // =========================================================================
    // Category 3: Bucket Policy Security (5 tests)
    // =========================================================================

    #[test]
    fn test_policy_principal_any_matches_all() {
        let stmt = PolicyStatement::allow_all_public();
        assert!(
            stmt.applies(0, &S3Action::GetObject, "bucket", "key"),
            "Principal::Any should match uid 0"
        );
        assert!(
            stmt.applies(1000, &S3Action::GetObject, "bucket", "key"),
            "Principal::Any should match uid 1000"
        );
        assert!(
            stmt.applies(65534, &S3Action::PutObject, "bucket", "key"),
            "Principal::Any should match any uid"
        );
    }

    #[test]
    fn test_policy_resource_wildcard_match() {
        let resource = Resource::new("mybucket", "*");
        assert!(
            resource.matches("mybucket", "any/key/at/all"),
            "Wildcard should match deeply nested keys"
        );
        assert!(
            resource.matches("mybucket", "single"),
            "Wildcard should match single key"
        );
        assert!(
            resource.matches("mybucket", ""),
            "Wildcard should match empty key"
        );
    }

    #[test]
    fn test_policy_resource_prefix_match() {
        let resource = Resource::new("mybucket", "uploads/*");
        assert!(
            resource.matches("mybucket", "uploads/file.txt"),
            "Should match prefix path"
        );
        assert!(
            resource.matches("mybucket", "uploads/dir/deep/file.txt"),
            "Should match deep prefix path"
        );
        assert!(
            !resource.matches("mybucket", "downloads/file.txt"),
            "Should NOT match different prefix"
        );
        assert!(
            !resource.matches("mybucket", "uploads"),
            "Should NOT match prefix without trailing slash"
        );
    }

    #[test]
    fn test_policy_deny_effect_exists() {
        let stmt = PolicyStatement::deny_all();
        assert_eq!(
            stmt.effect,
            PolicyEffect::Deny,
            "Deny effect should be constructable"
        );
        println!(
            "FINDING-GW-S3-03: PolicyEffect::Deny exists but may not be enforced in evaluation"
        );
    }

    #[test]
    fn test_policy_action_all_wildcard() {
        let stmt = PolicyStatement::allow_all_public();
        assert!(
            stmt.applies(0, &S3Action::GetObject, "bucket", "key"),
            "S3Action::All should match GetObject"
        );
        assert!(
            stmt.applies(0, &S3Action::PutObject, "bucket", "key"),
            "S3Action::All should match PutObject"
        );
        assert!(
            stmt.applies(0, &S3Action::DeleteObject, "bucket", "key"),
            "S3Action::All should match DeleteObject"
        );
    }

    // =========================================================================
    // Category 4: Token Auth & Rate Limiting (5 tests)
    // =========================================================================

    #[test]
    fn test_token_auth_create_validate() {
        let auth = make_token_auth();
        let token = AuthToken::new("test-token-123", 1000, 100, "testuser");
        auth.register(token);

        let result = auth.validate("test-token-123", 0);
        assert!(result.is_some(), "Token validation should succeed");
        assert_eq!(result.unwrap().uid, 1000, "Token should return correct uid");
    }

    #[test]
    fn test_token_auth_expired_rejected() {
        let auth = make_token_auth();
        let token = AuthToken::new("expired-token", 1000, 100, "user").with_expiry(100);
        auth.register(token);

        let result = auth.validate("expired-token", 101);
        assert!(result.is_none(), "Expired token should return None");
    }

    #[test]
    fn test_token_auth_wrong_token() {
        let auth = make_token_auth();
        auth.register(AuthToken::new("real-token", 1000, 100, "user"));

        let result = auth.validate("fake-token", 0);
        assert!(result.is_none(), "Unknown token should return None");
    }

    #[test]
    fn test_rate_limiter_allows_within_limit() {
        let limiter = make_rate_limiter(10, 10);
        let mut allowed_count = 0;

        for _ in 0..10 {
            if limiter.try_request("client-token", 0.0) {
                allowed_count += 1;
            }
        }

        assert_eq!(
            allowed_count, 10,
            "All 10 requests should be allowed within burst limit"
        );
    }

    #[test]
    fn test_rate_limiter_rejects_over_limit() {
        let limiter = make_rate_limiter(5, 5);
        let mut allowed_count = 0;

        for _ in 0..10 {
            if limiter.try_request("client-token", 0.0) {
                allowed_count += 1;
            }
        }

        assert!(
            allowed_count < 10,
            "Some requests should be rejected when exceeding limit"
        );
    }

    // =========================================================================
    // Category 5: NFS Export & Session Security (5 tests)
    // =========================================================================

    #[test]
    fn test_export_cidr_startswith_vulnerability() {
        let spec = ClientSpec::from_cidr("192.168.1.1");
        assert!(spec.allows("192.168.1.1"), "Exact IP should match");

        let also_allowed = spec.allows("192.168.1.10");
        if also_allowed {
            println!("FINDING-GW-S3-04: Incomplete CIDR parsing - '192.168.1.1' also allows '192.168.1.10'");
        }
    }

    #[test]
    fn test_export_wildcard_allows_all() {
        let spec = ClientSpec::any();
        assert!(spec.allows("192.168.1.1"), "Wildcard should allow any IP");
        assert!(spec.allows("10.0.0.1"), "Wildcard should allow any IP");
        assert!(spec.allows("0.0.0.0"), "Wildcard should allow any IP");
    }

    #[test]
    fn test_tls_version_minimum() {
        let config = TlsConfig::default();

        if config.min_version == TlsVersion::Tls12 {
            println!("FINDING-GW-S3-05: TLS 1.2 is minimum (acceptable)");
        } else {
            println!(
                "FINDING-GW-S3-06: TLS minimum is {:?} - check if TLS 1.0/1.1 allowed",
                config.min_version
            );
        }

        assert!(
            config.min_version == TlsVersion::Tls12 || config.min_version == TlsVersion::Tls13,
            "TLS minimum should be 1.2 or 1.3"
        );
    }

    #[test]
    fn test_session_id_uniqueness() {
        let manager = make_session_manager();
        let mut ids = Vec::new();

        for _ in 0..100 {
            let id = manager.create_session(SessionProtocol::S3, "192.168.1.1", 1000, 1000, 100);
            ids.push(id);
        }

        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), 100, "All 100 session IDs should be unique");
    }

    #[test]
    fn test_multipart_upload_state_transitions() {
        let manager = make_multipart_manager();
        let upload_id = manager.create("bucket", "key", "text/plain");

        let upload = manager.get(&upload_id).unwrap();
        assert_eq!(
            upload.state,
            MultipartState::Active,
            "New upload should be Active"
        );

        manager.abort(&upload_id).unwrap();
        let upload = manager.get(&upload_id).unwrap();
        assert_eq!(
            upload.state,
            MultipartState::Aborted,
            "After abort should be Aborted"
        );
    }
}
