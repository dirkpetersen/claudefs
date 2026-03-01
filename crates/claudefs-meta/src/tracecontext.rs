//! Distributed tracing context for metadata operations.
//!
//! Propagates trace context (trace ID, span ID) through metadata
//! operations for end-to-end latency attribution. Compatible with
//! OpenTelemetry W3C Trace Context format. Spans are collected locally
//! and exported via the metrics/management layer.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use crate::types::*;

/// A 128-bit trace identifier for distributed tracing.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TraceId {
    /// High 64 bits of the trace ID.
    pub high: u64,
    /// Low 64 bits of the trace ID.
    pub low: u64,
}

impl TraceId {
    /// Creates a trace ID from high and low 64-bit values.
    pub fn new(high: u64, low: u64) -> Self {
        Self { high, low }
    }

    /// Generates a random trace ID.
    pub fn random() -> Self {
        Self {
            high: rand_u64(),
            low: rand_u64(),
        }
    }
}

/// A 64-bit span identifier within a trace.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpanId(pub u64);

impl SpanId {
    /// Creates a span ID from a raw 64-bit value.
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// Returns the raw 64-bit value of this span ID.
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

/// Carries trace context through a request chain.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TraceContext {
    /// The trace this span belongs to.
    pub trace_id: TraceId,
    /// The current span within the trace.
    pub span_id: SpanId,
    /// The parent span ID, if this is a child span.
    pub parent_span_id: Option<SpanId>,
    /// Whether this trace is being sampled.
    pub sampled: bool,
}

/// A recorded span with timing and metadata.
#[derive(Clone, Debug)]
pub struct SpanRecord {
    /// Name of the span (e.g., "metadata_lookup").
    pub name: String,
    /// Trace context for this span.
    pub trace_ctx: TraceContext,
    /// When the span started.
    pub start_time: Timestamp,
    /// When the span ended, if finished.
    pub end_time: Option<Timestamp>,
    /// Key-value attributes attached to the span.
    pub attributes: Vec<(String, String)>,
    /// Final status of the span.
    pub status: SpanStatus,
}

/// Status of a traced operation.
#[derive(Clone, Debug)]
pub enum SpanStatus {
    /// Operation completed successfully.
    Ok,
    /// Operation failed with an error.
    Error {
        /// Error message describing the failure.
        message: String,
    },
}

fn rand_u64() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let mut hash: u64 = nanos as u64;
    hash = hash
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    hash
}

/// Collects and manages trace spans for distributed tracing.
pub struct TraceCollector {
    completed_spans: RwLock<Vec<SpanRecord>>,
    span_id_generator: AtomicU64,
    max_spans: usize,
}

impl TraceCollector {
    /// Creates a new trace collector with the specified maximum span buffer size.
    pub fn new(max_spans: usize) -> Self {
        Self {
            completed_spans: RwLock::new(Vec::new()),
            span_id_generator: AtomicU64::new(1),
            max_spans,
        }
    }

    /// Creates a new trace with a randomly generated trace ID and span ID.
    pub fn new_trace(&self) -> TraceContext {
        let trace_id = TraceId::random();
        let span_id = SpanId(self.span_id_generator.fetch_add(1, Ordering::Relaxed));
        TraceContext {
            trace_id,
            span_id,
            parent_span_id: None,
            sampled: true,
        }
    }

    /// Creates a child span context from a parent context.
    pub fn child_span(&self, parent: &TraceContext) -> TraceContext {
        let span_id = SpanId(self.span_id_generator.fetch_add(1, Ordering::Relaxed));
        TraceContext {
            trace_id: parent.trace_id,
            span_id,
            parent_span_id: Some(parent.span_id),
            sampled: parent.sampled,
        }
    }

    /// Starts a new span with the given name and context.
    pub fn start_span(&self, name: String, ctx: &TraceContext) -> SpanRecord {
        SpanRecord {
            name,
            trace_ctx: ctx.clone(),
            start_time: Timestamp::now(),
            end_time: None,
            attributes: Vec::new(),
            status: SpanStatus::Ok,
        }
    }

    /// Ends a span by setting its end time to the current timestamp.
    pub fn end_span(&self, span: &mut SpanRecord) {
        span.end_time = Some(Timestamp::now());
    }

    /// Records a completed span to the buffer.
    pub fn record_span(&self, span: SpanRecord) {
        let mut spans = self.completed_spans.write().unwrap();
        if spans.len() >= self.max_spans {
            spans.remove(0);
        }
        spans.push(span);
        tracing::debug!("Recorded span, total completed: {}", spans.len());
    }

    /// Drains and returns all completed spans, clearing the buffer.
    pub fn drain_spans(&self) -> Vec<SpanRecord> {
        let mut spans = self.completed_spans.write().unwrap();
        let drained: Vec<SpanRecord> = spans.drain(..).collect();
        tracing::debug!("Drained {} spans", drained.len());
        drained
    }

    /// Returns the number of completed spans currently stored.
    pub fn span_count(&self) -> usize {
        let spans = self.completed_spans.read().unwrap();
        spans.len()
    }

    /// Returns all spans for a specific trace ID.
    pub fn spans_for_trace(&self, trace_id: TraceId) -> Vec<SpanRecord> {
        let spans = self.completed_spans.read().unwrap();
        spans
            .iter()
            .filter(|s| s.trace_ctx.trace_id == trace_id)
            .cloned()
            .collect()
    }

