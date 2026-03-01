//! W3C Trace Context support for distributed tracing.
//!
//! Implements the W3C Trace Context specification for propagating
//! trace context across service boundaries.

use std::str::FromStr;

/// HTTP header name for traceparent per W3C specification.
pub const TRACEPARENT_HEADER: &str = "traceparent";
/// HTTP header name for tracestate per W3C specification.
pub const TRACESTATE_HEADER: &str = "tracestate";

/// Current traceparent version (must be 0 per W3C spec).
const TRACEPARENT_VERSION: u8 = 0;
/// Hex string representation of traceparent version.
const TRACEPARENT_VERSION_HEX: &str = "00";
/// Delimiter between traceparent fields.
const TRACEPARENT_DELIMITER: &str = "-";
/// Expected length of a valid traceparent header value.
const TRACEPARENT_LENGTH: usize = 55;

/// W3C Trace Flags containing trace options.
///
/// The only defined flag is the sampled flag (bit 0). When set, it indicates
/// that the trace should be sampled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceFlags(u8);

impl TraceFlags {
    /// No flags set - trace is not sampled.
    pub const NONE: TraceFlags = TraceFlags(0);
    /// Sampled flag - trace should be sampled.
    pub const TRACE_FLAG: TraceFlags = TraceFlags(1);

    /// Returns true if the sampled flag is set.
    pub fn sampled(self) -> bool {
        self.0 & Self::TRACE_FLAG.0 != 0
    }

    /// Returns a new TraceFlags with the sampled flag set to the given value.
    pub fn with_sampled(self, sampled: bool) -> Self {
        if sampled {
            TraceFlags(self.0 | Self::TRACE_FLAG.0)
        } else {
            TraceFlags(self.0 & !Self::TRACE_FLAG.0)
        }
    }
}

impl Default for TraceFlags {
    fn default() -> Self {
        Self::NONE
    }
}

impl std::fmt::Display for TraceFlags {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:02x}", self.0)
    }
}

impl FromStr for TraceFlags {
    type Err = <u8 as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        u8::from_str_radix(s, 16).map(TraceFlags)
    }
}

/// 128-bit trace identifier.
///
/// Globally unique identifier for the trace. The first bit must be 0
/// to comply with W3C specification.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TraceId([u8; 16]);

impl TraceId {
    /// Creates a new random TraceId.
    ///
    /// The first byte is masked to ensure the first bit is 0.
    pub fn new() -> Self {
        let mut bytes = [0u8; 16];
        getrandom::getrandom(&mut bytes).ok();
        bytes[0] &= 0x7F;
        TraceId(bytes)
    }

    /// Creates a TraceId from raw bytes.
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        TraceId(bytes)
    }

    /// Parses a TraceId from a 32-character hex string.
    ///
    /// Returns None if the string is not exactly 32 characters
    /// or contains invalid hex characters.
    pub fn from_hex(hex: &str) -> Option<Self> {
        if hex.len() != 32 {
            return None;
        }
        let mut bytes = [0u8; 16];
        for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
            let s = std::str::from_utf8(chunk).ok()?;
            bytes[i] = u8::from_str_radix(s, 16).ok()?;
        }
        Some(TraceId(bytes))
    }

    /// Returns the raw bytes of this TraceId.
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// Returns true if this TraceId is all zeros (invalid).
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|&b| b == 0)
    }

    /// Converts the TraceId to a 32-character hex string.
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

impl std::fmt::Display for TraceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// 64-bit span identifier.
///
/// Identifies a specific span within a trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpanId([u8; 8]);

impl SpanId {
    /// Creates a new random SpanId.
    pub fn new() -> Self {
        let mut bytes = [0u8; 8];
        getrandom::getrandom(&mut bytes).ok();
        SpanId(bytes)
    }

    /// Creates a SpanId from raw bytes.
    pub fn from_bytes(bytes: [u8; 8]) -> Self {
        SpanId(bytes)
    }

    /// Parses a SpanId from a 16-character hex string.
    ///
    /// Returns None if the string is not exactly 16 characters
    /// or contains invalid hex characters.
    pub fn from_hex(hex: &str) -> Option<Self> {
        if hex.len() != 16 {
            return None;
        }
        let mut bytes = [0u8; 8];
        for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
            let s = std::str::from_utf8(chunk).ok()?;
            bytes[i] = u8::from_str_radix(s, 16).ok()?;
        }
        Some(SpanId(bytes))
    }

    /// Returns the raw bytes of this SpanId.
    pub fn as_bytes(&self) -> &[u8; 8] {
        &self.0
    }

    /// Converts the SpanId to a 16-character hex string.
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

impl std::fmt::Display for SpanId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// W3C TraceParent header value.
///
/// Format: version-trace_id-parent_id-trace_flags
/// Example: 00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01
///
/// The traceparent header identifies the trace and the current span.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TraceParent {
    /// Protocol version (must be 0).
    pub version: u8,
    /// 128-bit trace identifier.
    pub trace_id: TraceId,
    /// 64-bit identifier of the parent span.
    pub parent_id: SpanId,
    /// Trace flags (bit 0 = sampled).
    pub flags: TraceFlags,
}

impl TraceParent {
    /// Creates a new TraceParent with the given trace ID, parent ID, and flags.
    pub fn new(trace_id: TraceId, parent_id: SpanId, flags: TraceFlags) -> Self {
        Self {
            version: TRACEPARENT_VERSION,
            trace_id,
            parent_id,
            flags,
        }
    }

    /// Creates a new child span within the same trace.
    ///
    /// The trace_id is preserved, but a new random parent_id is generated.
    pub fn new_child(&self) -> Self {
        Self {
            version: TRACEPARENT_VERSION,
            trace_id: self.trace_id.clone(),
            parent_id: SpanId::new(),
            flags: self.flags,
        }
    }

