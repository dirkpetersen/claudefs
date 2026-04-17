//! OpenTelemetry tracing integration for FUSE client.
//!
//! Provides distributed tracing for FUSE operations with sampling,
//! span context propagation, and export capabilities.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Trace ID wrapper for distributed tracing correlation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TraceId(pub u128);

impl TraceId {
    /// Creates a new random trace ID.
    pub fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u128;
        let random = (now * 0x9e3779b97f4a7c15) ^ (now >> 17);
        TraceId(random.wrapping_add(0x123456789ABCDEF0))
    }

    /// Returns the trace ID as a 32-character hexadecimal string.
    pub fn as_hex(&self) -> String {
        format!("{:032x}", self.0)
    }

    /// Returns true if this is a zero trace ID.
    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }
}

impl Default for TraceId {
    fn default() -> Self {
        TraceId(0)
    }
}

/// Span ID wrapper for individual span identification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpanId(pub u64);

impl SpanId {
    /// Creates a new random span ID.
    pub fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        SpanId(now.wrapping_mul(0x1234567890ABCDEF))
    }

    /// Returns the span ID as a 16-character hexadecimal string.
    pub fn as_hex(&self) -> String {
        format!("{:016x}", self.0)
    }

    /// Returns true if this is a zero span ID.
    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }
}

impl Default for SpanId {
    fn default() -> Self {
        SpanId(0)
    }
}

/// FUSE operation types for tracing classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FuseOp {
    /// Directory lookup (path -> inode)
    Lookup,
    /// Get file attributes (stat)
    GetAttr,
    /// Set file attributes (chmod, chown, utimes)
    SetAttr,
    /// Read file data
    Read,
    /// Write file data
    Write,
    /// Create new file
    Create,
    /// Create directory
    Mkdir,
    /// Remove file
    Unlink,
    /// Rename/move file
    Rename,
    /// Create hard link
    Link,
    /// Other operations
    Other(&'static str),
}

impl FuseOp {
    /// Returns a debug-friendly string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            FuseOp::Lookup => "lookup",
            FuseOp::GetAttr => "getattr",
            FuseOp::SetAttr => "setattr",
            FuseOp::Read => "read",
            FuseOp::Write => "write",
            FuseOp::Create => "create",
            FuseOp::Mkdir => "mkdir",
            FuseOp::Unlink => "unlink",
            FuseOp::Rename => "rename",
            FuseOp::Link => "link",
            FuseOp::Other(s) => *s,
        }
    }
}

/// Span completion status.
#[derive(Debug, Clone)]
pub enum SpanStatus {
    /// Operation completed successfully.
    Success,
    /// Operation failed with error message.
    Error(String),
    /// Operation was throttled/delayed.
    Throttled,
}

/// FUSE operation span context for tracking in-flight spans.
#[derive(Debug, Clone)]
pub struct FuseSpanContext {
    /// Trace identifier for correlation.
    pub trace_id: TraceId,
    /// Span identifier for this specific operation.
    pub span_id: SpanId,
    /// Parent span ID for hierarchy.
    pub parent_span_id: Option<SpanId>,
    /// The FUSE operation type.
    pub operation: FuseOp,
    /// Target inode for the operation.
    pub inode: u64,
    /// Start timestamp in nanoseconds since epoch.
    pub start_ns: u64,
}

impl FuseSpanContext {
    /// Creates a new span context with a generated trace and span ID.
    pub fn new(op: FuseOp, inode: u64) -> Self {
        let start_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        Self {
            trace_id: TraceId::new(),
            span_id: SpanId::new(),
            parent_span_id: None,
            operation: op,
            inode,
            start_ns,
        }
    }

    /// Creates a child span context with parent reference.
    pub fn child(op: FuseOp, inode: u64, parent: &FuseSpanContext) -> Self {
        let start_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        Self {
            trace_id: parent.trace_id,
            span_id: SpanId::new(),
            parent_span_id: Some(parent.span_id),
            operation: op,
            inode,
            start_ns,
        }
    }
}

/// Completed span ready for export.
#[derive(Debug, Clone)]
pub struct CompletedSpan {
    /// Trace identifier.
    pub trace_id: TraceId,
    /// Span identifier.
    pub span_id: SpanId,
    /// Parent span identifier.
    pub parent_span_id: Option<SpanId>,
    /// Operation type.
    pub operation: FuseOp,
    /// Start timestamp.
    pub start_ns: u64,
    /// End timestamp.
    pub end_ns: u64,
    /// Completion status.
    pub status: SpanStatus,
    /// Additional attributes for filtering/aggregation.
    pub attributes: HashMap<String, String>,
}

