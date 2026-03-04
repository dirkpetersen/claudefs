//! OpenTelemetry OTLP trace bridge for distributed tracing.
//!
//! This module converts internal `observability::Span` records into an OTLP-compatible
//! wire format and queues them for batched export to an OpenTelemetry Collector endpoint.
//! No actual HTTP/gRPC calls — just the data model, encoding, batching, and queue.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::observability;
use crate::tracecontext;

/// OTLP-compatible span record (subset of OTLP spec sufficient for ClaudeFS).
#[derive(Debug, Clone)]
pub struct OtlpSpan {
    /// 128-bit trace identifier.
    pub trace_id: [u8; 16],
    /// 64-bit span identifier.
    pub span_id: [u8; 8],
    /// Parent span ID, if any.
    pub parent_span_id: Option<[u8; 8]>,
    /// Span name (operation name).
    pub name: String,
    /// Start timestamp in nanoseconds since Unix epoch.
    pub start_time_unix_nano: u64,
    /// End timestamp in nanoseconds since Unix epoch.
    pub end_time_unix_nano: u64,
    /// Span status code.
    pub status_code: OtlpStatusCode,
    /// Span attributes.
    pub attributes: Vec<OtlpAttribute>,
    /// Span events (timed logs).
    pub events: Vec<OtlpEvent>,
}

impl OtlpSpan {
    /// Creates a new OTLP span with default values.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            trace_id: [0u8; 16],
            span_id: [0u8; 8],
            parent_span_id: None,
            name: name.into(),
            start_time_unix_nano: 0,
            end_time_unix_nano: 0,
            status_code: OtlpStatusCode::Unset,
            attributes: Vec::new(),
            events: Vec::new(),
        }
    }
}

/// OTLP status codes (subset).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtlpStatusCode {
    /// Default status (not set).
    Unset,
    /// Operation completed successfully.
    Ok,
    /// Operation encountered an error.
    Error,
}

/// OTLP key-value attribute.
#[derive(Debug, Clone)]
pub struct OtlpAttribute {
    /// Attribute key.
    pub key: String,
    /// Attribute value.
    pub value: OtlpValue,
}

impl OtlpAttribute {
    /// Creates a new OTLP attribute.
    pub fn new(key: impl Into<String>, value: OtlpValue) -> Self {
        Self {
            key: key.into(),
            value,
        }
    }
}

/// OTLP attribute value types.
#[derive(Debug, Clone)]
pub enum OtlpValue {
    /// String value.
    String(String),
    /// Signed 64-bit integer.
    Int(i64),
    /// Double-precision floating point.
    Double(f64),
    /// Boolean value.
    Bool(bool),
    /// Byte array.
    Bytes(Vec<u8>),
}

impl OtlpValue {
    /// Creates a string OTLP value.
    pub fn string(v: impl Into<String>) -> Self {
        Self::String(v.into())
    }

    /// Creates an integer OTLP value.
    pub fn int(v: i64) -> Self {
        Self::Int(v)
    }

    /// Creates a double OTLP value.
    pub fn double(v: f64) -> Self {
        Self::Double(v)
    }

    /// Creates a boolean OTLP value.
    pub fn bool(v: bool) -> Self {
        Self::Bool(v)
    }

    /// Creates a bytes OTLP value.
    pub fn bytes(v: Vec<u8>) -> Self {
        Self::Bytes(v)
    }
}

/// OTLP event (timed log record within a span).
#[derive(Debug, Clone)]
pub struct OtlpEvent {
    /// Event name.
    pub name: String,
    /// Event timestamp in nanoseconds since Unix epoch.
    pub time_unix_nano: u64,
    /// Event attributes.
    pub attributes: Vec<OtlpAttribute>,
}

impl OtlpEvent {
    /// Creates a new OTLP event.
    pub fn new(name: impl Into<String>, time_unix_nano: u64) -> Self {
        Self {
            name: name.into(),
            time_unix_nano,
            attributes: Vec::new(),
        }
    }

    /// Adds attributes to the event.
    pub fn with_attributes(mut self, attrs: Vec<OtlpAttribute>) -> Self {
        self.attributes = attrs;
        self
    }
}

