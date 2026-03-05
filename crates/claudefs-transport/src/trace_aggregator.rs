//! Trace aggregator for distributed tracing.
//!
//! Aggregates OTEL spans across the full distributed request path (client -> metadata -> storage),
//! computes critical path latency, and exports consolidated traces.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::otel::{OtlpAttribute, OtlpStatusCode};

/// 128-bit trace identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TraceId(pub [u8; 16]);

impl TraceId {
    /// Creates a new trace ID from bytes.
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Creates a new random trace ID.
    pub fn random() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let mut bytes = [0u8; 16];
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        bytes[0..8].copy_from_slice(&now.to_le_bytes());
        bytes[8..16].copy_from_slice(&(now.wrapping_mul(1103515245)).to_le_bytes());
        Self(bytes)
    }
}

impl Default for TraceId {
    fn default() -> Self {
        Self([0u8; 16])
    }
}

/// A single span record within a trace.
#[derive(Debug, Clone)]
pub struct SpanRecord {
    pub span_id: [u8; 8],
    pub parent_span_id: Option<[u8; 8]>,
    pub name: String,
    pub start_time_unix_nano: u64,
    pub end_time_unix_nano: u64,
    pub status: OtlpStatusCode,
    pub attributes: Vec<OtlpAttribute>,
}

impl SpanRecord {
    /// Creates a new span record.
    pub fn new(
        span_id: [u8; 8],
        parent_span_id: Option<[u8; 8]>,
        name: impl Into<String>,
        start_time_unix_nano: u64,
        end_time_unix_nano: u64,
    ) -> Self {
        Self {
            span_id,
            parent_span_id,
            name: name.into(),
            start_time_unix_nano,
            end_time_unix_nano,
            status: OtlpStatusCode::Unset,
            attributes: Vec::new(),
        }
    }

    /// Duration of the span in nanoseconds.
    pub fn duration_ns(&self) -> u64 {
        self.end_time_unix_nano
            .saturating_sub(self.start_time_unix_nano)
    }

    /// Sets the span status.
    pub fn with_status(mut self, status: OtlpStatusCode) -> Self {
        self.status = status;
        self
    }

    /// Sets span attributes.
    pub fn with_attributes(mut self, attrs: Vec<OtlpAttribute>) -> Self {
        self.attributes = attrs;
        self
    }
}

/// Complete trace data with all spans.
#[derive(Debug, Clone)]
pub struct TraceData {
    pub trace_id: TraceId,
    pub root_span_id: [u8; 8],
    pub spans: Vec<SpanRecord>,
    pub received_at_ns: u64,
}

impl TraceData {
    /// Computes latency statistics across all spans.
    pub fn latency_stats(&self) -> TraceLatencyStats {
        if self.spans.is_empty() {
            return TraceLatencyStats::default();
        }

        let durations: Vec<u64> = self.spans.iter().map(|s| s.duration_ns()).collect();
        let mut sorted = durations.clone();
        sorted.sort();

        let min = durations.iter().min().copied().unwrap_or(0);
        let max = durations.iter().max().copied().unwrap_or(0);
        let sum: u64 = durations.iter().sum();
        let mean = sum / durations.len() as u64;

        let p50_idx = (sorted.len() as f64 * 0.50) as usize;
        let p99_idx = (sorted.len() as f64 * 0.99) as usize;

        TraceLatencyStats {
            min_ns: min,
            max_ns: max,
            mean_ns: mean,
            p50_ns: sorted.get(p50_idx).copied().unwrap_or(mean),
            p99_ns: sorted.get(p99_idx).copied().unwrap_or(mean),
            span_count: self.spans.len(),
        }
    }

    /// Finds the root span (span with no parent).
    pub fn root_span(&self) -> Option<&SpanRecord> {
        self.spans.iter().find(|s| s.parent_span_id.is_none())
    }
}

