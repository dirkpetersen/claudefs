//! TLS enforcement policy for the replication conduit (addresses FINDING-05).
//!
//! Provides `TlsMode`, `TlsValidator`, `TlsPolicyBuilder`, and `validate_tls_config()`
//! to enforce mandatory mTLS in production replication channels.

use thiserror::Error;

/// Errors from TLS policy validation.
#[derive(Debug, Error, PartialEq)]
pub enum TlsPolicyError {
    /// TLS required but not configured (plaintext not allowed).
    #[error("plaintext not allowed: TLS is required")]
    PlaintextNotAllowed,
    /// Certificate, key, or CA is empty or malformed.
    #[error("invalid certificate: {reason}")]
    InvalidCertificate {
        /// Reason for the certificate being invalid.
        reason: String,
    },
    /// Configuration mode conflict.
    #[error("mode conflict: {msg}")]
    ModeConflict {
        /// Description of the conflict.
        msg: String,
    },
}

/// TLS enforcement mode.
#[derive(Debug, Clone, PartialEq)]
pub enum TlsMode {
    /// TLS mandatory — reject plaintext connections. Use in production.
    Required,
    /// Allow plaintext for in-process test channels.
    TestOnly,
    /// TLS completely off (development/debug only).
    Disabled,
}

/// Placeholder for TLS config (mirrors conduit::ConduitTlsConfig for standalone use).
#[derive(Debug, Clone)]
pub struct TlsConfigRef {
    /// PEM-encoded certificate.
    pub cert_pem: Vec<u8>,
    /// PEM-encoded private key.
    pub key_pem: Vec<u8>,
    /// PEM-encoded CA certificate.
    pub ca_pem: Vec<u8>,
}

/// Validates TLS configuration against the current `TlsMode`.
pub struct TlsValidator {
    mode: TlsMode,
}

impl TlsValidator {
    /// Create a new validator with the given mode.
    pub fn new(mode: TlsMode) -> Self {
        Self { mode }
    }

    /// Get the current TLS mode.
    pub fn mode(&self) -> &TlsMode {
        &self.mode
    }

    /// Returns true if plaintext (no TLS) is allowed.
    pub fn is_plaintext_allowed(&self) -> bool {
        matches!(self.mode, TlsMode::TestOnly | TlsMode::Disabled)
    }

    /// Validate a TLS config option against the current mode.
    ///
    /// - `Required`: tls must be Some with non-empty PEM fields starting with "-----BEGIN"
    /// - `TestOnly`: None allowed (plaintext for tests), Some also accepted
    /// - `Disabled`: always Ok
    pub fn validate_config(&self, tls: &Option<TlsConfigRef>) -> Result<(), TlsPolicyError> {
        match &self.mode {
            TlsMode::Required => {
                let cfg = tls.as_ref().ok_or(TlsPolicyError::PlaintextNotAllowed)?;
                validate_tls_config(&cfg.cert_pem, &cfg.key_pem, &cfg.ca_pem)
            }
            TlsMode::TestOnly | TlsMode::Disabled => Ok(()),
        }
    }
}

/// Builder for `TlsValidator`.
#[derive(Default)]
pub struct TlsPolicyBuilder {
    mode: Option<TlsMode>,
}

impl TlsPolicyBuilder {
    /// Create a new builder (defaults to `TestOnly`).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the TLS mode.
    pub fn mode(mut self, mode: TlsMode) -> Self {
        self.mode = Some(mode);
        self
    }

    /// Build the `TlsValidator`.
    pub fn build(self) -> TlsValidator {
        TlsValidator::new(self.mode.unwrap_or(TlsMode::TestOnly))
    }
}

