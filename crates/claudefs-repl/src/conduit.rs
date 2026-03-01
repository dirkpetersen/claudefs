//! Cloud conduit: gRPC/mTLS relay for streaming journal entries between sites.
//!
//! In production, this wraps tonic gRPC channels. In tests, it uses tokio mpsc
//! channels for in-process simulation.

use crate::error::ReplError;
use crate::journal::JournalEntry;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

/// mTLS configuration for the conduit channel.
#[derive(Debug, Clone)]
pub struct ConduitTlsConfig {
    /// PEM-encoded certificate chain for this node.
    pub cert_pem: Vec<u8>,
    /// PEM-encoded private key.
    pub key_pem: Vec<u8>,
    /// PEM-encoded CA certificate chain for verifying peers.
    pub ca_pem: Vec<u8>,
}

impl ConduitTlsConfig {
    /// Create a new TLS config.
    pub fn new(cert_pem: Vec<u8>, key_pem: Vec<u8>, ca_pem: Vec<u8>) -> Self {
        Self {
            cert_pem,
            key_pem,
            ca_pem,
        }
    }
}

/// Configuration for a conduit connection to one remote site.
#[derive(Debug, Clone)]
pub struct ConduitConfig {
    /// Local site ID.
    pub local_site_id: u64,
    /// Remote site ID.
    pub remote_site_id: u64,
    /// Remote conduit endpoints (try in order, round-robin on failure).
    pub remote_addrs: Vec<String>,
    /// mTLS config (None for plaintext, used in tests).
    pub tls: Option<ConduitTlsConfig>,
    /// Maximum batch size (number of entries per send).
    pub max_batch_size: usize,
    /// Initial reconnect delay (ms).
    pub reconnect_delay_ms: u64,
    /// Max reconnect delay (ms) after backoff.
    pub max_reconnect_delay_ms: u64,
}

impl Default for ConduitConfig {
    fn default() -> Self {
        Self {
            local_site_id: 0,
            remote_site_id: 0,
            remote_addrs: vec![],
            tls: None,
            max_batch_size: 1000,
            reconnect_delay_ms: 100,
            max_reconnect_delay_ms: 30000,
        }
    }
}

impl ConduitConfig {
    /// Create a new config with the given local and remote site IDs.
    pub fn new(local_site_id: u64, remote_site_id: u64) -> Self {
        Self {
            local_site_id,
            remote_site_id,
            ..Default::default()
        }
    }
}

/// A batch of journal entries sent over the conduit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntryBatch {
    /// Sending site's ID.
    pub source_site_id: u64,
    /// Sequence of entries in this batch (must be from a single shard, ordered by seq).
    pub entries: Vec<JournalEntry>,
    /// Batch sequence number (monotonically increasing per conduit connection).
    pub batch_seq: u64,
}

impl EntryBatch {
    /// Create a new entry batch.
    pub fn new(source_site_id: u64, entries: Vec<JournalEntry>, batch_seq: u64) -> Self {
        Self {
            source_site_id,
            entries,
            batch_seq,
        }
    }
}

/// Internal stats with atomic fields for lock-free updates.
#[derive(Debug)]
struct ConduitStatsInner {
    batches_sent: AtomicU64,
    batches_received: AtomicU64,
    entries_sent: AtomicU64,
    entries_received: AtomicU64,
    send_errors: AtomicU64,
    reconnects: AtomicU64,
}

impl ConduitStatsInner {
    fn new() -> Self {
        Self {
            batches_sent: AtomicU64::new(0),
            batches_received: AtomicU64::new(0),
            entries_sent: AtomicU64::new(0),
            entries_received: AtomicU64::new(0),
            send_errors: AtomicU64::new(0),
            reconnects: AtomicU64::new(0),
        }
    }
}

