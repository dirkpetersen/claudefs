//! NFSv4.1 Session Management (RFC 5661).
//!
//! Implements NFSv4.1 client registration (EXCHANGE_ID), session creation
//! (CREATE_SESSION), and slot table management for exactly-once semantics.
//! Sessions track per-slot sequence numbers to detect and handle replayed requests.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use thiserror::Error;
use tracing::{debug, info, warn};

/// NFSv4 client ID (64-bit opaque)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClientId(pub u64);

/// NFSv4.1 session ID (16 bytes, globally unique)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub [u8; 16]);

impl SessionId {
    /// Create a new session ID from client_id, session sequence, and random component
    pub fn new(client_id: u64, session_seq: u32, random: u32) -> Self {
        let mut id = [0u8; 16];
        let client_bytes = client_id.to_be_bytes();
        let seq_bytes = session_seq.to_be_bytes();
        let rand_bytes = random.to_be_bytes();

        id[0..8].copy_from_slice(&client_bytes);
        id[8..12].copy_from_slice(&seq_bytes);
        id[12..16].copy_from_slice(&rand_bytes);

        SessionId(id)
    }

    /// Format as hex string "xxxxxxxx-xxxxxxxx-xxxxxxxx-xxxxxxxx"
    pub fn to_hex(&self) -> String {
        format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}",
            self.0[0], self.0[1], self.0[2], self.0[3],
            self.0[4], self.0[5], self.0[6], self.0[7],
            self.0[8], self.0[9], self.0[10], self.0[11],
            self.0[12], self.0[13], self.0[14], self.0[15]
        )
    }
}

/// NFSv4.1 session state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    Active,
    Draining,
    Destroyed,
}

impl std::fmt::Display for SessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionState::Active => write!(f, "Active"),
            SessionState::Draining => write!(f, "Draining"),
            SessionState::Destroyed => write!(f, "Destroyed"),
        }
    }
}

/// A slot in the NFSv4.1 session slot table
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slot {
    pub slot_id: u32,
    pub sequence_id: u32,
    pub in_use: bool,
    pub cached_reply: Option<Vec<u8>>,
}

impl Slot {
    pub fn new(slot_id: u32) -> Self {
        Slot {
            slot_id,
            sequence_id: 0,
            in_use: false,
            cached_reply: None,
        }
    }

    pub fn is_available(&self) -> bool {
        !self.in_use
    }

    /// Validate incoming sequence_id — must be last+1 or equal (replay)
    pub fn validate_sequence(&self, incoming_seq: u32) -> SlotResult {
        if incoming_seq == self.sequence_id {
            SlotResult::Replay
        } else if incoming_seq == self.sequence_id.wrapping_add(1) {
            SlotResult::NewRequest
        } else {
            SlotResult::InvalidSequence {
                expected: self.sequence_id.wrapping_add(1),
                got: incoming_seq,
            }
        }
    }

    /// Mark slot as in-use for a new request
    pub fn acquire(&mut self, sequence_id: u32) {
        self.in_use = true;
        self.sequence_id = sequence_id;
        self.cached_reply = None;
    }

    /// Release slot, optionally caching the reply
    pub fn release(&mut self, reply: Option<Vec<u8>>) {
        self.in_use = false;
        self.cached_reply = reply;
    }
}

/// Result of slot sequence validation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlotResult {
    NewRequest,
    Replay,
    InvalidSequence { expected: u32, got: u32 },
}

/// NFSv4.1 session — manages a slot table and session state
#[derive(Debug, Serialize, Deserialize)]
pub struct NfsSession {
    pub session_id: SessionId,
    pub client_id: ClientId,
    pub state: SessionState,
    pub fore_channel_slots: Vec<Slot>,
    pub back_channel_slots: Vec<Slot>,
    pub max_requests: u32,
    pub max_response_size: u32,
    pub created_at_secs: u64,
    pub last_used_secs: u64,
}

