//! Transport layer observability: distributed tracing spans, events, and metrics.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Unique span identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpanId(pub u64);

/// Span status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanStatus {
    /// Span completed successfully.
    Ok,
    /// Span encountered an error.
    Error,
    /// Span timed out.
    Timeout,
    /// Span was cancelled.
    Cancelled,
}

/// Severity level for events within a span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventSeverity {
    /// Debug-level event for detailed diagnostics.
    Debug,
    /// Informational event.
    Info,
    /// Warning event indicating potential issues.
    Warn,
    /// Error event indicating a failure.
    Error,
}

/// A key-value attribute on a span or event.
#[derive(Debug, Clone)]
pub struct Attribute {
    /// Attribute key name.
    pub key: String,
    /// Attribute value.
    pub value: AttributeValue,
}

impl Attribute {
    /// Creates a new attribute with the given key and value.
    pub fn new(key: impl Into<String>, value: AttributeValue) -> Self {
        Self {
            key: key.into(),
            value,
        }
    }
}

/// Value types for attributes.
#[derive(Debug, Clone)]
pub enum AttributeValue {
    /// String value.
    String(String),
    /// Integer value.
    Int(i64),
    /// Floating-point value.
    Float(f64),
    /// Boolean value.
    Bool(bool),
}

impl AttributeValue {
    /// Creates a string attribute value.
    pub fn string(value: impl Into<String>) -> Self {
        Self::String(value.into())
    }

    /// Creates an integer attribute value.
    pub fn int(value: i64) -> Self {
        Self::Int(value)
    }

    /// Creates a floating-point attribute value.
    pub fn float(value: f64) -> Self {
        Self::Float(value)
    }

    /// Creates a boolean attribute value.
    pub fn bool(value: bool) -> Self {
        Self::Bool(value)
    }
}

/// An event recorded within a span.
#[derive(Debug, Clone)]
pub struct SpanEvent {
    /// Event name.
    pub name: String,
    /// Event severity level.
    pub severity: EventSeverity,
    /// Event timestamp in microseconds since Unix epoch.
    pub timestamp_us: u64,
    /// Event attributes.
    pub attributes: Vec<Attribute>,
}

impl SpanEvent {
    /// Creates a new span event with the given name, severity, and timestamp.
    pub fn new(name: impl Into<String>, severity: EventSeverity, timestamp_us: u64) -> Self {
        Self {
            name: name.into(),
            severity,
            timestamp_us,
            attributes: Vec::new(),
        }
    }

    /// Adds attributes to the event.
    pub fn with_attributes(mut self, attributes: Vec<Attribute>) -> Self {
        self.attributes = attributes;
        self
    }
}

/// A completed span with timing and metadata.
#[derive(Debug, Clone)]
pub struct Span {
    /// Unique span identifier.
    pub id: SpanId,
    /// Parent span ID, if any.
    pub parent_id: Option<SpanId>,
    /// Span name.
    pub name: String,
    /// Span completion status.
    pub status: SpanStatus,
    /// Start timestamp in microseconds since Unix epoch.
    pub start_us: u64,
    /// End timestamp in microseconds since Unix epoch.
    pub end_us: u64,
    /// Span attributes.
    pub attributes: Vec<Attribute>,
    /// Events recorded within the span.
    pub events: Vec<SpanEvent>,
}

impl Span {
    /// Creates a new span with the given ID, optional parent, name, and start time.
    pub fn new(id: SpanId, parent_id: Option<SpanId>, name: String, start_us: u64) -> Self {
        Self {
            id,
            parent_id,
            name,
            status: SpanStatus::Ok,
            start_us,
            end_us: 0,
            attributes: Vec::new(),
            events: Vec::new(),
        }
    }

    /// Adds attributes to the span.
    pub fn with_attributes(mut self, attributes: Vec<Attribute>) -> Self {
        self.attributes = attributes;
        self
    }

    /// Adds an event to the span.
    pub fn add_event(&mut self, event: SpanEvent) {
        self.events.push(event);
    }

    /// Returns the span duration in microseconds.
    pub fn duration_us(&self) -> u64 {
        self.end_us.saturating_sub(self.start_us)
    }
}

