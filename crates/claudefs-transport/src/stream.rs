//! Streaming data transfer for large payloads.
//!
//! Provides a chunked streaming abstraction above the RPC layer for transferring
//! large data (erasure-coded stripes, file segments) without full buffering.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Configuration for streaming data transfer.
#[derive(Debug, Clone)]
pub struct StreamConfig {
    /// Maximum bytes per chunk.
    pub max_chunk_size: usize,
    /// Flow control window in bytes.
    pub window_size: u64,
    /// Stream timeout in milliseconds.
    pub timeout_ms: u64,
    /// Maximum number of concurrent streams.
    pub max_concurrent_streams: usize,
}

impl Default for StreamConfig {
    fn default() -> Self {
        StreamConfig {
            max_chunk_size: 65536,
            window_size: 4 * 1024 * 1024,
            timeout_ms: 30000,
            max_concurrent_streams: 256,
        }
    }
}

/// Unique identifier for a stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StreamId(u64);

impl StreamId {
    /// Creates a new stream ID from a u64 value.
    pub fn new(id: u64) -> Self {
        StreamId(id)
    }

    /// Returns the inner u64 value.
    pub fn inner(&self) -> u64 {
        self.0
    }
}

impl fmt::Display for StreamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "stream-{}", self.0)
    }
}

/// State of a stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamState {
    /// Stream is being opened.
    Opening,
    /// Stream is actively transferring data.
    Active,
    /// Stream is draining (no more data to send).
    Draining,
    /// Stream has completed normally.
    Closed,
    /// Stream was aborted abnormally.
    Aborted,
}

/// A single chunk of data within a stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    /// The stream this chunk belongs to.
    pub stream_id: StreamId,
    /// Sequence number of this chunk.
    pub sequence: u64,
    /// The data payload.
    pub data: Vec<u8>,
    /// Whether this is the last chunk.
    pub is_last: bool,
}

/// Statistics for stream operations.
#[derive(Debug, Clone, Default)]
pub struct StreamStats {
    /// Total bytes sent.
    pub bytes_sent: u64,
    /// Total bytes received.
    pub bytes_received: u64,
    /// Total chunks sent.
    pub chunks_sent: u64,
    /// Total chunks received.
    pub chunks_received: u64,
    /// Number of currently active streams.
    pub active_streams: usize,
    /// Number of completed streams.
    pub completed_streams: u64,
    /// Number of aborted streams.
    pub aborted_streams: u64,
}

/// Snapshot of stream statistics at a point in time.
#[derive(Debug, Clone, Default)]
pub struct StreamStatsSnapshot {
    /// Total bytes sent.
    pub bytes_sent: u64,
    /// Total bytes received.
    pub bytes_received: u64,
    /// Total chunks sent.
    pub chunks_sent: u64,
    /// Total chunks received.
    pub chunks_received: u64,
    /// Number of currently active streams.
    pub active_streams: usize,
    /// Number of completed streams.
    pub completed_streams: u64,
    /// Number of aborted streams.
    pub aborted_streams: u64,
}

/// Errors that can occur during stream operations.
#[derive(Error, Debug, Serialize, Deserialize)]
pub enum StreamError {
    /// Stream is closed and cannot accept more data.
    #[error("Stream {stream_id} is closed")]
    StreamClosed {
        /// The stream ID.
        stream_id: StreamId,
    },

    /// Stream was aborted.
    #[error("Stream {stream_id} aborted: {reason}")]
    StreamAborted {
        /// The stream ID.
        stream_id: StreamId,
        /// Reason for abort.
        reason: String,
    },

    /// Flow control window is full.
    #[error("Flow control window full")]
    WindowFull,

    /// Invalid sequence number received.
    #[error("Invalid sequence: expected {expected}, got {got}")]
    InvalidSequence {
        /// Expected sequence number.
        expected: u64,
        /// Received sequence number.
        got: u64,
    },

    /// Maximum concurrent streams exceeded.
    #[error("Max concurrent streams exceeded: {max}")]
    MaxStreamsExceeded {
        /// Maximum allowed streams.
        max: usize,
    },
}

