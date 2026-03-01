//! OpenTelemetry tracing types for cross-site replication.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{info, warn};

static TRACE_COUNTER: AtomicU64 = AtomicU64::new(0);
static SPAN_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A 128-bit trace identifier.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TraceId(pub [u8; 16]);

impl TraceId {
    /// Creates a new trace ID using a sequential counter.
    pub fn new_random() -> Self {
        let counter = TRACE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut bytes = [0u8; 16];
        bytes[..8].copy_from_slice(&counter.to_be_bytes());
        TraceId(bytes)
    }

    /// Converts the trace ID to a 32-character lowercase hex string.
    pub fn to_hex(&self) -> String {
        self.0.iter().fold(String::new(), |mut acc, b| {
            acc.push_str(&format!("{:02x}", b));
            acc
        })
    }
}

/// A 64-bit span identifier.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpanId(pub [u8; 8]);

impl SpanId {
    /// Creates a new span ID using a sequential counter.
    pub fn new_random() -> Self {
        let counter = SPAN_COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&counter.to_be_bytes());
        SpanId(bytes)
    }

    /// Converts the span ID to a 16-character lowercase hex string.
    pub fn to_hex(&self) -> String {
        self.0.iter().fold(String::new(), |mut acc, b| {
            acc.push_str(&format!("{:02x}", b));
            acc
        })
    }
}

/// Trace context carrying trace and span identifiers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceContext {
    /// The trace ID.
    pub trace_id: TraceId,
    /// The span ID.
    pub span_id: SpanId,
    /// The parent span ID if this is a child span.
    pub parent_span_id: Option<SpanId>,
    /// Whether this trace is sampled.
    pub sampled: bool,
}

impl TraceContext {
    /// Creates a new root trace context.
    pub fn new_root() -> Self {
        TraceContext {
            trace_id: TraceId::new_random(),
            span_id: SpanId::new_random(),
            parent_span_id: None,
            sampled: true,
        }
    }

    /// Creates a child trace context with the same trace ID.
    pub fn child(&self) -> Self {
        TraceContext {
            trace_id: self.trace_id,
            span_id: SpanId::new_random(),
            parent_span_id: Some(self.span_id),
            sampled: self.sampled,
        }
    }

    /// Returns the W3C traceparent header format.
    pub fn traceparent(&self) -> String {
        let flags = if self.sampled { "01" } else { "00" };
        format!(
            "00-{}-{}-{}",
            self.trace_id.to_hex(),
            self.span_id.to_hex(),
            flags
        )
    }
}

/// A replication span representing a single replication operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplSpan {
    /// The trace context.
    pub context: TraceContext,
    /// The operation name.
    pub operation: String,
    /// The source site.
    pub site_from: String,
    /// The destination site.
    pub site_to: String,
    /// Start timestamp in nanoseconds.
    pub start_ns: u64,
    /// End timestamp in nanoseconds.
    pub end_ns: Option<u64>,
    /// Number of bytes transferred.
    pub bytes_transferred: u64,
    /// Error message if the operation failed.
    pub error: Option<String>,
}

impl ReplSpan {
    /// Creates a new replication span.
    pub fn new(
        ctx: TraceContext,
        operation: impl Into<String>,
        site_from: impl Into<String>,
        site_to: impl Into<String>,
        start_ns: u64,
    ) -> Self {
        ReplSpan {
            context: ctx,
            operation: operation.into(),
            site_from: site_from.into(),
            site_to: site_to.into(),
            start_ns,
            end_ns: None,
            bytes_transferred: 0,
            error: None,
        }
    }

    /// Marks the span as completed.
    pub fn complete(&mut self, end_ns: u64, bytes: u64) {
        self.end_ns = Some(end_ns);
        self.bytes_transferred = bytes;
        info!(
            operation = %self.operation,
            site_from = %self.site_from,
            site_to = %self.site_to,
            duration_ns = end_ns - self.start_ns,
            bytes = bytes,
            "replication span completed"
        );
    }

