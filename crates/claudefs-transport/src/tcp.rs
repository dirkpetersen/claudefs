//! TCP transport implementation

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::Mutex;

use crate::error::{TransportError, Result};
use crate::protocol::{Frame, FrameHeader, FRAME_HEADER_SIZE, MAX_PAYLOAD_SIZE};

/// TCP transport configuration
#[derive(Debug, Clone)]
pub struct TcpTransportConfig {
    /// Connection timeout in milliseconds.
    pub connect_timeout_ms: u64,
    /// Whether to enable TCP_NODELAY (disable Nagle's algorithm).
    pub nodelay: bool,
}

impl Default for TcpTransportConfig {
    fn default() -> Self {
        Self {
            connect_timeout_ms: 5000,
            nodelay: true,
        }
    }
}

/// TCP transport — creates connections
#[derive(Debug, Clone)]
pub struct TcpTransport {
    config: TcpTransportConfig,
}

impl TcpTransport {
    /// Creates a new TCP transport with the given configuration.
    pub fn new(config: TcpTransportConfig) -> Self {
        Self { config }
    }

    /// Establishes a TCP connection to the specified address.
    pub async fn connect(&self, addr: &str) -> Result<TcpConnection> {
        let timeout = std::time::Duration::from_millis(self.config.connect_timeout_ms);
        let stream = tokio::time::timeout(timeout, tokio::net::TcpStream::connect(addr))
            .await
            .map_err(|_| TransportError::ConnectionTimeout {
                addr: addr.to_string(),
                timeout_ms: self.config.connect_timeout_ms,
            })?
            .map_err(TransportError::IoError)?;
        if self.config.nodelay {
            stream.set_nodelay(true).map_err(TransportError::IoError)?;
        }
        tracing::debug!(addr = addr, "TCP connected");
        TcpConnection::from_stream(stream)
    }

    /// Binds to the specified address and returns a listener for incoming connections.
    pub async fn listen(&self, addr: &str) -> Result<tokio::net::TcpListener> {
        tokio::net::TcpListener::bind(addr)
            .await
            .map_err(TransportError::IoError)
    }

    /// Accepts an incoming TCP connection from the listener.
    pub async fn accept(&self, listener: &tokio::net::TcpListener) -> Result<TcpConnection> {
        let (stream, _) = listener.accept().await.map_err(TransportError::IoError)?;
        if self.config.nodelay {
            stream.set_nodelay(true).map_err(TransportError::IoError)?;
        }
        TcpConnection::from_stream(stream)
    }
}

/// A single TCP connection with concurrent read/write support
pub struct TcpConnection {
    read: Mutex<OwnedReadHalf>,
    write: Mutex<OwnedWriteHalf>,
    peer_addr: String,
    local_addr: String,
}

impl TcpConnection {
    pub(crate) fn from_stream(stream: tokio::net::TcpStream) -> Result<Self> {
        let peer_addr = stream
            .peer_addr()
            .map(|a| a.to_string())
            .unwrap_or_default();
        let local_addr = stream
            .local_addr()
            .map(|a| a.to_string())
            .unwrap_or_default();
        let (read, write) = stream.into_split();
        Ok(Self {
            read: Mutex::new(read),
            write: Mutex::new(write),
            peer_addr,
            local_addr,
        })
    }

    /// Sends a frame over the TCP connection.
    pub async fn send_frame(&self, frame: &Frame) -> Result<()> {
        let encoded = frame.encode();
        let mut write = self.write.lock().await;
        write.write_all(&encoded).await.map_err(TransportError::IoError)?;
        write.flush().await.map_err(TransportError::IoError)?;
        Ok(())
    }

    /// Receives a frame from the TCP connection.
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

    /// Returns the remote peer address of this TCP connection as a string.
    pub fn peer_addr(&self) -> &str {
        &self.peer_addr
    }

    /// Returns the local address of this TCP connection as a string.
    pub fn local_addr(&self) -> &str {
        &self.local_addr
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Opcode;

    #[tokio::test]
    async fn test_send_recv_frame() {
        let transport = TcpTransport::new(TcpTransportConfig::default());
        let listener = transport.listen("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();

        let server = tokio::spawn(async move {
            let conn = transport.accept(&listener).await.unwrap();
            let frame = conn.recv_frame().await.unwrap();
            assert_eq!(frame.opcode(), Opcode::Heartbeat);
            let response = frame.make_response(b"pong".to_vec());
            conn.send_frame(&response).await.unwrap();
        });

        let client_transport = TcpTransport::new(TcpTransportConfig::default());
        let conn = client_transport.connect(&addr).await.unwrap();
        let frame = Frame::new(Opcode::Heartbeat, 1, b"ping".to_vec());
        conn.send_frame(&frame).await.unwrap();
        let response = conn.recv_frame().await.unwrap();
        assert!(response.is_response());
        assert_eq!(response.payload, b"pong");

        server.await.unwrap();
    }
}
