//! TLS configuration for ClaudeFS gateway endpoints

use std::collections::HashMap;
use thiserror::Error;
use tracing::debug;

/// TLS protocol version
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsVersion {
    /// TLS 1.2
    Tls12,
    /// TLS 1.3
    Tls13,
}

/// TLS cipher suite preference
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CipherPreference {
    /// Modern: only strong ciphers
    Modern,
    /// Compatible: allows some older ciphers for compatibility
    Compatible,
    /// Legacy: allows legacy ciphers (not recommended)
    Legacy,
}

/// Certificate source
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CertSource {
    /// Load certificate from PEM files
    PemFiles {
        /// Path to certificate file
        cert_path: String,
        /// Path to private key file
        key_path: String,
    },
    /// Use certificate from memory
    InMemory {
        /// Certificate in PEM format
        cert_pem: Vec<u8>,
        /// Private key in PEM format
        key_pem: Vec<u8>,
    },
}

/// Client certificate validation mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientCertMode {
    /// No client certificate required
    None,
    /// Client certificate is optional
    Optional,
    /// Client certificate is required
    Required,
}

/// ALPN protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlpnProtocol {
    /// HTTP/1.1
    Http11,
    /// HTTP/2
    Http2,
    /// NFS
    Nfs,
}

/// TLS configuration for a gateway endpoint
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsConfig {
    /// Minimum TLS version
    pub min_version: TlsVersion,
    /// Cipher suite preference
    pub cipher_pref: CipherPreference,
    /// Certificate source
    pub cert_source: CertSource,
    /// Client certificate validation mode
    pub client_cert_mode: ClientCertMode,
    /// ALPN protocols to negotiate
    pub alpn_protocols: Vec<AlpnProtocol>,
    /// Session cache size
    pub session_cache_size: usize,
    /// Handshake timeout in milliseconds
    pub handshake_timeout_ms: u64,
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            min_version: TlsVersion::Tls13,
            cipher_pref: CipherPreference::Modern,
            cert_source: CertSource::PemFiles {
                cert_path: "/etc/claudefs/tls/server.crt".into(),
                key_path: "/etc/claudefs/tls/server.key".into(),
            },
            client_cert_mode: ClientCertMode::None,
            alpn_protocols: vec![AlpnProtocol::Http2, AlpnProtocol::Http11],
            session_cache_size: 1024,
            handshake_timeout_ms: 5000,
        }
    }
}

/// TLS configuration error
#[derive(Debug, Error)]
pub enum TlsConfigError {
    /// Certificate path is empty
    #[error("cert path is empty")]
    EmptyCertPath,
    /// Key path is empty
    #[error("key path is empty")]
    EmptyKeyPath,
    /// Session cache size must be > 0
    #[error("session cache size must be > 0")]
    InvalidSessionCacheSize,
    /// Handshake timeout must be > 0
    #[error("handshake timeout must be > 0")]
    InvalidHandshakeTimeout,
    /// At least one ALPN protocol required
    #[error("at least one ALPN protocol required")]
    NoAlpnProtocols,
}

/// Validates a TLS config and returns any issues found
pub struct TlsConfigValidator;

impl TlsConfigValidator {
    /// Validates a TLS configuration
    ///
    /// Checks:
    /// - If CertSource::PemFiles, paths must be non-empty
    /// - If CertSource::InMemory, cert and key must be non-empty
    /// - session_cache_size must be > 0
    /// - handshake_timeout_ms must be > 0
    /// - At least one ALPN protocol required
    pub fn validate(config: &TlsConfig) -> Result<(), TlsConfigError> {
        debug!("validating TLS config");

        match &config.cert_source {
            CertSource::PemFiles {
                cert_path,
                key_path,
            } => {
                if cert_path.is_empty() {
                    debug!("validation failed: empty cert path");
                    return Err(TlsConfigError::EmptyCertPath);
                }
                if key_path.is_empty() {
                    debug!("validation failed: empty key path");
                    return Err(TlsConfigError::EmptyKeyPath);
                }
            }
            CertSource::InMemory { cert_pem, key_pem } => {
                if cert_pem.is_empty() {
                    debug!("validation failed: empty in-memory cert");
                    return Err(TlsConfigError::EmptyCertPath);
                }
                if key_pem.is_empty() {
                    debug!("validation failed: empty in-memory key");
                    return Err(TlsConfigError::EmptyKeyPath);
                }
            }
        }

        if config.session_cache_size == 0 {
            debug!("validation failed: invalid session cache size");
            return Err(TlsConfigError::InvalidSessionCacheSize);
        }

        if config.handshake_timeout_ms == 0 {
            debug!("validation failed: invalid handshake timeout");
            return Err(TlsConfigError::InvalidHandshakeTimeout);
        }

        if config.alpn_protocols.is_empty() {
            debug!("validation failed: no ALPN protocols");
            return Err(TlsConfigError::NoAlpnProtocols);
        }

        debug!("TLS config validation passed");
        Ok(())
    }
}

