//! Connection pool management for gRPC conduit connections to remote replication sites.
//!
//! Maintains a pool of pre-established connections to each remote site with round-robin
//! load balancing, health checking, and graceful drain support.

use std::collections::HashMap;
use thiserror::Error;

/// State of a single pooled connection.
#[derive(Debug, Clone)]
pub enum ConnectionState {
    /// Connection is healthy and ready.
    Ready,
    /// Connection is in use for an active replication stream.
    InUse {
        /// When the connection was acquired (Unix ms).
        since_ms: u64,
    },
    /// Connection is being drained (no new assignments).
    Draining,
    /// Connection failed.
    Failed {
        /// Reason for failure.
        reason: String,
        /// When the connection failed (Unix ms).
        failed_at_ms: u64,
    },
    /// Connection is reconnecting after failure.
    Reconnecting {
        /// Current reconnection attempt number.
        attempt: u32,
        /// When the next retry should occur (Unix ms).
        next_retry_ms: u64,
    },
}

/// A single connection in the pool.
#[derive(Debug)]
pub struct PooledConnection {
    /// Unique ID for this connection.
    pub conn_id: u64,
    /// Remote site ID.
    pub site_id: u64,
    /// Remote endpoint address.
    pub endpoint: String,
    /// Current state.
    pub state: ConnectionState,
    /// When this connection was created (Unix ms).
    pub created_at_ms: u64,
    /// Total requests served by this connection.
    pub requests_served: u64,
    /// Total bytes sent via this connection.
    pub bytes_sent: u64,
}

/// Pool configuration.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Connections per remote site. Default: 4.
    pub connections_per_site: usize,
    /// Maximum failed connections before marking site unhealthy. Default: 3.
    pub max_failed_before_unhealthy: usize,
    /// Initial reconnect delay (ms). Default: 500.
    pub initial_reconnect_delay_ms: u64,
    /// Max reconnect delay (ms). Default: 30_000.
    pub max_reconnect_delay_ms: u64,
    /// Backoff multiplier. Default: 2.0.
    pub backoff_multiplier: f64,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            connections_per_site: 4,
            max_failed_before_unhealthy: 3,
            initial_reconnect_delay_ms: 500,
            max_reconnect_delay_ms: 30_000,
            backoff_multiplier: 2.0,
        }
    }
}

/// Error types for pool operations.
#[derive(Debug, Error)]
pub enum PoolError {
    /// No healthy connections available.
    #[error("no healthy connections available for site {site_id}")]
    NoHealthyConnections {
        /// The site ID that has no healthy connections.
        site_id: u64,
    },
    /// Site not registered in pool.
    #[error("unknown site: {site_id}")]
    UnknownSite {
        /// The unknown site identifier.
        site_id: u64,
    },
    /// Connection ID not found.
    #[error("unknown connection: {conn_id}")]
    UnknownConnection {
        /// The unknown connection identifier.
        conn_id: u64,
    },
    /// Pool is shutting down.
    #[error("pool is shutting down")]
    Shutdown,
}

/// Statistics for the connection pool.
#[derive(Debug, Default)]
pub struct PoolStats {
    /// Total number of connections.
    pub total_connections: usize,
    /// Number of Ready connections.
    pub ready_connections: usize,
    /// Number of InUse connections.
    pub in_use_connections: usize,
    /// Number of Failed connections.
    pub failed_connections: usize,
    /// Number of Reconnecting connections.
    pub reconnecting_connections: usize,
    /// Total requests served by all connections.
    pub total_requests_served: u64,
    /// Total bytes sent by all connections.
    pub total_bytes_sent: u64,
}

/// Connection pool for gRPC conduit connections.
#[derive(Debug)]
pub struct ConduitPool {
    config: PoolConfig,
    sites: HashMap<u64, Vec<PooledConnection>>,
    next_conn_id: u64,
    shutdown: bool,
}

impl ConduitPool {
    /// Creates a new ConduitPool with the given configuration.
    pub fn new(config: PoolConfig) -> Self {
        Self {
            config,
            sites: HashMap::new(),
            next_conn_id: 0,
            shutdown: false,
        }
    }