/// Configuration for observability.
#[derive(Debug, Clone)]
pub struct ObservabilityConfig {
    /// Maximum number of completed spans to retain.
    pub max_spans: usize,
    /// Maximum events allowed per span.
    pub max_events_per_span: usize,
    /// Maximum attributes per event.
    pub max_attributes: usize,
    /// Sampling rate (0.0 to 1.0).
    pub sample_rate: f64,
    /// Whether observability is enabled.
    pub enabled: bool,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            max_spans: 4096,
            max_events_per_span: 64,
            max_attributes: 32,
            sample_rate: 1.0,
            enabled: true,
        }
    }
}

/// Builder for creating spans.
pub struct SpanBuilder {
    name: String,
    parent_id: Option<SpanId>,
    attributes: Vec<Attribute>,
    start_us: u64,
}

impl SpanBuilder {
    /// Creates a new span builder with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            parent_id: None,
            attributes: Vec::new(),
            start_us: current_time_us(),
        }
    }

    /// Sets the parent span ID.
    pub fn parent(mut self, parent_id: SpanId) -> Self {
        self.parent_id = Some(parent_id);
        self
    }

    /// Adds a typed attribute to the span.
    pub fn attribute(mut self, key: impl Into<String>, value: AttributeValue) -> Self {
        self.attributes.push(Attribute::new(key, value));
        self
    }

    /// Adds a string attribute to the span.
    pub fn string_attr(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes
            .push(Attribute::new(key, AttributeValue::string(value)));
        self
    }

    /// Adds an integer attribute to the span.
    pub fn int_attr(mut self, key: impl Into<String>, value: i64) -> Self {
        self.attributes
            .push(Attribute::new(key, AttributeValue::int(value)));
        self
    }

    /// Adds a boolean attribute to the span.
    pub fn bool_attr(mut self, key: impl Into<String>, value: bool) -> Self {
        self.attributes
            .push(Attribute::new(key, AttributeValue::bool(value)));
        self
    }

    /// Adds a float attribute to the span.
    pub fn float_attr(mut self, key: impl Into<String>, value: f64) -> Self {
        self.attributes
            .push(Attribute::new(key, AttributeValue::float(value)));
        self
    }

    /// Sets a custom start timestamp.
    pub fn start_us(mut self, time_us: u64) -> Self {
        self.start_us = time_us;
        self
    }

    /// Builds the span with the given ID.
    pub fn build(self, span_id: SpanId) -> Span {
        Span::new(span_id, self.parent_id, self.name, self.start_us)
            .with_attributes(self.attributes)
    }
}

fn current_time_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

/// Stats tracking for observability.
pub struct ObservabilityStats {
    spans_created: AtomicU64,
    spans_completed: AtomicU64,
    spans_dropped: AtomicU64,
    events_recorded: AtomicU64,
    error_spans: AtomicU64,
}

impl Default for ObservabilityStats {
    fn default() -> Self {
        Self::new()
    }
}

impl ObservabilityStats {
    /// Creates a new stats tracker with all counters initialized to zero.
    pub fn new() -> Self {
        Self {
            spans_created: AtomicU64::new(0),
            spans_completed: AtomicU64::new(0),
            spans_dropped: AtomicU64::new(0),
            events_recorded: AtomicU64::new(0),
            error_spans: AtomicU64::new(0),
        }
    }

