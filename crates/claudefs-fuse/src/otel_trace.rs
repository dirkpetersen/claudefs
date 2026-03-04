//! OpenTelemetry-compatible tracing types for FUSE operations.
//!
//! This module provides span, sampler, and export buffer types that follow
//! the OpenTelemetry specification for distributed tracing.

use crate::tracing_client::{SpanId, TraceContext, TraceId};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Status of a span indicating whether it completed successfully.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpanStatus {
    /// The span completed successfully.
    Ok,
    /// The span encountered an error with an optional message.
    Error(String),
    /// The span status has not been set.
    Unset,
}

/// Classification of a span's role in a distributed trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    /// Internal work within a service.
    Internal,
    /// A synchronous outgoing request to an external service.
    Client,
    /// A synchronous incoming request handler.
    Server,
    /// A message producer (asynchronous).
    Producer,
    /// A message consumer (asynchronous).
    Consumer,
}

/// A key-value attribute attached to a span.
#[derive(Debug, Clone)]
pub struct SpanAttribute {
    /// The attribute key name.
    pub key: String,
    /// The attribute value as a string.
    pub value: String,
}

/// An OpenTelemetry span representing a unit of work.
#[derive(Debug, Clone)]
pub struct OtelSpan {
    /// The trace ID this span belongs to.
    pub trace_id: TraceId,
    /// The unique identifier for this span.
    pub span_id: SpanId,
    /// The parent span ID, if this is a child span.
    pub parent_span_id: Option<SpanId>,
    /// The operation name describing this span's work.
    pub operation: String,
    /// The service name that emitted this span.
    pub service: String,
    /// The start timestamp in Unix nanoseconds.
    pub start_unix_ns: u64,
    /// The end timestamp in Unix nanoseconds.
    pub end_unix_ns: u64,
    /// The status indicating success or failure.
    pub status: SpanStatus,
    /// The kind classification of this span.
    pub kind: SpanKind,
    /// Additional attributes attached to this span.
    pub attributes: Vec<SpanAttribute>,
}

impl OtelSpan {
    /// Returns the duration of the span in nanoseconds.
    pub fn duration_ns(&self) -> u64 {
        self.end_unix_ns.saturating_sub(self.start_unix_ns)
    }

    /// Returns true if this span represents an error.
    pub fn is_error(&self) -> bool {
        matches!(self.status, SpanStatus::Error(_))
    }

    /// Adds a key-value attribute to this span.
    pub fn add_attribute(&mut self, key: String, value: String) {
        self.attributes.push(SpanAttribute { key, value });
    }

    /// Sets the status of this span.
    pub fn set_status(&mut self, status: SpanStatus) {
        self.status = status;
    }

    /// Marks the span as finished with the given end timestamp.
    pub fn finish(&mut self, end_ns: u64) {
        self.end_unix_ns = end_ns;
    }
}

/// A builder for constructing `OtelSpan` instances.
pub struct OtelSpanBuilder {
    /// The trace ID to use, or None to generate a default.
    pub trace_id: Option<TraceId>,
    /// The parent span ID for trace linking.
    pub parent_span_id: Option<SpanId>,
    /// The operation name for the span.
    pub operation: String,
    /// The service name for the span.
    pub service: String,
    /// The start timestamp in Unix nanoseconds.
    pub start_unix_ns: u64,
    /// The optional end timestamp in Unix nanoseconds.
    pub end_unix_ns: Option<u64>,
    /// The status of the span.
    pub status: SpanStatus,
    /// The kind classification of the span.
    pub kind: SpanKind,
    /// Attributes to attach to the span.
    pub attributes: Vec<SpanAttribute>,
}

impl OtelSpanBuilder {
    /// Creates a new span builder with the given operation, service, and start time.
    pub fn new(operation: String, service: String, start_unix_ns: u64) -> Self {
        Self {
            trace_id: None,
            parent_span_id: None,
            operation,
            service,
            start_unix_ns,
            end_unix_ns: None,
            status: SpanStatus::Unset,
            kind: SpanKind::Internal,
            attributes: Vec::new(),
        }
    }

    /// Sets the trace and parent span IDs from an existing trace context.
    pub fn with_parent(mut self, parent: &TraceContext) -> Self {
        self.trace_id = Some(parent.trace_id);
        self.parent_span_id = Some(parent.span_id);
        self
    }

    /// Sets the trace ID directly.
    pub fn with_trace_id(mut self, trace_id: TraceId) -> Self {
        self.trace_id = Some(trace_id);
        self
    }

