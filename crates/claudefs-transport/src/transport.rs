//! Abstract transport layer for ClaudeFS network communication.
//!
//! This module defines the Transport, Connection, and Listener traits that
//! provide a unified interface for different transport implementations (TCP, RDMA).

use async_trait::async_trait;

use crate::error::{Result, TransportError};
use crate::protocol::Frame;

/// Abstract transport layer for sending and receiving frames.
///
/// This trait defines the interface that all transport implementations must support.
/// The TCP and RDMA backends both implement this trait.
#[async_trait]
pub trait Transport: Send + Sync + 'static {
    /// Connect to a remote peer at the given address.
    ///
    /// # Arguments
    /// * `addr` - The address to connect to (e.g., "192.168.1.1:9000")
    ///
    /// # Returns
    /// A connection to the remote peer.
    async fn connect(&self, addr: &str) -> Result<Box<dyn Connection>>;

    /// Listen for incoming connections on the given address.
    ///
    /// # Arguments
    /// * `addr` - The address to listen on (e.g., "0.0.0.0:9000")
    ///
    /// # Returns
    /// A listener that can accept incoming connections.
    async fn listen(&self, addr: &str) -> Result<Box<dyn Listener>>;
}

/// An established connection for sending and receiving frames.
///
/// This trait defines the interface for active connections to remote peers.
#[async_trait]
pub trait Connection: Send + Sync {
    /// Send a frame over the connection.
    ///
    /// # Arguments
    /// * `frame` - The frame to send.
    async fn send_frame(&self, frame: &Frame) -> Result<()>;

    /// Receive a frame from the connection.
    ///
    /// # Returns
    /// The received frame.
    async fn recv_frame(&self) -> Result<Frame>;

    /// Get the remote peer address.
    ///
    /// # Returns
    /// The address of the remote peer as a string.
    fn peer_addr(&self) -> &str;

    /// Get the local address.
    ///
    /// # Returns
    /// The local address as a string.
    fn local_addr(&self) -> &str;
}

/// A listener that accepts incoming connections.
///
/// This trait defines the interface for servers that accept incoming connections.
#[async_trait]
pub trait Listener: Send + Sync {
    /// Accept a new incoming connection.
    ///
    /// # Returns
    /// A new connection to the accepted client.
    async fn accept(&self) -> Result<Box<dyn Connection>>;

    /// Get the local address this listener is bound to.
    ///
    /// # Returns
    /// The local address as a string.
    fn local_addr(&self) -> Result<String>;
}

// ============================================================================
// TCP Transport Implementation
// ============================================================================

use crate::tcp::{TcpTransport as TcpTransportImpl, TcpTransportConfig};

/// TCP transport implementation using tokio.
#[derive(Debug, Clone)]
pub struct TcpTransport {
    inner: TcpTransportImpl,
}

impl TcpTransport {
    /// Creates a new TCP transport with default configuration.
    pub fn new() -> Self {
        Self {
            inner: TcpTransportImpl::new(TcpTransportConfig::default()),
        }
    }

    /// Creates a new TCP transport with the given configuration.
    pub fn with_config(config: TcpTransportConfig) -> Self {
        Self {
            inner: TcpTransportImpl::new(config),
        }
    }
}

impl Default for TcpTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Transport for TcpTransport {
    async fn connect(&self, addr: &str) -> Result<Box<dyn Connection>> {
        let conn = self.inner.connect(addr).await?;
        Ok(Box::new(TcpConnection::new(conn)))
    }

    async fn listen(&self, addr: &str) -> Result<Box<dyn Listener>> {
        let listener = self.inner.listen(addr).await?;
        Ok(Box::new(TcpListener::new(listener)))
    }
}

/// Wrapper around the internal TCP connection to implement the Connection trait.
pub struct TcpConnection {
    inner: crate::tcp::TcpConnection,
    peer_addr: String,
    local_addr: String,
}

impl std::fmt::Debug for TcpConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TcpConnection")
            .field("peer_addr", &self.peer_addr)
            .field("local_addr", &self.local_addr)
            .finish()
    }
}

impl TcpConnection {
    fn new(inner: crate::tcp::TcpConnection) -> Self {
        let peer_addr = inner.peer_addr().to_string();
        let local_addr = inner.local_addr().to_string();
        Self {
            inner,
            peer_addr,
            local_addr,
        }
    }
}

