//! TLS/mTLS support for ClaudeFS transport layer.
//!
//! Provides mutual TLS authentication for all inter-node communication.
//! Per architecture decision D7, all internal communication uses mTLS with
//! cluster-issued certificates from a self-contained cluster CA.

use crate::error::{Result, TransportError};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use std::fmt;
use std::sync::Arc;

pub use tokio_rustls::TlsConnector as TlsConnectorInner;
pub use tokio_rustls::TlsAcceptor as TlsAcceptorInner;

/// Configuration for TLS/mTLS connections.
///
/// Contains all necessary certificates and keys for establishing
/// mutual TLS connections between cluster nodes.
#[derive(Debug, Clone)]
pub struct TlsConfig {
    /// PEM-encoded CA certificate for verifying peer certificates.
    pub ca_cert_pem: Vec<u8>,
    /// PEM-encoded certificate chain (node cert + intermediates).
    pub cert_chain_pem: Vec<u8>,
    /// PEM-encoded private key for this node.
    pub private_key_pem: Vec<u8>,
    /// Whether to require client certificates (true for mTLS).
    pub require_client_auth: bool,
}

impl TlsConfig {
    /// Creates a new TLS configuration.
    pub fn new(
        ca_cert_pem: Vec<u8>,
        cert_chain_pem: Vec<u8>,
        private_key_pem: Vec<u8>,
        require_client_auth: bool,
    ) -> Self {
        Self {
            ca_cert_pem,
            cert_chain_pem,
            private_key_pem,
            require_client_auth,
        }
    }
}

/// Client-side TLS connector for establishing mTLS connections.
pub struct TlsConnector {
    inner: TlsConnectorInner,
}

impl fmt::Debug for TlsConnector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsConnector").finish()
    }
}

impl TlsConnector {
    /// Creates a new TLS connector from configuration.
    pub fn new(config: &TlsConfig) -> Result<Self> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let certs = load_certs_from_pem(&config.cert_chain_pem)?;
        let key = load_private_key_from_pem(&config.private_key_pem)?;
        let ca_certs = load_certs_from_pem(&config.ca_cert_pem)?;

        let mut root_store = rustls::RootCertStore::empty();
        for cert in ca_certs {
            root_store.add(cert).map_err(|e| TransportError::TlsError {
                reason: format!("failed to add CA cert: {}", e),
            })?;
        }

        let client_config = rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_client_auth_cert(certs, key)
            .map_err(|e| TransportError::TlsError {
                reason: format!("failed to create client config: {}", e),
            })?;

        let inner = TlsConnectorInner::from(Arc::new(client_config));
        Ok(Self { inner })
    }

    /// Establishes a TLS connection over an existing stream.
    pub async fn connect<IO>(&self, domain: &str, stream: IO) -> Result<TlsStream<IO>>
    where
        IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        let domain = if let Ok(ip) = domain.parse::<std::net::IpAddr>() {
            ServerName::IpAddress(ip.into())
        } else {
            ServerName::try_from(domain.to_string()).map_err(|e| TransportError::TlsError {
                reason: format!("invalid domain: {}", e),
            })?
        };
        let stream = self.inner.connect(domain, stream).await.map_err(|e| TransportError::TlsError {
            reason: format!("TLS handshake failed: {}", e),
        })?;
        Ok(TlsStream::Client(stream))
    }
}

/// Server-side TLS acceptor for accepting mTLS connections.
pub struct TlsAcceptor {
    inner: TlsAcceptorInner,
}

impl fmt::Debug for TlsAcceptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsAcceptor").finish()
    }
}

impl TlsAcceptor {
    /// Creates a new TLS acceptor from configuration.
    pub fn new(config: &TlsConfig) -> Result<Self> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let certs = load_certs_from_pem(&config.cert_chain_pem)?;
        let key = load_private_key_from_pem(&config.private_key_pem)?;

        let server_config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| TransportError::TlsError {
                reason: format!("failed to set server cert: {}", e),
            })?;

        let inner = TlsAcceptorInner::from(Arc::new(server_config));
        Ok(Self { inner })
    }

    /// Accepts a TLS connection over an existing stream.
    pub async fn accept<IO>(&self, stream: IO) -> Result<TlsStream<IO>>
    where
        IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        let stream = self.inner.accept(stream).await.map_err(|e| TransportError::TlsError {
            reason: format!("TLS handshake failed: {}", e),
        })?;
        Ok(TlsStream::Server(stream))
    }
}