    /// Sets the span kind.
    pub fn with_kind(mut self, kind: SpanKind) -> Self {
        self.kind = kind;
        self
    }

    /// Adds an attribute to the span being built.
    pub fn with_attribute(mut self, key: String, value: String) -> Self {
        self.attributes.push(SpanAttribute { key, value });
        self
    }

    /// Builds the final `OtelSpan` with the given end timestamp.
    pub fn build(self, end_unix_ns: u64) -> OtelSpan {
        let trace_id = self.trace_id.unwrap_or(TraceId(0));
        let parent_span_id = self.parent_span_id;

        let span_id = {
            let mut hasher = DefaultHasher::new();
            self.operation.hash(&mut hasher);
            self.start_unix_ns.hash(&mut hasher);
            SpanId(hasher.finish())
        };

        OtelSpan {
            trace_id,
            span_id,
            parent_span_id,
            operation: self.operation,
            service: self.service,
            start_unix_ns: self.start_unix_ns,
            end_unix_ns,
            status: self.status,
            kind: self.kind,
            attributes: self.attributes,
        }
    }
}

/// A fixed-capacity buffer for collecting spans before export.
#[derive(Debug, Clone)]
pub struct OtelExportBuffer {
    /// The maximum number of spans the buffer can hold.
    pub capacity: usize,
    spans: Vec<OtelSpan>,
}

impl OtelExportBuffer {
    /// Creates a new buffer with the given capacity (clamped to 1-10000).
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.clamp(1, 10_000),
            spans: Vec::new(),
        }
    }

    /// Adds a span to the buffer, evicting the oldest if at capacity.
    pub fn push(&mut self, span: OtelSpan) {
        if self.spans.len() >= self.capacity {
            self.spans.remove(0);
        }
        self.spans.push(span);
    }

    /// Removes and returns all spans from the buffer.
    pub fn drain(&mut self) -> Vec<OtelSpan> {
        std::mem::take(&mut self.spans)
    }

    /// Returns the current number of spans in the buffer.
    pub fn len(&self) -> usize {
        self.spans.len()
    }

    /// Returns true if the buffer contains no spans.
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }
}

/// The sampling decision for a trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplingDecision {
    /// Record the span and sample it for export.
    RecordAndSample,
    /// Drop the span and do not export it.
    Drop,
}

/// A sampler that decides which traces to record based on a sample rate.
pub struct OtelSampler {
    sample_rate: f64,
}

