use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use dashmap::DashMap;
use thiserror::Error;
use crate::cross_protocol_consistency::Protocol;

/// Span status
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpanStatus {
    Ok,
    Error(String),
    Cancelled,
}

/// Span event
#[derive(Debug, Clone)]
pub struct SpanEvent {
    pub name: String,
    pub timestamp_ns: u64,
    pub attributes: Vec<(String, String)>,
}

/// Protocol span
#[derive(Debug, Clone)]
pub struct ProtocolSpan {
    pub trace_id: [u8; 16],
    pub span_id: [u8; 8],
    pub parent_span_id: Option<[u8; 8]>,
    pub protocol: Protocol,
    pub operation: String,
    pub client_id: u64,
    pub inode_id: u64,
    pub start_time_ns: u64,
    pub end_time_ns: u64,
    pub status: SpanStatus,
    pub attributes: Vec<(String, String)>,
    pub events: Vec<SpanEvent>,
}

/// Latency histogram
#[derive(Debug, Clone, Default)]
pub struct LatencyHistogram {
    pub min_ns: u64,
    pub max_ns: u64,
    pub mean_ns: f64,
    pub p50_ns: u64,
    pub p99_ns: u64,
}

/// Operation metrics
#[derive(Debug, Clone, Default)]
pub struct OpMetrics {
    pub op_name: String,
    pub count: u64,
    pub latency_ns: LatencyHistogram,
    pub errors: u64,
}

/// Protocol metrics
#[derive(Debug, Clone, Default)]
pub struct ProtocolMetrics {
    pub protocol: Protocol,
    pub operations: Arc<DashMap<String, OpMetrics>>,
}

/// Global metrics
#[derive(Debug, Clone, Default)]
pub struct GlobalMetrics {
    pub total_requests: u64,
    pub total_errors: u64,
    pub total_latency_ns: u64,
    pub critical_path_latency: Vec<u64>,
}

/// Observability errors
#[derive(Error, Debug)]
pub enum ObservabilityError {
    #[error("span not found")]
    SpanNotFound,
    #[error("aggregation failed")]
    AggregationFailed,
    #[error("invalid trace")]
    InvalidTrace,
}

/// Gateway observer
pub struct GatewayObserver {
    span_buffer: Arc<DashMap<[u8; 16], Vec<ProtocolSpan>>>,
    per_protocol_metrics: Arc<DashMap<Protocol, ProtocolMetrics>>,
    global_metrics: Arc<std::sync::Mutex<GlobalMetrics>>,
}

impl GatewayObserver {
    pub fn new() -> Self {
        Self {
            span_buffer: Arc::new(DashMap::new()),
            per_protocol_metrics: Arc::new(DashMap::new()),
            global_metrics: Arc::new(tokio::sync::RwLock::new(GlobalMetrics::default())),
        }
    }

    pub fn start_operation_span(
        &self,
        protocol: Protocol,
        operation: &str,
        client_id: u64,
        inode_id: u64,
    ) -> OperationSpanGuard {
        let now_ns = current_time_ns();
        let trace_id = generate_trace_id();
        let span_id = generate_span_id();

        OperationSpanGuard {
            observer: Arc::new(self.clone_ref()),
            trace_id,
            span_id,
            protocol,
            operation: operation.to_string(),
            client_id,
            inode_id,
            start_time_ns: now_ns,
            status: Arc::new(tokio::sync::Mutex::new(SpanStatus::Ok)),
        }
    }

    pub fn record_event(
        &self,
        trace_id: [u8; 16],
        event_name: &str,
        attributes: Vec<(String, String)>,
    ) -> Result<(), ObservabilityError> {
        let event = SpanEvent {
            name: event_name.to_string(),
            timestamp_ns: current_time_ns(),
            attributes,
        };

        if let Some(mut spans) = self.span_buffer.get_mut(&trace_id) {
            if let Some(last_span) = spans.last_mut() {
                last_span.events.push(event);
                Ok(())
            } else {
                Err(ObservabilityError::SpanNotFound)
            }
        } else {
            Err(ObservabilityError::SpanNotFound)
        }
    }