/// Internal state for StreamSender.
#[derive(Clone)]
struct SenderState {
    state: StreamState,
    sequence: u64,
    bytes_sent: u64,
    chunks_sent: u64,
}

/// Sender side of a stream.
pub struct StreamSender {
    /// The stream identifier.
    stream_id: StreamId,
    /// Configuration for this stream.
    config: StreamConfig,
    /// Shared internal state.
    inner: Rc<RefCell<SenderState>>,
}

impl Clone for StreamSender {
    fn clone(&self) -> StreamSender {
        StreamSender {
            stream_id: self.stream_id,
            config: self.config.clone(),
            inner: Rc::clone(&self.inner),
        }
    }
}

impl StreamSender {
    /// Creates a new stream sender.
    pub fn new(stream_id: StreamId, config: StreamConfig) -> Self {
        StreamSender {
            stream_id,
            config,
            inner: Rc::new(RefCell::new(SenderState {
                state: StreamState::Opening,
                sequence: 0,
                bytes_sent: 0,
                chunks_sent: 0,
            })),
        }
    }

    /// Sends a chunk of data on the stream.
    ///
    /// Truncates data to max_chunk_size if needed.
    /// Returns an error if the stream is not in a valid state.
    pub fn send_chunk(&mut self, mut data: Vec<u8>) -> Result<StreamChunk, StreamError> {
        let mut inner = self.inner.borrow_mut();

        if inner.state == StreamState::Closed || inner.state == StreamState::Aborted {
            return Err(StreamError::StreamClosed {
                stream_id: self.stream_id,
            });
        }

        if inner.state == StreamState::Draining {
            return Err(StreamError::StreamClosed {
                stream_id: self.stream_id,
            });
        }

        if data.len() > self.config.max_chunk_size {
            data.truncate(self.config.max_chunk_size);
        }

        inner.state = StreamState::Active;

        let chunk = StreamChunk {
            stream_id: self.stream_id,
            sequence: inner.sequence,
            data,
            is_last: false,
        };

        let data_len = chunk.data.len() as u64;
        inner.bytes_sent += data_len;
        inner.chunks_sent += 1;
        inner.sequence += 1;

        Ok(chunk)
    }

    /// Finishes the stream, creating a final chunk.
    ///
    /// Transitions the stream to the Draining state.
    /// Returns an error if the stream is already Closed or Aborted.
    pub fn finish(&mut self) -> Result<StreamChunk, StreamError> {
        let mut inner = self.inner.borrow_mut();

        if inner.state == StreamState::Closed || inner.state == StreamState::Aborted {
            return Err(StreamError::StreamClosed {
                stream_id: self.stream_id,
            });
        }

        inner.state = StreamState::Draining;

        let chunk = StreamChunk {
            stream_id: self.stream_id,
            sequence: inner.sequence,
            data: Vec::new(),
            is_last: true,
        };

        inner.chunks_sent += 1;
        inner.sequence += 1;

        Ok(chunk)
    }

    /// Aborts the stream.
    pub fn abort(&mut self) {
        let mut inner = self.inner.borrow_mut();
        inner.state = StreamState::Aborted;
    }

    /// Returns the current state of the stream.
    pub fn state(&self) -> StreamState {
        self.inner.borrow().state
    }

    /// Returns the stream identifier.
    pub fn stream_id(&self) -> StreamId {
        self.stream_id
    }

    /// Returns the total bytes sent.
    pub fn bytes_sent(&self) -> u64 {
        self.inner.borrow().bytes_sent
    }

    /// Returns the total chunks sent.
    pub fn chunks_sent(&self) -> u64 {
        self.inner.borrow().chunks_sent
    }
}

/// Internal state for StreamReceiver.
#[derive(Clone)]
struct ReceiverState {
    state: StreamState,
    expected_sequence: u64,
    bytes_received: u64,
    chunks_received: u64,
}

/// Receiver side of a stream.
pub struct StreamReceiver {
    /// The stream identifier.
    stream_id: StreamId,
    /// Configuration for this stream.
    config: StreamConfig,
    /// Shared internal state.
    inner: Rc<RefCell<ReceiverState>>,
}