/// Configuration for the OTLP exporter.
#[derive(Debug, Clone)]
pub struct OtlpConfig {
    /// OTLP collector endpoint (e.g., "http://localhost:4318").
    pub endpoint: String,
    /// Maximum spans per export batch.
    pub batch_size: usize,
    /// Maximum queue depth before dropping.
    pub queue_capacity: usize,
    /// Service name for the resource.
    pub service_name: String,
    /// Whether export is enabled.
    pub enabled: bool,
}

impl Default for OtlpConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:4318".to_string(),
            batch_size: 512,
            queue_capacity: 4096,
            service_name: "claudefs".to_string(),
            enabled: true,
        }
    }
}

/// Export queue stats tracking.
pub struct OtlpExporterStats {
    spans_queued: AtomicU64,
    spans_dropped: AtomicU64,
    batches_prepared: AtomicU64,
    export_errors: AtomicU64,
}

impl Default for OtlpExporterStats {
    fn default() -> Self {
        Self::new()
    }
}

impl OtlpExporterStats {
    /// Creates a new stats tracker with all counters zero.
    pub fn new() -> Self {
        Self {
            spans_queued: AtomicU64::new(0),
            spans_dropped: AtomicU64::new(0),
            batches_prepared: AtomicU64::new(0),
            export_errors: AtomicU64::new(0),
        }
    }

    fn inc_queued(&self) {
        self.spans_queued.fetch_add(1, Ordering::Relaxed);
    }

    fn inc_dropped(&self) {
        self.spans_dropped.fetch_add(1, Ordering::Relaxed);
    }

    fn inc_batches(&self) {
        self.batches_prepared.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns a snapshot of current stats.
    pub fn snapshot(&self) -> OtlpExporterStatsSnapshot {
        OtlpExporterStatsSnapshot {
            spans_queued: self.spans_queued.load(Ordering::Relaxed),
            spans_dropped: self.spans_dropped.load(Ordering::Relaxed),
            batches_prepared: self.batches_prepared.load(Ordering::Relaxed),
            export_errors: self.export_errors.load(Ordering::Relaxed),
        }
    }
}

/// Export queue stats snapshot.
#[derive(Debug, Clone)]
pub struct OtlpExporterStatsSnapshot {
    /// Total spans successfully queued.
    pub spans_queued: u64,
    /// Total spans dropped due to queue full.
    pub spans_dropped: u64,
    /// Total batches prepared for export.
    pub batches_prepared: u64,
    /// Total export errors.
    pub export_errors: u64,
}

/// OTLP span exporter: queues spans, drains in batches.
pub struct OtlpExporter {
    config: OtlpConfig,
    queue: Mutex<VecDeque<OtlpSpan>>,
    stats: OtlpExporterStats,
}

impl OtlpExporter {
    /// Creates a new OTLP exporter with the given configuration.
    pub fn new(config: OtlpConfig) -> Self {
        Self {
            config,
            queue: Mutex::new(VecDeque::new()),
            stats: OtlpExporterStats::new(),
        }
    }

    /// Enqueues a span for later export.
    ///
    /// Returns `true` if the span was queued, `false` if dropped (queue full or disabled).
    pub fn enqueue(&self, span: OtlpSpan) -> bool {
        if !self.config.enabled {
            return false;
        }

        if let Ok(mut queue) = self.queue.lock() {
            if queue.len() >= self.config.queue_capacity {
                self.stats.inc_dropped();
                return false;
            }
            queue.push_back(span);
            self.stats.inc_queued();
            return true;
        }
        self.stats.inc_dropped();
        false
    }

    /// Drains up to `batch_size` spans from the queue.
    ///
    /// Returns the drained spans as a Vec for export.
    pub fn drain_batch(&self) -> Vec<OtlpSpan> {
        if let Ok(mut queue) = self.queue.lock() {
            let count = std::cmp::min(self.config.batch_size, queue.len());
            let batch: Vec<OtlpSpan> = queue.drain(..count).collect();
            if !batch.is_empty() {
                self.stats.inc_batches();
            }
            return batch;
        }
        Vec::new()
    }

