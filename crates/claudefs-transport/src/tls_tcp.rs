//! TLS-wrapped TCP transport implementation.
//!
//! Provides secure TCP transport using TLS/mTLS for inter-node communication.

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

use crate::error::{Result, TransportError};
use crate::protocol::{Frame, FrameHeader, FRAME_HEADER_SIZE, MAX_PAYLOAD_SIZE};
use crate::tcp::TcpTransportConfig;
use crate::tls::{TlsAcceptor, TlsConfig, TlsConnector, TlsStream};
use crate::transport::{Connection, Listener, Transport};

/// TLS-TCP transport configuration.
#[derive(Debug, Clone)]
pub struct TlsTcpTransportConfig {
    /// Underlying TCP transport configuration.
    pub tcp: TcpTransportConfig,
    /// TLS configuration.
    pub tls: TlsConfig,
}

impl Default for TlsTcpTransportConfig {
    fn default() -> Self {
        Self {
            tcp: TcpTransportConfig::default(),
            tls: TlsConfig::new(vec![], vec![], vec![], true),
        }
    }
}

/// TLS-wrapped TCP transport that creates secure connections.
#[derive(Debug, Clone)]
pub struct TlsTcpTransport {
    tcp_config: TcpTransportConfig,
    tls_config: TlsConfig,
}

impl TlsTcpTransport {
    /// Creates a new TLS-TCP transport with the given configurations.
    pub fn new(tcp_config: TcpTransportConfig, tls_config: TlsConfig) -> Result<Self> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        Ok(Self {
            tcp_config,
            tls_config,
        })
    }
}

#[async_trait]
impl Transport for TlsTcpTransport {
    async fn connect(&self, addr: &str) -> Result<Box<dyn Connection>> {
        let timeout =
            std::time::Duration::from_millis(self.tcp_config.connect_timeout_ms);
        let stream = tokio::time::timeout(timeout, tokio::net::TcpStream::connect(addr))
            .await
            .map_err(|_| TransportError::ConnectionTimeout {
                addr: addr.to_string(),
                timeout_ms: self.tcp_config.connect_timeout_ms,
            })?
            .map_err(TransportError::IoError)?;

        let local_addr = stream.local_addr().map(|a| a.to_string()).unwrap_or_default();

        if self.tcp_config.nodelay {
            stream.set_nodelay(true).map_err(TransportError::IoError)?;
        }

        let connector = TlsConnector::new(&self.tls_config)?;
        let host = addr.rsplit_once(':').map(|(h, _)| h).unwrap_or(addr);
        let tls_stream = connector
            .connect(host, stream)
            .await
            .map_err(|e| TransportError::TlsError {
                reason: format!("TLS connect failed: {}", e),
            })?;

        let peer_addr = addr.to_string();

        tracing::debug!(addr = addr, "TLS-TCP connected");

        let conn = TlsTcpConnection::new(tls_stream, peer_addr, local_addr);
        Ok(Box::new(conn))
    }

    async fn listen(&self, addr: &str) -> Result<Box<dyn Listener>> {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(TransportError::IoError)?;
        tracing::debug!(addr = addr, "TLS-TCP listening");

        let acceptor = TlsAcceptor::new(&self.tls_config)?;
        Ok(Box::new(TlsTcpListener::new(listener, acceptor)))
    }
}

/// A single TLS-TCP connection with concurrent read/write support.
pub struct TlsTcpConnection {
    read: Mutex<tokio::io::ReadHalf<TlsStream<tokio::net::TcpStream>>>,
    write: Mutex<tokio::io::WriteHalf<TlsStream<tokio::net::TcpStream>>>,
    peer_addr: String,
    local_addr: String,
}

impl std::fmt::Debug for TlsTcpConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsTcpConnection")
            .field("peer_addr", &self.peer_addr)
            .field("local_addr", &self.local_addr)
            .finish()
    }
}

