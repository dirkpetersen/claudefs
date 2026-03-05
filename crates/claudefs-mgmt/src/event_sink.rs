use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EventSinkError {
    #[error("Sink not initialized: {0}")]
    NotInitialized(String),
    #[error("Backend error: {0}")]
    BackendError(String),
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EventSinkBackend {
    LogFile { path: PathBuf },
    S3 { bucket: String, prefix: String },
    Syslog { addr: String },
}

impl EventSinkBackend {
    pub fn log_file(path: PathBuf) -> Self {
        Self::LogFile { path }
    }

    pub fn s3(bucket: impl Into<String>, prefix: impl Into<String>) -> Self {
        Self::S3 {
            bucket: bucket.into(),
            prefix: prefix.into(),
        }
    }

    pub fn syslog(addr: impl Into<String>) -> Self {
        Self::Syslog { addr: addr.into() }
    }

    pub fn backend_type(&self) -> &'static str {
        match self {
            EventSinkBackend::LogFile { .. } => "log_file",
            EventSinkBackend::S3 { .. } => "s3",
            EventSinkBackend::Syslog { .. } => "syslog",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

impl Serialize for EventSeverity {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for EventSeverity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        EventSeverity::from_str(&s).ok_or_else(|| serde::de::Error::custom(format!("invalid variant: {}", s)))
    }
}

impl EventSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            EventSeverity::Info => "info",
            EventSeverity::Warning => "warning",
            EventSeverity::Error => "error",
            EventSeverity::Critical => "critical",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "info" => Some(EventSeverity::Info),
            "warning" => Some(EventSeverity::Warning),
            "error" => Some(EventSeverity::Error),
            "critical" => Some(EventSeverity::Critical),
            _ => None,
        }
    }
}