impl Clone for StreamReceiver {
    fn clone(&self) -> StreamReceiver {
        StreamReceiver {
            stream_id: self.stream_id,
            config: self.config.clone(),
            inner: Rc::clone(&self.inner),
        }
    }
}

impl StreamReceiver {
    /// Creates a new stream receiver.
    pub fn new(stream_id: StreamId, config: StreamConfig) -> Self {
        StreamReceiver {
            stream_id,
            config,
            inner: Rc::new(RefCell::new(ReceiverState {
                state: StreamState::Opening,
                expected_sequence: 0,
                bytes_received: 0,
                chunks_received: 0,
            })),
        }
    }

    /// Receives a chunk and processes it.
    ///
    /// Validates the sequence number and updates statistics.
    /// Returns the data if present, or None if this was the final chunk.
    pub fn receive_chunk(&mut self, chunk: StreamChunk) -> Result<Option<Vec<u8>>, StreamError> {
        let mut inner = self.inner.borrow_mut();

        if inner.state == StreamState::Aborted {
            return Err(StreamError::StreamAborted {
                stream_id: self.stream_id,
                reason: "receiver already aborted".to_string(),
            });
        }

        if inner.state == StreamState::Closed {
            return Err(StreamError::StreamClosed {
                stream_id: self.stream_id,
            });
        }

        if chunk.sequence != inner.expected_sequence {
            return Err(StreamError::InvalidSequence {
                expected: inner.expected_sequence,
                got: chunk.sequence,
            });
        }

        inner.bytes_received += chunk.data.len() as u64;
        inner.chunks_received += 1;
        inner.expected_sequence += 1;
        inner.state = StreamState::Active;

        let data = chunk.data;
        let is_last = chunk.is_last;

        if is_last {
            inner.state = StreamState::Closed;
            if data.is_empty() {
                return Ok(None);
            }
            return Ok(Some(data));
        }

        Ok(Some(data))
    }

    /// Returns true if the stream has completed.
    pub fn is_complete(&self) -> bool {
        self.inner.borrow().state == StreamState::Closed
    }

    /// Returns the current state of the stream.
    pub fn state(&self) -> StreamState {
        self.inner.borrow().state
    }

    /// Returns the stream identifier.
    pub fn stream_id(&self) -> StreamId {
        self.stream_id
    }

    /// Returns the total bytes received.
    pub fn bytes_received(&self) -> u64 {
        self.inner.borrow().bytes_received
    }

    /// Returns the total chunks received.
    pub fn chunks_received(&self) -> u64 {
        self.inner.borrow().chunks_received
    }
}

/// Manager for creating and tracking multiple streams.
pub struct StreamManager {
    /// Configuration for streams.
    config: StreamConfig,
    /// Next stream ID to assign.
    next_id: u64,
    /// Active senders.
    active_senders: HashMap<StreamId, StreamSender>,
    /// Active receivers.
    active_receivers: HashMap<StreamId, StreamReceiver>,
    /// Number of completed streams.
    completed_streams: u64,
    /// Number of aborted streams.
    aborted_streams: u64,
    /// Cumulative bytes sent across all streams.
    total_bytes_sent: u64,
    /// Cumulative bytes received across all streams.
    total_bytes_received: u64,
    /// Cumulative chunks sent across all streams.
    total_chunks_sent: u64,
    /// Cumulative chunks received across all streams.
    total_chunks_received: u64,
}

impl StreamManager {
    /// Creates a new stream manager.
    pub fn new(config: StreamConfig) -> Self {
        StreamManager {
            config,
            next_id: 1,
            active_senders: HashMap::new(),
            active_receivers: HashMap::new(),
            completed_streams: 0,
            aborted_streams: 0,
            total_bytes_sent: 0,
            total_bytes_received: 0,
            total_chunks_sent: 0,
            total_chunks_received: 0,
        }
    }

    /// Creates a new sender and returns it.
    ///
    /// Returns an error if the maximum concurrent streams is exceeded.
    pub fn create_sender(&mut self) -> Result<StreamSender, StreamError> {
        let current_count = self.active_senders.len() + self.active_receivers.len();
        if current_count >= self.config.max_concurrent_streams {
            return Err(StreamError::MaxStreamsExceeded {
                max: self.config.max_concurrent_streams,
            });
        }

        let stream_id = StreamId::new(self.next_id);
        self.next_id += 1;

        let sender = StreamSender::new(stream_id, self.config.clone());
        self.active_senders.insert(stream_id, sender.clone());

        Ok(sender)
    }