impl NfsSession {
    pub fn new(
        session_id: SessionId,
        client_id: ClientId,
        fore_channel_slots: u32,
        back_channel_slots: u32,
        max_response_size: u32,
        created_at_secs: u64,
    ) -> Self {
        let mut slots = Vec::with_capacity(fore_channel_slots as usize);
        for i in 0..fore_channel_slots {
            slots.push(Slot::new(i));
        }

        let mut back_slots = Vec::with_capacity(back_channel_slots as usize);
        for i in 0..back_channel_slots {
            back_slots.push(Slot::new(i));
        }

        NfsSession {
            session_id,
            client_id,
            state: SessionState::Active,
            fore_channel_slots: slots,
            back_channel_slots: back_slots,
            max_requests: fore_channel_slots,
            max_response_size,
            created_at_secs,
            last_used_secs: created_at_secs,
        }
    }

    pub fn is_active(&self) -> bool {
        self.state == SessionState::Active
    }

    pub fn is_destroyed(&self) -> bool {
        self.state == SessionState::Destroyed
    }

    pub fn fore_slot(&self, slot_id: u32) -> Option<&Slot> {
        if (slot_id as usize) < self.fore_channel_slots.len() {
            self.fore_channel_slots.get(slot_id as usize)
        } else {
            None
        }
    }

    pub fn fore_slot_mut(&mut self, slot_id: u32) -> Option<&mut Slot> {
        if (slot_id as usize) < self.fore_channel_slots.len() {
            self.fore_channel_slots.get_mut(slot_id as usize)
        } else {
            None
        }
    }

    pub fn idle_secs(&self, now_secs: u64) -> u64 {
        now_secs.saturating_sub(self.last_used_secs)
    }

    pub fn update_last_used(&mut self, now_secs: u64) {
        self.last_used_secs = now_secs;
    }

    pub fn start_drain(&mut self) {
        if self.state == SessionState::Active {
            self.state = SessionState::Draining;
            debug!("Session {:?} started draining", self.session_id.to_hex());
        }
    }

    pub fn destroy(&mut self) {
        self.state = SessionState::Destroyed;
        self.fore_channel_slots.clear();
        self.back_channel_slots.clear();
        info!("Session {:?} destroyed", self.session_id.to_hex());
    }
}

/// NFSv4 client record
#[derive(Debug, Serialize, Deserialize)]
pub struct NfsClient {
    pub client_id: ClientId,
    pub verifier: [u8; 8],
    pub owner_id: Vec<u8>,
    pub confirmed: bool,
    pub sequence_id: u32,
    pub sessions: Vec<SessionId>,
    pub lease_expiry_secs: u64,
}

impl NfsClient {
    pub fn new(
        client_id: ClientId,
        verifier: [u8; 8],
        owner_id: Vec<u8>,
        confirmed: bool,
        lease_expiry_secs: u64,
    ) -> Self {
        NfsClient {
            client_id,
            verifier,
            owner_id,
            confirmed,
            sequence_id: 0,
            sessions: Vec::new(),
            lease_expiry_secs,
        }
    }

    pub fn is_lease_expired(&self, now_secs: u64) -> bool {
        now_secs >= self.lease_expiry_secs
    }

    pub fn renew_lease(&mut self, now_secs: u64, lease_duration_secs: u64) {
        self.lease_expiry_secs = now_secs + lease_duration_secs;
    }

    pub fn confirm(&mut self) {
        self.confirmed = true;
    }
}

/// Session manager — tracks all clients and sessions
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionManager {
    clients: HashMap<u64, NfsClient>,
    sessions: HashMap<[u8; 16], NfsSession>,
    next_client_id: u64,
    lease_duration_secs: u64,
}

impl SessionManager {
    pub fn new(lease_duration_secs: u64) -> Self {
        SessionManager {
            clients: HashMap::new(),
            sessions: HashMap::new(),
            next_client_id: 1,
            lease_duration_secs,
        }
    }

    /// Register a new client, returns the assigned ClientId
    pub fn create_client(
        &mut self,
        verifier: [u8; 8],
        owner_id: Vec<u8>,
        now_secs: u64,
    ) -> ClientId {
        let client_id = ClientId(self.next_client_id);
        self.next_client_id += 1;

        let lease_expiry = now_secs + self.lease_duration_secs;
        let client = NfsClient::new(client_id, verifier, owner_id, false, lease_expiry);

        debug!(
            "Created client {:?} with lease expiry {}",
            client_id, lease_expiry
        );
        self.clients.insert(client_id.0, client);

        client_id
    }

