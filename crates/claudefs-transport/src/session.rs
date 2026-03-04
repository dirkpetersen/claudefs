//! Session management for authenticated connections.
//!
//! This module provides session lifecycle management including authentication,
//! reconnection handling, and expiration tracking.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use thiserror::Error;

/// Configuration for session management.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Interval between heartbeat messages in milliseconds.
    pub heartbeat_interval_ms: u64,
    /// Time after which a session is considered expired in milliseconds.
    pub session_timeout_ms: u64,
    /// Maximum number of reconnection attempts before giving up.
    pub max_reconnect_attempts: u32,
    /// Base backoff time for reconnection attempts in milliseconds.
    pub reconnect_backoff_ms: u64,
    /// Time-to-live for authentication tokens in milliseconds.
    pub auth_token_ttl_ms: u64,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval_ms: 5000,
            session_timeout_ms: 30000,
            max_reconnect_attempts: 5,
            reconnect_backoff_ms: 1000,
            auth_token_ttl_ms: 3600000,
        }
    }
}

/// Unique identifier for a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub u64);

impl SessionId {
    /// Creates a new session ID from a raw u64 value.
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// Returns the inner u64 value.
    pub fn inner(&self) -> u64 {
        self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SessionId({})", self.0)
    }
}

/// State of a session in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    /// Session is being established.
    Connecting,
    /// Session has been authenticated.
    Authenticated,
    /// Session is active and operational.
    Active,
    /// Session is attempting to reconnect.
    Reconnecting,
    /// Session has been closed normally.
    Closed,
    /// Session has expired due to timeout.
    Expired,
}

/// Authentication token for a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionToken {
    /// The token string.
    pub token: String,
    /// Absolute expiration time in milliseconds since epoch.
    pub expires_at_ms: u64,
}

impl SessionToken {
    /// Creates a new session token.
    ///
    /// # Arguments
    /// * `token` - The token string.
    /// * `ttl_ms` - Time-to-live in milliseconds.
    /// * `now_ms` - Current time in milliseconds since epoch.
    pub fn new(token: String, ttl_ms: u64, now_ms: u64) -> Self {
        Self {
            token,
            expires_at_ms: now_ms.saturating_add(ttl_ms),
        }
    }

    /// Checks if the token has expired.
    pub fn is_expired(&self, now_ms: u64) -> bool {
        now_ms >= self.expires_at_ms
    }

    /// Returns the remaining time until expiration in milliseconds.
    pub fn remaining_ms(&self, now_ms: u64) -> u64 {
        self.expires_at_ms.saturating_sub(now_ms)
    }
}

/// Errors that can occur during session operations.
#[derive(Debug, Error)]
pub enum SessionError {
    /// The session has expired.
    #[error("Session {session_id} has expired")]
    SessionExpired {
        /// The expired session's ID.
        session_id: SessionId,
    },
    /// Authentication failed.
    #[error("Authentication failed: {reason}")]
    AuthFailed {
        /// Reason for the failure.
        reason: String,
    },
    /// Maximum reconnection attempts exceeded.
    #[error("Maximum reconnection attempts ({max}) exceeded")]
    MaxReconnectsExceeded {
        /// The maximum number of attempts allowed.
        max: u32,
    },
    /// Session not found.
    #[error("Session {session_id} not found")]
    SessionNotFound {
        /// The session ID that was not found.
        session_id: SessionId,
    },
    /// Token has expired.
    #[error("Token has expired")]
    TokenExpired,
}

/// Statistics about session management.
#[derive(Debug, Clone, Default)]
pub struct SessionStats {
    /// Number of currently active sessions.
    pub active_sessions: usize,
    /// Total number of sessions created.
    pub total_sessions: u64,
    /// Total number of reconnection attempts.
    pub reconnect_count: u64,
    /// Total number of successful authentications.
    pub auth_count: u64,
    /// Total number of expired sessions.
    pub expired_count: u64,
    /// Total number of requests sent.
    pub total_requests_sent: u64,
    /// Total number of requests received.
    pub total_requests_received: u64,
}

/// Snapshot of session statistics.
#[derive(Debug, Clone)]
pub struct SessionStatsSnapshot {
    /// Number of currently active sessions.
    pub active_sessions: usize,
    /// Total number of sessions created.
    pub total_sessions: u64,
    /// Total number of reconnection attempts.
    pub reconnect_count: u64,
    /// Total number of successful authentications.
    pub auth_count: u64,
    /// Total number of expired sessions.
    pub expired_count: u64,
    /// Total number of requests sent.
    pub total_requests_sent: u64,
    /// Total number of requests received.
    pub total_requests_received: u64,
}

