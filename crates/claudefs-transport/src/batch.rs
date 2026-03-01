//! Batch RPC module for request batching and coalescing.
//!
//! This module provides request batching for efficient network communication.
//! Many small metadata operations (lookups, getattrs, etc.) can be batched
//! together to reduce round-trip overhead.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{Result, TransportError};

/// Configuration for batch collection behavior.
#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// Maximum number of requests per batch.
    pub max_batch_size: usize,
    /// Maximum total payload bytes per batch.
    pub max_batch_bytes: usize,
    /// How long to wait for more requests before flushing.
    pub linger_duration: Duration,
    /// Whether batching is enabled.
    pub enabled: bool,
}

impl Default for BatchConfig {
    fn default() -> Self {
        BatchConfig {
            max_batch_size: 32,
            max_batch_bytes: 1024 * 1024,
            linger_duration: Duration::from_millis(1),
            enabled: true,
        }
    }
}

/// A single request within a batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRequest {
    /// Unique request identifier.
    pub request_id: u64,
    /// Operation code as a u8.
    pub opcode: u8,
    /// Serialized request payload.
    pub payload: Vec<u8>,
}

impl BatchRequest {
    /// Create a new batch request.
    pub fn new(request_id: u64, opcode: u8, payload: Vec<u8>) -> Self {
        BatchRequest {
            request_id,
            opcode,
            payload,
        }
    }

    /// Returns the size of the payload in bytes.
    pub fn payload_size(&self) -> usize {
        self.payload.len()
    }
}

/// A single response within a batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResponse {
    /// Unique request identifier this response corresponds to.
    pub request_id: u64,
    /// Operation code as a u8.
    pub opcode: u8,
    /// Serialized response payload.
    pub payload: Vec<u8>,
    /// Error message if the operation failed.
    pub error: Option<String>,
}

impl BatchResponse {
    /// Create a new batch response.
    pub fn new(request_id: u64, opcode: u8, payload: Vec<u8>, error: Option<String>) -> Self {
        BatchResponse {
            request_id,
            opcode,
            payload,
            error,
        }
    }

    /// Create a successful batch response.
    pub fn success(request_id: u64, opcode: u8, payload: Vec<u8>) -> Self {
        BatchResponse {
            request_id,
            opcode,
            payload,
            error: None,
        }
    }

    /// Create an error batch response.
    pub fn error(request_id: u64, opcode: u8, error_msg: String) -> Self {
        BatchResponse {
            request_id,
            opcode,
            payload: Vec::new(),
            error: Some(error_msg),
        }
    }

    /// Check if this response indicates an error.
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }

    /// Returns the size of the payload in bytes.
    pub fn payload_size(&self) -> usize {
        self.payload.len()
    }
}

/// A single item within a batch envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BatchItem {
    /// A request item.
    Request(BatchRequest),
    /// A response item.
    Response(BatchResponse),
}

impl BatchItem {
    /// Returns the payload size of this item.
    pub fn payload_size(&self) -> usize {
        match self {
            BatchItem::Request(req) => req.payload_size(),
            BatchItem::Response(resp) => resp.payload_size(),
        }
    }
}

/// Wire format for a batch of requests or responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchEnvelope {
    /// Unique batch identifier.
    pub batch_id: u64,
    /// Items in this batch.
    pub items: Vec<BatchItem>,
}

impl BatchEnvelope {
    /// Create a new request batch envelope.
    pub fn new_request_batch(batch_id: u64, requests: Vec<BatchRequest>) -> Self {
        let items = requests.into_iter().map(BatchItem::Request).collect();
        BatchEnvelope { batch_id, items }
    }

    /// Create a new response batch envelope.
    pub fn new_response_batch(batch_id: u64, responses: Vec<BatchResponse>) -> Self {
        let items = responses.into_iter().map(BatchItem::Response).collect();
        BatchEnvelope { batch_id, items }
    }

    /// Encode the envelope to bytes using bincode.
    pub fn encode(&self) -> Vec<u8> {
        bincode::serialize(self).expect("BatchEnvelope serialization should never fail")
    }

