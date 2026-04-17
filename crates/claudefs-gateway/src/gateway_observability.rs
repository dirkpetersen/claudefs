//! OpenTelemetry span instrumentation and per-protocol latency tracking.
//!
//! This module provides distributed tracing capabilities with per-protocol metrics.

use dashmap::DashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use thiserror::Error;

use crate::protocol::Protocol;

/// Trace ID for distributed tracing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TraceId(pub [u8; 16]);

impl TraceId {
    /// Generate a new random trace ID.
    pub fn new() -> Self {
        use rand::RngCore;
        let mut id = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut id);
        TraceId(id)
    }
}

impl Default for TraceId {
    fn default() -> Self {
        Self::new()
    }
}

/// Span ID for distributed tracing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpanId([u8; 8]);

impl SpanId {
    /// Generate a new random span ID.
    pub fn new() -> Self {
        use rand::RngCore;
        let mut id = [0u8; 8];
        rand::thread_rng().fill_bytes(&mut id);
        SpanId(id)
    }
}

/// Span status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SpanStatus {
    /// Span completed successfully
    Ok,
    /// Span encountered an error
    Error,
    /// Span was cancelled
    Cancelled,
}

/// Span event.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SpanEvent {
    /// Event name
    pub name: String,
    /// Event timestamp (ns since epoch)
    pub timestamp_ns: u64,
}

/// Protocol span record.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProtocolSpan {
    /// Trace ID this span belongs to
    pub trace_id: TraceId,
    /// Unique span ID
    pub span_id: SpanId,
    /// Parent span ID if any
    pub parent_span_id: Option<SpanId>,
    /// Which protocol
    pub protocol: Protocol,
    /// Operation name (READ, WRITE, MKDIR, etc.)
    pub operation: String,
    /// Client ID
    pub client_id: u64,
    /// Inode ID
    pub inode_id: u64,
    /// Span start time (ns since epoch)
    pub start_time_ns: u64,
    /// Span end time (ns since epoch)
    pub end_time_ns: u64,
    /// Final status
    pub status: SpanStatus,
    /// Associated events
    pub events: Vec<SpanEvent>,
}

/// Operation metrics.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OpMetrics {
    /// Operation name
    pub op_name: String,
    /// Total count
    pub count: u64,
    /// Total latency in nanoseconds
    pub total_latency_ns: u64,
    /// Minimum latency
    pub min_latency_ns: u64,
    /// Maximum latency
    pub max_latency_ns: u64,
    /// Error count
    pub errors: u64,
}

/// Protocol-level metrics.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProtocolMetrics {
    /// Protocol
    pub protocol: Protocol,
    /// Total operations
    pub total_ops: u64,
    /// Total latency
    pub total_latency_ns: u64,
    /// Total errors
    pub total_errors: u64,
}

/// Global metrics.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GlobalMetrics {
    /// Total requests
    pub total_requests: u64,
    /// Total errors
    pub total_errors: u64,
    /// Total latency
    pub total_latency_ns: u64,
}

/// Errors for observability operations.
#[derive(Debug, Error)]
pub enum ObservabilityError {
    /// Span not found
    #[error("Span not found")]
    SpanNotFound,
    /// Aggregation failed
    #[error("Aggregation failed")]
    AggregationFailed,
    /// Invalid trace
    #[error("Invalid trace")]
    InvalidTrace,
}

/// Gateway observer for distributed tracing and metrics.
pub struct GatewayObserver {
    span_buffer: Arc<DashMap<TraceId, Vec<ProtocolSpan>>>,
    per_protocol_metrics: Arc<DashMap<Protocol, ProtocolMetrics>>,
    global_metrics: Arc<parking_lot::Mutex<GlobalMetrics>>,
}

impl GatewayObserver {
    /// Create a new gateway observer.
    pub fn new() -> Self {
        Self {
            span_buffer: Arc::new(DashMap::new()),
            per_protocol_metrics: Arc::new(DashMap::new()),
            global_metrics: Arc::new(parking_lot::Mutex::new(GlobalMetrics {
                total_requests: 0,
                total_errors: 0,
                total_latency_ns: 0,
            })),
        }
    }

    /// Start a new operation span.
    pub fn start_operation_span(
        &self,
        trace_id: TraceId,
        protocol: Protocol,
        operation: &str,
        client_id: u64,
        inode_id: u64,
    ) -> ProtocolSpan {
        let start_time_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        ProtocolSpan {
            trace_id,
            span_id: SpanId::new(),
            parent_span_id: None,
            protocol,
            operation: operation.to_string(),
            client_id,
            inode_id,
            start_time_ns,
            end_time_ns: 0,
            status: SpanStatus::Ok,
            events: Vec::new(),
        }
    }