impl From<&SessionStats> for SessionStatsSnapshot {
    fn from(stats: &SessionStats) -> Self {
        Self {
            active_sessions: stats.active_sessions,
            total_sessions: stats.total_sessions,
            reconnect_count: stats.reconnect_count,
            auth_count: stats.auth_count,
            expired_count: stats.expired_count,
            total_requests_sent: stats.total_requests_sent,
            total_requests_received: stats.total_requests_received,
        }
    }
}

/// Represents an individual session.
#[derive(Debug)]
pub struct Session {
    /// Unique identifier for this session.
    session_id: SessionId,
    /// Configuration for the session.
    config: SessionConfig,
    /// Current state of the session.
    state: SessionState,
    /// Authentication token, if authenticated.
    token: Option<SessionToken>,
    /// Timestamp of last activity in milliseconds.
    last_activity_ms: u64,
    /// Timestamp when the session was created.
    created_at_ms: u64,
    /// Number of reconnection attempts made.
    reconnect_attempts: u32,
    /// Total requests sent through this session.
    requests_sent: u64,
    /// Total requests received through this session.
    requests_received: u64,
}

impl Session {
    /// Creates a new session in the Connecting state.
    ///
    /// # Arguments
    /// * `session_id` - Unique identifier for the session.
    /// * `config` - Configuration for the session.
    /// * `now_ms` - Current time in milliseconds since epoch.
    pub fn new(session_id: SessionId, config: SessionConfig, now_ms: u64) -> Self {
        Self {
            session_id,
            config,
            state: SessionState::Connecting,
            token: None,
            last_activity_ms: now_ms,
            created_at_ms: now_ms,
            reconnect_attempts: 0,
            requests_sent: 0,
            requests_received: 0,
        }
    }

    /// Authenticates the session with a token.
    ///
    /// Returns an error if the token has expired.
    pub fn authenticate(&mut self, token: SessionToken, now_ms: u64) -> Result<(), SessionError> {
        if token.is_expired(now_ms) {
            return Err(SessionError::TokenExpired);
        }
        self.token = Some(token);
        self.state = SessionState::Authenticated;
        self.last_activity_ms = now_ms;
        Ok(())
    }

    /// Records activity on this session, updating the last activity timestamp.
    pub fn record_activity(&mut self, now_ms: u64) {
        self.last_activity_ms = now_ms;
        if self.state == SessionState::Authenticated {
            self.state = SessionState::Active;
        }
    }

    /// Records that a request was sent through this session.
    pub fn record_request_sent(&mut self) {
        self.requests_sent += 1;
    }

    /// Records that a request was received through this session.
    pub fn record_request_received(&mut self) {
        self.requests_received += 1;
    }

    /// Checks if the session has expired based on inactivity timeout.
    pub fn is_expired(&self, now_ms: u64) -> bool {
        let inactive_for = now_ms.saturating_sub(self.last_activity_ms);
        inactive_for >= self.config.session_timeout_ms
    }

    /// Returns the current state of the session.
    pub fn state(&self) -> SessionState {
        self.state
    }

    /// Returns the session ID.
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the timestamp when the session was created.
    pub fn created_at_ms(&self) -> u64 {
        self.created_at_ms
    }

    /// Requests a reconnection attempt.
    ///
    /// Returns an error if the maximum number of reconnection attempts has been exceeded.
    pub fn request_reconnect(&mut self) -> Result<(), SessionError> {
        if self.reconnect_attempts >= self.config.max_reconnect_attempts {
            return Err(SessionError::MaxReconnectsExceeded {
                max: self.config.max_reconnect_attempts,
            });
        }
        self.reconnect_attempts += 1;
        self.state = SessionState::Reconnecting;
        Ok(())
    }

    /// Marks the session as successfully reconnected.
    pub fn mark_reconnected(&mut self, now_ms: u64) {
        self.state = SessionState::Active;
        self.last_activity_ms = now_ms;
    }

    /// Closes the session normally.
    pub fn close(&mut self) {
        self.state = SessionState::Closed;
    }

    /// Expires the session.
    pub fn expire(&mut self) {
        self.state = SessionState::Expired;
    }

    /// Returns the number of reconnection attempts made.
    pub fn reconnect_count(&self) -> u32 {
        self.reconnect_attempts
    }
}

/// Manages a collection of sessions.
#[derive(Debug)]
pub struct SessionManager {
    /// Configuration for new sessions.
    config: SessionConfig,
    /// Map of session ID to session.
    sessions: HashMap<SessionId, Session>,
    /// Counter for generating new session IDs.
    next_session_id: u64,
    /// Aggregated statistics.
    stats: SessionStats,
}