/// A TLS stream that can be either client or server side.
///
/// Provides access to peer certificates for mTLS authentication.
#[derive(Debug)]
pub enum TlsStream<IO> {
    /// Client-side TLS stream.
    Client(tokio_rustls::client::TlsStream<IO>),
    /// Server-side TLS stream.
    Server(tokio_rustls::server::TlsStream<IO>),
}

impl<IO> TlsStream<IO>
where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    /// Returns the peer's certificate chain, if available.
    pub fn peer_certificates(&self) -> Option<Vec<Vec<u8>>> {
        match self {
            TlsStream::Client(s) => {
                let conn = &s.get_ref().1;
                conn.peer_certificates().map(|certs| {
                    certs.iter().map(|c| c.as_ref().to_vec()).collect()
                })
            }
            TlsStream::Server(s) => {
                let conn = &s.get_ref().1;
                conn.peer_certificates().map(|certs| {
                    certs.iter().map(|c| c.as_ref().to_vec()).collect()
                })
            }
        }
    }

    /// Splits the TLS stream into read and write halves.
    pub fn split_owned(self: TlsStream<IO>) -> (
        tokio::io::ReadHalf<TlsStream<IO>>,
        tokio::io::WriteHalf<TlsStream<IO>>,
    ) {
        tokio::io::split(self)
    }
}

