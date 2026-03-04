//! Distributed tracing support for FUSE operations.
//!
//! This module provides W3C TraceContext-compatible tracing for FUSE file system
//! operations, enabling distributed tracing across the ClaudeFS cluster. It
//! supports span hierarchy, sampling, and integration with external tracing
//! backends via the `traceparent` header format.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// A 128-bit trace identifier, unique across the entire distributed trace.
///
/// Trace IDs identify the overall trace and are shared by all spans within
/// that trace. They are typically generated at the trace root and propagated
/// to all child spans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceId(pub u128);

impl TraceId {
    /// Creates a new trace ID from a raw 128-bit value.
    pub fn new(id: u128) -> Self {
        TraceId(id)
    }

    /// Returns the trace ID as a 32-character lowercase hexadecimal string.
    pub fn as_hex(&self) -> String {
        format!("{:032x}", self.0)
    }

    /// Returns the zero trace ID (all bits set to 0).
    ///
    /// The zero trace ID represents an invalid or absent trace.
    pub fn zero() -> Self {
        TraceId(0)
    }

    /// Returns `true` if this is the zero trace ID.
    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }
}

/// A 64-bit span identifier, unique within a trace.
///
/// Span IDs identify individual operations within a trace. Each span has a
/// unique ID and may reference a parent span ID to form a hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpanId(pub u64);

impl SpanId {
    /// Creates a new span ID from a raw 64-bit value.
    pub fn new(id: u64) -> Self {
        SpanId(id)
    }

    /// Returns the span ID as a 16-character lowercase hexadecimal string.
    pub fn as_hex(&self) -> String {
        format!("{:016x}", self.0)
    }

    /// Returns the zero span ID (all bits set to 0).
    ///
    /// The zero span ID represents an invalid or absent span.
    pub fn zero() -> Self {
        SpanId(0)
    }

    /// Returns `true` if this is the zero span ID.
    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }
}

/// W3C TraceContext for distributed tracing propagation.
///
/// Encapsulates the trace identity and sampling decision for a span, enabling
/// propagation across process boundaries via the `traceparent` header format.
#[derive(Debug, Clone)]
pub struct TraceContext {
    /// The trace ID, shared by all spans in this trace.
    pub trace_id: TraceId,
    /// The span ID for this specific span.
    pub span_id: SpanId,
    /// Whether this trace is sampled (should be recorded).
    pub sampled: bool,
}

impl TraceContext {
    /// Creates a new trace context with the given identifiers and sampling flag.
    pub fn new(trace_id: TraceId, span_id: SpanId, sampled: bool) -> Self {
        TraceContext {
            trace_id,
            span_id,
            sampled,
        }
    }

    /// Serializes this context to a W3C `traceparent` header string.
    ///
    /// Format: `00-{trace-id}-{span-id}-{flags}` where flags is `01` if sampled
    /// or `00` otherwise.
    pub fn to_traceparent(&self) -> String {
        let flags = if self.sampled { "01" } else { "00" };
        format!(
            "00-{}-{}-{}",
            self.trace_id.as_hex(),
            self.span_id.as_hex(),
            flags
        )
    }

    /// Parses a W3C `traceparent` header string into a trace context.
    ///
    /// Returns `None` if the string is malformed or uses an unsupported version.
    pub fn from_traceparent(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 4 {
            return None;
        }
        if parts[0] != "00" {
            return None;
        }
        let trace_hex = parts[1];
        let span_hex = parts[2];
        let flags = parts[3];

        if trace_hex.len() != 32 || span_hex.len() != 16 {
            return None;
        }

        let trace_id = u128::from_str_radix(trace_hex, 16).ok()?;
        let span_id = u64::from_str_radix(span_hex, 16).ok()?;
        let sampled = flags == "01";

        Some(TraceContext {
            trace_id: TraceId(trace_id),
            span_id: SpanId(span_id),
            sampled,
        })
    }

    /// Creates a root trace context with sampling enabled.
    pub fn root(trace_id: TraceId, span_id: SpanId) -> Self {
        TraceContext {
            trace_id,
            span_id,
            sampled: true,
        }
    }
}