impl SessionManager {
    /// Creates a new session manager with the given configuration.
    pub fn new(config: SessionConfig) -> Self {
        Self {
            config,
            sessions: HashMap::new(),
            next_session_id: 1,
            stats: SessionStats::default(),
        }
    }

    /// Creates a new session and returns it.
    ///
    /// The session is NOT automatically registered with the manager.
    pub fn create_session(&mut self, now_ms: u64) -> Session {
        let session_id = SessionId::new(self.next_session_id);
        self.next_session_id += 1;
        self.stats.total_sessions += 1;
        Session::new(session_id, self.config.clone(), now_ms)
    }

    /// Registers a session with the manager.
    pub fn register(&mut self, session: Session) {
        self.stats.active_sessions += 1;
        self.sessions.insert(session.session_id(), session);
    }

    /// Returns a reference to a session by ID.
    pub fn get_session(&self, session_id: SessionId) -> Option<&Session> {
        self.sessions.get(&session_id)
    }

    /// Returns a mutable reference to a session by ID.
    pub fn get_session_mut(&mut self, session_id: SessionId) -> Option<&mut Session> {
        self.sessions.get_mut(&session_id)
    }

    /// Removes a session by ID and returns it.
    pub fn remove_session(&mut self, session_id: SessionId) -> Option<Session> {
        if let Some(session) = self.sessions.remove(&session_id) {
            self.stats.active_sessions = self.stats.active_sessions.saturating_sub(1);
            Some(session)
        } else {
            None
        }
    }

    /// Evicts all expired sessions and returns the count of evicted sessions.
    pub fn evict_expired(&mut self, now_ms: u64) -> usize {
        let expired_ids: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(_, session)| session.is_expired(now_ms))
            .map(|(id, _)| *id)
            .collect();