    /// Creates a new receiver for an existing stream.
    ///
    /// Returns an error if the maximum concurrent streams is exceeded.
    pub fn create_receiver(&mut self, stream_id: StreamId) -> Result<StreamReceiver, StreamError> {
        let current_count = self.active_senders.len() + self.active_receivers.len();
        if current_count >= self.config.max_concurrent_streams {
            return Err(StreamError::MaxStreamsExceeded {
                max: self.config.max_concurrent_streams,
            });
        }

        let receiver = StreamReceiver::new(stream_id, self.config.clone());
        self.active_receivers.insert(stream_id, receiver.clone());

        Ok(receiver)
    }

    /// Removes a sender from the manager.
    ///
    /// Updates completed/aborted stream counters based on final state.
    pub fn remove_sender(&mut self, stream_id: StreamId) {
        if let Some(sender) = self.active_senders.remove(&stream_id) {
            self.total_bytes_sent += sender.bytes_sent();
            self.total_chunks_sent += sender.chunks_sent();
            match sender.state() {
                StreamState::Closed | StreamState::Draining => self.completed_streams += 1,
                StreamState::Aborted => self.aborted_streams += 1,
                _ => {}
            }
        }
    }

    /// Removes a receiver from the manager.
    ///
    /// Updates completed/aborted stream counters based on final state.
    pub fn remove_receiver(&mut self, stream_id: StreamId) {
        if let Some(receiver) = self.active_receivers.remove(&stream_id) {
            self.total_bytes_received += receiver.bytes_received();
            self.total_chunks_received += receiver.chunks_received();
            match receiver.state() {
                StreamState::Closed => self.completed_streams += 1,
                StreamState::Aborted => self.aborted_streams += 1,
                _ => {}
            }
        }
    }

    /// Returns the number of active streams.
    pub fn active_stream_count(&self) -> usize {
        self.active_senders.len() + self.active_receivers.len()
    }

    /// Returns aggregated statistics for all streams.
    pub fn stats(&self) -> StreamStats {
        let mut bytes_sent = self.total_bytes_sent;
        let mut bytes_received = self.total_bytes_received;
        let mut chunks_sent = self.total_chunks_sent;
        let mut chunks_received = self.total_chunks_received;

        for sender in self.active_senders.values() {
            bytes_sent += sender.bytes_sent();
            chunks_sent += sender.chunks_sent();
        }

        for receiver in self.active_receivers.values() {
            bytes_received += receiver.bytes_received();
            chunks_received += receiver.chunks_received();
        }

        StreamStats {
            bytes_sent,
            bytes_received,
            chunks_sent,
            chunks_received,
            active_streams: self.active_stream_count(),
            completed_streams: self.completed_streams,
            aborted_streams: self.aborted_streams,
        }
    }