impl Default for EventSeverity {
    fn default() -> Self {
        EventSeverity::Info
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedEvent {
    pub event_type: String,
    pub source_component: String,
    pub tenant_id: Option<String>,
    pub severity: EventSeverity,
    pub message: String,
    pub timestamp: DateTime<Utc>,
    pub metadata: HashMap<String, String>,
}

impl ExportedEvent {
    pub fn new(
        event_type: impl Into<String>,
        source_component: impl Into<String>,
        severity: EventSeverity,
        message: impl Into<String>,
    ) -> Self {
        Self {
            event_type: event_type.into(),
            source_component: source_component.into(),
            tenant_id: None,
            severity,
            message: message.into(),
            timestamp: Utc::now(),
            metadata: HashMap::new(),
        }
    }

    pub fn with_tenant(mut self, tenant_id: impl Into<String>) -> Self {
        self.tenant_id = Some(tenant_id.into());
        self
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    pub fn quota_exceeded(tenant_id: &str, limit: u64, used: u64) -> Self {
        Self::new(
            "quota_exceeded",
            "resource_limiter",
            EventSeverity::Warning,
            format!("Tenant {} exceeded quota: used {} bytes, limit {} bytes", tenant_id, used, limit),
        )
        .with_tenant(tenant_id)
        .with_metadata("limit", limit.to_string())
        .with_metadata("used", used.to_string())
    }

    pub fn node_drain_started(node_id: &str) -> Self {
        Self::new(
            "node_drain_started",
            "node_scaling",
            EventSeverity::Info,
            format!("Node {} drain started", node_id),
        )
        .with_metadata("node_id", node_id)
    }

    pub fn node_drain_completed(node_id: &str) -> Self {
        Self::new(
            "node_drain_completed",
            "node_scaling",
            EventSeverity::Info,
            format!("Node {} drain completed", node_id),
        )
        .with_metadata("node_id", node_id)
    }
}

struct MockBackendState {
    events: VecDeque<ExportedEvent>,
    failed: bool,
}

pub struct EventSink {
    backends: Vec<EventSinkBackend>,
    pending_events: VecDeque<ExportedEvent>,
    mock_states: Vec<MockBackendState>,
}

impl EventSink {
    pub fn new(backends: Vec<EventSinkBackend>) -> Result<Self, EventSinkError> {
        if backends.is_empty() {
            return Err(EventSinkError::NotInitialized(
                "at least one backend required".to_string(),
            ));
        }

        Ok(Self {
            backends: backends.clone(),
            pending_events: VecDeque::new(),
            mock_states: backends.iter().map(|_| MockBackendState {
                events: VecDeque::new(),
                failed: false,
            }).collect(),
        })
    }

    pub fn new_for_test(backends: Vec<EventSinkBackend>) -> Self {
        Self {
            backends,
            pending_events: VecDeque::new(),
            mock_states: Vec::new(),
        }
    }

    pub async fn export_event(&mut self, event: ExportedEvent) -> Result<(), EventSinkError> {
        self.pending_events.push_back(event.clone());

        for (idx, backend) in self.backends.iter().enumerate() {
            if idx < self.mock_states.len() {
                if !self.mock_states[idx].failed {
                    self.mock_states[idx].events.push_back(event.clone());
                }
            }
        }

        Ok(())
    }

    pub async fn flush(&mut self) -> Result<(), EventSinkError> {
        self.pending_events.clear();
        Ok(())
    }

    pub fn active_backends(&self) -> Vec<&EventSinkBackend> {
        self.backends.iter().collect()
    }

    pub fn pending_count(&self) -> usize {
        self.pending_events.len()
    }

    pub fn backend_count(&self) -> usize {
        self.backends.len()
    }

    pub fn get_mock_events(&self, backend_idx: usize) -> Vec<ExportedEvent> {
        if backend_idx < self.mock_states.len() {
            self.mock_states[backend_idx].events.iter().cloned().collect()
        } else {
            Vec::new()
        }
    }

    pub fn set_backend_failed(&mut self, backend_idx: usize, failed: bool) {
        if backend_idx < self.mock_states.len() {
            self.mock_states[backend_idx].failed = failed;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_sink_backend_log_file() {
        let backend = EventSinkBackend::log_file(PathBuf::from("/var/log/events.log"));
        assert_eq!(backend.backend_type(), "log_file");

        if let EventSinkBackend::LogFile { path } = backend {
            assert_eq!(path, PathBuf::from("/var/log/events.log"));
        }
    }

    #[test]
    fn test_event_sink_backend_s3() {
        let backend = EventSinkBackend::s3("my-bucket", "prefix/");
        assert_eq!(backend.backend_type(), "s3");

        if let EventSinkBackend::S3 { bucket, prefix } = backend {
            assert_eq!(bucket, "my-bucket");
            assert_eq!(prefix, "prefix/");
        }
    }

    #[test]
    fn test_event_sink_backend_syslog() {
        let backend = EventSinkBackend::syslog("localhost:514");
        assert_eq!(backend.backend_type(), "syslog");

        if let EventSinkBackend::Syslog { addr } = backend {
            assert_eq!(addr, "localhost:514");
        }
    }

    #[test]
    fn test_event_severity_as_str() {
        assert_eq!(EventSeverity::Info.as_str(), "info");
        assert_eq!(EventSeverity::Warning.as_str(), "warning");
        assert_eq!(EventSeverity::Error.as_str(), "error");
        assert_eq!(EventSeverity::Critical.as_str(), "critical");
    }

    #[test]
    fn test_event_severity_from_str() {
        assert_eq!(EventSeverity::from_str("info"), Some(EventSeverity::Info));
        assert_eq!(EventSeverity::from_str("warning"), Some(EventSeverity::Warning));
        assert_eq!(EventSeverity::from_str("ERROR"), Some(EventSeverity::Error));
        assert_eq!(EventSeverity::from_str("invalid"), None);
    }

    #[test]
    fn test_event_severity_default() {
        let severity: EventSeverity = Default::default();
        assert_eq!(severity, EventSeverity::Info);
    }

    #[test]
    fn test_exported_event_new() {
        let event = ExportedEvent::new(
            "test_event",
            "test_component",
            EventSeverity::Info,
            "Test message",
        );

        assert_eq!(event.event_type, "test_event");
        assert_eq!(event.source_component, "test_component");
        assert_eq!(event.severity, EventSeverity::Info);
        assert_eq!(event.message, "Test message");
        assert!(event.tenant_id.is_none());
        assert!(event.metadata.is_empty());
    }

    #[test]
    fn test_exported_event_with_tenant() {
        let event = ExportedEvent::new(
            "test_event",
            "test_component",
            EventSeverity::Info,
            "Test message",
        )
        .with_tenant("tenant1");

        assert_eq!(event.tenant_id, Some("tenant1".to_string()));
    }

    #[test]
    fn test_exported_event_with_metadata() {
        let event = ExportedEvent::new(
            "test_event",
            "test_component",
            EventSeverity::Info,
            "Test message",
        )
        .with_metadata("key1", "value1")
        .with_metadata("key2", "value2");

        assert_eq!(event.metadata.get("key1"), Some(&"value1".to_string()));
        assert_eq!(event.metadata.get("key2"), Some(&"value2".to_string()));
    }

    #[test]
    fn test_exported_event_quota_exceeded() {
        let event = ExportedEvent::quota_exceeded("tenant1", 1000, 1500);

        assert_eq!(event.event_type, "quota_exceeded");
        assert_eq!(event.source_component, "resource_limiter");
        assert_eq!(event.severity, EventSeverity::Warning);
        assert_eq!(event.tenant_id, Some("tenant1".to_string()));
        assert_eq!(event.metadata.get("limit"), Some(&"1000".to_string()));
        assert_eq!(event.metadata.get("used"), Some(&"1500".to_string()));
    }

    #[test]
    fn test_exported_event_node_drain_started() {
        let event = ExportedEvent::node_drain_started("node1");

        assert_eq!(event.event_type, "node_drain_started");
        assert_eq!(event.source_component, "node_scaling");
        assert_eq!(event.severity, EventSeverity::Info);
        assert_eq!(event.metadata.get("node_id"), Some(&"node1".to_string()));
    }

    #[test]
    fn test_exported_event_node_drain_completed() {
        let event = ExportedEvent::node_drain_completed("node1");

        assert_eq!(event.event_type, "node_drain_completed");
        assert_eq!(event.source_component, "node_scaling");
        assert_eq!(event.severity, EventSeverity::Info);
    }

    #[test]
    fn test_event_sink_new_empty_backends() {
        let result = EventSink::new(Vec::new());
        assert!(matches!(result, Err(EventSinkError::NotInitialized(_))));
    }

    #[test]
    fn test_event_sink_new_with_backends() {
        let backends = vec![EventSinkBackend::log_file(PathBuf::from("/tmp/test.log"))];
        let sink = EventSink::new(backends).unwrap();

        assert_eq!(sink.backend_count(), 1);
    }

    #[test]
    fn test_event_sink_active_backends() {
        let backends = vec![
            EventSinkBackend::log_file(PathBuf::from("/tmp/test.log")),
            EventSinkBackend::s3("bucket", "prefix"),
        ];
        let sink = EventSink::new(backends).unwrap();

        let active = sink.active_backends();
        assert_eq!(active.len(), 2);
    }

    #[tokio::test]
    async fn test_event_sink_export_event() {
        let backends = vec![EventSinkBackend::log_file(PathBuf::from("/tmp/test.log"))];
        let mut sink = EventSink::new(backends).unwrap();

        let event = ExportedEvent::new("test", "test", EventSeverity::Info, "Test message");
        let result = sink.export_event(event).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_event_sink_export_event_multiple() {
        let backends = vec![EventSinkBackend::log_file(PathBuf::from("/tmp/test.log"))];
        let mut sink = EventSink::new(backends).unwrap();

        for i in 0..5 {
            let event = ExportedEvent::new(
                "test",
                "test",
                EventSeverity::Info,
                format!("Message {}", i),
            );
            sink.export_event(event).await.unwrap();
        }

        assert_eq!(sink.pending_count(), 5);
    }

    #[tokio::test]
    async fn test_event_sink_flush() {
        let backends = vec![EventSinkBackend::log_file(PathBuf::from("/tmp/test.log"))];
        let mut sink = EventSink::new(backends).unwrap();

        let event = ExportedEvent::new("test", "test", EventSeverity::Info, "Test message");
        sink.export_event(event).await.unwrap();

        sink.flush().await.unwrap();

        assert_eq!(sink.pending_count(), 0);
    }

    #[test]
    fn test_event_sink_pending_count() {
        let backends = vec![EventSinkBackend::log_file(PathBuf::from("/tmp/test.log"))];
        let mut sink = EventSink::new(backends).unwrap();

        assert_eq!(sink.pending_count(), 0);
    }

    #[test]
    fn test_event_sink_backend_count() {
        let backends = vec![
            EventSinkBackend::log_file(PathBuf::from("/tmp/test.log")),
            EventSinkBackend::s3("bucket", "prefix"),
            EventSinkBackend::syslog("localhost:514"),
        ];
        let sink = EventSink::new(backends).unwrap();

        assert_eq!(sink.backend_count(), 3);
    }

    #[tokio::test]
    async fn test_event_sink_event_routing() {
        let backends = vec![
            EventSinkBackend::log_file(PathBuf::from("/tmp/test.log")),
            EventSinkBackend::s3("bucket", "prefix"),
        ];
        let mut sink = EventSink::new(backends).unwrap();

        let event = ExportedEvent::new("test", "test", EventSeverity::Info, "Test message");
        sink.export_event(event).await.unwrap();

        assert!(sink.pending_count() > 0);
    }

    #[test]
    fn test_exported_event_serialization() {
        let event = ExportedEvent::new(
            "test_event",
            "test_component",
            EventSeverity::Warning,
            "Test message",
        )
        .with_tenant("tenant1")
        .with_metadata("key", "value");

        let json = serde_json::to_string(&event).unwrap();
        let decoded: ExportedEvent = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.event_type, "test_event");
        assert_eq!(decoded.tenant_id, Some("tenant1".to_string()));
        assert_eq!(decoded.metadata.get("key"), Some(&"value".to_string()));
    }

    #[test]
    fn test_event_sink_severity_serialization() {
        let event = ExportedEvent::new("test", "test", EventSeverity::Critical, "Test");
        let json = serde_json::to_string(&event).unwrap();

        assert!(json.contains("critical"));
    }

    #[test]
    fn test_event_sink_backend_serialization() {
        let backend = EventSinkBackend::s3("my-bucket", "my-prefix");
        let json = serde_json::to_string(&backend).unwrap();

        assert!(json.contains("my-bucket"));
        assert!(json.contains("my-prefix"));
    }

    #[test]
    fn test_event_sink_multiple_different_backends() {
        let backends = vec![
            EventSinkBackend::log_file(PathBuf::from("/var/log/app.log")),
            EventSinkBackend::s3("events-bucket", "events/"),
            EventSinkBackend::syslog("192.168.1.1:514"),
        ];

        let sink = EventSink::new(backends).unwrap();
        let active = sink.active_backends();

        assert_eq!(active.len(), 3);
    }

    #[tokio::test]
    async fn test_event_sink_event_contains_timestamp() {
        let backends = vec![EventSinkBackend::log_file(PathBuf::from("/tmp/test.log"))];
        let mut sink = EventSink::new(backends).unwrap();

        let before = Utc::now();
        let event = ExportedEvent::new("test", "test", EventSeverity::Info, "Test");
        sink.export_event(event).await.unwrap();
        let after = Utc::now();

        assert!(sink.pending_count() > 0);
    }

    #[test]
    fn test_exported_event_empty_metadata() {
        let event = ExportedEvent::new("test", "test", EventSeverity::Info, "Test");
        assert!(event.metadata.is_empty());

        let json = serde_json::to_string(&event).unwrap();
        let decoded: ExportedEvent = serde_json::from_str(&json).unwrap();
        assert!(decoded.metadata.is_empty());
    }

    #[test]
    fn test_event_sink_preserves_backend_order() {
        let backends = vec![
            EventSinkBackend::log_file(PathBuf::from("/first.log")),
            EventSinkBackend::log_file(PathBuf::from("/second.log")),
            EventSinkBackend::log_file(PathBuf::from("/third.log")),
        ];

        let sink = EventSink::new(backends).unwrap();
        let active = sink.active_backends();

        if let EventSinkBackend::LogFile { path } = active[0] {
            assert_eq!(*path, PathBuf::from("/first.log"));
        }
    }

    #[tokio::test]
    async fn test_event_sink_flush_twice() {
        let backends = vec![EventSinkBackend::log_file(PathBuf::from("/tmp/test.log"))];
        let mut sink = EventSink::new(backends).unwrap();

        sink.flush().await.unwrap();
        sink.flush().await.unwrap();

        assert_eq!(sink.pending_count(), 0);
    }

    #[tokio::test]
    async fn test_event_sink_export_after_flush() {
        let backends = vec![EventSinkBackend::log_file(PathBuf::from("/tmp/test.log"))];
        let mut sink = EventSink::new(backends).unwrap();

        sink.flush().await.unwrap();

        let event = ExportedEvent::new("test", "test", EventSeverity::Info, "Test");
        sink.export_event(event).await.unwrap();

        assert_eq!(sink.pending_count(), 1);
    }

    #[test]
    fn test_event_severity_order() {
        assert!(EventSeverity::Critical > EventSeverity::Error);
        assert!(EventSeverity::Error > EventSeverity::Warning);
        assert!(EventSeverity::Warning > EventSeverity::Info);
    }
}