impl<IO> tokio::io::AsyncRead for TlsStream<IO>
where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match &mut *self {
            TlsStream::Client(s) => std::pin::Pin::new(s).poll_read(cx, buf),
            TlsStream::Server(s) => std::pin::Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl<IO> tokio::io::AsyncWrite for TlsStream<IO>
where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::result::Result<usize, std::io::Error>> {
        match &mut *self {
            TlsStream::Client(s) => std::pin::Pin::new(s).poll_write(cx, buf),
            TlsStream::Server(s) => std::pin::Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::result::Result<(), std::io::Error>> {
        match &mut *self {
            TlsStream::Client(s) => std::pin::Pin::new(s).poll_flush(cx),
            TlsStream::Server(s) => std::pin::Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::result::Result<(), std::io::Error>> {
        match &mut *self {
            TlsStream::Client(s) => std::pin::Pin::new(s).poll_shutdown(cx),
            TlsStream::Server(s) => std::pin::Pin::new(s).poll_shutdown(cx),
        }
    }
}

/// Loads certificates from PEM-encoded data.
pub fn load_certs_from_pem(pem: &[u8]) -> Result<Vec<CertificateDer<'static>>> {
    let mut certs = Vec::new();
    let mut cursor = std::io::Cursor::new(pem);
    while let Ok(Some(rustls_pemfile::Item::X509Certificate(cert))) =
        rustls_pemfile::read_one(&mut cursor)
    {
        certs.push(cert);
    }

    if certs.is_empty() {
        return Err(TransportError::TlsError {
            reason: "no certificates found in PEM".to_string(),
        });
    }

    Ok(certs)
}

/// Loads a private key from PEM-encoded data.
pub fn load_private_key_from_pem(pem: &[u8]) -> Result<PrivateKeyDer<'static>> {
    let mut cursor = std::io::Cursor::new(pem);
    if let Ok(Some(rustls_pemfile::Item::Pkcs8Key(key))) = rustls_pemfile::read_one(&mut cursor) {
        return Ok(PrivateKeyDer::Pkcs8(key));
    }

    Err(TransportError::TlsError {
        reason: "no private key found in PEM".to_string(),
    })
}

/// Generates a self-signed CA certificate and key pair using rcgen.
///
/// Returns (CA certificate PEM, CA key PEM).
pub fn generate_self_signed_ca() -> Result<(Vec<u8>, Vec<u8>)> {
    let key_pair = rcgen::KeyPair::generate().map_err(|e| TransportError::TlsError {
        reason: format!("failed to generate CA key: {}", e),
    })?;

    let mut params = rcgen::CertificateParams::default();
    params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);

    let cert = params.self_signed(&key_pair).map_err(|e| TransportError::TlsError {
        reason: format!("failed to create CA certificate: {}", e),
    })?;

    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();

    Ok((cert_pem.into_bytes(), key_pem.into_bytes()))
}

/// Generates a node certificate signed by the cluster CA.
///
/// Returns (node certificate PEM, node key PEM).
pub fn generate_node_cert(
    ca_cert_pem: &[u8],
    ca_key_pem: &[u8],
    node_name: &str,
) -> Result<(Vec<u8>, Vec<u8>)> {
    let ca_key = rcgen::KeyPair::from_pem(std::str::from_utf8(ca_key_pem).map_err(
        |e| TransportError::TlsError {
            reason: format!("invalid CA key PEM: {}", e),
        },
    )?)
    .map_err(|e| TransportError::TlsError {
        reason: format!("failed to parse CA key: {}", e),
    })?;

    let ca_cert_pem_str = std::str::from_utf8(ca_cert_pem).map_err(|e| TransportError::TlsError {
        reason: format!("invalid CA cert PEM: {}", e),
    })?;

    let ca_cert_params = rcgen::CertificateParams::from_ca_cert_pem(ca_cert_pem_str)
        .map_err(|e| TransportError::TlsError {
            reason: format!("failed to parse CA certificate: {}", e),
        })?;

    let ca_cert = ca_cert_params.self_signed(&ca_key).map_err(|e| TransportError::TlsError {
        reason: format!("failed to reconstruct CA certificate: {}", e),
    })?;

    let node_key = rcgen::KeyPair::generate().map_err(|e| TransportError::TlsError {
        reason: format!("failed to generate node key: {}", e),
    })?;

    let node_params = rcgen::CertificateParams::new(vec![node_name.to_string()])
        .map_err(|e| TransportError::TlsError {
            reason: format!("failed to create node certificate params: {}", e),
        })?;

    let node_cert = node_params
        .signed_by(&node_key, &ca_cert, &ca_key)
        .map_err(|e| TransportError::TlsError {
            reason: format!("failed to sign node certificate: {}", e),
        })?;

    let cert_pem = node_cert.pem();
    let key_pem = node_key.serialize_pem();

    Ok((cert_pem.into_bytes(), key_pem.into_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_self_signed_ca() {
        let (ca_cert_pem, ca_key_pem) = generate_self_signed_ca().unwrap();
        assert!(!ca_cert_pem.is_empty());
        assert!(!ca_key_pem.is_empty());
        assert!(String::from_utf8_lossy(&ca_cert_pem).contains("BEGIN CERTIFICATE"));
        assert!(String::from_utf8_lossy(&ca_key_pem).contains("BEGIN PRIVATE KEY"));
    }

    #[test]
    fn test_generate_node_cert() {
        let (ca_cert_pem, ca_key_pem) = generate_self_signed_ca().unwrap();
        let (node_cert_pem, node_key_pem) =
            generate_node_cert(&ca_cert_pem, &ca_key_pem, "node1").unwrap();
        assert!(!node_cert_pem.is_empty());
        assert!(!node_key_pem.is_empty());
        assert!(String::from_utf8_lossy(&node_cert_pem).contains("BEGIN CERTIFICATE"));
        assert!(String::from_utf8_lossy(&node_key_pem).contains("BEGIN PRIVATE KEY"));
    }

    #[test]
    fn test_tls_config_creation() {
        let (ca_cert_pem, _ca_key_pem) = generate_self_signed_ca().unwrap();
        let (cert_chain_pem, private_key_pem) =
            generate_node_cert(&ca_cert_pem, &_ca_key_pem, "test-node").unwrap();

        let config = TlsConfig::new(
            ca_cert_pem.clone(),
            cert_chain_pem.clone(),
            private_key_pem.clone(),
            true,
        );

        let _connector = TlsConnector::new(&config).unwrap();
        let _acceptor = TlsAcceptor::new(&config).unwrap();
    }

    #[test]
    fn test_load_certs_from_pem() {
        let (ca_cert_pem, _ca_key_pem) = generate_self_signed_ca().unwrap();
        let certs = load_certs_from_pem(&ca_cert_pem).unwrap();
        assert!(!certs.is_empty());
    }

    #[test]
    fn test_load_private_key_from_pem() {
        let (_ca_cert_pem, ca_key_pem) = generate_self_signed_ca().unwrap();
        let key = load_private_key_from_pem(&ca_key_pem).unwrap();
        assert!(!key.secret_der().is_empty());
    }
}