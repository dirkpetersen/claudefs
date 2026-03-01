//! Certificate enrollment for mTLS authentication per architecture decision D7.
//!
//! This module provides:
//! - Cluster CA generation and management
//! - One-time token-based client enrollment
//! - Certificate lifecycle management (issuance, renewal, revocation)
//! - CRL distribution for revoked certificates

use rcgen::{BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tracing::info;

/// Configuration for the enrollment process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentConfig {
    /// Validity period for CA certificates in days (default: 3650 = 10 years).
    pub ca_validity_days: u32,
    /// Validity period for issued certificates in days (default: 365 = 1 year).
    pub cert_validity_days: u32,
    /// Length of enrollment tokens in bytes (default: 32).
    pub token_length: usize,
    /// Validity period for enrollment tokens in seconds (default: 3600 = 1 hour).
    pub token_validity_secs: u64,
    /// Days before expiry to trigger automatic renewal (default: 30).
    pub renewal_threshold_days: u32,
    /// Maximum tokens per node (default: 10).
    pub max_tokens_per_node: usize,
    /// CRL refresh interval in seconds (default: 300 = 5 min).
    pub crl_refresh_interval_secs: u64,
}

impl Default for EnrollmentConfig {
    fn default() -> Self {
        Self {
            ca_validity_days: 3650,
            cert_validity_days: 365,
            token_length: 32,
            token_validity_secs: 3600,
            renewal_threshold_days: 30,
            max_tokens_per_node: 10,
            crl_refresh_interval_secs: 300,
        }
    }
}

/// Errors that can occur during enrollment operations.
#[derive(Error, Debug)]
pub enum EnrollmentError {
    #[error("CA generation failed: {reason}")]
    CaGenerationFailed { reason: String },

    #[error("Certificate signing failed: {reason}")]
    CertSigningFailed { reason: String },

    #[error("Invalid token: {reason}")]
    InvalidToken { reason: String },

    #[error("Token expired: {token}")]
    TokenExpired { token: String },

    #[error("Token already used: {token}")]
    TokenAlreadyUsed { token: String },

    #[error("Certificate revoked: {serial}")]
    CertificateRevoked { serial: String },

    #[error("Certificate expired: {serial}")]
    CertificateExpired { serial: String },

    #[error("Renewal not needed yet: {serial}")]
    RenewalNotNeeded { serial: String },

    #[error("Max tokens exceeded for node {node_id}, max: {max}")]
    MaxTokensExceeded { node_id: String, max: usize },
}

/// Reason for certificate revocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RevocationReason {
    /// Private key compromise.
    KeyCompromise,
    /// Certificate holder ceased operation.
    CessationOfOperation,
    /// Certificate was superseded by a new one.
    Superseded,
    /// Administrative revocation.
    AdminRevoked,
}

/// A CRL entry for revoked certificates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevocationEntry {
    /// Certificate serial number.
    pub serial: String,
    /// Reason for revocation.
    pub reason: RevocationReason,
    /// Timestamp when revoked (epoch ms).
    pub revoked_at: u64,
}

/// A signed certificate with its private key and CA chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertificateBundle {
    /// PEM-encoded certificate.
    pub cert_pem: String,
    /// PEM-encoded private key.
    pub key_pem: String,
    /// PEM-encoded CA certificate.
    pub ca_cert_pem: String,
    /// Certificate serial number (hex).
    pub serial: String,
    /// Certificate not-before timestamp (epoch ms).
    pub not_before: u64,
    /// Certificate not-after timestamp (epoch ms).
    pub not_after: u64,
    /// Certificate subject (CN).
    pub subject: String,
}

/// One-time token for client enrollment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentToken {
    /// The token value (random hex string).
    pub token: String,
    /// When the token was created (epoch ms).
    pub created_at: u64,
    /// When the token expires (epoch ms).
    pub expires_at: u64,
    /// Which node this token is for.
    pub node_id: String,
    /// Whether the token has been used.
    pub used: bool,
}

