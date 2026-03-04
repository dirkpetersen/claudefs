//! Unix Domain Socket IPC Transport for same-host communication.
//!
//! Provides a local-node IPC abstraction for same-host communication between
//! ClaudeFS components. In `cfs server` mode, all subsystems (FUSE, metadata,
//! storage, replication) run in the same process. IPC lets them bypass TCP
//! for zero-copy local messaging.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for IPC transport.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcConfig {
    /// Socket path, e.g. "/var/run/cfs/cfs.sock".
    pub socket_path: String,
    /// Maximum number of concurrent connections.
    pub max_connections: usize,
    /// Send buffer size in bytes.
    pub send_buffer_bytes: usize,
    /// Receive buffer size in bytes.
    pub recv_buffer_bytes: usize,
    /// Connection timeout in milliseconds.
    pub connect_timeout_ms: u64,
    /// Maximum message size in bytes.
    pub max_message_size: usize,
}

impl Default for IpcConfig {
    fn default() -> Self {
        Self {
            socket_path: "/var/run/cfs/cfs.sock".to_string(),
            max_connections: 128,
            send_buffer_bytes: 65536,
            recv_buffer_bytes: 65536,
            connect_timeout_ms: 1000,
            max_message_size: 16 * 1024 * 1024,
        }
    }
}

/// State of an IPC connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IpcConnectionState {
    /// Connection is being established.
    Connecting,
    /// Connection is established and active.
    Connected,
    /// Connection has been closed.
    Disconnected,
    /// Connection encountered an error.
    Error(String),
}

/// Represents a single IPC connection endpoint.
#[derive(Debug, Clone)]
pub struct IpcConnection {
    /// Unique connection identifier.
    id: u64,
    /// Socket path for this connection.
    path: String,
    /// Current connection state.
    state: IpcConnectionState,
    /// Total bytes sent on this connection.
    bytes_sent: u64,
    /// Total bytes received on this connection.
    bytes_received: u64,
    /// Total messages sent on this connection.
    messages_sent: u64,
    /// Total messages received on this connection.
    messages_received: u64,
    /// Timestamp when the connection was created (ms since epoch).
    created_at_ms: u64,
    /// Timestamp of last activity (ms since epoch).
    last_active_ms: u64,
}

impl IpcConnection {
    /// Creates a new IPC connection in the Connecting state.
    ///
    /// # Arguments
    /// * `id` - Unique connection identifier.
    /// * `path` - Socket path for this connection.
    /// * `now_ms` - Current time in milliseconds since epoch.
    pub fn new(id: u64, path: String, now_ms: u64) -> Self {
        Self {
            id,
            path,
            state: IpcConnectionState::Connecting,
            bytes_sent: 0,
            bytes_received: 0,
            messages_sent: 0,
            messages_received: 0,
            created_at_ms: now_ms,
            last_active_ms: now_ms,
        }
    }

    /// Returns the connection ID.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Returns the socket path for this connection.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the current connection state.
    pub fn state(&self) -> &IpcConnectionState {
        &self.state
    }

    /// Marks the connection as successfully connected.
    pub fn mark_connected(&mut self, now_ms: u64) {
        self.state = IpcConnectionState::Connected;
        self.last_active_ms = now_ms;
    }

    /// Marks the connection as disconnected.
    pub fn mark_disconnected(&mut self) {
        self.state = IpcConnectionState::Disconnected;
    }

    /// Marks the connection as having encountered an error.
    pub fn mark_error(&mut self, reason: String) {
        self.state = IpcConnectionState::Error(reason);
    }

    /// Records a send operation on this connection.
    pub fn record_send(&mut self, bytes: usize, now_ms: u64) {
        self.bytes_sent += bytes as u64;
        self.messages_sent += 1;
        self.last_active_ms = now_ms;
    }

    /// Records a receive operation on this connection.
    pub fn record_recv(&mut self, bytes: usize, now_ms: u64) {
        self.bytes_received += bytes as u64;
        self.messages_received += 1;
        self.last_active_ms = now_ms;
    }

    /// Returns true if the connection is in the Connected state.
    pub fn is_active(&self) -> bool {
        matches!(self.state, IpcConnectionState::Connected)
    }

    /// Returns the time since last activity in milliseconds.
    pub fn idle_ms(&self, now_ms: u64) -> u64 {
        now_ms.saturating_sub(self.last_active_ms)
    }

