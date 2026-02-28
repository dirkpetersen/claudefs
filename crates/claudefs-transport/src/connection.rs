//! Connection pool for managing TCP connections to cluster peers.

use crate::error::Result;
use crate::tcp::{TcpTransport, TcpConnection};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Connection pool configuration.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum connections per peer address (default: 4).
    pub max_connections_per_peer: usize,
    /// Idle connection timeout in seconds (default: 300).
    pub idle_timeout_secs: u64,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self { max_connections_per_peer: 4, idle_timeout_secs: 300 }
    }
}

/// Pool statistics.
#[derive(Debug, Clone)]
pub struct PoolStats {
    /// Total connections across all peers.
    pub total_connections: usize,
    /// Number of connected peers.
    pub peers: usize,
}

/// Connection pool managing connections to multiple peers.
pub struct ConnectionPool {
    transport: Arc<TcpTransport>,
    config: PoolConfig,
    connections: Arc<Mutex<HashMap<String, Vec<Arc<TcpConnection>>>>>,
}

impl ConnectionPool {
    /// Create a new connection pool.
    pub fn new(transport: Arc<TcpTransport>, config: PoolConfig) -> Self {
        Self {
            transport,
            config,
            connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get or create a connection to the given peer address.
    pub async fn get_connection(&self, addr: &str) -> Result<Arc<TcpConnection>> {
        let mut conns = self.connections.lock().await;
        if let Some(pool) = conns.get_mut(addr) {
            if let Some(conn) = pool.pop() {
                return Ok(conn);
            }
        }
        drop(conns);
        let conn = self.transport.connect(addr).await?;
        Ok(Arc::new(conn))
    }

    /// Return a connection to the pool for reuse.
    pub async fn return_connection(&self, addr: &str, conn: Arc<TcpConnection>) {
        let mut conns = self.connections.lock().await;
        let pool = conns.entry(addr.to_string()).or_default();
        if pool.len() < self.config.max_connections_per_peer {
            pool.push(conn);
        }
    }

    /// Remove all connections for a peer.
    pub async fn remove_peer(&self, addr: &str) {
        let mut conns = self.connections.lock().await;
        conns.remove(addr);
    }

    /// Get pool statistics.
    pub async fn stats(&self) -> PoolStats {
        let conns = self.connections.lock().await;
        let total = conns.values().map(|v| v.len()).sum();
        PoolStats { total_connections: total, peers: conns.len() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tcp::TcpTransportConfig;

    #[tokio::test]
    async fn test_pool_stats() {
        let transport = Arc::new(TcpTransport::new(TcpTransportConfig::default()));
        let pool = ConnectionPool::new(transport, PoolConfig::default());
        let stats = pool.stats().await;
        assert_eq!(stats.total_connections, 0);
        assert_eq!(stats.peers, 0);
    }
}