    pub fn get_client(&self, client_id: u64) -> Option<&NfsClient> {
        self.clients.get(&client_id)
    }

    pub fn confirm_client(&mut self, client_id: u64) -> Result<(), SessionError> {
        match self.clients.get_mut(&client_id) {
            Some(client) => {
                client.confirm();
                info!("Client {:?} confirmed", client_id);
                Ok(())
            }
            None => Err(SessionError::ClientNotFound(client_id)),
        }
    }

    /// Create a new session for a confirmed client
    pub fn create_session(
        &mut self,
        client_id: u64,
        session_seq: u32,
        fore_slots: u32,
        back_slots: u32,
        max_response_size: u32,
        now_secs: u64,
    ) -> Result<SessionId, SessionError> {
        let client = self
            .clients
            .get_mut(&client_id)
            .ok_or(SessionError::ClientNotFound(client_id))?;

        if !client.confirmed {
            return Err(SessionError::ClientNotConfirmed(client_id));
        }

        if client.sessions.len() >= 64 {
            return Err(SessionError::TooManySessions(client_id));
        }

        let random = rand_simple(now_secs);
        let session_id = SessionId::new(client_id, session_seq, random);

        let session = NfsSession::new(
            session_id,
            ClientId(client_id),
            fore_slots,
            back_slots,
            max_response_size,
            now_secs,
        );

        client.sessions.push(session_id);
        self.sessions.insert(session_id.0, session);

        info!(
            "Created session {:?} for client {:?}",
            session_id.to_hex(),
            client_id
        );

        Ok(session_id)
    }

    pub fn get_session(&self, session_id: &[u8; 16]) -> Option<&NfsSession> {
        self.sessions.get(session_id)
    }

    pub fn get_session_mut(&mut self, session_id: &[u8; 16]) -> Option<&mut NfsSession> {
        self.sessions.get_mut(session_id)
    }

    pub fn destroy_session(&mut self, session_id: &[u8; 16]) -> Result<(), SessionError> {
        let session = self
            .sessions
            .get_mut(session_id)
            .ok_or(SessionError::SessionNotFound)?;

        if !session.is_active() {
            return Err(SessionError::SessionNotActive(format!(
                "{:?}",
                session.state
            )));
        }

        let client_id = session.client_id.0;
        session.destroy();

        if let Some(client) = self.clients.get_mut(&client_id) {
            client.sessions.retain(|s| s.0 != *session_id);
        }

        self.sessions.remove(session_id);

        Ok(())
    }

    pub fn active_session_count(&self) -> usize {
        self.sessions.values().filter(|s| s.is_active()).count()
    }

    pub fn expire_stale_clients(&mut self, now_secs: u64) -> Vec<ClientId> {
        let mut expired = Vec::new();
        let mut session_ids_to_remove = Vec::new();

        let client_ids: Vec<u64> = self.clients.keys().copied().collect();

        for cid in client_ids {
            if let Some(client) = self.clients.get(&cid) {
                if client.is_lease_expired(now_secs) {
                    expired.push(client.client_id);

                    for sid in &client.sessions {
                        if let Some(session) = self.sessions.get_mut(&sid.0) {
                            session.destroy();
                        }
                        session_ids_to_remove.push(sid.0);
                    }
                }
            }
        }

        for sid in &session_ids_to_remove {
            self.sessions.remove(sid);
        }

        for cid in &expired {
            self.clients.remove(&cid.0);
        }

        if !expired.is_empty() {
            warn!("Expired {} stale clients", expired.len());
        }

        expired
    }
}

fn rand_simple(seed: u64) -> u32 {
    let mut x = seed.wrapping_mul(1103515245).wrapping_add(12345);
    x = x.wrapping_mul(1103515245).wrapping_add(12345);
    (x >> 16) as u32
}

