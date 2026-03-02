//! Client session management

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

/// Session identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

impl SessionId {
    /// Creates a new session ID from a raw u64 value.
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// Returns the underlying u64 value.
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

/// Session protocol type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionProtocol {
    /// NFSv3 protocol
    Nfs3,
    /// S3 protocol
    S3,
    /// SMB3 protocol
    Smb3,
}

/// Client session state
#[derive(Debug, Clone)]
pub struct ClientSession {
    /// Session ID
    pub id: SessionId,
    /// Protocol type
    pub protocol: SessionProtocol,
    /// Client IP address
    pub client_ip: String,
    /// Effective UID
    pub uid: u32,
    /// Effective GID
    pub gid: u32,
    /// Creation timestamp
    pub created_at: u64,
    /// Last activity timestamp
    pub last_active: u64,
    /// Operation count
    pub op_count: u64,
    /// Total bytes transferred
    pub bytes_transferred: u64,
    /// Mounted paths
    pub mounts: Vec<String>,
}

impl ClientSession {
    /// Creates a new client session with the given parameters.
    pub fn new(
        id: SessionId,
        protocol: SessionProtocol,
        client_ip: &str,
        uid: u32,
        gid: u32,
        now: u64,
    ) -> Self {
        Self {
            id,
            protocol,
            client_ip: client_ip.to_string(),
            uid,
            gid,
            created_at: now,
            last_active: now,
            op_count: 0,
            bytes_transferred: 0,
            mounts: Vec::new(),
        }
    }

    /// Updates the last activity timestamp to the current time.
    pub fn touch(&mut self, now: u64) {
        self.last_active = now;
    }

    /// Records an operation execution, updating activity time and counters.
    pub fn record_op(&mut self, now: u64, bytes: u64) {
        self.last_active = now;
        self.op_count += 1;
        self.bytes_transferred += bytes;
    }

    /// Adds a mounted path to the session if not already present.
    pub fn add_mount(&mut self, path: &str) {
        if !self.mounts.contains(&path.to_string()) {
            self.mounts.push(path.to_string());
        }
    }

    /// Removes a mounted path from the session.
    pub fn remove_mount(&mut self, path: &str) {
        self.mounts.retain(|p| p != path);
    }

    /// Returns true if the session has been idle longer than the specified timeout.
    pub fn is_idle(&self, now: u64, timeout_secs: u64) -> bool {
        now.saturating_sub(self.last_active) > timeout_secs
    }
}

/// Session manager - tracks client sessions across all protocols
pub struct SessionManager {
    sessions: RwLock<HashMap<SessionId, ClientSession>>,
    next_id: AtomicU64,
}

impl SessionManager {
    /// Creates a new session manager with empty session tracking.
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    /// Creates a new client session and returns its ID.
    pub fn create_session(
        &self,
        protocol: SessionProtocol,
        client_ip: &str,
        uid: u32,
        gid: u32,
        now: u64,
    ) -> SessionId {
        let id = SessionId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let session = ClientSession::new(id, protocol, client_ip, uid, gid, now);

        if let Ok(mut sessions) = self.sessions.write() {
            sessions.insert(id, session);
        }

        id
    }

    /// Retrieves a session by ID, if it exists.
    pub fn get_session(&self, id: SessionId) -> Option<ClientSession> {
        self.sessions.read().ok()?.get(&id).cloned()
    }

    /// Updates the last activity time for a session.
    pub fn touch_session(&self, id: SessionId, now: u64) {
        if let Ok(mut sessions) = self.sessions.write() {
            if let Some(session) = sessions.get_mut(&id) {
                session.touch(now);
            }
        }
    }

    /// Records an operation for a session.
    pub fn record_op(&self, id: SessionId, now: u64, bytes: u64) {
        if let Ok(mut sessions) = self.sessions.write() {
            if let Some(session) = sessions.get_mut(&id) {
                session.record_op(now, bytes);
            }
        }
    }

    /// Adds a mount path to a session.
    pub fn add_mount(&self, id: SessionId, path: &str) {
        if let Ok(mut sessions) = self.sessions.write() {
            if let Some(session) = sessions.get_mut(&id) {
                session.add_mount(path);
            }
        }
    }

    /// Removes a mount path from a session.
    pub fn remove_mount(&self, id: SessionId, path: &str) {
        if let Ok(mut sessions) = self.sessions.write() {
            if let Some(session) = sessions.get_mut(&id) {
                session.remove_mount(path);
            }
        }
    }

    /// Ends and removes a session, returning true if it existed.
    pub fn end_session(&self, id: SessionId) -> bool {
        if let Ok(mut sessions) = self.sessions.write() {
            sessions.remove(&id).is_some()
        } else {
            false
        }
    }

