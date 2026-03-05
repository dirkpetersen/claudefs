Add missing Rust doc comments to the conn_auth.rs module for the claudefs-transport crate.

The file has #![warn(missing_docs)] enabled at the crate level, so ALL public items need doc comments.

Rules:
- Add `/// <doc comment>` immediately before each public item that lacks one
- Do NOT modify any existing code, logic, tests, or existing doc comments
- Do NOT add comments to private/internal items (only pub items)
- Keep doc comments concise and accurate to what the code does
- Output the COMPLETE file with ALL the added doc comments

Items needing docs:
- AuthLevel enum and all its variants (None, TlsOnly, MutualTls, MutualTlsStrict)
- CertificateInfo struct and all its fields (subject, issuer, serial, fingerprint_sha256, not_before_ms, not_after_ms, is_ca)
- AuthConfig struct and all its fields (level, allowed_subjects, allowed_fingerprints, max_cert_age_days, require_cluster_ca, cluster_ca_fingerprint)
- AuthResult enum and all its variants (Allowed, Denied, CertificateExpired, CertificateRevoked)
- RevocationList struct and all its fields (revoked_serials, revoked_fingerprints, last_updated_ms) and methods (new, revoke_serial, revoke_fingerprint, is_revoked_serial, is_revoked_fingerprint, len, is_empty)
- AuthStats struct and all its fields (total_allowed, total_denied, revoked_count)
- ConnectionAuthenticator struct and all its methods (new, authenticate, revoke_serial, revoke_fingerprint, set_time, stats)

Here is the COMPLETE current file content:

//! Connection authentication module for mTLS and certificate handling.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AuthLevel {
    None,
    TlsOnly,
    #[default]
    MutualTls,
    MutualTlsStrict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertificateInfo {
    pub subject: String,
    pub issuer: String,
    pub serial: String,
    pub fingerprint_sha256: String,
    pub not_before_ms: u64,
    pub not_after_ms: u64,
    pub is_ca: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub level: AuthLevel,
    pub allowed_subjects: Vec<String>,
    pub allowed_fingerprints: Vec<String>,
    pub max_cert_age_days: u32,
    pub require_cluster_ca: bool,
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

#[derive(Debug, Clone)]
pub enum AuthResult {
    Allowed { identity: String },
    Denied { reason: String },
    CertificateExpired { subject: String, expired_at_ms: u64 },
    CertificateRevoked { subject: String, serial: String },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RevocationList {
    pub revoked_serials: Vec<String>,
    pub revoked_fingerprints: Vec<String>,
    pub last_updated_ms: u64,
}

impl RevocationList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn revoke_serial(&mut self, serial: String) {
        if !self.revoked_serials.contains(&serial) {
            self.revoked_serials.push(serial);
            self.last_updated_ms = 0;
        }
    }

    pub fn revoke_fingerprint(&mut self, fingerprint: String) {
        if !self.revoked_fingerprints.contains(&fingerprint) {
            self.revoked_fingerprints.push(fingerprint);
            self.last_updated_ms = 0;
        }
    }

    pub fn is_revoked_serial(&self, serial: &str) -> bool {
        self.revoked_serials.contains(&serial.to_string())
    }

    pub fn is_revoked_fingerprint(&self, fingerprint: &str) -> bool {
        self.revoked_fingerprints.contains(&fingerprint.to_string())
    }

    pub fn len(&self) -> usize {
        self.revoked_serials.len() + self.revoked_fingerprints.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Clone, Default)]
pub struct AuthStats {
    pub total_allowed: u64,
    pub total_denied: u64,
    pub revoked_count: usize,
}

pub struct ConnectionAuthenticator {
    config: AuthConfig,
    revocation_list: RevocationList,
    total_allowed: u64,
    total_denied: u64,
    current_time_ms: u64,
}

impl ConnectionAuthenticator {
    pub fn new(config: AuthConfig) -> Self {
        Self {
            config,
            revocation_list: RevocationList::new(),
            total_allowed: 0,
            total_denied: 0,
            current_time_ms: 0,
        }
    }

    pub fn authenticate(&mut self, cert: &CertificateInfo) -> AuthResult {
        // ... implementation ...
    }

    pub fn revoke_serial(&mut self, serial: String) {
        self.revocation_list.revoke_serial(serial);
    }

    pub fn revoke_fingerprint(&mut self, fingerprint: String) {
        self.revocation_list.revoke_fingerprint(fingerprint);
    }

    pub fn set_time(&mut self, ms: u64) {
        self.current_time_ms = ms;
    }

    pub fn stats(&self) -> AuthStats {
        AuthStats {
            total_allowed: self.total_allowed,
            total_denied: self.total_denied,
            revoked_count: self.revocation_list.len(),
        }
    }
}

Output the COMPLETE conn_auth.rs file with all missing doc comments added. Use the ORIGINAL file content (with the full authenticate implementation). Output ONLY the Rust source code, no markdown fences.