    /// Returns total bytes sent.
    pub fn bytes_sent(&self) -> u64 {
        self.bytes_sent
    }

    /// Returns total bytes received.
    pub fn bytes_received(&self) -> u64 {
        self.bytes_received
    }

    /// Returns total messages sent.
    pub fn messages_sent(&self) -> u64 {
        self.messages_sent
    }

    /// Returns total messages received.
    pub fn messages_received(&self) -> u64 {
        self.messages_received
    }

    /// Returns timestamp when connection was created.
    pub fn created_at_ms(&self) -> u64 {
        self.created_at_ms
    }

    /// Returns timestamp of last activity.
    pub fn last_active_ms(&self) -> u64 {
        self.last_active_ms
    }
}

/// Statistics about IPC connections.
#[derive(Debug, Clone, Default)]
pub struct IpcStats {
    /// Number of currently active connections.
    pub active_connections: usize,
    /// Total number of connections established.
    pub total_connects: u64,
    /// Total number of disconnections.
    pub total_disconnects: u64,
    /// Total number of connection errors.
    pub total_errors: u64,
    /// Total bytes sent across all connections.
    pub total_bytes_sent: u64,
    /// Total bytes received across all connections.
    pub total_bytes_received: u64,
    /// Total messages sent across all connections.
    pub total_messages_sent: u64,
    /// Total messages received across all connections.
    pub total_messages_received: u64,
}

/// Immutable snapshot of IPC statistics.
#[derive(Debug, Clone)]
pub struct IpcStatsSnapshot {
    /// Number of currently active connections.
    pub active_connections: usize,
    /// Total number of connections established.
    pub total_connects: u64,
    /// Total number of disconnections.
    pub total_disconnects: u64,
    /// Total number of connection errors.
    pub total_errors: u64,
    /// Total bytes sent across all connections.
    pub total_bytes_sent: u64,
    /// Total bytes received across all connections.
    pub total_bytes_received: u64,
    /// Total messages sent across all connections.
    pub total_messages_sent: u64,
    /// Total messages received across all connections.
    pub total_messages_received: u64,
}

impl From<&IpcStats> for IpcStatsSnapshot {
    fn from(stats: &IpcStats) -> Self {
        Self {
            active_connections: stats.active_connections,
            total_connects: stats.total_connects,
            total_disconnects: stats.total_disconnects,
            total_errors: stats.total_errors,
            total_bytes_sent: stats.total_bytes_sent,
            total_bytes_received: stats.total_bytes_received,
            total_messages_sent: stats.total_messages_sent,
            total_messages_received: stats.total_messages_received,
        }
    }
}

/// Manages a pool of IPC connections.
#[derive(Debug)]
pub struct IpcManager {
    /// Configuration for the manager.
    config: IpcConfig,
    /// Map of connection ID to connection.
    connections: HashMap<u64, IpcConnection>,
    /// Next connection ID to assign.
    next_id: u64,
    /// Total successful connections.
    total_connects: u64,
    /// Total disconnections.
    total_disconnects: u64,
    /// Total connection errors.
    total_errors: u64,
}

impl IpcManager {
    /// Creates a new IPC manager with the given configuration.
    pub fn new(config: IpcConfig) -> Self {
        Self {
            config,
            connections: HashMap::new(),
            next_id: 1,
            total_connects: 0,
            total_disconnects: 0,
            total_errors: 0,
        }
    }