/// A single span representing a FUSE operation.
///
/// Tracks the operation name, timing, and optional error information for
/// a single unit of work within a distributed trace.
#[derive(Debug)]
pub struct FuseSpan {
    /// The FUSE operation name (e.g., "read", "write", "lookup").
    pub op: String,
    /// The trace ID this span belongs to.
    pub trace_id: TraceId,
    /// The unique identifier for this span.
    pub span_id: SpanId,
    /// The parent span ID, if this is a child span.
    pub parent_span_id: Option<SpanId>,
    /// The instant when this span was created.
    pub started_at: Instant,
    /// Whether this span has been finished.
    pub finished: bool,
    /// Optional error message if the operation failed.
    pub error: Option<String>,
}

impl FuseSpan {
    /// Creates a new root span for the given operation.
    pub fn new(op: &str, trace_id: TraceId, span_id: SpanId) -> Self {
        FuseSpan {
            op: op.to_string(),
            trace_id,
            span_id,
            parent_span_id: None,
            started_at: Instant::now(),
            finished: false,
            error: None,
        }
    }

    /// Creates a child span with the given parent context.
    pub fn with_parent(op: &str, ctx: &TraceContext, child_span_id: SpanId) -> Self {
        FuseSpan {
            op: op.to_string(),
            trace_id: ctx.trace_id,
            span_id: child_span_id,
            parent_span_id: Some(ctx.span_id),
            started_at: Instant::now(),
            finished: false,
            error: None,
        }
    }

    /// Marks this span as finished.
    pub fn finish(&mut self) {
        self.finished = true;
    }

    /// Marks this span as finished with an error message.
    pub fn finish_with_error(&mut self, err: &str) {
        self.finished = true;
        self.error = Some(err.to_string());
    }

    /// Returns the elapsed time in microseconds since this span was created.
    pub fn elapsed_us(&self) -> u64 {
        self.started_at.elapsed().as_micros() as u64
    }
}

/// Configuration for the FUSE tracing subsystem.
#[derive(Debug, Clone)]
pub struct TracingConfig {
    /// Whether tracing is enabled.
    pub enabled: bool,
    /// Sampling rate: 1 means record every span, 2 means every other, etc.
    pub sample_rate: u32,
    /// Maximum number of concurrently active spans before dropping new ones.
    pub max_active_spans: usize,
}

impl Default for TracingConfig {
    fn default() -> Self {
        TracingConfig {
            enabled: true,
            sample_rate: 1,
            max_active_spans: 1024,
        }
    }
}

/// The FUSE tracer, managing span lifecycle and sampling.
///
/// Thread-safe tracer that coordinates span creation, sampling, and metrics
/// collection for FUSE operations.
pub struct FuseTracer {
    config: TracingConfig,
    span_counter: AtomicU64,
    active_count: AtomicU64,
    dropped_count: AtomicU64,
    total_count: AtomicU64,
}

impl FuseTracer {
    /// Creates a new tracer with the given configuration.
    pub fn new(config: TracingConfig) -> Self {
        FuseTracer {
            config,
            span_counter: AtomicU64::new(0),
            active_count: AtomicU64::new(0),
            dropped_count: AtomicU64::new(0),
            total_count: AtomicU64::new(0),
        }
    }

    /// Starts a new root span for the given operation.
    ///
    /// Returns `None` if tracing is disabled, sampling rejects the span,
    /// or the maximum active span limit has been reached.
    pub fn start_span(&self, op: &str, trace_id: TraceId) -> Option<FuseSpan> {
        if !self.config.enabled {
            return None;
        }

        let counter = self.span_counter.fetch_add(1, Ordering::SeqCst);

        if self.config.sample_rate > 0 && !counter.is_multiple_of(self.config.sample_rate as u64) {
            return None;
        }

        let active = self.active_count.load(Ordering::SeqCst);
        if active >= self.config.max_active_spans as u64 {
            self.dropped_count.fetch_add(1, Ordering::SeqCst);
            return None;
        }

        self.active_count.fetch_add(1, Ordering::SeqCst);
        self.total_count.fetch_add(1, Ordering::SeqCst);

        let span_id = SpanId::new(counter);
        Some(FuseSpan::new(op, trace_id, span_id))
    }

