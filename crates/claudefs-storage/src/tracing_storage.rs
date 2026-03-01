//! OpenTelemetry-compatible distributed tracing for storage operations.
//!
//! This module provides structured tracing and span tracking for all storage I/O operations,
//! with W3C traceparent propagation for distributed tracing across the FUSE → transport → storage pipeline.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use tracing::{debug, warn};

/// 128-bit trace identifier.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TraceId([u8; 16]);

impl TraceId {
    /// Creates a new TraceId from 16 bytes.
    pub fn new(bytes: [u8; 16]) -> Self {
        TraceId(bytes)
    }

    /// Returns a zero trace ID (all zeros).
    pub fn zero() -> Self {
        TraceId([0u8; 16])
    }
}

impl fmt::Display for TraceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", u128::from_be_bytes(self.0))
    }
}

impl fmt::Debug for TraceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TraceId({})", self)
    }
}

/// 64-bit span identifier.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpanId([u8; 8]);

impl SpanId {
    /// Creates a new SpanId from 8 bytes.
    pub fn new(bytes: [u8; 8]) -> Self {
        SpanId(bytes)
    }

    /// Returns a zero span ID (all zeros).
    pub fn zero() -> Self {
        SpanId([0u8; 8])
    }
}

impl fmt::Display for SpanId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", u64::from_be_bytes(self.0))
    }
}

impl fmt::Debug for SpanId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SpanId({})", self)
    }
}

/// Trace context containing trace and span identifiers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceContext {
    /// The trace ID this span belongs to.
    pub trace_id: TraceId,
    /// The span ID of this span.
    pub span_id: SpanId,
    /// The parent span ID if this is a child span.
    pub parent_span_id: Option<SpanId>,
    /// Whether this trace is being sampled.
    pub sampled: bool,
}

/// Storage operation types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StorageOp {
    /// Block read operation.
    BlockRead,
    /// Block write operation.
    BlockWrite,
    /// Block allocation.
    Allocate,
    /// Block deallocation.
    Free,
    /// Flush operation.
    Flush,
    /// Sync operation.
    Sync,
    /// Compaction operation.
    Compact,
    /// Scrubbing operation.
    Scrub,
    /// Snapshot creation.
    Snapshot,
    /// Recovery operation.
    Recover,
    /// Tiering operation.
    Tier,
    /// Encryption operation.
    Encrypt,
    /// Decryption operation.
    Decrypt,
}

impl fmt::Display for StorageOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            StorageOp::BlockRead => "BlockRead",
            StorageOp::BlockWrite => "BlockWrite",
            StorageOp::Allocate => "Allocate",
            StorageOp::Free => "Free",
            StorageOp::Flush => "Flush",
            StorageOp::Sync => "Sync",
            StorageOp::Compact => "Compact",
            StorageOp::Scrub => "Scrub",
            StorageOp::Snapshot => "Snapshot",
            StorageOp::Recover => "Recover",
            StorageOp::Tier => "Tier",
            StorageOp::Encrypt => "Encrypt",
            StorageOp::Decrypt => "Decrypt",
        };
        write!(f, "{}", s)
    }
}

/// Span status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SpanStatus {
    /// Span completed successfully.
    Ok,
    /// Span completed with an error.
    Error(String),
    /// Span was cancelled.
    Cancelled,
}

/// A storage span representing a single storage operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageSpan {
    /// The trace context for this span.
    pub context: TraceContext,
    /// The type of operation.
    pub operation: StorageOp,
    /// Start time in nanoseconds since epoch.
    pub start_time_ns: u64,
    /// End time in nanoseconds since epoch (None if still active).
    pub end_time_ns: Option<u64>,
    /// The final status of the span.
    pub status: SpanStatus,
    /// Additional attributes for this span.
    pub attributes: HashMap<String, String>,
    /// Number of blocks processed.
    pub block_count: u64,
    /// Number of bytes processed.
    pub bytes_processed: u64,
}

