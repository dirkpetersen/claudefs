use crate::tracing_client::{SpanId, TraceContext, TraceId};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpanStatus {
    Ok,
    Error(String),
    Unset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    Internal,
    Client,
    Server,
    Producer,
    Consumer,
}

#[derive(Debug, Clone)]
pub struct SpanAttribute {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct OtelSpan {
    pub trace_id: TraceId,
    pub span_id: SpanId,
    pub parent_span_id: Option<SpanId>,
    pub operation: String,
    pub service: String,
    pub start_unix_ns: u64,
    pub end_unix_ns: u64,
    pub status: SpanStatus,
    pub kind: SpanKind,
    pub attributes: Vec<SpanAttribute>,
}

impl OtelSpan {
    pub fn duration_ns(&self) -> u64 {
        self.end_unix_ns.saturating_sub(self.start_unix_ns)
    }

    pub fn is_error(&self) -> bool {
        matches!(self.status, SpanStatus::Error(_))
    }

    pub fn add_attribute(&mut self, key: String, value: String) {
        self.attributes.push(SpanAttribute { key, value });
    }

    pub fn set_status(&mut self, status: SpanStatus) {
        self.status = status;
    }

    pub fn finish(&mut self, end_ns: u64) {
        self.end_unix_ns = end_ns;
    }
}

pub struct OtelSpanBuilder {
    pub trace_id: Option<TraceId>,
    pub parent_span_id: Option<SpanId>,
    pub operation: String,
    pub service: String,
    pub start_unix_ns: u64,
    pub end_unix_ns: Option<u64>,
    pub status: SpanStatus,
    pub kind: SpanKind,
    pub attributes: Vec<SpanAttribute>,
}

impl OtelSpanBuilder {
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

    pub fn with_parent(mut self, parent: &TraceContext) -> Self {
        self.trace_id = Some(parent.trace_id);
        self.parent_span_id = Some(parent.span_id);
        self
    }

    pub fn with_trace_id(mut self, trace_id: TraceId) -> Self {
        self.trace_id = Some(trace_id);
        self
    }

    pub fn with_kind(mut self, kind: SpanKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn with_attribute(mut self, key: String, value: String) -> Self {
        self.attributes.push(SpanAttribute { key, value });
        self
    }

    pub fn build(self, end_unix_ns: u64) -> OtelSpan {
        let trace_id = self.trace_id.unwrap_or_else(|| TraceId(0));
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

#[derive(Debug, Clone)]
pub struct OtelExportBuffer {
    pub capacity: usize,
    spans: Vec<OtelSpan>,
}

impl OtelExportBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1).min(10_000),
            spans: Vec::new(),
        }
    }

    pub fn push(&mut self, span: OtelSpan) {
        if self.spans.len() >= self.capacity {
            self.spans.remove(0);
        }
        self.spans.push(span);
    }

    pub fn drain(&mut self) -> Vec<OtelSpan> {
        std::mem::take(&mut self.spans)
    }

    pub fn len(&self) -> usize {
        self.spans.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplingDecision {
    RecordAndSample,
    Drop,
}

pub struct OtelSampler {
    sample_rate: f64,
}

impl OtelSampler {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate: sample_rate.clamp(0.0, 1.0),
        }
    }

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