    /// Creates a child span from the given parent context.
    ///
    /// Returns `None` if tracing is disabled or the maximum active span
    /// limit has been reached.
    pub fn child_span(&self, op: &str, parent: &TraceContext) -> Option<FuseSpan> {
        if !self.config.enabled {
            return None;
        }

        let counter = self.span_counter.fetch_add(1, Ordering::SeqCst);

        let active = self.active_count.load(Ordering::SeqCst);
        if active >= self.config.max_active_spans as u64 {
            self.dropped_count.fetch_add(1, Ordering::SeqCst);
            return None;
        }

        self.active_count.fetch_add(1, Ordering::SeqCst);
        self.total_count.fetch_add(1, Ordering::SeqCst);

        let child_span_id = SpanId::new(counter);
        Some(FuseSpan::with_parent(op, parent, child_span_id))
    }

    /// Finishes a span and decrements the active span count.
    pub fn finish_span(&self, span: &mut FuseSpan) {
        span.finish();
        self.active_count.fetch_sub(1, Ordering::SeqCst);
    }

    /// Finishes a span with an error and decrements the active span count.
    pub fn finish_span_error(&self, span: &mut FuseSpan, err: &str) {
        span.finish_with_error(err);
        self.active_count.fetch_sub(1, Ordering::SeqCst);
    }

    /// Returns the current number of active (unfinished) spans.
    pub fn active_spans(&self) -> u64 {
        self.active_count.load(Ordering::SeqCst)
    }

    /// Returns the total number of spans dropped due to the active limit.
    pub fn dropped_spans(&self) -> u64 {
        self.dropped_count.load(Ordering::SeqCst)
    }

    /// Returns the total number of spans ever created by this tracer.
    pub fn total_spans(&self) -> u64 {
        self.total_count.load(Ordering::SeqCst)
    }