    pub fn end_operation_span(
        &self,
        trace_id: [u8; 16],
        span_id: [u8; 8],
        status: SpanStatus,
    ) -> Result<(), ObservabilityError> {
        let end_time_ns = current_time_ns();

        if let Some(mut spans) = self.span_buffer.get_mut(&trace_id) {
            if let Some(span) = spans.iter_mut().find(|s| s.span_id == span_id) {
                span.end_time_ns = end_time_ns;
                span.status = status.clone();

                let duration_ns = span.end_time_ns - span.start_time_ns;
                let mut global = self.global_metrics.lock();
                global.total_requests += 1;
                global.total_latency_ns += duration_ns;

                if matches!(status, SpanStatus::Error(_)) {
                    global.total_errors += 1;
                }

                Ok(())
            } else {
                Err(ObservabilityError::SpanNotFound)
            }
        } else {
            Err(ObservabilityError::SpanNotFound)
        }
    }

    pub async fn flush_to_aggregator(&self) -> Result<usize, ObservabilityError> {
        let count = self.span_buffer.len();
        self.span_buffer.clear();
        Ok(count)
    }

    pub fn get_protocol_metrics(&self, protocol: Protocol) -> Option<ProtocolMetrics> {
        self.per_protocol_metrics.get(&protocol).map(|m| m.clone())
    }

    pub fn get_operation_latency(
        &self,
        protocol: Protocol,
        operation: &str,
    ) -> Option<OpMetrics> {
        if let Some(metrics) = self.per_protocol_metrics.get(&protocol) {
            metrics.operations.get(operation).map(|m| m.clone())
        } else {
            None
        }
    }

    pub fn global_metrics(&self) -> GlobalMetrics {
        self.global_metrics.lock().clone()
    }

    fn clone_ref(&self) -> Self {
        Self {
            span_buffer: Arc::clone(&self.span_buffer),
            per_protocol_metrics: Arc::clone(&self.per_protocol_metrics),
            global_metrics: Arc::clone(&self.global_metrics),
        }
    }
}

impl Clone for GatewayObserver {
    fn clone(&self) -> Self {
        self.clone_ref()
    }
}

impl Default for GatewayObserver {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII guard for spans
pub struct OperationSpanGuard {
    observer: Arc<GatewayObserver>,
    trace_id: [u8; 16],
    span_id: [u8; 8],
    protocol: Protocol,
    operation: String,
    client_id: u64,
    inode_id: u64,
    start_time_ns: u64,
    status: Arc<std::sync::Mutex<SpanStatus>>,
}

impl Drop for OperationSpanGuard {
    fn drop(&mut self) {
        let status = self.status.lock().clone();
        let _ = self.observer.end_operation_span(self.trace_id, self.span_id, status);
    }
}

fn current_time_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

fn generate_trace_id() -> [u8; 16] {
    let mut id = [0u8; 16];
    let nanos = current_time_ns();
    id[..8].copy_from_slice(&nanos.to_ne_bytes());
    id
}

fn generate_span_id() -> [u8; 8] {
    let nanos = current_time_ns();
    nanos.to_ne_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_start_operation_span() {
        let observer = GatewayObserver::new();
        let _guard = observer.start_operation_span(Protocol::NFS, "READ", 1, 100);
        // Guard auto-completes on drop
    }

    #[tokio::test]
    async fn test_record_event() {
        let observer = GatewayObserver::new();
        let trace_id = [1u8; 16];
        let result = observer
            .record_event(trace_id, "test_event", vec![("key".to_string(), "value".to_string())]);

        assert!(result.is_err()); // No spans yet
    }

    #[tokio::test]
    async fn test_flush_to_aggregator() {
        let observer = GatewayObserver::new();
        let count = observer.flush_to_aggregator().await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_global_metrics() {
        let observer = GatewayObserver::new();
        let metrics = observer.global_metrics();
        assert_eq!(metrics.total_requests, 0);
    }

    #[tokio::test]
    async fn test_clone_observer() {
        let observer1 = GatewayObserver::new();
        let _observer2 = observer1.clone();
        // Both should share same data structures
    }
}
