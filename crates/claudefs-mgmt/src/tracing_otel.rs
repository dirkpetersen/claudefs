use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpanContext {
    pub trace_id: u128,
    pub span_id: u64,
    pub parent_span_id: Option<u64>,
    pub trace_flags: u8,
    pub is_remote: bool,
}

impl SpanContext {
    pub fn new(trace_id: u128, span_id: u64) -> Self {
        Self {
            trace_id,
            span_id,
            parent_span_id: None,
            trace_flags: 0x01,
            is_remote: false,
        }
    }

    pub fn with_parent(mut self, parent_span_id: u64) -> Self {
        self.parent_span_id = Some(parent_span_id);
        self
    }

    pub fn with_flags(mut self, flags: u8) -> Self {
        self.trace_flags = flags;
        self
    }

    pub fn remote(context: SpanContext) -> Self {
        Self {
            is_remote: true,
            ..context
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SpanStatus {
    Unset,
    Ok,
    Error(String),
}

impl Default for SpanStatus {
    fn default() -> Self {
        SpanStatus::Unset
    }
}

impl SpanStatus {
    pub fn is_error(&self) -> bool {
        matches!(self, SpanStatus::Error(_))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AttributeValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanAttribute {
    pub key: String,
    pub value: AttributeValue,
}

impl SpanAttribute {
    pub fn new(key: impl Into<String>, value: AttributeValue) -> Self {
        Self {
            key: key.into(),
            value,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanEvent {
    pub name: String,
    pub time_ns: u64,
    pub attributes: Vec<SpanAttribute>,
}

impl SpanEvent {
    pub fn new(name: impl Into<String>, time_ns: u64) -> Self {
        Self {
            name: name.into(),
            time_ns,
            attributes: vec![],
        }
    }

    pub fn with_attribute(mut self, attr: SpanAttribute) -> Self {
        self.attributes.push(attr);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    pub context: SpanContext,
    pub operation_name: String,
    pub service_name: String,
    pub start_time_ns: u64,
    pub end_time_ns: u64,
    pub attributes: Vec<SpanAttribute>,
    pub events: Vec<SpanEvent>,
    pub status: SpanStatus,
}

impl Span {
    pub fn duration_ns(&self) -> u64 {
        self.end_time_ns.saturating_sub(self.start_time_ns)
    }
}

pub struct SpanBuilder {
    operation_name: Option<String>,
    service_name: Option<String>,
    parent: Option<SpanContext>,
    attributes: Vec<SpanAttribute>,
    start_time_ns: Option<u64>,
    end_time_ns: Option<u64>,
    events: Vec<SpanEvent>,
    status: SpanStatus,
}

impl SpanBuilder {
    pub fn new() -> Self {
        Self {
            operation_name: None,
            service_name: None,
            parent: None,
            attributes: vec![],
            start_time_ns: None,
            end_time_ns: None,
            events: vec![],
            status: SpanStatus::Unset,
        }
    }

    pub fn operation(mut self, name: impl Into<String>) -> Self {
        self.operation_name = Some(name.into());
        self
    }

    pub fn service(mut self, name: impl Into<String>) -> Self {
        self.service_name = Some(name.into());
        self
    }

    pub fn parent(mut self, ctx: SpanContext) -> Self {
        self.parent = Some(ctx);
        self
    }

    pub fn attribute(mut self, key: impl Into<String>, value: AttributeValue) -> Self {
        self.attributes.push(SpanAttribute::new(key, value));
        self
    }

    pub fn event(mut self, event: SpanEvent) -> Self {
        self.events.push(event);
        self
    }

    pub fn start(mut self, time_ns: u64) -> Self {
        self.start_time_ns = Some(time_ns);
        self
    }

    pub fn finish(mut self, time_ns: u64, status: SpanStatus) -> Self {
        self.end_time_ns = Some(time_ns);
        self.status = status;
        self
    }

    pub fn build(self) -> Span {
        let trace_id = self
            .parent
            .as_ref()
            .map(|p| p.trace_id)
            .unwrap_or_else(rand_trace_id);
        let span_id = rand_span_id();
        let parent_span_id = self.parent.as_ref().map(|p| p.span_id);

        Span {
            context: SpanContext {
                trace_id,
                span_id,
                parent_span_id,
                trace_flags: 0x01,
                is_remote: false,
            },
            operation_name: self.operation_name.unwrap_or_default(),
            service_name: self.service_name.unwrap_or_else(|| "unknown".to_string()),
            start_time_ns: self.start_time_ns.unwrap_or(0),
            end_time_ns: self.end_time_ns.unwrap_or(0),
            attributes: self.attributes,
            events: self.events,
            status: self.status,
        }
    }
}

impl Default for SpanBuilder {
    fn default() -> Self {
        Self::new()
    }
}

fn rand_trace_id() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    now ^ (now << 32)
}

fn rand_span_id() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}

pub struct TraceBuffer {
    capacity: usize,
    spans: VecDeque<Span>,
}

impl TraceBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            spans: VecDeque::with_capacity(capacity),
        }
    }

    pub fn push(&mut self, span: Span) {
        if self.spans.len() >= self.capacity {
            self.spans.pop_front();
        }
        self.spans.push_back(span);
    }

    pub fn drain(&mut self) -> Vec<Span> {
        self.spans.drain(..).collect()
    }

    pub fn len(&self) -> usize {
        self.spans.len()
    }

    pub fn is_full(&self) -> bool {
        self.spans.len() >= self.capacity
    }

    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SamplingDecision {
    pub sampled: bool,
    pub reason: &'static str,
}

pub struct RateSampler {
    rate: u32,
}

impl RateSampler {
    pub fn new(rate: u32) -> Self {
        Self { rate: rate.max(1) }
    }

    pub fn should_sample(&self, trace_id: u128) -> SamplingDecision {
        if self.rate == 1 {
            return SamplingDecision {
                sampled: true,
                reason: "always sampled (rate=1)",
            };
        }

        let hash = (trace_id % self.rate as u128) as u32;
        let sampled = hash == 0;

        SamplingDecision {
            sampled,
            reason: if sampled {
                "random sample"
            } else {
                "not sampled"
            },
        }
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }
}

pub struct TracePropagator;

impl TracePropagator {
    pub fn inject(ctx: &SpanContext) -> String {
        let trace_id_hex = format!("{:032x}", ctx.trace_id);
        let span_id_hex = format!("{:016x}", ctx.span_id);
        let flags_hex = format!("{:02x}", ctx.trace_flags);
        format!("00-{}-{}-{}", trace_id_hex, span_id_hex, flags_hex)
    }

    pub fn extract(header: &str) -> Option<SpanContext> {
        let parts: Vec<&str> = header.split('-').collect();
        if parts.len() != 4 {
            return None;
        }

        if parts[0] != "00" {
            return None;
        }

        let trace_id = u128::from_str_radix(parts[1], 16).ok()?;
        let span_id = u64::from_str_radix(parts[2], 16).ok()?;
        let flags = u8::from_str_radix(parts[3], 16).ok()?;

        Some(SpanContext {
            trace_id,
            span_id,
            parent_span_id: None,
            trace_flags: flags,
            is_remote: true,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceExportBatch {
    pub spans: Vec<Span>,
    pub exported_at_ns: u64,
    pub service_name: String,
}

impl TraceExportBatch {
    pub fn new(spans: Vec<Span>, service_name: String) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let exported_at_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        Self {
            spans,
            exported_at_ns,
            service_name,
        }
    }

    pub fn span_count(&self) -> usize {
        self.spans.len()
    }
}

#[derive(Debug, Default, Clone)]
pub struct SpanStats {
    pub total_spans: u64,
    pub sampled_spans: u64,
    pub dropped_spans: u64,
    pub error_spans: u64,
}

impl SpanStats {
    pub fn total(&self) -> u64 {
        self.total_spans
    }

    pub fn sampled(&self) -> u64 {
        self.sampled_spans
    }

    pub fn dropped(&self) -> u64 {
        self.dropped_spans
    }

    pub fn errors(&self) -> u64 {
        self.error_spans
    }
}

pub struct TracingManager {
    sampler: RateSampler,
    buffer: TraceBuffer,
    stats: SpanStats,
}

impl TracingManager {
    pub fn new(sample_rate: u32, buffer_capacity: usize) -> Self {
        Self {
            sampler: RateSampler::new(sample_rate),
            buffer: TraceBuffer::new(buffer_capacity),
            stats: SpanStats::default(),
        }
    }

    pub fn record(&mut self, span: Span) {
        self.stats.total_spans += 1;

        let decision = self.sampler.should_sample(span.context.trace_id);

        if decision.sampled {
            self.stats.sampled_spans += 1;

            if span.status.is_error() {
                self.stats.error_spans += 1;
            }

            if self.buffer.is_full() {
                self.stats.dropped_spans += 1;
            } else {
                self.buffer.push(span);
            }
        }
    }

    pub fn flush(&mut self) -> TraceExportBatch {
        let spans = self.buffer.drain();
        let batch = TraceExportBatch::new(spans, "claudefs".to_string());
        batch
    }

    pub fn stats(&self) -> &SpanStats {
        &self.stats
    }

    pub fn reset_stats(&mut self) {
        self.stats = SpanStats::default();
    }

    pub fn buffer_len(&self) -> usize {
        self.buffer.len()
    }

    pub fn sample_rate(&self) -> u32 {
        self.sampler.rate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_span_context_new() {
        let ctx = SpanContext::new(123, 456);
        assert_eq!(ctx.trace_id, 123);
        assert_eq!(ctx.span_id, 456);
        assert_eq!(ctx.trace_flags, 0x01);
        assert!(!ctx.is_remote);
    }

    #[test]
    fn test_span_context_with_parent() {
        let parent = SpanContext::new(100, 200);
        let ctx = SpanContext::new(123, 456).with_parent(200);
        assert_eq!(ctx.parent_span_id, Some(200));
    }

    #[test]
    fn test_span_builder_builds_valid_span() {
        let span = SpanBuilder::new()
            .operation("test_operation")
            .service("test_service")
            .attribute("key1", AttributeValue::String("value1".to_string()))
            .start(1000)
            .finish(2000, SpanStatus::Ok)
            .build();

        assert_eq!(span.operation_name, "test_operation");
        assert_eq!(span.service_name, "test_service");
        assert_eq!(span.attributes.len(), 1);
        assert_eq!(span.start_time_ns, 1000);
        assert_eq!(span.end_time_ns, 2000);
    }

    #[test]
    fn test_span_builder_with_events() {
        let event = SpanEvent::new("test_event", 1500)
            .with_attribute(SpanAttribute::new("evt_key", AttributeValue::Int(42)));

        let span = SpanBuilder::new()
            .operation("test_op")
            .service("test_svc")
            .event(event)
            .start(1000)
            .finish(2000, SpanStatus::Ok)
            .build();

        assert_eq!(span.events.len(), 1);
        assert_eq!(span.events[0].name, "test_event");
        assert_eq!(span.events[0].attributes.len(), 1);
    }

    #[test]
    fn test_span_status_is_error() {
        assert!(!SpanStatus::Unset.is_error());
        assert!(!SpanStatus::Ok.is_error());
        assert!(SpanStatus::Error("test".to_string()).is_error());
    }

    #[test]
    fn test_attribute_value_variants() {
        let _ = AttributeValue::String("test".to_string());
        let _ = AttributeValue::Int(42);
        let _ = AttributeValue::Float(3.14);
        let _ = AttributeValue::Bool(true);
    }

    #[test]
    fn test_trace_buffer_push_up_to_capacity() {
        let mut buffer = TraceBuffer::new(3);
        assert!(!buffer.is_full());

        for i in 0..3 {
            let span = SpanBuilder::new()
                .operation(format!("op{}", i))
                .service("svc")
                .start(i)
                .finish(i + 1, SpanStatus::Ok)
                .build();
            buffer.push(span);
        }

        assert!(buffer.is_full());
        assert_eq!(buffer.len(), 3);
    }

    #[test]
    fn test_trace_buffer_push_beyond_capacity_drops_oldest() {
        let mut buffer = TraceBuffer::new(3);

        for i in 0..5 {
            let span = SpanBuilder::new()
                .operation(format!("op{}", i))
                .service("svc")
                .start(i)
                .finish(i + 1, SpanStatus::Ok)
                .build();
            buffer.push(span);
        }

        assert_eq!(buffer.len(), 3);
    }

    #[test]
    fn test_trace_buffer_drain_clears_buffer() {
        let mut buffer = TraceBuffer::new(3);

        let span = SpanBuilder::new()
            .operation("op")
            .service("svc")
            .start(0)
            .finish(1, SpanStatus::Ok)
            .build();
        buffer.push(span);

        let drained = buffer.drain();
        assert_eq!(drained.len(), 1);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_rate_sampler_rate_1_samples_everything() {
        let sampler = RateSampler::new(1);
        for i in 0..100 {
            let decision = sampler.should_sample(i);
            assert!(decision.sampled, "trace_id {} should be sampled", i);
        }
    }

    #[test]
    fn test_rate_sampler_rate_100_samples_approx_1_percent() {
        let sampler = RateSampler::new(100);
        let mut sampled_count = 0;
        let iterations = 10000;

        for i in 0..iterations {
            let decision = sampler.should_sample(i as u128);
            if decision.sampled {
                sampled_count += 1;
            }
        }

        let actual_rate = sampled_count as f64 / iterations as f64;
        assert!(
            actual_rate > 0.005 && actual_rate < 0.02,
            "Expected ~1% but got {:.2}%",
            actual_rate * 100.0
        );
    }

    #[test]
    fn test_rate_sampler_rate_0_uses_1() {
        let sampler = RateSampler::new(0);
        let decision = sampler.should_sample(123);
        assert!(decision.sampled);
    }

    #[test]
    fn test_trace_propagator_inject_produces_valid_format() {
        let ctx = SpanContext::new(0x0af7651916cd43dd8448eb211c80319c, 0x00f067aa0ba902b7);
        let header = TracePropagator::inject(&ctx);

        assert!(header.starts_with("00-"));
        assert_eq!(header.len(), 55); // "00-" (3) + 32 + "-" (1) + 16 + "-" (1) + 2
    }

    #[test]
    fn test_trace_propagator_inject_round_trip() {
        let original = SpanContext::new(0x0af7651916cd43dd8448eb211c80319c, 0x00f067aa0ba902b7);
        let header = TracePropagator::inject(&original);
        let extracted = TracePropagator::extract(&header);

        assert!(extracted.is_some());
        let ctx = extracted.unwrap();
        assert_eq!(ctx.trace_id, original.trace_id);
        assert_eq!(ctx.span_id, original.span_id);
    }

    #[test]
    fn test_trace_propagator_extract_valid_traceparent() {
        let header = "00-0af7651916cd43dd8448eb211c80319c-00f067aa0ba902b7-01";
        let ctx = TracePropagator::extract(header);

        assert!(ctx.is_some());
        let ctx = ctx.unwrap();
        assert_eq!(ctx.trace_id, 0x0af7651916cd43dd8448eb211c80319c);
        assert_eq!(ctx.span_id, 0x00f067aa0ba902b7);
        assert_eq!(ctx.trace_flags, 0x01);
        assert!(ctx.is_remote);
    }

    #[test]
    fn test_trace_propagator_extract_invalid_returns_none() {
        assert!(TracePropagator::extract("invalid").is_none());
        assert!(TracePropagator::extract("").is_none());
        assert!(TracePropagator::extract("00-").is_none());
    }

    #[test]
    fn test_trace_propagator_extract_wrong_version() {
        let header = "01-0af7651916cd43dd8448eb211c80319c-00f067aa0ba902b7-01";
        assert!(TracePropagator::extract(header).is_none());
    }

    #[test]
    fn test_tracing_manager_record_updates_stats() {
        let mut mgr = TracingManager::new(1, 100);

        let span = SpanBuilder::new()
            .operation("test")
            .service("svc")
            .start(0)
            .finish(1, SpanStatus::Ok)
            .build();
        mgr.record(span);

        let stats = mgr.stats();
        assert_eq!(stats.total_spans, 1);
        assert_eq!(stats.sampled_spans, 1);
    }

    #[test]
    fn test_tracing_manager_buffer_fills() {
        let mut mgr = TracingManager::new(1, 2);

        for i in 0..3 {
            let span = SpanBuilder::new()
                .operation(format!("op{}", i))
                .service("svc")
                .start(i)
                .finish(i + 1, SpanStatus::Ok)
                .build();
            mgr.record(span);
        }

        assert_eq!(mgr.buffer_len(), 2);
    }

    #[test]
    fn test_tracing_manager_flush_drains_buffer() {
        let mut mgr = TracingManager::new(1, 100);

        let span = SpanBuilder::new()
            .operation("test")
            .service("svc")
            .start(0)
            .finish(1, SpanStatus::Ok)
            .build();
        mgr.record(span);

        let batch = mgr.flush();
        assert_eq!(batch.span_count(), 1);
        assert_eq!(mgr.buffer_len(), 0);
    }

    #[test]
    fn test_tracing_manager_dropped_spans_increments_when_buffer_full() {
        let mut mgr = TracingManager::new(1, 1);

        let span1 = SpanBuilder::new()
            .operation("op1")
            .service("svc")
            .start(0)
            .finish(1, SpanStatus::Ok)
            .build();
        mgr.record(span1);

        let span2 = SpanBuilder::new()
            .operation("op2")
            .service("svc")
            .start(1)
            .finish(2, SpanStatus::Ok)
            .build();
        mgr.record(span2);

        let stats = mgr.stats();
        assert_eq!(stats.dropped_spans, 1);
    }

    #[test]
    fn test_tracing_manager_error_spans_counted() {
        let mut mgr = TracingManager::new(1, 100);

        let span = SpanBuilder::new()
            .operation("test")
            .service("svc")
            .start(0)
            .finish(1, SpanStatus::Error("failed".to_string()))
            .build();
        mgr.record(span);

        assert_eq!(mgr.stats().error_spans, 1);
    }

    #[test]
    fn test_tracing_manager_reset_stats() {
        let mut mgr = TracingManager::new(1, 100);

        let span = SpanBuilder::new()
            .operation("test")
            .service("svc")
            .start(0)
            .finish(1, SpanStatus::Ok)
            .build();
        mgr.record(span);

        mgr.reset_stats();
        assert_eq!(mgr.stats().total_spans, 0);
    }

    #[test]
    fn test_span_duration_ns() {
        let span = SpanBuilder::new()
            .operation("test")
            .service("svc")
            .start(1000)
            .finish(2500, SpanStatus::Ok)
            .build();

        assert_eq!(span.duration_ns(), 1500);
    }

    #[test]
    fn test_sampling_decision_reasons() {
        let sampler = RateSampler::new(1);
        let decision = sampler.should_sample(123);
        assert_eq!(decision.reason, "always sampled (rate=1)");
    }
}