    /// Marks the span as failed with an error.
    pub fn fail(&mut self, end_ns: u64, error: impl Into<String>) {
        self.end_ns = Some(end_ns);
        self.error = Some(error.into());
        warn!(
            operation = %self.operation,
            site_from = %self.site_from,
            site_to = %self.site_to,
            error = %self.error.as_ref().unwrap(),
            "replication span failed"
        );
    }

    /// Returns the duration in nanoseconds if the span is complete.
    pub fn duration_ns(&self) -> Option<u64> {
        self.end_ns.map(|e| e - self.start_ns)
    }
}

/// Collector for managing replication spans.
#[derive(Debug)]
pub struct ReplSpanCollector {
    spans: Vec<ReplSpan>,
    max_spans: usize,
}

impl ReplSpanCollector {
    /// Creates a new span collector with the specified maximum capacity.
    pub fn new(max_spans: usize) -> Self {
        ReplSpanCollector {
            spans: Vec::new(),
            max_spans,
        }
    }

    /// Adds a span to the collector, evicting the oldest if at capacity.
    pub fn push(&mut self, span: ReplSpan) {
        self.spans.push(span);
        if self.spans.len() > self.max_spans {
            self.spans.remove(0);
        }
    }

    /// Returns all completed spans.
    pub fn completed_spans(&self) -> Vec<&ReplSpan> {
        self.spans.iter().filter(|s| s.end_ns.is_some()).collect()
    }

    /// Returns all failed spans.
    pub fn failed_spans(&self) -> Vec<&ReplSpan> {
        self.spans.iter().filter(|s| s.error.is_some()).collect()
    }