    /// Record an event within a span.
    pub fn record_event(
        &self,
        trace_id: TraceId,
        event_name: &str,
    ) -> Result<(), ObservabilityError> {
        let timestamp_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        if let Some(mut spans) = self.span_buffer.get_mut(&trace_id) {
            if let Some(span) = spans.last_mut() {
                span.events.push(SpanEvent {
                    name: event_name.to_string(),
                    timestamp_ns,
                });
                return Ok(());
            }
        }

        Err(ObservabilityError::SpanNotFound)
    }

    /// End an operation span.
    pub fn end_operation_span(
        &self,
        trace_id: TraceId,
        mut span: ProtocolSpan,
        status: SpanStatus,
    ) -> Result<(), ObservabilityError> {
        let end_time_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let latency_ns = end_time_ns.saturating_sub(span.start_time_ns);
        span.end_time_ns = end_time_ns;
        span.status = status;

        self.span_buffer
            .entry(trace_id)
            .or_insert_with(Vec::new)
            .push(span.clone());

        // Update per-protocol metrics
        let is_error = span.status == SpanStatus::Error;
        self.per_protocol_metrics
            .entry(span.protocol)
            .or_insert_with(|| ProtocolMetrics {
                protocol: span.protocol,
                total_ops: 0,
                total_latency_ns: 0,
                total_errors: 0,
            })
            .and_modify(|m| {
                m.total_ops += 1;
                m.total_latency_ns += latency_ns;
                if is_error {
                    m.total_errors += 1;
                }
            });

        // Update global metrics
        {
            let mut global = self.global_metrics.lock();
            global.total_requests += 1;
            global.total_latency_ns += latency_ns;
            if is_error {
                global.total_errors += 1;
            }
        }

        Ok(())
    }

    /// Get metrics for a protocol.
    pub fn get_protocol_metrics(&self, protocol: Protocol) -> Option<ProtocolMetrics> {
        self.per_protocol_metrics.get(&protocol).map(|m| m.clone())
    }

    /// Get global metrics.
    pub fn global_metrics(&self) -> GlobalMetrics {
        self.global_metrics.lock().clone()
    }

    /// Export and flush all spans.
    pub fn flush_to_aggregator(&self) -> Result<usize, ObservabilityError> {
        let count = self.span_buffer.len();
        self.span_buffer.clear();
        Ok(count)
    }
}

