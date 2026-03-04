Add missing Rust doc comments to the observability.rs module for the claudefs-transport crate.

The file has #![warn(missing_docs)] enabled at the crate level, so ALL public items need doc comments.

Rules:
- Add `/// <doc comment>` immediately before each public item that lacks one
- Do NOT modify any existing code, logic, tests, or existing doc comments
- Do NOT add comments to private/internal items (only pub items)
- Keep doc comments concise and accurate to what the code does
- Output the COMPLETE file with ALL the added doc comments

Here is the current file content:

```rust
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
    Ok,
    Error,
    Timeout,
    Cancelled,
}

/// Severity level for events within a span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventSeverity {
    Debug,
    Info,
    Warn,
    Error,
}

/// A key-value attribute on a span or event.
#[derive(Debug, Clone)]
pub struct Attribute {
    pub key: String,
    pub value: AttributeValue,
}

impl Attribute {
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
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

impl AttributeValue {
    pub fn string(value: impl Into<String>) -> Self {
        Self::String(value.into())
    }

    pub fn int(value: i64) -> Self {
        Self::Int(value)
    }

    pub fn float(value: f64) -> Self {
        Self::Float(value)
    }

    pub fn bool(value: bool) -> Self {
        Self::Bool(value)
    }
}

/// An event recorded within a span.
#[derive(Debug, Clone)]
pub struct SpanEvent {
    pub name: String,
    pub severity: EventSeverity,
    pub timestamp_us: u64,
    pub attributes: Vec<Attribute>,
}

impl SpanEvent {
    pub fn new(name: impl Into<String>, severity: EventSeverity, timestamp_us: u64) -> Self {
        Self {
            name: name.into(),
            severity,
            timestamp_us,
            attributes: Vec::new(),
        }
    }

    pub fn with_attributes(mut self, attributes: Vec<Attribute>) -> Self {
        self.attributes = attributes;
        self
    }
}

/// A completed span with timing and metadata.
#[derive(Debug, Clone)]
pub struct Span {
    pub id: SpanId,
    pub parent_id: Option<SpanId>,
    pub name: String,
    pub status: SpanStatus,
    pub start_us: u64,
    pub end_us: u64,
    pub attributes: Vec<Attribute>,
    pub events: Vec<SpanEvent>,
}

impl Span {
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

    pub fn with_attributes(mut self, attributes: Vec<Attribute>) -> Self {
        self.attributes = attributes;
        self
    }

    pub fn add_event(&mut self, event: SpanEvent) {
        self.events.push(event);
    }

    pub fn duration_us(&self) -> u64 {
        self.end_us.saturating_sub(self.start_us)
    }
}

/// Configuration for observability.
#[derive(Debug, Clone)]
pub struct ObservabilityConfig {
    pub max_spans: usize,
    pub max_events_per_span: usize,
    pub max_attributes: usize,
    pub sample_rate: f64,
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
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            parent_id: None,
            attributes: Vec::new(),
            start_us: current_time_us(),
        }
    }

    pub fn parent(mut self, parent_id: SpanId) -> Self {
        self.parent_id = Some(parent_id);
        self
    }

    pub fn attribute(mut self, key: impl Into<String>, value: AttributeValue) -> Self {
        self.attributes.push(Attribute::new(key, value));
        self
    }

    pub fn string_attr(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes
            .push(Attribute::new(key, AttributeValue::string(value)));
        self
    }

    pub fn int_attr(mut self, key: impl Into<String>, value: i64) -> Self {
        self.attributes
            .push(Attribute::new(key, AttributeValue::int(value)));
        self
    }

    pub fn bool_attr(mut self, key: impl Into<String>, value: bool) -> Self {
        self.attributes
            .push(Attribute::new(key, AttributeValue::bool(value)));
        self
    }

    pub fn float_attr(mut self, key: impl Into<String>, value: f64) -> Self {
        self.attributes
            .push(Attribute::new(key, AttributeValue::float(value)));
        self
    }

    pub fn start_us(mut self, time_us: u64) -> Self {
        self.start_us = time_us;
        self
    }

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
    pub fn new() -> Self {
        Self {
            spans_created: AtomicU64::new(0),
            spans_completed: AtomicU64::new(0),
            spans_dropped: AtomicU64::new(0),
            events_recorded: AtomicU64::new(0),
            error_spans: AtomicU64::new(0),
        }
    }

    pub fn inc_spans_created(&self) {
        self.spans_created.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_spans_completed(&self) {
        self.spans_completed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_spans_dropped(&self) {
        self.spans_dropped.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_events_recorded(&self) {
        self.events_recorded.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_error_spans(&self) {
        self.error_spans.fetch_add(1, Ordering::Relaxed);
    }

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
    pub spans_created: u64,
    pub spans_completed: u64,
    pub spans_dropped: u64,
    pub events_recorded: u64,
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
    pub fn new(config: ObservabilityConfig) -> Self {
        Self {
            config,
            in_progress: Mutex::new(HashMap::new()),
            completed_spans: Mutex::new(Vec::new()),
            next_span_id: AtomicU64::new(1),
            stats: ObservabilityStats::new(),
        }
    }

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

    pub fn add_event(
        &self,
        span_id: SpanId,
        name: impl Into<String>,
        severity: EventSeverity,
    ) -> bool {
        self.add_event_with_attrs(span_id, name, severity, Vec::new())
    }

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

    pub fn get_span(&self, span_id: SpanId) -> Option<Span> {
        if let Ok(in_progress) = self.in_progress.lock() {
            if let Some(span) = in_progress.get(&span_id) {
                return Some(span.clone());
            }
        }

        None
    }

    pub fn drain_completed(&self) -> Vec<Span> {
        if let Ok(mut completed) = self.completed_spans.lock() {
            let drained: Vec<Span> = completed.drain(..).collect();
            return drained;
        }
        Vec::new()
    }

    pub fn completed_count(&self) -> usize {
        if let Ok(completed) = self.completed_spans.lock() {
            return completed.len();
        }
        0
    }

    pub fn stats(&self) -> ObservabilityStatsSnapshot {
        self.stats.snapshot()
    }
}
```

Output the COMPLETE updated observability.rs file with all missing doc comments added. Output ONLY the Rust source code, no markdown fences.