impl CompletedSpan {
    /// Returns the elapsed time in nanoseconds.
    pub fn elapsed_ns(&self) -> u64 {
        self.end_ns.saturating_sub(self.start_ns)
    }

    /// Returns true if the span represents an error.
    pub fn is_error(&self) -> bool {
        matches!(self.status, SpanStatus::Error(_))
    }
}

/// OpenTelemetry exporter function type.
pub type OtelExportFn = Arc<dyn Fn(&CompletedSpan) -> Result<(), String> + Send + Sync>;

/// No-op exporter for testing or disabled scenarios.
pub struct NoopExporter;

impl NoopExporter {
    /// Creates a no-op exporter function.
    pub fn to_export_fn() -> OtelExportFn {
        Arc::new(|_span| Ok(()))
    }
}

/// Global FUSE tracer with sampling and export capabilities.
pub struct FuseTracer {
    exporter: OtelExportFn,
    enabled: bool,
    sampling_rate: f32,
    active_spans: AtomicU64,
    exported_spans: AtomicU64,
}

impl FuseTracer {
    /// Creates a new tracer with the given exporter and sampling rate.
    pub fn new(exporter: OtelExportFn, sampling_rate: f32) -> Self {
        Self {
            exporter,
            enabled: true,
            sampling_rate: sampling_rate.clamp(0.0, 1.0),
            active_spans: AtomicU64::new(0),
            exported_spans: AtomicU64::new(0),
        }
    }

    /// Creates a new disabled tracer.
    pub fn disabled() -> Self {
        Self {
            exporter: NoopExporter::to_export_fn(),
            enabled: false,
            sampling_rate: 0.0,
            active_spans: AtomicU64::new(0),
            exported_spans: AtomicU64::new(0),
        }
    }

    /// Returns the current number of active spans.
    pub fn active_span_count(&self) -> u64 {
        self.active_spans.load(Ordering::SeqCst)
    }

    /// Returns the total number of exported spans.
    pub fn exported_span_count(&self) -> u64 {
        self.exported_spans.load(Ordering::SeqCst)
    }

    /// Returns whether tracing is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Returns the sampling rate.
    pub fn sampling_rate(&self) -> f32 {
        self.sampling_rate
    }

    /// Determines if a span should be sampled based on trace ID.
    fn should_sample(&self, trace_id: TraceId) -> bool {
        if !self.enabled {
            return false;
        }
        if self.sampling_rate >= 1.0 {
            return true;
        }
        if self.sampling_rate <= 0.0 {
            return false;
        }
        let TraceId(id) = trace_id;
        let hash = id.wrapping_mul(0x9e3779b97f4a7c15);
        let normalized = (hash % 10000) as f32 / 10000.0;
        normalized < self.sampling_rate
    }

    /// Starts a new span for a FUSE operation.
    pub fn start_span(&self, op: FuseOp, inode: u64) -> Option<FuseSpanContext> {
        if !self.should_sample(TraceId::new()) {
            return None;
        }
        let ctx = FuseSpanContext::new(op, inode);
        self.active_spans.fetch_add(1, Ordering::SeqCst);
        Some(ctx)
    }

    /// Finishes a span and exports it.
    pub fn finish_span(&self, ctx: FuseSpanContext, status: SpanStatus) {
        let end_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        let mut attributes = HashMap::new();
        attributes.insert("inode".to_string(), ctx.inode.to_string());
        attributes.insert("operation".to_string(), ctx.operation.as_str().to_string());

        let span = CompletedSpan {
            trace_id: ctx.trace_id,
            span_id: ctx.span_id,
            parent_span_id: ctx.parent_span_id,
            operation: ctx.operation,
            start_ns: ctx.start_ns,
            end_ns,
            status,
            attributes,
        };

        let _ = (self.exporter)(&span);
        self.active_spans.fetch_sub(1, Ordering::SeqCst);
        self.exported_spans.fetch_add(1, Ordering::SeqCst);
    }