    /// Parses a TraceParent from a traceparent header value string.
    ///
    /// Returns None if the string is not valid per W3C specification.
    pub fn parse_traceparent(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.len() != TRACEPARENT_LENGTH {
            return None;
        }

        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 4 {
            return None;
        }

        let version = u8::from_str_radix(parts[0], 16).ok()?;
        if version != TRACEPARENT_VERSION {
            return None;
        }

        let trace_id = TraceId::from_hex(parts[1])?;
        let parent_id = SpanId::from_hex(parts[2])?;
        let flags = parts[3].parse().ok()?;

        Some(TraceParent {
            version,
            trace_id,
            parent_id,
            flags,
        })
    }
}

impl std::fmt::Display for TraceParent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}-{}-{}{}{}",
            TRACEPARENT_VERSION_HEX,
            self.trace_id,
            self.parent_id,
            TRACEPARENT_DELIMITER,
            self.flags
        )
    }
}

/// W3C TraceState header value.
///
/// Contains vendor-specific trace information as key-value pairs.
/// The order of entries matters and should be preserved.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TraceState(std::collections::HashMap<String, String>);

impl TraceState {
    /// Creates a new empty TraceState.
    pub fn new() -> Self {
        TraceState(std::collections::HashMap::new())
    }

    /// Parses a TraceState from a tracestate header value string.
    pub fn from_header(header: &str) -> Self {
        let mut map = std::collections::HashMap::new();
        for entry in header.split(',') {
            let entry = entry.trim();
            if let Some((key, value)) = entry.split_once('=') {
                let key = key.trim();
                let value = value.trim();
                if !key.is_empty() && !value.contains(',') {
                    map.insert(key.to_string(), value.to_string());
                }
            }
        }
        TraceState(map)
    }

    /// Inserts a key-value pair into the TraceState.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.0.insert(key.into(), value.into());
    }

    /// Gets a value from the TraceState by key.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(|s| s.as_str())
    }

    /// Removes a key from the TraceState.
    pub fn remove(&mut self, key: &str) {
        self.0.remove(key);
    }
}

impl std::fmt::Display for TraceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            self.0
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}

/// Combined W3C Trace Context containing both traceparent and tracestate.
///
/// This is the main entry point for working with W3C Trace Context
/// in distributed tracing scenarios.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TraceContext {
    /// The traceparent header value.
    pub traceparent: TraceParent,
    /// The tracestate header value.
    pub tracestate: TraceState,
}

impl TraceContext {
    /// Creates a new empty TraceContext.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new TraceContext with a fresh random trace.
    ///
    /// This starts a new trace with the sampled flag set.
    pub fn new_trace() -> Self {
        let trace_id = TraceId::new();
        let parent_id = SpanId::new();
        Self {
            traceparent: TraceParent::new(trace_id, parent_id, TraceFlags::TRACE_FLAG),
            tracestate: TraceState::new(),
        }
    }

    /// Creates a TraceContext from HTTP header values.
    ///
    /// Returns None if the traceparent header is missing or invalid.
    pub fn from_headers(traceparent: Option<&str>, tracestate: Option<&str>) -> Option<Self> {
        let traceparent = traceparent?;
        let tp = TraceParent::parse_traceparent(traceparent)?;
        let ts = tracestate.map(TraceState::from_header).unwrap_or_default();
        Some(TraceContext {
            traceparent: tp,
            tracestate: ts,
        })
    }

    /// Creates a child TraceContext.
    ///
    /// The child has the same trace_id but a new random parent_id.
    pub fn child(&self) -> Self {
        Self {
            traceparent: self.traceparent.new_child(),
            tracestate: self.tracestate.clone(),
        }
    }

    /// Returns the traceparent header value as a String.
    pub fn get_header(&self) -> String {
        format!("{}", self.traceparent)
    }

    /// Returns the tracestate header value as a String.
    pub fn get_state_header(&self) -> String {
        format!("{}", self.tracestate)
    }
}

impl std::fmt::Display for TraceContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.traceparent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_traceparent_parse() {
        let tp = TraceParent::parse_traceparent(
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
        )
        .unwrap();
        assert_eq!(tp.version, 0);
        assert!(!tp.trace_id.is_empty());
        assert_eq!(tp.flags.0, 1);
    }

    #[test]
    fn test_traceparent_roundtrip() {
        let tp = TraceParent::new(
            TraceId::from_hex("0af7651916cd43dd8448eb211c80319c").unwrap(),
            SpanId::from_hex("b7ad6b7169203331").unwrap(),
            TraceFlags::TRACE_FLAG,
        );
        let s = format!("{}", tp);
        let parsed = TraceParent::parse_traceparent(&s).unwrap();
        assert_eq!(tp.trace_id, parsed.trace_id);
        assert_eq!(tp.parent_id, parsed.parent_id);
    }

    #[test]
    fn test_trace_context_child() {
        let ctx = TraceContext::new_trace();
        let child = ctx.child();
        assert_eq!(ctx.traceparent.trace_id, child.traceparent.trace_id);
        assert_ne!(ctx.traceparent.parent_id, child.traceparent.parent_id);
    }

    #[test]
    fn test_trace_state() {
        let mut state = TraceState::new();
        state.insert("vendor", "claudefs");
        state.insert("tenant", "test");
        assert_eq!(state.get("vendor"), Some("claudefs"));
        let output = format!("{}", state);
        assert!(output.contains("vendor=claudefs"));
        assert!(output.contains("tenant=test"));
    }
}