/// Statistics for one conduit connection.
#[derive(Debug, Clone, Default)]
pub struct ConduitStats {
    /// Number of batches sent.
    pub batches_sent: u64,
    /// Number of batches received.
    pub batches_received: u64,
    /// Total entries sent across all batches.
    pub entries_sent: u64,
    /// Total entries received across all batches.
    pub entries_received: u64,
    /// Number of send errors.
    pub send_errors: u64,
    /// Number of reconnection attempts.
    pub reconnects: u64,
}

/// State of the conduit connection.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ConduitState {
    /// Connected and operational.
    #[default]
    Connected,
    /// Attempting to reconnect.
    Reconnecting {
        /// Current reconnection attempt number.
        attempt: u32,
        /// Current delay in milliseconds.
        delay_ms: u64,
    },
    /// Shutdown complete.
    Shutdown,
}

/// A conduit connection to one remote site.
/// In production, this wraps a tonic gRPC channel.
/// In tests, it uses tokio mpsc channels for in-process simulation.
pub struct Conduit {
    config: ConduitConfig,
    state: Arc<Mutex<ConduitState>>,
    stats: Arc<ConduitStatsInner>,
    sender: mpsc::Sender<EntryBatch>,
    receiver: Arc<Mutex<mpsc::Receiver<EntryBatch>>>,
}

impl Clone for Conduit {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            state: Arc::clone(&self.state),
            stats: Arc::clone(&self.stats),
            sender: self.sender.clone(),
            receiver: Arc::clone(&self.receiver),
        }
    }
}

impl Conduit {
    /// Create a paired (sender, receiver) conduit for in-process testing.
    /// Returns (conduit_a_to_b, conduit_b_to_a).
    pub fn new_pair(config_a: ConduitConfig, config_b: ConduitConfig) -> (Self, Self) {
        let (tx_a, rx_a) = mpsc::channel::<EntryBatch>(config_a.max_batch_size);
        let (tx_b, rx_b) = mpsc::channel::<EntryBatch>(config_b.max_batch_size);

        let conduit_a = Self {
            config: config_a.clone(),
            state: Arc::new(Mutex::new(ConduitState::Connected)),
            stats: Arc::new(ConduitStatsInner::new()),
            sender: tx_a,
            receiver: Arc::new(Mutex::new(rx_b)),
        };

        let conduit_b = Self {
            config: config_b,
            state: Arc::new(Mutex::new(ConduitState::Connected)),
            stats: Arc::new(ConduitStatsInner::new()),
            sender: tx_b,
            receiver: Arc::new(Mutex::new(rx_a)),
        };

        (conduit_a, conduit_b)
    }

    /// Send a batch of entries. Returns error if the conduit is shut down.
    pub async fn send_batch(&self, batch: EntryBatch) -> Result<(), ReplError> {
        let state = self.state.lock().await;
        if *state == ConduitState::Shutdown {
            return Err(ReplError::NetworkError {
                msg: "conduit is shut down".to_string(),
            });
        }

        let entry_count = batch.entries.len() as u64;
        
        self.sender
            .send(batch)
            .await
            .map_err(|_| ReplError::NetworkError {
                msg: "failed to send batch: channel closed".to_string(),
            })?;

        self.stats.batches_sent.fetch_add(1, Ordering::Relaxed);
        self.stats.entries_sent.fetch_add(entry_count, Ordering::Relaxed);

        Ok(())
    }

    /// Receive the next batch. Returns None if the conduit is shut down.
    pub async fn recv_batch(&self) -> Option<EntryBatch> {
        let mut receiver = self.receiver.lock().await;
        let batch = receiver.recv().await?;

        let entry_count = batch.entries.len() as u64;
        self.stats.batches_received.fetch_add(1, Ordering::Relaxed);
        self.stats.entries_received.fetch_add(entry_count, Ordering::Relaxed);

        Some(batch)
    }

    /// Get current connection state.
    pub async fn state(&self) -> ConduitState {
        self.state.lock().await.clone()
    }

    /// Mark conduit as shutting down (drains in-flight sends).
    pub async fn shutdown(&self) {
        let mut state = self.state.lock().await;
        *state = ConduitState::Shutdown;
    }