    /// Returns whether tracing is enabled for this tracer.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_trace_id_as_hex_is_32_chars() {
        let id = TraceId::new(0x123456789abcdef0123456789abcdef);
        let hex = id.as_hex();
        assert_eq!(hex.len(), 32);
    }

    #[test]
    fn test_span_id_as_hex_is_16_chars() {
        let id = SpanId::new(0x123456789abcdef0);
        let hex = id.as_hex();
        assert_eq!(hex.len(), 16);
    }

    #[test]
    fn test_trace_id_zero_is_all_zeros() {
        let id = TraceId::zero();
        assert_eq!(id.as_hex(), "00000000000000000000000000000000");
    }

    #[test]
    fn test_span_id_zero_is_all_zeros() {
        let id = SpanId::zero();
        assert_eq!(id.as_hex(), "0000000000000000");
    }

    #[test]
    fn test_trace_id_is_zero_returns_true_for_zero() {
        assert!(TraceId::zero().is_zero());
        assert!(!TraceId::new(1).is_zero());
    }

    #[test]
    fn test_span_id_is_zero_returns_true_for_zero() {
        assert!(SpanId::zero().is_zero());
        assert!(!SpanId::new(1).is_zero());
    }

    #[test]
    fn test_traceparent_format_is_correct() {
        let ctx = TraceContext::new(TraceId::new(1), SpanId::new(2), true);
        let parent = ctx.to_traceparent();
        assert!(parent.starts_with("00-"));
        assert_eq!(
            parent,
            "00-00000000000000000000000000000001-0000000000000002-01"
        );
    }

    #[test]
    fn test_traceparent_parse_roundtrip() {
        let ctx = TraceContext::new(TraceId::new(0xabc), SpanId::new(0xdef), true);
        let parsed = TraceContext::from_traceparent(&ctx.to_traceparent()).unwrap();
        assert_eq!(parsed.trace_id.0, ctx.trace_id.0);
        assert_eq!(parsed.span_id.0, ctx.span_id.0);
        assert_eq!(parsed.sampled, ctx.sampled);
    }

    #[test]
    fn test_traceparent_parse_invalid_returns_none() {
        assert!(TraceContext::from_traceparent("invalid").is_none());
        assert!(TraceContext::from_traceparent("01-aaaa-bb-01").is_none());
        assert!(TraceContext::from_traceparent("00-short-long-01").is_none());
    }

    #[test]
    fn test_trace_context_root_is_sampled() {
        let ctx = TraceContext::root(TraceId::new(1), SpanId::new(2));
        assert!(ctx.sampled);
    }

    #[test]
    fn test_fuse_span_new_is_not_finished() {
        let span = FuseSpan::new("read", TraceId::new(1), SpanId::new(2));
        assert!(!span.finished);
    }

    #[test]
    fn test_fuse_span_finish_sets_finished() {
        let mut span = FuseSpan::new("read", TraceId::new(1), SpanId::new(2));
        span.finish();
        assert!(span.finished);
    }

    #[test]
    fn test_fuse_span_finish_with_error_sets_error() {
        let mut span = FuseSpan::new("read", TraceId::new(1), SpanId::new(2));
        span.finish_with_error("failed");
        assert!(span.finished);
        assert_eq!(span.error, Some("failed".to_string()));
    }

    #[test]
    fn test_fuse_span_elapsed_us_increases_over_time() {
        let span = FuseSpan::new("read", TraceId::new(1), SpanId::new(2));
        thread::sleep(Duration::from_millis(1));
        assert!(span.elapsed_us() >= 1000);
    }

    #[test]
    fn test_fuse_span_with_parent_sets_parent_span_id() {
        let ctx = TraceContext::new(TraceId::new(1), SpanId::new(2), true);
        let span = FuseSpan::with_parent("child", &ctx, SpanId::new(3));
        assert_eq!(span.parent_span_id, Some(SpanId::new(2)));
    }

    #[test]
    fn test_tracer_new_starts_with_zero_counts() {
        let tracer = FuseTracer::new(TracingConfig::default());
        assert_eq!(tracer.active_spans(), 0);
        assert_eq!(tracer.dropped_spans(), 0);
        assert_eq!(tracer.total_spans(), 0);
    }

    #[test]
    fn test_tracer_start_span_when_disabled_returns_none() {
        let config = TracingConfig {
            enabled: false,
            ..Default::default()
        };
        let tracer = FuseTracer::new(config);
        let span = tracer.start_span("read", TraceId::new(1));
        assert!(span.is_none());
    }

    #[test]
    fn test_tracer_start_span_returns_some_when_enabled() {
        let tracer = FuseTracer::new(TracingConfig::default());
        let span = tracer.start_span("read", TraceId::new(1));
        assert!(span.is_some());
    }

    #[test]
    fn test_tracer_active_spans_increments_on_start() {
        let tracer = FuseTracer::new(TracingConfig::default());
        tracer.start_span("read", TraceId::new(1));
        assert_eq!(tracer.active_spans(), 1);
    }

    #[test]
    fn test_tracer_active_spans_decrements_on_finish() {
        let tracer = FuseTracer::new(TracingConfig::default());
        let mut span = tracer.start_span("read", TraceId::new(1)).unwrap();
        tracer.finish_span(&mut span);
        assert_eq!(tracer.active_spans(), 0);
    }

    #[test]
    fn test_tracer_total_spans_increments() {
        let tracer = FuseTracer::new(TracingConfig::default());
        tracer.start_span("read", TraceId::new(1));
        assert_eq!(tracer.total_spans(), 1);
    }

    #[test]
    fn test_tracer_sample_rate_2_accepts_every_other() {
        let config = TracingConfig {
            sample_rate: 2,
            ..Default::default()
        };
        let tracer = FuseTracer::new(config);
        let s1 = tracer.start_span("read", TraceId::new(1));
        let s2 = tracer.start_span("read", TraceId::new(1));
        assert!(s1.is_some());
        assert!(s2.is_none());
    }

    #[test]
    fn test_tracer_max_active_spans_drops_when_exceeded() {
        let config = TracingConfig {
            max_active_spans: 1,
            ..Default::default()
        };
        let tracer = FuseTracer::new(config);
        let _s1 = tracer.start_span("read", TraceId::new(1));
        let s2 = tracer.start_span("read", TraceId::new(1));
        assert!(s2.is_none());
        assert_eq!(tracer.dropped_spans(), 1);
    }

    #[test]
    fn test_tracer_dropped_count_increments_on_drop() {
        let config = TracingConfig {
            max_active_spans: 0,
            ..Default::default()
        };
        let tracer = FuseTracer::new(config);
        tracer.start_span("read", TraceId::new(1));
        assert_eq!(tracer.dropped_spans(), 1);
    }

    #[test]
    fn test_child_span_inherits_trace_id() {
        let tracer = FuseTracer::new(TracingConfig::default());
        let ctx = TraceContext::new(TraceId::new(42), SpanId::new(1), true);
        let span = tracer.child_span("child", &ctx).unwrap();
        assert_eq!(span.trace_id.0, 42);
        assert_eq!(span.parent_span_id, Some(SpanId::new(1)));
    }
}