    /// Increments the spans created counter.
    pub fn inc_spans_created(&self) {
        self.spans_created.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the spans completed counter.
    pub fn inc_spans_completed(&self) {
        self.spans_completed.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the spans dropped counter.
    pub fn inc_spans_dropped(&self) {
        self.spans_dropped.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the events recorded counter.
    pub fn inc_events_recorded(&self) {
        self.events_recorded.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the error spans counter.
    pub fn inc_error_spans(&self) {
        self.error_spans.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns a snapshot of the current stats.
    pub fn snapshot(&self) -> ObservabilityStatsSnapshot {
        ObservabilityStatsSnapshot {
            spans_created: self.spans_created.load(Ordering::Relaxed),
            spans_completed: self.spans_completed.load(Ordering::Relaxed),
            spans_dropped: self.spans_dropped.load(Ordering::Relaxed),
            events_recorded: self.events_recorded.load(Ordering::Relaxed),
            error_spans: self.error_spans.load(Ordering::Relaxed),
        }
    }
}

/// Snapshot of observability stats.
#[derive(Debug, Clone)]
pub struct ObservabilityStatsSnapshot {
    /// Total spans created.
    pub spans_created: u64,
    /// Total spans completed.
    pub spans_completed: u64,
    /// Total spans dropped due to buffer limits.
    pub spans_dropped: u64,
    /// Total events recorded.
    pub events_recorded: u64,
    /// Total spans with error status.
    pub error_spans: u64,
}

/// Active span collector.
pub struct SpanCollector {
    config: ObservabilityConfig,
    in_progress: Mutex<HashMap<SpanId, Span>>,
    completed_spans: Mutex<Vec<Span>>,
    next_span_id: AtomicU64,
    stats: ObservabilityStats,
}

impl SpanCollector {
    /// Creates a new span collector with the given configuration.
    pub fn new(config: ObservabilityConfig) -> Self {
        Self {
            config,
            in_progress: Mutex::new(HashMap::new()),
            completed_spans: Mutex::new(Vec::new()),
            next_span_id: AtomicU64::new(1),
            stats: ObservabilityStats::new(),
        }
    }

    /// Starts a new span from a builder, returning the span ID.
    pub fn start_span(&self, builder: SpanBuilder) -> SpanId {
        if !self.config.enabled {
            return SpanId(self.next_span_id.fetch_add(1, Ordering::Relaxed));
        }

        let span_id = SpanId(self.next_span_id.fetch_add(1, Ordering::Relaxed));
        let span = builder.build(span_id);

        if self.config.sample_rate < 1.0 {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            span_id.hash(&mut hasher);
            let hash = hasher.finish() as f64 / u64::MAX as f64;
            if hash >= self.config.sample_rate {
                return span_id;
            }
        }

        self.stats.inc_spans_created();

        if let Ok(mut in_progress) = self.in_progress.lock() {
            in_progress.insert(span_id, span);
        }

        span_id
    }

    /// Adds an event to a span without attributes.
    pub fn add_event(
        &self,
        span_id: SpanId,
        name: impl Into<String>,
        severity: EventSeverity,
    ) -> bool {
        self.add_event_with_attrs(span_id, name, severity, Vec::new())
    }

    /// Adds an event with attributes to a span.
    pub fn add_event_with_attrs(
        &self,
        span_id: SpanId,
        name: impl Into<String>,
        severity: EventSeverity,
        attrs: Vec<Attribute>,
    ) -> bool {
        if !self.config.enabled {
            return false;
        }

        self.stats.inc_events_recorded();

        if attrs.len() > self.config.max_attributes {
            return false;
        }

        let event = SpanEvent::new(name, severity, current_time_us()).with_attributes(attrs);

        if let Ok(mut in_progress) = self.in_progress.lock() {
            if let Some(span) = in_progress.get_mut(&span_id) {
                if span.events.len() < self.config.max_events_per_span {
                    span.add_event(event);
                    return true;
                }
            }
        }
        false
    }

    /// Ends a span with the given status.
    pub fn end_span(&self, span_id: SpanId, status: SpanStatus) -> bool {
        if !self.config.enabled {
            return true;
        }

        let mut completed_span: Option<Span> = None;

        if let Ok(mut in_progress) = self.in_progress.lock() {
            if let Some(mut span) = in_progress.remove(&span_id) {
                span.end_us = current_time_us();
                span.status = status;

                if status == SpanStatus::Error {
                    self.stats.inc_error_spans();
                }

                completed_span = Some(span);
            }
        }

        if let Some(span) = completed_span {
            self.stats.inc_spans_completed();

            if let Ok(mut completed) = self.completed_spans.lock() {
                if completed.len() >= self.config.max_spans {
                    completed.remove(0);
                    self.stats.inc_spans_dropped();
                }
                completed.push(span);
            }
            return true;
        }

        false
    }

    /// Returns a copy of an in-progress span, if it exists.
    pub fn get_span(&self, span_id: SpanId) -> Option<Span> {
        if let Ok(in_progress) = self.in_progress.lock() {
            if let Some(span) = in_progress.get(&span_id) {
                return Some(span.clone());
            }
        }

        None
    }

    /// Drains and returns all completed spans.
    pub fn drain_completed(&self) -> Vec<Span> {
        if let Ok(mut completed) = self.completed_spans.lock() {
            let drained: Vec<Span> = completed.drain(..).collect();
            return drained;
        }
        Vec::new()
    }

    /// Returns the count of completed spans currently held.
    pub fn completed_count(&self) -> usize {
        if let Ok(completed) = self.completed_spans.lock() {
            return completed.len();
        }
        0
    }

    /// Returns a snapshot of observability stats.
    pub fn stats(&self) -> ObservabilityStatsSnapshot {
        self.stats.snapshot()
    }
}

impl Default for SpanCollector {
    fn default() -> Self {
        Self::new(ObservabilityConfig::default())
    }
}

impl std::fmt::Debug for SpanCollector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpanCollector")
            .field("config", &self.config)
            .field("completed_count", &self.completed_count())
            .field("stats", &self.stats())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = ObservabilityConfig::default();
        assert_eq!(config.max_spans, 4096);
        assert_eq!(config.max_events_per_span, 64);
        assert_eq!(config.max_attributes, 32);
        assert_eq!(config.sample_rate, 1.0);
        assert!(config.enabled);
    }

    #[test]
    fn test_span_id() {
        let id1 = SpanId(1);
        let id2 = SpanId(1);
        let id3 = SpanId(2);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
        assert_eq!(id1.0, 1);
    }

    #[test]
    fn test_span_status_values() {
        let ok = SpanStatus::Ok;
        let error = SpanStatus::Error;
        let timeout = SpanStatus::Timeout;
        let cancelled = SpanStatus::Cancelled;

        assert_ne!(ok, error);
        assert_ne!(ok, timeout);
        assert_ne!(ok, cancelled);
        assert_ne!(error, timeout);
        assert_ne!(error, cancelled);
        assert_ne!(timeout, cancelled);
    }

    #[test]
    fn test_event_severity_ordering() {
        assert!(EventSeverity::Debug < EventSeverity::Info);
        assert!(EventSeverity::Info < EventSeverity::Warn);
        assert!(EventSeverity::Warn < EventSeverity::Error);
        assert!(EventSeverity::Debug < EventSeverity::Error);
    }

    #[test]
    fn test_attribute_string() {
        let attr = Attribute::new("key", AttributeValue::string("value"));
        assert_eq!(attr.key, "key");
        assert!(matches!(attr.value, AttributeValue::String(v) if v == "value"));
    }

    #[test]
    fn test_attribute_int() {
        let attr = Attribute::new("count", AttributeValue::int(42));
        assert_eq!(attr.key, "count");
        assert!(matches!(attr.value, AttributeValue::Int(v) if v == 42));
    }

    #[test]
    fn test_attribute_float() {
        let attr = Attribute::new("rate", AttributeValue::float(3.14));
        assert_eq!(attr.key, "rate");
        assert!(matches!(attr.value, AttributeValue::Float(v) if (v - 3.14).abs() < 0.001));
    }

    #[test]
    fn test_attribute_bool() {
        let attr = Attribute::new("enabled", AttributeValue::bool(true));
        assert_eq!(attr.key, "enabled");
        assert!(matches!(attr.value, AttributeValue::Bool(v) if v));
    }

    #[test]
    fn test_span_builder_basic() {
        let builder = SpanBuilder::new("test_span");
        let span_id = SpanId(123);
        let span = builder.build(span_id);

        assert_eq!(span.id, span_id);
        assert_eq!(span.name, "test_span");
        assert_eq!(span.parent_id, None);
    }

    #[test]
    fn test_span_builder_parent() {
        let parent_id = SpanId(100);
        let builder = SpanBuilder::new("child_span").parent(parent_id);
        let span = builder.build(SpanId(200));

        assert_eq!(span.parent_id, Some(parent_id));
    }

    #[test]
    fn test_span_builder_attributes() {
        let builder = SpanBuilder::new("test")
            .string_attr("name", "value")
            .int_attr("count", 5)
            .bool_attr("flag", true);
        let span = builder.build(SpanId(1));

        assert_eq!(span.attributes.len(), 3);
    }

    #[test]
    fn test_collector_start_span() {
        let collector = SpanCollector::new(ObservabilityConfig::default());
        let id = collector.start_span(SpanBuilder::new("test_span"));

        assert!(id.0 > 0);
        assert!(collector.get_span(id).is_some());
    }

    #[test]
    fn test_collector_end_span() {
        let collector = SpanCollector::new(ObservabilityConfig::default());
        let id = collector.start_span(SpanBuilder::new("test_span"));
        let result = collector.end_span(id, SpanStatus::Ok);

        assert!(result);
        assert!(collector.get_span(id).is_none());
    }

    #[test]
    fn test_collector_add_event() {
        let collector = SpanCollector::new(ObservabilityConfig::default());
        let id = collector.start_span(SpanBuilder::new("test_span"));

        let result = collector.add_event(id, "test_event", EventSeverity::Info);

        assert!(result);

        let span = collector.get_span(id).unwrap();
        assert_eq!(span.events.len(), 1);
        assert_eq!(span.events[0].name, "test_event");
        assert_eq!(span.events[0].severity, EventSeverity::Info);
    }

    #[test]
    fn test_collector_add_event_with_attrs() {
        let collector = SpanCollector::new(ObservabilityConfig::default());
        let id = collector.start_span(SpanBuilder::new("test_span"));

        let attrs = vec![
            Attribute::new("key", AttributeValue::string("value")),
            Attribute::new("count", AttributeValue::int(10)),
        ];

        let result =
            collector.add_event_with_attrs(id, "event_with_attrs", EventSeverity::Warn, attrs);

        assert!(result);

        let span = collector.get_span(id).unwrap();
        assert_eq!(span.events.len(), 1);
        assert_eq!(span.events[0].attributes.len(), 2);
    }

    #[test]
    fn test_collector_get_span() {
        let collector = SpanCollector::new(ObservabilityConfig::default());
        let id = collector.start_span(SpanBuilder::new("test_span"));

        let span = collector.get_span(id).expect("span should exist");
        assert_eq!(span.name, "test_span");

        let none = collector.get_span(SpanId(99999));
        assert!(none.is_none());
    }

    #[test]
    fn test_collector_drain_completed() {
        let config = ObservabilityConfig {
            max_spans: 10,
            ..Default::default()
        };
        let collector = SpanCollector::new(config);

        let id1 = collector.start_span(SpanBuilder::new("span1"));
        let id2 = collector.start_span(SpanBuilder::new("span2"));

        collector.end_span(id1, SpanStatus::Ok);
        collector.end_span(id2, SpanStatus::Ok);

        assert_eq!(collector.completed_count(), 2);

        let drained = collector.drain_completed();

        assert_eq!(drained.len(), 2);
        assert_eq!(collector.completed_count(), 0);
    }

    #[test]
    fn test_collector_parent_child() {
        let collector = SpanCollector::new(ObservabilityConfig::default());

        let parent_id = collector.start_span(SpanBuilder::new("parent"));

        let child_builder = SpanBuilder::new("child").parent(parent_id);
        let child_id = collector.start_span(child_builder);

        collector.end_span(child_id, SpanStatus::Ok);
        collector.end_span(parent_id, SpanStatus::Ok);

        let completed = collector.drain_completed();
        assert_eq!(completed.len(), 2);

        let child_span = completed.iter().find(|s| s.name == "child").unwrap();
        assert_eq!(child_span.parent_id, Some(parent_id));
    }

    #[test]
    fn test_collector_stats() {
        let collector = SpanCollector::new(ObservabilityConfig::default());

        let id1 = collector.start_span(SpanBuilder::new("span1"));
        let id2 = collector.start_span(SpanBuilder::new("span2"));

        collector.add_event(id1, "event1", EventSeverity::Info);

        collector.end_span(id1, SpanStatus::Error);
        collector.end_span(id2, SpanStatus::Ok);

        let stats = collector.stats();

        assert_eq!(stats.spans_created, 2);
        assert_eq!(stats.spans_completed, 2);
        assert_eq!(stats.events_recorded, 1);
        assert_eq!(stats.error_spans, 1);
    }

    #[test]
    fn test_collector_max_spans() {
        let config = ObservabilityConfig {
            max_spans: 2,
            ..Default::default()
        };
        let collector = SpanCollector::new(config);

        let id1 = collector.start_span(SpanBuilder::new("span1"));
        let id2 = collector.start_span(SpanBuilder::new("span2"));
        let id3 = collector.start_span(SpanBuilder::new("span3"));

        collector.end_span(id1, SpanStatus::Ok);
        collector.end_span(id2, SpanStatus::Ok);
        collector.end_span(id3, SpanStatus::Ok);

        let stats = collector.stats();
        assert_eq!(stats.spans_dropped, 1);

        let completed = collector.drain_completed();
        assert_eq!(completed.len(), 2);
    }

    #[test]
    fn test_collector_disabled() {
        let config = ObservabilityConfig {
            enabled: false,
            ..Default::default()
        };
        let collector = SpanCollector::new(config);

        let id = collector.start_span(SpanBuilder::new("test"));
        assert!(id.0 > 0);

        assert!(collector.get_span(id).is_none());

        collector.add_event(id, "event", EventSeverity::Info);

        let result = collector.end_span(id, SpanStatus::Ok);
        assert!(result);

        let stats = collector.stats();
        assert_eq!(stats.spans_created, 0);
    }

    #[test]
    fn test_span_timing() {
        let collector = SpanCollector::new(ObservabilityConfig::default());
        let id = collector.start_span(SpanBuilder::new("test"));

        std::thread::sleep(std::time::Duration::from_micros(100));

        collector.end_span(id, SpanStatus::Ok);

        let completed = collector.drain_completed();
        let span = &completed[0];

        assert!(span.end_us >= span.start_us);
        assert!(span.duration_us() >= 100);
    }

    #[test]
    fn test_collector_multiple_events() {
        let collector = SpanCollector::new(ObservabilityConfig::default());
        let id = collector.start_span(SpanBuilder::new("test"));

        for i in 0..5 {
            collector.add_event(id, format!("event_{}", i), EventSeverity::Info);
        }

        let span = collector.get_span(id).unwrap();
        assert_eq!(span.events.len(), 5);
    }

    #[test]
    fn test_span_builder_float_attr() {
        let builder = SpanBuilder::new("test").float_attr("pi", 3.14159);
        let span = builder.build(SpanId(1));

        assert_eq!(span.attributes.len(), 1);
        assert!(matches!(
            span.attributes[0].value,
            AttributeValue::Float(v) if (v - 3.14159).abs() < 0.0001
        ));
    }

    #[test]
    fn test_collector_end_nonexistent_span() {
        let collector = SpanCollector::new(ObservabilityConfig::default());
        let result = collector.end_span(SpanId(99999), SpanStatus::Ok);
        assert!(!result);
    }

    #[test]
    fn test_collector_events_limited_by_max() {
        let config = ObservabilityConfig {
            max_events_per_span: 2,
            ..Default::default()
        };
        let collector = SpanCollector::new(config);
        let id = collector.start_span(SpanBuilder::new("test"));

        collector.add_event(id, "event1", EventSeverity::Info);
        collector.add_event(id, "event2", EventSeverity::Info);
        let third_result = collector.add_event(id, "event3", EventSeverity::Info);

        assert!(!third_result);

        let span = collector.get_span(id).unwrap();
        assert_eq!(span.events.len(), 2);
    }

    #[test]
    fn test_collector_attributes_limited_by_max() {
        let config = ObservabilityConfig {
            max_attributes: 2,
            ..Default::default()
        };
        let collector = SpanCollector::new(config);
        let id = collector.start_span(SpanBuilder::new("test"));

        let attrs = vec![
            Attribute::new("a", AttributeValue::string("1")),
            Attribute::new("b", AttributeValue::string("2")),
            Attribute::new("c", AttributeValue::string("3")),
        ];

        let result = collector.add_event_with_attrs(id, "event", EventSeverity::Info, attrs);
        assert!(!result);
    }

    #[test]
    fn test_collector_status_tracking() {
        let collector = SpanCollector::new(ObservabilityConfig::default());

        let id_ok = collector.start_span(SpanBuilder::new("ok"));
        let id_err = collector.start_span(SpanBuilder::new("error"));
        let id_timeout = collector.start_span(SpanBuilder::new("timeout"));
        let id_cancelled = collector.start_span(SpanBuilder::new("cancelled"));

        collector.end_span(id_ok, SpanStatus::Ok);
        collector.end_span(id_err, SpanStatus::Error);
        collector.end_span(id_timeout, SpanStatus::Timeout);
        collector.end_span(id_cancelled, SpanStatus::Cancelled);

        let completed = collector.drain_completed();
        assert_eq!(completed.len(), 4);

        let error_count = completed
            .iter()
            .filter(|s| s.status == SpanStatus::Error)
            .count();
        assert_eq!(error_count, 1);

        let stats = collector.stats();
        assert_eq!(stats.error_spans, 1);
    }
}
