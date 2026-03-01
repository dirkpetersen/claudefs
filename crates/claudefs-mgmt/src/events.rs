use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EventError {
    #[error("Channel closed")]
    ChannelClosed,
    #[error("Webhook delivery failed: {0}")]
    WebhookFailed(String),
    #[error("Subscription not found: {0}")]
    SubscriptionNotFound(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EventKind {
    Created,
    Deleted,
    Modified,
    Renamed { old_path: String },
    OwnerChanged { old_uid: u32, old_gid: u32 },
    PermissionChanged { old_mode: u32 },
    Replicated,
    Tiered { destination: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsEvent {
    pub event_id: String,
    pub kind: EventKind,
    pub path: String,
    pub inode: u64,
    pub owner_uid: u32,
    pub size_bytes: u64,
    pub timestamp: u64,
    pub cluster_id: String,
    pub node_id: String,
    pub seq: u64,
}

impl FsEvent {
    pub fn new(
        kind: EventKind,
        path: String,
        inode: u64,
        owner_uid: u32,
        size_bytes: u64,
        cluster_id: String,
        node_id: String,
        seq: u64,
    ) -> Self {
        Self {
            event_id: format!("evt-{}", seq),
            kind,
            path,
            inode,
            owner_uid,
            size_bytes,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            cluster_id,
            node_id,
            seq,
        }
    }

    pub fn is_data_change(&self) -> bool {
        matches!(
            self.kind,
            EventKind::Created
                | EventKind::Deleted
                | EventKind::Modified
                | EventKind::Renamed { .. }
        )
    }

    pub fn is_metadata_change(&self) -> bool {
        matches!(
            self.kind,
            EventKind::OwnerChanged { .. } | EventKind::PermissionChanged { .. }
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookSubscription {
    pub id: String,
    pub url: String,
    pub event_filter: Vec<String>,
    pub secret: Option<String>,
    pub active: bool,
}

impl WebhookSubscription {
    pub fn new(id: String, url: String) -> Self {
        Self {
            id,
            url,
            event_filter: vec![],
            secret: None,
            active: true,
        }
    }

    pub fn matches(&self, event: &FsEvent) -> bool {
        if self.event_filter.is_empty() {
            return true;
        }
        let kind_name = match &event.kind {
            EventKind::Created => "Created",
            EventKind::Deleted => "Deleted",
            EventKind::Modified => "Modified",
            EventKind::Renamed { .. } => "Renamed",
            EventKind::OwnerChanged { .. } => "OwnerChanged",
            EventKind::PermissionChanged { .. } => "PermissionChanged",
            EventKind::Replicated => "Replicated",
            EventKind::Tiered { .. } => "Tiered",
        };
        self.event_filter.contains(&kind_name.to_string())
    }

    pub fn add_filter(&mut self, event_kind: String) {
        if !self.event_filter.contains(&event_kind) {
            self.event_filter.push(event_kind);
        }
    }
}

pub struct EventBus {
    sender: broadcast::Sender<FsEvent>,
    subscriptions: Arc<RwLock<HashMap<String, WebhookSubscription>>>,
    capacity: usize,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
            capacity,
        }
    }

    pub fn publish(&self, event: FsEvent) -> Result<usize, EventError> {
        let count = self.sender.receiver_count();
        if count == 0 {
            return Ok(0);
        }
        self.sender.send(event).map_err(|_| EventError::ChannelClosed)?;
        Ok(self.sender.receiver_count())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<FsEvent> {
        self.sender.subscribe()
    }

    pub async fn add_webhook(&self, sub: WebhookSubscription) {
        let mut subs = self.subscriptions.write().await;
        subs.insert(sub.id.clone(), sub);
    }

    pub async fn remove_webhook(&self, id: &str) -> Result<(), EventError> {
        let mut subs = self.subscriptions.write().await;
        if subs.remove(id).is_some() {
            Ok(())
        } else {
            Err(EventError::SubscriptionNotFound(id.to_string()))
        }
    }

    pub async fn webhooks(&self) -> Vec<WebhookSubscription> {
        let subs = self.subscriptions.read().await;
        subs.values().cloned().collect()
    }

    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fs_event_new_generates_event_id() {
        let event = FsEvent::new(
            EventKind::Created,
            "/test/file".to_string(),
            12345,
            1000,
            4096,
            "cluster1".to_string(),
            "node1".to_string(),
            42,
        );
        assert_eq!(event.event_id, "evt-42");
    }

    #[test]
    fn test_fs_event_is_data_change_created() {
        let event = FsEvent::new(
            EventKind::Created,
            "/test/file".to_string(),
            12345,
            1000,
            4096,
            "cluster1".to_string(),
            "node1".to_string(),
            1,
        );
        assert!(event.is_data_change());
    }

    #[test]
    fn test_fs_event_is_data_change_modified() {
        let event = FsEvent::new(
            EventKind::Modified,
            "/test/file".to_string(),
            12345,
            1000,
            4096,
            "cluster1".to_string(),
            "node1".to_string(),
            1,
        );
        assert!(event.is_data_change());
    }

    #[test]
    fn test_fs_event_is_data_change_deleted() {
        let event = FsEvent::new(
            EventKind::Deleted,
            "/test/file".to_string(),
            12345,
            1000,
            4096,
            "cluster1".to_string(),
            "node1".to_string(),
            1,
        );
        assert!(event.is_data_change());
    }

    #[test]
    fn test_fs_event_is_data_change_renamed() {
        let event = FsEvent::new(
            EventKind::Renamed { old_path: "/test/old".to_string() },
            "/test/new".to_string(),
            12345,
            1000,
            4096,
            "cluster1".to_string(),
            "node1".to_string(),
            1,
        );
        assert!(event.is_data_change());
    }

    #[test]
    fn test_fs_event_is_data_change_false_for_owner_changed() {
        let event = FsEvent::new(
            EventKind::OwnerChanged { old_uid: 1000, old_gid: 1000 },
            "/test/file".to_string(),
            12345,
            1000,
            4096,
            "cluster1".to_string(),
            "node1".to_string(),
            1,
        );
        assert!(!event.is_data_change());
    }

    #[test]
    fn test_fs_event_is_data_change_false_for_permission_changed() {
        let event = FsEvent::new(
            EventKind::PermissionChanged { old_mode: 0o644 },
            "/test/file".to_string(),
            12345,
            1000,
            4096,
            "cluster1".to_string(),
            "node1".to_string(),
            1,
        );
        assert!(!event.is_data_change());
    }

    #[test]
    fn test_fs_event_is_metadata_change_false_for_created() {
        let event = FsEvent::new(
            EventKind::Created,
            "/test/file".to_string(),
            12345,
            1000,
            4096,
            "cluster1".to_string(),
            "node1".to_string(),
            1,
        );
        assert!(!event.is_metadata_change());
    }

    #[test]
    fn test_fs_event_is_metadata_change_true_for_owner_changed() {
        let event = FsEvent::new(
            EventKind::OwnerChanged { old_uid: 1000, old_gid: 1000 },
            "/test/file".to_string(),
            12345,
            1000,
            4096,
            "cluster1".to_string(),
            "node1".to_string(),
            1,
        );
        assert!(event.is_metadata_change());
    }

    #[test]
    fn test_fs_event_is_metadata_change_true_for_permission_changed() {
        let event = FsEvent::new(
            EventKind::PermissionChanged { old_mode: 0o644 },
            "/test/file".to_string(),
            12345,
            1000,
            4096,
            "cluster1".to_string(),
            "node1".to_string(),
            1,
        );
        assert!(event.is_metadata_change());
    }

    #[test]
    fn test_event_kind_serde_roundtrip_created() {
        let kind = EventKind::Created;
        let json = serde_json::to_string(&kind).unwrap();
        let decoded: EventKind = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, kind);
    }

    #[test]
    fn test_event_kind_serde_roundtrip_deleted() {
        let kind = EventKind::Deleted;
        let json = serde_json::to_string(&kind).unwrap();
        let decoded: EventKind = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, kind);
    }

    #[test]
    fn test_event_kind_serde_roundtrip_renamed() {
        let kind = EventKind::Renamed { old_path: "/test/old".to_string() };
        let json = serde_json::to_string(&kind).unwrap();
        let decoded: EventKind = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, kind);
    }

    #[test]
    fn test_event_kind_serde_roundtrip_owner_changed() {
        let kind = EventKind::OwnerChanged { old_uid: 1000, old_gid: 1000 };
        let json = serde_json::to_string(&kind).unwrap();
        let decoded: EventKind = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, kind);
    }

    #[test]
    fn test_webhook_subscription_new() {
        let sub = WebhookSubscription::new("webhook1".to_string(), "http://example.com/webhook".to_string());
        assert_eq!(sub.id, "webhook1");
        assert_eq!(sub.url, "http://example.com/webhook");
        assert!(sub.active);
        assert!(sub.event_filter.is_empty());
    }

    #[test]
    fn test_webhook_subscription_matches_no_filter() {
        let sub = WebhookSubscription::new("webhook1".to_string(), "http://example.com".to_string());
        let event = FsEvent::new(
            EventKind::Created,
            "/test/file".to_string(),
            12345,
            1000,
            4096,
            "cluster1".to_string(),
            "node1".to_string(),
            1,
        );
        assert!(sub.matches(&event));
    }

    #[test]
    fn test_webhook_subscription_matches_filter_match() {
        let mut sub = WebhookSubscription::new("webhook1".to_string(), "http://example.com".to_string());
        sub.add_filter("Created".to_string());
        let event = FsEvent::new(
            EventKind::Created,
            "/test/file".to_string(),
            12345,
            1000,
            4096,
            "cluster1".to_string(),
            "node1".to_string(),
            1,
        );
        assert!(sub.matches(&event));
    }

    #[test]
    fn test_webhook_subscription_matches_filter_no_match() {
        let mut sub = WebhookSubscription::new("webhook1".to_string(), "http://example.com".to_string());
        sub.add_filter("Deleted".to_string());
        let event = FsEvent::new(
            EventKind::Created,
            "/test/file".to_string(),
            12345,
            1000,
            4096,
            "cluster1".to_string(),
            "node1".to_string(),
            1,
        );
        assert!(!sub.matches(&event));
    }

    #[test]
    fn test_webhook_subscription_add_filter() {
        let mut sub = WebhookSubscription::new("webhook1".to_string(), "http://example.com".to_string());
        sub.add_filter("Created".to_string());
        sub.add_filter("Deleted".to_string());
        assert_eq!(sub.event_filter.len(), 2);
    }

    #[tokio::test]
    async fn test_event_bus_new() {
        let bus = EventBus::new(100);
        assert_eq!(bus.subscriber_count(), 0);
    }

    #[tokio::test]
    async fn test_event_bus_publish_no_subscribers() {
        let bus = EventBus::new(100);
        let event = FsEvent::new(
            EventKind::Created,
            "/test/file".to_string(),
            12345,
            1000,
            4096,
            "cluster1".to_string(),
            "node1".to_string(),
            1,
        );
        let count = bus.publish(event).unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_event_bus_subscribe_and_receive() {
        let bus = EventBus::new(100);
        let mut receiver = bus.subscribe();
        
        let event = FsEvent::new(
            EventKind::Created,
            "/test/file".to_string(),
            12345,
            1000,
            4096,
            "cluster1".to_string(),
            "node1".to_string(),
            1,
        );
        
        bus.publish(event.clone()).unwrap();
        
        let received = receiver.recv().await.unwrap();
        assert_eq!(received.path, "/test/file");
    }

    #[tokio::test]
    async fn test_event_bus_add_webhook() {
        let bus = EventBus::new(100);
        let sub = WebhookSubscription::new("webhook1".to_string(), "http://example.com".to_string());
        bus.add_webhook(sub).await;
        
        let webhooks = bus.webhooks().await;
        assert_eq!(webhooks.len(), 1);
    }

    #[tokio::test]
    async fn test_event_bus_remove_webhook() {
        let bus = EventBus::new(100);
        let sub = WebhookSubscription::new("webhook1".to_string(), "http://example.com".to_string());
        bus.add_webhook(sub).await;
        
        bus.remove_webhook("webhook1").await.unwrap();
        
        let webhooks = bus.webhooks().await;
        assert!(webhooks.is_empty());
    }

    #[tokio::test]
    async fn test_event_bus_remove_webhook_unknown() {
        let bus = EventBus::new(100);
        let result = bus.remove_webhook("unknown").await;
        assert!(result.is_err());
    }
}