    /// Returns the current queue depth.
    pub fn queue_depth(&self) -> usize {
        if let Ok(queue) = self.queue.lock() {
            queue.len()
        } else {
            0
        }
    }

    /// Returns a snapshot of exporter stats.
    pub fn stats(&self) -> OtlpExporterStatsSnapshot {
        self.stats.snapshot()
    }
}

impl Default for OtlpExporter {
    fn default() -> Self {
        Self::new(OtlpConfig::default())
    }
}

/// Converts an `observability::Span` to an `OtlpSpan`.
///
/// Uses 8 bytes of `span.id.0` (u64 as little-endian `[u8; 8]`) for `span_id`.
/// Uses zeros for `trace_id` (real trace ID comes from `inject_trace_context`).
pub fn span_to_otlp(span: &observability::Span) -> OtlpSpan {
    let span_id = span.id.0.to_le_bytes();

    let parent_span_id = span.parent_id.map(|p| p.0.to_le_bytes());

    let status_code = match span.status {
        observability::SpanStatus::Ok => OtlpStatusCode::Ok,
        observability::SpanStatus::Error => OtlpStatusCode::Error,
        observability::SpanStatus::Timeout => OtlpStatusCode::Error,
        observability::SpanStatus::Cancelled => OtlpStatusCode::Error,
    };

    let attributes = span
        .attributes
        .iter()
        .map(|attr| {
            let value = match &attr.value {
                observability::AttributeValue::String(s) => OtlpValue::String(s.clone()),
                observability::AttributeValue::Int(i) => OtlpValue::Int(*i),
                observability::AttributeValue::Float(f) => OtlpValue::Double(*f),
                observability::AttributeValue::Bool(b) => OtlpValue::Bool(*b),
            };
            OtlpAttribute::new(&attr.key, value)
        })
        .collect();

    let events = span
        .events
        .iter()
        .map(|evt| {
            let attrs = evt
                .attributes
                .iter()
                .map(|attr| {
                    let value = match &attr.value {
                        observability::AttributeValue::String(s) => OtlpValue::String(s.clone()),
                        observability::AttributeValue::Int(i) => OtlpValue::Int(*i),
                        observability::AttributeValue::Float(f) => OtlpValue::Double(*f),
                        observability::AttributeValue::Bool(b) => OtlpValue::Bool(*b),
                    };
                    OtlpAttribute::new(&attr.key, value)
                })
                .collect();
            OtlpEvent::new(&evt.name, evt.timestamp_us * 1000).with_attributes(attrs)
        })
        .collect();

    OtlpSpan {
        trace_id: [0u8; 16],
        span_id,
        parent_span_id,
        name: span.name.clone(),
        start_time_unix_nano: span.start_us * 1000,
        end_time_unix_nano: span.end_us * 1000,
        status_code,
        attributes,
        events,
    }
}

/// Injects W3C trace context into an `OtlpSpan`.
///
/// Overwrites `trace_id` and `parent_span_id` from the `TraceContext`.
pub fn inject_trace_context(span: &mut OtlpSpan, ctx: &tracecontext::TraceContext) {
    span.trace_id = *ctx.traceparent.trace_id.as_bytes();
    span.parent_span_id = Some(*ctx.traceparent.parent_id.as_bytes());
}

#[cfg(test)]
mod tests {
    use crate::observability::{
        self, Attribute, AttributeValue, EventSeverity, Span, SpanEvent, SpanId, SpanStatus,
    };
    use crate::otel::{
        inject_trace_context, span_to_otlp, OtlpAttribute, OtlpConfig, OtlpEvent, OtlpExporter,
        OtlpSpan, OtlpStatusCode, OtlpValue,
    };
    use crate::tracecontext::{TraceContext, TraceFlags, TraceId, TraceParent};

    #[test]
    fn test_otlp_config_default() {
        let config = OtlpConfig::default();
        assert_eq!(config.endpoint, "http://localhost:4318");
        assert_eq!(config.batch_size, 512);
        assert_eq!(config.queue_capacity, 4096);
        assert_eq!(config.service_name, "claudefs");
        assert!(config.enabled);
    }

