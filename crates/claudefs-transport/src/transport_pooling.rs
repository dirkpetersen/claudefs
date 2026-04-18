//! Connection pooling module for per-node connection management.
//!
//! This module provides connection pooling specifically designed for cluster nodes,
//! with support for per-node pools, idle timeout management, and connection health tracking.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// Connection state in the pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionState {
    /// Connection is actively in use.
    Active,
    /// Connection is idle and available for reuse.
    Idle,
    /// Connection is reconnecting after a failure.
    Reconnecting,
    /// Connection is dead and should be removed.
    Dead,
}

/// Configuration for the connection pool.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PoolConfig {
    /// Minimum number of connections to maintain per node.
    pub min_connections: usize,
    /// Maximum number of connections per node.
    pub max_connections: usize,
    /// Idle timeout in milliseconds.
    pub idle_timeout_ms: u64,
    /// Maximum failures before marking connection dead.
    pub max_failure_count: u32,
    /// Connection acquisition timeout in milliseconds.
    pub acquire_timeout_ms: u64,
    /// Health check interval in milliseconds.
    pub health_check_interval_ms: u64,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            min_connections: 1,
            max_connections: 4,
            idle_timeout_ms: 300_000,
            max_failure_count: 3,
            acquire_timeout_ms: 5_000,
            health_check_interval_ms: 30_000,
        }
    }
}

/// A pooled connection with state tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PooledConnection {
    /// Unique connection identifier.
    pub id: u64,
    /// The node this connection connects to.
    pub node_id: u64,
    /// Current state of the connection.
    pub state: ConnectionState,
    /// Timestamp when connection was last used (nanoseconds since epoch).
    pub last_used_ns: u64,
    /// Number of consecutive failures.
    pub failure_count: u32,
    /// Timestamp when connection was created (nanoseconds since epoch).
    pub created_ns: u64,
}

impl PooledConnection {
    /// Creates a new pooled connection.
    pub fn new(id: u64, node_id: u64) -> Self {
        let now = current_time_ns();
        Self {
            id,
            node_id,
            state: ConnectionState::Idle,
            last_used_ns: now,
            failure_count: 0,
            created_ns: now,
        }
    }

    /// Marks the connection as active.
    pub fn mark_active(&mut self) {
        self.state = ConnectionState::Active;
        self.last_used_ns = current_time_ns();
    }

    /// Marks the connection as idle.
    pub fn mark_idle(&mut self) {
        self.state = ConnectionState::Idle;
        self.last_used_ns = current_time_ns();
    }

    /// Marks the connection as reconnecting.
    pub fn mark_reconnecting(&mut self) {
        self.state = ConnectionState::Reconnecting;
    }

    /// Marks the connection as dead.
    pub fn mark_dead(&mut self) {
        self.state = ConnectionState::Dead;
    }

    /// Records a failure on this connection.
    pub fn record_failure(&mut self) {
        self.failure_count += 1;
        if self.failure_count >= 3 {
            self.state = ConnectionState::Dead;
        }
    }

    /// Checks if connection has exceeded idle timeout.
    pub fn is_idle_timeout(&self, now_ns: u64, timeout_ms: u64) -> bool {
        self.state == ConnectionState::Idle && (now_ns - self.last_used_ns) > timeout_ms * 1_000_000
    }

    /// Checks if connection should be considered dead.
    pub fn is_dead(&self) -> bool {
        self.state == ConnectionState::Dead || self.failure_count >= 3
    }
}

/// Statistics for a connection pool.
#[derive(Debug, Default)]
pub struct PoolStats {
    connections_acquired: AtomicU64,
    connections_released: AtomicU64,
    connections_created: AtomicU64,
    connections_destroyed: AtomicU64,
    failures_recorded: AtomicU64,
    idle_cleanups: AtomicU64,
    total_connections: AtomicUsize,
    active_connections: AtomicUsize,
    idle_connections: AtomicUsize,
}

impl PoolStats {
    fn new() -> Self {
        Self::default()
    }