/// Statistics for the enrollment service.
#[derive(Debug, Default, Clone)]
pub struct EnrollmentStats {
    /// Total certificates issued.
    pub total_issued: u64,
    /// Total certificates renewed.
    pub total_renewed: u64,
    /// Total certificates revoked.
    pub total_revoked: u64,
    /// Active (unused) tokens.
    pub active_tokens: u64,
    /// Expired tokens.
    pub expired_tokens: u64,
}

/// The cluster Certificate Authority for mTLS.
pub struct ClusterCA {
    ca_cert_pem: String,
    ca_key_pem: String,
    ca_fingerprint: String,
    cert_validity_days: u32,
    #[allow(dead_code)]
    not_before: SystemTime,
    #[allow(dead_code)]
    not_after: SystemTime,
}

impl ClusterCA {
    /// Creates a new cluster CA.
    pub fn new(config: &EnrollmentConfig) -> Result<Self, EnrollmentError> {
        let key_pair = KeyPair::generate().map_err(|e| EnrollmentError::CaGenerationFailed {
            reason: format!("failed to generate CA key: {}", e),
        })?;

        let now = SystemTime::now();
        let valid_for = Duration::from_secs(config.ca_validity_days as u64 * 24 * 60 * 60);
        let not_after = now
            .checked_add(valid_for)
            .unwrap_or(SystemTime::UNIX_EPOCH + Duration::from_secs(86400 * 365 * 20));

        let mut params = CertificateParams::default();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);

        let mut distinguished_name = DistinguishedName::new();
        distinguished_name.push(DnType::CommonName, "ClusterCA");
        distinguished_name.push(DnType::OrganizationName, "ClaudeFS");
        params.distinguished_name = distinguished_name;

        let cert =
            params
                .self_signed(&key_pair)
                .map_err(|e| EnrollmentError::CaGenerationFailed {
                    reason: format!("failed to create CA certificate: {}", e),
                })?;

        let ca_cert_pem = cert.pem();
        let ca_key_pem = key_pair.serialize_pem();

        let der_bytes = cert.der();
        let fingerprint = format_sha256_hex(der_bytes);