    #[test]
    fn test_otlp_span_new() {
        let span = OtlpSpan::new("test_operation");
        assert_eq!(span.name, "test_operation");
        assert_eq!(span.trace_id, [0u8; 16]);
        assert_eq!(span.span_id, [0u8; 8]);
        assert!(span.parent_span_id.is_none());
        assert_eq!(span.status_code, OtlpStatusCode::Unset);
    }

    #[test]
    fn test_otlp_status_code_values() {
        assert_ne!(OtlpStatusCode::Unset, OtlpStatusCode::Ok);
        assert_ne!(OtlpStatusCode::Unset, OtlpStatusCode::Error);
        assert_ne!(OtlpStatusCode::Ok, OtlpStatusCode::Error);
    }

    #[test]
    fn test_otlp_attribute_string() {
        let attr = OtlpAttribute::new("key", OtlpValue::string("value"));
        assert_eq!(attr.key, "key");
        assert!(matches!(attr.value, OtlpValue::String(s) if s == "value"));
    }

    #[test]
    fn test_otlp_attribute_int() {
        let attr = OtlpAttribute::new("count", OtlpValue::int(42));
        assert_eq!(attr.key, "count");
        assert!(matches!(attr.value, OtlpValue::Int(i) if i == 42));
    }

    #[test]
    fn test_otlp_attribute_double() {
        let attr = OtlpAttribute::new("rate", OtlpValue::double(3.14));
        assert_eq!(attr.key, "rate");
        assert!(matches!(attr.value, OtlpValue::Double(d) if (d - 3.14).abs() < 0.001));
    }

    #[test]
    fn test_otlp_attribute_bool() {
        let attr = OtlpAttribute::new("flag", OtlpValue::bool(true));
        assert_eq!(attr.key, "flag");
        assert!(matches!(attr.value, OtlpValue::Bool(b) if b));
    }

    #[test]
    fn test_otlp_attribute_bytes() {
        let attr = OtlpAttribute::new("data", OtlpValue::bytes(vec![1, 2, 3]));
        assert_eq!(attr.key, "data");
        assert!(matches!(attr.value, OtlpValue::Bytes(b) if b == vec![1, 2, 3]));
    }

    #[test]
    fn test_otlp_event_new() {
        let evt = OtlpEvent::new("test_event", 12345);
        assert_eq!(evt.name, "test_event");
        assert_eq!(evt.time_unix_nano, 12345);
        assert!(evt.attributes.is_empty());
    }

    #[test]
    fn test_otlp_event_with_attributes() {
        let attrs = vec![
            OtlpAttribute::new("a", OtlpValue::int(1)),
            OtlpAttribute::new("b", OtlpValue::string("x")),
        ];
        let evt = OtlpEvent::new("evt", 100).with_attributes(attrs);
        assert_eq!(evt.attributes.len(), 2);
    }

    #[test]
    fn test_otlp_exporter_new() {
        let config = OtlpConfig::default();
        let exporter = OtlpExporter::new(config);
        assert_eq!(exporter.queue_depth(), 0);
    }

    #[test]
    fn test_otlp_exporter_enqueue() {
        let exporter = OtlpExporter::default();
        let span = OtlpSpan::new("test");
        assert!(exporter.enqueue(span));
        assert_eq!(exporter.queue_depth(), 1);
        let stats = exporter.stats();
        assert_eq!(stats.spans_queued, 1);
    }

    #[test]
    fn test_otlp_exporter_enqueue_disabled() {
        let config = OtlpConfig {
            enabled: false,
            ..Default::default()
        };
        let exporter = OtlpExporter::new(config);
        let span = OtlpSpan::new("test");
        assert!(!exporter.enqueue(span));
        assert_eq!(exporter.queue_depth(), 0);
    }