impl TlsTcpConnection {
    pub(crate) fn new(stream: TlsStream<tokio::net::TcpStream>, peer_addr: String, local_addr: String) -> Self {
        let (read, write) = stream.split_owned();
        let read = Mutex::new(read);
        let write = Mutex::new(write);
        Self {
            read,
            write,
            peer_addr,
            local_addr,
        }
    }

    /// Sends a frame over the TLS-TCP connection.
    pub async fn send_frame(&self, frame: &Frame) -> Result<()> {
        let encoded = frame.encode();
        let mut write = self.write.lock().await;
        write.write_all(&encoded).await.map_err(TransportError::IoError)?;
        write.flush().await.map_err(TransportError::IoError)?;
        Ok(())
    }

    /// Receives a frame from the TLS-TCP connection.
    pub async fn recv_frame(&self) -> Result<Frame> {
        let mut read = self.read.lock().await;
        let mut header_buf = [0u8; FRAME_HEADER_SIZE];
        read.read_exact(&mut header_buf)
            .await
            .map_err(TransportError::IoError)?;
        let header = FrameHeader::decode(&header_buf)?;
        if header.payload_length > MAX_PAYLOAD_SIZE {
            return Err(TransportError::PayloadTooLarge {
                size: header.payload_length,
                max_size: MAX_PAYLOAD_SIZE,
            });
        }
        let mut payload = vec![0u8; header.payload_length as usize];
        if !payload.is_empty() {
            read.read_exact(&mut payload)
                .await
                .map_err(TransportError::IoError)?;
        }
        let frame = Frame { header, payload };
        frame.validate()?;
        Ok(frame)
    }

    /// Returns the remote peer address of this connection.
    pub fn peer_addr(&self) -> &str {
        &self.peer_addr
    }

    /// Returns the local address of this connection.
    pub fn local_addr(&self) -> &str {
        &self.local_addr
    }
}

#[async_trait]
impl Connection for TlsTcpConnection {
    async fn send_frame(&self, frame: &Frame) -> Result<()> {
        self.send_frame(frame).await
    }

    async fn recv_frame(&self) -> Result<Frame> {
        self.recv_frame().await
    }

    fn peer_addr(&self) -> &str {
        self.peer_addr()
    }

    fn local_addr(&self) -> &str {
        self.local_addr()
    }
}

/// TLS-TCP listener that accepts incoming TLS connections.
#[derive(Debug)]
pub struct TlsTcpListener {
    tcp_listener: tokio::net::TcpListener,
    tls_acceptor: TlsAcceptor,
}

impl TlsTcpListener {
    fn new(tcp_listener: tokio::net::TcpListener, tls_acceptor: TlsAcceptor) -> Self {
        Self {
            tcp_listener,
            tls_acceptor,
        }
    }
}

#[async_trait]
impl Listener for TlsTcpListener {
    async fn accept(&self) -> Result<Box<dyn Connection>> {
        let (stream, remote_addr) = self
            .tcp_listener
            .accept()
            .await
            .map_err(TransportError::IoError)?;

        stream.set_nodelay(true).map_err(TransportError::IoError)?;

        let tls_stream = self
            .tls_acceptor
            .accept(stream)
            .await
            .map_err(|e| TransportError::TlsError {
                reason: format!("TLS accept failed: {}", e),
            })?;

        let local_addr = "0.0.0.0:0".to_string();

        let peer_addr = remote_addr.to_string();

        tracing::debug!(peer = %peer_addr, "TLS-TCP accepted");

        let conn = TlsTcpConnection::new(tls_stream, peer_addr, local_addr);
        Ok(Box::new(conn))
    }