#[async_trait]
impl Connection for TcpConnection {
    async fn send_frame(&self, frame: &Frame) -> Result<()> {
        self.inner.send_frame(frame).await
    }

    async fn recv_frame(&self) -> Result<Frame> {
        self.inner.recv_frame().await
    }

    fn peer_addr(&self) -> &str {
        &self.peer_addr
    }

    fn local_addr(&self) -> &str {
        &self.local_addr
    }
}

/// Wrapper around tokio's TcpListener to implement the Listener trait.
#[derive(Debug)]
pub struct TcpListener {
    inner: tokio::net::TcpListener,
}

impl TcpListener {
    fn new(inner: tokio::net::TcpListener) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl Listener for TcpListener {
    async fn accept(&self) -> Result<Box<dyn Connection>> {
        let (stream, _) = self
            .inner
            .accept()
            .await
            .map_err(TransportError::IoError)?;
        stream
            .set_nodelay(true)
            .map_err(TransportError::IoError)?;
        let conn = crate::tcp::TcpConnection::from_stream(stream)?;
        Ok(Box::new(TcpConnection::new(conn)))
    }

    fn local_addr(&self) -> Result<String> {
        self.inner
            .local_addr()
            .map(|addr| addr.to_string())
            .map_err(TransportError::IoError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Opcode;

    #[tokio::test]
    async fn test_transport_connect_and_listen() {
        let transport = TcpTransport::new();
        
        // Start listening
        let listener = transport.listen("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        
        // Server task: accept connection and echo
        let server = tokio::spawn(async move {
            let conn = listener.accept().await.unwrap();
            let frame = conn.recv_frame().await.unwrap();
            assert_eq!(frame.opcode(), Opcode::Lookup);
            let response = frame.make_response(b"response".to_vec());
            conn.send_frame(&response).await.unwrap();
        });
        
        // Client: connect and send
        let conn = transport.connect(&addr).await.unwrap();
        assert!(!conn.peer_addr().is_empty());
        
        let frame = Frame::new(Opcode::Lookup, 1, b"request".to_vec());
        conn.send_frame(&frame).await.unwrap();
        let response = conn.recv_frame().await.unwrap();
        assert!(response.is_response());
        assert_eq!(response.payload, b"response");
        
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_transport_connection_addresses() {
        let transport = TcpTransport::new();
        let listener = transport.listen("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        
        let server = tokio::spawn(async move {
            let conn = listener.accept().await.unwrap();
            // Server connection should have both addresses
            assert!(!conn.peer_addr().is_empty());
            assert!(!conn.local_addr().is_empty());
            conn.send_frame(&Frame::new(Opcode::Heartbeat, 1, vec![])).await.unwrap();
        });
        
        let conn = transport.connect(&addr).await.unwrap();
        assert!(!conn.peer_addr().is_empty());
        assert!(!conn.local_addr().is_empty());
        
        let _response = conn.recv_frame().await.unwrap();
        
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_multiple_connections() {
        let transport = TcpTransport::new();
        let listener = transport.listen("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        
        let server = tokio::spawn(async move {
            // Accept first connection
            let conn = listener.accept().await.unwrap();
            let frame = conn.recv_frame().await.unwrap();
            let response = frame.make_response(b"ok".to_vec());
            conn.send_frame(&response).await.unwrap();
        });
        
        // Client connects and communicates
        let conn = transport.connect(&addr).await.unwrap();
        let frame = Frame::new(Opcode::Heartbeat, 0, vec![]);
        conn.send_frame(&frame).await.unwrap();
        let response = conn.recv_frame().await.unwrap();
        assert!(response.is_response());
        
        server.await.unwrap();
    }

    #[test]
    fn test_tcp_transport_debug() {
        let transport = TcpTransport::new();
        let debug_str = format!("{:?}", transport);
        assert!(debug_str.contains("TcpTransport"));
    }

    #[tokio::test]
    async fn test_listener_local_addr() {
        let transport = TcpTransport::new();
        let listener = transport.listen("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        assert!(addr.contains(':'));
        assert!(addr.starts_with("127.0.0.1:"));
    }
}