    #[test]
    fn test_otlp_exporter_enqueue_capacity() {
        let config = OtlpConfig {
            queue_capacity: 2,
            ..Default::default()
        };
        let exporter = OtlpExporter::new(config);
        assert!(exporter.enqueue(OtlpSpan::new("a")));
        assert!(exporter.enqueue(OtlpSpan::new("b")));
        assert!(!exporter.enqueue(OtlpSpan::new("c")));
        let stats = exporter.stats();
        assert_eq!(stats.spans_dropped, 1);
    }

    #[test]
    fn test_otlp_exporter_drain_batch_empty() {
        let exporter = OtlpExporter::default();
        let batch = exporter.drain_batch();
        assert!(batch.is_empty());
        let stats = exporter.stats();
        assert_eq!(stats.batches_prepared, 0);
    }

    #[test]
    fn test_otlp_exporter_drain_batch_size() {
        let config = OtlpConfig {
            batch_size: 2,
            ..Default::default()
        };
        let exporter = OtlpExporter::new(config);
        exporter.enqueue(OtlpSpan::new("a"));
        exporter.enqueue(OtlpSpan::new("b"));
        exporter.enqueue(OtlpSpan::new("c"));

        let batch = exporter.drain_batch();
        assert_eq!(batch.len(), 2);
        assert_eq!(exporter.queue_depth(), 1);
        let stats = exporter.stats();
        assert_eq!(stats.batches_prepared, 1);
    }

    #[test]
    fn test_otlp_exporter_drain_all() {
        let exporter = OtlpExporter::default();
        exporter.enqueue(OtlpSpan::new("a"));
        exporter.enqueue(OtlpSpan::new("b"));

        let batch1 = exporter.drain_batch();
        assert_eq!(batch1.len(), 2);

        let batch2 = exporter.drain_batch();
        assert!(batch2.is_empty());
    }

    #[test]
    fn test_span_to_otlp_basic() {
        let obs_span = Span {
            id: SpanId(0x0102030405060708),
            parent_id: Some(SpanId(0x1122334455667788)),
            name: "test_op".to_string(),
            status: observability::SpanStatus::Ok,
            start_us: 1000,
            end_us: 2000,
            attributes: vec![],
            events: vec![],
        };

        let otlp = span_to_otlp(&obs_span);
        assert_eq!(otlp.name, "test_op");
        assert_eq!(otlp.span_id, 0x0102030405060708u64.to_le_bytes());
        assert_eq!(
            otlp.parent_span_id,
            Some(0x1122334455667788u64.to_le_bytes())
        );
        assert_eq!(otlp.status_code, OtlpStatusCode::Ok);
        assert_eq!(otlp.start_time_unix_nano, 1000 * 1000);
        assert_eq!(otlp.end_time_unix_nano, 2000 * 1000);
    }

    #[test]
    fn test_span_to_otlp_status_mapping() {
        let ok_span = Span {
            id: SpanId(1),
            parent_id: None,
            name: "ok".to_string(),
            status: observability::SpanStatus::Ok,
            start_us: 0,
            end_us: 0,
            attributes: vec![],
            events: vec![],
        };
        assert_eq!(span_to_otlp(&ok_span).status_code, OtlpStatusCode::Ok);

        let err_span = Span {
            status: observability::SpanStatus::Error,
            ..ok_span.clone()
        };
        assert_eq!(span_to_otlp(&err_span).status_code, OtlpStatusCode::Error);

        let timeout_span = Span {
            status: observability::SpanStatus::Timeout,
            ..ok_span.clone()
        };
        assert_eq!(
            span_to_otlp(&timeout_span).status_code,
            OtlpStatusCode::Error
        );

        let cancelled_span = Span {
            status: observability::SpanStatus::Cancelled,
            ..ok_span
        };
        assert_eq!(
            span_to_otlp(&cancelled_span).status_code,
            OtlpStatusCode::Error
        );
    }