/// Session errors
#[derive(Debug, Error)]
pub enum SessionError {
    #[error("client {0} not found")]
    ClientNotFound(u64),
    #[error("client {0} not confirmed")]
    ClientNotConfirmed(u64),
    #[error("session not found")]
    SessionNotFound,
    #[error("session is not active (state: {0})")]
    SessionNotActive(String),
    #[error("too many sessions for client {0}")]
    TooManySessions(u64),
    #[error("slot {0} out of range (max {1})")]
    SlotOutOfRange(u32, u32),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_id_new() {
        let sid = SessionId::new(12345, 10, 99999);
        let expected = {
            let mut arr = [0u8; 16];
            arr[0..8].copy_from_slice(&12345u64.to_be_bytes());
            arr[8..12].copy_from_slice(&10u32.to_be_bytes());
            arr[12..16].copy_from_slice(&99999u32.to_be_bytes());
            SessionId(arr)
        };
        assert_eq!(sid.0, expected.0);
    }

    #[test]
    fn test_session_id_to_hex() {
        let sid = SessionId([
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
            0x0f, 0x10,
        ]);
        let hex = sid.to_hex();
        assert_eq!(hex.len(), 35);
        assert_eq!(hex, "01020304-05060708-090a0b0c-0d0e0f10");
    }

    #[test]
    fn test_slot_new() {
        let slot = Slot::new(5);
        assert_eq!(slot.slot_id, 5);
        assert_eq!(slot.sequence_id, 0);
        assert!(!slot.in_use);
        assert!(slot.is_available());
    }

    #[test]
    fn test_slot_validate_sequence_new_request() {
        let slot = Slot::new(0);
        let result = slot.validate_sequence(1);
        assert!(matches!(result, SlotResult::NewRequest));
    }

    #[test]
    fn test_slot_validate_sequence_replay() {
        let slot = Slot::new(0);
        let result = slot.validate_sequence(0);
        assert!(matches!(result, SlotResult::Replay));
    }

    #[test]
    fn test_slot_validate_sequence_invalid() {
        let slot = Slot::new(0);
        let result = slot.validate_sequence(5);
        match result {
            SlotResult::InvalidSequence { expected, got } => {
                assert_eq!(expected, 1);
                assert_eq!(got, 5);
            }
            _ => panic!("expected InvalidSequence"),
        }
    }

    #[test]
    fn test_slot_acquire_release() {
        let mut slot = Slot::new(0);
        slot.acquire(10);
        assert!(slot.in_use);
        assert_eq!(slot.sequence_id, 10);

        slot.release(None);
        assert!(!slot.in_use);
    }

    #[test]
    fn test_slot_release_caches_reply() {
        let mut slot = Slot::new(0);
        slot.acquire(1);
        slot.release(Some(vec![1, 2, 3, 4]));

        assert!(!slot.in_use);
        assert_eq!(slot.cached_reply, Some(vec![1, 2, 3, 4]));
    }

    #[test]
    fn test_nfs_session_new() {
        let sid = SessionId::new(1, 1, 100);
        let session = NfsSession::new(sid, ClientId(1), 4, 2, 1024, 1000);

        assert_eq!(session.session_id, sid);
        assert_eq!(session.client_id, ClientId(1));
        assert!(session.is_active());
        assert_eq!(session.fore_channel_slots.len(), 4);
        assert_eq!(session.back_channel_slots.len(), 2);
        assert_eq!(session.max_requests, 4);
    }

    #[test]
    fn test_nfs_session_is_active() {
        let sid = SessionId::new(1, 1, 100);
        let session = NfsSession::new(sid, ClientId(1), 4, 2, 1024, 1000);

        assert!(session.is_active());
        assert!(!session.is_destroyed());
    }

    #[test]
    fn test_nfs_session_fore_slot() {
        let sid = SessionId::new(1, 1, 100);
        let mut session = NfsSession::new(sid, ClientId(1), 4, 2, 1024, 1000);

        let slot = session.fore_slot(2);
        assert!(slot.is_some());
        assert_eq!(slot.unwrap().slot_id, 2);

        assert!(session.fore_slot(10).is_none());

        let slot_mut = session.fore_slot_mut(1);
        assert!(slot_mut.is_some());
        slot_mut.unwrap().acquire(5);

        let slot = session.fore_slot(1);
        assert!(slot.is_some());
        assert!(slot.unwrap().in_use);
    }