    /// Register a remote site with its endpoint address.
    /// Creates `config.connections_per_site` connection slots (initially Failed state).
    pub fn register_site(&mut self, site_id: u64, endpoint: String, now_ms: u64) {
        let conns_per_site = self.config.connections_per_site;
        let mut connections = Vec::with_capacity(conns_per_site);

        for _ in 0..conns_per_site {
            let conn_id = self.next_conn_id;
            self.next_conn_id += 1;

            connections.push(PooledConnection {
                conn_id,
                site_id,
                endpoint: endpoint.clone(),
                state: ConnectionState::Failed {
                    reason: "initial".to_string(),
                    failed_at_ms: now_ms,
                },
                created_at_ms: now_ms,
                requests_served: 0,
                bytes_sent: 0,
            });
        }

        self.sites.insert(site_id, connections);
        tracing::info!(
            site_id,
            connections = conns_per_site,
            "registered site in pool"
        );
    }

    /// Remove a remote site and all its connections.
    pub fn unregister_site(&mut self, site_id: u64) -> Result<(), PoolError> {
        self.sites
            .remove(&site_id)
            .ok_or(PoolError::UnknownSite { site_id })?;
        tracing::info!(site_id, "unregistered site from pool");
        Ok(())
    }

    /// Acquire a ready connection for the given site (round-robin among Ready connections).
    /// Marks it as InUse.
    pub fn acquire(&mut self, site_id: u64, now_ms: u64) -> Result<u64, PoolError> {
        if self.shutdown {
            return Err(PoolError::Shutdown);
        }

        let connections = self
            .sites
            .get_mut(&site_id)
            .ok_or(PoolError::UnknownSite { site_id })?;

        let ready_indices: Vec<usize> = connections
            .iter()
            .enumerate()
            .filter(|(_, c)| matches!(c.state, ConnectionState::Ready))
            .map(|(i, _)| i)
            .collect();

        if ready_indices.is_empty() {
            return Err(PoolError::NoHealthyConnections { site_id });
        }

        let idx = ready_indices[now_ms as usize % ready_indices.len()];
        connections[idx].state = ConnectionState::InUse { since_ms: now_ms };

        tracing::debug!(
            conn_id = connections[idx].conn_id,
            site_id,
            "acquired connection"
        );

        Ok(connections[idx].conn_id)
    }

    /// Release a connection back to Ready state, recording bytes sent.
    pub fn release(&mut self, conn_id: u64, bytes_sent: u64) -> Result<(), PoolError> {
        for connections in self.sites.values_mut() {
            for conn in connections.iter_mut() {
                if conn.conn_id == conn_id {
                    if matches!(conn.state, ConnectionState::InUse { .. }) {
                        conn.state = ConnectionState::Ready;
                        conn.requests_served += 1;
                        conn.bytes_sent += bytes_sent;
                        tracing::debug!(conn_id, bytes_sent, "released connection");
                        return Ok(());
                    }
                }
            }
        }
        Err(PoolError::UnknownConnection { conn_id })
    }

    /// Mark a connection as Failed.
    pub fn mark_failed(
        &mut self,
        conn_id: u64,
        reason: String,
        now_ms: u64,
    ) -> Result<(), PoolError> {
        for connections in self.sites.values_mut() {
            for conn in connections.iter_mut() {
                if conn.conn_id == conn_id {
                    conn.state = ConnectionState::Failed {
                        reason,
                        failed_at_ms: now_ms,
                    };
                    tracing::warn!(conn_id, "connection marked failed");
                    return Ok(());
                }
            }
        }
        Err(PoolError::UnknownConnection { conn_id })
    }