    #[test]
    fn test_span_to_otlp_attributes() {
        let obs_span = Span {
            id: SpanId(1),
            parent_id: None,
            name: "test".to_string(),
            status: observability::SpanStatus::Ok,
            start_us: 0,
            end_us: 0,
            attributes: vec![
                Attribute::new("s", AttributeValue::String("hello".to_string())),
                Attribute::new("i", AttributeValue::Int(42)),
                Attribute::new("f", AttributeValue::Float(3.14)),
                Attribute::new("b", AttributeValue::Bool(true)),
            ],
            events: vec![],
        };

        let otlp = span_to_otlp(&obs_span);
        assert_eq!(otlp.attributes.len(), 4);
        assert!(matches!(&otlp.attributes[0].value, OtlpValue::String(s) if s == "hello"));
        assert!(matches!(&otlp.attributes[1].value, OtlpValue::Int(42)));
    }

    #[test]
    fn test_span_to_otlp_events() {
        let obs_span = Span {
            id: SpanId(1),
            parent_id: None,
            name: "test".to_string(),
            status: observability::SpanStatus::Ok,
            start_us: 0,
            end_us: 0,
            attributes: vec![],
            events: vec![SpanEvent {
                name: "evt".to_string(),
                severity: EventSeverity::Info,
                timestamp_us: 100,
                attributes: vec![Attribute::new("k", AttributeValue::Int(1))],
            }],
        };

        let otlp = span_to_otlp(&obs_span);
        assert_eq!(otlp.events.len(), 1);
        assert_eq!(otlp.events[0].name, "evt");
        assert_eq!(otlp.events[0].time_unix_nano, 100 * 1000);
        assert_eq!(otlp.events[0].attributes.len(), 1);
    }

    #[test]
    fn test_inject_trace_context() {
        let mut otlp = OtlpSpan::new("test");
        let trace_id = TraceId::from_hex("0af7651916cd43dd8448eb211c80319c").unwrap();
        let parent_id_bytes = [0xb7, 0xad, 0x6b, 0x71, 0x69, 0x20, 0x33, 0x31];
        let ctx = TraceContext {
            traceparent: TraceParent {
                version: 0,
                trace_id: trace_id.clone(),
                parent_id: crate::tracecontext::SpanId::from_bytes(parent_id_bytes),
                flags: TraceFlags::TRACE_FLAG,
            },
            tracestate: crate::tracecontext::TraceState::new(),
        };

        inject_trace_context(&mut otlp, &ctx);

        assert_eq!(otlp.trace_id, *trace_id.as_bytes());
        assert_eq!(otlp.parent_span_id, Some(parent_id_bytes));
    }

    #[test]
    fn test_otlp_value_clone() {
        let v1 = OtlpValue::string("test");
        let v2 = v1.clone();
        assert!(matches!(v2, OtlpValue::String(s) if s == "test"));
    }

    #[test]
    fn test_otlp_attribute_clone() {
        let a1 = OtlpAttribute::new("k", OtlpValue::int(1));
        let a2 = a1.clone();
        assert_eq!(a2.key, "k");
    }

    #[test]
    fn test_otlp_event_clone() {
        let e1 = OtlpEvent::new("evt", 1);
        let e2 = e1.clone();
        assert_eq!(e2.name, "evt");
    }

    #[test]
    fn test_otlp_span_clone() {
        let s1 = OtlpSpan::new("op");
        let s2 = s1.clone();
        assert_eq!(s2.name, "op");
    }

    #[test]
    fn test_stats_snapshot() {
        let exporter = OtlpExporter::default();
        exporter.enqueue(OtlpSpan::new("a"));
        exporter.enqueue(OtlpSpan::new("b"));
        exporter.drain_batch();

        let snap = exporter.stats();
        assert_eq!(snap.spans_queued, 2);
        assert_eq!(snap.batches_prepared, 1);
        assert_eq!(snap.spans_dropped, 0);
    }

    #[test]
    fn test_roundtrip_enqueue_drain() {
        let exporter = OtlpExporter::default();

        let mut span = OtlpSpan::new("operation");
        span.trace_id = [1; 16];
        span.span_id = [2; 8];
        span.status_code = OtlpStatusCode::Ok;

        exporter.enqueue(span.clone());
        let drained = exporter.drain_batch();

        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].name, "operation");
        assert_eq!(drained[0].trace_id, [1; 16]);
    }
}