/// Configuration for tracing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracingConfig {
    /// Whether tracing is enabled.
    pub enabled: bool,
    /// Sample rate (0.0-1.0).
    pub sample_rate: f64,
    /// Maximum number of spans to keep in memory.
    pub max_spans: usize,
    /// Export interval in milliseconds.
    pub export_interval_ms: u64,
}

impl Default for TracingConfig {
    fn default() -> Self {
        TracingConfig {
            enabled: true,
            sample_rate: 1.0,
            max_spans: 10000,
            export_interval_ms: 5000,
        }
    }
}

/// Statistics for tracing.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TracingStats {
    /// Total spans created.
    pub total_spans_created: u64,
    /// Total spans completed.
    pub total_spans_completed: u64,
    /// Total spans dropped due to capacity limits.
    pub total_spans_dropped: u64,
    /// Currently active span count.
    pub active_span_count: usize,
    /// Average span duration in nanoseconds.
    pub avg_span_duration_ns: u64,
}

/// W3C traceparent header parser and formatter.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct W3CTraceparent {
    /// Traceparent version (must be 00).
    pub version: u8,
    /// 128-bit trace ID.
    pub trace_id: TraceId,
    /// 64-bit parent span ID.
    pub parent_id: SpanId,
    /// Trace flags (bit 0 = sampled).
    pub flags: u8,
}

impl W3CTraceparent {
    /// Parses a W3C traceparent header.
    ///
    /// Format: 00-{trace_id}-{parent_id}-{flags}
    pub fn parse(header: &str) -> Option<Self> {
        let parts: Vec<&str> = header.split('-').collect();
        if parts.len() != 4 {
            return None;
        }

        let version = u8::from_str_radix(parts[0], 16).ok()?;
        if version != 0 {
            return None;
        }

        let trace_id_bytes = parse_hex_bytes_16(parts[1])?;
        let trace_id = TraceId::new(trace_id_bytes);

        let parent_id_bytes = parse_hex_bytes_8(parts[2])?;
        let parent_id = SpanId::new(parent_id_bytes);

        let flags = u8::from_str_radix(parts[3], 16).ok()?;

        Some(W3CTraceparent {
            version,
            trace_id,
            parent_id,
            flags,
        })
    }

    /// Formats this traceparent as a W3C traceparent string.
    pub fn format(&self) -> String {
        format!(
            "00-{:032x}-{:016x}-{:02x}",
            u128::from_be_bytes(self.trace_id.0),
            u64::from_be_bytes(self.parent_id.0),
            self.flags
        )
    }
}

fn parse_hex_bytes_16(s: &str) -> Option<[u8; 16]> {
    if s.len() != 32 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        let chunk_str = std::str::from_utf8(chunk).ok()?;
        bytes[i] = u8::from_str_radix(chunk_str, 16).ok()?;
    }
    Some(bytes)
}

fn parse_hex_bytes_8(s: &str) -> Option<[u8; 8]> {
    if s.len() != 16 {
        return None;
    }
    let mut bytes = [0u8; 8];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        let chunk_str = std::str::from_utf8(chunk).ok()?;
        bytes[i] = u8::from_str_radix(chunk_str, 16).ok()?;
    }
    Some(bytes)
}

/// Storage tracer for managing spans.
pub struct StorageTracer {
    /// Configuration for the tracer.
    config: TracingConfig,
    /// Currently active spans.
    active_spans: HashMap<SpanId, StorageSpan>,
    /// Completed spans ready for export.
    completed_spans: Vec<StorageSpan>,
    /// Counter for generating span IDs.
    span_counter: u64,
    /// Total spans created.
    total_spans_created: u64,
    /// Total spans completed.
    total_spans_completed: u64,
    /// Total spans dropped.
    total_spans_dropped: u64,
    /// Sum of all span durations for averaging.
    total_duration_ns: u64,
}

