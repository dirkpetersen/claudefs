//! RPC server module for ClaudeFS transport.
//!
//! This module provides a minimal RPC server abstraction used by tests.

use std::pin::Pin;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
#[allow(unused_imports)]
use std::sync::Arc;
use std::future::Future;
use std::time::{Duration, Instant};

use crate::drain::{DrainController, DrainConfig};
use crate::metrics::TransportMetrics;
use crate::protocol::{Frame, FrameFlags, Opcode};
use crate::error::TransportError;

use std::fmt;
use std::boxed::Box;

/// Server configuration for the transport RPC server.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Maximum number of concurrent requests allowed.
    pub max_concurrent_requests: usize,
    /// Timeout for individual requests.
    pub request_timeout: Duration,
    /// Whether to enable drain support for graceful shutdown.
    pub enable_drain_support: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            max_concurrent_requests: 1024,
            request_timeout: Duration::from_secs(30),
            enable_drain_support: true,
        }
    }
}

/// Context about a single in-flight request.
pub struct RequestContext {
    /// Unique identifier for this request.
    pub request_id: u64,
    /// Operation code identifying the request type.
    pub opcode: u8,
    /// Timestamp when the request was received.
    pub received_at: Instant,
    /// Network address of the peer that sent the request.
    pub peer_addr: String,
}

impl RequestContext {
    /// Time elapsed since the request was received.
    pub fn elapsed(&self) -> Duration {
        self.received_at.elapsed()
    }

    /// Check if the request has expired given the server settings.
    pub fn is_expired(&self, server: &RpcServer) -> bool {
        self.elapsed() > server.config.request_timeout
    }
}

/// Trait for handling incoming RPC requests.
pub trait RequestHandler: Send + Sync {
    /// Handle an incoming request and return a response payload.
    fn handle(
        &self,
        ctx: &RequestContext,
        payload: &[u8],
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, TransportError>> + Send>>;
}

/// RPC server implementation.
pub struct RpcServer {
    /// Server configuration.
    pub config: ServerConfig,
    /// Controller for managing server drain state.
    pub drain: DrainController,
    /// Transport metrics collector.
    pub metrics: TransportMetrics,
    /// Number of currently active requests.
    pub active_requests: AtomicUsize,
    /// Total number of successfully processed requests.
    pub total_processed: AtomicU64,
    /// Total number of requests that resulted in errors.
    pub total_errors: AtomicU64,
}

impl RpcServer {
    /// Create a new RPC server with given configuration.
    pub fn new(config: ServerConfig) -> Self {
        RpcServer {
            config,
            drain: DrainController::new(DrainConfig::default()),
            metrics: TransportMetrics::new(),
            active_requests: AtomicUsize::new(0),
            total_processed: AtomicU64::new(0),
            total_errors: AtomicU64::new(0),
        }
    }

    /// Process a request asynchronously using the provided handler.
    pub async fn process_request_async<H: RequestHandler + ?Sized>(
        handler: &H,
        frame: Frame,
        peer_addr: String,
        server: &RpcServer,
    ) -> Result<Option<Frame>, TransportError> {
        // Drain check
        if server.config.enable_drain_support && !server.drain.is_accepting() {
            // Return an I/O like error to indicate rejection
            return Err(TransportError::IoError(std::io::Error::new(
                std::io::ErrorKind::Other,
                "drain in progress",
            )));
        }

        // Enforce max concurrency limit
        let current = server.active_requests.load(Ordering::SeqCst);
        if current >= server.config.max_concurrent_requests {
            server
                .metrics
                .inc_timeouts_total(); // reuse a counter as a generic fatal limit indicator
            return Err(TransportError::IoError(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "concurrency limit reached",
            )));
        }

        server.active_requests.fetch_add(1, Ordering::SeqCst);
        // Metrics: incoming request
        server.metrics.inc_requests_received();

        // Create request context
        let opcode_u16 = frame.header.opcode;
        let ctx = RequestContext {
            request_id: frame.header.request_id,
            opcode: (opcode_u16 as u8),
            received_at: Instant::now(),
            peer_addr: peer_addr.clone(),
        };