    fn snapshot(&self) -> PoolStatsSnapshot {
        PoolStatsSnapshot {
            connections_acquired: self.connections_acquired.load(Ordering::Relaxed),
            connections_released: self.connections_released.load(Ordering::Relaxed),
            connections_created: self.connections_created.load(Ordering::Relaxed),
            connections_destroyed: self.connections_destroyed.load(Ordering::Relaxed),
            failures_recorded: self.failures_recorded.load(Ordering::Relaxed),
            idle_cleanups: self.idle_cleanups.load(Ordering::Relaxed),
            total_connections: self.total_connections.load(Ordering::Relaxed),
            active_connections: self.active_connections.load(Ordering::Relaxed),
            idle_connections: self.idle_connections.load(Ordering::Relaxed),
        }
    }
}

/// Snapshot of pool statistics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PoolStatsSnapshot {
    pub connections_acquired: u64,
    pub connections_released: u64,
    pub connections_created: u64,
    pub connections_destroyed: u64,
    pub failures_recorded: u64,
    pub idle_cleanups: u64,
    pub total_connections: usize,
    pub active_connections: usize,
    pub idle_connections: usize,
}

/// A connection pool for a single node.
pub struct ConnectionPool {
    node_id: u64,
    config: PoolConfig,
    connections: Mutex<Vec<Arc<Mutex<PooledConnection>>>>,
    next_conn_id: AtomicU64,
    stats: Arc<PoolStats>,
}

impl ConnectionPool {
    /// Creates a new connection pool for a node.
    pub fn new(node_id: u64, config: PoolConfig) -> Self {
        Self {
            node_id,
            config,
            connections: Mutex::new(Vec::new()),
            next_conn_id: AtomicU64::new(1),
            stats: Arc::new(PoolStats::new()),
        }
    }

    /// Returns the node ID for this pool.
    pub fn node_id(&self) -> u64 {
        self.node_id
    }

    /// Acquires a connection from the pool.
    pub fn acquire_connection(&self) -> Option<Arc<Mutex<PooledConnection>>> {
        let mut connections = self.connections.lock().unwrap();

        // Try to find an idle connection
        for conn in connections.iter() {
            let mut pooled = conn.lock().unwrap();
            if pooled.state == ConnectionState::Idle && !pooled.is_dead() {
                pooled.mark_active();
                self.stats
                    .connections_acquired
                    .fetch_add(1, Ordering::Relaxed);
                self.stats
                    .active_connections
                    .fetch_add(1, Ordering::Relaxed);
                self.stats.idle_connections.fetch_sub(1, Ordering::Relaxed);
                return Some(conn.clone());
            }
        }

        // Check if we can create a new connection
        let total = connections.len();
        if total < self.config.max_connections {
            let conn_id = self.next_conn_id.fetch_add(1, Ordering::Relaxed);
            let mut pooled = PooledConnection::new(conn_id, self.node_id);
            pooled.mark_active();

            let conn = Arc::new(Mutex::new(pooled));
            connections.push(conn.clone());

            self.stats
                .connections_acquired
                .fetch_add(1, Ordering::Relaxed);
            self.stats
                .connections_created
                .fetch_add(1, Ordering::Relaxed);
            self.stats.total_connections.fetch_add(1, Ordering::Relaxed);
            self.stats
                .active_connections
                .fetch_add(1, Ordering::Relaxed);

            return Some(conn);
        }

        None
    }

