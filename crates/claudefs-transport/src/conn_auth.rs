//! Connection authentication module for mTLS and certificate handling.

use serde::{Deserialize, Serialize};

/// Authentication level for connection security.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AuthLevel {
    /// No authentication required.
    None,
    /// TLS encryption only, no client certificate validation.
    TlsOnly,
    /// Mutual TLS with client certificate validation.
    #[default]
    MutualTls,
    /// Mutual TLS with strict fingerprint whitelist enforcement.
    MutualTlsStrict,
}

/// Information extracted from an X.509 certificate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertificateInfo {
    /// Certificate subject distinguished name.
    pub subject: String,
    /// Certificate issuer distinguished name.
    pub issuer: String,
    /// Certificate serial number in hex.
    pub serial: String,
    /// SHA-256 fingerprint of the certificate.
    pub fingerprint_sha256: String,
    /// Certificate validity start time in milliseconds since epoch.
    pub not_before_ms: u64,
    /// Certificate validity end time in milliseconds since epoch.
    pub not_after_ms: u64,
    /// Whether this certificate is a CA certificate.
    pub is_ca: bool,
}

/// Configuration for connection authentication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Required authentication level.
    pub level: AuthLevel,
    /// List of allowed certificate subjects (DN patterns).
    pub allowed_subjects: Vec<String>,
    /// List of allowed certificate SHA-256 fingerprints.
    pub allowed_fingerprints: Vec<String>,
    /// Maximum age of certificates in days before rejection.
    pub max_cert_age_days: u32,
    /// Whether to require certificates be issued by the cluster CA.
    pub require_cluster_ca: bool,
    /// Expected cluster CA fingerprint for issuer validation.
    pub cluster_ca_fingerprint: Option<String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            level: AuthLevel::MutualTls,
            allowed_subjects: Vec::new(),
            allowed_fingerprints: Vec::new(),
            max_cert_age_days: 365,
            require_cluster_ca: true,
            cluster_ca_fingerprint: None,
        }
    }
}

/// Result of a certificate authentication attempt.
#[derive(Debug, Clone)]
pub enum AuthResult {
    /// Certificate accepted with the given identity.
    Allowed {
        /// Authenticated identity from certificate subject.
        identity: String,
    },
    /// Certificate rejected with a specific reason.
    Denied {
        /// Human-readable reason for rejection.
        reason: String,
    },
    /// Certificate has expired.
    CertificateExpired {
        /// Subject of the expired certificate.
        subject: String,
        /// Expiration timestamp in milliseconds since epoch.
        expired_at_ms: u64,
    },
    /// Certificate has been revoked.
    CertificateRevoked {
        /// Subject of the revoked certificate.
        subject: String,
        /// Serial number of the revoked certificate.
        serial: String,
    },
}

/// List of revoked certificates tracked by serial and fingerprint.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RevocationList {
    /// Revoked certificate serial numbers.
    pub revoked_serials: Vec<String>,
    /// Revoked certificate SHA-256 fingerprints.
    pub revoked_fingerprints: Vec<String>,
    /// Last update timestamp in milliseconds since epoch (0 = needs sync).
    pub last_updated_ms: u64,
}

impl RevocationList {
    /// Creates a new empty revocation list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a certificate serial number to the revocation list.
    pub fn revoke_serial(&mut self, serial: String) {
        if !self.revoked_serials.contains(&serial) {
            self.revoked_serials.push(serial);
            self.last_updated_ms = 0;
        }
    }

    /// Adds a certificate fingerprint to the revocation list.
    pub fn revoke_fingerprint(&mut self, fingerprint: String) {
        if !self.revoked_fingerprints.contains(&fingerprint) {
            self.revoked_fingerprints.push(fingerprint);
            self.last_updated_ms = 0;
        }
    }

    /// Checks if a certificate serial is in the revocation list.
    pub fn is_revoked_serial(&self, serial: &str) -> bool {
        self.revoked_serials.contains(&serial.to_string())
    }

    /// Checks if a certificate fingerprint is in the revocation list.
    pub fn is_revoked_fingerprint(&self, fingerprint: &str) -> bool {
        self.revoked_fingerprints.contains(&fingerprint.to_string())
    }

    /// Returns the total number of revoked entries.
    pub fn len(&self) -> usize {
        self.revoked_serials.len() + self.revoked_fingerprints.len()
    }