    /// Decode an envelope from bytes using bincode.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        bincode::deserialize(bytes).map_err(|e| TransportError::SerializationError(e.to_string()))
    }

    /// Returns the number of items in this batch.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns true if this batch contains no items.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Returns the sum of all item payload sizes.
    pub fn total_payload_bytes(&self) -> usize {
        self.items.iter().map(|item| item.payload_size()).sum()
    }
}

/// Result of adding a request to a batch collector.
#[derive(Debug)]
pub enum BatchResult {
    /// Request was buffered, waiting for more requests.
    Buffered,
    /// Batch is ready to be sent.
    Ready(BatchEnvelope),
}

/// Statistics for batch operations.
pub struct BatchStats {
    batches_created: AtomicU64,
    requests_batched: AtomicU64,
    bytes_batched: AtomicU64,
    flushes: AtomicU64,
}

impl BatchStats {
    /// Create a new batch stats instance.
    pub fn new() -> Self {
        BatchStats {
            batches_created: AtomicU64::new(0),
            requests_batched: AtomicU64::new(0),
            bytes_batched: AtomicU64::new(0),
            flushes: AtomicU64::new(0),
        }
    }

    /// Get a snapshot of current statistics.
    pub fn snapshot(&self) -> BatchStatsSnapshot {
        BatchStatsSnapshot {
            batches_created: self.batches_created.load(Ordering::Relaxed),
            requests_batched: self.requests_batched.load(Ordering::Relaxed),
            bytes_batched: self.bytes_batched.load(Ordering::Relaxed),
            flushes: self.flushes.load(Ordering::Relaxed),
        }
    }

    fn record_batch(&self, request_count: u64, byte_count: u64) {
        self.batches_created.fetch_add(1, Ordering::Relaxed);
        self.requests_batched
            .fetch_add(request_count, Ordering::Relaxed);
        self.bytes_batched.fetch_add(byte_count, Ordering::Relaxed);
    }

    fn record_flush(&self) {
        self.flushes.fetch_add(1, Ordering::Relaxed);
    }
}

impl Default for BatchStats {
    fn default() -> Self {
        Self::new()
    }
}

/// A snapshot of batch statistics at a point in time.
#[derive(Debug, Clone, Default)]
pub struct BatchStatsSnapshot {
    /// Total number of batches created.
    pub batches_created: u64,
    /// Total number of requests that have been batched.
    pub requests_batched: u64,
    /// Total bytes batched.
    pub bytes_batched: u64,
    /// Total number of explicit flushes.
    pub flushes: u64,
}

/// Accumulates requests into batches for efficient network transmission.
pub struct BatchCollector {
    config: BatchConfig,
    pending: Mutex<Vec<BatchRequest>>,
    pending_bytes: AtomicUsize,
    batch_counter: AtomicU64,
    stats: BatchStats,
}

impl BatchCollector {
    /// Create a new batch collector with the given configuration.
    pub fn new(config: BatchConfig) -> Self {
        BatchCollector {
            config,
            pending: Mutex::new(Vec::new()),
            pending_bytes: AtomicUsize::new(0),
            batch_counter: AtomicU64::new(0),
            stats: BatchStats::new(),
        }
    }

    /// Add a request to the batch collector.
    ///
    /// If the batch is full (by count or bytes), returns `BatchResult::Ready`
    /// with the completed batch. Otherwise returns `BatchResult::Buffered`.
    pub fn add(&self, request: BatchRequest) -> BatchResult {
        let request_bytes = request.payload_size();

        if !self.config.enabled {
            let batch_id = self.batch_counter.fetch_add(1, Ordering::Relaxed);
            let envelope = BatchEnvelope::new_request_batch(batch_id, vec![request]);
            self.stats.record_batch(1, request_bytes as u64);
            return BatchResult::Ready(envelope);
        }

        let mut pending = self.pending.lock().unwrap();
        let current_count = pending.len();
        let current_bytes = self.pending_bytes.load(Ordering::Relaxed);
        let new_bytes = current_bytes + request_bytes;

        let would_exceed_count = current_count >= self.config.max_batch_size;
        let would_exceed_bytes = new_bytes > self.config.max_batch_bytes;

        if would_exceed_count || would_exceed_bytes {
            let envelope = self.flush_locked(&mut pending);
            pending.push(request);
            self.pending_bytes.store(request_bytes, Ordering::Relaxed);
            return BatchResult::Ready(envelope);
        }

        pending.push(request);
        self.pending_bytes.store(new_bytes, Ordering::Relaxed);
        BatchResult::Buffered
    }