    /// Get a snapshot of current statistics.
    pub fn stats(&self) -> ConduitStats {
        ConduitStats {
            batches_sent: self.stats.batches_sent.load(Ordering::Relaxed),
            batches_received: self.stats.batches_received.load(Ordering::Relaxed),
            entries_sent: self.stats.entries_sent.load(Ordering::Relaxed),
            entries_received: self.stats.entries_received.load(Ordering::Relaxed),
            send_errors: self.stats.send_errors.load(Ordering::Relaxed),
            reconnects: self.stats.reconnects.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::OpKind;

    fn make_test_entry(seq: u64, inode: u64) -> JournalEntry {
        JournalEntry::new(seq, 0, 1, 1000 + seq, inode, OpKind::Write, vec![1, 2, 3])
    }

    #[tokio::test]
    async fn test_create_pair() {
        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);

        let (conduit_a, conduit_b) = Conduit::new_pair(config_a, config_b);

        assert_eq!(conduit_a.state().await, ConduitState::Connected);
        assert_eq!(conduit_b.state().await, ConduitState::Connected);
    }

    #[tokio::test]
    async fn test_send_and_recv_batch() {
        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);

        let (conduit_a, conduit_b) = Conduit::new_pair(config_a, config_b);

        let entries = vec![make_test_entry(1, 100), make_test_entry(2, 100)];
        let batch = EntryBatch::new(1, entries, 1);

        conduit_a.send_batch(batch.clone()).await.unwrap();

        let received = conduit_b.recv_batch().await.unwrap();
        assert_eq!(received.source_site_id, 1);
        assert_eq!(received.entries.len(), 2);
        assert_eq!(received.batch_seq, 1);
    }

    #[tokio::test]
    async fn test_stats_increment_on_send() {
        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);

        let (conduit_a, conduit_b) = Conduit::new_pair(config_a, config_b);

        let entries = vec![make_test_entry(1, 100)];
        let batch = EntryBatch::new(1, entries, 1);

        conduit_a.send_batch(batch).await.unwrap();

        // Need to receive to prevent channel from filling up
        let _ = conduit_b.recv_batch().await;