impl StorageTracer {
    /// Creates a new storage tracer with the given configuration.
    pub fn new(config: TracingConfig) -> Self {
        debug!("Initializing storage tracer with config: {:?}", config);
        StorageTracer {
            config,
            active_spans: HashMap::new(),
            completed_spans: Vec::new(),
            span_counter: 0,
            total_spans_created: 0,
            total_spans_completed: 0,
            total_spans_dropped: 0,
            total_duration_ns: 0,
        }
    }

    /// Starts a new span for the given operation.
    ///
    /// If parent is None, creates a new root trace.
    /// If parent is Some, creates a child span with the parent's trace ID.
    ///
    /// Returns the trace context for the new span.
    /// If sampling decides not to sample this trace, the context will have sampled=false.
    pub fn start_span(&mut self, op: StorageOp, parent: Option<&TraceContext>) -> TraceContext {
        let should_sample = self.config.enabled
            && (self.config.sample_rate >= 1.0 || rand_simple() < self.config.sample_rate);

        let (trace_id, parent_span_id) = if let Some(p) = parent {
            (p.trace_id, Some(p.span_id))
        } else {
            (TraceId::new(rand_bytes_16()), None)
        };

        let span_id = SpanId::new(rand_bytes_8());
        self.span_counter += 1;
        self.total_spans_created += 1;

        if should_sample {
            if self.active_spans.len() >= self.config.max_spans {
                self.total_spans_dropped += 1;
                warn!(
                    "Max spans limit reached ({}), dropping span",
                    self.config.max_spans
                );
            } else {
                let start_time_ns = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos() as u64;

                let span = StorageSpan {
                    context: TraceContext {
                        trace_id,
                        span_id,
                        parent_span_id,
                        sampled: true,
                    },
                    operation: op,
                    start_time_ns,
                    end_time_ns: None,
                    status: SpanStatus::Ok,
                    attributes: HashMap::new(),
                    block_count: 0,
                    bytes_processed: 0,
                };
                self.active_spans.insert(span_id, span);
            }
        }

        debug!(
            "Started span: {:?} with trace_id={}, span_id={}, sampled={}",
            op, trace_id, span_id, should_sample
        );

        TraceContext {
            trace_id,
            span_id,
            parent_span_id,
            sampled: should_sample,
        }
    }

    /// Ends a span with the given status and bytes processed.
    ///
    /// Returns the completed span if found, None otherwise.
    pub fn end_span(
        &mut self,
        span_id: SpanId,
        status: SpanStatus,
        bytes_processed: u64,
    ) -> Option<StorageSpan> {
        let span = self.active_spans.remove(&span_id)?;

        let end_time_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        let duration = end_time_ns.saturating_sub(span.start_time_ns);
        self.total_duration_ns += duration;
        self.total_spans_completed += 1;

        let mut completed = span;
        completed.end_time_ns = Some(end_time_ns);
        completed.status = status;
        completed.bytes_processed = bytes_processed;

        debug!(
            "Ended span: {:?}, duration={}ns",
            completed.operation, duration
        );

        self.completed_spans.push(completed.clone());
        Some(completed)
    }

    /// Adds an attribute to an active span.
    pub fn add_attribute(&mut self, span_id: SpanId, key: String, value: String) {
        if let Some(span) = self.active_spans.get_mut(&span_id) {
            let key_ref = key.clone();
            let value_ref = value.clone();
            span.attributes.insert(key, value);
            debug!(
                "Added attribute to span {:?}: {}={}",
                span_id, key_ref, value_ref
            );
        }
    }

    /// Sets the block count for an active span.
    pub fn set_block_count(&mut self, span_id: SpanId, count: u64) {
        if let Some(span) = self.active_spans.get_mut(&span_id) {
            span.block_count = count;
            debug!("Set block count for span {:?}: {}", span_id, count);
        }
    }