    /// Returns all active sessions.
    pub fn list_sessions(&self) -> Vec<ClientSession> {
        self.sessions
            .read()
            .ok()
            .map(|s| s.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Returns all sessions for a specific protocol.
    pub fn sessions_for_protocol(&self, protocol: SessionProtocol) -> Vec<ClientSession> {
        self.sessions
            .read()
            .ok()
            .map(|s| {
                s.values()
                    .filter(|session| session.protocol == protocol)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Removes all idle sessions older than the timeout. Returns count of expired sessions.
    pub fn expire_idle(&self, now: u64, timeout_secs: u64) -> usize {
        if let Ok(mut sessions) = self.sessions.write() {
            let count_before = sessions.len();
            sessions.retain(|_, session| !session.is_idle(now, timeout_secs));
            count_before - sessions.len()
        } else {
            0
        }
    }

    /// Returns the total number of active sessions.
    pub fn count(&self) -> usize {
        self.sessions.read().ok().map(|s| s.len()).unwrap_or(0)
    }

    /// Returns the total number of operations across all sessions.
    pub fn total_ops(&self) -> u64 {
        self.sessions
            .read()
            .ok()
            .map(|s| s.values().map(|sess| sess.op_count).sum())
            .unwrap_or(0)
    }

    /// Returns the total bytes transferred across all sessions.
    pub fn total_bytes(&self) -> u64 {
        self.sessions
            .read()
            .ok()
            .map(|s| s.values().map(|sess| sess.bytes_transferred).sum())
            .unwrap_or(0)
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_id_new() {
        let id = SessionId::new(123);
        assert_eq!(id.as_u64(), 123);
    }

    #[test]
    fn test_client_session_new() {
        let id = SessionId::new(1);
        let session = ClientSession::new(id, SessionProtocol::Nfs3, "192.168.1.1", 1000, 1000, 100);

        assert_eq!(session.id, id);
        assert_eq!(session.protocol, SessionProtocol::Nfs3);
        assert_eq!(session.client_ip, "192.168.1.1");
        assert_eq!(session.uid, 1000);
        assert_eq!(session.gid, 1000);
        assert_eq!(session.created_at, 100);
        assert_eq!(session.last_active, 100);
        assert_eq!(session.op_count, 0);
    }

    #[test]
    fn test_client_session_touch() {
        let id = SessionId::new(1);
        let mut session = ClientSession::new(id, SessionProtocol::S3, "10.0.0.1", 500, 500, 100);

        session.touch(200);

        assert_eq!(session.last_active, 200);
    }

    #[test]
    fn test_client_session_record_op() {
        let id = SessionId::new(1);
        let mut session = ClientSession::new(id, SessionProtocol::Nfs3, "10.0.0.1", 0, 0, 100);

        session.record_op(150, 4096);

        assert_eq!(session.op_count, 1);
        assert_eq!(session.bytes_transferred, 4096);
        assert_eq!(session.last_active, 150);
    }

    #[test]
    fn test_client_session_add_mount() {
        let id = SessionId::new(1);
        let mut session = ClientSession::new(id, SessionProtocol::Nfs3, "10.0.0.1", 0, 0, 100);

        session.add_mount("/export");
        session.add_mount("/data");

        assert_eq!(session.mounts.len(), 2);
    }

    #[test]
    fn test_client_session_add_mount_duplicate() {
        let id = SessionId::new(1);
        let mut session = ClientSession::new(id, SessionProtocol::Nfs3, "10.0.0.1", 0, 0, 100);

        session.add_mount("/export");
        session.add_mount("/export");

        assert_eq!(session.mounts.len(), 1);
    }

    #[test]
    fn test_client_session_remove_mount() {
        let id = SessionId::new(1);
        let mut session = ClientSession::new(id, SessionProtocol::Nfs3, "10.0.0.1", 0, 0, 100);

        session.add_mount("/export");
        session.add_mount("/data");
        session.remove_mount("/export");

        assert_eq!(session.mounts.len(), 1);
        assert!(!session.mounts.contains(&"/export".to_string()));
    }

    #[test]
    fn test_client_session_is_idle() {
        let id = SessionId::new(1);
        let session = ClientSession::new(id, SessionProtocol::Nfs3, "10.0.0.1", 0, 0, 100);

        assert!(!session.is_idle(150, 60));
        assert!(session.is_idle(200, 60));
    }

    #[test]
    fn test_session_manager_new() {
        let manager = SessionManager::new();
        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn test_session_manager_create_session() {
        let manager = SessionManager::new();
        let id = manager.create_session(SessionProtocol::Nfs3, "192.168.1.1", 1000, 1000, 100);

        assert!(id.as_u64() > 0);
        assert_eq!(manager.count(), 1);
    }

    #[test]
    fn test_session_manager_get_session() {
        let manager = SessionManager::new();
        let id = manager.create_session(SessionProtocol::Nfs3, "192.168.1.1", 1000, 1000, 100);

        let session = manager.get_session(id);
        assert!(session.is_some());
        assert_eq!(session.unwrap().uid, 1000);
    }

    #[test]
    fn test_session_manager_get_nonexistent() {
        let manager = SessionManager::new();
        let session = manager.get_session(SessionId::new(999));
        assert!(session.is_none());
    }

    #[test]
    fn test_session_manager_touch_session() {
        let manager = SessionManager::new();
        let id = manager.create_session(SessionProtocol::Nfs3, "192.168.1.1", 1000, 1000, 100);

        manager.touch_session(id, 200);

        let session = manager.get_session(id).unwrap();
        assert_eq!(session.last_active, 200);
    }

    #[test]
    fn test_session_manager_record_op() {
        let manager = SessionManager::new();
        let id = manager.create_session(SessionProtocol::S3, "10.0.0.1", 500, 500, 100);

        manager.record_op(id, 150, 8192);

        let session = manager.get_session(id).unwrap();
        assert_eq!(session.op_count, 1);
        assert_eq!(session.bytes_transferred, 8192);
    }

    #[test]
    fn test_session_manager_add_mount() {
        let manager = SessionManager::new();
        let id = manager.create_session(SessionProtocol::Nfs3, "192.168.1.1", 1000, 1000, 100);

        manager.add_mount(id, "/export");

        let session = manager.get_session(id).unwrap();
        assert_eq!(session.mounts.len(), 1);
    }

    #[test]
    fn test_session_manager_remove_mount() {
        let manager = SessionManager::new();
        let id = manager.create_session(SessionProtocol::Nfs3, "192.168.1.1", 1000, 1000, 100);

        manager.add_mount(id, "/export");
        manager.remove_mount(id, "/export");

        let session = manager.get_session(id).unwrap();
        assert!(session.mounts.is_empty());
    }

    #[test]
    fn test_session_manager_end_session() {
        let manager = SessionManager::new();
        let id = manager.create_session(SessionProtocol::Nfs3, "192.168.1.1", 1000, 1000, 100);

        let result = manager.end_session(id);

        assert!(result);
        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn test_session_manager_end_nonexistent() {
        let manager = SessionManager::new();
        let result = manager.end_session(SessionId::new(999));
        assert!(!result);
    }

    #[test]
    fn test_session_manager_list_sessions() {
        let manager = SessionManager::new();
        let _ = manager.create_session(SessionProtocol::Nfs3, "192.168.1.1", 1000, 1000, 100);
        let _ = manager.create_session(SessionProtocol::S3, "10.0.0.1", 500, 500, 100);

        let sessions = manager.list_sessions();
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn test_session_manager_sessions_for_protocol() {
        let manager = SessionManager::new();
        let _ = manager.create_session(SessionProtocol::Nfs3, "192.168.1.1", 1000, 1000, 100);
        let _ = manager.create_session(SessionProtocol::S3, "10.0.0.1", 500, 500, 100);

        let nfs_sessions = manager.sessions_for_protocol(SessionProtocol::Nfs3);
        assert_eq!(nfs_sessions.len(), 1);

        let s3_sessions = manager.sessions_for_protocol(SessionProtocol::S3);
        assert_eq!(s3_sessions.len(), 1);
    }

    #[test]
    fn test_session_manager_expire_idle() {
        let manager = SessionManager::new();
        let id1 = manager.create_session(SessionProtocol::Nfs3, "192.168.1.1", 1000, 1000, 100);
        let id2 = manager.create_session(SessionProtocol::Nfs3, "10.0.0.1", 500, 500, 300);

        manager.touch_session(id2, 350);

        let expired = manager.expire_idle(400, 60);

        assert_eq!(expired, 1);
        assert_eq!(manager.count(), 1);
    }

    #[test]
    fn test_session_manager_count() {
        let manager = SessionManager::new();
        let _ = manager.create_session(SessionProtocol::Nfs3, "192.168.1.1", 1000, 1000, 100);
        let _ = manager.create_session(SessionProtocol::S3, "10.0.0.1", 500, 500, 100);

        assert_eq!(manager.count(), 2);
    }

    #[test]
    fn test_session_manager_total_ops() {
        let manager = SessionManager::new();
        let id1 = manager.create_session(SessionProtocol::Nfs3, "192.168.1.1", 1000, 1000, 100);
        let id2 = manager.create_session(SessionProtocol::S3, "10.0.0.1", 500, 500, 100);

        manager.record_op(id1, 150, 4096);
        manager.record_op(id1, 160, 4096);
        manager.record_op(id2, 170, 8192);

        assert_eq!(manager.total_ops(), 3);
    }

    #[test]
    fn test_session_manager_total_bytes() {
        let manager = SessionManager::new();
        let id1 = manager.create_session(SessionProtocol::Nfs3, "192.168.1.1", 1000, 1000, 100);
        let id2 = manager.create_session(SessionProtocol::S3, "10.0.0.1", 500, 500, 100);

        manager.record_op(id1, 150, 4096);
        manager.record_op(id2, 170, 8192);

        assert_eq!(manager.total_bytes(), 12288);
    }

    #[test]
    fn test_session_protocol_equality() {
        assert_eq!(SessionProtocol::Nfs3, SessionProtocol::Nfs3);
        assert_ne!(SessionProtocol::Nfs3, SessionProtocol::S3);
    }
}