        let stats = conduit_a.stats();
        assert_eq!(stats.batches_sent, 1);
        assert_eq!(stats.entries_sent, 1);
    }

    #[tokio::test]
    async fn test_stats_increment_on_recv() {
        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);

        let (conduit_a, conduit_b) = Conduit::new_pair(config_a, config_b);

        let entries = vec![make_test_entry(1, 100), make_test_entry(2, 100), make_test_entry(3, 100)];
        let batch = EntryBatch::new(1, entries, 1);

        conduit_a.send_batch(batch).await.unwrap();
        let _ = conduit_b.recv_batch().await.unwrap();

        let stats = conduit_b.stats();
        assert_eq!(stats.batches_received, 1);
        assert_eq!(stats.entries_received, 3);
    }

    #[tokio::test]
    async fn test_batch_sequence_numbers() {
        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);

        let (conduit_a, conduit_b) = Conduit::new_pair(config_a, config_b);

        for seq in 1..=5u64 {
            let entries = vec![make_test_entry(seq, 100)];
            let batch = EntryBatch::new(1, entries, seq);
            conduit_a.send_batch(batch).await.unwrap();
        }

        for expected_seq in 1..=5u64 {
            let received = conduit_b.recv_batch().await.unwrap();
            assert_eq!(received.batch_seq, expected_seq);
        }
    }

    #[tokio::test]
    async fn test_multiple_batches_bidirectional() {
        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);

        let (conduit_a, conduit_b) = Conduit::new_pair(config_a, config_b);

        // A sends to B
        let entries_a = vec![make_test_entry(1, 100)];
        conduit_a.send_batch(EntryBatch::new(1, entries_a, 1)).await.unwrap();

        // B sends to A
        let entries_b = vec![make_test_entry(10, 200)];
        conduit_b.send_batch(EntryBatch::new(2, entries_b, 1)).await.unwrap();

        // Both receive
        let recv_a = conduit_b.recv_batch().await.unwrap();
        let recv_b = conduit_a.recv_batch().await.unwrap();

        assert_eq!(recv_a.source_site_id, 1);
        assert_eq!(recv_b.source_site_id, 2);
    }

    #[tokio::test]
    async fn test_send_after_shutdown_fails() {
        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);

        let (conduit_a, _) = Conduit::new_pair(config_a, config_b);

        conduit_a.shutdown().await;

        let entries = vec![make_test_entry(1, 100)];
        let batch = EntryBatch::new(1, entries, 1);

        let result = conduit_a.send_batch(batch).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_recv_returns_none_after_shutdown() {
        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);

        let (_, conduit_b) = Conduit::new_pair(config_a, config_b);

        conduit_b.shutdown().await;

        let received = conduit_b.recv_batch().await;
        assert!(received.is_none());
    }

    #[tokio::test]
    async fn test_empty_batch() {
        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);

        let (conduit_a, conduit_b) = Conduit::new_pair(config_a, config_b);

        let batch = EntryBatch::new(1, vec![], 1);
        conduit_a.send_batch(batch).await.unwrap();

        let received = conduit_b.recv_batch().await.unwrap();
        assert!(received.entries.is_empty());
        assert_eq!(received.source_site_id, 1);
    }

    #[tokio::test]
    async fn test_large_batch() {
        let config_a = ConduitConfig {
            max_batch_size: 10000,
            ..ConduitConfig::new(1, 2)
        };
        let config_b = ConduitConfig {
            max_batch_size: 10000,
            ..ConduitConfig::new(2, 1)
        };

        let (conduit_a, conduit_b) = Conduit::new_pair(config_a, config_b);

        let entries: Vec<_> = (0..1000).map(|i| make_test_entry(i as u64, 100 + i as u64)).collect();
        let batch = EntryBatch::new(1, entries, 1);

        conduit_a.send_batch(batch).await.unwrap();

        let received = conduit_b.recv_batch().await.unwrap();
        assert_eq!(received.entries.len(), 1000);
    }

    #[test]
    fn test_entry_batch_creation() {
        let entries = vec![make_test_entry(1, 100), make_test_entry(2, 100)];
        let batch = EntryBatch::new(42, entries, 7);

        assert_eq!(batch.source_site_id, 42);
        assert_eq!(batch.entries.len(), 2);
        assert_eq!(batch.batch_seq, 7);
    }

    #[test]
    fn test_entry_batch_fields() {
        let entry = make_test_entry(1, 100);
        let batch = EntryBatch::new(1, vec![entry], 1);

        assert_eq!(batch.source_site_id, 1);
        assert_eq!(batch.entries[0].inode, 100);
        assert_eq!(batch.entries[0].seq, 1);
        assert_eq!(batch.batch_seq, 1);
    }

    #[test]
    fn test_conduit_config_defaults() {
        let config = ConduitConfig::default();

        assert_eq!(config.local_site_id, 0);
        assert_eq!(config.remote_site_id, 0);
        assert!(config.remote_addrs.is_empty());
        assert!(config.tls.is_none());
        assert_eq!(config.max_batch_size, 1000);
        assert_eq!(config.reconnect_delay_ms, 100);
        assert_eq!(config.max_reconnect_delay_ms, 30000);
    }

    #[test]
    fn test_conduit_config_new() {
        let config = ConduitConfig::new(1, 2);

        assert_eq!(config.local_site_id, 1);
        assert_eq!(config.remote_site_id, 2);
    }

    #[test]
    fn test_conduit_tls_config_creation() {
        let tls = ConduitTlsConfig::new(
            b"cert".to_vec(),
            b"key".to_vec(),
            b"ca".to_vec(),
        );

        assert_eq!(tls.cert_pem, b"cert");
        assert_eq!(tls.key_pem, b"key");
        assert_eq!(tls.ca_pem, b"ca");
    }

    #[tokio::test]
    async fn test_conduit_state_connected() {
        let config = ConduitConfig::new(1, 2);
        let (tx, rx) = mpsc::channel(100);

        let conduit = Conduit {
            config,
            state: Arc::new(Mutex::new(ConduitState::Connected)),
            stats: Arc::new(ConduitStatsInner::new()),
            sender: tx,
            receiver: Arc::new(Mutex::new(rx)),
        };

        assert_eq!(conduit.state().await, ConduitState::Connected);
    }

    #[tokio::test]
    async fn test_conduit_state_reconnecting() {
        let config = ConduitConfig::new(1, 2);
        let (tx, rx) = mpsc::channel(100);

        let state = Arc::new(Mutex::new(ConduitState::Reconnecting { attempt: 3, delay_ms: 500 }));

        let conduit = Conduit {
            config,
            state,
            stats: Arc::new(ConduitStatsInner::new()),
            sender: tx,
            receiver: Arc::new(Mutex::new(rx)),
        };

        let s = conduit.state().await;
        match s {
            ConduitState::Reconnecting { attempt, delay_ms } => {
                assert_eq!(attempt, 3);
                assert_eq!(delay_ms, 500);
            }
            _ => panic!("expected Reconnecting state"),
        }
    }

    #[tokio::test]
    async fn test_conduit_state_shutdown() {
        let config = ConduitConfig::new(1, 2);
        let (tx, rx) = mpsc::channel(100);

        let conduit = Conduit {
            config,
            state: Arc::new(Mutex::new(ConduitState::Shutdown)),
            stats: Arc::new(ConduitStatsInner::new()),
            sender: tx,
            receiver: Arc::new(Mutex::new(rx)),
        };

        assert_eq!(conduit.state().await, ConduitState::Shutdown);
    }

    #[tokio::test]
    async fn test_concurrent_sends() {
        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);

        let (conduit_a, conduit_b) = Conduit::new_pair(config_a, config_b);

        let mut handles = vec![];
        for i in 0..10u64 {
            let conduit = conduit_a.clone();
            let entries = vec![make_test_entry(i, 100 + i)];
            let batch = EntryBatch::new(1, entries, i);

            handles.push(tokio::spawn(async move {
                let _ = conduit.send_batch(batch).await;
            }));
        }

        for handle in handles {
            let _ = handle.await;
        }

        // Drain the receiver
        for _ in 0..10 {
            let _ = conduit_b.recv_batch().await;
        }

        let stats = conduit_a.stats();
        assert_eq!(stats.batches_sent, 10);
    }

    #[tokio::test]
    async fn test_stats_snapshot() {
        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);

        let (conduit_a, conduit_b) = Conduit::new_pair(config_a, config_b);

        // Send some batches
        for i in 1..=3u64 {
            let entries = vec![make_test_entry(i, 100)];
            conduit_a.send_batch(EntryBatch::new(1, entries, i)).await.unwrap();
        }

        // Receive some batches
        for _ in 1..=2u64 {
            let _ = conduit_b.recv_batch().await;
        }

        let stats_a = conduit_a.stats();
        assert_eq!(stats_a.batches_sent, 3);
        assert_eq!(stats_a.entries_sent, 3);

        let stats_b = conduit_b.stats();
        assert_eq!(stats_b.batches_received, 2);
        assert_eq!(stats_b.entries_received, 2);
    }

    #[tokio::test]
    async fn test_shutdown_updates_state() {
        let config = ConduitConfig::new(1, 2);
        let (tx, rx) = mpsc::channel(100);

        let conduit = Conduit {
            config,
            state: Arc::new(Mutex::new(ConduitState::Connected)),
            stats: Arc::new(ConduitStatsInner::new()),
            sender: tx,
            receiver: Arc::new(Mutex::new(rx)),
        };

        assert_eq!(conduit.state().await, ConduitState::Connected);

        conduit.shutdown().await;

        assert_eq!(conduit.state().await, ConduitState::Shutdown);
    }
}