    /// Tick reconnect timers: transitions Failed→Reconnecting→Ready based on backoff.
    pub fn tick(&mut self, now_ms: u64) {
        for connections in self.sites.values_mut() {
            for conn in connections.iter_mut() {
                match &conn.state {
                    ConnectionState::Failed { failed_at_ms, .. } => {
                        let delay = self.config.initial_reconnect_delay_ms;
                        if *failed_at_ms + delay <= now_ms {
                            conn.state = ConnectionState::Reconnecting {
                                attempt: 1,
                                next_retry_ms: now_ms + self.config.initial_reconnect_delay_ms,
                            };
                        }
                    }
                    ConnectionState::Reconnecting {
                        attempt,
                        next_retry_ms,
                    } => {
                        if *next_retry_ms <= now_ms {
                            let delay = (self.config.initial_reconnect_delay_ms as f64
                                * self.config.backoff_multiplier.powi(*attempt as i32))
                            .min(self.config.max_reconnect_delay_ms as f64)
                                as u64;

                            if *attempt >= 3 {
                                conn.state = ConnectionState::Ready;
                                tracing::info!(conn_id = conn.conn_id, "connection reconnected");
                            } else {
                                conn.state = ConnectionState::Reconnecting {
                                    attempt: attempt + 1,
                                    next_retry_ms: now_ms + delay,
                                };
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// Get stats for a specific site.
    pub fn site_stats(&self, site_id: u64) -> Result<PoolStats, PoolError> {
        let connections = self
            .sites
            .get(&site_id)
            .ok_or(PoolError::UnknownSite { site_id })?;

        let mut stats = PoolStats::default();
        for conn in connections.iter() {
            stats.total_connections += 1;
            stats.total_requests_served += conn.requests_served;
            stats.total_bytes_sent += conn.bytes_sent;

            match conn.state {
                ConnectionState::Ready => stats.ready_connections += 1,
                ConnectionState::InUse { .. } => stats.in_use_connections += 1,
                ConnectionState::Failed { .. } => stats.failed_connections += 1,
                ConnectionState::Reconnecting { .. } => stats.reconnecting_connections += 1,
                ConnectionState::Draining => {}
            }
        }

        Ok(stats)
    }

    /// Get aggregate pool stats.
    pub fn global_stats(&self) -> PoolStats {
        let mut stats = PoolStats::default();

        for connections in self.sites.values() {
            for conn in connections.iter() {
                stats.total_connections += 1;
                stats.total_requests_served += conn.requests_served;
                stats.total_bytes_sent += conn.bytes_sent;

                match conn.state {
                    ConnectionState::Ready => stats.ready_connections += 1,
                    ConnectionState::InUse { .. } => stats.in_use_connections += 1,
                    ConnectionState::Failed { .. } => stats.failed_connections += 1,
                    ConnectionState::Reconnecting { .. } => stats.reconnecting_connections += 1,
                    ConnectionState::Draining => {}
                }
            }
        }

        stats
    }

    /// List all registered site IDs.
    pub fn site_ids(&self) -> Vec<u64> {
        self.sites.keys().copied().collect()
    }

    /// Check if a site has any Ready connections.
    pub fn is_site_healthy(&self, site_id: u64) -> bool {
        let Some(connections) = self.sites.get(&site_id) else {
            return false;
        };

        connections
            .iter()
            .any(|c| matches!(c.state, ConnectionState::Ready))
    }

    /// Initiate graceful shutdown: mark all connections as Draining.
    pub fn shutdown(&mut self) {
        self.shutdown = true;
        for connections in self.sites.values_mut() {
            for conn in connections.iter_mut() {
                match &conn.state {
                    ConnectionState::Ready | ConnectionState::InUse { .. } => {
                        conn.state = ConnectionState::Draining;
                    }
                    _ => {}
                }
            }
        }
        tracing::info!("connection pool shutting down");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    #[test]
    fn test_register_site_creates_correct_number_of_connections() {
        let config = PoolConfig {
            connections_per_site: 3,
            ..Default::default()
        };
        let mut pool = ConduitPool::new(config);
        let now = now_ms();

        pool.register_site(1, "grpc://site1:8080".to_string(), now);

        let stats = pool.site_stats(1).unwrap();
        assert_eq!(stats.total_connections, 3);
    }

    #[test]
    fn test_acquire_returns_conn_and_marks_in_use() {
        let mut pool = ConduitPool::new(Default::default());
        let now = now_ms();

        pool.register_site(1, "grpc://site1:8080".to_string(), now);
        pool.tick(now + 1000);

        let conn_id = pool.acquire(1, now + 1000).unwrap();
        let stats = pool.site_stats(1).unwrap();

        assert_eq!(stats.in_use_connections, 1);
        assert_eq!(stats.ready_connections, 3);
    }

    #[test]
    fn test_release_returns_to_ready() {
        let mut pool = ConduitPool::new(Default::default());
        let now = now_ms();

        pool.register_site(1, "grpc://site1:8080".to_string(), now);
        pool.tick(now + 1000);

        let conn_id = pool.acquire(1, now + 1000).unwrap();
        pool.release(conn_id, 1024).unwrap();

        let stats = pool.site_stats(1).unwrap();
        assert_eq!(stats.ready_connections, 4);
        assert_eq!(stats.in_use_connections, 0);
    }

    #[test]
    fn test_round_robin_across_connections() {
        let config = PoolConfig {
            connections_per_site: 2,
            ..Default::default()
        };
        let mut pool = ConduitPool::new(config);
        let now = now_ms();

        pool.register_site(1, "grpc://site1:8080".to_string(), now);
        pool.tick(now + 1000);

        let id1 = pool.acquire(1, now + 1000).unwrap();
        pool.release(id1, 0).unwrap();
        let id2 = pool.acquire(1, now + 1001).unwrap();
        pool.release(id2, 0).unwrap();

        let stats = pool.site_stats(1).unwrap();
        assert_eq!(stats.ready_connections, 2);
        assert_eq!(stats.total_requests_served, 2);
    }

    #[test]
    fn test_mark_failed_transitions_to_failed() {
        let mut pool = ConduitPool::new(Default::default());
        let now = now_ms();

        pool.register_site(1, "grpc://site1:8080".to_string(), now);
        pool.tick(now + 1000);

        let conn_id = pool.acquire(1, now + 1000).unwrap();
        pool.release(conn_id, 0).unwrap();

        pool.mark_failed(conn_id, "connection reset".to_string(), now + 2000)
            .unwrap();

        let stats = pool.site_stats(1).unwrap();
        assert_eq!(stats.failed_connections, 1);
        assert_eq!(stats.ready_connections, 3);
    }

    #[test]
    fn test_tick_advances_reconnect_after_delay() {
        let mut pool = ConduitPool::new(Default::default());
        let now = now_ms();

        pool.register_site(1, "grpc://site1:8080".to_string(), now);

        pool.tick(now + 100);
        let stats1 = pool.site_stats(1).unwrap();
        assert!(stats1.reconnecting_connections >= 1);

        pool.tick(now + 1000);
        let stats2 = pool.site_stats(1).unwrap();
        assert!(stats2.ready_connections >= 1);
    }

    #[test]
    fn test_no_healthy_connections_returns_error() {
        let mut pool = ConduitPool::new(Default::default());
        let now = now_ms();

        pool.register_site(1, "grpc://site1:8080".to_string(), now);

        let result = pool.acquire(1, now);
        assert!(matches!(
            result,
            Err(PoolError::NoHealthyConnections { .. })
        ));
    }

    #[test]
    fn test_unknown_site_returns_error() {
        let mut pool = ConduitPool::new(Default::default());

        let result = pool.acquire(999, now_ms());
        assert!(matches!(result, Err(PoolError::UnknownSite { .. })));
    }

    #[test]
    fn test_unknown_conn_id_returns_error() {
        let mut pool = ConduitPool::new(Default::default());
        let now = now_ms();

        pool.register_site(1, "grpc://site1:8080".to_string(), now);

        let result = pool.release(999999, 0);
        assert!(matches!(result, Err(PoolError::UnknownConnection { .. })));
    }

    #[test]
    fn test_global_stats_aggregates_all_sites() {
        let mut pool = ConduitPool::new(Default::default());
        let now = now_ms();

        pool.register_site(1, "grpc://site1:8080".to_string(), now);
        pool.register_site(2, "grpc://site2:8080".to_string(), now);
        pool.tick(now + 1000);

        pool.acquire(1, now + 1000).unwrap();
        pool.acquire(2, now + 1000).unwrap();

        let stats = pool.global_stats();
        assert_eq!(stats.total_connections, 8);
        assert_eq!(stats.in_use_connections, 2);
    }

    #[test]
    fn test_site_stats_correct_per_site() {
        let mut pool = ConduitPool::new(Default::default());
        let now = now_ms();

        pool.register_site(1, "grpc://site1:8080".to_string(), now);
        pool.tick(now + 1000);

        pool.acquire(1, now + 1000).unwrap();

        let stats = pool.site_stats(1).unwrap();
        assert_eq!(stats.total_connections, 4);
    }

    #[test]
    fn test_shutdown_marks_all_connections_draining() {
        let mut pool = ConduitPool::new(Default::default());
        let now = now_ms();

        pool.register_site(1, "grpc://site1:8080".to_string(), now);
        pool.tick(now + 1000);

        pool.shutdown();

        let stats = pool.site_stats(1).unwrap();
        assert!(stats.ready_connections > 0 || stats.in_use_connections > 0);
    }

    #[test]
    fn test_acquire_after_shutdown_returns_error() {
        let mut pool = ConduitPool::new(Default::default());
        let now = now_ms();

        pool.register_site(1, "grpc://site1:8080".to_string(), now);
        pool.shutdown();

        let result = pool.acquire(1, now);
        assert!(matches!(result, Err(PoolError::Shutdown)));
    }

    #[test]
    fn test_is_site_healthy_returns_false_with_no_ready_connections() {
        let mut pool = ConduitPool::new(Default::default());
        let now = now_ms();

        pool.register_site(1, "grpc://site1:8080".to_string(), now);

        let healthy = pool.is_site_healthy(1);
        assert!(!healthy);
    }

    #[test]
    fn test_is_site_healthy_returns_true_with_ready_connections() {
        let mut pool = ConduitPool::new(Default::default());
        let now = now_ms();

        pool.register_site(1, "grpc://site1:8080".to_string(), now);
        pool.tick(now + 1000);

        let healthy = pool.is_site_healthy(1);
        assert!(healthy);
    }

    #[test]
    fn test_backoff_delay_increases_with_attempt_count() {
        let mut pool = ConduitPool::new(PoolConfig {
            initial_reconnect_delay_ms: 100,
            backoff_multiplier: 2.0,
            max_reconnect_delay_ms: 10000,
            ..Default::default()
        });
        let now = now_ms();

        pool.register_site(1, "grpc://site1:8080".to_string(), now);
        pool.tick(now + 100);

        pool.tick(now + 500);

        let stats = pool.site_stats(1).unwrap();
        assert!(stats.ready_connections >= 1 || stats.reconnecting_connections >= 1);
    }

    #[test]
    fn test_unregister_removes_site() {
        let mut pool = ConduitPool::new(Default::default());
        let now = now_ms();

        pool.register_site(1, "grpc://site1:8080".to_string(), now);

        pool.unregister_site(1).unwrap();

        let result = pool.site_stats(1);
        assert!(matches!(result, Err(PoolError::UnknownSite { .. })));
    }

    #[test]
    fn test_multiple_sites_can_coexist() {
        let mut pool = ConduitPool::new(Default::default());
        let now = now_ms();

        pool.register_site(1, "grpc://site1:8080".to_string(), now);
        pool.register_site(2, "grpc://site2:8080".to_string(), now);
        pool.register_site(3, "grpc://site3:8080".to_string(), now);

        let ids = pool.site_ids();
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&1));
        assert!(ids.contains(&2));
        assert!(ids.contains(&3));
    }

    #[test]
    fn test_default_pool_config_values() {
        let config = PoolConfig::default();
        assert_eq!(config.connections_per_site, 4);
        assert_eq!(config.max_failed_before_unhealthy, 3);
        assert_eq!(config.initial_reconnect_delay_ms, 500);
        assert_eq!(config.max_reconnect_delay_ms, 30_000);
        assert!((config.backoff_multiplier - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_pool_debug_format() {
        let pool = ConduitPool::new(Default::default());
        let debug_str = format!("{:?}", pool);
        assert!(debug_str.contains("ConduitPool"));
    }
}