    /// Injects trace context into RPC headers for cross-service propagation.
    pub fn inject_context(&self, ctx: &FuseSpanContext) -> HashMap<String, String> {
        let mut headers = HashMap::new();
        headers.insert(
            "traceparent".to_string(),
            format!("00-{}-{}-01", ctx.trace_id.as_hex(), ctx.span_id.as_hex()),
        );
        headers
    }

    /// Extracts trace context from RPC headers.
    pub fn extract_context(&self, headers: &HashMap<String, String>) -> Option<TraceId> {
        let traceparent = headers.get("traceparent")?;
        let parts: Vec<&str> = traceparent.split('-').collect();
        if parts.len() != 4 {
            return None;
        }
        let trace_hex = parts.get(1)?;
        if trace_hex.len() != 32 {
            return None;
        }
        let id = u128::from_str_radix(trace_hex, 16).ok()?;
        Some(TraceId(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_id_generation_unique() {
        let id1 = TraceId::new();
        let id2 = TraceId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_trace_id_hex_format() {
        let id = TraceId(0x123456789ABCDEF0123456789ABCDEF0);
        assert_eq!(id.as_hex().len(), 32);
    }

    #[test]
    fn test_span_id_generation() {
        let id1 = SpanId::new();
        let id2 = SpanId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_span_id_hex_format() {
        let id = SpanId(0x123456789ABCDEF0);
        assert_eq!(id.as_hex().len(), 16);
    }

    #[test]
    fn test_sampling_rate_zero_disables() {
        let tracer = FuseTracer::new(NoopExporter::to_export_fn(), 0.0);
        let span = tracer.start_span(FuseOp::Read, 1);
        assert!(span.is_none());
    }

    #[test]
    fn test_sampling_rate_one_enables_all() {
        let tracer = FuseTracer::new(NoopExporter::to_export_fn(), 1.0);
        let span = tracer.start_span(FuseOp::Read, 1);
        assert!(span.is_some());
    }

    #[test]
    fn test_disabled_tracer_noop() {
        let tracer = FuseTracer::disabled();
        assert!(!tracer.is_enabled());
        let span = tracer.start_span(FuseOp::Write, 1);
        assert!(span.is_none());
    }

    #[test]
    fn test_fuse_op_as_str() {
        assert_eq!(FuseOp::Read.as_str(), "read");
        assert_eq!(FuseOp::Lookup.as_str(), "lookup");
        assert_eq!(FuseOp::Other("custom").as_str(), "custom");
    }

    #[test]
    fn test_fuse_span_context_creation() {
        let ctx = FuseSpanContext::new(FuseOp::Write, 42);
        assert_eq!(ctx.inode, 42);
        assert!(!ctx.trace_id.is_zero());
        assert!(!ctx.span_id.is_zero());
        assert!(ctx.parent_span_id.is_none());
    }

    #[test]
    fn test_fuse_span_context_child() {
        let parent = FuseSpanContext::new(FuseOp::Read, 10);
        let child = FuseSpanContext::child(FuseOp::Write, 20, &parent);
        assert_eq!(child.trace_id, parent.trace_id);
        assert_eq!(child.parent_span_id, Some(parent.span_id));
    }

    #[test]
    fn test_completed_span_elapsed_time() {
        let span = CompletedSpan {
            trace_id: TraceId(1),
            span_id: SpanId(1),
            parent_span_id: None,
            operation: FuseOp::Read,
            start_ns: 1000,
            end_ns: 2000,
            status: SpanStatus::Success,
            attributes: HashMap::new(),
        };
        assert_eq!(span.elapsed_ns(), 1000);
    }

    #[test]
    fn test_completed_span_is_error() {
        let span_ok = CompletedSpan {
            trace_id: TraceId(1),
            span_id: SpanId(1),
            parent_span_id: None,
            operation: FuseOp::Read,
            start_ns: 1000,
            end_ns: 2000,
            status: SpanStatus::Success,
            attributes: HashMap::new(),
        };
        assert!(!span_ok.is_error());

        let span_err = CompletedSpan {
            trace_id: TraceId(1),
            span_id: SpanId(1),
            parent_span_id: None,
            operation: FuseOp::Read,
            start_ns: 1000,
            end_ns: 2000,
            status: SpanStatus::Error("test error".to_string()),
            attributes: HashMap::new(),
        };
        assert!(span_err.is_error());
    }

    #[test]
    fn test_inject_context_creates_headers() {
        let tracer = FuseTracer::new(NoopExporter::to_export_fn(), 1.0);
        let ctx = tracer.start_span(FuseOp::Read, 1).unwrap();
        let headers = tracer.inject_context(&ctx);
        assert!(headers.contains_key("traceparent"));
        assert!(headers.get("traceparent").unwrap().starts_with("00-"));
    }

    #[test]
    fn test_extract_context_from_headers() {
        let tracer = FuseTracer::new(NoopExporter::to_export_fn(), 1.0);
        let mut headers = HashMap::new();
        headers.insert(
            "traceparent".to_string(),
            "00-00000000000000000000000000000001-0000000000000002-01".to_string(),
        );
        let trace_id = tracer.extract_context(&headers);
        assert!(trace_id.is_some());
    }

    #[test]
    fn test_empty_headers_returns_none() {
        let tracer = FuseTracer::new(NoopExporter::to_export_fn(), 1.0);
        let headers = HashMap::new();
        let trace_id = tracer.extract_context(&headers);
        assert!(trace_id.is_none());
    }

    #[test]
    fn test_invalid_traceparent_returns_none() {
        let tracer = FuseTracer::new(NoopExporter::to_export_fn(), 1.0);
        let mut headers = HashMap::new();
        headers.insert("traceparent".to_string(), "invalid".to_string());
        let trace_id = tracer.extract_context(&headers);
        assert!(trace_id.is_none());
    }

    #[test]
    fn test_span_status_error_includes_message() {
        let status = SpanStatus::Error("permission denied".to_string());
        match status {
            SpanStatus::Error(msg) => assert_eq!(msg, "permission denied"),
            _ => panic!("expected error status"),
        }
    }

    #[test]
    fn test_finish_span_with_success() {
        let exporter = NoopExporter::to_export_fn();
        let tracer = FuseTracer::new(exporter, 1.0);
        let ctx = tracer.start_span(FuseOp::Read, 1).unwrap();
        tracer.finish_span(ctx, SpanStatus::Success);
        assert_eq!(tracer.active_span_count(), 0);
    }

    #[test]
    fn test_finish_span_with_error() {
        let exporter = NoopExporter::to_export_fn();
        let tracer = FuseTracer::new(exporter, 1.0);
        let ctx = tracer.start_span(FuseOp::Write, 1).unwrap();
        tracer.finish_span(ctx, SpanStatus::Error("IO error".to_string()));
        assert_eq!(tracer.active_span_count(), 0);
    }

    #[test]
    fn test_finish_span_with_throttled() {
        let exporter = NoopExporter::to_export_fn();
        let tracer = FuseTracer::new(exporter, 1.0);
        let ctx = tracer.start_span(FuseOp::Read, 1).unwrap();
        tracer.finish_span(ctx, SpanStatus::Throttled);
        assert_eq!(tracer.active_span_count(), 0);
    }

    #[test]
    fn test_multiple_concurrent_spans() {
        let exporter = NoopExporter::to_export_fn();
        let tracer = FuseTracer::new(exporter, 1.0);

        let ctx1 = tracer.start_span(FuseOp::Read, 1).unwrap();
        let ctx2 = tracer.start_span(FuseOp::Write, 2).unwrap();

        assert_eq!(tracer.active_span_count(), 2);

        tracer.finish_span(ctx1, SpanStatus::Success);
        assert_eq!(tracer.active_span_count(), 1);

        tracer.finish_span(ctx2, SpanStatus::Success);
        assert_eq!(tracer.active_span_count(), 0);
    }

    #[test]
    fn test_sampling_probabilistic_distribution() {
        let tracer = FuseTracer::new(NoopExporter::to_export_fn(), 0.5);
        let mut sampled = 0;
        for _ in 0..1000 {
            if tracer.start_span(FuseOp::Read, 1).is_some() {
                sampled += 1;
            }
        }
        assert!(sampled > 300, "expected ~50% samples, got {}", sampled);
        assert!(sampled < 700, "expected ~50% samples, got {}", sampled);
    }

    #[test]
    fn test_fuse_op_debug_format() {
        let op = FuseOp::Read;
        let debug_str = format!("{:?}", op);
        assert!(debug_str.contains("Read"));
    }

    #[test]
    fn test_trace_id_default_is_zero() {
        let id = TraceId::default();
        assert!(id.is_zero());
    }

    #[test]
    fn test_span_id_default_is_zero() {
        let id = SpanId::default();
        assert!(id.is_zero());
    }
}