        let count = expired_ids.len();
        for id in expired_ids {
            if let Some(mut session) = self.sessions.remove(&id) {
                session.expire();
                self.stats.expired_count += 1;
                self.stats.active_sessions = self.stats.active_sessions.saturating_sub(1);
            }
        }
        count
    }

    /// Returns the number of active sessions.
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// Returns current session statistics.
    pub fn stats(&self) -> SessionStats {
        let mut stats = self.stats.clone();
        stats.active_sessions = self.sessions.len();
        stats.reconnect_count = self
            .sessions
            .values()
            .map(|s| s.reconnect_count() as u64)
            .sum();
        stats.total_requests_sent = self.sessions.values().map(|s| s.requests_sent).sum();
        stats.total_requests_received = self.sessions.values().map(|s| s.requests_received).sum();
        stats
    }

    /// Returns a snapshot of current session statistics.
    pub fn stats_snapshot(&self) -> SessionStatsSnapshot {
        SessionStatsSnapshot::from(&self.stats())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = SessionConfig::default();
        assert_eq!(config.heartbeat_interval_ms, 5000);
        assert_eq!(config.session_timeout_ms, 30000);
        assert_eq!(config.max_reconnect_attempts, 5);
        assert_eq!(config.reconnect_backoff_ms, 1000);
        assert_eq!(config.auth_token_ttl_ms, 3600000);
    }

    #[test]
    fn test_session_id_new() {
        let id = SessionId::new(42);
        assert_eq!(id.inner(), 42);
    }

    #[test]
    fn test_session_id_display() {
        let id = SessionId::new(123);
        assert_eq!(format!("{}", id), "SessionId(123)");
    }

    #[test]
    fn test_session_id_equality() {
        let id1 = SessionId::new(100);
        let id2 = SessionId::new(100);
        let id3 = SessionId::new(200);
        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_token_creation() {
        let token = SessionToken::new("secret".to_string(), 1000, 5000);
        assert_eq!(token.token, "secret");
        assert_eq!(token.expires_at_ms, 6000);
    }

    #[test]
    fn test_token_not_expired() {
        let token = SessionToken::new("secret".to_string(), 1000, 5000);
        assert!(!token.is_expired(5000));
        assert!(!token.is_expired(5999));
    }

    #[test]
    fn test_token_expired() {
        let token = SessionToken::new("secret".to_string(), 1000, 5000);
        assert!(token.is_expired(6000));
        assert!(token.is_expired(7000));
    }

    #[test]
    fn test_token_remaining_ms() {
        let token = SessionToken::new("secret".to_string(), 1000, 5000);
        assert_eq!(token.remaining_ms(5000), 1000);
        assert_eq!(token.remaining_ms(5500), 500);
        assert_eq!(token.remaining_ms(6000), 0);
        assert_eq!(token.remaining_ms(7000), 0);
    }

    #[test]
    fn test_session_new_starts_connecting() {
        let config = SessionConfig::default();
        let session = Session::new(SessionId::new(1), config, 0);
        assert_eq!(session.state(), SessionState::Connecting);
        assert_eq!(session.session_id(), SessionId::new(1));
        assert_eq!(session.reconnect_count(), 0);
    }

    #[test]
    fn test_session_authenticate_success() {
        let config = SessionConfig::default();
        let mut session = Session::new(SessionId::new(1), config, 0);
        let token = SessionToken::new("secret".to_string(), 1000, 0);

        let result = session.authenticate(token, 0);
        assert!(result.is_ok());
        assert_eq!(session.state(), SessionState::Authenticated);
    }

    #[test]
    fn test_session_authenticate_expired_token() {
        let config = SessionConfig::default();
        let mut session = Session::new(SessionId::new(1), config, 0);
        let token = SessionToken::new("secret".to_string(), 1000, 0);

        let result = session.authenticate(token, 2000);
        assert!(matches!(result, Err(SessionError::TokenExpired)));
        assert_eq!(session.state(), SessionState::Connecting);
    }

    #[test]
    fn test_session_record_activity() {
        let config = SessionConfig::default();
        let mut session = Session::new(SessionId::new(1), config, 0);
        let token = SessionToken::new("secret".to_string(), 1000, 0);
        session.authenticate(token, 0).unwrap();

        session.record_activity(5000);
        assert_eq!(session.state(), SessionState::Active);
    }

    #[test]
    fn test_session_request_counters() {
        let config = SessionConfig::default();
        let mut session = Session::new(SessionId::new(1), config, 0);

        session.record_request_sent();
        session.record_request_sent();
        session.record_request_received();

        assert_eq!(session.requests_sent, 2);
        assert_eq!(session.requests_received, 1);
    }

    #[test]
    fn test_session_expiry() {
        let mut config = SessionConfig::default();
        config.session_timeout_ms = 1000;

        let session = Session::new(SessionId::new(1), config, 0);
        assert!(!session.is_expired(500));
        assert!(!session.is_expired(999));
        assert!(session.is_expired(1000));
        assert!(session.is_expired(2000));
    }

    #[test]
    fn test_session_reconnect_success() {
        let config = SessionConfig::default();
        let mut session = Session::new(SessionId::new(1), config, 0);

        let result = session.request_reconnect();
        assert!(result.is_ok());
        assert_eq!(session.state(), SessionState::Reconnecting);
        assert_eq!(session.reconnect_count(), 1);
    }

    #[test]
    fn test_session_reconnect_max_exceeded() {
        let mut config = SessionConfig::default();
        config.max_reconnect_attempts = 2;

        let mut session = Session::new(SessionId::new(1), config, 0);

        assert!(session.request_reconnect().is_ok());
        assert!(session.request_reconnect().is_ok());

        let result = session.request_reconnect();
        assert!(matches!(
            result,
            Err(SessionError::MaxReconnectsExceeded { max: 2 })
        ));
    }

    #[test]
    fn test_session_mark_reconnected() {
        let config = SessionConfig::default();
        let mut session = Session::new(SessionId::new(1), config, 0);
        session.request_reconnect().unwrap();

        session.mark_reconnected(1000);
        assert_eq!(session.state(), SessionState::Active);
    }

    #[test]
    fn test_session_close() {
        let config = SessionConfig::default();
        let mut session = Session::new(SessionId::new(1), config, 0);

        session.close();
        assert_eq!(session.state(), SessionState::Closed);
    }

    #[test]
    fn test_session_expire() {
        let config = SessionConfig::default();
        let mut session = Session::new(SessionId::new(1), config, 0);

        session.expire();
        assert_eq!(session.state(), SessionState::Expired);
    }

    #[test]
    fn test_manager_create_session() {
        let config = SessionConfig::default();
        let mut manager = SessionManager::new(config);

        let session = manager.create_session(0);
        assert_eq!(session.session_id(), SessionId::new(1));
        assert_eq!(session.state(), SessionState::Connecting);
    }

    #[test]
    fn test_manager_register_and_get() {
        let config = SessionConfig::default();
        let mut manager = SessionManager::new(config);

        let session = manager.create_session(0);
        let session_id = session.session_id();
        manager.register(session);

        assert!(manager.get_session(session_id).is_some());
        assert!(manager.get_session(SessionId::new(999)).is_none());
    }

    #[test]
    fn test_manager_get_mut() {
        let config = SessionConfig::default();
        let mut manager = SessionManager::new(config);

        let session = manager.create_session(0);
        let session_id = session.session_id();
        manager.register(session);

        let session = manager.get_session_mut(session_id).unwrap();
        session.record_request_sent();

        let session = manager.get_session(session_id).unwrap();
        assert_eq!(session.requests_sent, 1);
    }

    #[test]
    fn test_manager_remove_session() {
        let config = SessionConfig::default();
        let mut manager = SessionManager::new(config);

        let session = manager.create_session(0);
        let session_id = session.session_id();
        manager.register(session);

        let removed = manager.remove_session(session_id);
        assert!(removed.is_some());
        assert!(manager.get_session(session_id).is_none());
        assert_eq!(manager.session_count(), 0);
    }

    #[test]
    fn test_manager_evict_expired() {
        let mut config = SessionConfig::default();
        config.session_timeout_ms = 100;

        let mut manager = SessionManager::new(config);

        let s1 = manager.create_session(0);
        let s2 = manager.create_session(0);
        let id1 = s1.session_id();
        let id2 = s2.session_id();
        manager.register(s1);
        manager.register(s2);

        assert_eq!(manager.evict_expired(200), 2);
        assert!(manager.get_session(id1).is_none());
        assert!(manager.get_session(id2).is_none());
    }

    #[test]
    fn test_manager_evict_partial() {
        let mut config = SessionConfig::default();
        config.session_timeout_ms = 100;

        let mut manager = SessionManager::new(config);

        let mut s1 = manager.create_session(0);
        s1.record_activity(200);
        let id1 = s1.session_id();
        let s2 = manager.create_session(0);
        let id2 = s2.session_id();
        manager.register(s1);
        manager.register(s2);

        assert_eq!(manager.evict_expired(200), 1);
        assert!(manager.get_session(id1).is_some());
        assert!(manager.get_session(id2).is_none());
    }

    #[test]
    fn test_manager_stats() {
        let config = SessionConfig::default();
        let mut manager = SessionManager::new(config);

        let s1 = manager.create_session(0);
        manager.register(s1);
        let s2 = manager.create_session(0);
        manager.register(s2);

        let stats = manager.stats();
        assert_eq!(stats.active_sessions, 2);
        assert_eq!(stats.total_sessions, 2);
    }

    #[test]
    fn test_manager_stats_snapshot() {
        let config = SessionConfig::default();
        let mut manager = SessionManager::new(config);

        let session = manager.create_session(0);
        manager.register(session);

        let snapshot = manager.stats_snapshot();
        assert_eq!(snapshot.active_sessions, 1);
        assert_eq!(snapshot.total_sessions, 1);
    }

    #[test]
    fn test_session_id_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(SessionId::new(1));
        set.insert(SessionId::new(2));
        set.insert(SessionId::new(1));

        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_session_id_serde() {
        let id = SessionId::new(42);
        let json = serde_json::to_string(&id).unwrap();
        let decoded: SessionId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, decoded);
    }

    #[test]
    fn test_session_state_serde() {
        let state = SessionState::Active;
        let json = serde_json::to_string(&state).unwrap();
        let decoded: SessionState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, decoded);
    }

    #[test]
    fn test_session_token_serde() {
        let token = SessionToken::new("mytoken".to_string(), 1000, 5000);
        let json = serde_json::to_string(&token).unwrap();
        let decoded: SessionToken = serde_json::from_str(&json).unwrap();
        assert_eq!(token.token, decoded.token);
        assert_eq!(token.expires_at_ms, decoded.expires_at_ms);
    }

    #[test]
    fn test_error_display() {
        let err = SessionError::SessionExpired {
            session_id: SessionId::new(1),
        };
        assert_eq!(format!("{}", err), "Session SessionId(1) has expired");

        let err = SessionError::AuthFailed {
            reason: "bad credentials".to_string(),
        };
        assert_eq!(format!("{}", err), "Authentication failed: bad credentials");

        let err = SessionError::MaxReconnectsExceeded { max: 5 };
        assert_eq!(
            format!("{}", err),
            "Maximum reconnection attempts (5) exceeded"
        );

        let err = SessionError::SessionNotFound {
            session_id: SessionId::new(42),
        };
        assert_eq!(format!("{}", err), "Session SessionId(42) not found");

        let err = SessionError::TokenExpired;
        assert_eq!(format!("{}", err), "Token has expired");
    }
}
