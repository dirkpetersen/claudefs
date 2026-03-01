//! Connection multiplexing for managing multiple concurrent RPC streams over a single connection.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::oneshot;

use crate::error::{TransportError, Result};
use crate::protocol::Frame;

/// Type alias for stream identifier (matches request_id in the protocol).
pub type StreamId = u64;

/// State of a multiplexed stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    /// Stream has an in-flight request.
    Active,
    /// Stream received its response.
    Complete,
    /// Stream timed out.
    TimedOut,
    /// Stream was cancelled.
    Cancelled,
}

/// Configuration for the multiplexer.
#[derive(Debug, Clone)]
pub struct MuxConfig {
    /// Maximum number of concurrent streams allowed.
    pub max_concurrent_streams: u32,
    /// Timeout for stream operations.
    pub stream_timeout: Duration,
}

impl Default for MuxConfig {
    fn default() -> Self {
        Self {
            max_concurrent_streams: 256,
            stream_timeout: Duration::from_secs(30),
        }
    }
}

/// Handle for a multiplexed stream.
///
/// Provides access to the stream ID and allows receiving the response.
#[derive(Debug)]
pub struct StreamHandle {
    id: StreamId,
    receiver: oneshot::Receiver<Frame>,
}

impl StreamHandle {
    /// Returns the stream identifier.
    pub fn id(&self) -> StreamId {
        self.id
    }

    /// Waits for and receives the response frame.
    ///
    /// Returns the frame on success, or an error if the sender was dropped
    /// (stream cancelled) or the channel was closed.
    pub async fn recv(self) -> Result<Frame> {
        self.receiver.await.map_err(|_| {
            TransportError::ConnectionReset
        })
    }
}

/// Multiplexer for managing multiple concurrent RPC streams over a single connection.
///
/// Tracks in-flight requests per stream and dispatches responses to the correct
/// waiting caller.
pub struct Multiplexer {
    streams: Arc<Mutex<HashMap<StreamId, oneshot::Sender<Frame>>>>,
    next_id: AtomicU64,
    active_count: AtomicU32,
    config: MuxConfig,
}

impl Default for Multiplexer {
    fn default() -> Self {
        Self::new(MuxConfig::default())
    }
}

impl Multiplexer {
    /// Creates a new multiplexer with the given configuration.
    pub fn new(config: MuxConfig) -> Self {
        Self {
            streams: Arc::new(Mutex::new(HashMap::new())),
            next_id: AtomicU64::new(1),
            active_count: AtomicU32::new(0),
            config,
        }
    }