    /// Returns the number of spans in the collector.
    pub fn span_count(&self) -> usize {
        self.spans.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_id_new_random_not_zero() {
        let id = TraceId::new_random();
        assert_ne!(id.0, [0u8; 16]);
    }

    #[test]
    fn span_id_new_random_not_zero() {
        let id = SpanId::new_random();
        assert_ne!(id.0, [0u8; 8]);
    }

    #[test]
    fn trace_id_to_hex_length_32() {
        let id = TraceId::new_random();
        assert_eq!(id.to_hex().len(), 32);
    }

    #[test]
    fn span_id_to_hex_length_16() {
        let id = SpanId::new_random();
        assert_eq!(id.to_hex().len(), 16);
    }

    #[test]
    fn trace_context_new_root_no_parent() {
        let ctx = TraceContext::new_root();
        assert!(ctx.parent_span_id.is_none());
        assert!(ctx.sampled);
    }

    #[test]
    fn trace_context_child_same_trace_id() {
        let parent = TraceContext::new_root();
        let child = parent.child();
        assert_eq!(parent.trace_id, child.trace_id);
    }

    #[test]
    fn trace_context_child_new_span_id() {
        let parent = TraceContext::new_root();
        let child = parent.child();
        assert_ne!(parent.span_id, child.span_id);
    }

    #[test]
    fn trace_context_child_parent_is_original_span() {
        let parent = TraceContext::new_root();
        let child = parent.child();
        assert_eq!(child.parent_span_id, Some(parent.span_id));
    }

    #[test]
    fn traceparent_format_sampled() {
        let ctx = TraceContext::new_root();
        let tp = ctx.traceparent();
        assert!(tp.starts_with("00-"));
        assert!(tp.ends_with("-01"));
    }

    #[test]
    fn traceparent_format_unsampled() {
        let mut ctx = TraceContext::new_root();
        ctx.sampled = false;
        let tp = ctx.traceparent();
        assert!(tp.starts_with("00-"));
        assert!(tp.ends_with("-00"));
    }

    #[test]
    fn traceparent_length() {
        let ctx = TraceContext::new_root();
        let tp = ctx.traceparent();
        assert_eq!(tp.len(), 55);
    }

    #[test]
    fn repl_span_new_fields() {
        let ctx = TraceContext::new_root();
        let span = ReplSpan::new(ctx, "test_op", "site_a", "site_b", 1000);
        assert_eq!(span.operation, "test_op");
        assert_eq!(span.site_from, "site_a");
        assert_eq!(span.site_to, "site_b");
        assert_eq!(span.start_ns, 1000);
        assert!(span.end_ns.is_none());
        assert_eq!(span.bytes_transferred, 0);
        assert!(span.error.is_none());
    }

    #[test]
    fn repl_span_complete_sets_end_ns() {
        let ctx = TraceContext::new_root();
        let mut span = ReplSpan::new(ctx, "test_op", "site_a", "site_b", 1000);
        span.complete(2000, 500);
        assert_eq!(span.end_ns, Some(2000));
    }

    #[test]
    fn repl_span_complete_sets_bytes() {
        let ctx = TraceContext::new_root();
        let mut span = ReplSpan::new(ctx, "test_op", "site_a", "site_b", 1000);
        span.complete(2000, 500);
        assert_eq!(span.bytes_transferred, 500);
    }

    #[test]
    fn repl_span_fail_sets_error() {
        let ctx = TraceContext::new_root();
        let mut span = ReplSpan::new(ctx, "test_op", "site_a", "site_b", 1000);
        span.fail(2000, "something went wrong");
        assert_eq!(span.end_ns, Some(2000));
        assert_eq!(span.error, Some("something went wrong".to_string()));
    }

    #[test]
    fn repl_span_duration_ns_none_before_complete() {
        let ctx = TraceContext::new_root();
        let span = ReplSpan::new(ctx, "test_op", "site_a", "site_b", 1000);
        assert!(span.duration_ns().is_none());
    }

    #[test]
    fn repl_span_duration_ns_after_complete() {
        let ctx = TraceContext::new_root();
        let mut span = ReplSpan::new(ctx, "test_op", "site_a", "site_b", 1000);
        span.complete(2500, 500);
        assert_eq!(span.duration_ns(), Some(1500));
    }

    #[test]
    fn collector_push_and_count() {
        let mut collector = ReplSpanCollector::new(10);
        let ctx = TraceContext::new_root();
        collector.push(ReplSpan::new(ctx, "op", "a", "b", 0));
        assert_eq!(collector.span_count(), 1);
    }

    #[test]
    fn collector_evicts_oldest_when_full() {
        let mut collector = ReplSpanCollector::new(2);
        for i in 0..3 {
            let ctx = TraceContext::new_root();
            collector.push(ReplSpan::new(ctx, &format!("op{}", i), "a", "b", 0));
        }
        assert_eq!(collector.span_count(), 2);
    }

    #[test]
    fn collector_completed_spans() {
        let mut collector = ReplSpanCollector::new(10);
        let ctx = TraceContext::new_root();
        let mut span = ReplSpan::new(ctx, "op", "a", "b", 0);
        span.complete(100, 50);
        collector.push(span);
        let ctx2 = TraceContext::new_root();
        collector.push(ReplSpan::new(ctx2, "op2", "a", "b", 0));
        assert_eq!(collector.completed_spans().len(), 1);
    }

    #[test]
    fn collector_failed_spans() {
        let mut collector = ReplSpanCollector::new(10);
        let ctx = TraceContext::new_root();
        let mut span = ReplSpan::new(ctx, "op", "a", "b", 0);
        span.fail(100, "error");
        collector.push(span);
        let ctx2 = TraceContext::new_root();
        collector.push(ReplSpan::new(ctx2, "op2", "a", "b", 0));
        assert_eq!(collector.failed_spans().len(), 1);
    }

    #[test]
    fn span_serialize_roundtrip() {
        let ctx = TraceContext::new_root();
        let span = ReplSpan::new(ctx, "test_op", "site_a", "site_b", 1000);
        let serialized = bincode::serialize(&span).unwrap();
        let deserialized: ReplSpan = bincode::deserialize(&serialized).unwrap();
        assert_eq!(span.operation, deserialized.operation);
        assert_eq!(span.site_from, deserialized.site_from);
        assert_eq!(span.site_to, deserialized.site_to);
        assert_eq!(span.start_ns, deserialized.start_ns);
    }
}