    /// Adds a new connection and returns its ID.
    ///
    /// # Arguments
    /// * `path` - Socket path for the connection.
    /// * `now_ms` - Current time in milliseconds since epoch.
    pub fn add_connection(&mut self, path: String, now_ms: u64) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let conn = IpcConnection::new(id, path, now_ms);
        self.connections.insert(id, conn);
        id
    }

    /// Removes a connection by ID and returns it.
    pub fn remove_connection(&mut self, id: u64) -> Option<IpcConnection> {
        self.connections
            .remove(&id)
            .inspect(|conn| match &conn.state {
                IpcConnectionState::Connected => self.total_disconnects += 1,
                IpcConnectionState::Error(_) => self.total_errors += 1,
                _ => {}
            })
    }

    /// Returns a reference to a connection by ID.
    pub fn get(&self, id: u64) -> Option<&IpcConnection> {
        self.connections.get(&id)
    }

    /// Returns a mutable reference to a connection by ID.
    pub fn get_mut(&mut self, id: u64) -> Option<&mut IpcConnection> {
        self.connections.get_mut(&id)
    }

    /// Returns the count of currently active (connected) connections.
    pub fn active_count(&self) -> usize {
        self.connections.values().filter(|c| c.is_active()).count()
    }

    /// Returns true if capacity is available for new connections.
    pub fn capacity_available(&self) -> bool {
        self.active_count() < self.config.max_connections
    }

    /// Computes and returns current IPC statistics.
    pub fn stats(&self) -> IpcStats {
        let active: Vec<_> = self
            .connections
            .values()
            .filter(|c| c.is_active())
            .collect();

        IpcStats {
            active_connections: active.len(),
            total_connects: self.total_connects,
            total_disconnects: self.total_disconnects,
            total_errors: self.total_errors,
            total_bytes_sent: self.connections.values().map(|c| c.bytes_sent()).sum(),
            total_bytes_received: self.connections.values().map(|c| c.bytes_received()).sum(),
            total_messages_sent: self.connections.values().map(|c| c.messages_sent()).sum(),
            total_messages_received: self
                .connections
                .values()
                .map(|c| c.messages_received())
                .sum(),
        }
    }

    /// Returns a snapshot of current IPC statistics.
    pub fn snapshot(&self) -> IpcStatsSnapshot {
        IpcStatsSnapshot::from(&self.stats())
    }

    /// Returns the configuration.
    pub fn config(&self) -> &IpcConfig {
        &self.config
    }

    /// Returns the total number of connections (active and inactive).
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    /// Marks a connection as connected (internal use when connection succeeds).
    pub fn mark_connected(&mut self, id: u64, now_ms: u64) {
        if let Some(conn) = self.connections.get_mut(&id) {
            conn.mark_connected(now_ms);
            self.total_connects += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = IpcConfig::default();
        assert_eq!(config.socket_path, "/var/run/cfs/cfs.sock");
        assert_eq!(config.max_connections, 128);
        assert_eq!(config.send_buffer_bytes, 65536);
        assert_eq!(config.recv_buffer_bytes, 65536);
        assert_eq!(config.connect_timeout_ms, 1000);
        assert_eq!(config.max_message_size, 16 * 1024 * 1024);
    }

    #[test]
    fn test_config_custom() {
        let config = IpcConfig {
            socket_path: "/tmp/test.sock".to_string(),
            max_connections: 64,
            send_buffer_bytes: 32768,
            recv_buffer_bytes: 32768,
            connect_timeout_ms: 5000,
            max_message_size: 8 * 1024 * 1024,
        };
        assert_eq!(config.socket_path, "/tmp/test.sock");
        assert_eq!(config.max_connections, 64);
    }

    #[test]
    fn test_connection_new() {
        let conn = IpcConnection::new(1, "/tmp/test.sock".to_string(), 1000);
        assert_eq!(conn.id(), 1);
        assert_eq!(conn.path(), "/tmp/test.sock");
        assert_eq!(conn.state(), &IpcConnectionState::Connecting);
        assert!(!conn.is_active());
        assert_eq!(conn.bytes_sent(), 0);
        assert_eq!(conn.bytes_received(), 0);
        assert_eq!(conn.messages_sent(), 0);
        assert_eq!(conn.messages_received(), 0);
        assert_eq!(conn.created_at_ms(), 1000);
        assert_eq!(conn.last_active_ms(), 1000);
    }

    #[test]
    fn test_connection_mark_connected() {
        let mut conn = IpcConnection::new(1, "/tmp/test.sock".to_string(), 1000);
        conn.mark_connected(2000);
        assert_eq!(conn.state(), &IpcConnectionState::Connected);
        assert!(conn.is_active());
        assert_eq!(conn.last_active_ms(), 2000);
    }

    #[test]
    fn test_connection_mark_disconnected() {
        let mut conn = IpcConnection::new(1, "/tmp/test.sock".to_string(), 1000);
        conn.mark_connected(2000);
        conn.mark_disconnected();
        assert_eq!(conn.state(), &IpcConnectionState::Disconnected);
        assert!(!conn.is_active());
    }

    #[test]
    fn test_connection_mark_error() {
        let mut conn = IpcConnection::new(1, "/tmp/test.sock".to_string(), 1000);
        conn.mark_error("connection refused".to_string());
        assert_eq!(
            conn.state(),
            &IpcConnectionState::Error("connection refused".to_string())
        );
        assert!(!conn.is_active());
    }

    #[test]
    fn test_connection_record_send() {
        let mut conn = IpcConnection::new(1, "/tmp/test.sock".to_string(), 1000);
        conn.mark_connected(2000);
        conn.record_send(1024, 3000);
        assert_eq!(conn.bytes_sent(), 1024);
        assert_eq!(conn.messages_sent(), 1);
        assert_eq!(conn.last_active_ms(), 3000);

        conn.record_send(2048, 4000);
        assert_eq!(conn.bytes_sent(), 3072);
        assert_eq!(conn.messages_sent(), 2);
        assert_eq!(conn.last_active_ms(), 4000);
    }

    #[test]
    fn test_connection_record_recv() {
        let mut conn = IpcConnection::new(1, "/tmp/test.sock".to_string(), 1000);
        conn.mark_connected(2000);
        conn.record_recv(512, 3000);
        assert_eq!(conn.bytes_received(), 512);
        assert_eq!(conn.messages_received(), 1);
        assert_eq!(conn.last_active_ms(), 3000);
    }

    #[test]
    fn test_connection_idle_ms() {
        let mut conn = IpcConnection::new(1, "/tmp/test.sock".to_string(), 1000);
        conn.record_send(100, 2000);
        assert_eq!(conn.idle_ms(5000), 3000);
        assert_eq!(conn.idle_ms(2000), 0);
    }

    #[test]
    fn test_manager_new() {
        let config = IpcConfig::default();
        let manager = IpcManager::new(config);
        assert_eq!(manager.connection_count(), 0);
        assert_eq!(manager.active_count(), 0);
        assert!(manager.capacity_available());
    }

    #[test]
    fn test_manager_add_connection() {
        let config = IpcConfig::default();
        let mut manager = IpcManager::new(config);

        let id1 = manager.add_connection("/tmp/test.sock".to_string(), 1000);
        assert_eq!(id1, 1);
        assert_eq!(manager.connection_count(), 1);

        let id2 = manager.add_connection("/tmp/test2.sock".to_string(), 2000);
        assert_eq!(id2, 2);
        assert_eq!(manager.connection_count(), 2);
    }

    #[test]
    fn test_manager_get_connection() {
        let config = IpcConfig::default();
        let mut manager = IpcManager::new(config);

        let id = manager.add_connection("/tmp/test.sock".to_string(), 1000);
        let conn = manager.get(id).unwrap();
        assert_eq!(conn.path(), "/tmp/test.sock");
        assert_eq!(conn.state(), &IpcConnectionState::Connecting);

        assert!(manager.get(999).is_none());
    }

    #[test]
    fn test_manager_get_mut_connection() {
        let config = IpcConfig::default();
        let mut manager = IpcManager::new(config);

        let id = manager.add_connection("/tmp/test.sock".to_string(), 1000);
        let conn = manager.get_mut(id).unwrap();
        conn.mark_connected(2000);

        let conn = manager.get(id).unwrap();
        assert!(conn.is_active());
    }

    #[test]
    fn test_manager_remove_connection() {
        let config = IpcConfig::default();
        let mut manager = IpcManager::new(config);

        let id = manager.add_connection("/tmp/test.sock".to_string(), 1000);
        let removed = manager.remove_connection(id).unwrap();
        assert_eq!(removed.id(), id);
        assert_eq!(manager.connection_count(), 0);
        assert!(manager.get(id).is_none());
    }

    #[test]
    fn test_manager_mark_connected() {
        let config = IpcConfig::default();
        let mut manager = IpcManager::new(config);

        let id = manager.add_connection("/tmp/test.sock".to_string(), 1000);
        manager.mark_connected(id, 2000);

        assert_eq!(manager.active_count(), 1);
        let stats = manager.stats();
        assert_eq!(stats.total_connects, 1);
    }

    #[test]
    fn test_manager_active_count() {
        let config = IpcConfig::default();
        let mut manager = IpcManager::new(config);

        let id1 = manager.add_connection("/tmp/test1.sock".to_string(), 1000);
        let id2 = manager.add_connection("/tmp/test2.sock".to_string(), 1000);
        let id3 = manager.add_connection("/tmp/test3.sock".to_string(), 1000);

        assert_eq!(manager.active_count(), 0);

        manager.mark_connected(id1, 2000);
        assert_eq!(manager.active_count(), 1);

        manager.mark_connected(id2, 2000);
        assert_eq!(manager.active_count(), 2);

        manager.mark_connected(id3, 2000);
        assert_eq!(manager.active_count(), 3);
    }

    #[test]
    fn test_manager_capacity_available() {
        let config = IpcConfig {
            max_connections: 2,
            ..Default::default()
        };
        let mut manager = IpcManager::new(config);

        assert!(manager.capacity_available());

        let id1 = manager.add_connection("/tmp/test1.sock".to_string(), 1000);
        manager.mark_connected(id1, 2000);
        assert!(manager.capacity_available());

        let id2 = manager.add_connection("/tmp/test2.sock".to_string(), 1000);
        manager.mark_connected(id2, 2000);
        assert!(!manager.capacity_available());
    }

    #[test]
    fn test_manager_stats() {
        let config = IpcConfig::default();
        let mut manager = IpcManager::new(config);

        let id1 = manager.add_connection("/tmp/test1.sock".to_string(), 1000);
        manager.mark_connected(id1, 2000);
        manager.get_mut(id1).unwrap().record_send(100, 3000);
        manager.get_mut(id1).unwrap().record_recv(200, 3000);

        let id2 = manager.add_connection("/tmp/test2.sock".to_string(), 1000);
        manager.mark_connected(id2, 2000);
        manager.get_mut(id2).unwrap().record_send(150, 3000);

        let stats = manager.stats();
        assert_eq!(stats.active_connections, 2);
        assert_eq!(stats.total_connects, 2);
        assert_eq!(stats.total_bytes_sent, 250);
        assert_eq!(stats.total_bytes_received, 200);
        assert_eq!(stats.total_messages_sent, 2);
        assert_eq!(stats.total_messages_received, 1);
    }

    #[test]
    fn test_manager_stats_snapshot() {
        let config = IpcConfig::default();
        let mut manager = IpcManager::new(config);

        let id = manager.add_connection("/tmp/test.sock".to_string(), 1000);
        manager.mark_connected(id, 2000);

        let snapshot = manager.snapshot();
        assert_eq!(snapshot.active_connections, 1);
        assert_eq!(snapshot.total_connects, 1);
    }

    #[test]
    fn test_manager_remove_connected_tracks_disconnects() {
        let config = IpcConfig::default();
        let mut manager = IpcManager::new(config);

        let id = manager.add_connection("/tmp/test.sock".to_string(), 1000);
        manager.mark_connected(id, 2000);
        manager.remove_connection(id);

        let stats = manager.stats();
        assert_eq!(stats.total_disconnects, 1);
    }

    #[test]
    fn test_manager_remove_error_connection() {
        let config = IpcConfig::default();
        let mut manager = IpcManager::new(config);

        let id = manager.add_connection("/tmp/test.sock".to_string(), 1000);
        manager
            .get_mut(id)
            .unwrap()
            .mark_error("test error".to_string());
        manager.remove_connection(id);

        let stats = manager.stats();
        assert_eq!(stats.total_errors, 1);
    }

    #[test]
    fn test_config_serde() {
        let config = IpcConfig {
            socket_path: "/tmp/custom.sock".to_string(),
            max_connections: 64,
            send_buffer_bytes: 32768,
            recv_buffer_bytes: 32768,
            connect_timeout_ms: 5000,
            max_message_size: 8388608,
        };
        let json = serde_json::to_string(&config).unwrap();
        let decoded: IpcConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.socket_path, decoded.socket_path);
        assert_eq!(config.max_connections, decoded.max_connections);
    }

    #[test]
    fn test_connection_state_serde() {
        let state = IpcConnectionState::Connected;
        let json = serde_json::to_string(&state).unwrap();
        let decoded: IpcConnectionState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, decoded);

        let state = IpcConnectionState::Error("test".to_string());
        let json = serde_json::to_string(&state).unwrap();
        let decoded: IpcConnectionState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, decoded);
    }
}