    /// Returns a snapshot of current statistics.
    pub fn stats_snapshot(&self) -> StreamStatsSnapshot {
        let stats = self.stats();
        StreamStatsSnapshot {
            bytes_sent: stats.bytes_sent,
            bytes_received: stats.bytes_received,
            chunks_sent: stats.chunks_sent,
            chunks_received: stats.chunks_received,
            active_streams: stats.active_streams,
            completed_streams: stats.completed_streams,
            aborted_streams: stats.aborted_streams,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = StreamConfig::default();
        assert_eq!(config.max_chunk_size, 65536);
        assert_eq!(config.window_size, 4 * 1024 * 1024);
        assert_eq!(config.timeout_ms, 30000);
        assert_eq!(config.max_concurrent_streams, 256);
    }

    #[test]
    fn test_stream_id_new_and_inner() {
        let id = StreamId::new(42);
        assert_eq!(id.inner(), 42);
    }

    #[test]
    fn test_stream_id_display() {
        let id = StreamId::new(123);
        let display = format!("{}", id);
        assert_eq!(display, "stream-123");
    }

    #[test]
    fn test_sender_new() {
        let config = StreamConfig::default();
        let sender = StreamSender::new(StreamId::new(1), config.clone());

        assert_eq!(sender.stream_id().inner(), 1);
        assert_eq!(sender.state(), StreamState::Opening);
        assert_eq!(sender.bytes_sent(), 0);
        assert_eq!(sender.chunks_sent(), 0);
    }

    #[test]
    fn test_sender_send_chunk() {
        let config = StreamConfig::default();
        let mut sender = StreamSender::new(StreamId::new(1), config);

        let chunk = sender.send_chunk(vec![1, 2, 3]).unwrap();
        assert_eq!(chunk.stream_id.inner(), 1);
        assert_eq!(chunk.sequence, 0);
        assert_eq!(chunk.data, vec![1, 2, 3]);
        assert!(!chunk.is_last);
        assert_eq!(sender.bytes_sent(), 3);
        assert_eq!(sender.chunks_sent(), 1);
        assert_eq!(sender.state(), StreamState::Active);
    }

    #[test]
    fn test_sender_finish_marks_last() {
        let config = StreamConfig::default();
        let mut sender = StreamSender::new(StreamId::new(1), config);

        sender.send_chunk(vec![1, 2, 3]).unwrap();
        let chunk = sender.finish().unwrap();

        assert!(chunk.is_last);
        assert_eq!(sender.state(), StreamState::Draining);
    }

    #[test]
    fn test_sender_abort() {
        let config = StreamConfig::default();
        let mut sender = StreamSender::new(StreamId::new(1), config);

        sender.send_chunk(vec![1, 2, 3]).unwrap();
        sender.abort();

        assert_eq!(sender.state(), StreamState::Aborted);
    }

    #[test]
    fn test_sender_after_finish_fails() {
        let config = StreamConfig::default();
        let mut sender = StreamSender::new(StreamId::new(1), config);

        sender.finish().unwrap();
        let result = sender.send_chunk(vec![1, 2, 3]);

        assert!(matches!(result, Err(StreamError::StreamClosed { .. })));
    }

    #[test]
    fn test_receiver_new() {
        let config = StreamConfig::default();
        let receiver = StreamReceiver::new(StreamId::new(1), config.clone());

        assert_eq!(receiver.stream_id().inner(), 1);
        assert_eq!(receiver.state(), StreamState::Opening);
        assert_eq!(receiver.bytes_received(), 0);
        assert_eq!(receiver.chunks_received(), 0);
        assert!(!receiver.is_complete());
    }

    #[test]
    fn test_receiver_receive_chunk() {
        let config = StreamConfig::default();
        let mut receiver = StreamReceiver::new(StreamId::new(1), config);

        let chunk = StreamChunk {
            stream_id: StreamId::new(1),
            sequence: 0,
            data: vec![1, 2, 3],
            is_last: false,
        };

        let result = receiver.receive_chunk(chunk).unwrap();
        assert_eq!(result, Some(vec![1, 2, 3]));
        assert_eq!(receiver.bytes_received(), 3);
        assert_eq!(receiver.chunks_received(), 1);
        assert!(!receiver.is_complete());
    }

    #[test]
    fn test_receiver_sequence_validation() {
        let config = StreamConfig::default();
        let mut receiver = StreamReceiver::new(StreamId::new(1), config);

        let chunk = StreamChunk {
            stream_id: StreamId::new(1),
            sequence: 5,
            data: vec![1, 2, 3],
            is_last: false,
        };

        let result = receiver.receive_chunk(chunk);
        assert!(matches!(
            result,
            Err(StreamError::InvalidSequence {
                expected: 0,
                got: 5
            })
        ));
    }

    #[test]
    fn test_receiver_completes_on_last_chunk() {
        let config = StreamConfig::default();
        let mut receiver = StreamReceiver::new(StreamId::new(1), config);

        let chunk = StreamChunk {
            stream_id: StreamId::new(1),
            sequence: 0,
            data: vec![1, 2, 3],
            is_last: true,
        };

        let result = receiver.receive_chunk(chunk).unwrap();
        assert_eq!(result, Some(vec![1, 2, 3]));
        assert!(receiver.is_complete());
        assert_eq!(receiver.state(), StreamState::Closed);
    }

    #[test]
    fn test_receiver_after_close_fails() {
        let config = StreamConfig::default();
        let mut receiver = StreamReceiver::new(StreamId::new(1), config);

        let chunk1 = StreamChunk {
            stream_id: StreamId::new(1),
            sequence: 0,
            data: vec![1, 2, 3],
            is_last: true,
        };
        receiver.receive_chunk(chunk1).unwrap();

        let chunk2 = StreamChunk {
            stream_id: StreamId::new(1),
            sequence: 1,
            data: vec![4, 5, 6],
            is_last: false,
        };
        let result = receiver.receive_chunk(chunk2);
        assert!(matches!(result, Err(StreamError::StreamClosed { .. })));
    }

    #[test]
    fn test_manager_create_sender() {
        let config = StreamConfig::default();
        let mut manager = StreamManager::new(config);

        let sender = manager.create_sender().unwrap();
        assert_eq!(manager.active_stream_count(), 1);

        manager.remove_sender(sender.stream_id());
        assert_eq!(manager.active_stream_count(), 0);
    }

    #[test]
    fn test_manager_create_receiver() {
        let config = StreamConfig::default();
        let mut manager = StreamManager::new(config);

        let stream_id = StreamId::new(100);
        let receiver = manager.create_receiver(stream_id).unwrap();
        assert_eq!(manager.active_stream_count(), 1);

        manager.remove_receiver(stream_id);
        assert_eq!(manager.active_stream_count(), 0);
    }

    #[test]
    fn test_manager_max_concurrent_streams() {
        let config = StreamConfig {
            max_concurrent_streams: 2,
            ..Default::default()
        };
        let mut manager = StreamManager::new(config);

        let _ = manager.create_sender().unwrap();
        let _ = manager.create_sender().unwrap();

        let result = manager.create_sender();
        assert!(matches!(
            result,
            Err(StreamError::MaxStreamsExceeded { .. })
        ));
    }

    #[test]
    fn test_full_stream_simulation() {
        let config = StreamConfig::default();
        let mut manager = StreamManager::new(config);

        let mut sender = manager.create_sender().unwrap();
        let stream_id = sender.stream_id();

        let mut receiver = manager.create_receiver(stream_id).unwrap();

        for i in 0..5 {
            let chunk = sender.send_chunk(vec![i as u8; 100]).unwrap();
            let data = receiver.receive_chunk(chunk).unwrap();
            assert_eq!(data.unwrap().len(), 100);
        }

        let final_chunk = sender.finish().unwrap();
        let data = receiver.receive_chunk(final_chunk).unwrap();
        assert!(data.is_none());

        assert!(receiver.is_complete());

        manager.remove_sender(stream_id);
        manager.remove_receiver(stream_id);

        let stats = manager.stats();
        assert_eq!(stats.completed_streams, 2);
        assert_eq!(stats.chunks_sent, 6);
        assert_eq!(stats.chunks_received, 6);
    }

    #[test]
    fn test_stats_tracking() {
        let config = StreamConfig::default();
        let mut manager = StreamManager::new(config);

        let mut sender1 = manager.create_sender().unwrap();
        let mut sender2 = manager.create_sender().unwrap();

        sender1.send_chunk(vec![1; 100]).unwrap();
        sender1.send_chunk(vec![2; 200]).unwrap();
        sender2.send_chunk(vec![3; 300]).unwrap();
        sender1.finish().unwrap();
        sender2.abort();

        let stats = manager.stats_snapshot();
        assert_eq!(stats.bytes_sent, 600);
        assert_eq!(stats.chunks_sent, 4);
        assert_eq!(stats.active_streams, 2);

        let stream_id1 = sender1.stream_id();
        let stream_id2 = sender2.stream_id();

        manager.remove_sender(stream_id1);
        manager.remove_sender(stream_id2);

        let final_stats = manager.stats();
        assert_eq!(final_stats.completed_streams, 1);
        assert_eq!(final_stats.aborted_streams, 1);
    }
}