    /// Flush all pending requests into a batch.
    ///
    /// Returns `None` if there are no pending requests.
    pub fn flush(&self) -> Option<BatchEnvelope> {
        let mut pending = self.pending.lock().unwrap();
        if pending.is_empty() {
            None
        } else {
            Some(self.flush_locked(&mut pending))
        }
    }

    fn flush_locked(&self, pending: &mut Vec<BatchRequest>) -> BatchEnvelope {
        let requests = std::mem::take(pending);
        let request_count = requests.len() as u64;
        let byte_count = self.pending_bytes.load(Ordering::Relaxed) as u64;

        self.pending_bytes.store(0, Ordering::Relaxed);

        let batch_id = self.batch_counter.fetch_add(1, Ordering::Relaxed);
        let envelope = BatchEnvelope::new_request_batch(batch_id, requests);

        self.stats.record_batch(request_count, byte_count);
        self.stats.record_flush();

        envelope
    }

    /// Returns the number of pending requests waiting to be batched.
    pub fn pending_count(&self) -> usize {
        self.pending.lock().unwrap().len()
    }

    /// Returns the total bytes of pending requests.
    pub fn pending_bytes(&self) -> usize {
        self.pending_bytes.load(Ordering::Relaxed)
    }

    /// Get a snapshot of current batch statistics.
    pub fn stats(&self) -> BatchStatsSnapshot {
        self.stats.snapshot()
    }

    /// Get the configuration for this collector.
    pub fn config(&self) -> &BatchConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_config_default() {
        let config = BatchConfig::default();
        assert_eq!(config.max_batch_size, 32);
        assert_eq!(config.max_batch_bytes, 1024 * 1024);
        assert_eq!(config.linger_duration, Duration::from_millis(1));
        assert!(config.enabled);
    }

    #[test]
    fn test_batch_request_new() {
        let request = BatchRequest::new(42, 1, vec![1, 2, 3, 4]);
        assert_eq!(request.request_id, 42);
        assert_eq!(request.opcode, 1);
        assert_eq!(request.payload, vec![1, 2, 3, 4]);
        assert_eq!(request.payload_size(), 4);
    }

    #[test]
    fn test_batch_response_new() {
        let response = BatchResponse::new(42, 1, vec![5, 6, 7], Some("error".to_string()));
        assert_eq!(response.request_id, 42);
        assert_eq!(response.opcode, 1);
        assert_eq!(response.payload, vec![5, 6, 7]);
        assert_eq!(response.error, Some("error".to_string()));
        assert!(response.is_error());
    }

    #[test]
    fn test_batch_response_success() {
        let response = BatchResponse::success(42, 1, vec![1, 2, 3]);
        assert_eq!(response.request_id, 42);
        assert_eq!(response.opcode, 1);
        assert_eq!(response.payload, vec![1, 2, 3]);
        assert!(response.error.is_none());
        assert!(!response.is_error());
    }

    #[test]
    fn test_batch_response_error() {
        let response = BatchResponse::error(42, 1, "something went wrong".to_string());
        assert_eq!(response.request_id, 42);
        assert_eq!(response.opcode, 1);
        assert!(response.payload.is_empty());
        assert_eq!(response.error, Some("something went wrong".to_string()));
        assert!(response.is_error());
    }

    #[test]
    fn test_batch_envelope_request_batch() {
        let requests = vec![
            BatchRequest::new(1, 1, vec![1]),
            BatchRequest::new(2, 2, vec![2, 2]),
        ];
        let envelope = BatchEnvelope::new_request_batch(100, requests);
        assert_eq!(envelope.batch_id, 100);
        assert_eq!(envelope.len(), 2);
        assert!(!envelope.is_empty());
    }