/// Latency statistics for a trace.
#[derive(Debug, Clone, Default)]
pub struct TraceLatencyStats {
    pub min_ns: u64,
    pub max_ns: u64,
    pub mean_ns: u64,
    pub p50_ns: u64,
    pub p99_ns: u64,
    pub span_count: usize,
}

/// Configuration for trace aggregator.
#[derive(Debug, Clone)]
pub struct TraceAggregatorConfig {
    pub max_traces_in_flight: usize,
    pub trace_timeout_ms: u64,
    pub export_batch_size: usize,
    pub export_interval_ms: u64,
}

impl Default for TraceAggregatorConfig {
    fn default() -> Self {
        Self {
            max_traces_in_flight: 10_000,
            trace_timeout_ms: 30_000,
            export_batch_size: 100,
            export_interval_ms: 5_000,
        }
    }
}

/// Internal state for a trace.
struct TraceState {
    spans: VecDeque<SpanRecord>,
    created_at_ns: u64,
    root_span_id: Option<[u8; 8]>,
    completed: bool,
}

/// Atomic stats for trace aggregator.
struct TraceAggregatorStatsInner {
    traces_recorded: AtomicU64,
    traces_completed: AtomicU64,
    spans_recorded: AtomicU64,
    traces_timed_out: AtomicU64,
}

impl TraceAggregatorStatsInner {
    fn new() -> Self {
        Self {
            traces_recorded: AtomicU64::new(0),
            traces_completed: AtomicU64::new(0),
            spans_recorded: AtomicU64::new(0),
            traces_timed_out: AtomicU64::new(0),
        }
    }
}

/// Stats snapshot for trace aggregator.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TraceAggregatorStats {
    pub traces_recorded: u64,
    pub traces_completed: u64,
    pub spans_recorded: u64,
    pub traces_timed_out: u64,
    pub active_traces: usize,
}

impl TraceAggregatorStatsInner {
    fn snapshot(&self, active_count: usize) -> TraceAggregatorStats {
        TraceAggregatorStats {
            traces_recorded: self.traces_recorded.load(Ordering::Relaxed),
            traces_completed: self.traces_completed.load(Ordering::Relaxed),
            spans_recorded: self.spans_recorded.load(Ordering::Relaxed),
            traces_timed_out: self.traces_timed_out.load(Ordering::Relaxed),
            active_traces: active_count,
        }
    }
}

/// Trace aggregator for collecting and exporting distributed traces.
pub struct TraceAggregator {
    config: TraceAggregatorConfig,
    traces: Mutex<HashMap<TraceId, TraceState>>,
    stats: Arc<TraceAggregatorStatsInner>,
}

impl TraceAggregator {
    /// Creates a new trace aggregator with the given configuration.
    pub fn new(config: TraceAggregatorConfig) -> Self {
        Self {
            config,
            traces: Mutex::new(HashMap::new()),
            stats: Arc::new(TraceAggregatorStatsInner::new()),
        }
    }

    /// Records a span to an existing or new trace.
    pub fn record_span(&self, trace_id: TraceId, span: SpanRecord) {
        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let mut traces = match self.traces.lock() {
            Ok(t) => t,
            Err(_) => return,
        };

        if traces.len() >= self.config.max_traces_in_flight {
            self.stats.traces_timed_out.fetch_add(1, Ordering::Relaxed);
            return;
        }

        let is_new_trace = !traces.contains_key(&trace_id);
        if is_new_trace {
            self.stats.traces_recorded.fetch_add(1, Ordering::Relaxed);
        }

        let trace = traces.entry(trace_id).or_insert_with(|| TraceState {
            spans: VecDeque::new(),
            created_at_ns: now_ns,
            root_span_id: None,
            completed: false,
        });

        if span.parent_span_id.is_none() {
            trace.root_span_id = Some(span.span_id);
        }

        trace.spans.push_back(span);
        self.stats.spans_recorded.fetch_add(1, Ordering::Relaxed);
    }