    #[test]
    fn test_nfs_session_idle_secs() {
        let sid = SessionId::new(1, 1, 100);
        let session = NfsSession::new(sid, ClientId(1), 4, 2, 1024, 1000);

        assert_eq!(session.idle_secs(1100), 100);
        assert_eq!(session.idle_secs(900), 0);
        assert_eq!(session.idle_secs(1000), 0);
    }

    #[test]
    fn test_nfs_session_update_last_used() {
        let sid = SessionId::new(1, 1, 100);
        let mut session = NfsSession::new(sid, ClientId(1), 4, 2, 1024, 1000);

        assert_eq!(session.last_used_secs, 1000);
        session.update_last_used(1500);
        assert_eq!(session.last_used_secs, 1500);
    }

    #[test]
    fn test_nfs_session_drain() {
        let sid = SessionId::new(1, 1, 100);
        let mut session = NfsSession::new(sid, ClientId(1), 4, 2, 1024, 1000);

        assert!(session.is_active());
        session.start_drain();
        assert_eq!(session.state, SessionState::Draining);
    }

    #[test]
    fn test_nfs_session_destroy() {
        let sid = SessionId::new(1, 1, 100);
        let mut session = NfsSession::new(sid, ClientId(1), 4, 2, 1024, 1000);

        assert!(session.is_active());
        session.destroy();

        assert!(session.is_destroyed());
        assert!(session.fore_channel_slots.is_empty());
    }

    #[test]
    fn test_nfs_client_lease_expiry() {
        let client = NfsClient::new(ClientId(1), [0u8; 8], b"test".to_vec(), false, 1000);

        assert!(!client.is_lease_expired(500));
        assert!(!client.is_lease_expired(999));
        assert!(client.is_lease_expired(1000));
        assert!(client.is_lease_expired(2000));
    }

    #[test]
    fn test_nfs_client_renew_lease() {
        let mut client = NfsClient::new(ClientId(1), [0u8; 8], b"test".to_vec(), false, 1000);

        client.renew_lease(500, 300);
        assert_eq!(client.lease_expiry_secs, 800);

        client.renew_lease(1000, 500);
        assert_eq!(client.lease_expiry_secs, 1500);
    }

    #[test]
    fn test_nfs_client_confirm() {
        let mut client = NfsClient::new(ClientId(1), [0u8; 8], b"test".to_vec(), false, 1000);

        assert!(!client.confirmed);
        client.confirm();
        assert!(client.confirmed);
    }

    #[test]
    fn test_session_manager_create_client() {
        let mut mgr = SessionManager::new(60);

        let id1 = mgr.create_client([1u8; 8], b"client1".to_vec(), 100);
        assert_eq!(id1.0, 1);

        let id2 = mgr.create_client([2u8; 8], b"client2".to_vec(), 100);
        assert_eq!(id2.0, 2);

        let client = mgr.get_client(1).unwrap();
        assert!(!client.confirmed);
    }

    #[test]
    fn test_session_manager_confirm_client() {
        let mut mgr = SessionManager::new(60);

        let id = mgr.create_client([1u8; 8], b"client1".to_vec(), 100);
        mgr.confirm_client(id.0).unwrap();

        let client = mgr.get_client(id.0).unwrap();
        assert!(client.confirmed);
    }

    #[test]
    fn test_session_manager_create_session() {
        let mut mgr = SessionManager::new(60);

        let client_id = mgr.create_client([1u8; 8], b"client1".to_vec(), 100);
        mgr.confirm_client(client_id.0).unwrap();

        let session_id = mgr.create_session(client_id.0, 1, 4, 2, 4096, 200).unwrap();

        let session = mgr.get_session(&session_id.0).unwrap();
        assert!(session.is_active());
        assert_eq!(session.max_requests, 4);
    }