        Ok(Self {
            ca_cert_pem,
            ca_key_pem,
            ca_fingerprint: fingerprint,
            cert_validity_days: config.cert_validity_days,
            not_before: now,
            not_after,
        })
    }

    /// Returns the CA certificate as PEM.
    pub fn ca_cert_pem(&self) -> &str {
        &self.ca_cert_pem
    }

    /// Returns the SHA-256 fingerprint of the CA certificate.
    pub fn ca_fingerprint(&self) -> &str {
        &self.ca_fingerprint
    }

    /// Signs a CSR and returns the signed certificate as PEM.
    #[allow(dead_code)]
    pub fn sign_csr(&self, _csr_pem: &str) -> Result<String, EnrollmentError> {
        Err(EnrollmentError::CertSigningFailed {
            reason: "CSR signing not yet implemented".to_string(),
        })
    }

    /// Issues a certificate for a node.
    pub fn issue_node_cert(&self, node_id: &str) -> Result<CertificateBundle, EnrollmentError> {
        self.issue_cert(node_id, "node")
    }

    /// Issues a certificate for a client.
    pub fn issue_client_cert(&self, client_id: &str) -> Result<CertificateBundle, EnrollmentError> {
        self.issue_cert(client_id, "client")
    }

    fn issue_cert(
        &self,
        subject: &str,
        cert_type: &str,
    ) -> Result<CertificateBundle, EnrollmentError> {
        let ca_key = KeyPair::from_pem(&self.ca_key_pem).map_err(|e| {
            EnrollmentError::CertSigningFailed {
                reason: format!("failed to parse CA key: {}", e),
            }
        })?;

        let ca_cert_params =
            CertificateParams::from_ca_cert_pem(&self.ca_cert_pem).map_err(|e| {
                EnrollmentError::CertSigningFailed {
                    reason: format!("failed to parse CA certificate: {}", e),
                }
            })?;

        let ca_cert = ca_cert_params.self_signed(&ca_key).map_err(|e| {
            EnrollmentError::CertSigningFailed {
                reason: format!("failed to reconstruct CA cert: {}", e),
            }
        })?;

        let key_pair = KeyPair::generate().map_err(|e| EnrollmentError::CertSigningFailed {
            reason: format!("failed to generate key: {}", e),
        })?;

        let now = SystemTime::now();
        let valid_for = Duration::from_secs(self.cert_validity_days as u64 * 24 * 60 * 60);
        let not_after = now
            .checked_add(valid_for)
            .unwrap_or(SystemTime::UNIX_EPOCH + Duration::from_secs(86400 * 365 * 20));

        let mut params = CertificateParams::new(vec![subject.to_string()]).map_err(|e| {
            EnrollmentError::CertSigningFailed {
                reason: format!("failed to create cert params: {}", e),
            }
        })?;

        let mut distinguished_name = DistinguishedName::new();
        distinguished_name.push(DnType::CommonName, subject);
        distinguished_name.push(DnType::OrganizationName, format!("ClaudeFS {}", cert_type));
        params.distinguished_name = distinguished_name;

        let cert = params
            .signed_by(&key_pair, &ca_cert, &ca_key)
            .map_err(|e| EnrollmentError::CertSigningFailed {
                reason: format!("failed to sign certificate: {}", e),
            })?;

        let cert_pem = cert.pem();
        let key_pem = key_pair.serialize_pem();
        let der_bytes = cert.der();
        let serial = format_sha256_hex(&der_bytes[..8.min(der_bytes.len())]);

        let not_before_ms = now
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let not_after_ms = not_after
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(u64::MAX);

        Ok(CertificateBundle {
            cert_pem,
            key_pem,
            ca_cert_pem: self.ca_cert_pem.clone(),
            serial,
            not_before: not_before_ms,
            not_after: not_after_ms,
            subject: subject.to_string(),
        })
    }
}

/// The enrollment service managing certificate lifecycle.
pub struct EnrollmentService {
    ca: ClusterCA,
    config: EnrollmentConfig,
    tokens: HashMap<String, EnrollmentToken>,
    revoked: HashMap<String, RevocationEntry>,
    stats: EnrollmentStats,
}

impl EnrollmentService {
    /// Creates a new enrollment service.
    pub fn new(ca: ClusterCA, config: EnrollmentConfig) -> Self {
        Self {
            ca,
            config,
            tokens: HashMap::new(),
            revoked: HashMap::new(),
            stats: EnrollmentStats::default(),
        }
    }

    /// Generates an enrollment token for a node.
    pub fn generate_token(&mut self, node_id: &str) -> Result<EnrollmentToken, EnrollmentError> {
        let now_ms = current_time_ms();
        let expires_at = now_ms + (self.config.token_validity_secs * 1000);

        let token_count = self
            .tokens
            .values()
            .filter(|t| t.node_id == node_id && !t.used && t.expires_at > now_ms)
            .count();

        if token_count >= self.config.max_tokens_per_node {
            return Err(EnrollmentError::MaxTokensExceeded {
                node_id: node_id.to_string(),
                max: self.config.max_tokens_per_node,
            });
        }

        let mut token_bytes = vec![0u8; self.config.token_length];
        getrandom::getrandom(&mut token_bytes).map_err(|e| {
            EnrollmentError::CaGenerationFailed {
                reason: format!("Failed to generate random token: {}", e),
            }
        })?;
        let token_value = token_to_hex(&token_bytes);

        let enrollment_token = EnrollmentToken {
            token: token_value.clone(),
            created_at: now_ms,
            expires_at,
            node_id: node_id.to_string(),
            used: false,
        };

        self.tokens.insert(token_value.clone(), enrollment_token);
        self.stats.active_tokens += 1;

        Ok(self.tokens.get(&token_value).cloned().unwrap())
    }

