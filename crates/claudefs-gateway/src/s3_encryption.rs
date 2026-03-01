//! S3 Server-Side Encryption (SSE) management.
//!
//! Provides SSE-S3 (AES-256) and SSE-KMS (Key Management Service) encryption
//! for objects stored via the S3 API. This module manages encryption metadata,
//! headers, and configuration - the actual encryption is performed by A3/claudefs-reduce.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use tracing::{debug, warn};

pub const HEADER_SSE: &str = "x-amz-server-side-encryption";
pub const HEADER_SSE_KMS_KEY_ID: &str = "x-amz-server-side-encryption-aws-kms-key-id";
pub const HEADER_SSE_KMS_CONTEXT: &str = "x-amz-server-side-encryption-context";
pub const HEADER_SSE_BUCKET_KEY_ENABLED: &str = "x-amz-server-side-encryption-bucket-key-enabled";
pub const HEADER_SSE_CUSTOMER_ALGORITHM: &str = "x-amz-server-side-encryption-customer-algorithm";
pub const HEADER_SSE_CUSTOMER_KEY: &str = "x-amz-server-side-encryption-customer-key";
pub const HEADER_SSE_CUSTOMER_KEY_MD5: &str = "x-amz-server-side-encryption-customer-key-md5";

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum SseError {
    #[error("Invalid SSE algorithm: {0}")]
    InvalidAlgorithm(String),
    #[error("KMS key is required for this bucket")]
    KmsKeyRequired,
    #[error("Encryption is required for this bucket")]
    EncryptionRequired,
    #[error("Invalid KMS key ID: {0}")]
    InvalidKeyId(String),
    #[error("Invalid encryption context: {0}")]
    InvalidEncryptionContext(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SseAlgorithm {
    None,
    #[serde(rename = "AES256")]
    AesCbc256,
    #[serde(rename = "aws:kms")]
    AwsKms,
    #[serde(rename = "aws:kms:dsse")]
    AwsKmsDsse,
}

impl SseAlgorithm {
    pub fn from_str(s: &str) -> Result<Self, SseError> {
        match s {
            "" | "NONE" => Ok(SseAlgorithm::None),
            "AES256" => Ok(SseAlgorithm::AesCbc256),
            "aws:kms" => Ok(SseAlgorithm::AwsKms),
            "aws:kms:dsse" => Ok(SseAlgorithm::AwsKmsDsse),
            _ => Err(SseError::InvalidAlgorithm(s.to_string())),
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            SseAlgorithm::None => "NONE".to_string(),
            SseAlgorithm::AesCbc256 => "AES256".to_string(),
            SseAlgorithm::AwsKms => "aws:kms".to_string(),
            SseAlgorithm::AwsKmsDsse => "aws:kms:dsse".to_string(),
        }
    }

    pub fn is_kms(&self) -> bool {
        matches!(self, SseAlgorithm::AwsKms | SseAlgorithm::AwsKmsDsse)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SseContext {
    pub algorithm: SseAlgorithm,
    pub key_id: Option<String>,
    pub encryption_context: HashMap<String, String>,
    pub bucket_key_enabled: bool,
}

impl Default for SseContext {
    fn default() -> Self {
        Self {
            algorithm: SseAlgorithm::None,
            key_id: None,
            encryption_context: HashMap::new(),
            bucket_key_enabled: false,
        }
    }
}

impl SseContext {
    pub fn new(algorithm: SseAlgorithm) -> Self {
        Self {
            algorithm,
            key_id: None,
            encryption_context: HashMap::new(),
            bucket_key_enabled: false,
        }
    }

    pub fn with_key_id(mut self, key_id: String) -> Self {
        self.key_id = Some(key_id);
        self
    }

    pub fn with_encryption_context(mut self, context: HashMap<String, String>) -> Self {
        self.encryption_context = context;
        self
    }

    pub fn with_bucket_key_enabled(mut self, enabled: bool) -> Self {
        self.bucket_key_enabled = enabled;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SseBucketConfig {
    pub bucket: String,
    pub default_algorithm: SseAlgorithm,
    pub default_key_id: Option<String>,
    pub enforce_encryption: bool,
}

impl SseBucketConfig {
    pub fn new(bucket: String) -> Self {
        Self {
            bucket,
            default_algorithm: SseAlgorithm::None,
            default_key_id: None,
            enforce_encryption: false,
        }
    }

    pub fn with_default_algorithm(mut self, algorithm: SseAlgorithm) -> Self {
        self.default_algorithm = algorithm;
        self
    }

    pub fn with_default_key_id(mut self, key_id: String) -> Self {
        self.default_key_id = Some(key_id);
        self
    }

    pub fn with_enforce_encryption(mut self, enforce: bool) -> Self {
        self.enforce_encryption = enforce;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SseObjectMetadata {
    pub algorithm: SseAlgorithm,
    pub key_id: Option<String>,
    pub key_md5: Option<String>,
    pub bucket_key_enabled: bool,
}

impl Default for SseObjectMetadata {
    fn default() -> Self {
        Self {
            algorithm: SseAlgorithm::None,
            key_id: None,
            key_md5: None,
            bucket_key_enabled: false,
        }
    }
}

impl SseObjectMetadata {
    pub fn new(algorithm: SseAlgorithm) -> Self {
        Self {
            algorithm,
            key_id: None,
            key_md5: None,
            bucket_key_enabled: false,
        }
    }

    pub fn with_key_id(mut self, key_id: String) -> Self {
        self.key_id = Some(key_id);
        self
    }

    pub fn with_key_md5(mut self, md5: String) -> Self {
        self.key_md5 = Some(md5);
        self
    }

    pub fn with_bucket_key_enabled(mut self, enabled: bool) -> Self {
        self.bucket_key_enabled = enabled;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SseBucketConfigResponse {
    pub bucket: String,
    pub default_algorithm: String,
    pub default_key_id: Option<String>,
    pub enforce_encryption: bool,
}

impl From<&SseBucketConfig> for SseBucketConfigResponse {
    fn from(config: &SseBucketConfig) -> Self {
        Self {
            bucket: config.bucket.clone(),
            default_algorithm: config.default_algorithm.to_string(),
            default_key_id: config.default_key_id.clone(),
            enforce_encryption: config.enforce_encryption,
        }
    }
}

pub struct SseManager {
    bucket_configs: HashMap<String, SseBucketConfig>,
}

impl Default for SseManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SseManager {
    pub fn new() -> Self {
        Self {
            bucket_configs: HashMap::new(),
        }
    }

    pub fn configure_bucket(&mut self, config: SseBucketConfig) {
        debug!(
            bucket = %config.bucket,
            algorithm = ?config.default_algorithm,
            enforce = config.enforce_encryption,
            "Configuring bucket SSE"
        );
        self.bucket_configs.insert(config.bucket.clone(), config);
    }

    pub fn get_bucket_config(&self, bucket: &str) -> Option<&SseBucketConfig> {
        self.bucket_configs.get(bucket)
    }

    pub fn remove_bucket(&mut self, bucket: &str) -> Option<SseBucketConfig> {
        self.bucket_configs.remove(bucket)
    }

    pub fn list_buckets(&self) -> Vec<&String> {
        self.bucket_configs.keys().collect()
    }

    pub fn resolve_sse_for_upload(
        &self,
        bucket: &str,
        request_context: &SseContext,
    ) -> Result<SseObjectMetadata, SseError> {
        let bucket_config = self.bucket_configs.get(bucket);

        let algorithm = if request_context.algorithm != SseAlgorithm::None {
            request_context.algorithm
        } else if let Some(config) = bucket_config {
            config.default_algorithm
        } else {
            SseAlgorithm::None
        };

        let key_id = request_context
            .key_id
            .clone()
            .or_else(|| bucket_config.and_then(|c| c.default_key_id.clone()));

        if bucket_config.map(|c| c.enforce_encryption).unwrap_or(false) {
            if algorithm == SseAlgorithm::None {
                warn!(bucket = %bucket, "Upload rejected: encryption required");
                return Err(SseError::EncryptionRequired);
            }

            if algorithm.is_kms() && key_id.is_none() {
                warn!(bucket = %bucket, "Upload rejected: KMS key required");
                return Err(SseError::KmsKeyRequired);
            }
        }

        if algorithm.is_kms() && key_id.is_none() {
            if let Some(config) = bucket_config {
                if config.default_key_id.is_some() {
                    // Bucket has default key, use it
                } else {
                    return Err(SseError::KmsKeyRequired);
                }
            } else {
                return Err(SseError::KmsKeyRequired);
            }
        }

        let bucket_key_enabled = request_context.bucket_key_enabled
            || bucket_config
                .map(|c| c.default_key_id.is_some())
                .unwrap_or(false);

        debug!(
            bucket = %bucket,
            algorithm = ?algorithm,
            key_id = ?key_id,
            bucket_key_enabled = bucket_key_enabled,
            "Resolved SSE for upload"
        );

        Ok(SseObjectMetadata {
            algorithm,
            key_id,
            key_md5: None,
            bucket_key_enabled,
        })
    }

    pub fn validate_sse_headers(
        &self,
        bucket: &str,
        headers: &HashMap<String, String>,
    ) -> Result<SseContext, SseError> {
        let sse_header = headers.get(HEADER_SSE).map(|s| s.as_str());
        let sse_customer_algo = headers
            .get(HEADER_SSE_CUSTOMER_ALGORITHM)
            .map(|s| s.as_str());

        if sse_customer_algo.is_some() {
            if let Some(algo) = sse_customer_algo {
                if algo != "AES256" {
                    return Err(SseError::InvalidAlgorithm(algo.to_string()));
                }
            }

            if !headers.contains_key(HEADER_SSE_CUSTOMER_KEY) {
                return Err(SseError::InvalidAlgorithm("SSE-C key required".to_string()));
            }

            return Ok(SseContext::new(SseAlgorithm::AesCbc256));
        }

        let algorithm = match sse_header {
            Some(v) => SseAlgorithm::from_str(v)?,
            None => SseAlgorithm::None,
        };

        let key_id = headers.get(HEADER_SSE_KMS_KEY_ID).cloned();

        let encryption_context = if let Some(context_b64) = headers.get(HEADER_SSE_KMS_CONTEXT) {
            let decoded = base64_decode(context_b64)
                .map_err(|e| SseError::InvalidEncryptionContext(e.to_string()))?;
            let json_str = String::from_utf8(decoded)
                .map_err(|e| SseError::InvalidEncryptionContext(e.to_string()))?;
            parse_simple_json(&json_str)?
        } else {
            HashMap::new()
        };

        let bucket_key_enabled = headers
            .get(HEADER_SSE_BUCKET_KEY_ENABLED)
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(false);

        debug!(
            bucket = %bucket,
            algorithm = ?algorithm,
            has_key_id = key_id.is_some(),
            bucket_key_enabled = bucket_key_enabled,
            "Validated SSE headers"
        );

        Ok(SseContext {
            algorithm,
            key_id,
            encryption_context,
            bucket_key_enabled,
        })
    }

    pub fn generate_response_headers(
        &self,
        metadata: &SseObjectMetadata,
    ) -> HashMap<String, String> {
        let mut headers = HashMap::new();

        if metadata.algorithm == SseAlgorithm::None {
            return headers;
        }

        headers.insert(HEADER_SSE.to_string(), metadata.algorithm.to_string());

        if let Some(ref key_id) = metadata.key_id {
            headers.insert(HEADER_SSE_KMS_KEY_ID.to_string(), key_id.clone());
        }

        if metadata.bucket_key_enabled {
            headers.insert(
                HEADER_SSE_BUCKET_KEY_ENABLED.to_string(),
                "true".to_string(),
            );
        }

        debug!(algorithm = ?metadata.algorithm, "Generated SSE response headers");

        headers
    }

    pub fn validate_kms_key_id(&self, key_id: &str) -> Result<(), SseError> {
        if key_id.is_empty() {
            return Err(SseError::InvalidKeyId("empty key ID".to_string()));
        }

        let valid_prefixes = ["arn:aws:kms:", "arn:aws-cn:kms:", "arn:aws-us-gov:kms:"];

        let has_valid_prefix = valid_prefixes.iter().any(|p| key_id.starts_with(p));
        if !has_valid_prefix && !key_id.starts_with("alias/") {
            return Err(SseError::InvalidKeyId(format!(
                "Invalid key ARN format: {}",
                key_id
            )));
        }

        Ok(())
    }
}

fn base64_decode(input: &str) -> Result<Vec<u8>, base64::DecodeError> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    STANDARD.decode(input)
}

fn parse_simple_json(json_str: &str) -> Result<HashMap<String, String>, SseError> {
    let trimmed = json_str.trim();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return Err(SseError::InvalidEncryptionContext(
            "Invalid JSON object format".to_string(),
        ));
    }

    let mut result = HashMap::new();
    let inner = &trimmed[1..trimmed.len() - 1];

    if inner.trim().is_empty() {
        return Ok(result);
    }

    let mut pos = 0;
    while pos < inner.len() {
        let remaining = &inner[pos..];
        let remaining_trimmed = remaining.trim();

        if !remaining_trimmed.starts_with('"') {
            return Err(SseError::InvalidEncryptionContext(format!(
                "Expected key string at position {}",
                pos
            )));
        }

        let key_end = find_string_end(&remaining_trimmed[1..]);
        if key_end.is_none() {
            return Err(SseError::InvalidEncryptionContext(
                "Unterminated key string".to_string(),
            ));
        }
        let key_end = key_end.unwrap();
        let key = &remaining_trimmed[1..key_end];

        let after_key = remaining_trimmed[key_end + 1..].trim();
        if !after_key.starts_with(':') {
            return Err(SseError::InvalidEncryptionContext(
                "Expected colon after key".to_string(),
            ));
        }

        let after_colon = after_key[1..].trim();
        if !after_colon.starts_with('"') {
            return Err(SseError::InvalidEncryptionContext(
                "Expected value string".to_string(),
            ));
        }

        let value_end = find_string_end(&after_colon[1..]);
        if value_end.is_none() {
            return Err(SseError::InvalidEncryptionContext(
                "Unterminated value string".to_string(),
            ));
        }
        let value_end = value_end.unwrap();
        let value = &after_colon[1..value_end];

        result.insert(key.to_string(), value.to_string());

        pos += remaining.len() - after_colon[value_end + 1..].trim().len();

        if pos < inner.len() {
            let rest = &inner[pos..];
            let rest_trimmed = rest.trim();
            if rest_trimmed.starts_with(',') {
                pos += 1;
            }
        }
    }

    Ok(result)
}

fn find_string_end(s: &str) -> Option<usize> {
    let mut chars = s.chars().enumerate();
    while let Some((i, c)) = chars.next() {
        if c == '\\' {
            if chars.next().is_none() {
                return None;
            }
        } else if c == '"' {
            return Some(i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_algorithm_from_str() {
        assert_eq!(SseAlgorithm::from_str("").unwrap(), SseAlgorithm::None);
        assert_eq!(SseAlgorithm::from_str("NONE").unwrap(), SseAlgorithm::None);
        assert_eq!(
            SseAlgorithm::from_str("AES256").unwrap(),
            SseAlgorithm::AesCbc256
        );
        assert_eq!(
            SseAlgorithm::from_str("aws:kms").unwrap(),
            SseAlgorithm::AwsKms
        );
        assert_eq!(
            SseAlgorithm::from_str("aws:kms:dsse").unwrap(),
            SseAlgorithm::AwsKmsDsse
        );
        assert!(SseAlgorithm::from_str("invalid").is_err());
        assert!(SseAlgorithm::from_str("AES128").is_err());
    }

    #[test]
    fn test_sse_algorithm_to_string() {
        assert_eq!(SseAlgorithm::None.to_string(), "NONE");
        assert_eq!(SseAlgorithm::AesCbc256.to_string(), "AES256");
        assert_eq!(SseAlgorithm::AwsKms.to_string(), "aws:kms");
        assert_eq!(SseAlgorithm::AwsKmsDsse.to_string(), "aws:kms:dsse");
    }

    #[test]
    fn test_sse_algorithm_is_kms() {
        assert!(!SseAlgorithm::None.is_kms());
        assert!(!SseAlgorithm::AesCbc256.is_kms());
        assert!(SseAlgorithm::AwsKms.is_kms());
        assert!(SseAlgorithm::AwsKmsDsse.is_kms());
    }

    #[test]
    fn test_sse_context_default() {
        let ctx = SseContext::default();
        assert_eq!(ctx.algorithm, SseAlgorithm::None);
        assert!(ctx.key_id.is_none());
        assert!(ctx.encryption_context.is_empty());
        assert!(!ctx.bucket_key_enabled);
    }

    #[test]
    fn test_sse_context_builder() {
        let ctx = SseContext::new(SseAlgorithm::AwsKms)
            .with_key_id("arn:aws:kms:us-east-1:123456789012:key/abc-123".to_string())
            .with_encryption_context(
                [("bucket".to_string(), "mybucket".to_string())]
                    .into_iter()
                    .collect(),
            )
            .with_bucket_key_enabled(true);

        assert_eq!(ctx.algorithm, SseAlgorithm::AwsKms);
        assert!(ctx.key_id.is_some());
        assert_eq!(
            ctx.encryption_context.get("bucket"),
            Some(&"mybucket".to_string())
        );
        assert!(ctx.bucket_key_enabled);
    }

    #[test]
    fn test_sse_bucket_config_builder() {
        let config = SseBucketConfig::new("mybucket".to_string())
            .with_default_algorithm(SseAlgorithm::AesCbc256)
            .with_default_key_id("alias/my-key".to_string())
            .with_enforce_encryption(true);

        assert_eq!(config.bucket, "mybucket");
        assert_eq!(config.default_algorithm, SseAlgorithm::AesCbc256);
        assert_eq!(config.default_key_id, Some("alias/my-key".to_string()));
        assert!(config.enforce_encryption);
    }

    #[test]
    fn test_sse_object_metadata_builder() {
        let meta = SseObjectMetadata::new(SseAlgorithm::AwsKms)
            .with_key_id("arn:aws:kms:us-east-1:123456789012:key/abc-123".to_string())
            .with_key_md5("abc123".to_string())
            .with_bucket_key_enabled(true);

        assert_eq!(meta.algorithm, SseAlgorithm::AwsKms);
        assert!(meta.key_id.is_some());
        assert_eq!(meta.key_md5, Some("abc123".to_string()));
        assert!(meta.bucket_key_enabled);
    }

    #[test]
    fn test_sse_manager_new() {
        let manager = SseManager::new();
        assert!(manager.get_bucket_config("test").is_none());
        assert!(manager.list_buckets().is_empty());
    }

    #[test]
    fn test_sse_manager_configure_bucket() {
        let mut manager = SseManager::new();
        let config = SseBucketConfig::new("test-bucket".to_string())
            .with_default_algorithm(SseAlgorithm::AesCbc256);

        manager.configure_bucket(config);

        let retrieved = manager.get_bucket_config("test-bucket").unwrap();
        assert_eq!(retrieved.bucket, "test-bucket");
        assert_eq!(retrieved.default_algorithm, SseAlgorithm::AesCbc256);
    }

    #[test]
    fn test_sse_manager_remove_bucket() {
        let mut manager = SseManager::new();
        manager.configure_bucket(SseBucketConfig::new("test".to_string()));

        let removed = manager.remove_bucket("test");
        assert!(removed.is_some());
        assert!(manager.get_bucket_config("test").is_none());
    }

    #[test]
    fn test_resolve_sse_request_overrides_bucket_default() {
        let mut manager = SseManager::new();
        manager.configure_bucket(
            SseBucketConfig::new("test-bucket".to_string())
                .with_default_algorithm(SseAlgorithm::AesCbc256)
                .with_enforce_encryption(true),
        );

        let request_ctx = SseContext::new(SseAlgorithm::AwsKms)
            .with_key_id("arn:aws:kms:us-east-1:123456789012:key/test".to_string());

        let result = manager
            .resolve_sse_for_upload("test-bucket", &request_ctx)
            .unwrap();
        assert_eq!(result.algorithm, SseAlgorithm::AwsKms);
    }

    #[test]
    fn test_resolve_sse_uses_bucket_default() {
        let mut manager = SseManager::new();
        manager.configure_bucket(
            SseBucketConfig::new("test-bucket".to_string())
                .with_default_algorithm(SseAlgorithm::AesCbc256),
        );

        let request_ctx = SseContext::default();

        let result = manager
            .resolve_sse_for_upload("test-bucket", &request_ctx)
            .unwrap();
        assert_eq!(result.algorithm, SseAlgorithm::AesCbc256);
    }

    #[test]
    fn test_resolve_sse_enforce_encryption_rejects_none() {
        let mut manager = SseManager::new();
        manager.configure_bucket(
            SseBucketConfig::new("test-bucket".to_string()).with_enforce_encryption(true),
        );

        let request_ctx = SseContext::default();

        let err = manager
            .resolve_sse_for_upload("test-bucket", &request_ctx)
            .unwrap_err();
        assert!(matches!(err, SseError::EncryptionRequired));
    }

    #[test]
    fn test_resolve_sse_enforce_kms_requires_key() {
        let mut manager = SseManager::new();
        manager.configure_bucket(
            SseBucketConfig::new("test-bucket".to_string())
                .with_default_algorithm(SseAlgorithm::AwsKms)
                .with_enforce_encryption(true),
        );

        let request_ctx = SseContext::default();

        let err = manager
            .resolve_sse_for_upload("test-bucket", &request_ctx)
            .unwrap_err();
        assert!(matches!(err, SseError::KmsKeyRequired));
    }

    #[test]
    fn test_resolve_sse_kms_with_key_succeeds() {
        let mut manager = SseManager::new();
        manager.configure_bucket(
            SseBucketConfig::new("test-bucket".to_string())
                .with_default_algorithm(SseAlgorithm::AwsKms)
                .with_default_key_id("alias/my-key".to_string())
                .with_enforce_encryption(true),
        );

        let request_ctx = SseContext::default();

        let result = manager
            .resolve_sse_for_upload("test-bucket", &request_ctx)
            .unwrap();
        assert_eq!(result.algorithm, SseAlgorithm::AwsKms);
        assert!(result.key_id.is_some());
    }

    #[test]
    fn test_validate_sse_headers_parsing() {
        let mut manager = SseManager::new();
        manager.configure_bucket(SseBucketConfig::new("test-bucket".to_string()));

        let mut headers = HashMap::new();
        headers.insert(HEADER_SSE.to_string(), "AES256".to_string());

        let ctx = manager
            .validate_sse_headers("test-bucket", &headers)
            .unwrap();
        assert_eq!(ctx.algorithm, SseAlgorithm::AesCbc256);
    }

    #[test]
    fn test_validate_sse_headers_kms_with_key() {
        let mut manager = SseManager::new();
        manager.configure_bucket(SseBucketConfig::new("test-bucket".to_string()));

        let mut headers = HashMap::new();
        headers.insert(HEADER_SSE.to_string(), "aws:kms".to_string());
        headers.insert(
            HEADER_SSE_KMS_KEY_ID.to_string(),
            "alias/my-key".to_string(),
        );

        let ctx = manager
            .validate_sse_headers("test-bucket", &headers)
            .unwrap();
        assert_eq!(ctx.algorithm, SseAlgorithm::AwsKms);
        assert_eq!(ctx.key_id, Some("alias/my-key".to_string()));
    }

    #[test]
    fn test_validate_sse_headers_bucket_key_enabled() {
        let mut manager = SseManager::new();
        manager.configure_bucket(SseBucketConfig::new("test-bucket".to_string()));

        let mut headers = HashMap::new();
        headers.insert(HEADER_SSE.to_string(), "aws:kms".to_string());
        headers.insert(
            HEADER_SSE_BUCKET_KEY_ENABLED.to_string(),
            "true".to_string(),
        );

        let ctx = manager
            .validate_sse_headers("test-bucket", &headers)
            .unwrap();
        assert!(ctx.bucket_key_enabled);
    }

    #[test]
    fn test_validate_sse_headers_empty() {
        let mut manager = SseManager::new();
        manager.configure_bucket(SseBucketConfig::new("test-bucket".to_string()));

        let headers = HashMap::new();
        let ctx = manager
            .validate_sse_headers("test-bucket", &headers)
            .unwrap();
        assert_eq!(ctx.algorithm, SseAlgorithm::None);
    }

    #[test]
    fn test_validate_sse_headers_invalid_algorithm() {
        let mut manager = SseManager::new();
        manager.configure_bucket(SseBucketConfig::new("test-bucket".to_string()));

        let mut headers = HashMap::new();
        headers.insert(HEADER_SSE.to_string(), "INVALID".to_string());

        let err = manager
            .validate_sse_headers("test-bucket", &headers)
            .unwrap_err();
        assert!(matches!(err, SseError::InvalidAlgorithm(_)));
    }

    #[test]
    fn test_generate_response_headers_sse_s3() {
        let manager = SseManager::new();
        let metadata = SseObjectMetadata::new(SseAlgorithm::AesCbc256);

        let headers = manager.generate_response_headers(&metadata);

        assert_eq!(headers.get(HEADER_SSE), Some(&"AES256".to_string()));
    }

    #[test]
    fn test_generate_response_headers_sse_kms() {
        let manager = SseManager::new();
        let metadata = SseObjectMetadata::new(SseAlgorithm::AwsKms)
            .with_key_id("alias/my-key".to_string())
            .with_bucket_key_enabled(true);

        let headers = manager.generate_response_headers(&metadata);

        assert_eq!(headers.get(HEADER_SSE), Some(&"aws:kms".to_string()));
        assert_eq!(
            headers.get(HEADER_SSE_KMS_KEY_ID),
            Some(&"alias/my-key".to_string())
        );
        assert_eq!(
            headers.get(HEADER_SSE_BUCKET_KEY_ENABLED),
            Some(&"true".to_string())
        );
    }

    #[test]
    fn test_generate_response_headers_none() {
        let manager = SseManager::new();
        let metadata = SseObjectMetadata::new(SseAlgorithm::None);

        let headers = manager.generate_response_headers(&metadata);
        assert!(headers.is_empty());
    }

    #[test]
    fn test_validate_kms_key_id_arn() {
        let manager = SseManager::new();

        let result = manager.validate_kms_key_id("arn:aws:kms:us-east-1:123456789012:key/abc-123");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_kms_key_id_alias() {
        let manager = SseManager::new();

        let result = manager.validate_kms_key_id("alias/my-key");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_kms_key_id_invalid() {
        let manager = SseManager::new();

        let result = manager.validate_kms_key_id("invalid-key");
        assert!(matches!(result, Err(SseError::InvalidKeyId(_))));
    }

    #[test]
    fn test_validate_kms_key_id_empty() {
        let manager = SseManager::new();

        let result = manager.validate_kms_key_id("");
        assert!(matches!(result, Err(SseError::InvalidKeyId(_))));
    }

    #[test]
    fn test_resolve_sse_no_bucket_config() {
        let manager = SseManager::new();
        let request_ctx = SseContext::default();

        let result = manager
            .resolve_sse_for_upload("unknown-bucket", &request_ctx)
            .unwrap();
        assert_eq!(result.algorithm, SseAlgorithm::None);
    }

    #[test]
    fn test_resolve_sse_bucket_key_enabled_from_bucket() {
        let mut manager = SseManager::new();
        manager.configure_bucket(
            SseBucketConfig::new("test-bucket".to_string())
                .with_default_key_id("alias/my-key".to_string()),
        );

        let request_ctx = SseContext::default();

        let result = manager
            .resolve_sse_for_upload("test-bucket", &request_ctx)
            .unwrap();
        assert!(result.bucket_key_enabled);
    }

    #[test]
    fn test_sse_bucket_config_response_conversion() {
        let config = SseBucketConfig::new("test-bucket".to_string())
            .with_default_algorithm(SseAlgorithm::AwsKms)
            .with_default_key_id("alias/my-key".to_string())
            .with_enforce_encryption(true);

        let response: SseBucketConfigResponse = (&config).into();

        assert_eq!(response.bucket, "test-bucket");
        assert_eq!(response.default_algorithm, "aws:kms");
        assert_eq!(response.default_key_id, Some("alias/my-key".to_string()));
        assert!(response.enforce_encryption);
    }
}