    #[test]
    fn test_batch_envelope_response_batch() {
        let responses = vec![
            BatchResponse::success(1, 1, vec![1]),
            BatchResponse::error(2, 2, "error".to_string()),
        ];
        let envelope = BatchEnvelope::new_response_batch(200, responses);
        assert_eq!(envelope.batch_id, 200);
        assert_eq!(envelope.len(), 2);
        assert!(!envelope.is_empty());
    }

    #[test]
    fn test_batch_envelope_encode_decode() {
        let requests = vec![
            BatchRequest::new(1, 1, vec![1, 2, 3]),
            BatchRequest::new(2, 2, vec![4, 5, 6, 7]),
        ];
        let original = BatchEnvelope::new_request_batch(42, requests);

        let encoded = original.encode();
        let decoded = BatchEnvelope::decode(&encoded).unwrap();

        assert_eq!(decoded.batch_id, original.batch_id);
        assert_eq!(decoded.len(), original.len());
    }

    #[test]
    fn test_batch_envelope_len() {
        let envelope = BatchEnvelope::new_request_batch(
            1,
            vec![
                BatchRequest::new(1, 1, vec![]),
                BatchRequest::new(2, 2, vec![]),
                BatchRequest::new(3, 3, vec![]),
            ],
        );
        assert_eq!(envelope.len(), 3);
    }

    #[test]
    fn test_batch_envelope_empty() {
        let envelope = BatchEnvelope::new_request_batch(1, vec![]);
        assert!(envelope.is_empty());
        assert_eq!(envelope.len(), 0);
    }

    #[test]
    fn test_batch_envelope_total_bytes() {
        let requests = vec![
            BatchRequest::new(1, 1, vec![1; 10]),
            BatchRequest::new(2, 2, vec![2; 20]),
            BatchRequest::new(3, 3, vec![3; 30]),
        ];
        let envelope = BatchEnvelope::new_request_batch(1, requests);
        assert_eq!(envelope.total_payload_bytes(), 60);
    }

    #[test]
    fn test_collector_add_buffered() {
        let config = BatchConfig {
            max_batch_size: 10,
            ..Default::default()
        };
        let collector = BatchCollector::new(config);

        let request = BatchRequest::new(1, 1, vec![1, 2, 3]);
        let result = collector.add(request);

        assert!(matches!(result, BatchResult::Buffered));
        assert_eq!(collector.pending_count(), 1);
    }

    #[test]
    fn test_collector_add_triggers_ready() {
        let config = BatchConfig {
            max_batch_size: 2,
            ..Default::default()
        };
        let collector = BatchCollector::new(config);

        let r1 = BatchRequest::new(1, 1, vec![1]);
        let r2 = BatchRequest::new(2, 2, vec![2]);
        let r3 = BatchRequest::new(3, 3, vec![3]);

        let result1 = collector.add(r1);
        assert!(matches!(result1, BatchResult::Buffered));

        let result2 = collector.add(r2);
        assert!(matches!(result2, BatchResult::Buffered));

        let result3 = collector.add(r3);
        match result3 {
            BatchResult::Ready(envelope) => {
                assert_eq!(envelope.len(), 2);
            }
            _ => panic!("Expected Ready with full batch"),
        }

        assert_eq!(collector.pending_count(), 1);
    }

    #[test]
    fn test_collector_byte_limit() {
        let config = BatchConfig {
            max_batch_size: 100,
            max_batch_bytes: 10,
            ..Default::default()
        };
        let collector = BatchCollector::new(config);

        let r1 = BatchRequest::new(1, 1, vec![1; 5]);
        let r2 = BatchRequest::new(2, 2, vec![2; 5]);
        let r3 = BatchRequest::new(3, 3, vec![3; 6]);

        collector.add(r1);
        collector.add(r2);

        let result = collector.add(r3);
        match result {
            BatchResult::Ready(envelope) => {
                assert_eq!(envelope.len(), 2);
            }
            _ => panic!("Expected Ready when byte limit exceeded"),
        }

        assert_eq!(collector.pending_count(), 1);
        assert_eq!(collector.pending_bytes(), 6);
    }

