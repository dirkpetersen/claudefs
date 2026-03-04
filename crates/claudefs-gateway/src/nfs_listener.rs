//! Async tokio TCP listener for NFSv3 RPC calls (RFC 5531 record marking)

use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tracing::{debug, error, info, warn};

use crate::nfs::VfsBackend;
use crate::server::RpcDispatcher;

/// Maximum allowed RPC record size (4 MB). Larger frames are rejected.
const MAX_RPC_RECORD: usize = 4 * 1024 * 1024;

/// Listener handle — call `shutdown()` to stop the listener.
pub struct NfsShutdown {
    sender: watch::Sender<bool>,
}

impl NfsShutdown {
    /// Signal the listener to stop accepting new connections.
    pub fn shutdown(&self) {
        let _ = self.sender.send(true);
    }
}

/// Async TCP listener that accepts NFS RPC connections.
pub struct NfsListener {
    /// Bind address (e.g., "0.0.0.0:2049")
    pub bind_addr: String,
    /// Shutdown signal receiver
    shutdown: watch::Receiver<bool>,
}

impl NfsListener {
    /// Create a new `NfsListener` bound to `addr`.
    ///
    /// Returns the listener and a `NfsShutdown` handle.
    pub fn new(addr: &str) -> (Self, NfsShutdown) {
        let (tx, rx) = watch::channel(false);
        let listener = NfsListener {
            bind_addr: addr.to_string(),
            shutdown: rx,
        };
        let shutdown = NfsShutdown { sender: tx };
        (listener, shutdown)
    }

    /// Start listening and dispatching NFS RPC calls.
    ///
    /// Each accepted connection is handled in a separate `tokio::spawn`'d task.
    /// Returns a `JoinHandle` that completes when the listener shuts down.
    pub fn run<B>(self, dispatcher: Arc<RpcDispatcher<B>>) -> tokio::task::JoinHandle<()>
    where
        B: VfsBackend + Send + Sync + 'static,
    {
        tokio::spawn(async move {
            let listener = match TcpListener::bind(&self.bind_addr).await {
                Ok(l) => {
                    info!(addr = %self.bind_addr, "NFS TCP listener started");
                    l
                }
                Err(e) => {
                    error!(addr = %self.bind_addr, error = %e, "Failed to bind NFS listener");
                    return;
                }
            };

            let mut shutdown = self.shutdown;

            loop {
                tokio::select! {
                    accept = listener.accept() => {
                        match accept {
                            Ok((stream, peer)) => {
                                debug!(peer = %peer, "NFS connection accepted");
                                let dispatcher = dispatcher.clone();
                                let shutdown_rx = shutdown.clone();
                                tokio::spawn(async move {
                                    handle_connection(stream, dispatcher, shutdown_rx).await;
                                });
                            }
                            Err(e) => {
                                warn!(error = %e, "NFS accept error");
                            }
                        }
                    }
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() {
                            info!("NFS listener shutting down");
                            break;
                        }
                    }
                }
            }
        })
    }
}

/// Handle a single NFS/TCP connection using RFC 5531 record marking.
async fn handle_connection<B>(
    mut stream: TcpStream,
    dispatcher: Arc<RpcDispatcher<B>>,
    mut shutdown: watch::Receiver<bool>,
) where
    B: VfsBackend + Send + Sync + 'static,
{
    let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or_default();

    loop {
        let mut mark_buf = [0u8; 4];
        let read_mark = tokio::select! {
            r = stream.read_exact(&mut mark_buf) => r,
            _ = shutdown.changed() => break,
        };

        match read_mark {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                debug!(peer = %peer, "NFS connection closed by client");
                break;
            }
            Err(e) => {
                warn!(peer = %peer, error = %e, "Error reading RPC record mark");
                break;
            }
        }

        let mark = u32::from_be_bytes(mark_buf);
        let _last_fragment = (mark & 0x8000_0000) != 0;
        let fragment_len = (mark & 0x7FFF_FFFF) as usize;

        if fragment_len > MAX_RPC_RECORD {
            warn!(peer = %peer, len = fragment_len, "RPC record too large, closing connection");
            break;
        }

        let mut body = vec![0u8; fragment_len];
        if let Err(e) = stream.read_exact(&mut body).await {
            warn!(peer = %peer, error = %e, "Error reading RPC body");
            break;
        }

        let response = dispatcher.dispatch(&body);

        let resp_len = response.len() as u32;
        let resp_mark: u32 = 0x8000_0000 | resp_len;
        let mut out = Vec::with_capacity(4 + response.len());
        out.extend_from_slice(&resp_mark.to_be_bytes());
        out.extend_from_slice(&response);

        if let Err(e) = stream.write_all(&out).await {
            warn!(peer = %peer, error = %e, "Error writing RPC response");
            break;
        }
    }

    debug!(peer = %peer, "NFS connection handler finished");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nfs_listener_new() {
        let (listener, shutdown) = NfsListener::new("127.0.0.1:0");
        assert_eq!(listener.bind_addr, "127.0.0.1:0");
        shutdown.shutdown();
    }

    #[test]
    fn test_nfs_shutdown_signal() {
        let (listener, shutdown) = NfsListener::new("0.0.0.0:2049");
        assert!(!*listener.shutdown.borrow());
        shutdown.shutdown();
        assert_eq!(listener.bind_addr, "0.0.0.0:2049");
    }

    #[test]
    fn test_max_rpc_record_constant() {
        assert_eq!(MAX_RPC_RECORD, 4 * 1024 * 1024);
    }

    #[test]
    fn test_record_mark_parsing() {
        let mark: u32 = 0x8000_0005;
        let last_fragment = (mark & 0x8000_0000) != 0;
        let fragment_len = (mark & 0x7FFF_FFFF) as usize;
        assert!(last_fragment);
        assert_eq!(fragment_len, 5);

        let mark2: u32 = 0x0000_0100;
        let last_fragment2 = (mark2 & 0x8000_0000) != 0;
        let fragment_len2 = (mark2 & 0x7FFF_FFFF) as usize;
        assert!(!last_fragment2);
        assert_eq!(fragment_len2, 256);
    }
}