    /// Enrolls a client using an enrollment token.
    pub fn enroll_with_token(
        &mut self,
        token_str: &str,
    ) -> Result<CertificateBundle, EnrollmentError> {
        let now_ms = current_time_ms();

        let token =
            self.tokens
                .get_mut(token_str)
                .ok_or_else(|| EnrollmentError::InvalidToken {
                    reason: "token not found".to_string(),
                })?;

        if token.used {
            let token_clone = token_str.to_string();
            return Err(EnrollmentError::TokenAlreadyUsed { token: token_clone });
        }

        if now_ms > token.expires_at {
            token.used = true;
            self.stats.active_tokens = self.stats.active_tokens.saturating_sub(1);
            self.stats.expired_tokens += 1;
            return Err(EnrollmentError::TokenExpired {
                token: token_str.to_string(),
            });
        }

        let node_id = token.node_id.clone();
        token.used = true;
        self.stats.active_tokens = self.stats.active_tokens.saturating_sub(1);
        self.stats.total_issued += 1;

        info!("Enrolled node: {}", node_id);

        self.ca.issue_client_cert(&node_id)
    }

    /// Attempts to renew an existing certificate.
    #[allow(dead_code)]
    pub fn renew_certificate(
        &self,
        _old_cert_pem: &str,
    ) -> Result<CertificateBundle, EnrollmentError> {
        Err(EnrollmentError::CertSigningFailed {
            reason:
                "Certificate renewal requires parsing old certificate which is not yet implemented"
                    .to_string(),
        })
    }

    /// Revokes a certificate by serial number.
    pub fn revoke(
        &mut self,
        serial: &str,
        reason: RevocationReason,
    ) -> Result<(), EnrollmentError> {
        let now_ms = current_time_ms();

        let entry = RevocationEntry {
            serial: serial.to_string(),
            reason,
            revoked_at: now_ms,
        };

        let reason_display = format!("{:?}", entry.reason);
        self.revoked.insert(serial.to_string(), entry);
        self.stats.total_revoked += 1;

        info!(
            "Revoked certificate {} with reason: {}",
            serial, reason_display
        );

        Ok(())
    }

    /// Checks if a certificate is revoked.
    pub fn is_revoked(&self, serial: &str) -> bool {
        self.revoked.contains_key(serial)
    }

    /// Returns the current CRL.
    pub fn get_crl(&self) -> Vec<RevocationEntry> {
        self.revoked.values().cloned().collect()
    }

    /// Returns enrollment statistics.
    pub fn stats(&self) -> EnrollmentStats {
        self.stats.clone()
    }

    /// Returns the CA certificate PEM.
    pub fn ca_cert_pem(&self) -> &str {
        self.ca.ca_cert_pem()
    }

    /// Returns the CA fingerprint.
    pub fn ca_fingerprint(&self) -> &str {
        self.ca.ca_fingerprint()
    }

    /// Issues a node certificate (for internal use).
    pub fn issue_node_cert(&self, node_id: &str) -> Result<CertificateBundle, EnrollmentError> {
        self.ca.issue_node_cert(node_id)
    }

    /// Issues a client certificate (for internal use).
    pub fn issue_client_cert(&self, client_id: &str) -> Result<CertificateBundle, EnrollmentError> {
        self.ca.issue_client_cert(client_id)
    }

    /// Gets token validity config.
    pub fn token_validity_secs(&self) -> u64 {
        self.config.token_validity_secs
    }

    /// Gets renewal threshold.
    pub fn renewal_threshold_days(&self) -> u32 {
        self.config.renewal_threshold_days
    }
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn token_to_hex(token_bytes: &[u8]) -> String {
    token_bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn format_sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    result.iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = EnrollmentConfig::default();
        assert_eq!(config.ca_validity_days, 3650);
        assert_eq!(config.cert_validity_days, 365);
        assert_eq!(config.token_length, 32);
        assert_eq!(config.token_validity_secs, 3600);
        assert_eq!(config.renewal_threshold_days, 30);
        assert_eq!(config.max_tokens_per_node, 10);
        assert_eq!(config.crl_refresh_interval_secs, 300);
    }