impl Default for GatewayObserver {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII guard for automatic span completion.
pub struct OperationSpanGuard {
    observer: Arc<GatewayObserver>,
    span: ProtocolSpan,
    trace_id: TraceId,
    completed: Arc<AtomicUsize>,
}

impl OperationSpanGuard {
    /// Create a new span guard.
    pub fn new(observer: Arc<GatewayObserver>, trace_id: TraceId, span: ProtocolSpan) -> Self {
        Self {
            observer,
            span,
            trace_id,
            completed: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Set the final status.
    pub fn set_status(&mut self, status: SpanStatus) {
        self.span.status = status;
    }
}

impl Drop for OperationSpanGuard {
    fn drop(&mut self) {
        let was_completed = self
            .completed
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok();

        if was_completed {
            let _ = self.observer.end_operation_span(
                self.trace_id,
                self.span.clone(),
                self.span.status,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_id_generation() {
        let id1 = TraceId::new();
        let id2 = TraceId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_span_id_generation() {
        let id1 = SpanId::new();
        let id2 = SpanId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_start_operation_span() {
        let observer = GatewayObserver::new();
        let trace_id = TraceId::new();

        let span = observer.start_operation_span(trace_id, Protocol::Nfs3, "READ", 1, 100);

        assert_eq!(span.trace_id, trace_id);
        assert_eq!(span.protocol, Protocol::Nfs3);
        assert_eq!(span.operation, "READ");
        assert_eq!(span.status, SpanStatus::Ok);
    }

    #[test]
    fn test_end_operation_span() {
        let observer = Arc::new(GatewayObserver::new());
        let trace_id = TraceId::new();

        let span = observer.start_operation_span(trace_id, Protocol::Nfs3, "READ", 1, 100);

        let result = observer.end_operation_span(trace_id, span, SpanStatus::Ok);
        assert!(result.is_ok());

        let metrics = observer.global_metrics();
        assert_eq!(metrics.total_requests, 1);
    }

    #[test]
    fn test_record_event() {
        let observer = Arc::new(GatewayObserver::new());
        let trace_id = TraceId::new();

        let span = observer.start_operation_span(trace_id, Protocol::Nfs3, "READ", 1, 100);

        observer
            .span_buffer
            .insert(trace_id, vec![span]);

        let result = observer.record_event(trace_id, "metadata_lookup");
        assert!(result.is_ok());

        let spans = observer.span_buffer.get(&trace_id).unwrap();
        assert_eq!(spans[0].events.len(), 1);
    }

    #[test]
    fn test_protocol_metrics() {
        let observer = Arc::new(GatewayObserver::new());

        for i in 0..5 {
            let trace_id = TraceId::new();
            let span = observer.start_operation_span(trace_id, Protocol::Nfs3, "READ", 1, 100 + i);
            let _ = observer.end_operation_span(trace_id, span, SpanStatus::Ok);
        }

        let metrics = observer.get_protocol_metrics(Protocol::Nfs3).unwrap();
        assert_eq!(metrics.total_ops, 5);
    }

    #[test]
    fn test_error_counting() {
        let observer = Arc::new(GatewayObserver::new());

        let trace_id1 = TraceId::new();
        let span1 = observer.start_operation_span(trace_id1, Protocol::Nfs3, "READ", 1, 100);
        observer.end_operation_span(trace_id1, span1, SpanStatus::Ok).ok();

        let trace_id2 = TraceId::new();
        let span2 = observer.start_operation_span(trace_id2, Protocol::Nfs3, "WRITE", 1, 100);
        observer.end_operation_span(trace_id2, span2, SpanStatus::Error).ok();

        let metrics = observer.global_metrics();
        assert_eq!(metrics.total_requests, 2);
        assert_eq!(metrics.total_errors, 1);
    }

    #[test]
    fn test_global_metrics() {
        let observer = Arc::new(GatewayObserver::new());

        let trace_id = TraceId::new();
        let span = observer.start_operation_span(trace_id, Protocol::S3, "GET", 2, 200);
        observer.end_operation_span(trace_id, span, SpanStatus::Ok).ok();

        let metrics = observer.global_metrics();
        assert_eq!(metrics.total_requests, 1);
        assert!(metrics.total_latency_ns > 0);
    }

    #[test]
    fn test_flush_to_aggregator() {
        let observer = Arc::new(GatewayObserver::new());

        let trace_id = TraceId::new();
        let span = observer.start_operation_span(trace_id, Protocol::Nfs3, "READ", 1, 100);
        let _ = observer.end_operation_span(trace_id, span, SpanStatus::Ok);

        let count = observer.flush_to_aggregator().unwrap();
        assert_eq!(count, 1);

        // After flush, buffer should be empty
        let count2 = observer.flush_to_aggregator().unwrap();
        assert_eq!(count2, 0);
    }

    #[test]
    fn test_operation_span_guard_drop() {
        let observer = Arc::new(GatewayObserver::new());
        let trace_id = TraceId::new();

        {
            let span = observer.start_operation_span(trace_id, Protocol::Nfs3, "READ", 1, 100);
            let _guard = OperationSpanGuard::new(Arc::clone(&observer), trace_id, span);
        } // Guard dropped here

        let metrics = observer.global_metrics();
        assert_eq!(metrics.total_requests, 1);
    }

    #[test]
    fn test_multiple_protocols() {
        let observer = Arc::new(GatewayObserver::new());

        for protocol in &[Protocol::Nfs3, Protocol::S3, Protocol::Smb3] {
            let trace_id = TraceId::new();
            let span = observer.start_operation_span(trace_id, *protocol, "OP", 1, 100);
            observer.end_operation_span(trace_id, span, SpanStatus::Ok).ok();
        }

        let metrics = observer.global_metrics();
        assert_eq!(metrics.total_requests, 3);

        let nfs_metrics = observer.get_protocol_metrics(Protocol::Nfs3).unwrap();
        assert_eq!(nfs_metrics.total_ops, 1);

        let s3_metrics = observer.get_protocol_metrics(Protocol::S3).unwrap();
        assert_eq!(s3_metrics.total_ops, 1);
    }

    #[test]
    fn test_latency_accumulation() {
        let observer = Arc::new(GatewayObserver::new());

        let trace_id1 = TraceId::new();
        let span1 = observer.start_operation_span(trace_id1, Protocol::Nfs3, "OP1", 1, 100);
        std::thread::sleep(std::time::Duration::from_millis(10));
        observer.end_operation_span(trace_id1, span1, SpanStatus::Ok).ok();

        let metrics = observer.global_metrics();
        assert!(metrics.total_latency_ns >= 10_000_000); // At least 10ms
    }

    #[test]
    fn test_span_not_found_error() {
        let observer = Arc::new(GatewayObserver::new());
        let trace_id = TraceId::new();

        let result = observer.record_event(trace_id, "event");
        assert!(matches!(result, Err(ObservabilityError::SpanNotFound)));
    }
}