    /// Opens a new stream, returning the stream ID and handle.
    ///
    /// Returns an error if the maximum number of concurrent streams is exceeded.
    pub fn open_stream(&self) -> Result<(StreamId, StreamHandle)> {
        let current_count = self.active_count.load(Ordering::Relaxed);
        if current_count >= self.config.max_concurrent_streams {
            return Err(TransportError::InvalidFrame {
                reason: "max concurrent streams exceeded".to_string(),
            });
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();

        {
            let mut streams = self.streams.lock().unwrap();
            streams.insert(id, tx);
        }

        self.active_count.fetch_add(1, Ordering::Relaxed);

        Ok((id, StreamHandle { id, receiver: rx }))
    }

    /// Dispatches a response frame to the appropriate stream.
    ///
    /// Returns `true` if the response was dispatched, `false` if the stream
    /// was not found (already timed out or cancelled).
    pub fn dispatch_response(&self, request_id: StreamId, frame: Frame) -> bool {
        let sender = {
            let mut streams = self.streams.lock().unwrap();
            streams.remove(&request_id)
        };

        if sender.is_none() {
            return false;
        }

        let _ = sender.unwrap().send(frame);
        self.active_count.fetch_sub(1, Ordering::Relaxed);
        true
    }

    /// Cancels a stream by removing it from the active map.
    ///
    /// Returns `true` if the stream was found and cancelled, `false` if not found.
    pub fn cancel_stream(&self, id: StreamId) -> bool {
        let removed = {
            let mut streams = self.streams.lock().unwrap();
            streams.remove(&id)
        };

        if removed.is_some() {
            self.active_count.fetch_sub(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// Returns the current number of active streams.
    pub fn active_streams(&self) -> u32 {
        self.active_count.load(Ordering::Relaxed)
    }

    /// Returns a reference to the multiplexer configuration.
    pub fn config(&self) -> &MuxConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Opcode;

    #[test]
    fn test_mux_config_default() {
        let config = MuxConfig::default();
        assert_eq!(config.max_concurrent_streams, 256);
        assert_eq!(config.stream_timeout, Duration::from_secs(30));
    }

    #[test]
    fn test_open_and_dispatch() {
        let mux = Multiplexer::new(MuxConfig::default());
        
        let (id, handle) = mux.open_stream().unwrap();
        assert_eq!(mux.active_streams(), 1);

        let frame = Frame::new(Opcode::Read, id, vec![1, 2, 3]);
        let dispatched = mux.dispatch_response(id, frame.clone());
        assert!(dispatched);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        
        let received = rt.block_on(handle.recv()).unwrap();
        assert_eq!(received.request_id(), id);
        assert_eq!(mux.active_streams(), 0);
    }

    #[test]
    fn test_max_concurrent_streams() {
        let config = MuxConfig {
            max_concurrent_streams: 3,
            stream_timeout: Duration::from_secs(30),
        };
        let mux = Multiplexer::new(config);

        let _ = mux.open_stream().unwrap();
        let _ = mux.open_stream().unwrap();
        let _ = mux.open_stream().unwrap();

        let err = mux.open_stream().unwrap_err();
        assert!(matches!(err, TransportError::InvalidFrame { .. }));

        mux.cancel_stream(1);
        mux.cancel_stream(2);
        mux.cancel_stream(3);
    }

    #[test]
    fn test_cancel_stream() {
        let mux = Multiplexer::new(MuxConfig::default());
        
        let (_, handle) = mux.open_stream().unwrap();
        assert_eq!(mux.active_streams(), 1);

        let cancelled = mux.cancel_stream(1);
        assert!(cancelled);
        assert_eq!(mux.active_streams(), 0);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        
        let result = rt.block_on(handle.recv());
        assert!(result.is_err());
    }

    #[test]
    fn test_dispatch_unknown_stream() {
        let mux = Multiplexer::new(MuxConfig::default());
        
        let frame = Frame::new(Opcode::Read, 999, vec![]);
        let dispatched = mux.dispatch_response(999, frame);
        assert!(!dispatched);
        assert_eq!(mux.active_streams(), 0);
    }

    #[test]
    fn test_active_streams_count() {
        let mux = Multiplexer::new(MuxConfig::default());
        
        let (id1, handle1) = mux.open_stream().unwrap();
        let (_id2, _handle2) = mux.open_stream().unwrap();
        
        assert_eq!(mux.active_streams(), 2);

        let frame1 = Frame::new(Opcode::Read, id1, vec![]);
        mux.dispatch_response(id1, frame1);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        
        let _ = rt.block_on(handle1.recv());
        
        assert_eq!(mux.active_streams(), 1);
    }

    #[test]
    fn test_stream_ids_unique() {
        let mux = Multiplexer::new(MuxConfig::default());
        
        let mut ids = Vec::new();
        for _ in 0..100 {
            let (id, _) = mux.open_stream().unwrap();
            ids.push(id);
        }

        let mut unique_ids = ids.clone();
        unique_ids.sort();
        unique_ids.dedup();

        assert_eq!(ids.len(), unique_ids.len());
    }

    #[tokio::test]
    async fn test_concurrent_dispatch() {
        let mux = Multiplexer::new(MuxConfig::default());
        
        let mut handles = Vec::new();
        let mut ids = Vec::new();
        
        for _ in 0..5 {
            let (id, handle) = mux.open_stream().unwrap();
            ids.push(id);
            handles.push(handle);
        }

        let mux_clone = Arc::new(mux);

        let handles: Vec<_> = ids
            .iter()
            .copied()
            .zip(handles.into_iter())
            .collect();

        let mux_for_spawn = mux_clone.clone();
        
        let handles: Vec<_> = handles
            .into_iter()
            .map(|(id, handle)| {
                let mux_inner = mux_for_spawn.clone();
                let frame = Frame::new(Opcode::Read, id, vec![id as u8]);
                async move {
                    mux_inner.dispatch_response(id, frame);
                    let result = handle.recv().await;
                    (id, result)
                }
            })
            .collect();

        async fn run_all(
            futures: Vec<impl std::future::Future<Output = (u64, std::result::Result<Frame, TransportError>)>>,
        ) -> Vec<(u64, std::result::Result<Frame, TransportError>)> {
            let mut results = Vec::new();
            for f in futures {
                results.push(f.await);
            }
            results
        }

        let results = run_all(handles).await;
        
        for (id, result) in results {
            assert!(result.is_ok());
            assert_eq!(result.unwrap().request_id(), id);
        }

        assert_eq!(mux_clone.active_streams(), 0);
    }
}