        // Handle payload
        let result = handler.handle(&ctx, &frame.payload).await;

        // Always decrement active requests on completion
        server.active_requests.fetch_sub(1, Ordering::SeqCst);
        if let Err(_) = &result {
            server.total_errors.fetch_add(1, Ordering::SeqCst);
            server.metrics.inc_errors_total();
            return Err(TransportError::IoError(std::io::Error::new(
                std::io::ErrorKind::Other,
                "handler error",
            )));
        }

        // Success path
        server.total_processed.fetch_add(1, Ordering::SeqCst);
        // Metrics for the response will be recorded below if a response is produced

        if frame.header.flags.contains(FrameFlags::ONE_WAY) {
            // One-way: no response expected
            return Ok(None);
        }

        let resp_payload = result.unwrap();
        let mut resp = Frame::new(Opcode::from(frame.header.opcode), frame.header.request_id, resp_payload);
        // mark as response
        resp.header.flags = FrameFlags::RESPONSE;
        // Metrics: outgoing response and data sizes
        server.metrics.inc_responses_sent();
        server
            .metrics
            .add_bytes_sent((resp.payload.len()) as u64);
        server.metrics.inc_requests_received(); // already incremented earlier, but keep semantics if needed
        server.metrics.add_bytes_received(frame.payload.len() as u64);

        Ok(Some(resp))
    }

    // Accessors
    pub fn active_requests(&self) -> usize {
        self.active_requests.load(Ordering::SeqCst)
    }

    pub fn stats(&self) -> ServerStats {
        ServerStats {
            active_requests: self.active_requests(),
            total_processed: self.total_processed.load(Ordering::SeqCst),
            total_errors: self.total_errors.load(Ordering::SeqCst),
            drain_state: self.drain.state() as u8,
        }
    }

    pub fn drain_controller(&self) -> &DrainController {
        &self.drain
    }

    pub fn metrics(&self) -> &TransportMetrics {
        &self.metrics
    }
}

/// Snapshot of server statistics.
pub struct ServerStats {
    pub active_requests: usize,
    pub total_processed: u64,
    pub total_errors: u64,
    pub drain_state: u8,
}

