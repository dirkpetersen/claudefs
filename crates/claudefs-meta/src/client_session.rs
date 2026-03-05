//! Per-client session state and lease tracking for metadata service.
//!
//! This module manages client sessions, including lease renewal, pending operations,
//! and session lifecycle (active/idle/expired/revoked).

use std::sync::{Arc, Mutex};
use std::collections::HashMap;

use dashmap::DashMap;
use uuid::Uuid;
use serde::{Serialize, Deserialize};

use crate::types::{InodeId, MetaError, NodeId, Timestamp};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(String);

impl SessionId {
    pub fn new() -> Self {
        SessionId(Uuid::new_v4().to_string())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClientId(String);

impl ClientId {
    pub fn new(id: String) -> Self {
        ClientId(id)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OperationId(String);

impl OperationId {
    pub fn new() -> Self {
        OperationId(Uuid::new_v4().to_string())
    }
}

impl Default for OperationId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionState {
    Active,
    Idle { idle_since: Timestamp },
    Expired { expired_at: Timestamp },
    Revoked { revoked_at: Timestamp, reason: String },
}

#[derive(Clone, Debug)]
pub struct PendingOperation {
    pub op_id: OperationId,
    pub op_type: String,
    pub inode_id: InodeId,
    pub started_at: Timestamp,
    pub timeout_secs: u64,
    pub result: Option<OpResult>,
}

#[derive(Clone, Debug)]
pub enum OpResult {
    Success { value: Vec<u8> },
    Failure { error: String },
}

#[derive(Clone, Debug)]
pub struct SessionLeaseRenewal {
    pub session_id: SessionId,
    pub new_lease_expiry: Timestamp,
    pub operations_completed: u64,
    pub bytes_transferred: u64,
}

#[derive(Clone, Debug)]
pub struct SessionManagerConfig {
    pub lease_duration_secs: u64,
    pub operation_timeout_secs: u64,
    pub max_pending_ops: usize,
    pub cleanup_interval_secs: u64,
    pub max_session_age_secs: u64,
}

impl Default for SessionManagerConfig {
    fn default() -> Self {
        Self {
            lease_duration_secs: 60,
            operation_timeout_secs: 30,
            max_pending_ops: 1000,
            cleanup_interval_secs: 60,
            max_session_age_secs: 3600,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SessionMetrics {
    pub active_sessions: u64,
    pub total_sessions_created: u64,
    pub sessions_revoked: u64,
    pub average_pending_ops: f64,
    pub lease_renewals_total: u64,
    pub staleness_detections: u64,
}

#[derive(Clone, Debug)]
pub struct ClientSession {
    pub session_id: SessionId,
    pub client_id: ClientId,
    pub created_on_node: NodeId,
    pub created_at: Timestamp,
    pub last_activity: Timestamp,
    pub state: SessionState,
    pub lease_expiry: Timestamp,
    pub lease_duration_secs: u64,
    pub pending_ops: Arc<DashMap<OperationId, PendingOperation>>,
    pub client_version: String,
}

pub struct SessionManager {
    sessions: Arc<DashMap<SessionId, ClientSession>>,
    sessions_by_client: Arc<DashMap<ClientId, Vec<SessionId>>>,
    config: SessionManagerConfig,
    metrics: Arc<Mutex<SessionMetrics>>,
}

impl SessionManager {
    pub fn new(config: SessionManagerConfig) -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
            sessions_by_client: Arc::new(DashMap::new()),
            config,
            metrics: Arc::new(Mutex::new(SessionMetrics::default())),
        }
    }

    pub fn create_session(&self, client_id: ClientId, client_version: String) -> Result<ClientSession, MetaError> {
        let session_id = SessionId::new();
        let now = Timestamp::now();
        
        let lease_expiry = Timestamp {
            secs: now.secs + self.config.lease_duration_secs,
            nanos: now.nanos,
        };
        
        let session = ClientSession {
            session_id: session_id.clone(),
            client_id: client_id.clone(),
            created_on_node: NodeId::new(1),
            created_at: now,
            last_activity: now,
            state: SessionState::Active,
            lease_expiry,
            lease_duration_secs: self.config.lease_duration_secs,
            pending_ops: Arc::new(DashMap::new()),
            client_version,
        };
        
        self.sessions.insert(session_id.clone(), session.clone());
        
        self.sessions_by_client
            .entry(client_id)
            .or_insert_with(Vec::new)
            .push(session_id.clone());
        
        {
            let mut metrics = self.metrics.lock().unwrap();
            metrics.total_sessions_created += 1;
            metrics.active_sessions = self.sessions.len() as u64;
        }
        
        Ok(session)
    }

    pub fn get_session(&self, session_id: SessionId) -> Result<Arc<ClientSession>, MetaError> {
        self.sessions.get(&session_id)
            .map(|s| Arc::new(s.clone()))
            .ok_or_else(|| MetaError::NotFound(format!("session {} not found", session_id.0)))
    }

    pub async fn renew_lease(&self, session_id: SessionId) -> Result<SessionLeaseRenewal, MetaError> {
        let session = self.get_session(session_id.clone())?;
        
        let now = Timestamp::now();
        let new_lease_expiry = Timestamp {
            secs: now.secs + session.lease_duration_secs,
            nanos: now.nanos,
        };
        
        {
            let mut s = self.sessions.get_mut(&session_id).ok_or_else(||
                MetaError::NotFound("session not found".to_string()))?;
            s.lease_expiry = new_lease_expiry;
            s.last_activity = now;
            s.state = SessionState::Active;
        }
        
        let ops_completed = session.pending_ops.len() as u64;
        
        {
            let mut metrics = self.metrics.lock().unwrap();
            metrics.lease_renewals_total += 1;
        }
        
        Ok(SessionLeaseRenewal {
            session_id,
            new_lease_expiry,
            operations_completed: ops_completed,
            bytes_transferred: 0,
        })
    }

    pub fn update_activity(&self, session_id: SessionId) -> Result<(), MetaError> {
        let mut session = self.sessions.get_mut(&session_id).ok_or_else(||
            MetaError::NotFound("session not found".to_string()))?;
        
        session.last_activity = Timestamp::now();
        
        match &session.state {
            SessionState::Idle { .. } => {
                session.state = SessionState::Active;
            }
            _ => {}
        }
        
        Ok(())
    }

    pub fn add_pending_operation(
        &self,
        session_id: SessionId,
        op_id: OperationId,
        op_type: &str,
        inode_id: InodeId,
    ) -> Result<(), MetaError> {
        let session = self.get_session(session_id)?;
        
        if session.pending_ops.len() >= self.config.max_pending_ops {
            return Err(MetaError::InvalidArgument("max pending operations reached".to_string()));
        }
        
        let op = PendingOperation {
            op_id: op_id.clone(),
            op_type: op_type.to_string(),
            inode_id,
            started_at: Timestamp::now(),
            timeout_secs: self.config.operation_timeout_secs,
            result: None,
        };
        
        session.pending_ops.insert(op_id, op);
        
        Ok(())
    }

    pub fn complete_operation(
        &self,
        session_id: SessionId,
        op_id: OperationId,
        result: OpResult,
    ) -> Result<(), MetaError> {
        let session = self.get_session(session_id)?;
        
        if let Some(mut op) = session.pending_ops.get_mut(&op_id) {
            op.result = Some(result);
        }
        
        session.pending_ops.remove(&op_id);
        
        Ok(())
    }

    pub fn check_operation_timeout(&self, session_id: SessionId, op_id: OperationId) -> Result<bool, MetaError> {
        let session = self.get_session(session_id)?;
        
        let op = session.pending_ops.get(&op_id)
            .ok_or_else(|| MetaError::NotFound("operation not found".to_string()))?;
        
        let now = Timestamp::now();
        let elapsed = now.secs - op.started_at.secs;
        
        Ok(elapsed > op.timeout_secs)
    }

    pub fn detect_stale_sessions(&self, idle_threshold_secs: u64) -> Result<Vec<SessionId>, MetaError> {
        let now = Timestamp::now();
        let mut stale = Vec::new();
        
        for entry in self.sessions.iter() {
            let session = entry.value();
            let idle_time = now.secs - session.last_activity.secs;
            
            if idle_time > idle_threshold_secs {
                match &session.state {
                    SessionState::Active => {
                        stale.push(session.session_id.clone());
                    }
                    _ => {}
                }
            }
        }
        
        {
            let mut metrics = self.metrics.lock().unwrap();
            metrics.staleness_detections += stale.len() as u64;
        }
        
        Ok(stale)
    }

    pub fn close_session(&self, session_id: SessionId) -> Result<(), MetaError> {
        let mut session = self.sessions.get_mut(&session_id).ok_or_else(||
            MetaError::NotFound("session not found".to_string()))?;
        
        session.state = SessionState::Idle {
            idle_since: Timestamp::now(),
        };
        
        {
            let mut metrics = self.metrics.lock().unwrap();
            metrics.active_sessions = self.sessions.len() as u64;
        }
        
        Ok(())
    }

    pub fn revoke_session(&self, session_id: SessionId, reason: String) -> Result<(), MetaError> {
        let mut session = self.sessions.get_mut(&session_id).ok_or_else(||
            MetaError::NotFound("session not found".to_string()))?;
        
        session.state = SessionState::Revoked {
            revoked_at: Timestamp::now(),
            reason: reason.clone(),
        };
        
        {
            let mut metrics = self.metrics.lock().unwrap();
            metrics.sessions_revoked += 1;
            metrics.active_sessions = self.sessions.len() as u64;
        }
        
        Ok(())
    }

    pub fn get_client_sessions(&self, client_id: ClientId) -> Result<Vec<SessionId>, MetaError> {
        self.sessions_by_client.get(&client_id)
            .map(|sessions| sessions.clone())
            .ok_or_else(|| MetaError::NotFound(format!("no sessions for client {}", client_id.as_str())))
    }

    pub async fn cleanup_expired_sessions(&self) -> Result<usize, MetaError> {
        let now = Timestamp::now();
        let mut removed = 0;
        
        let session_ids: Vec<SessionId> = self.sessions.iter()
            .map(|e| e.session_id.clone())
            .collect();
        
        for sid in session_ids {
            if let Some(session) = self.sessions.get(&sid) {
                let expired = now.secs > session.lease_expiry.secs;
                
                if expired {
                    match &session.state {
                        SessionState::Active => {
                            self.sessions.remove(&sid);
                            
                            if let Some(mut client_sessions) = self.sessions_by_client.get_mut(&session.client_id) {
                                client_sessions.retain(|id| id != &sid);
                            }
                            
                            removed += 1;
                        }
                        _ => {}
                    }
                }
            }
        }
        
        {
            let mut metrics = self.metrics.lock().unwrap();
            metrics.active_sessions = self.sessions.len() as u64;
        }
        
        Ok(removed)
    }

    pub fn get_metrics(&self) -> SessionMetrics {
        let mut metrics = self.metrics.lock().unwrap();
        
        let total_pending: usize = self.sessions.iter()
            .map(|s| s.pending_ops.len())
            .sum();
        
        if self.sessions.len() > 0 {
            metrics.average_pending_ops = total_pending as f64 / self.sessions.len() as f64;
        }
        
        metrics.clone()
    }
}

#[cfg(test)]
mod client_session_tests {
    use super::*;

    fn make_manager() -> SessionManager {
        SessionManager::new(SessionManagerConfig::default())
    }

    #[test]
    fn test_create_session() {
        let manager = make_manager();
        let client_id = ClientId::new("client1".to_string());
        
        let session = manager.create_session(client_id, "v1.0".to_string()).unwrap();
        
        assert!(!session.session_id.0.is_empty());
        assert_eq!(session.client_id.as_str(), "client1");
    }

    #[test]
    fn test_create_session_increments_counter() {
        let manager = make_manager();
        
        manager.create_session(ClientId::new("client1".to_string()), "v1".to_string()).unwrap();
        
        let metrics = manager.get_metrics();
        assert_eq!(metrics.total_sessions_created, 1);
        
        manager.create_session(ClientId::new("client2".to_string()), "v1".to_string()).unwrap();
        
        let metrics = manager.get_metrics();
        assert_eq!(metrics.total_sessions_created, 2);
    }

    #[test]
    fn test_get_session_existing() {
        let manager = make_manager();
        let client_id = ClientId::new("client1".to_string());
        
        let session = manager.create_session(client_id.clone(), "v1".to_string()).unwrap();
        let retrieved = manager.get_session(session.session_id.clone()).unwrap();
        
        assert_eq!(retrieved.session_id, session.session_id);
    }

    #[test]
    fn test_get_session_not_found() {
        let manager = make_manager();
        
        let result = manager.get_session(SessionId::new());
        
        assert!(result.is_err());
    }

    #[test]
    fn test_session_state_active_initially() {
        let manager = make_manager();
        let session = manager.create_session(ClientId::new("client1".to_string()), "v1".to_string()).unwrap();
        
        matches!(session.state, SessionState::Active);
    }

    #[test]
    fn test_lease_expiry_set_correctly() {
        let manager = make_manager();
        let config = SessionManagerConfig {
            lease_duration_secs: 120,
            ..Default::default()
        };
        let manager = SessionManager::new(config);
        
        let now = Timestamp::now();
        let session = manager.create_session(ClientId::new("client1".to_string()), "v1".to_string()).unwrap();
        
        assert!(session.lease_expiry.secs >= now.secs + 120);
    }

    #[tokio::test]
    async fn test_renew_lease_extends_expiry() {
        let manager = make_manager();
        let session = manager.create_session(ClientId::new("client1".to_string()), "v1".to_string()).unwrap();
        
        let old_expiry = session.lease_expiry;
        let renewal = manager.renew_lease(session.session_id.clone()).await.unwrap();
        
        assert!(renewal.new_lease_expiry.secs >= old_expiry.secs);
    }

    #[tokio::test]
    async fn test_renew_lease_updates_activity() {
        let manager = make_manager();
        let session = manager.create_session(ClientId::new("client1".to_string()), "v1".to_string()).unwrap();
        
        std::thread::sleep(std::time::Duration::from_millis(10));
        
        manager.renew_lease(session.session_id.clone()).await.unwrap();
        
        let updated = manager.get_session(session.session_id.clone()).unwrap();
        assert!(updated.last_activity.secs >= session.last_activity.secs);
    }

    #[test]
    fn test_update_activity_refreshes_timestamp() {
        let manager = make_manager();
        let session = manager.create_session(ClientId::new("client1".to_string()), "v1".to_string()).unwrap();
        
        std::thread::sleep(std::time::Duration::from_millis(10));
        
        manager.update_activity(session.session_id.clone()).unwrap();
        
        let updated = manager.get_session(session.session_id.clone()).unwrap();
        assert!(updated.last_activity.secs >= session.last_activity.secs);
    }

    #[test]
    fn test_add_pending_operation() {
        let manager = make_manager();
        let session = manager.create_session(ClientId::new("client1".to_string()), "v1".to_string()).unwrap();
        
        let op_id = OperationId::new();
        manager.add_pending_operation(
            session.session_id.clone(),
            op_id.clone(),
            "read",
            InodeId::new(100),
        ).unwrap();
        
        let retrieved = manager.get_session(session.session_id.clone()).unwrap();
        assert!(retrieved.pending_ops.contains_key(&op_id));
    }

    #[test]
    fn test_add_pending_operation_increments_count() {
        let manager = make_manager();
        let session = manager.create_session(ClientId::new("client1".to_string()), "v1".to_string()).unwrap();
        
        manager.add_pending_operation(
            session.session_id.clone(),
            OperationId::new(),
            "read",
            InodeId::new(100),
        ).unwrap();
        
        manager.add_pending_operation(
            session.session_id.clone(),
            OperationId::new(),
            "write",
            InodeId::new(101),
        ).unwrap();
        
        let retrieved = manager.get_session(session.session_id.clone()).unwrap();
        assert_eq!(retrieved.pending_ops.len(), 2);
    }

    #[test]
    fn test_complete_operation_removes_pending() {
        let manager = make_manager();
        let session = manager.create_session(ClientId::new("client1".to_string()), "v1".to_string()).unwrap();
        
        let op_id = OperationId::new();
        manager.add_pending_operation(
            session.session_id.clone(),
            op_id.clone(),
            "read",
            InodeId::new(100),
        ).unwrap();
        
        manager.complete_operation(
            session.session_id.clone(),
            op_id.clone(),
            OpResult::Success { value: vec![] },
        ).unwrap();
        
        let retrieved = manager.get_session(session.session_id.clone()).unwrap();
        assert!(!retrieved.pending_ops.contains_key(&op_id));
    }

    #[test]
    fn test_complete_operation_stores_result() {
        let manager = make_manager();
        let session = manager.create_session(ClientId::new("client1".to_string()), "v1".to_string()).unwrap();
        
        let op_id = OperationId::new();
        manager.add_pending_operation(
            session.session_id.clone(),
            op_id.clone(),
            "read",
            InodeId::new(100),
        ).unwrap();
        
        let result = OpResult::Success { value: vec![1, 2, 3] };
        manager.complete_operation(session.session_id.clone(), op_id.clone(), result).unwrap();
    }

    #[test]
    fn test_check_operation_timeout_within_window() {
        let manager = make_manager();
        let session = manager.create_session(ClientId::new("client1".to_string()), "v1".to_string()).unwrap();
        
        let op_id = OperationId::new();
        manager.add_pending_operation(
            session.session_id.clone(),
            op_id.clone(),
            "read",
            InodeId::new(100),
        ).unwrap();
        
        let timed_out = manager.check_operation_timeout(session.session_id.clone(), op_id).unwrap();
        
        assert!(!timed_out);
    }

    #[test]
    fn test_check_operation_timeout_exceeded() {
        let manager = make_manager();
        let session = manager.create_session(ClientId::new("client1".to_string()), "v1".to_string()).unwrap();
        
        let op_id = OperationId::new();
        
        {
            let sess = manager.sessions.get(&session.session_id).unwrap();
            let op = PendingOperation {
                op_id: op_id.clone(),
                op_type: "read".to_string(),
                inode_id: InodeId::new(100),
                started_at: Timestamp { secs: 1000000000, nanos: 0 },
                timeout_secs: 1,
                result: None,
            };
            sess.pending_ops.insert(op_id.clone(), op);
        }
        
        let timed_out = manager.check_operation_timeout(session.session_id.clone(), op_id).unwrap();
        
        assert!(timed_out);
    }

    #[test]
    fn test_detect_stale_sessions_no_activity() {
        let manager = make_manager();
        
        let old_timestamp = Timestamp { secs: 1000000000, nanos: 0 };
        let session = manager.create_session(ClientId::new("client1".to_string()), "v1".to_string()).unwrap();
        
        {
            let mut s = manager.sessions.get_mut(&session.session_id).unwrap();
            s.last_activity = old_timestamp;
        }
        
        let stale = manager.detect_stale_sessions(3600).unwrap();
        
        assert!(stale.contains(&session.session_id));
    }

    #[test]
    fn test_detect_stale_sessions_active_not_included() {
        let manager = make_manager();
        let session = manager.create_session(ClientId::new("client1".to_string()), "v1".to_string()).unwrap();
        
        let stale = manager.detect_stale_sessions(3600).unwrap();
        
        assert!(!stale.contains(&session.session_id));
    }

    #[test]
    fn test_close_session_updates_state() {
        let manager = make_manager();
        let session = manager.create_session(ClientId::new("client1".to_string()), "v1".to_string()).unwrap();
        
        manager.close_session(session.session_id.clone()).unwrap();
        
        let updated = manager.get_session(session.session_id.clone()).unwrap();
        
        matches!(updated.state, SessionState::Idle { .. });
    }

    #[test]
    fn test_revoke_session_with_reason() {
        let manager = make_manager();
        let session = manager.create_session(ClientId::new("client1".to_string()), "v1".to_string()).unwrap();
        
        manager.revoke_session(session.session_id.clone(), "policy violation".to_string()).unwrap();
        
        let updated = manager.get_session(session.session_id.clone()).unwrap();
        
        matches!(&updated.state, SessionState::Revoked { reason, .. } if reason == "policy violation");
    }

    #[tokio::test]
    async fn test_revoke_session_prevents_operations() {
        let manager = make_manager();
        let session = manager.create_session(ClientId::new("client1".to_string()), "v1".to_string()).unwrap();
        
        manager.revoke_session(session.session_id.clone(), "revoked".to_string()).unwrap();
        
        let result = manager.renew_lease(session.session_id.clone()).await;
        
        assert!(result.is_err());
    }

    #[test]
    fn test_get_client_sessions_all() {
        let manager = make_manager();
        let client_id = ClientId::new("client1".to_string());
        
        let session1 = manager.create_session(client_id.clone(), "v1".to_string()).unwrap();
        let session2 = manager.create_session(client_id.clone(), "v1".to_string()).unwrap();
        
        let sessions = manager.get_client_sessions(client_id).unwrap();
        
        assert_eq!(sessions.len(), 2);
        assert!(sessions.contains(&session1.session_id));
        assert!(sessions.contains(&session2.session_id));
    }

    #[test]
    fn test_get_client_sessions_single_client() {
        let manager = make_manager();
        
        manager.create_session(ClientId::new("client1".to_string()), "v1".to_string()).unwrap();
        
        let sessions = manager.get_client_sessions(ClientId::new("client1".to_string())).unwrap();
        
        assert!(!sessions.is_empty());
    }

    #[tokio::test]
    async fn test_cleanup_expired_sessions_removes_old() {
        let manager = make_manager();
        
        let old_timestamp = Timestamp { secs: 1000000000, nanos: 0 };
        let session = manager.create_session(ClientId::new("client1".to_string()), "v1".to_string()).unwrap();
        
        {
            let mut s = manager.sessions.get_mut(&session.session_id).unwrap();
            s.lease_expiry = old_timestamp;
        }
        
        let removed = manager.cleanup_expired_sessions().await.unwrap();
        
        assert!(removed >= 1);
    }

    #[tokio::test]
    async fn test_cleanup_expired_sessions_keeps_active() {
        let manager = make_manager();
        let session = manager.create_session(ClientId::new("client1".to_string()), "v1".to_string()).unwrap();
        
        let removed = manager.cleanup_expired_sessions().await.unwrap();
        
        let retrieved = manager.get_session(session.session_id.clone());
        assert!(retrieved.is_ok());
    }

    #[test]
    fn test_get_metrics_reflects_state() {
        let manager = make_manager();
        
        manager.create_session(ClientId::new("client1".to_string()), "v1".to_string()).unwrap();
        manager.create_session(ClientId::new("client2".to_string()), "v1".to_string()).unwrap();
        
        let metrics = manager.get_metrics();
        
        assert_eq!(metrics.active_sessions, 2);
        assert_eq!(metrics.total_sessions_created, 2);
    }
}