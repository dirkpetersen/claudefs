//! Distributed session management for FUSE mounts.
//!
//! Manages distributed sessions across FUSE mount points with lease
//! management and operation tracking.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use dashmap::DashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Session ID unique per mount + client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

impl SessionId {
    /// Creates a new session ID from a raw value.
    pub fn new(id: u64) -> Self {
        SessionId(id)
    }

    /// Returns the raw value.
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl Default for SessionId {
    fn default() -> Self {
        SessionId(0)
    }
}

/// Distributed session bound to a primary node.
#[derive(Debug, Clone)]
pub struct DistributedSession {
    /// Session identifier.
    pub session_id: SessionId,
    /// Client identifier.
    pub client_id: String,
    /// Mount point path.
    pub mount_point: String,
    /// Primary node identifier.
    pub primary_node_id: String,
    /// Replica node identifiers.
    pub replica_node_ids: Vec<String>,
    /// Creation timestamp (ns since epoch).
    pub created_ns: u64,
    /// Lease expiration timestamp (ns since epoch).
    pub lease_until_ns: u64,
}

impl DistributedSession {
    /// Creates a new distributed session.
    pub fn new(
        session_id: SessionId,
        client_id: String,
        mount_point: String,
        primary_node_id: String,
        replicas: Vec<String>,
    ) -> Self {
        let now = Self::now_ns();
        Self {
            session_id,
            client_id,
            mount_point,
            primary_node_id,
            replica_node_ids: replicas,
            created_ns: now,
            lease_until_ns: now + 30_000_000_000, // 30 second default lease
        }
    }

    /// Gets current timestamp in nanoseconds.
    fn now_ns() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    }

    /// Checks if the session lease has expired.
    pub fn is_expired(&self) -> bool {
        Self::now_ns() >= self.lease_until_ns
    }

    /// Returns the remaining lease time in nanoseconds.
    pub fn lease_remaining_ns(&self) -> i64 {
        self.lease_until_ns as i64 - Self::now_ns() as i64
    }
}

/// Context for a distributed operation.
#[derive(Debug, Clone)]
pub struct DistributedOpContext {
    /// Session this operation belongs to.
    pub session_id: SessionId,
    /// Unique operation identifier.
    pub operation_id: u64,
    /// Target inode.
    pub inode: u64,
    /// Deadline timestamp (ns since epoch).
    pub deadline_ns: u64,
    /// Operation priority (0-255).
    pub priority: u8,
}

impl DistributedOpContext {
    /// Creates a new operation context.
    pub fn new(session_id: SessionId, operation_id: u64, inode: u64, latency_bound_ns: u64) -> Self {
        let now = Self::now_ns();
        Self {
            session_id,
            operation_id,
            inode,
            deadline_ns: now.saturating_add(latency_bound_ns),
            priority: 128,
        }
    }

    fn now_ns() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    }

    /// Checks if the operation deadline has been exceeded.
    pub fn is_deadline_exceeded(&self) -> bool {
        Self::now_ns() >= self.deadline_ns
    }
}

/// Session lease renewal request.
#[derive(Debug)]
pub struct LeaseRenewalRequest {
    /// Session to renew.
    pub session_id: SessionId,
    /// New lease duration in nanoseconds.
    pub new_lease_duration_ns: u64,
}

/// Session metrics for monitoring.
#[derive(Debug, Clone)]
pub struct SessionMetric {
    /// Session identifier.
    pub session_id: SessionId,
    /// Client identifier.
    pub client_id: String,
    /// Number of active pending operations.
    pub active_ops: u32,
    /// Remaining lease time in nanoseconds.
    pub lease_remaining_ns: u64,
    /// Total operations completed.
    pub operations_completed: u64,
}

/// Distributed session manager.
pub struct DistributedSessionManager {
    /// Active sessions by session ID.
    sessions: Arc<DashMap<SessionId, DistributedSession>>,
    /// Session to primary node mapping.
    session_to_primary: Arc<DashMap<SessionId, String>>,
    /// Pending operations by operation ID.
    pending_ops: Arc<DashMap<u64, DistributedOpContext>>,
    /// Completed operations for tracking.
    completed_ops: Arc<DashMap<u64, u64>>,
    /// Next session ID counter.
    next_session_id: AtomicU64,
    /// Next operation ID counter.
    next_op_id: AtomicU64,
    /// Operations completed counter per session.
    ops_completed_count: Arc<DashMap<SessionId, u64>>,
}