/// TLS endpoint binding configuration
#[derive(Debug, Clone, PartialEq)]
pub struct TlsEndpoint {
    /// Address to listen on
    pub listen_addr: String,
    /// Port to bind to
    pub port: u16,
    /// TLS configuration
    pub config: TlsConfig,
    /// Whether TLS is enabled
    pub enabled: bool,
}

impl TlsEndpoint {
    /// Creates a new TLS endpoint that is enabled by default
    pub fn new(listen_addr: &str, port: u16, config: TlsConfig) -> Self {
        Self {
            listen_addr: listen_addr.to_string(),
            port,
            config,
            enabled: true,
        }
    }

    /// Disables TLS for this endpoint
    pub fn disable(&mut self) {
        self.enabled = false;
        debug!("TLS endpoint disabled: {}:{}", self.listen_addr, self.port);
    }

    /// Enables TLS for this endpoint
    pub fn enable(&mut self) {
        self.enabled = true;
        debug!("TLS endpoint enabled: {}:{}", self.listen_addr, self.port);
    }

    /// Returns the bind address as "addr:port"
    pub fn bind_address(&self) -> String {
        format!("{}:{}", self.listen_addr, self.port)
    }
}

/// Registry of TLS endpoints (S3 HTTPS, secure NFS)
#[derive(Debug, Default)]
pub struct TlsRegistry {
    endpoints: HashMap<String, TlsEndpoint>,
}

impl TlsRegistry {
    /// Creates a new empty TLS registry
    pub fn new() -> Self {
        Self {
            endpoints: HashMap::new(),
        }
    }

    /// Registers a TLS endpoint with a name
    pub fn register(&mut self, name: &str, endpoint: TlsEndpoint) {
        debug!("registering TLS endpoint: {}", name);
        self.endpoints.insert(name.to_string(), endpoint);
    }

    /// Gets a TLS endpoint by name
    pub fn get(&self, name: &str) -> Option<&TlsEndpoint> {
        self.endpoints.get(name)
    }

    /// Removes and returns a TLS endpoint by name
    pub fn remove(&mut self, name: &str) -> Option<TlsEndpoint> {
        debug!("removing TLS endpoint: {}", name);
        self.endpoints.remove(name)
    }

    /// Returns the count of enabled TLS endpoints
    pub fn enabled_count(&self) -> usize {
        self.endpoints.values().filter(|e| e.enabled).count()
    }