/// Validate raw PEM fields.
///
/// Returns `Ok(())` if all fields are non-empty and cert_pem starts with "-----BEGIN".
/// Returns `Err(TlsPolicyError::InvalidCertificate)` otherwise.
pub fn validate_tls_config(
    cert_pem: &[u8],
    key_pem: &[u8],
    ca_pem: &[u8],
) -> Result<(), TlsPolicyError> {
    if cert_pem.is_empty() {
        return Err(TlsPolicyError::InvalidCertificate {
            reason: "cert_pem is empty".to_string(),
        });
    }
    if key_pem.is_empty() {
        return Err(TlsPolicyError::InvalidCertificate {
            reason: "key_pem is empty".to_string(),
        });
    }
    if ca_pem.is_empty() {
        return Err(TlsPolicyError::InvalidCertificate {
            reason: "ca_pem is empty".to_string(),
        });
    }
    if !cert_pem.starts_with(b"-----BEGIN") {
        return Err(TlsPolicyError::InvalidCertificate {
            reason: "cert_pem does not start with -----BEGIN".to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tls(cert: &[u8], key: &[u8], ca: &[u8]) -> Option<TlsConfigRef> {
        Some(TlsConfigRef {
            cert_pem: cert.to_vec(),
            key_pem: key.to_vec(),
            ca_pem: ca.to_vec(),
        })
    }

    fn valid_tls() -> Option<TlsConfigRef> {
        make_tls(
            b"-----BEGIN CERTIFICATE-----\ntest\n-----END CERTIFICATE-----",
            b"key-data",
            b"ca-data",
        )
    }

    #[test]
    fn test_tls_mode_required_rejects_none_tls() {
        let validator = TlsValidator::new(TlsMode::Required);
        let result = validator.validate_config(&None);
        assert!(matches!(result, Err(TlsPolicyError::PlaintextNotAllowed)));
    }

    #[test]
    fn test_tls_mode_required_accepts_valid_tls() {
        let validator = TlsValidator::new(TlsMode::Required);
        let result = validator.validate_config(&valid_tls());
        assert!(result.is_ok());
    }

    #[test]
    fn test_tls_mode_required_rejects_empty_cert() {
        let validator = TlsValidator::new(TlsMode::Required);
        let result = validator.validate_config(&make_tls(b"", b"key", b"ca"));
        assert!(matches!(
            result,
            Err(TlsPolicyError::InvalidCertificate { .. })
        ));
    }

    #[test]
    fn test_tls_mode_required_rejects_empty_key() {
        let validator = TlsValidator::new(TlsMode::Required);
        let result = validator.validate_config(&make_tls(b"cert", b"", b"ca"));
        assert!(matches!(
            result,
            Err(TlsPolicyError::InvalidCertificate { .. })
        ));
    }

    #[test]
    fn test_tls_mode_required_rejects_empty_ca() {
        let validator = TlsValidator::new(TlsMode::Required);
        let result = validator.validate_config(&make_tls(b"cert", b"key", b""));
        assert!(matches!(
            result,
            Err(TlsPolicyError::InvalidCertificate { .. })
        ));
    }

    #[test]
    fn test_tls_mode_test_only_allows_none() {
        let validator = TlsValidator::new(TlsMode::TestOnly);
        let result = validator.validate_config(&None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_tls_mode_test_only_allows_tls() {
        let validator = TlsValidator::new(TlsMode::TestOnly);
        let result = validator.validate_config(&valid_tls());
        assert!(result.is_ok());
    }

    #[test]
    fn test_tls_mode_disabled_allows_none() {
        let validator = TlsValidator::new(TlsMode::Disabled);
        let result = validator.validate_config(&None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_tls_mode_disabled_allows_tls() {
        let validator = TlsValidator::new(TlsMode::Disabled);
        let result = validator.validate_config(&valid_tls());
        assert!(result.is_ok());
    }

    #[test]
    fn test_plaintext_allowed_required() {
        let validator = TlsValidator::new(TlsMode::Required);
        assert!(!validator.is_plaintext_allowed());
    }

    #[test]
    fn test_plaintext_allowed_test_only() {
        let validator = TlsValidator::new(TlsMode::TestOnly);
        assert!(validator.is_plaintext_allowed());
    }

    #[test]
    fn test_plaintext_allowed_disabled() {
        let validator = TlsValidator::new(TlsMode::Disabled);
        assert!(validator.is_plaintext_allowed());
    }

    #[test]
    fn test_validator_mode_accessor() {
        let validator = TlsValidator::new(TlsMode::Required);
        assert!(matches!(validator.mode(), TlsMode::Required));

        let validator2 = TlsValidator::new(TlsMode::TestOnly);
        assert!(matches!(validator2.mode(), TlsMode::TestOnly));
    }

    #[test]
    fn test_builder_default_mode() {
        let builder = TlsPolicyBuilder::new();
        let validator = builder.build();
        assert!(matches!(validator.mode(), TlsMode::TestOnly));
    }

    #[test]
    fn test_builder_set_mode() {
        let builder = TlsPolicyBuilder::new().mode(TlsMode::Required);
        let validator = builder.build();
        assert!(matches!(validator.mode(), TlsMode::Required));
    }

    #[test]
    fn test_validate_tls_config_valid() {
        let result = validate_tls_config(
            b"-----BEGIN CERTIFICATE-----\ntest\n-----END CERTIFICATE-----",
            b"key-data",
            b"ca-data",
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_tls_config_empty_cert() {
        let result = validate_tls_config(b"", b"key", b"ca");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_tls_config_empty_key() {
        let result = validate_tls_config(b"cert", b"", b"ca");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_tls_config_empty_ca() {
        let result = validate_tls_config(b"cert", b"key", b"");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_tls_config_pem_check() {
        let result = validate_tls_config(b"NOT A CERTIFICATE", b"key-data", b"ca-data");
        assert!(result.is_err());
    }

    #[test]
    fn test_tls_policy_error_display() {
        let err = TlsPolicyError::PlaintextNotAllowed;
        assert_eq!(format!("{}", err), "plaintext not allowed: TLS is required");

        let err = TlsPolicyError::InvalidCertificate {
            reason: "test reason".to_string(),
        };
        assert_eq!(format!("{}", err), "invalid certificate: test reason");

        let err = TlsPolicyError::ModeConflict {
            msg: "conflict msg".to_string(),
        };
        assert_eq!(format!("{}", err), "mode conflict: conflict msg");
    }

    #[test]
    fn test_tls_mode_clone_eq() {
        let mode1 = TlsMode::Required;
        let mode2 = TlsMode::Required;
        let mode3 = TlsMode::TestOnly;

        assert_eq!(mode1, mode2);
        assert_ne!(mode1, mode3);

        let cloned = mode1.clone();
        assert_eq!(mode1, cloned);
    }
}