    #[test]
    fn test_collector_flush_empty() {
        let collector = BatchCollector::new(BatchConfig::default());
        let result = collector.flush();
        assert!(result.is_none());
    }

    #[test]
    fn test_collector_flush_pending() {
        let config = BatchConfig {
            max_batch_size: 100,
            ..Default::default()
        };
        let collector = BatchCollector::new(config);

        collector.add(BatchRequest::new(1, 1, vec![1]));
        collector.add(BatchRequest::new(2, 2, vec![2]));
        collector.add(BatchRequest::new(3, 3, vec![3]));

        let envelope = collector.flush().unwrap();
        assert_eq!(envelope.len(), 3);
        assert!(collector.flush().is_none());
        assert_eq!(collector.pending_count(), 0);
    }

    #[test]
    fn test_collector_pending_count() {
        let config = BatchConfig {
            max_batch_size: 100,
            ..Default::default()
        };
        let collector = BatchCollector::new(config);

        assert_eq!(collector.pending_count(), 0);

        collector.add(BatchRequest::new(1, 1, vec![]));
        assert_eq!(collector.pending_count(), 1);

        collector.add(BatchRequest::new(2, 2, vec![]));
        assert_eq!(collector.pending_count(), 2);

        collector.flush();
        assert_eq!(collector.pending_count(), 0);
    }

    #[test]
    fn test_collector_stats() {
        let config = BatchConfig {
            max_batch_size: 2,
            ..Default::default()
        };
        let collector = BatchCollector::new(config);

        let stats = collector.stats();
        assert_eq!(stats.batches_created, 0);
        assert_eq!(stats.requests_batched, 0);

        collector.add(BatchRequest::new(1, 1, vec![1; 10]));
        collector.add(BatchRequest::new(2, 2, vec![2; 10]));
        collector.add(BatchRequest::new(3, 3, vec![3; 10]));

        let stats = collector.stats();
        assert_eq!(stats.batches_created, 1);
        assert_eq!(stats.requests_batched, 2);
        assert_eq!(stats.bytes_batched, 20);
        assert_eq!(stats.flushes, 1);

        collector.flush();
        let stats = collector.stats();
        assert_eq!(stats.batches_created, 2);
        assert_eq!(stats.requests_batched, 3);
        assert_eq!(stats.flushes, 2);
    }

    #[test]
    fn test_collector_disabled() {
        let config = BatchConfig {
            enabled: false,
            ..Default::default()
        };
        let collector = BatchCollector::new(config);

        let r1 = BatchRequest::new(1, 1, vec![1]);
        let r2 = BatchRequest::new(2, 2, vec![2]);

        match collector.add(r1) {
            BatchResult::Ready(envelope) => {
                assert_eq!(envelope.len(), 1);
            }
            _ => panic!("Expected immediate Ready when disabled"),
        }

        match collector.add(r2) {
            BatchResult::Ready(envelope) => {
                assert_eq!(envelope.len(), 1);
            }
            _ => panic!("Expected immediate Ready when disabled"),
        }

        assert_eq!(collector.pending_count(), 0);

        let stats = collector.stats();
        assert_eq!(stats.batches_created, 2);
        assert_eq!(stats.requests_batched, 2);
    }

    #[test]
    fn test_batch_item_payload_size() {
        let req_item = BatchItem::Request(BatchRequest::new(1, 1, vec![1; 50]));
        assert_eq!(req_item.payload_size(), 50);

        let resp_item = BatchItem::Response(BatchResponse::success(1, 1, vec![2; 30]));
        assert_eq!(resp_item.payload_size(), 30);
    }

    #[test]
    fn test_batch_stats_snapshot() {
        let stats = BatchStats::new();
        stats.record_batch(5, 100);
        stats.record_flush();

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.batches_created, 1);
        assert_eq!(snapshot.requests_batched, 5);
        assert_eq!(snapshot.bytes_batched, 100);
        assert_eq!(snapshot.flushes, 1);
    }
}