    /// Returns true if the revocation list is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Statistics for authentication operations.
#[derive(Debug, Clone, Default)]
pub struct AuthStats {
    /// Total number of allowed authentications.
    pub total_allowed: u64,
    /// Total number of denied authentications.
    pub total_denied: u64,
    /// Number of entries in the revocation list.
    pub revoked_count: usize,
}

/// Authenticator for validating client certificates against configuration rules.
pub struct ConnectionAuthenticator {
    config: AuthConfig,
    revocation_list: RevocationList,
    total_allowed: u64,
    total_denied: u64,
    current_time_ms: u64,
}

impl ConnectionAuthenticator {
    /// Creates a new authenticator with the given configuration.
    pub fn new(config: AuthConfig) -> Self {
        Self {
            config,
            revocation_list: RevocationList::new(),
            total_allowed: 0,
            total_denied: 0,
            current_time_ms: 0,
        }
    }

    /// Authenticates a certificate against the configured rules.
    ///
    /// Checks revocation, expiration, validity window, allowed lists,
    /// cluster CA requirements, and certificate age.
    pub fn authenticate(&mut self, cert: &CertificateInfo) -> AuthResult {
        if self.config.level == AuthLevel::None {
            self.total_allowed += 1;
            return AuthResult::Allowed {
                identity: cert.subject.clone(),
            };
        }

        if self.revocation_list.is_revoked_serial(&cert.serial)
            || self
                .revocation_list
                .is_revoked_fingerprint(&cert.fingerprint_sha256)
        {
            self.total_denied += 1;
            return AuthResult::CertificateRevoked {
                subject: cert.subject.clone(),
                serial: cert.serial.clone(),
            };
        }

        if self.current_time_ms > cert.not_after_ms {
            self.total_denied += 1;
            return AuthResult::CertificateExpired {
                subject: cert.subject.clone(),
                expired_at_ms: cert.not_after_ms,
            };
        }

        if self.current_time_ms < cert.not_before_ms {
            self.total_denied += 1;
            return AuthResult::Denied {
                reason: "certificate not yet valid".to_string(),
            };
        }

        if self.config.level == AuthLevel::MutualTlsStrict
            && !self.config.allowed_fingerprints.is_empty()
            && !self
                .config
                .allowed_fingerprints
                .contains(&cert.fingerprint_sha256)
        {
            self.total_denied += 1;
            return AuthResult::Denied {
                reason: "certificate fingerprint not in allowed list".to_string(),
            };
        }

        if !self.config.allowed_subjects.is_empty()
            && !self.config.allowed_subjects.contains(&cert.subject)
        {
            self.total_denied += 1;
            return AuthResult::Denied {
                reason: "certificate subject not in allowed list".to_string(),
            };
        }

        if self.config.require_cluster_ca {
            if let Some(ref ca_fingerprint) = self.config.cluster_ca_fingerprint {
                if !cert.issuer.contains(ca_fingerprint) {
                    self.total_denied += 1;
                    return AuthResult::Denied {
                        reason: "certificate not issued by expected cluster CA".to_string(),
                    };
                }
            }
        }

        let cert_age_days = if self.current_time_ms > cert.not_before_ms {
            (self.current_time_ms - cert.not_before_ms) / 86400000
        } else {
            0
        };
        if cert_age_days > self.config.max_cert_age_days as u64 {
            self.total_denied += 1;
            return AuthResult::Denied {
                reason: "certificate exceeds maximum age".to_string(),
            };
        }

        self.total_allowed += 1;
        AuthResult::Allowed {
            identity: cert.subject.clone(),
        }
    }

    /// Revokes a certificate by serial number.
    pub fn revoke_serial(&mut self, serial: String) {
        self.revocation_list.revoke_serial(serial);
    }

    /// Revokes a certificate by fingerprint.
    pub fn revoke_fingerprint(&mut self, fingerprint: String) {
        self.revocation_list.revoke_fingerprint(fingerprint);
    }

    /// Sets the current time for certificate validity checks.
    pub fn set_time(&mut self, ms: u64) {
        self.current_time_ms = ms;
    }

