//! S3 presigned URL generation and validation

use sha2::{Digest, Sha256};
use std::collections::HashMap;

const PRESIGNED_URL_ALGO: &str = "CFSV1-HMAC-SHA256";

/// Request parameters for generating a presigned URL.
#[derive(Debug, Clone)]
pub struct PresignedRequest {
    /// HTTP method (GET, PUT, etc.)
    pub method: String,
    /// S3 bucket name
    pub bucket: String,
    /// Object key within the bucket
    pub key: String,
    /// Expiration time in seconds (capped at 604800)
    pub expires_in: u32,
    /// Additional query parameters to include in the signed URL
    pub extra_params: HashMap<String, String>,
}

impl PresignedRequest {
    /// Creates a new presigned request with the given parameters.
    pub fn new(method: &str, bucket: &str, key: &str, expires_in: u32) -> Self {
        Self {
            method: method.to_uppercase(),
            bucket: bucket.to_string(),
            key: key.to_string(),
            expires_in: expires_in.min(604800),
            extra_params: HashMap::new(),
        }
    }

    /// Creates a GET request for downloading an object.
    pub fn get(bucket: &str, key: &str, expires_in: u32) -> Self {
        Self::new("GET", bucket, key, expires_in)
    }

    /// Creates a PUT request for uploading an object.
    pub fn put(bucket: &str, key: &str, expires_in: u32) -> Self {
        Self::new("PUT", bucket, key, expires_in)
    }
}

/// A generated presigned URL with all signature components.
#[derive(Debug, Clone)]
pub struct PresignedUrl {
    /// The full URL path with query parameters
    pub url_path: String,
    /// Access key ID used for signing
    pub access_key_id: String,
    /// Unix timestamp when the URL was created
    pub created_at: u64,
    /// Unix timestamp when the URL expires
    pub expires_at: u64,
    /// HMAC-SHA256 signature
    pub signature: String,
    /// The canonical string that was signed
    pub canonical_string: String,
}

impl PresignedUrl {
    /// Returns true if the URL is expired at the given time.
    pub fn is_expired(&self, now: u64) -> bool {
        now > self.expires_at
    }
}

/// Signs and validates presigned URL requests.
pub struct PresignedSigner {
    access_key_id: String,
    secret_access_key: String,
}

impl PresignedSigner {
    /// Creates a new signer with the given credentials.
    pub fn new(access_key_id: &str, secret_access_key: &str) -> Self {
        Self {
            access_key_id: access_key_id.to_string(),
            secret_access_key: secret_access_key.to_string(),
        }
    }

    fn sign(&self, canonical: &str) -> String {
        let input = format!("{}:{}", self.secret_access_key, canonical);
        let hash = Sha256::digest(input.as_bytes());
        hash.iter().map(|b| format!("{:02x}", b)).collect()
    }

    fn canonical_string(
        method: &str,
        bucket: &str,
        key: &str,
        access_key_id: &str,
        expires_at: u64,
    ) -> String {
        format!(
            "{}/{}/{}/{}/{}",
            method, bucket, key, access_key_id, expires_at
        )
    }

    /// Signs a presigned request and returns the generated URL.
    pub fn sign_request(&self, req: &PresignedRequest, now: u64) -> PresignedUrl {
        let expires_at = now + req.expires_in as u64;
        let canonical = Self::canonical_string(
            &req.method,
            &req.bucket,
            &req.key,
            &self.access_key_id,
            expires_at,
        );
        let sig = self.sign(&canonical);

        let url_path = format!(
            "/{}/{}?X-CFS-Algorithm={}&X-CFS-AccessKeyId={}&X-CFS-Expires={}&X-CFS-Signature={}",
            req.bucket, req.key, PRESIGNED_URL_ALGO, self.access_key_id, expires_at, sig
        );

        PresignedUrl {
            url_path,
            access_key_id: self.access_key_id.clone(),
            created_at: now,
            expires_at,
            signature: sig,
            canonical_string: canonical,
        }
    }

