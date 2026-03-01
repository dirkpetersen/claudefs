use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WebhookEvent {
    FileCreated {
        path: String,
        size: u64,
        owner: u32,
    },
    FileDeleted {
        path: String,
    },
    FileModified {
        path: String,
        new_size: u64,
    },
    DirectoryCreated {
        path: String,
    },
    DirectoryDeleted {
        path: String,
    },
    NodeJoined {
        node_id: String,
        node_addr: String,
    },
    NodeDeparted {
        node_id: String,
    },
    SlaViolation {
        metric: String,
        actual: f64,
        threshold: f64,
    },
    QuotaExceeded {
        tenant_id: String,
        used_bytes: u64,
        quota_bytes: u64,
    },
    SnapshotCreated {
        snapshot_id: String,
        source_path: String,
    },
    ReplicationLag {
        site_id: String,
        lag_ms: u64,
    },
}

impl WebhookEvent {
    pub fn event_type_name(&self) -> &'static str {
        match self {
            WebhookEvent::FileCreated { .. } => "file_created",
            WebhookEvent::FileDeleted { .. } => "file_deleted",
            WebhookEvent::FileModified { .. } => "file_modified",
            WebhookEvent::DirectoryCreated { .. } => "directory_created",
            WebhookEvent::DirectoryDeleted { .. } => "directory_deleted",
            WebhookEvent::NodeJoined { .. } => "node_joined",
            WebhookEvent::NodeDeparted { .. } => "node_departed",
            WebhookEvent::SlaViolation { .. } => "sla_violation",
            WebhookEvent::QuotaExceeded { .. } => "quota_exceeded",
            WebhookEvent::SnapshotCreated { .. } => "snapshot_created",
            WebhookEvent::ReplicationLag { .. } => "replication_lag",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookPayload {
    pub event_id: String,
    pub event_type: String,
    pub cluster_id: String,
    pub timestamp: u64,
    pub event: WebhookEvent,
}

impl WebhookPayload {
    pub fn new(cluster_id: String, event: WebhookEvent) -> Self {
        Self {
            event_id: uuid_v4(),
            event_type: event.event_type_name().to_string(),
            cluster_id,
            timestamp: current_time_ns(),
            event,
        }
    }

    pub fn event_type(&self) -> &str {
        &self.event_type
    }

    pub fn to_json_body(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

fn uuid_v4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!(
        "{:032x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        now,
        (now >> 48) as u16 & 0xFFFF,
        (now >> 36) as u16 & 0x0FFF,
        (now >> 20) as u16 & 0x3FFF | 0x8000,
        now & 0xFFFFFFFFFFFF
    )
}

fn current_time_ns() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookEndpoint {
    pub endpoint_id: String,
    pub url: String,
    pub secret: Option<String>,
    pub event_filter: Vec<String>,
    pub created_at: u64,
    pub active: bool,
}

impl WebhookEndpoint {
    pub fn new(endpoint_id: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            endpoint_id: endpoint_id.into(),
            url: url.into(),
            secret: None,
            event_filter: vec![],
            created_at: current_time_ns(),
            active: true,
        }
    }

    pub fn with_secret(mut self, secret: String) -> Self {
        self.secret = Some(secret);
        self
    }

    pub fn with_filter(mut self, events: Vec<String>) -> Self {
        self.event_filter = events;
        self
    }

    pub fn matches(&self, event_type: &str) -> bool {
        if self.event_filter.is_empty() {
            return true;
        }
        self.event_filter.iter().any(|f| f == event_type)
    }

    pub fn compute_signature(&self, body: &str) -> Option<String> {
        let key = self.secret.as_ref()?;

        let xor_hash = compute_xor_hash(key.as_bytes(), body.as_bytes());
        Some(format!("sha256={:016x}", xor_hash))
    }
}

fn compute_xor_hash(key: &[u8], data: &[u8]) -> u64 {
    let mut result: u64 = 0;
    let key_len = key.len();

    for (i, &byte) in data.iter().enumerate() {
        let key_byte = key[i % key_len];
        result = result
            .wrapping_mul(31)
            .wrapping_add((byte ^ key_byte) as u64);
    }

    result
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryAttempt {
    pub attempt_number: u32,
    pub delivered_at: u64,
    pub success: bool,
    pub status_code: Option<u16>,
    pub error_message: Option<String>,
}

impl DeliveryAttempt {
    pub fn new(
        attempt_number: u32,
        success: bool,
        status_code: Option<u16>,
        error_message: Option<String>,
    ) -> Self {
        Self {
            attempt_number,
            delivered_at: current_time_ns(),
            success,
            status_code,
            error_message,
        }
    }

    pub fn success(attempt_number: u32, status_code: u16) -> Self {
        Self::new(attempt_number, true, Some(status_code), None)
    }

    pub fn failure(attempt_number: u32, status_code: Option<u16>, error: &str) -> Self {
        Self::new(attempt_number, false, status_code, Some(error.to_string()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryRecord {
    pub event_id: String,
    pub endpoint_id: String,
    pub payload: WebhookPayload,
    pub attempts: Vec<DeliveryAttempt>,
    pub final_success: bool,
}

impl DeliveryRecord {
    pub fn new(endpoint_id: String, payload: WebhookPayload) -> Self {
        Self {
            event_id: payload.event_id.clone(),
            endpoint_id,
            payload,
            attempts: vec![],
            final_success: false,
        }
    }

    pub fn add_attempt(&mut self, attempt: DeliveryAttempt) {
        self.final_success = attempt.success;
        self.attempts.push(attempt);
    }

    pub fn attempt_count(&self) -> usize {
        self.attempts.len()
    }

    pub fn last_attempt(&self) -> Option<&DeliveryAttempt> {
        self.attempts.last()
    }

    pub fn is_successful(&self) -> bool {
        self.final_success
    }
}

#[derive(Debug, Error)]
pub enum WebhookError {
    #[error("Endpoint not found: {0}")]
    EndpointNotFound(String),
    #[error("Duplicate endpoint: {0}")]
    DuplicateEndpoint(String),
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
}

pub struct WebhookRegistry {
    endpoints: HashMap<String, WebhookEndpoint>,
    delivery_history: HashMap<String, Vec<DeliveryRecord>>,
}

impl WebhookRegistry {
    pub fn new() -> Self {
        Self {
            endpoints: HashMap::new(),
            delivery_history: HashMap::new(),
        }
    }

    pub fn register(&mut self, endpoint: WebhookEndpoint) -> Result<(), WebhookError> {
        if self.endpoints.contains_key(&endpoint.endpoint_id) {
            return Err(WebhookError::DuplicateEndpoint(endpoint.endpoint_id));
        }

        if !endpoint.url.starts_with("http://") && !endpoint.url.starts_with("https://") {
            return Err(WebhookError::InvalidUrl(endpoint.url.clone()));
        }

        self.endpoints
            .insert(endpoint.endpoint_id.clone(), endpoint);
        Ok(())
    }

    pub fn unregister(&mut self, endpoint_id: &str) -> Result<(), WebhookError> {
        if self.endpoints.remove(endpoint_id).is_none() {
            return Err(WebhookError::EndpointNotFound(endpoint_id.to_string()));
        }
        Ok(())
    }

    pub fn get_endpoint(&self, id: &str) -> Option<&WebhookEndpoint> {
        self.endpoints.get(id)
    }

    pub fn active_endpoints(&self) -> Vec<&WebhookEndpoint> {
        self.endpoints.values().filter(|e| e.active).collect()
    }

    pub fn endpoints_for_event(&self, event_type: &str) -> Vec<&WebhookEndpoint> {
        self.endpoints
            .values()
            .filter(|e| e.active && e.matches(event_type))
            .collect()
    }

    pub fn record_delivery(&mut self, record: DeliveryRecord) {
        let endpoint_id = record.endpoint_id.clone();
        self.delivery_history
            .entry(endpoint_id)
            .or_insert_with(Vec::new)
            .push(record);
    }

    pub fn delivery_history(&self, endpoint_id: &str) -> Vec<&DeliveryRecord> {
        self.delivery_history
            .get(endpoint_id)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    pub fn endpoint_count(&self) -> usize {
        self.endpoints.len()
    }

    pub fn success_rate(&self, endpoint_id: &str) -> f64 {
        let history = match self.delivery_history.get(endpoint_id) {
            Some(h) => h,
            None => return 0.0,
        };

        if history.is_empty() {
            return 0.0;
        }

        let success_count = history.iter().filter(|r| r.is_successful()).count();
        success_count as f64 / history.len() as f64
    }

    pub fn clear_history(&mut self, endpoint_id: &str) {
        self.delivery_history.remove(endpoint_id);
    }
}

impl Default for WebhookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webhook_payload_event_type_file_created() {
        let event = WebhookEvent::FileCreated {
            path: "/test".to_string(),
            size: 1000,
            owner: 1000,
        };
        let payload = WebhookPayload::new("cluster1".to_string(), event);
        assert_eq!(payload.event_type(), "file_created");
    }

    #[test]
    fn test_webhook_payload_event_type_file_deleted() {
        let event = WebhookEvent::FileDeleted {
            path: "/test".to_string(),
        };
        let payload = WebhookPayload::new("cluster1".to_string(), event);
        assert_eq!(payload.event_type(), "file_deleted");
    }

    #[test]
    fn test_webhook_payload_event_type_file_modified() {
        let event = WebhookEvent::FileModified {
            path: "/test".to_string(),
            new_size: 2000,
        };
        let payload = WebhookPayload::new("cluster1".to_string(), event);
        assert_eq!(payload.event_type(), "file_modified");
    }

    #[test]
    fn test_webhook_payload_event_type_directory_created() {
        let event = WebhookEvent::DirectoryCreated {
            path: "/test".to_string(),
        };
        let payload = WebhookPayload::new("cluster1".to_string(), event);
        assert_eq!(payload.event_type(), "directory_created");
    }

    #[test]
    fn test_webhook_payload_event_type_directory_deleted() {
        let event = WebhookEvent::DirectoryDeleted {
            path: "/test".to_string(),
        };
        let payload = WebhookPayload::new("cluster1".to_string(), event);
        assert_eq!(payload.event_type(), "directory_deleted");
    }

    #[test]
    fn test_webhook_payload_event_type_node_joined() {
        let event = WebhookEvent::NodeJoined {
            node_id: "node1".to_string(),
            node_addr: "192.168.1.1".to_string(),
        };
        let payload = WebhookPayload::new("cluster1".to_string(), event);
        assert_eq!(payload.event_type(), "node_joined");
    }

    #[test]
    fn test_webhook_payload_event_type_node_departed() {
        let event = WebhookEvent::NodeDeparted {
            node_id: "node1".to_string(),
        };
        let payload = WebhookPayload::new("cluster1".to_string(), event);
        assert_eq!(payload.event_type(), "node_departed");
    }

    #[test]
    fn test_webhook_payload_event_type_sla_violation() {
        let event = WebhookEvent::SlaViolation {
            metric: "read_latency".to_string(),
            actual: 5000.0,
            threshold: 3000.0,
        };
        let payload = WebhookPayload::new("cluster1".to_string(), event);
        assert_eq!(payload.event_type(), "sla_violation");
    }

    #[test]
    fn test_webhook_payload_event_type_quota_exceeded() {
        let event = WebhookEvent::QuotaExceeded {
            tenant_id: "tenant1".to_string(),
            used_bytes: 1000000,
            quota_bytes: 500000,
        };
        let payload = WebhookPayload::new("cluster1".to_string(), event);
        assert_eq!(payload.event_type(), "quota_exceeded");
    }

    #[test]
    fn test_webhook_payload_event_type_snapshot_created() {
        let event = WebhookEvent::SnapshotCreated {
            snapshot_id: "snap1".to_string(),
            source_path: "/data".to_string(),
        };
        let payload = WebhookPayload::new("cluster1".to_string(), event);
        assert_eq!(payload.event_type(), "snapshot_created");
    }

    #[test]
    fn test_webhook_payload_event_type_replication_lag() {
        let event = WebhookEvent::ReplicationLag {
            site_id: "site1".to_string(),
            lag_ms: 5000,
        };
        let payload = WebhookPayload::new("cluster1".to_string(), event);
        assert_eq!(payload.event_type(), "replication_lag");
    }

    #[test]
    fn test_webhook_payload_to_json_body() {
        let event = WebhookEvent::FileCreated {
            path: "/test".to_string(),
            size: 1000,
            owner: 1000,
        };
        let payload = WebhookPayload::new("cluster1".to_string(), event);
        let json = payload.to_json_body();

        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_object());
    }

    #[test]
    fn test_webhook_endpoint_matches_filter_empty() {
        let endpoint = WebhookEndpoint::new("e1", "https://example.com");
        assert!(endpoint.matches("file_created"));
    }

    #[test]
    fn test_webhook_endpoint_matches_filter_matching() {
        let endpoint = WebhookEndpoint::new("e1", "https://example.com")
            .with_filter(vec!["file_created".to_string(), "file_deleted".to_string()]);
        assert!(endpoint.matches("file_created"));
        assert!(endpoint.matches("file_deleted"));
    }

    #[test]
    fn test_webhook_endpoint_matches_filter_non_matching() {
        let endpoint = WebhookEndpoint::new("e1", "https://example.com")
            .with_filter(vec!["file_created".to_string()]);
        assert!(!endpoint.matches("file_deleted"));
    }

    #[test]
    fn test_webhook_endpoint_compute_signature_no_secret() {
        let endpoint = WebhookEndpoint::new("e1", "https://example.com");
        let result = endpoint.compute_signature("body");
        assert!(result.is_none());
    }

    #[test]
    fn test_webhook_endpoint_compute_signature_with_secret() {
        let endpoint =
            WebhookEndpoint::new("e1", "https://example.com").with_secret("secret_key".to_string());
        let result = endpoint.compute_signature("body");
        assert!(result.is_some());
        let sig = result.unwrap();
        assert!(sig.starts_with("sha256="));
    }

    #[test]
    fn test_webhook_endpoint_same_key_body_produces_same_signature() {
        let endpoint =
            WebhookEndpoint::new("e1", "https://example.com").with_secret("secret_key".to_string());

        let sig1 = endpoint.compute_signature("body").unwrap();
        let sig2 = endpoint.compute_signature("body").unwrap();

        assert_eq!(sig1, sig2);
    }

    #[test]
    fn test_webhook_endpoint_different_body_produces_different_signature() {
        let endpoint =
            WebhookEndpoint::new("e1", "https://example.com").with_secret("secret_key".to_string());

        let sig1 = endpoint.compute_signature("body1").unwrap();
        let sig2 = endpoint.compute_signature("body2").unwrap();

        assert_ne!(sig1, sig2);
    }

    #[test]
    fn test_delivery_record_add_attempt() {
        let event = WebhookEvent::FileCreated {
            path: "/test".to_string(),
            size: 1000,
            owner: 1000,
        };
        let payload = WebhookPayload::new("cluster1".to_string(), event);
        let mut record = DeliveryRecord::new("e1".to_string(), payload);

        let attempt = DeliveryAttempt::success(1, 200);
        record.add_attempt(attempt);

        assert_eq!(record.attempt_count(), 1);
    }

    #[test]
    fn test_delivery_record_last_attempt() {
        let event = WebhookEvent::FileCreated {
            path: "/test".to_string(),
            size: 1000,
            owner: 1000,
        };
        let payload = WebhookPayload::new("cluster1".to_string(), event);
        let mut record = DeliveryRecord::new("e1".to_string(), payload);

        record.add_attempt(DeliveryAttempt::success(1, 200));
        record.add_attempt(DeliveryAttempt::failure(2, Some(500), "error"));

        let last = record.last_attempt().unwrap();
        assert!(!last.success);
    }

    #[test]
    fn test_webhook_registry_register_unregister_round_trip() {
        let mut registry = WebhookRegistry::new();
        let endpoint = WebhookEndpoint::new("e1", "https://example.com");

        registry.register(endpoint).unwrap();
        assert!(registry.get_endpoint("e1").is_some());

        registry.unregister("e1").unwrap();
        assert!(registry.get_endpoint("e1").is_none());
    }

    #[test]
    fn test_webhook_registry_duplicate_registration() {
        let mut registry = WebhookRegistry::new();
        let endpoint1 = WebhookEndpoint::new("e1", "https://example.com");
        let endpoint2 = WebhookEndpoint::new("e1", "https://example2.com");

        registry.register(endpoint1).unwrap();
        let result = registry.register(endpoint2);

        assert!(matches!(result, Err(WebhookError::DuplicateEndpoint(_))));
    }

    #[test]
    fn test_webhook_registry_active_endpoints() {
        let mut registry = WebhookRegistry::new();

        let endpoint1 = WebhookEndpoint::new("e1", "https://example.com");
        let mut endpoint2 = WebhookEndpoint::new("e2", "https://example2.com");
        endpoint2.active = false;

        registry.register(endpoint1).unwrap();
        registry.register(endpoint2).unwrap();

        let active = registry.active_endpoints();
        assert_eq!(active.len(), 1);
    }

    #[test]
    fn test_webhook_registry_endpoints_for_event() {
        let mut registry = WebhookRegistry::new();

        let endpoint1 = WebhookEndpoint::new("e1", "https://example.com")
            .with_filter(vec!["file_created".to_string()]);
        let endpoint2 = WebhookEndpoint::new("e2", "https://example2.com")
            .with_filter(vec!["file_deleted".to_string()]);

        registry.register(endpoint1).unwrap();
        registry.register(endpoint2).unwrap();

        let for_created = registry.endpoints_for_event("file_created");
        assert_eq!(for_created.len(), 1);

        let for_deleted = registry.endpoints_for_event("file_deleted");
        assert_eq!(for_deleted.len(), 1);
    }

    #[test]
    fn test_webhook_registry_record_delivery_and_history() {
        let mut registry = WebhookRegistry::new();

        let event = WebhookEvent::FileCreated {
            path: "/test".to_string(),
            size: 1000,
            owner: 1000,
        };
        let payload = WebhookPayload::new("cluster1".to_string(), event);
        let mut record = DeliveryRecord::new("e1".to_string(), payload);

        record.add_attempt(DeliveryAttempt::success(1, 200));

        registry.record_delivery(record);

        let history = registry.delivery_history("e1");
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn test_webhook_registry_success_rate_all_successful() {
        let mut registry = WebhookRegistry::new();

        for i in 0..5 {
            let event = WebhookEvent::FileCreated {
                path: "/test".to_string(),
                size: 1000,
                owner: 1000,
            };
            let payload = WebhookPayload::new("cluster1".to_string(), event);
            let mut record = DeliveryRecord::new("e1".to_string(), payload);
            record.add_attempt(DeliveryAttempt::success(1, 200));
            registry.record_delivery(record);
        }

        let rate = registry.success_rate("e1");
        assert_eq!(rate, 1.0);
    }

    #[test]
    fn test_webhook_registry_success_rate_half_successful() {
        let mut registry = WebhookRegistry::new();

        for i in 0..4 {
            let event = WebhookEvent::FileCreated {
                path: "/test".to_string(),
                size: 1000,
                owner: 1000,
            };
            let payload = WebhookPayload::new("cluster1".to_string(), event);
            let mut record = DeliveryRecord::new("e1".to_string(), payload);

            if i < 2 {
                record.add_attempt(DeliveryAttempt::success(1, 200));
            } else {
                record.add_attempt(DeliveryAttempt::failure(1, Some(500), "error"));
            }
            registry.record_delivery(record);
        }

        let rate = registry.success_rate("e1");
        assert_eq!(rate, 0.5);
    }

    #[test]
    fn test_webhook_registry_success_rate_empty_history() {
        let registry = WebhookRegistry::new();
        let rate = registry.success_rate("nonexistent");
        assert_eq!(rate, 0.0);
    }

    #[test]
    fn test_webhook_error_display() {
        let err = WebhookError::EndpointNotFound("e1".to_string());
        assert!(err.to_string().contains("e1"));

        let err = WebhookError::DuplicateEndpoint("e1".to_string());
        assert!(err.to_string().contains("e1"));

        let err = WebhookError::InvalidUrl("badurl".to_string());
        assert!(err.to_string().contains("badurl"));
    }

    #[test]
    fn test_webhook_endpoint_new() {
        let endpoint = WebhookEndpoint::new("e1", "https://example.com");
        assert_eq!(endpoint.endpoint_id, "e1");
        assert_eq!(endpoint.url, "https://example.com");
        assert!(endpoint.active);
        assert!(endpoint.secret.is_none());
        assert!(endpoint.event_filter.is_empty());
    }

    #[test]
    fn test_webhook_endpoint_with_secret() {
        let endpoint =
            WebhookEndpoint::new("e1", "https://example.com").with_secret("mysecret".to_string());
        assert_eq!(endpoint.secret, Some("mysecret".to_string()));
    }

    #[test]
    fn test_webhook_endpoint_with_filter() {
        let endpoint = WebhookEndpoint::new("e1", "https://example.com")
            .with_filter(vec!["event1".to_string(), "event2".to_string()]);
        assert_eq!(endpoint.event_filter.len(), 2);
    }

    #[test]
    fn test_delivery_attempt_success() {
        let attempt = DeliveryAttempt::success(1, 200);
        assert!(attempt.success);
        assert_eq!(attempt.status_code, Some(200));
        assert!(attempt.error_message.is_none());
    }

    #[test]
    fn test_delivery_attempt_failure() {
        let attempt = DeliveryAttempt::failure(1, Some(500), "server error");
        assert!(!attempt.success);
        assert_eq!(attempt.status_code, Some(500));
        assert_eq!(attempt.error_message, Some("server error".to_string()));
    }

    #[test]
    fn test_webhook_registry_endpoint_count() {
        let mut registry = WebhookRegistry::new();

        registry
            .register(WebhookEndpoint::new("e1", "https://example.com"))
            .unwrap();
        registry
            .register(WebhookEndpoint::new("e2", "https://example2.com"))
            .unwrap();

        assert_eq!(registry.endpoint_count(), 2);
    }

    #[test]
    fn test_webhook_registry_invalid_url() {
        let mut registry = WebhookRegistry::new();
        let endpoint = WebhookEndpoint::new("e1", "ftp://example.com");
        let result = registry.register(endpoint);
        assert!(matches!(result, Err(WebhookError::InvalidUrl(_))));
    }
}