    #[test]
    fn test_session_manager_unconfirmed_client_error() {
        let mut mgr = SessionManager::new(60);

        let client_id = mgr.create_client([1u8; 8], b"client1".to_vec(), 100);

        let result = mgr.create_session(client_id.0, 1, 4, 2, 4096, 200);

        assert!(matches!(result, Err(SessionError::ClientNotConfirmed(_))));
    }

    #[test]
    fn test_session_manager_destroy_session() {
        let mut mgr = SessionManager::new(60);

        let client_id = mgr.create_client([1u8; 8], b"client1".to_vec(), 100);
        mgr.confirm_client(client_id.0).unwrap();

        let session_id = mgr.create_session(client_id.0, 1, 4, 2, 4096, 200).unwrap();

        mgr.destroy_session(&session_id.0).unwrap();

        assert!(mgr.get_session(&session_id.0).is_none());
    }

    #[test]
    fn test_session_manager_expire_stale_clients() {
        let mut mgr = SessionManager::new(60);

        let client1 = mgr.create_client([1u8; 8], b"client1".to_vec(), 100);
        mgr.confirm_client(client1.0).unwrap();

        let client2 = mgr.create_client([2u8; 8], b"client2".to_vec(), 100);
        mgr.confirm_client(client2.0).unwrap();

        let _ = mgr.create_session(client1.0, 1, 4, 2, 4096, 100);

        let expired = mgr.expire_stale_clients(200);

        assert_eq!(expired.len(), 2);
        assert!(expired.contains(&client1));
        assert!(expired.contains(&client2));

        assert!(mgr.get_client(client1.0).is_none());
    }

    #[test]
    fn test_session_manager_client_not_found() {
        let mut mgr = SessionManager::new(60);

        let result = mgr.confirm_client(999);
        assert!(matches!(result, Err(SessionError::ClientNotFound(999))));
    }

    #[test]
    fn test_session_manager_session_not_found() {
        let mut mgr = SessionManager::new(60);

        let result = mgr.destroy_session(&[0u8; 16]);
        assert!(matches!(result, Err(SessionError::SessionNotFound)));
    }

    #[test]
    fn test_slot_validate_sequence_wrapping() {
        let mut slot = Slot::new(0);
        slot.sequence_id = u32::MAX;

        let result = slot.validate_sequence(0);
        assert!(matches!(result, SlotResult::NewRequest));
    }

    #[test]
    fn test_nfs_session_state_display() {
        assert_eq!(format!("{}", SessionState::Active), "Active");
        assert_eq!(format!("{}", SessionState::Draining), "Draining");
        assert_eq!(format!("{}", SessionState::Destroyed), "Destroyed");
    }

    #[test]
    fn test_session_error_display() {
        assert_eq!(
            format!("{}", SessionError::ClientNotFound(5)),
            "client 5 not found"
        );
        assert_eq!(
            format!("{}", SessionError::ClientNotConfirmed(5)),
            "client 5 not confirmed"
        );
        assert_eq!(
            format!("{}", SessionError::SessionNotFound),
            "session not found"
        );
        assert_eq!(
            format!("{}", SessionError::SessionNotActive("Active".to_string())),
            "session is not active (state: Active)"
        );
    }

    #[test]
    fn test_session_manager_active_session_count() {
        let mut mgr = SessionManager::new(60);

        let client_id = mgr.create_client([1u8; 8], b"client1".to_vec(), 100);
        mgr.confirm_client(client_id.0).unwrap();

        assert_eq!(mgr.active_session_count(), 0);

        let _ = mgr.create_session(client_id.0, 1, 4, 2, 4096, 200);

        assert_eq!(mgr.active_session_count(), 1);
    }

    #[test]
    fn test_slot_validate_after_acquire() {
        let mut slot = Slot::new(0);
        slot.acquire(1);

        let result = slot.validate_sequence(2);
        assert!(matches!(result, SlotResult::NewRequest));

        let result = slot.validate_sequence(1);
        assert!(matches!(result, SlotResult::Replay));

        let result = slot.validate_sequence(0);
        match result {
            SlotResult::InvalidSequence { expected, got } => {
                assert_eq!(expected, 2);
                assert_eq!(got, 0);
            }
            _ => panic!("expected InvalidSequence"),
        }
    }
}