    /// Returns a list of currently active spans.
    pub fn active_spans(&self) -> Vec<&StorageSpan> {
        self.active_spans.values().collect()
    }

    /// Drains completed spans for export.
    ///
    /// This removes all completed spans from the internal buffer.
    pub fn drain_completed(&mut self) -> Vec<StorageSpan> {
        debug!("Draining {} completed spans", self.completed_spans.len());
        std::mem::take(&mut self.completed_spans)
    }

    /// Returns tracing statistics.
    pub fn stats(&self) -> TracingStats {
        let avg_duration = if self.total_spans_completed > 0 {
            self.total_duration_ns / self.total_spans_completed
        } else {
            0
        };

        TracingStats {
            total_spans_created: self.total_spans_created,
            total_spans_completed: self.total_spans_completed,
            total_spans_dropped: self.total_spans_dropped,
            active_span_count: self.active_spans.len(),
            avg_span_duration_ns: avg_duration,
        }
    }
}

fn rand_simple() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    (nanos as f64) / (u32::MAX as f64 + 1.0)
}

fn rand_bytes_16() -> [u8; 16] {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    let mut bytes = [0u8; 16];
    let mut state = seed;
    for byte in bytes.iter_mut() {
        state = state.wrapping_mul(1103515245).wrapping_add(12345);
        *byte = (state >> 16) as u8;
    }
    bytes
}