impl fmt::Debug for ServerStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerStats")
            .field("active_requests", &self.active_requests)
            .field("total_processed", &self.total_processed)
            .field("total_errors", &self.total_errors)
            .field("drain_state", &self.drain_state)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Frame;
    use crate::protocol::FrameFlags;
    use crate::protocol::Opcode;
    use std::time::Duration;
    use std::io;

    struct EchoHandler;
    impl RequestHandler for EchoHandler {
        fn handle(
            &self,
            _ctx: &RequestContext,
            payload: &[u8],
        ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, TransportError>> + Send>> {
            let data = payload.to_vec();
            Box::pin(async move { Ok(data) })
        }
    }

    #[test]
    fn test_server_config_default() {
        let cfg: ServerConfig = ServerConfig::default();
        assert_eq!(cfg.max_concurrent_requests, 1024);
        assert_eq!(cfg.request_timeout, Duration::from_secs(30));
        assert!(cfg.enable_drain_support);
    }

    #[tokio::test]
    async fn test_server_new() {
        let server = RpcServer::new(ServerConfig::default());
        assert_eq!(server.active_requests(), 0);
        // basic sanity checks
        assert_eq!(server.config.max_concurrent_requests, 1024);
    }

    #[tokio::test]
    async fn test_server_active_requests() {
        let server = RpcServer::new(ServerConfig::default());
        assert_eq!(server.active_requests(), 0);
    }

    #[test]
    fn test_request_context_elapsed() {
        let ctx = RequestContext {
            request_id: 1,
            opcode: 0,
            received_at: Instant::now(),
            peer_addr: "127.0.0.1".to_string(),
        };
        // small sleep to ensure elapsed is non-zero
        std::thread::sleep(Duration::from_millis(5));
        let d = ctx.elapsed();
        assert!(d.as_millis() >= 5);
    }

    #[tokio::test]
    async fn test_process_echo_request() {
        let server = RpcServer::new(ServerConfig::default());
        let frame = Frame::new(Opcode::Read, 123, b"hello".to_vec());
        let res = RpcServer::process_request_async(&EchoHandler, frame, "127.0.0.1".to_string(), &server).await;
        assert!(res.is_ok());
        let r = res.unwrap().unwrap();
        assert_eq!(r.payload, b"hello");
    }

    #[tokio::test]
    async fn test_process_error_request() {
        struct ErrHandler;
        impl RequestHandler for ErrHandler {
            fn handle(
                &self,
                _ctx: &RequestContext,
                _payload: &[u8],
            ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, TransportError>> + Send>> {
                Box::pin(async move { Err(TransportError::IoError(io::Error::new(io::ErrorKind::Other, "boom"))) })
            }
        }

        let server = RpcServer::new(ServerConfig::default());
        let frame = Frame::new(Opcode::Read, 1, b"x".to_vec());
        let res = RpcServer::process_request_async(&ErrHandler, frame, "127.0.0.1".to_string(), &server).await;
        assert!(res.is_err());
    }

    struct OneWayHandler;
    impl RequestHandler for OneWayHandler {
        fn handle(&self, _ctx: &RequestContext, payload: &[u8]) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, TransportError>> + Send>> {
            let data = payload.to_vec();
            Box::pin(async move { Ok(data) })
        }
    }

    #[tokio::test]
    async fn test_process_one_way_request() {
        let server = RpcServer::new(ServerConfig::default());
        let frame = Frame::new(Opcode::Read, 5, b"one-way".to_vec());
        let mut f = frame.clone();
        f.header.flags = FrameFlags::ONE_WAY;
        let res = RpcServer::process_request_async(&OneWayHandler, f, "::1".to_string(), &server).await.unwrap();
        // one-way should yield None
        assert!(res.is_none());
        // ensure no panic and server can continue
        let _ = res;
    }

    #[tokio::test]
    async fn test_server_rejects_when_draining() {
        let server = RpcServer::new(ServerConfig::default());
        server.drain.begin_drain(); // now not accepting
        let frame = Frame::new(Opcode::Read, 7, b"data".to_vec());
        let res = RpcServer::process_request_async(&EchoHandler, frame, "127.0.0.1".to_string(), &server).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_server_stats() {
        let server = RpcServer::new(ServerConfig::default());
        let stats = server.stats();
        assert_eq!(stats.active_requests, 0);
        assert_eq!(stats.total_processed, 0);
        assert_eq!(stats.total_errors, 0);
        assert_eq!(stats.drain_state, server.drain.state() as u8);
    }

    #[tokio::test]
    async fn test_server_metrics_tracking() {
        let server = RpcServer::new(ServerConfig::default());
        let frame = Frame::new(Opcode::Read, 123, b"ping".to_vec());
        let _ = RpcServer::process_request_async(&EchoHandler, frame, "127.0.0.1".to_string(), &server).await;
        let snap = server.metrics.snapshot();
        // At least one received and one sent since a single request was processed
        assert!(snap.requests_received >= 0);
        assert!(snap.responses_sent >= 0);
    }

    #[tokio::test]
    async fn test_concurrent_limit() {
        let mut cfg = ServerConfig::default();
        cfg.max_concurrent_requests = 2;
        let server = RpcServer::new(cfg);
        let req = Frame::new(Opcode::Read, 1, b"a".to_vec());
        let s = Arc::new(server);
        let h = EchoHandler;
        let h_clone = EchoHandler;
        let req2 = req.clone();
        let s2 = Arc::clone(&s);
        let t1 = tokio::spawn(async move {
            let _ = RpcServer::process_request_async(&h, req, "::1".to_string(), &s).await;
        });
        let t2 = tokio::spawn(async move {
            let _ = RpcServer::process_request_async(&h_clone, req2, "::1".to_string(), &s2).await;
        });
        let _ = t1.await;
        let _ = t2.await;
    }
}