impl DistributedSessionManager {
    /// Creates a new distributed session manager.
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
            session_to_primary: Arc::new(DashMap::new()),
            pending_ops: Arc::new(DashMap::new()),
            completed_ops: Arc::new(DashMap::new()),
            next_session_id: AtomicU64::new(1),
            next_op_id: AtomicU64::new(1),
            ops_completed_count: Arc::new(DashMap::new()),
        }
    }

    /// Generates a new unique session ID.
    fn generate_session_id(&self) -> SessionId {
        SessionId(self.next_session_id.fetch_add(1, Ordering::SeqCst))
    }

    /// Generates a new unique operation ID.
    fn generate_op_id(&self) -> u64 {
        self.next_op_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Gets current timestamp in nanoseconds.
    fn now_ns() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    }

    /// Creates a new distributed session.
    pub async fn create_session(
        &self,
        client_id: String,
        mount_point: String,
        primary_node_id: String,
        replicas: Vec<String>,
    ) -> Result<SessionId, String> {
        if client_id.is_empty() {
            return Err("client_id cannot be empty".to_string());
        }
        if mount_point.is_empty() {
            return Err("mount_point cannot be empty".to_string());
        }

        let session_id = self.generate_session_id();

        let session = DistributedSession::new(
            session_id,
            client_id.clone(),
            mount_point,
            primary_node_id.clone(),
            replicas,
        );

        self.sessions.insert(session_id, session);
        self.session_to_primary.insert(session_id, primary_node_id);
        self.ops_completed_count.insert(session_id, 0);

        Ok(session_id)
    }

    /// Gets a session by ID.
    pub fn get_session(&self, session_id: SessionId) -> Option<DistributedSession> {
        self.sessions.get(&session_id).map(|s| s.clone())
    }

    /// Renews a session lease.
    pub async fn renew_lease(&self, session_id: SessionId, duration_ns: u64) -> Result<(), String> {
        let mut session = self
            .sessions
            .get_mut(&session_id)
            .ok_or_else(|| format!("session {:?} not found", session_id))?;

        if session.is_expired() {
            return Err("cannot renew expired lease".to_string());
        }

        let now = Self::now_ns();
        session.lease_until_ns = now.saturating_add(duration_ns);

        Ok(())
    }

    /// Registers a pending distributed operation.
    pub fn register_op(&self, ctx: DistributedOpContext) -> Result<(), String> {
        if ctx.operation_id == 0 {
            return Err("operation_id cannot be zero".to_string());
        }

        self.pending_ops.insert(ctx.operation_id, ctx);
        Ok(())
    }

    /// Marks an operation as completed.
    pub fn complete_op(&self, operation_id: u64) -> Result<(), String> {
        let ctx = self
            .pending_ops
            .remove(&operation_id)
            .ok_or_else(|| format!("operation {} not found", operation_id))?;

        let session_id = ctx.1.session_id;
        let completion_ns = Self::now_ns();
        self.completed_ops.insert(operation_id, completion_ns);

        let mut count = self
            .ops_completed_count
            .entry(session_id)
            .or_insert(0);
        *count += 1;

        Ok(())
    }

    /// Checks if an operation's deadline has been exceeded.
    pub fn is_op_deadline_exceeded(&self, operation_id: u64) -> bool {
        let ctx = match self.pending_ops.get(&operation_id) {
            Some(c) => c,
            None => return false,
        };
        ctx.is_deadline_exceeded()
    }

    /// Gets all pending operations for a session.
    pub fn get_session_pending_ops(&self, session_id: SessionId) -> Vec<DistributedOpContext> {
        self.pending_ops
            .iter()
            .filter(|entry| entry.value().session_id == session_id)
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Cleans up expired sessions.
    pub async fn cleanup_expired_sessions(&self) -> Result<usize, String> {
        let now = Self::now_ns();
        let expired_ids: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|entry| entry.value().lease_until_ns <= now)
            .map(|entry| *entry.key())
            .collect();

        for session_id in &expired_ids {
            self.sessions.remove(session_id);
            self.session_to_primary.remove(session_id);
            self.ops_completed_count.remove(session_id);
        }

        Ok(expired_ids.len())
    }

    /// Exports session metrics for monitoring.
    pub fn export_metrics(&self) -> Vec<SessionMetric> {
        self.sessions
            .iter()
            .map(|entry| {
                let session = entry.value();
                let active_ops = self
                    .pending_ops
                    .iter()
                    .filter(|op| op.value().session_id == session.session_id)
                    .count() as u32;
                let completed = self
                    .ops_completed_count
                    .get(&session.session_id)
                    .map(|r| *r)
                    .unwrap_or(0u64);

                SessionMetric {
                    session_id: session.session_id,
                    client_id: session.client_id.clone(),
                    active_ops,
                    lease_remaining_ns: session.lease_remaining_ns() as u64,
                    operations_completed: completed,
                }
            })
            .collect()
    }

    /// Gets the primary node for a session.
    pub fn get_primary_node(&self, session_id: SessionId) -> Option<String> {
        self.session_to_primary.get(&session_id).map(|s| s.clone())
    }
}