    /// Returns statistics for authentication operations.
    pub fn stats(&self) -> AuthStats {
        AuthStats {
            total_allowed: self.total_allowed,
            total_denied: self.total_denied,
            revoked_count: self.revocation_list.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cert(
        subject: &str,
        issuer: &str,
        serial: &str,
        fingerprint: &str,
        not_before_ms: u64,
        not_after_ms: u64,
    ) -> CertificateInfo {
        CertificateInfo {
            subject: subject.to_string(),
            issuer: issuer.to_string(),
            serial: serial.to_string(),
            fingerprint_sha256: fingerprint.to_string(),
            not_before_ms,
            not_after_ms,
            is_ca: false,
        }
    }

    #[test]
    fn test_config_default() {
        let config = AuthConfig::default();
        assert_eq!(config.level, AuthLevel::MutualTls);
        assert!(config.allowed_subjects.is_empty());
        assert!(config.allowed_fingerprints.is_empty());
        assert_eq!(config.max_cert_age_days, 365);
        assert!(config.require_cluster_ca);
        assert!(config.cluster_ca_fingerprint.is_none());
    }

    #[test]
    fn test_auth_level_none_allows_all() {
        let config = AuthConfig {
            level: AuthLevel::None,
            ..Default::default()
        };
        let mut auth = ConnectionAuthenticator::new(config);

        let cert = make_cert("bad", "bad", "01", "badfingerprint", 0, 1000);

        auth.set_time(500);
        let result = auth.authenticate(&cert);

        assert!(matches!(result, AuthResult::Allowed { identity } if identity == "bad"));
        assert_eq!(auth.stats().total_allowed, 1);
    }

    #[test]
    fn test_mutual_tls_allows_valid_cert() {
        let config = AuthConfig::default();
        let mut auth = ConnectionAuthenticator::new(config);

        let cert = make_cert(
            "server1",
            "ClusterCA",
            "01",
            "abc123",
            1000,
            86400000 * 365 * 1000 + 1000,
        );

        auth.set_time(5000);
        let result = auth.authenticate(&cert);

        assert!(matches!(result, AuthResult::Allowed { identity } if identity == "server1"));
    }

    #[test]
    fn test_expired_cert_denied() {
        let config = AuthConfig::default();
        let mut auth = ConnectionAuthenticator::new(config);

        let expired_ms: u64 = 86400000 * 365 * 1000 + 1000;
        let cert = make_cert("server1", "ClusterCA", "01", "abc123", 1000, expired_ms);

        auth.set_time(expired_ms + 1);
        let result = auth.authenticate(&cert);

        assert!(matches!(
            result,
            AuthResult::CertificateExpired { subject, expired_at_ms }
            if subject == "server1" && expired_at_ms == expired_ms
        ));
    }

    #[test]
    fn test_not_yet_valid_cert_denied() {
        let config = AuthConfig::default();
        let mut auth = ConnectionAuthenticator::new(config);

        let cert = make_cert(
            "server1",
            "ClusterCA",
            "01",
            "abc123",
            10000,
            86400000 * 365 * 1000 + 10000,
        );

        auth.set_time(5000);
        let result = auth.authenticate(&cert);

        assert!(
            matches!(result, AuthResult::Denied { reason } if reason == "certificate not yet valid")
        );
    }

    #[test]
    fn test_revoked_serial_denied() {
        let config = AuthConfig::default();
        let mut auth = ConnectionAuthenticator::new(config);

        auth.revoke_serial("01".to_string());

        let cert = make_cert(
            "server1",
            "ClusterCA",
            "01",
            "abc123",
            1000,
            86400000 * 365 * 1000 + 1000,
        );

        auth.set_time(5000);
        let result = auth.authenticate(&cert);

        assert!(matches!(
            result,
            AuthResult::CertificateRevoked { subject, serial }
            if subject == "server1" && serial == "01"
        ));
    }

    #[test]
    fn test_revoked_fingerprint_denied() {
        let config = AuthConfig::default();
        let mut auth = ConnectionAuthenticator::new(config);

        auth.revoke_fingerprint("abc123".to_string());

        let cert = make_cert(
            "server1",
            "ClusterCA",
            "01",
            "abc123",
            1000,
            86400000 * 365 * 1000 + 1000,
        );

        auth.set_time(5000);
        let result = auth.authenticate(&cert);

        assert!(matches!(
            result,
            AuthResult::CertificateRevoked { subject, serial }
            if subject == "server1" && serial == "01"
        ));
    }

    #[test]
    fn test_strict_mode_fingerprint_check() {
        let config = AuthConfig {
            level: AuthLevel::MutualTlsStrict,
            allowed_fingerprints: vec!["abc123".to_string(), "def456".to_string()],
            ..Default::default()
        };
        let mut auth = ConnectionAuthenticator::new(config);

        let cert = make_cert(
            "server1",
            "ClusterCA",
            "01",
            "unknownfp",
            1000,
            86400000 * 365 * 1000 + 1000,
        );

        auth.set_time(5000);
        let result = auth.authenticate(&cert);

        assert!(
            matches!(result, AuthResult::Denied { reason } if reason == "certificate fingerprint not in allowed list")
        );
    }

    #[test]
    fn test_allowed_subjects_filter() {
        let config = AuthConfig {
            allowed_subjects: vec!["server1".to_string(), "server2".to_string()],
            ..Default::default()
        };
        let mut auth = ConnectionAuthenticator::new(config);

        let cert = make_cert(
            "server3",
            "ClusterCA",
            "01",
            "abc123",
            1000,
            86400000 * 365 * 1000 + 1000,
        );

        auth.set_time(5000);
        let result = auth.authenticate(&cert);

        assert!(
            matches!(result, AuthResult::Denied { reason } if reason == "certificate subject not in allowed list")
        );
    }

    #[test]
    fn test_cert_too_old() {
        let config = AuthConfig {
            max_cert_age_days: 100,
            ..Default::default()
        };
        let mut auth = ConnectionAuthenticator::new(config);

        let old_not_before: u64 = 150 * 86400000;
        let not_after: u64 = 400 * 86400000;

        let cert = make_cert(
            "server1",
            "ClusterCA",
            "01",
            "abc123",
            old_not_before,
            not_after,
        );

        let current_time = old_not_before + 101 * 86400000;
        auth.set_time(current_time);
        let result = auth.authenticate(&cert);

        assert!(
            matches!(result, AuthResult::Denied { reason } if reason == "certificate exceeds maximum age")
        );
    }

    #[test]
    fn test_revocation_list_operations() {
        let mut rl = RevocationList::new();

        assert!(rl.is_empty());
        assert_eq!(rl.len(), 0);

        rl.revoke_serial("01".to_string());
        rl.revoke_serial("02".to_string());
        rl.revoke_fingerprint("fp1".to_string());

        assert_eq!(rl.len(), 3);
        assert!(!rl.is_empty());

        assert!(rl.is_revoked_serial("01"));
        assert!(rl.is_revoked_serial("02"));
        assert!(!rl.is_revoked_serial("03"));

        assert!(rl.is_revoked_fingerprint("fp1"));
        assert!(!rl.is_revoked_fingerprint("fp2"));

        rl.revoke_serial("01".to_string());
        assert_eq!(rl.len(), 3);
    }

    #[test]
    fn test_stats_tracking() {
        let config = AuthConfig {
            allowed_subjects: vec!["server1".to_string()],
            ..Default::default()
        };
        let mut auth = ConnectionAuthenticator::new(config);

        let cert = make_cert(
            "server1",
            "ClusterCA",
            "01",
            "abc123",
            1000,
            86400000 * 365 * 1000 + 1000,
        );

        auth.set_time(5000);

        let result1 = auth.authenticate(&cert);
        assert!(matches!(result1, AuthResult::Allowed { .. }));

        let cert2 = make_cert(
            "server2",
            "ClusterCA",
            "02",
            "unknown",
            1000,
            86400000 * 365 * 1000 + 1000,
        );
        let result2 = auth.authenticate(&cert2);
        assert!(matches!(result2, AuthResult::Denied { .. }));

        let stats = auth.stats();
        assert_eq!(stats.total_allowed, 1);
        assert_eq!(stats.total_denied, 1);
    }

    #[test]
    fn test_cluster_ca_validation() {
        let config = AuthConfig {
            require_cluster_ca: true,
            cluster_ca_fingerprint: Some("ExpectedCA".to_string()),
            ..Default::default()
        };
        let mut auth = ConnectionAuthenticator::new(config);

        let cert = make_cert(
            "server1",
            "OtherCA",
            "01",
            "abc123",
            1000,
            86400000 * 365 * 1000 + 1000,
        );

        auth.set_time(5000);
        let result = auth.authenticate(&cert);

        assert!(
            matches!(result, AuthResult::Denied { reason } if reason == "certificate not issued by expected cluster CA")
        );

        let cert_ok = make_cert(
            "server1",
            "ExpectedCAIssuer",
            "01",
            "abc123",
            1000,
            86400000 * 365 * 1000 + 1000,
        );
        let result_ok = auth.authenticate(&cert_ok);
        assert!(matches!(result_ok, AuthResult::Allowed { .. }));
    }
}