    fn local_addr(&self) -> Result<String> {
        self.tcp_listener
            .local_addr()
            .map(|addr| addr.to_string())
            .map_err(TransportError::IoError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Opcode;
    use crate::tls::{generate_node_cert, generate_self_signed_ca};

    fn create_mtls_config() -> (TlsConfig, TlsConfig) {
        let (ca_cert_pem, ca_key_pem) = generate_self_signed_ca().unwrap();

        let (server_cert_pem, server_key_pem) =
            generate_node_cert(&ca_cert_pem, &ca_key_pem, "localhost").unwrap();
        let server_tls = TlsConfig::new(
            ca_cert_pem.clone(),
            server_cert_pem,
            server_key_pem,
            true,
        );

        let (client_cert_pem, client_key_pem) =
            generate_node_cert(&ca_cert_pem, &ca_key_pem, "client").unwrap();
        let client_tls = TlsConfig::new(
            ca_cert_pem,
            client_cert_pem,
            client_key_pem,
            true,
        );

        (server_tls, client_tls)
    }

    #[tokio::test]
    async fn test_tls_tcp_roundtrip() {
        let (server_tls, client_tls) = create_mtls_config();

        let transport = TlsTcpTransport::new(
            TcpTransportConfig::default(),
            server_tls,
        )
        .unwrap();

        let listener = transport.listen("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let addr = addr.replace("127.0.0.1", "localhost");

        let server = tokio::spawn(async move {
            let conn = listener.accept().await.unwrap();
            let frame = conn.recv_frame().await.unwrap();
            assert_eq!(frame.opcode(), Opcode::Lookup);
            let response = frame.make_response(b"response".to_vec());
            conn.send_frame(&response).await.unwrap();
        });

        let client_transport =
            TlsTcpTransport::new(TcpTransportConfig::default(), client_tls).unwrap();
        let conn = client_transport.connect(&addr).await.unwrap();

        let frame = Frame::new(Opcode::Lookup, 1, b"request".to_vec());
        conn.send_frame(&frame).await.unwrap();
        let response = conn.recv_frame().await.unwrap();
        assert!(response.is_response());
        assert_eq!(response.payload, b"response");

        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_tls_transport_trait() {
        let (server_tls, client_tls) = create_mtls_config();

        let transport =
            TlsTcpTransport::new(TcpTransportConfig::default(), server_tls).unwrap();

        let listener = transport.listen("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let addr = addr.replace("127.0.0.1", "localhost");

        let server = tokio::spawn(async move {
            let conn = listener.accept().await.unwrap();
            conn.send_frame(&Frame::new(Opcode::Heartbeat, 1, vec![]))
                .await
                .unwrap();
        });

        let client_transport =
            TlsTcpTransport::new(TcpTransportConfig::default(), client_tls).unwrap();
        let conn = client_transport.connect(&addr).await.unwrap();

        assert!(!conn.peer_addr().is_empty());
        assert!(!conn.local_addr().is_empty());

        let _frame = conn.recv_frame().await.unwrap();

        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_tls_reject_no_client_cert() {
        let (server_tls, _) = create_mtls_config();

        let transport = TlsTcpTransport::new(
            TcpTransportConfig::default(),
            server_tls,
        )
        .unwrap();

        let listener = transport.listen("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server_handle = tokio::spawn(async move {
            let listener = TlsTcpTransport::new(
                TcpTransportConfig::default(),
                TlsConfig::new(vec![], vec![], vec![], true),
            )
            .unwrap();
            let listener = listener.listen("127.0.0.1:0").await.unwrap();
            let result = listener.accept().await;
            assert!(result.is_err());
        });

        let no_client_cert_tls = TlsConfig::new(vec![], vec![], vec![], false);
        let client_transport = TlsTcpTransport::new(
            TcpTransportConfig::default(),
            no_client_cert_tls,
        )
        .unwrap();

        let result = client_transport.connect(&addr).await;
        assert!(result.is_err());

        let _ = server_handle.await;
    }

    #[test]
    fn test_tls_tcp_transport_debug() {
        let (server_tls, _) = create_mtls_config();
        let transport =
            TlsTcpTransport::new(TcpTransportConfig::default(), server_tls).unwrap();
        let debug_str = format!("{:?}", transport);
        assert!(debug_str.contains("TlsTcpTransport"));
    }
}