fn rand_bytes_8() -> [u8; 8] {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    let mut bytes = [0u8; 8];
    let mut state = seed;
    for byte in bytes.iter_mut() {
        state = state.wrapping_mul(1103515245).wrapping_add(12345);
        *byte = (state >> 16) as u8;
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_id_display() {
        let bytes = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
            0x0f, 0x10,
        ];
        let trace_id = TraceId::new(bytes);
        let expected = "0102030405060708090a0b0c0d0e0f10";
        assert_eq!(format!("{}", trace_id), expected);
    }

    #[test]
    fn test_trace_id_zero() {
        let trace_id = TraceId::zero();
        assert_eq!(format!("{}", trace_id), "00000000000000000000000000000000");
    }

    #[test]
    fn test_span_id_display() {
        let bytes = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let span_id = SpanId::new(bytes);
        let expected = "0102030405060708";
        assert_eq!(format!("{}", span_id), expected);
    }

    #[test]
    fn test_span_id_zero() {
        let span_id = SpanId::zero();
        assert_eq!(format!("{}", span_id), "0000000000000000");
    }

    #[test]
    fn test_w3c_traceparent_parse() {
        let header = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";
        let parsed = W3CTraceparent::parse(header);
        assert!(parsed.is_some());
        let tp = parsed.unwrap();
        assert_eq!(tp.version, 0);
        assert_eq!(tp.flags, 0x01);
    }

    #[test]
    fn test_w3c_traceparent_parse_invalid() {
        assert!(W3CTraceparent::parse("invalid").is_none());
        assert!(W3CTraceparent::parse("00-abc-bad").is_none());
        assert!(
            W3CTraceparent::parse("01-00000000000000000000000000000000-0000000000000000-00")
                .is_none()
        );
    }

    #[test]
    fn test_w3c_traceparent_format() {
        let trace_id = TraceId::new([
            0x0a, 0xf7, 0x65, 0x19, 0x16, 0xcd, 0x43, 0xdd, 0x84, 0x48, 0xeb, 0x21, 0x1c, 0x80,
            0x31, 0x9c,
        ]);
        let parent_id = SpanId::new([0xb7, 0xad, 0x6b, 0x71, 0x69, 0x20, 0x33, 0x31]);
        let tp = W3CTraceparent {
            version: 0,
            trace_id,
            parent_id,
            flags: 0x01,
        };
        let formatted = tp.format();
        assert_eq!(
            formatted,
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"
        );
    }

    #[test]
    fn test_storage_op_variants() {
        let _ = StorageOp::BlockRead;
        let _ = StorageOp::BlockWrite;
        let _ = StorageOp::Allocate;
        let _ = StorageOp::Free;
        let _ = StorageOp::Flush;
        let _ = StorageOp::Sync;
        let _ = StorageOp::Compact;
        let _ = StorageOp::Scrub;
        let _ = StorageOp::Snapshot;
        let _ = StorageOp::Recover;
        let _ = StorageOp::Tier;
        let _ = StorageOp::Encrypt;
        let _ = StorageOp::Decrypt;
    }

    #[test]
    fn test_tracing_config_defaults() {
        let config = TracingConfig::default();
        assert!(config.enabled);
        assert_eq!(config.sample_rate, 1.0);
        assert_eq!(config.max_spans, 10000);
        assert_eq!(config.export_interval_ms, 5000);
    }

    #[test]
    fn test_start_span_no_parent() {
        let config = TracingConfig::default();
        let mut tracer = StorageTracer::new(config);

        let ctx = tracer.start_span(StorageOp::BlockRead, None);

        assert_eq!(ctx.parent_span_id, None);
        assert!(ctx.sampled);

        let active = tracer.active_spans();
        assert_eq!(active.len(), 1);
    }

    #[test]
    fn test_start_span_with_parent() {
        let config = TracingConfig::default();
        let mut tracer = StorageTracer::new(config);

        let parent_ctx = tracer.start_span(StorageOp::BlockRead, None);

        let child_ctx = tracer.start_span(StorageOp::BlockWrite, Some(&parent_ctx));

        assert_eq!(child_ctx.trace_id, parent_ctx.trace_id);
        assert_eq!(child_ctx.parent_span_id, Some(parent_ctx.span_id));

        let active = tracer.active_spans();
        assert_eq!(active.len(), 2);
    }

    #[test]
    fn test_end_span_ok() {
        let config = TracingConfig::default();
        let mut tracer = StorageTracer::new(config);

        let ctx = tracer.start_span(StorageOp::BlockRead, None);
        let span_id = ctx.span_id;

        let completed = tracer.end_span(span_id, SpanStatus::Ok, 4096);

        assert!(completed.is_some());
        assert_eq!(completed.unwrap().status, SpanStatus::Ok);

        let stats = tracer.stats();
        assert_eq!(stats.total_spans_completed, 1);
        assert_eq!(stats.active_span_count, 0);
    }

    #[test]
    fn test_end_span_error() {
        let config = TracingConfig::default();
        let mut tracer = StorageTracer::new(config);

        let ctx = tracer.start_span(StorageOp::BlockWrite, None);
        let span_id = ctx.span_id;

        let completed = tracer.end_span(span_id, SpanStatus::Error("IO error".to_string()), 0);

        assert!(completed.is_some());
        let span = completed.unwrap();
        assert!(matches!(span.status, SpanStatus::Error(msg) if msg == "IO error"));
    }

    #[test]
    fn test_end_span_unknown() {
        let config = TracingConfig::default();
        let mut tracer = StorageTracer::new(config);

        let result = tracer.end_span(SpanId::zero(), SpanStatus::Ok, 0);

        assert!(result.is_none());
    }

    #[test]
    fn test_add_attribute() {
        let config = TracingConfig::default();
        let mut tracer = StorageTracer::new(config);

        let ctx = tracer.start_span(StorageOp::BlockRead, None);
        tracer.add_attribute(ctx.span_id, "device".to_string(), "nvme0n1".to_string());

        let active = tracer.active_spans();
        assert_eq!(
            active[0].attributes.get("device"),
            Some(&"nvme0n1".to_string())
        );
    }

    #[test]
    fn test_set_block_count() {
        let config = TracingConfig::default();
        let mut tracer = StorageTracer::new(config);

        let ctx = tracer.start_span(StorageOp::BlockRead, None);
        tracer.set_block_count(ctx.span_id, 16);

        let active = tracer.active_spans();
        assert_eq!(active[0].block_count, 16);
    }

    #[test]
    fn test_drain_completed() {
        let config = TracingConfig::default();
        let mut tracer = StorageTracer::new(config);

        let ctx = tracer.start_span(StorageOp::BlockRead, None);
        tracer.end_span(ctx.span_id, SpanStatus::Ok, 4096);

        let drained = tracer.drain_completed();

        assert_eq!(drained.len(), 1);
        assert_eq!(tracer.drain_completed().len(), 0);
    }

    #[test]
    fn test_drain_completed_empty() {
        let config = TracingConfig::default();
        let mut tracer = StorageTracer::new(config);

        let drained = tracer.drain_completed();

        assert!(drained.is_empty());
    }

    #[test]
    fn test_active_spans() {
        let config = TracingConfig::default();
        let mut tracer = StorageTracer::new(config);

        let _ = tracer.start_span(StorageOp::BlockRead, None);
        let _ = tracer.start_span(StorageOp::BlockWrite, None);

        let ctx = tracer.start_span(StorageOp::Allocate, None);
        tracer.end_span(ctx.span_id, SpanStatus::Ok, 0);

        let active = tracer.active_spans();
        assert_eq!(active.len(), 2);
    }

    #[test]
    fn test_sample_rate_zero() {
        let mut config = TracingConfig::default();
        config.sample_rate = 0.0;
        let mut tracer = StorageTracer::new(config);

        let _ = tracer.start_span(StorageOp::BlockRead, None);

        assert_eq!(tracer.active_spans().len(), 0);

        let stats = tracer.stats();
        assert_eq!(stats.total_spans_created, 1);
        assert_eq!(stats.active_span_count, 0);
    }

    #[test]
    fn test_sample_rate_full() {
        let config = TracingConfig::default();
        let mut tracer = StorageTracer::new(config);

        let _ = tracer.start_span(StorageOp::BlockRead, None);

        assert_eq!(tracer.active_spans().len(), 1);
    }

    #[test]
    fn test_max_spans_limit() {
        let mut config = TracingConfig::default();
        config.max_spans = 2;
        let mut tracer = StorageTracer::new(config);

        let _ctx1 = tracer.start_span(StorageOp::BlockRead, None);
        let _ctx2 = tracer.start_span(StorageOp::BlockWrite, None);
        let ctx3 = tracer.start_span(StorageOp::Allocate, None);
        let _ctx4 = tracer.start_span(StorageOp::Free, None);

        let stats = tracer.stats();
        assert!(stats.total_spans_dropped >= 1);
        assert_eq!(stats.active_span_count, 2);
        let _ = tracer.end_span(ctx3.span_id, SpanStatus::Ok, 0);
    }

    #[test]
    fn test_stats() {
        let config = TracingConfig::default();
        let mut tracer = StorageTracer::new(config);

        let ctx = tracer.start_span(StorageOp::BlockRead, None);
        tracer.end_span(ctx.span_id, SpanStatus::Ok, 4096);

        let ctx2 = tracer.start_span(StorageOp::BlockWrite, None);
        tracer.end_span(ctx2.span_id, SpanStatus::Error("test".to_string()), 8192);

        let stats = tracer.stats();

        assert_eq!(stats.total_spans_created, 2);
        assert_eq!(stats.total_spans_completed, 2);
        assert_eq!(stats.active_span_count, 0);
    }

    #[test]
    fn test_bytes_processed_tracking() {
        let config = TracingConfig::default();
        let mut tracer = StorageTracer::new(config);

        let ctx = tracer.start_span(StorageOp::BlockRead, None);
        let completed = tracer.end_span(ctx.span_id, SpanStatus::Ok, 8192);

        assert_eq!(completed.unwrap().bytes_processed, 8192);
    }
}