    /// Validates a presigned URL signature and checks expiration.
    pub fn validate_url(
        &self,
        method: &str,
        bucket: &str,
        key: &str,
        expires_at: u64,
        signature: &str,
        now: u64,
    ) -> Result<(), &'static str> {
        if now > expires_at {
            return Err("URL expired");
        }
        let canonical =
            Self::canonical_string(method, bucket, key, &self.access_key_id, expires_at);
        let expected_sig = self.sign(&canonical);
        if signature != expected_sig {
            return Err("Invalid signature");
        }
        Ok(())
    }

    /// Returns the access key ID.
    pub fn access_key_id(&self) -> &str {
        &self.access_key_id
    }
}

/// Extracts presigned URL parameters from a URL path query string.
pub fn parse_presigned_params(url_path: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    if let Some(query) = url_path.split('?').nth(1) {
        for pair in query.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                params.insert(k.to_string(), v.to_string());
            }
        }
    }
    params
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_presigned_request_new_sets_method_to_uppercase() {
        let req = PresignedRequest::new("get", "bucket", "key", 3600);
        assert_eq!(req.method, "GET");
    }

    #[test]
    fn test_presigned_request_get_creates_get_request() {
        let req = PresignedRequest::get("mybucket", "mykey", 3600);
        assert_eq!(req.method, "GET");
        assert_eq!(req.bucket, "mybucket");
        assert_eq!(req.key, "mykey");
    }

    #[test]
    fn test_presigned_request_put_creates_put_request() {
        let req = PresignedRequest::put("mybucket", "mykey", 3600);
        assert_eq!(req.method, "PUT");
    }

    #[test]
    fn test_presigned_request_expires_in_capped_at_max() {
        let req = PresignedRequest::new("GET", "bucket", "key", 700000);
        assert_eq!(req.expires_in, 604800);
    }

    #[test]
    fn test_presigned_request_expires_in_below_max_unchanged() {
        let req = PresignedRequest::new("GET", "bucket", "key", 3600);
        assert_eq!(req.expires_in, 3600);
    }

    #[test]
    fn test_presigned_signer_new_stores_keys() {
        let signer = PresignedSigner::new("accesskey", "secretkey");
        assert_eq!(signer.access_key_id(), "accesskey");
    }

    #[test]
    fn test_sign_request_returns_url_with_correct_bucket_and_key() {
        let signer = PresignedSigner::new("ak", "sk");
        let req = PresignedRequest::get("mybucket", "mykey", 3600);
        let url = signer.sign_request(&req, 1000);
        assert!(url.url_path.contains("/mybucket/mykey"));
    }

    #[test]
    fn test_sign_request_url_path_contains_algorithm() {
        let signer = PresignedSigner::new("ak", "sk");
        let req = PresignedRequest::get("bucket", "key", 3600);
        let url = signer.sign_request(&req, 1000);
        assert!(url.url_path.contains("X-CFS-Algorithm="));
    }

    #[test]
    fn test_sign_request_url_path_contains_access_key_id() {
        let signer = PresignedSigner::new("myaccesskey", "secret");
        let req = PresignedRequest::get("bucket", "key", 3600);
        let url = signer.sign_request(&req, 1000);
        assert!(url.url_path.contains("X-CFS-AccessKeyId=myaccesskey"));
    }

    #[test]
    fn test_sign_request_url_path_contains_expires() {
        let signer = PresignedSigner::new("ak", "sk");
        let req = PresignedRequest::get("bucket", "key", 3600);
        let url = signer.sign_request(&req, 1000);
        assert!(url.url_path.contains("X-CFS-Expires="));
    }

    #[test]
    fn test_sign_request_url_path_contains_signature() {
        let signer = PresignedSigner::new("ak", "sk");
        let req = PresignedRequest::get("bucket", "key", 3600);
        let url = signer.sign_request(&req, 1000);
        assert!(url.url_path.contains("X-CFS-Signature="));
    }

    #[test]
    fn test_sign_request_created_at_matches_now() {
        let signer = PresignedSigner::new("ak", "sk");
        let req = PresignedRequest::get("bucket", "key", 3600);
        let url = signer.sign_request(&req, 12345);
        assert_eq!(url.created_at, 12345);
    }

    #[test]
    fn test_sign_request_expires_at_equals_now_plus_expires_in() {
        let signer = PresignedSigner::new("ak", "sk");
        let req = PresignedRequest::get("bucket", "key", 3600);
        let url = signer.sign_request(&req, 1000);
        assert_eq!(url.expires_at, 4600);
    }

    #[test]
    fn test_presigned_url_is_expired_false_before_expiry() {
        let url = PresignedUrl {
            url_path: "/bucket/key".to_string(),
            access_key_id: "ak".to_string(),
            created_at: 1000,
            expires_at: 2000,
            signature: "sig".to_string(),
            canonical_string: "canon".to_string(),
        };
        assert!(!url.is_expired(1500));
    }

    #[test]
    fn test_presigned_url_is_expired_true_after_expiry() {
        let url = PresignedUrl {
            url_path: "/bucket/key".to_string(),
            access_key_id: "ak".to_string(),
            created_at: 1000,
            expires_at: 2000,
            signature: "sig".to_string(),
            canonical_string: "canon".to_string(),
        };
        assert!(url.is_expired(2001));
    }

    #[test]
    fn test_validate_url_returns_ok_for_valid_non_expired() {
        let signer = PresignedSigner::new("ak", "sk");
        let req = PresignedRequest::get("bucket", "key", 3600);
        let url = signer.sign_request(&req, 1000);
        let params = parse_presigned_params(&url.url_path);
        let result =
            signer.validate_url("GET", "bucket", "key", url.expires_at, &url.signature, 1500);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_url_returns_err_expired() {
        let signer = PresignedSigner::new("ak", "sk");
        let result = signer.validate_url("GET", "bucket", "key", 1000, "sig", 1001);
        assert_eq!(result.unwrap_err(), "URL expired");
    }

    #[test]
    fn test_validate_url_returns_err_invalid_signature() {
        let signer = PresignedSigner::new("ak", "sk");
        let result = signer.validate_url("GET", "bucket", "key", 2000, "badsig", 1500);
        assert_eq!(result.unwrap_err(), "Invalid signature");
    }

    #[test]
    fn test_same_request_signed_twice_produces_same_signature() {
        let signer = PresignedSigner::new("ak", "sk");
        let req = PresignedRequest::get("bucket", "key", 3600);
        let url1 = signer.sign_request(&req, 1000);
        let url2 = signer.sign_request(&req, 1000);
        assert_eq!(url1.signature, url2.signature);
    }

    #[test]
    fn test_different_signer_produces_different_signature() {
        let signer1 = PresignedSigner::new("ak", "secret1");
        let signer2 = PresignedSigner::new("ak", "secret2");
        let req = PresignedRequest::get("bucket", "key", 3600);
        let url1 = signer1.sign_request(&req, 1000);
        let url2 = signer2.sign_request(&req, 1000);
        assert_ne!(url1.signature, url2.signature);
    }

    #[test]
    fn test_parse_presigned_params_extracts_key_value_pairs() {
        let params = parse_presigned_params("/bucket/key?X-CFS-AccessKeyId=ak&X-CFS-Expires=12345");
        assert_eq!(params.get("X-CFS-AccessKeyId"), Some(&"ak".to_string()));
        assert_eq!(params.get("X-CFS-Expires"), Some(&"12345".to_string()));
    }

    #[test]
    fn test_parse_presigned_params_returns_empty_for_no_query() {
        let params = parse_presigned_params("/bucket/key");
        assert!(params.is_empty());
    }

    #[test]
    fn test_signature_contains_only_hex_chars() {
        let signer = PresignedSigner::new("ak", "sk");
        let req = PresignedRequest::get("bucket", "key", 3600);
        let url = signer.sign_request(&req, 1000);
        assert_eq!(url.signature.len(), 64);
        assert!(url.signature.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_access_key_id_getter_works() {
        let signer = PresignedSigner::new("mykey", "mysecret");
        assert_eq!(signer.access_key_id(), "mykey");
    }
}