    /// Marks a trace as completed and returns it.
    pub fn complete_trace(&self, trace_id: TraceId) -> Option<TraceData> {
        let mut traces = match self.traces.lock() {
            Ok(t) => t,
            Err(_) => return None,
        };

        let trace = traces.remove(&trace_id)?;
        self.stats.traces_completed.fetch_add(1, Ordering::Relaxed);

        let root_span_id = trace
            .root_span_id
            .unwrap_or_else(|| trace.spans.front().map(|s| s.span_id).unwrap_or([0u8; 8]));

        Some(TraceData {
            trace_id,
            root_span_id,
            spans: trace.spans.into_iter().collect(),
            received_at_ns: trace.created_at_ns,
        })
    }

    /// Returns a batch of completed (timed-out) traces for export.
    pub fn export_batch(&self) -> Vec<TraceData> {
        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let timeout_ns = self.config.trace_timeout_ms * 1_000_000;
        let mut traces = match self.traces.lock() {
            Ok(t) => t,
            Err(_) => return Vec::new(),
        };

        let mut exported = Vec::with_capacity(self.config.export_batch_size);

        let to_remove: Vec<TraceId> = traces
            .iter()
            .filter(|(_, t)| now_ns.saturating_sub(t.created_at_ns) > timeout_ns)
            .map(|(id, _)| id.clone())
            .take(self.config.export_batch_size)
            .collect();

        for id in to_remove {
            if let Some(trace) = traces.remove(&id) {
                self.stats.traces_completed.fetch_add(1, Ordering::Relaxed);
                self.stats.traces_timed_out.fetch_add(1, Ordering::Relaxed);

                let root_span_id = trace
                    .root_span_id
                    .unwrap_or_else(|| trace.spans.front().map(|s| s.span_id).unwrap_or([0u8; 8]));

                exported.push(TraceData {
                    trace_id: id,
                    root_span_id,
                    spans: trace.spans.into_iter().collect(),
                    received_at_ns: trace.created_at_ns,
                });
            }
        }

        exported
    }

    /// Returns current statistics.
    pub fn stats(&self) -> TraceAggregatorStats {
        let active = self.traces.lock().map(|t| t.len()).unwrap_or(0);
        self.stats.snapshot(active)
    }

    /// Returns the number of active traces.
    pub fn active_count(&self) -> usize {
        self.traces.lock().map(|t| t.len()).unwrap_or(0)
    }
}