impl OtelSampler {
    /// Creates a new sampler with the given sample rate (clamped to 0.0-1.0).
    pub fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate: sample_rate.clamp(0.0, 1.0),
        }
    }

    /// Determines whether to sample a trace based on its trace ID.
    pub fn should_sample(&self, trace_id: TraceId) -> SamplingDecision {
        if self.sample_rate >= 1.0 {
            return SamplingDecision::RecordAndSample;
        }
        if self.sample_rate <= 0.0 {
            return SamplingDecision::Drop;
        }

        let TraceId(id) = trace_id;
        let hash = id.wrapping_mul(0x9e3779b97f4a7c15).wrapping_add(id >> 2);
        let lower_bits = (hash & 0xFFFFFFFF) as u64;
        let threshold = (self.sample_rate * 1_000_000_003.0) as u64;

        if lower_bits % 1_000_000_003 < threshold {
            SamplingDecision::RecordAndSample
        } else {
            SamplingDecision::Drop
        }
    }

    /// Returns the configured sample rate.
    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duration_ns() {
        let span = OtelSpan {
            trace_id: TraceId(1),
            span_id: SpanId(1),
            parent_span_id: None,
            operation: "test".to_string(),
            service: "svc".to_string(),
            start_unix_ns: 1000,
            end_unix_ns: 2000,
            status: SpanStatus::Ok,
            kind: SpanKind::Internal,
            attributes: vec![],
        };
        assert_eq!(span.duration_ns(), 1000);
    }

    #[test]
    fn test_is_error() {
        let mut span = OtelSpan {
            trace_id: TraceId(1),
            span_id: SpanId(1),
            parent_span_id: None,
            operation: "test".to_string(),
            service: "svc".to_string(),
            start_unix_ns: 1000,
            end_unix_ns: 2000,
            status: SpanStatus::Ok,
            kind: SpanKind::Internal,
            attributes: vec![],
        };
        assert!(!span.is_error());

        span.status = SpanStatus::Error("fail".to_string());
        assert!(span.is_error());
    }

    #[test]
    fn test_add_attribute() {
        let mut span = OtelSpan {
            trace_id: TraceId(1),
            span_id: SpanId(1),
            parent_span_id: None,
            operation: "test".to_string(),
            service: "svc".to_string(),
            start_unix_ns: 1000,
            end_unix_ns: 2000,
            status: SpanStatus::Ok,
            kind: SpanKind::Internal,
            attributes: vec![],
        };
        span.add_attribute("key".to_string(), "value".to_string());
        assert_eq!(span.attributes.len(), 1);
        assert_eq!(span.attributes[0].key, "key");
    }

    #[test]
    fn test_span_builder_build() {
        let builder = OtelSpanBuilder::new("op".to_string(), "svc".to_string(), 1000);
        let span = builder.build(2000);
        assert_eq!(span.operation, "op");
        assert_eq!(span.service, "svc");
        assert_eq!(span.start_unix_ns, 1000);
        assert_eq!(span.end_unix_ns, 2000);
    }

    #[test]
    fn test_span_builder_with_parent() {
        let ctx = TraceContext {
            trace_id: TraceId(123),
            span_id: SpanId(456),
            sampled: true,
        };
        let builder =
            OtelSpanBuilder::new("op".to_string(), "svc".to_string(), 1000).with_parent(&ctx);
        let span = builder.build(2000);
        assert_eq!(span.trace_id, TraceId(123));
        assert_eq!(span.parent_span_id, Some(SpanId(456)));
    }

    #[test]
    fn test_export_buffer_push_drain() {
        let mut buf = OtelExportBuffer::new(10);
        let span = OtelSpan {
            trace_id: TraceId(1),
            span_id: SpanId(1),
            parent_span_id: None,
            operation: "test".to_string(),
            service: "svc".to_string(),
            start_unix_ns: 1000,
            end_unix_ns: 2000,
            status: SpanStatus::Ok,
            kind: SpanKind::Internal,
            attributes: vec![],
        };
        buf.push(span.clone());
        assert_eq!(buf.len(), 1);

        let drained = buf.drain();
        assert_eq!(drained.len(), 1);
        assert!(buf.is_empty());
    }

    #[test]
    fn test_export_buffer_drops_oldest_when_full() {
        let mut buf = OtelExportBuffer::new(2);
        for i in 0..3 {
            let span = OtelSpan {
                trace_id: TraceId(i as u128),
                span_id: SpanId(i as u64),
                parent_span_id: None,
                operation: format!("op{}", i),
                service: "svc".to_string(),
                start_unix_ns: 1000 + i as u64,
                end_unix_ns: 2000,
                status: SpanStatus::Ok,
                kind: SpanKind::Internal,
                attributes: vec![],
            };
            buf.push(span);
        }
        assert_eq!(buf.len(), 2);
    }

    #[test]
    fn test_sampler_sample_all() {
        let sampler = OtelSampler::new(1.0);
        assert_eq!(
            sampler.should_sample(TraceId(0)),
            SamplingDecision::RecordAndSample
        );
        assert_eq!(
            sampler.should_sample(TraceId(u128::MAX)),
            SamplingDecision::RecordAndSample
        );
    }

    #[test]
    fn test_sampler_drop_all() {
        let sampler = OtelSampler::new(0.0);
        assert_eq!(sampler.should_sample(TraceId(0)), SamplingDecision::Drop);
        assert_eq!(
            sampler.should_sample(TraceId(u128::MAX)),
            SamplingDecision::Drop
        );
    }

    #[test]
    fn test_sampler_determinism() {
        let sampler = OtelSampler::new(0.5);
        let tid = TraceId(123456);
        let decision1 = sampler.should_sample(tid);
        let decision2 = sampler.should_sample(tid);
        assert_eq!(decision1, decision2);
    }

    #[test]
    fn test_sampler_half_rate() {
        let sampler = OtelSampler::new(0.5);
        let mut yes = 0;
        let mut no = 0;
        for i in 0..1000 {
            match sampler.should_sample(TraceId(i * 1000)) {
                SamplingDecision::RecordAndSample => yes += 1,
                SamplingDecision::Drop => no += 1,
            }
        }
        assert!(yes > 400, "expected ~50% samples, got {}", yes);
        assert!(no > 400, "expected ~50% drops, got {}", no);
    }
}