    /// Adds a key-value attribute to a span.
    pub fn add_attribute(span: &mut SpanRecord, key: String, value: String) {
        span.attributes.push((key, value));
    }

    /// Sets the span status to error with the given message.
    pub fn set_error(span: &mut SpanRecord, message: String) {
        span.status = SpanStatus::Error { message };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_trace_generates_unique_ids() {
        let collector = TraceCollector::new(100);
        let trace1 = collector.new_trace();
        let trace2 = collector.new_trace();

        assert_ne!(trace1.trace_id, trace2.trace_id);
        assert_ne!(trace1.span_id, trace2.span_id);
    }

    #[test]
    fn test_new_trace_has_sampled() {
        let collector = TraceCollector::new(100);
        let trace = collector.new_trace();
        assert!(trace.sampled);
    }

    #[test]
    fn test_child_span_inherits_trace_id() {
        let collector = TraceCollector::new(100);
        let parent = collector.new_trace();
        let child = collector.child_span(&parent);

        assert_eq!(child.trace_id, parent.trace_id);
        assert_ne!(child.span_id, parent.span_id);
        assert_eq!(child.parent_span_id, Some(parent.span_id));
    }

    #[test]
    fn test_child_span_has_different_span_id() {
        let collector = TraceCollector::new(100);
        let parent = collector.new_trace();
        let child = collector.child_span(&parent);

        assert_ne!(child.span_id, parent.span_id);
    }

    #[test]
    fn test_start_end_span_timing() {
        let collector = TraceCollector::new(100);
        let trace = collector.new_trace();
        let mut span = collector.start_span("test_span".to_string(), &trace);

        std::thread::sleep(std::time::Duration::from_millis(10));

        collector.end_span(&mut span);

        assert!(span.end_time.is_some());
        assert!(span.end_time.unwrap().secs >= span.start_time.secs);
    }

    #[test]
    fn test_record_span_stores() {
        let collector = TraceCollector::new(100);
        let trace = collector.new_trace();
        let span = collector.start_span("test_span".to_string(), &trace);

        collector.record_span(span);

        assert_eq!(collector.span_count(), 1);
    }

    #[test]
    fn test_record_span_buffer_limit() {
        let collector = TraceCollector::new(3);
        let trace = collector.new_trace();

        for i in 0..5 {
            let span = collector.start_span(format!("span_{}", i), &trace);
            collector.record_span(span);
        }

        assert_eq!(collector.span_count(), 3);
    }

    #[test]
    fn test_drain_spans_clears() {
        let collector = TraceCollector::new(100);
        let trace = collector.new_trace();
        let span = collector.start_span("test_span".to_string(), &trace);
        collector.record_span(span);

        let drained = collector.drain_spans();

        assert_eq!(drained.len(), 1);
        assert_eq!(collector.span_count(), 0);
    }

    #[test]
    fn test_spans_for_trace_filters() {
        let collector = TraceCollector::new(100);
        let trace1 = collector.new_trace();
        let trace2 = collector.new_trace();
        let trace1_id = trace1.trace_id;

        let span1 = collector.start_span("span1".to_string(), &trace1);
        let span2 = collector.start_span("span2".to_string(), &trace2);

        collector.record_span(span1);
        collector.record_span(span2);

        let spans = collector.spans_for_trace(trace1_id);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_add_attribute() {
        let collector = TraceCollector::new(100);
        let trace = collector.new_trace();
        let mut span = collector.start_span("test".to_string(), &trace);

        TraceCollector::add_attribute(&mut span, "key1".to_string(), "value1".to_string());
        TraceCollector::add_attribute(&mut span, "key2".to_string(), "value2".to_string());

        assert_eq!(span.attributes.len(), 2);
        assert_eq!(
            span.attributes[0],
            ("key1".to_string(), "value1".to_string())
        );
    }

    #[test]
    fn test_set_error() {
        let collector = TraceCollector::new(100);
        let trace = collector.new_trace();
        let mut span = collector.start_span("test".to_string(), &trace);

        TraceCollector::set_error(&mut span, "something went wrong".to_string());

        matches!(&span.status, SpanStatus::Error { message } if message == "something went wrong");
    }

    #[test]
    fn test_set_error_replaces_previous() {
        let collector = TraceCollector::new(100);
        let trace = collector.new_trace();
        let mut span = collector.start_span("test".to_string(), &trace);

        TraceCollector::set_error(&mut span, "error 1".to_string());
        TraceCollector::set_error(&mut span, "error 2".to_string());

        matches!(&span.status, SpanStatus::Error { message } if message == "error 2");
    }

    #[test]
    fn test_span_id_increments() {
        let collector = TraceCollector::new(100);
        let trace1 = collector.new_trace();
        let trace2 = collector.new_trace();
        let trace3 = collector.new_trace();

        assert_ne!(trace1.span_id, trace2.span_id);
        assert_ne!(trace2.span_id, trace3.span_id);
    }

    #[test]
    fn test_empty_spans_for_nonexistent_trace() {
        let collector = TraceCollector::new(100);
        let nonexistent = TraceId::random();
        let spans = collector.spans_for_trace(nonexistent);
        assert!(spans.is_empty());
    }
}