impl Default for TraceAggregator {
    fn default() -> Self {
        Self::new(TraceAggregatorConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::otel::OtlpValue;

    fn make_span(id: u64, parent_id: Option<u64>, name: &str, start: u64, end: u64) -> SpanRecord {
        SpanRecord::new(
            id.to_le_bytes(),
            parent_id.map(|p| p.to_le_bytes()),
            name,
            start,
            end,
        )
    }

    #[test]
    fn test_trace_id_default() {
        let id = TraceId::default();
        assert_eq!(id.0, [0u8; 16]);
    }

    #[test]
    fn test_trace_id_random() {
        let id1 = TraceId::random();
        let id2 = TraceId::random();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_span_record_duration() {
        let span = SpanRecord::new([1; 8], None, "test", 1000, 2000);
        assert_eq!(span.duration_ns(), 1000);
    }

    #[test]
    fn test_span_record_with_status() {
        let span = SpanRecord::new([1; 8], None, "test", 0, 100).with_status(OtlpStatusCode::Error);
        assert_eq!(span.status, OtlpStatusCode::Error);
    }

    #[test]
    fn test_record_span_new_trace() {
        let agg = TraceAggregator::default();
        let trace_id = TraceId::random();
        let span = make_span(1, None, "root", 1000, 2000);

        agg.record_span(trace_id.clone(), span);

        let stats = agg.stats();
        assert_eq!(stats.traces_recorded, 1);
        assert_eq!(stats.spans_recorded, 1);
        assert_eq!(stats.active_traces, 1);
    }

    #[test]
    fn test_record_span_multiple_spans() {
        let agg = TraceAggregator::default();
        let trace_id = TraceId::random();

        agg.record_span(trace_id.clone(), make_span(1, None, "root", 1000, 2000));
        agg.record_span(trace_id.clone(), make_span(2, Some(1), "child", 1200, 1800));
        agg.record_span(
            trace_id.clone(),
            make_span(3, Some(1), "child2", 1300, 1500),
        );

        let stats = agg.stats();
        assert_eq!(stats.traces_recorded, 1);
        assert_eq!(stats.spans_recorded, 3);
    }

    #[test]
    fn test_complete_trace() {
        let agg = TraceAggregator::default();
        let trace_id = TraceId::random();

        agg.record_span(trace_id.clone(), make_span(1, None, "root", 1000, 2000));
        agg.record_span(trace_id.clone(), make_span(2, Some(1), "child", 1200, 1800));

        let trace = agg.complete_trace(trace_id.clone());
        assert!(trace.is_some());
        let trace = trace.unwrap();
        assert_eq!(trace.spans.len(), 2);
        assert_eq!(trace.root_span_id, 1u64.to_le_bytes());

        let stats = agg.stats();
        assert_eq!(stats.traces_completed, 1);
        assert_eq!(stats.active_traces, 0);
    }

    #[test]
    fn test_complete_nonexistent_trace() {
        let agg = TraceAggregator::default();
        let trace_id = TraceId::random();
        let result = agg.complete_trace(trace_id);
        assert!(result.is_none());
    }

    #[test]
    fn test_latency_stats_empty() {
        let trace = TraceData {
            trace_id: TraceId::default(),
            root_span_id: [0u8; 8],
            spans: vec![],
            received_at_ns: 0,
        };
        let stats = trace.latency_stats();
        assert_eq!(stats.span_count, 0);
    }

    #[test]
    fn test_latency_stats_single_span() {
        let trace = TraceData {
            trace_id: TraceId::default(),
            root_span_id: [1; 8],
            spans: vec![SpanRecord::new([1; 8], None, "op", 1000, 3000)],
            received_at_ns: 0,
        };
        let stats = trace.latency_stats();
        assert_eq!(stats.span_count, 1);
        assert_eq!(stats.min_ns, 2000);
        assert_eq!(stats.max_ns, 2000);
        assert_eq!(stats.mean_ns, 2000);
    }

    #[test]
    fn test_latency_stats_multiple_spans() {
        let trace = TraceData {
            trace_id: TraceId::default(),
            root_span_id: [1; 8],
            spans: vec![
                SpanRecord::new([1; 8], None, "root", 0, 100),
                SpanRecord::new([2; 8], Some([1; 8]), "child1", 10, 80),
                SpanRecord::new([3; 8], Some([1; 8]), "child2", 20, 50),
            ],
            received_at_ns: 0,
        };
        let stats = trace.latency_stats();
        assert_eq!(stats.span_count, 3);
        assert_eq!(stats.min_ns, 30);
        assert_eq!(stats.max_ns, 100);
    }

    #[test]
    fn test_root_span_finding() {
        let trace = TraceData {
            trace_id: TraceId::default(),
            root_span_id: [1; 8],
            spans: vec![
                SpanRecord::new([1; 8], None, "root", 0, 100),
                SpanRecord::new([2; 8], Some([1; 8]), "child", 10, 50),
            ],
            received_at_ns: 0,
        };
        let root = trace.root_span();
        assert!(root.is_some());
        assert_eq!(root.unwrap().name, "root");
    }

    #[test]
    fn test_export_batch_empty() {
        let agg = TraceAggregator::default();
        let batch = agg.export_batch();
        assert!(batch.is_empty());
    }

    #[test]
    fn test_config_defaults() {
        let config = TraceAggregatorConfig::default();
        assert_eq!(config.max_traces_in_flight, 10_000);
        assert_eq!(config.trace_timeout_ms, 30_000);
        assert_eq!(config.export_batch_size, 100);
        assert_eq!(config.export_interval_ms, 5_000);
    }

    #[test]
    fn test_concurrent_span_recording() {
        use std::sync::Arc;
        use std::thread;

        let agg = Arc::new(TraceAggregator::default());
        let mut handles = vec![];

        for i in 0..10 {
            let agg = Arc::clone(&agg);
            let handle = thread::spawn(move || {
                let trace_id = TraceId::random();
                for j in 0..100 {
                    let span = make_span(j, None, "span", i * 1000, i * 1000 + 100);
                    agg.record_span(trace_id.clone(), span);
                }
            });
            handles.push(handle);
        }

        for h in handles {
            h.join().unwrap();
        }

        let stats = agg.stats();
        assert_eq!(stats.traces_recorded, 10);
        assert_eq!(stats.spans_recorded, 1000);
    }

    #[test]
    fn test_span_attributes() {
        let attrs = vec![
            OtlpAttribute::new("key1", OtlpValue::string("value1")),
            OtlpAttribute::new("key2", OtlpValue::int(42)),
        ];
        let span = SpanRecord::new([1; 8], None, "test", 0, 100).with_attributes(attrs);
        assert_eq!(span.attributes.len(), 2);
    }

    #[test]
    fn test_trace_completion_on_timeout() {
        let config = TraceAggregatorConfig {
            trace_timeout_ms: 1,
            ..Default::default()
        };
        let agg = TraceAggregator::new(config);

        let trace_id = TraceId::random();
        agg.record_span(trace_id.clone(), make_span(1, None, "root", 0, 100));

        std::thread::sleep(std::time::Duration::from_millis(10));

        let batch = agg.export_batch();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].trace_id, trace_id);
    }

    #[test]
    fn test_max_traces_in_flight() {
        let config = TraceAggregatorConfig {
            max_traces_in_flight: 2,
            ..Default::default()
        };
        let agg = TraceAggregator::new(config);

        let trace1 = TraceId::random();
        let trace2 = TraceId::random();
        let trace3 = TraceId::random();

        agg.record_span(trace1, make_span(1, None, "root", 0, 100));
        agg.record_span(trace2, make_span(1, None, "root", 0, 100));
        agg.record_span(trace3, make_span(1, None, "root", 0, 100));

        let stats = agg.stats();
        assert_eq!(stats.traces_recorded, 2);
    }

    #[test]
    fn test_span_ordering_within_trace() {
        let agg = TraceAggregator::default();
        let trace_id = TraceId::random();

        agg.record_span(trace_id.clone(), make_span(3, Some(1), "child2", 30, 50));
        agg.record_span(trace_id.clone(), make_span(1, None, "root", 0, 100));
        agg.record_span(trace_id.clone(), make_span(2, Some(1), "child1", 10, 80));

        let trace = agg.complete_trace(trace_id);
        assert!(trace.is_some());
        let spans = &trace.unwrap().spans;
        assert_eq!(spans[0].name, "child2");
        assert_eq!(spans[1].name, "root");
        assert_eq!(spans[2].name, "child1");
    }

    #[test]
    fn test_trace_with_no_root_span() {
        let agg = TraceAggregator::default();
        let trace_id = TraceId::random();

        agg.record_span(trace_id.clone(), make_span(1, Some(999), "child", 10, 50));

        let trace = agg.complete_trace(trace_id);
        assert!(trace.is_some());
        assert_eq!(trace.unwrap().root_span_id, 1u64.to_le_bytes());
    }

    #[test]
    fn test_stats_serialization() {
        let stats = TraceAggregatorStats {
            traces_recorded: 100,
            traces_completed: 80,
            spans_recorded: 500,
            traces_timed_out: 20,
            active_traces: 5,
        };
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("traces_recorded"));
        let deserialized: TraceAggregatorStats = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.traces_recorded, 100);
    }
}