    #[test]
    fn test_config_serialization() {
        let config = EnrollmentConfig::default();
        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized: EnrollmentConfig = serde_json::from_str(&serialized).unwrap();
        assert_eq!(config.ca_validity_days, deserialized.ca_validity_days);
        assert_eq!(config.cert_validity_days, deserialized.cert_validity_days);
    }

    #[test]
    fn test_cluster_ca_creation() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).unwrap();
        assert!(!ca.ca_cert_pem().is_empty());
        assert!(ca.ca_cert_pem().contains("BEGIN CERTIFICATE"));
        assert!(!ca.ca_fingerprint().is_empty());
        assert_eq!(ca.ca_fingerprint().len(), 64);
    }

    #[test]
    fn test_ca_cert_pem_valid() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).unwrap();
        let cert_pem = ca.ca_cert_pem();
        assert!(cert_pem.contains("-----BEGIN CERTIFICATE-----"));
        assert!(cert_pem.contains("-----END CERTIFICATE-----"));
    }

    #[test]
    fn test_ca_fingerprint_non_empty_hex() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).unwrap();
        let fp = ca.ca_fingerprint();
        assert!(!fp.is_empty());
        assert_eq!(fp.len(), 64);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_issue_node_certificate() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).unwrap();
        let bundle = ca.issue_node_cert("node1").unwrap();
        assert!(!bundle.cert_pem.is_empty());
        assert!(!bundle.key_pem.is_empty());
        assert!(!bundle.ca_cert_pem.is_empty());
        assert!(!bundle.serial.is_empty());
        assert!(bundle.not_before > 0);
        assert!(bundle.not_after > bundle.not_before);
        assert_eq!(bundle.subject, "node1");
    }

    #[test]
    fn test_issue_client_certificate() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).unwrap();
        let bundle = ca.issue_client_cert("client1").unwrap();
        assert!(!bundle.cert_pem.is_empty());
        assert!(!bundle.key_pem.is_empty());
        assert!(!bundle.serial.is_empty());
        assert_eq!(bundle.subject, "client1");
    }

    #[test]
    fn test_certificate_bundle_fields() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).unwrap();
        let bundle = ca.issue_node_cert("node1").unwrap();
        assert!(bundle.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(bundle.key_pem.contains("BEGIN PRIVATE KEY"));
        assert!(bundle.ca_cert_pem.contains("BEGIN CERTIFICATE"));
    }

    #[test]
    fn test_enrollment_service_creation() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).unwrap();
        let service = EnrollmentService::new(ca, config.clone());
        assert_eq!(service.stats().total_issued, 0);
        assert_eq!(service.stats().total_renewed, 0);
        assert_eq!(service.stats().total_revoked, 0);
    }

    #[test]
    fn test_generate_enrollment_token() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).unwrap();
        let mut service = EnrollmentService::new(ca, config);

        let token = service.generate_token("node1").unwrap();
        assert!(!token.token.is_empty());
        assert!(token.token.len() >= 32);
        assert!(token.expires_at > token.created_at);
        assert_eq!(token.node_id, "node1");
        assert!(!token.used);
    }

    #[test]
    fn test_token_has_correct_length() {
        let mut config = EnrollmentConfig::default();
        config.token_length = 16;
        let ca = ClusterCA::new(&config).unwrap();
        let mut service = EnrollmentService::new(ca, config);

        let token = service.generate_token("node1").unwrap();
        assert_eq!(token.token.len(), 32);
    }

    #[test]
    fn test_token_has_valid_expiry() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).unwrap();
        let mut service = EnrollmentService::new(ca, config);

        let token = service.generate_token("node1").unwrap();
        let expected_expiry = token.created_at + (3600 * 1000);
        assert!((token.expires_at - expected_expiry) < 1000);
    }

    #[test]
    fn test_enroll_with_valid_token() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).unwrap();
        let mut service = EnrollmentService::new(ca, config);

        let token = service.generate_token("node1").unwrap();
        let bundle = service.enroll_with_token(&token.token).unwrap();

        assert!(!bundle.cert_pem.is_empty());
        assert!(!bundle.key_pem.is_empty());
        assert_eq!(bundle.subject, "node1");
        assert_eq!(service.stats().total_issued, 1);
    }

    #[test]
    fn test_enroll_with_invalid_token() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).unwrap();
        let mut service = EnrollmentService::new(ca, config);

        let result = service.enroll_with_token("invalid_token");
        assert!(matches!(result, Err(EnrollmentError::InvalidToken { .. })));
    }

    #[test]
    fn test_enroll_with_expired_token() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).unwrap();
        let mut service = EnrollmentService::new(ca, config);

        let token = service.generate_token("node1").unwrap();
        let token_value = token.token.clone();

        let mut expired_token = token;
        expired_token.expires_at = 0;
        service
            .tokens
            .insert(expired_token.token.clone(), expired_token);

        let result = service.enroll_with_token(&token_value);
        assert!(matches!(result, Err(EnrollmentError::TokenExpired { .. })));
    }

    #[test]
    fn test_enroll_with_already_used_token() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).unwrap();
        let mut service = EnrollmentService::new(ca, config);

        let token = service.generate_token("node1").unwrap();
        let _ = service.enroll_with_token(&token.token);

        let result = service.enroll_with_token(&token.token);
        assert!(matches!(
            result,
            Err(EnrollmentError::TokenAlreadyUsed { .. })
        ));
    }

    #[test]
    fn test_max_tokens_per_node_exceeded() {
        let mut config = EnrollmentConfig::default();
        config.max_tokens_per_node = 2;
        let ca = ClusterCA::new(&config).unwrap();
        let mut service = EnrollmentService::new(ca, config);

        let _ = service.generate_token("node1").unwrap();
        let _ = service.generate_token("node1").unwrap();

        let result = service.generate_token("node1");
        assert!(matches!(
            result,
            Err(EnrollmentError::MaxTokensExceeded { .. })
        ));
    }

    #[test]
    fn test_revoke_certificate() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).unwrap();
        let mut service = EnrollmentService::new(ca, config);

        let bundle = service.issue_client_cert("client1").unwrap();
        let serial = bundle.serial.clone();

        service
            .revoke(&serial, RevocationReason::KeyCompromise)
            .unwrap();

        assert!(service.is_revoked(&serial));
        assert_eq!(service.stats().total_revoked, 1);
    }

    #[test]
    fn test_is_revoked() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).unwrap();
        let mut service = EnrollmentService::new(ca, config);

        assert!(!service.is_revoked("unknown"));

        service
            .revoke("serial1", RevocationReason::AdminRevoked)
            .unwrap();

        assert!(service.is_revoked("serial1"));
        assert!(!service.is_revoked("serial2"));
    }

    #[test]
    fn test_get_crl_entries() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).unwrap();
        let mut service = EnrollmentService::new(ca, config);

        service
            .revoke("serial1", RevocationReason::KeyCompromise)
            .unwrap();
        service
            .revoke("serial2", RevocationReason::Superseded)
            .unwrap();

        let crl = service.get_crl();
        assert_eq!(crl.len(), 2);
    }

    #[test]
    fn test_revocation_reasons() {
        let reasons = [
            RevocationReason::KeyCompromise,
            RevocationReason::CessationOfOperation,
            RevocationReason::Superseded,
            RevocationReason::AdminRevoked,
        ];

        for reason in reasons {
            let serialized = serde_json::to_string(&reason).unwrap();
            let deserialized: RevocationReason = serde_json::from_str(&serialized).unwrap();
            assert_eq!(reason, deserialized);
        }
    }

    #[test]
    fn test_enrollment_stats_tracking() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).unwrap();
        let mut service = EnrollmentService::new(ca, config);

        let token = service.generate_token("node1").unwrap();
        let _ = service.enroll_with_token(&token.token);

        let stats = service.stats();
        assert_eq!(stats.total_issued, 1);
        assert_eq!(stats.active_tokens, 0);
    }

    #[test]
    fn test_multiple_enrollments() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).unwrap();
        let mut service = EnrollmentService::new(ca, config);

        let token1 = service.generate_token("node1").unwrap();
        let token2 = service.generate_token("node2").unwrap();

        let _ = service.enroll_with_token(&token1.token);
        let _ = service.enroll_with_token(&token2.token);

        assert_eq!(service.stats().total_issued, 2);
    }

    #[test]
    fn test_token_expiry_validation() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).unwrap();
        let mut service = EnrollmentService::new(ca, config);

        let token = service.generate_token("node1").unwrap();

        assert!(token.created_at > 0);
        assert!(token.expires_at > token.created_at);
        assert!(token.expires_at - token.created_at >= 3600 * 1000);
    }

    #[test]
    fn test_service_issue_node_cert() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).unwrap();
        let service = EnrollmentService::new(ca, config);

        let bundle = service.issue_node_cert("node1").unwrap();
        assert_eq!(bundle.subject, "node1");
    }

    #[test]
    fn test_service_issue_client_cert() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).unwrap();
        let service = EnrollmentService::new(ca, config);

        let bundle = service.issue_client_cert("client1").unwrap();
        assert_eq!(bundle.subject, "client1");
    }

    #[test]
    fn test_revocation_reason_copy() {
        let reason = RevocationReason::KeyCompromise;
        let _ = reason;
        let _ = reason;
    }

    #[test]
    fn test_revocation_entry_serialization() {
        let entry = RevocationEntry {
            serial: "abc123".to_string(),
            reason: RevocationReason::KeyCompromise,
            revoked_at: 1234567890,
        };

        let serialized = serde_json::to_string(&entry).unwrap();
        let deserialized: RevocationEntry = serde_json::from_str(&serialized).unwrap();

        assert_eq!(entry.serial, deserialized.serial);
        assert_eq!(entry.reason, deserialized.reason);
        assert_eq!(entry.revoked_at, deserialized.revoked_at);
    }

    #[test]
    fn test_certificate_bundle_serialization() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).unwrap();
        let bundle = ca.issue_client_cert("client1").unwrap();

        let serialized = serde_json::to_string(&bundle).unwrap();
        let deserialized: CertificateBundle = serde_json::from_str(&serialized).unwrap();

        assert_eq!(bundle.cert_pem, deserialized.cert_pem);
        assert_eq!(bundle.key_pem, deserialized.key_pem);
        assert_eq!(bundle.subject, deserialized.subject);
    }

    #[test]
    fn test_enrollment_token_serialization() {
        let token = EnrollmentToken {
            token: "abc123".to_string(),
            created_at: 1234567890,
            expires_at: 1234571490,
            node_id: "node1".to_string(),
            used: false,
        };

        let serialized = serde_json::to_string(&token).unwrap();
        let deserialized: EnrollmentToken = serde_json::from_str(&serialized).unwrap();

        assert_eq!(token.token, deserialized.token);
        assert_eq!(token.node_id, deserialized.node_id);
    }

    #[test]
    fn test_ca_and_service_integration() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).unwrap();
        let service = EnrollmentService::new(ca, config);

        assert!(!service.ca_cert_pem().is_empty());
        assert_eq!(service.ca_fingerprint().len(), 64);
    }

    #[test]
    fn test_different_node_tokens() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).unwrap();
        let mut service = EnrollmentService::new(ca, config);

        let _ = service.generate_token("node1").unwrap();
        let _ = service.generate_token("node2").unwrap();

        assert_eq!(service.stats().active_tokens, 2);
    }
}