impl Default for DistributedSessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_session_succeeds() {
        let manager = DistributedSessionManager::new();
        let session_id = manager
            .create_session(
                "client1".to_string(),
                "/mnt/data".to_string(),
                "node1".to_string(),
                vec!["node2".to_string()],
            )
            .await
            .unwrap();
        assert!(session_id.as_u64() > 0);
    }

    #[test]
    fn test_get_session_returns_correct_session() {
        let manager = DistributedSessionManager::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let session_id = rt.block_on(async {
            manager
                .create_session(
                    "client1".to_string(),
                    "/mnt/data".to_string(),
                    "node1".to_string(),
                    vec![],
                )
                .await
                .unwrap()
        });

        let session = manager.get_session(session_id);
        assert!(session.is_some());
        assert_eq!(session.unwrap().client_id, "client1");
    }

    #[tokio::test]
    async fn test_renew_lease_extends_expiry() {
        let manager = DistributedSessionManager::new();
        let session_id = manager
            .create_session(
                "client1".to_string(),
                "/mnt/data".to_string(),
                "node1".to_string(),
                vec![],
            )
            .await
            .unwrap();

        let before = manager.get_session(session_id).unwrap().lease_until_ns;

        manager
            .renew_lease(session_id, 60_000_000_000)
            .await
            .unwrap();

        let after = manager.get_session(session_id).unwrap().lease_until_ns;
        assert!(after > before);
    }

    #[test]
    fn test_register_op_succeeds() {
        let manager = DistributedSessionManager::new();
        let ctx = DistributedOpContext::new(SessionId(1), 1, 100, 1_000_000_000);
        let result = manager.register_op(ctx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_complete_op_removes_from_pending() {
        let manager = DistributedSessionManager::new();
        let op_id = manager.generate_op_id();
        let ctx = DistributedOpContext::new(SessionId(1), op_id, 100, 1_000_000_000);
        manager.register_op(ctx).unwrap();

        manager.complete_op(op_id).unwrap();

        assert!(manager.pending_ops.get(&op_id).is_none());
    }

    #[test]
    fn test_is_op_deadline_exceeded_true_past_deadline() {
        let manager = DistributedSessionManager::new();
        let ctx = DistributedOpContext::new(SessionId(1), 1, 100, 0); // Already expired
        manager.register_op(ctx).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(1));
        let exceeded = manager.is_op_deadline_exceeded(1);
        assert!(exceeded);
    }

    #[test]
    fn test_is_op_deadline_exceeded_false_before_deadline() {
        let manager = DistributedSessionManager::new();
        let ctx = DistributedOpContext::new(SessionId(1), 1, 100, 10_000_000_000); // 10 seconds
        manager.register_op(ctx).unwrap();

        let exceeded = manager.is_op_deadline_exceeded(1);
        assert!(!exceeded);
    }

    #[test]
    fn test_get_session_pending_ops_returns_all_for_session() {
        let manager = DistributedSessionManager::new();
        let session_id = SessionId(1);

        manager
            .register_op(DistributedOpContext::new(session_id, 1, 100, 1_000_000_000))
            .unwrap();
        manager
            .register_op(DistributedOpContext::new(session_id, 2, 200, 1_000_000_000))
            .unwrap();

        let ops = manager.get_session_pending_ops(session_id);
        assert_eq!(ops.len(), 2);
    }

    #[tokio::test]
    async fn test_cleanup_expired_sessions_removes_stale() {
        let manager = DistributedSessionManager::new();
        let session_id = manager
            .create_session(
                "client1".to_string(),
                "/mnt/data".to_string(),
                "node1".to_string(),
                vec![],
            )
            .await
            .unwrap();

        // Manually expire the session
        {
            let mut session = manager.sessions.get_mut(&session_id).unwrap();
            session.lease_until_ns = 0;
        }

        let cleaned = manager.cleanup_expired_sessions().await.unwrap();
        assert_eq!(cleaned, 1);
        assert!(manager.get_session(session_id).is_none());
    }

    #[test]
    fn test_export_metrics_format_valid() {
        let manager = DistributedSessionManager::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            manager
                .create_session(
                    "client1".to_string(),
                    "/mnt/data".to_string(),
                    "node1".to_string(),
                    vec![],
                )
                .await
                .unwrap();
        });

        let metrics = manager.export_metrics();
        assert!(!metrics.is_empty());
        assert!(metrics[0].session_id.as_u64() > 0);
    }

    #[test]
    fn test_multiple_sessions_isolated() {
        let manager = DistributedSessionManager::new();
        let rt = tokio::runtime::Runtime::new().unwrap();

        let id1 = rt.block_on(async {
            manager
                .create_session(
                    "client1".to_string(),
                    "/mnt/data1".to_string(),
                    "node1".to_string(),
                    vec![],
                )
                .await
                .unwrap()
        });

        let id2 = rt.block_on(async {
            manager
                .create_session(
                    "client2".to_string(),
                    "/mnt/data2".to_string(),
                    "node2".to_string(),
                    vec![],
                )
                .await
                .unwrap()
        });

        assert_ne!(id1, id2);

        let session1 = manager.get_session(id1).unwrap();
        let session2 = manager.get_session(id2).unwrap();
        assert_eq!(session1.client_id, "client1");
        assert_eq!(session2.client_id, "client2");
    }

    #[test]
    fn test_primary_node_mapping_fast_lookup() {
        let manager = DistributedSessionManager::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let session_id = rt.block_on(async {
            manager
                .create_session(
                    "client1".to_string(),
                    "/mnt/data".to_string(),
                    "node1".to_string(),
                    vec!["node2".to_string()],
                )
                .await
                .unwrap()
        });

        let primary = manager.get_primary_node(session_id);
        assert!(primary.is_some());
        assert_eq!(primary.unwrap(), "node1");
    }

    #[test]
    fn test_replica_nodes_stored_in_session() {
        let manager = DistributedSessionManager::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let session_id = rt.block_on(async {
            manager
                .create_session(
                    "client1".to_string(),
                    "/mnt/data".to_string(),
                    "node1".to_string(),
                    vec!["node2".to_string(), "node3".to_string()],
                )
                .await
                .unwrap()
        });

        let session = manager.get_session(session_id).unwrap();
        assert_eq!(session.replica_node_ids.len(), 2);
    }

    #[test]
    fn test_deadline_calculated_from_now_plus_latency_bound() {
        let ctx = DistributedOpContext::new(SessionId(1), 1, 100, 5_000_000_000);
        let now = DistributedOpContext::now_ns();
        assert!(ctx.deadline_ns > now);
        assert!(ctx.deadline_ns <= now + 5_000_000_010);
    }

    #[test]
    fn test_session_creation_timestamp_recorded() {
        let manager = DistributedSessionManager::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let before = DistributedSessionManager::now_ns();
        let session_id = rt.block_on(async {
            manager
                .create_session(
                    "client1".to_string(),
                    "/mnt/data".to_string(),
                    "node1".to_string(),
                    vec![],
                )
                .await
                .unwrap()
        });
        let after = DistributedSessionManager::now_ns();

        let session = manager.get_session(session_id).unwrap();
        assert!(session.created_ns >= before);
        assert!(session.created_ns <= after);
    }

    #[test]
    fn test_concurrent_operation_registration() {
        let manager = Arc::new(DistributedSessionManager::new());
        let mut handles = Vec::new();

        for i in 0..10 {
            let manager = Arc::clone(&manager);
            let handle = std::thread::spawn(move || {
                let ctx = DistributedOpContext::new(SessionId(1), i as u64 + 1, 100, 1_000_000_000);
                manager.register_op(ctx)
            });
            handles.push(handle);
        }

        let mut success_count = 0;
        for handle in handles {
            if handle.join().unwrap().is_ok() {
                success_count += 1;
            }
        }
        assert_eq!(success_count, 10);
    }

    #[test]
    fn test_operation_id_uniqueness() {
        let manager = DistributedSessionManager::new();
        let id1 = manager.generate_op_id();
        let id2 = manager.generate_op_id();
        assert_ne!(id1, id2);
    }

    #[tokio::test]
    async fn test_lease_renewal_updates_timestamp() {
        let manager = DistributedSessionManager::new();
        let session_id = manager
            .create_session(
                "client1".to_string(),
                "/mnt/data".to_string(),
                "node1".to_string(),
                vec![],
            )
            .await
            .unwrap();

        let original_lease = manager.get_session(session_id).unwrap().lease_until_ns;

        manager
            .renew_lease(session_id, 100_000_000_000)
            .await
            .unwrap();

        let new_lease = manager.get_session(session_id).unwrap().lease_until_ns;
        assert!(new_lease > original_lease);
    }
}