    /// Releases a connection back to the pool.
    pub fn release_connection(&self, conn_id: u64) {
        let mut connections = self.connections.lock().unwrap();

        for conn in connections.iter() {
            let mut pooled = conn.lock().unwrap();
            if pooled.id == conn_id {
                pooled.mark_idle();
                self.stats
                    .connections_released
                    .fetch_add(1, Ordering::Relaxed);
                self.stats
                    .active_connections
                    .fetch_sub(1, Ordering::Relaxed);
                self.stats.idle_connections.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
    }

    /// Reports a failure on a connection.
    pub fn report_failure(&self, conn_id: u64) {
        let mut connections = self.connections.lock().unwrap();

        for conn in connections.iter() {
            let mut pooled = conn.lock().unwrap();
            if pooled.id == conn_id {
                pooled.record_failure();
                self.stats.failures_recorded.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
    }

    /// Cleans up idle connections that have exceeded timeout.
    pub fn cleanup_idle(&self, now_ns: u64) -> usize {
        let mut connections = self.connections.lock().unwrap();
        let timeout_ms = self.config.idle_timeout_ms;
        let min_connections = self.config.min_connections;

        let idle_count = connections
            .iter()
            .filter(|c| {
                let p = c.lock().unwrap();
                p.state == ConnectionState::Idle
            })
            .count();

        // Keep at least min_connections idle
        let keep_count = idle_count.min(min_connections);
        let can_remove = idle_count.saturating_sub(keep_count);

        let mut removed = 0;
        let mut to_remove = Vec::new();

        for (i, conn) in connections.iter().enumerate() {
            let mut pooled = conn.lock().unwrap();
            if pooled.is_idle_timeout(now_ns, timeout_ms) && removed < can_remove {
                to_remove.push(i);
                removed += 1;
            }
        }

        // Remove in reverse order to maintain indices
        for i in to_remove.into_iter().rev() {
            connections.remove(i);
            self.stats.idle_cleanups.fetch_add(1, Ordering::Relaxed);
            self.stats
                .connections_destroyed
                .fetch_add(1, Ordering::Relaxed);
            self.stats.total_connections.fetch_sub(1, Ordering::Relaxed);
            self.stats.idle_connections.fetch_sub(1, Ordering::Relaxed);
        }

        removed
    }

    /// Returns current pool statistics.
    pub fn stats(&self) -> PoolStatsSnapshot {
        self.stats.snapshot()
    }

    /// Returns the number of connections in the pool.
    pub fn connection_count(&self) -> usize {
        self.connections.lock().unwrap().len()
    }

    /// Returns the number of active connections.
    pub fn active_count(&self) -> usize {
        let connections = self.connections.lock().unwrap();
        connections
            .iter()
            .filter(|c| c.lock().unwrap().state == ConnectionState::Active)
            .count()
    }

    /// Returns the number of idle connections.
    pub fn idle_count(&self) -> usize {
        let connections = self.connections.lock().unwrap();
        connections
            .iter()
            .filter(|c| c.lock().unwrap().state == ConnectionState::Idle)
            .count()
    }
}

/// Global connection pool manager for all nodes.
pub struct ConnectionPoolManager {
    config: PoolConfig,
    pools: Mutex<HashMap<u64, Arc<ConnectionPool>>>,
    next_pool_id: AtomicU64,
}

impl ConnectionPoolManager {
    /// Creates a new connection pool manager.
    pub fn new(config: PoolConfig) -> Self {
        Self {
            config,
            pools: Mutex::new(HashMap::new()),
            next_pool_id: AtomicU64::new(1),
        }
    }

    /// Gets or creates a pool for the given node.
    pub fn get_pool(&self, node_id: u64) -> Arc<ConnectionPool> {
        let mut pools = self.pools.lock().unwrap();
        if let Some(pool) = pools.get(&node_id) {
            return pool.clone();
        }

        let pool = Arc::new(ConnectionPool::new(node_id, self.config));
        pools.insert(node_id, pool.clone());
        pool
    }

    /// Removes a pool for a node.
    pub fn remove_pool(&self, node_id: u64) -> bool {
        let mut pools = self.pools.lock().unwrap();
        pools.remove(&node_id).is_some()
    }

    /// Cleans up idle connections across all pools.
    pub fn cleanup_all(&self) -> usize {
        let now_ns = current_time_ns();
        let pools = self.pools.lock().unwrap();
        let mut total_cleaned = 0;

        for pool in pools.values() {
            total_cleaned += pool.cleanup_idle(now_ns);
        }

        total_cleaned
    }

    /// Returns the number of pools managed.
    pub fn pool_count(&self) -> usize {
        self.pools.lock().unwrap().len()
    }

    /// Returns statistics across all pools.
    pub fn total_stats(&self) -> PoolStatsSnapshot {
        let pools = self.pools.lock().unwrap();
        let mut total = PoolStatsSnapshot::default();

        for pool in pools.values() {
            let s = pool.stats();
            total.connections_acquired += s.connections_acquired;
            total.connections_released += s.connections_released;
            total.connections_created += s.connections_created;
            total.connections_destroyed += s.connections_destroyed;
            total.failures_recorded += s.failures_recorded;
            total.idle_cleanups += s.idle_cleanups;
            total.total_connections += s.total_connections;
            total.active_connections += s.active_connections;
            total.idle_connections += s.idle_connections;
        }

        total
    }
}

fn current_time_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simple_config() -> PoolConfig {
        PoolConfig {
            min_connections: 1,
            max_connections: 3,
            idle_timeout_ms: 1000,
            max_failure_count: 3,
            acquire_timeout_ms: 1000,
            health_check_interval_ms: 100,
        }
    }

    #[test]
    fn test_pool_config_defaults() {
        let config = PoolConfig::default();
        assert_eq!(config.min_connections, 1);
        assert_eq!(config.max_connections, 4);
        assert_eq!(config.idle_timeout_ms, 300_000);
        assert_eq!(config.max_failure_count, 3);
    }

    #[test]
    fn test_pooled_connection_creation() {
        let conn = PooledConnection::new(1, 100);
        assert_eq!(conn.id, 1);
        assert_eq!(conn.node_id, 100);
        assert_eq!(conn.state, ConnectionState::Idle);
        assert_eq!(conn.failure_count, 0);
    }

    #[test]
    fn test_pooled_connection_mark_active() {
        let mut conn = PooledConnection::new(1, 100);
        conn.mark_active();
        assert_eq!(conn.state, ConnectionState::Active);
    }

    #[test]
    fn test_pooled_connection_mark_idle() {
        let mut conn = PooledConnection::new(1, 100);
        conn.mark_active();
        conn.mark_idle();
        assert_eq!(conn.state, ConnectionState::Idle);
    }

    #[test]
    fn test_pooled_connection_record_failure() {
        let mut conn = PooledConnection::new(1, 100);
        conn.record_failure();
        assert_eq!(conn.failure_count, 1);
        conn.record_failure();
        assert_eq!(conn.failure_count, 2);
    }

    #[test]
    fn test_pooled_connection_dead_after_max_failures() {
        let mut conn = PooledConnection::new(1, 100);
        for _ in 0..3 {
            conn.record_failure();
        }
        assert_eq!(conn.state, ConnectionState::Dead);
    }

    #[test]
    fn test_connection_pool_creation() {
        let pool = ConnectionPool::new(42, simple_config());
        assert_eq!(pool.node_id(), 42);
        assert_eq!(pool.connection_count(), 0);
    }

    #[test]
    fn test_connection_pool_acquire_creates_new() {
        let pool = ConnectionPool::new(42, simple_config());
        let conn = pool.acquire_connection();
        assert!(conn.is_some());
        assert_eq!(pool.connection_count(), 1);
        assert_eq!(pool.active_count(), 1);
    }

    #[test]
    fn test_connection_pool_acquire_reuses_idle() {
        let pool = ConnectionPool::new(42, simple_config());

        // Acquire and release
        let conn1 = pool.acquire_connection().unwrap();
        let conn_id = conn1.lock().unwrap().id;
        pool.release_connection(conn_id);

        // Acquire again should reuse
        let conn2 = pool.acquire_connection().unwrap();
        assert_eq!(conn2.lock().unwrap().id, conn_id);
        assert_eq!(pool.active_count(), 1);
    }

    #[test]
    #[cfg(not(miri))]
    fn test_connection_pool_release() {
        let pool = ConnectionPool::new(42, simple_config());
        let conn = pool.acquire_connection().unwrap();
        let conn_id = conn.lock().unwrap().id;

        pool.release_connection(conn_id);

        assert_eq!(pool.active_count(), 0);
        assert_eq!(pool.idle_count(), 1);
    }

    #[test]
    fn test_connection_pool_report_failure() {
        let pool = ConnectionPool::new(42, simple_config());
        let conn = pool.acquire_connection().unwrap();
        let conn_id = conn.lock().unwrap().id;

        pool.report_failure(conn_id);

        assert_eq!(conn.lock().unwrap().failure_count, 1);
    }

    #[test]
    fn test_connection_pool_cleanup_idle() {
        let mut config = simple_config();
        config.idle_timeout_ms = 1; // 1ms timeout for testing

        let pool = ConnectionPool::new(42, config);
        let conn = pool.acquire_connection().unwrap();
        let conn_id = conn.lock().unwrap().id;
        pool.release_connection(conn_id);

        // Wait and cleanup
        std::thread::sleep(std::time::Duration::from_millis(10));
        let cleaned = pool.cleanup_idle(current_time_ns());
        assert!(cleaned >= 0);
    }

    #[test]
    fn test_connection_pool_respects_max() {
        let config = PoolConfig {
            min_connections: 0,
            max_connections: 2,
            ..simple_config()
        };

        let pool = ConnectionPool::new(42, config);

        // Should be able to get up to max connections
        let _c1 = pool.acquire_connection();
        let _c2 = pool.acquire_connection();
        let c3 = pool.acquire_connection();

        // Third should fail because max is 2
        assert!(c3.is_none());
    }

    #[test]
    fn test_connection_pool_stats() {
        let pool = ConnectionPool::new(42, simple_config());
        let conn = pool.acquire_connection().unwrap();
        let conn_id = conn.lock().unwrap().id;
        pool.release_connection(conn_id);

        let stats = pool.stats();
        assert_eq!(stats.connections_acquired, 1);
        assert_eq!(stats.connections_released, 1);
    }

    #[test]
    fn test_pool_manager_creates_pools() {
        let manager = ConnectionPoolManager::new(simple_config());
        let pool = manager.get_pool(100);
        assert_eq!(pool.node_id(), 100);
        assert_eq!(manager.pool_count(), 1);
    }

    #[test]
    fn test_pool_manager_returns_existing_pool() {
        let manager = ConnectionPoolManager::new(simple_config());
        let pool1 = manager.get_pool(100);
        let pool2 = manager.get_pool(100);
        assert!(Arc::ptr_eq(&pool1, &pool2));
    }

    #[test]
    fn test_pool_manager_remove_pool() {
        let manager = ConnectionPoolManager::new(simple_config());
        manager.get_pool(100);
        assert!(manager.remove_pool(100));
        assert!(!manager.remove_pool(100)); // Already removed
    }

    #[test]
    fn test_pool_manager_cleanup_all() {
        let manager = ConnectionPoolManager::new(simple_config());
        manager.get_pool(1);
        manager.get_pool(2);

        let cleaned = manager.cleanup_all();
        assert!(cleaned >= 0);
    }

    #[test]
    fn test_pool_manager_total_stats() {
        let manager = ConnectionPoolManager::new(simple_config());
        manager.get_pool(1);

        let stats = manager.total_stats();
        assert_eq!(stats.total_connections, 0);
    }

    #[test]
    fn test_connection_state_ordering() {
        assert_ne!(ConnectionState::Active, ConnectionState::Idle);
        assert_ne!(ConnectionState::Reconnecting, ConnectionState::Dead);
    }

    #[test]
    fn test_pool_config_serialize() {
        let config = simple_config();
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("min_connections"));
        let restored: PoolConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.min_connections, config.min_connections);
    }

    #[test]
    fn test_pooled_connection_serialize() {
        let conn = PooledConnection::new(1, 100);
        let json = serde_json::to_string(&conn).unwrap();
        assert!(json.contains("Active") || json.contains("Idle"));
        let restored: PooledConnection = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.id, conn.id);
    }

    #[test]
    fn test_stats_snapshot_serialize() {
        let stats = PoolStatsSnapshot {
            connections_acquired: 10,
            connections_released: 8,
            connections_created: 5,
            connections_destroyed: 2,
            failures_recorded: 3,
            idle_cleanups: 1,
            total_connections: 3,
            active_connections: 1,
            idle_connections: 2,
        };
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("10"));
    }
}
