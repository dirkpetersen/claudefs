//! RPC layer providing request/response semantics over TCP connections.

use crate::error::{TransportError, Result};
use crate::protocol::{Frame, FrameFlags, Opcode};
use crate::tcp::{TcpTransport, TcpConnection};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};
use tracing::{debug, warn};

/// Configuration for RPC client.
#[derive(Debug, Clone)]
pub struct RpcClientConfig {
    /// Response timeout in milliseconds (default: 5000).
    pub response_timeout_ms: u64,
}

impl Default for RpcClientConfig {
    fn default() -> Self { Self { response_timeout_ms: 5000 } }
}

/// Trait for handling incoming RPC requests.
pub trait RpcHandler: Send + Sync + 'static {
    /// Handle a request and return the response payload bytes.
    fn handle(&self, request: Frame) -> Pin<Box<dyn Future<Output = Result<Vec<u8>>> + Send + '_>>;
}

/// RPC client for sending requests and receiving responses.
pub struct RpcClient {
    conn: Arc<TcpConnection>,
    config: RpcClientConfig,
    next_id: AtomicU64,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Frame>>>>,
    _reader_handle: tokio::task::JoinHandle<()>,
}

impl RpcClient {
    /// Create a new RPC client. Starts a background reader task.
    pub fn new(conn: Arc<TcpConnection>, config: RpcClientConfig) -> Self {
        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Frame>>>> = Arc::new(Mutex::new(HashMap::new()));
        let reader_conn = conn.clone();
        let reader_pending = pending.clone();
        let handle = tokio::spawn(async move {
            loop {
                match reader_conn.recv_frame().await {
                    Ok(frame) => {
                        let request_id = frame.request_id();
                        let mut map = reader_pending.lock().await;
                        if let Some(tx) = map.remove(&request_id) {
                            let _ = tx.send(frame);
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "RPC reader error, stopping");
                        break;
                    }
                }
            }
        });
        Self { conn, config, next_id: AtomicU64::new(1), pending, _reader_handle: handle }
    }

    /// Send a request and wait for the response.
    pub async fn call(&self, opcode: Opcode, payload: Vec<u8>) -> Result<Frame> {
        let request_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let frame = Frame::new(opcode, request_id, payload);
        let (tx, rx) = oneshot::channel();
        {
            let mut map = self.pending.lock().await;
            map.insert(request_id, tx);
        }
        self.conn.send_frame(&frame).await?;
        let timeout = std::time::Duration::from_millis(self.config.response_timeout_ms);
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err(TransportError::ConnectionReset),
            Err(_) => {
                let mut map = self.pending.lock().await;
                map.remove(&request_id);
                Err(TransportError::RequestTimeout {
                    request_id,
                    timeout_ms: self.config.response_timeout_ms,
                })
            }
        }
    }

    /// Send a fire-and-forget message (no response expected).
    pub async fn call_one_way(&self, opcode: Opcode, payload: Vec<u8>) -> Result<()> {
        let request_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut frame = Frame::new(opcode, request_id, payload);
        frame.header.flags = FrameFlags::ONE_WAY;
        // Recompute checksum since we didn't change payload
        self.conn.send_frame(&frame).await
    }

    /// Shutdown the RPC client.
    pub async fn shutdown(self) {
        self._reader_handle.abort();
    }
}

/// RPC server that dispatches requests to a handler.
pub struct RpcServer;

impl RpcServer {
    /// Run the server accept loop. Spawns a task per connection.
    pub async fn serve(
        _transport: &TcpTransport,
        listener: tokio::net::TcpListener,
        handler: Arc<dyn RpcHandler>,
    ) -> Result<()> {
        loop {
            let (stream, peer_addr) = listener.accept().await.map_err(TransportError::IoError)?;
            debug!(peer = %peer_addr, "Accepted connection");
            let handler = handler.clone();
            tokio::spawn(async move {
                let conn = match crate::tcp::TcpConnection::from_stream(stream) {
                    Ok(c) => c,
                    Err(e) => { warn!(error = %e, "Failed to create connection"); return; }
                };
                loop {
                    let frame = match conn.recv_frame().await {
                        Ok(f) => f,
                        Err(e) => { debug!(error = %e, "Connection closed"); break; }
                    };
                    let opcode = frame.header.opcode;
                    let request_id = frame.header.request_id;
                    let is_one_way = frame.header.flags.contains(FrameFlags::ONE_WAY);
                    match handler.handle(frame).await {
                        Ok(response_payload) => {
                            if !is_one_way {
                                // Build response manually using saved opcode and request_id
                                let response = Frame::new(
                                    Opcode::from(opcode),
                                    request_id,
                                    response_payload,
                                );
                                // Set RESPONSE flag
                                let mut resp = response;
                                resp.header.flags = FrameFlags::RESPONSE;
                                if let Err(e) = conn.send_frame(&resp).await {
                                    warn!(error = %e, "Failed to send response");
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            warn!(error = %e, "Handler error");
                        }
                    }
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tcp::TcpTransportConfig;

    struct EchoHandler;

    impl RpcHandler for EchoHandler {
        fn handle(&self, request: Frame) -> Pin<Box<dyn Future<Output = Result<Vec<u8>>> + Send + '_>> {
            Box::pin(async move { Ok(request.payload.clone()) })
        }
    }

    #[tokio::test]
    async fn test_rpc_roundtrip() {
        let transport = TcpTransport::new(TcpTransportConfig::default());
        let listener = transport.listen("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handler: Arc<dyn RpcHandler> = Arc::new(EchoHandler);

        tokio::spawn(async move {
            let _ = RpcServer::serve(&transport, listener, handler).await;
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let client_transport = TcpTransport::new(TcpTransportConfig::default());
        let conn = client_transport.connect(&addr).await.unwrap();
        let client = RpcClient::new(Arc::new(conn), RpcClientConfig::default());

        let response = client.call(Opcode::Heartbeat, b"hello".to_vec()).await.unwrap();
        assert_eq!(response.payload, b"hello");

        client.shutdown().await;
    }
}