    /// Returns all registered endpoint names
    pub fn all_names(&self) -> Vec<String> {
        self.endpoints.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tls_config_default_values() {
        let config = TlsConfig::default();
        assert_eq!(config.min_version, TlsVersion::Tls13);
        assert_eq!(config.cipher_pref, CipherPreference::Modern);
        assert_eq!(config.session_cache_size, 1024);
        assert_eq!(config.handshake_timeout_ms, 5000);
        assert_eq!(config.client_cert_mode, ClientCertMode::None);
        assert_eq!(config.alpn_protocols.len(), 2);
    }

    #[test]
    fn test_tls_config_validator_accepts_valid_config() {
        let config = TlsConfig::default();
        assert!(TlsConfigValidator::validate(&config).is_ok());
    }

    #[test]
    fn test_tls_config_validator_rejects_empty_cert_path() {
        let mut config = TlsConfig::default();
        config.cert_source = CertSource::PemFiles {
            cert_path: "".into(),
            key_path: "/path/to/key".into(),
        };
        let result = TlsConfigValidator::validate(&config);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TlsConfigError::EmptyCertPath));
    }

    #[test]
    fn test_tls_config_validator_rejects_empty_key_path() {
        let mut config = TlsConfig::default();
        config.cert_source = CertSource::PemFiles {
            cert_path: "/path/to/cert".into(),
            key_path: "".into(),
        };
        let result = TlsConfigValidator::validate(&config);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TlsConfigError::EmptyKeyPath));
    }

    #[test]
    fn test_tls_config_validator_rejects_session_cache_size_zero() {
        let mut config = TlsConfig::default();
        config.session_cache_size = 0;
        let result = TlsConfigValidator::validate(&config);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TlsConfigError::InvalidSessionCacheSize
        ));
    }

    #[test]
    fn test_tls_config_validator_rejects_handshake_timeout_zero() {
        let mut config = TlsConfig::default();
        config.handshake_timeout_ms = 0;
        let result = TlsConfigValidator::validate(&config);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TlsConfigError::InvalidHandshakeTimeout
        ));
    }

    #[test]
    fn test_tls_config_validator_rejects_empty_alpn_protocols() {
        let mut config = TlsConfig::default();
        config.alpn_protocols = vec![];
        let result = TlsConfigValidator::validate(&config);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TlsConfigError::NoAlpnProtocols
        ));
    }

    #[test]
    fn test_tls_version_variants() {
        assert_eq!(TlsVersion::Tls12, TlsVersion::Tls12);
        assert_eq!(TlsVersion::Tls13, TlsVersion::Tls13);
    }

    #[test]
    fn test_cipher_preference_variants() {
        assert_eq!(CipherPreference::Modern, CipherPreference::Modern);
        assert_eq!(CipherPreference::Compatible, CipherPreference::Compatible);
        assert_eq!(CipherPreference::Legacy, CipherPreference::Legacy);
    }

    #[test]
    fn test_client_cert_mode_required_validation_path() {
        let mut config = TlsConfig::default();
        config.client_cert_mode = ClientCertMode::Required;
        assert!(TlsConfigValidator::validate(&config).is_ok());
    }

    #[test]
    fn test_tls_endpoint_new_creates_enabled_endpoint() {
        let config = TlsConfig::default();
        let endpoint = TlsEndpoint::new("0.0.0.0", 9000, config);
        assert_eq!(endpoint.listen_addr, "0.0.0.0");
        assert_eq!(endpoint.port, 9000);
        assert!(endpoint.enabled);
    }

    #[test]
    fn test_tls_endpoint_disable_sets_enabled_false() {
        let config = TlsConfig::default();
        let mut endpoint = TlsEndpoint::new("0.0.0.0", 9000, config);
        endpoint.disable();
        assert!(!endpoint.enabled);
    }

    #[test]
    fn test_tls_endpoint_enable_sets_enabled_true() {
        let config = TlsConfig::default();
        let mut endpoint = TlsEndpoint::new("0.0.0.0", 9000, config);
        endpoint.enabled = false;
        endpoint.enable();
        assert!(endpoint.enabled);
    }

    #[test]
    fn test_tls_endpoint_bind_address_returns_addr_port() {
        let config = TlsConfig::default();
        let endpoint = TlsEndpoint::new("192.168.1.1", 8080, config);
        assert_eq!(endpoint.bind_address(), "192.168.1.1:8080");
    }

    #[test]
    fn test_tls_registry_new_is_empty() {
        let registry = TlsRegistry::new();
        assert_eq!(registry.enabled_count(), 0);
        assert!(registry.all_names().is_empty());
    }

    #[test]
    fn test_tls_registry_register_adds_endpoint() {
        let mut registry = TlsRegistry::new();
        let config = TlsConfig::default();
        let endpoint = TlsEndpoint::new("0.0.0.0", 9000, config);
        registry.register("s3", endpoint);
        assert_eq!(registry.all_names(), vec!["s3"]);
    }

    #[test]
    fn test_tls_registry_get_returns_some_for_registered_name() {
        let mut registry = TlsRegistry::new();
        let config = TlsConfig::default();
        let endpoint = TlsEndpoint::new("0.0.0.0", 9000, config);
        registry.register("s3", endpoint);
        assert!(registry.get("s3").is_some());
    }

    #[test]
    fn test_tls_registry_get_returns_none_for_unknown_name() {
        let registry = TlsRegistry::new();
        assert!(registry.get("unknown").is_none());
    }

    #[test]
    fn test_tls_registry_remove_returns_the_endpoint() {
        let mut registry = TlsRegistry::new();
        let config = TlsConfig::default();
        let endpoint = TlsEndpoint::new("0.0.0.0", 9000, config);
        registry.register("s3", endpoint);
        let removed = registry.remove("s3");
        assert!(removed.is_some());
        assert!(registry.get("s3").is_none());
    }

    #[test]
    fn test_tls_registry_enabled_count_counts_only_enabled() {
        let mut registry = TlsRegistry::new();

        let config = TlsConfig::default();
        let mut endpoint1 = TlsEndpoint::new("0.0.0.0", 9000, config.clone());
        registry.register("s3", endpoint1);

        let mut endpoint2 = TlsEndpoint::new("0.0.0.0", 2049, config);
        endpoint2.disable();
        registry.register("nfs", endpoint2);

        assert_eq!(registry.enabled_count(), 1);
    }

    #[test]
    fn test_tls_registry_all_names_returns_all_names() {
        let mut registry = TlsRegistry::new();
        let config = TlsConfig::default();

        let endpoint1 = TlsEndpoint::new("0.0.0.0", 9000, config.clone());
        let endpoint2 = TlsEndpoint::new("0.0.0.0", 2049, config);

        registry.register("s3", endpoint1);
        registry.register("nfs", endpoint2);

        let names = registry.all_names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"s3".to_string()));
        assert!(names.contains(&"nfs".to_string()));
    }

    #[test]
    fn test_in_memory_cert_source_validation() {
        let mut config = TlsConfig::default();
        config.cert_source = CertSource::InMemory {
            cert_pem: vec![],
            key_pem: vec![1, 2, 3, 4],
        };
        let result = TlsConfigValidator::validate(&config);
        assert!(result.is_err());

        config.cert_source = CertSource::InMemory {
            cert_pem: vec![1, 2, 3, 4],
            key_pem: vec![],
        };
        let result = TlsConfigValidator::validate(&config);
        assert!(result.is_err());

        config.cert_source = CertSource::InMemory {
            cert_pem: vec![1, 2, 3, 4],
            key_pem: vec![1, 2, 3, 4],
        };
        assert!(TlsConfigValidator::validate(&config).is_ok());
    }
}
