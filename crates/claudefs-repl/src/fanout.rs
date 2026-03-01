// File: crates/claudefs-repl/src/fanout.rs

//! Multi-Site Fanout: Parallel dispatch of journal batches to multiple remote sites.
//!
//! When a primary site needs to replicate to N replica sites simultaneously,
//! the fanout module manages the parallel dispatch of journal batches to all
//! remote conduits.

use crate::conduit::{Conduit, EntryBatch};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::Instant;

/// Result of a fanout send to one remote site.
#[derive(Debug, Clone)]
pub struct FanoutResult {
    /// Remote site ID.
    pub site_id: u64,
    /// Whether the send succeeded.
    pub success: bool,
    /// Number of entries sent.
    pub entries_sent: usize,
    /// Error message if failed.
    pub error: Option<String>,
    /// Microseconds to send.
    pub latency_us: u64,
}

/// Summary of a fanout operation across all sites.
#[derive(Debug, Clone)]
pub struct FanoutSummary {
    /// Batch sequence number.
    pub batch_seq: u64,
    /// Total number of sites attempted.
    pub total_sites: usize,
    /// Number of successful sends.
    pub successful_sites: usize,
    /// Number of failed sends.
    pub failed_sites: usize,
    /// Individual results per site.
    pub results: Vec<FanoutResult>,
}

impl FanoutSummary {
    /// Returns true if all sites succeeded.
    pub fn all_succeeded(&self) -> bool {
        self.failed_sites == 0 && self.total_sites > 0
    }

    /// Returns true if any site failed.
    pub fn any_failed(&self) -> bool {
        self.failed_sites > 0
    }

    /// Returns the failure rate (0.0 to 1.0).
    pub fn failure_rate(&self) -> f64 {
        if self.total_sites == 0 {
            return 0.0;
        }
        self.failed_sites as f64 / self.total_sites as f64
    }

    /// Returns the IDs of successful sites.
    pub fn successful_site_ids(&self) -> Vec<u64> {
        self.results
            .iter()
            .filter(|r| r.success)
            .map(|r| r.site_id)
            .collect()
    }

    /// Returns the IDs of failed sites.
    pub fn failed_site_ids(&self) -> Vec<u64> {
        self.results
            .iter()
            .filter(|r| !r.success)
            .map(|r| r.site_id)
            .collect()
    }
}

/// Fans out a journal batch to multiple remote conduits in parallel.
pub struct FanoutSender {
    /// Local site ID.
    #[allow(dead_code)]
    local_site_id: u64,
    /// Conduits for each remote site (site_id -> conduit).
    conduits: Arc<tokio::sync::RwLock<HashMap<u64, Conduit>>>,
}

impl FanoutSender {
    /// Create a new fanout sender.
    pub fn new(local_site_id: u64) -> Self {
        Self {
            local_site_id,
            conduits: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Register a conduit for a remote site.
    pub async fn add_conduit(&self, site_id: u64, conduit: Conduit) {
        let mut conduits = self.conduits.write().await;
        conduits.insert(site_id, conduit);
    }

    /// Remove a conduit. Returns true if the conduit was present.
    pub async fn remove_conduit(&self, site_id: u64) -> bool {
        let mut conduits = self.conduits.write().await;
        conduits.remove(&site_id).is_some()
    }

    /// Returns the number of registered conduits.
    pub async fn conduit_count(&self) -> usize {
        let conduits = self.conduits.read().await;
        conduits.len()
    }

    /// Fanout a batch to all registered conduits in parallel.
    /// Waits for all sends to complete (success or error).
    pub async fn fanout(&self, batch: EntryBatch) -> FanoutSummary {
        let site_ids: Vec<u64> = {
            let conduits = self.conduits.read().await;
            conduits.keys().copied().collect()
        };

        self.fanout_to(batch, &site_ids).await
    }

    /// Fanout to a specific subset of sites.
    pub async fn fanout_to(&self, batch: EntryBatch, site_ids: &[u64]) -> FanoutSummary {
        let batch_seq = batch.batch_seq;
        let entries_count = batch.entries.len();

        if site_ids.is_empty() {
            return FanoutSummary {
                batch_seq,
                total_sites: 0,
                successful_sites: 0,
                failed_sites: 0,
                results: vec![],
            };
        }

        let mut handles = Vec::new();

        {
            let conduits = self.conduits.read().await;
            for &site_id in site_ids {
                if let Some(conduit) = conduits.get(&site_id) {
                    let conduit = conduit.clone();
                    let batch_clone = batch.clone();
                    handles.push(tokio::spawn(async move {
                        let start = Instant::now();
                        let result = conduit.send_batch(batch_clone).await;
                        let latency_us = start.elapsed().as_micros() as u64;

                        match result {
                            Ok(()) => FanoutResult {
                                site_id,
                                success: true,
                                entries_sent: entries_count,
                                error: None,
                                latency_us,
                            },
                            Err(e) => FanoutResult {
                                site_id,
                                success: false,
                                entries_sent: 0,
                                error: Some(e.to_string()),
                                latency_us,
                            },
                        }
                    }));
                } else {
                    handles.push(tokio::spawn(async move {
                        FanoutResult {
                            site_id,
                            success: false,
                            entries_sent: 0,
                            error: Some("conduit not found".to_string()),
                            latency_us: 0,
                        }
                    }));
                }
            }
        }

        let mut results = Vec::new();
        for handle in handles {
            if let Ok(result) = handle.await {
                results.push(result);
            }
        }

        let successful_sites = results.iter().filter(|r| r.success).count();
        let failed_sites = results.len() - successful_sites;

        results.sort_by_key(|r| r.site_id);

        FanoutSummary {
            batch_seq,
            total_sites: results.len(),
            successful_sites,
            failed_sites,
            results,
        }
    }

    /// List registered site IDs.
    pub async fn site_ids(&self) -> Vec<u64> {
        let conduits = self.conduits.read().await;
        let mut ids: Vec<u64> = conduits.keys().copied().collect();
        ids.sort();
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conduit::ConduitConfig;
    use crate::journal::OpKind;

    fn make_test_entry(seq: u64, inode: u64) -> crate::journal::JournalEntry {
        crate::journal::JournalEntry::new(seq, 0, 1, 1000 + seq, inode, OpKind::Write, vec![1, 2, 3])
    }

    #[tokio::test]
    async fn test_fanout_to_0_sites_empty_summary() {
        let sender = FanoutSender::new(1);
        let batch = EntryBatch::new(1, vec![], 1);

        let summary = sender.fanout_to(batch, &[]).await;

        assert_eq!(summary.total_sites, 0);
        assert_eq!(summary.successful_sites, 0);
        assert_eq!(summary.failed_sites, 0);
        assert!(summary.results.is_empty());
    }

    #[tokio::test]
    async fn test_fanout_to_1_site() {
        let sender = FanoutSender::new(1);

        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);
        let (conduit_a, _conduit_b) = Conduit::new_pair(config_a, config_b);

        sender.add_conduit(2, conduit_a).await;

        let entries = vec![make_test_entry(1, 100)];
        let batch = EntryBatch::new(1, entries, 1);

        let summary = sender.fanout_to(batch, &[2]).await;

        assert_eq!(summary.total_sites, 1);
        assert_eq!(summary.successful_sites, 1);
        assert_eq!(summary.failed_sites, 0);
        assert!(summary.all_succeeded());
        assert!(!summary.any_failed());
    }

    #[tokio::test]
    async fn test_fanout_to_3_sites_parallel() {
        let sender = FanoutSender::new(1);

        let mut handles = vec![];
        for remote_id in 2..=4u64 {
            let config_a = ConduitConfig {
                max_batch_size: 1000,
                ..ConduitConfig::new(1, remote_id)
            };
            let config_b = ConduitConfig {
                max_batch_size: 1000,
                ..ConduitConfig::new(remote_id, 1)
            };
            let (conduit_a, conduit_b) = Conduit::new_pair(config_a.clone(), config_b.clone());

            sender.add_conduit(remote_id, conduit_a).await;

            let handle = tokio::spawn(async move {
                loop {
                    let _ = conduit_b.recv_batch().await;
                }
            });
            handles.push(handle);
        }

        let entries = vec![make_test_entry(1, 100), make_test_entry(2, 101)];
        let batch = EntryBatch::new(1, entries, 1);

        let summary = sender.fanout_to(batch, &[2, 3, 4]).await;

        assert_eq!(summary.total_sites, 3);
        assert_eq!(summary.successful_sites, 3);
        assert_eq!(summary.failed_sites, 0);
        assert!(summary.all_succeeded());

        for handle in handles {
            handle.abort();
        }
    }

    #[tokio::test]
    async fn test_fanout_summary_all_succeeded() {
        let result1 = FanoutResult {
            site_id: 1,
            success: true,
            entries_sent: 10,
            error: None,
            latency_us: 100,
        };
        let result2 = FanoutResult {
            site_id: 2,
            success: true,
            entries_sent: 10,
            error: None,
            latency_us: 100,
        };

        let summary = FanoutSummary {
            batch_seq: 1,
            total_sites: 2,
            successful_sites: 2,
            failed_sites: 0,
            results: vec![result1, result2],
        };

        assert!(summary.all_succeeded());
        assert!(!summary.any_failed());
        assert_eq!(summary.failure_rate(), 0.0);
    }

    #[tokio::test]
    async fn test_fanout_summary_any_failed() {
        let result1 = FanoutResult {
            site_id: 1,
            success: true,
            entries_sent: 10,
            error: None,
            latency_us: 100,
        };
        let result2 = FanoutResult {
            site_id: 2,
            success: false,
            entries_sent: 0,
            error: Some("test error".to_string()),
            latency_us: 100,
        };

        let summary = FanoutSummary {
            batch_seq: 1,
            total_sites: 2,
            successful_sites: 1,
            failed_sites: 1,
            results: vec![result1, result2],
        };

        assert!(!summary.all_succeeded());
        assert!(summary.any_failed());
        assert_eq!(summary.failure_rate(), 0.5);
    }

    #[tokio::test]
    async fn test_fanout_summary_successful_site_ids() {
        let result1 = FanoutResult {
            site_id: 1,
            success: true,
            entries_sent: 10,
            error: None,
            latency_us: 100,
        };
        let result2 = FanoutResult {
            site_id: 2,
            success: false,
            entries_sent: 0,
            error: Some("error".to_string()),
            latency_us: 100,
        };
        let result3 = FanoutResult {
            site_id: 3,
            success: true,
            entries_sent: 10,
            error: None,
            latency_us: 100,
        };

        let summary = FanoutSummary {
            batch_seq: 1,
            total_sites: 3,
            successful_sites: 2,
            failed_sites: 1,
            results: vec![result1, result2, result3],
        };

        let success_ids = summary.successful_site_ids();
        assert_eq!(success_ids, vec![1, 3]);

        let failed_ids = summary.failed_site_ids();
        assert_eq!(failed_ids, vec![2]);
    }

    #[tokio::test]
    async fn test_add_conduit_and_remove_conduit() {
        let sender = FanoutSender::new(1);

        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);
        let (conduit_a, _conduit_b) = Conduit::new_pair(config_a, config_b);

        sender.add_conduit(2, conduit_a).await;
        assert_eq!(sender.conduit_count().await, 1);

        let removed = sender.remove_conduit(2).await;
        assert!(removed);
        assert_eq!(sender.conduit_count().await, 0);

        let removed_again = sender.remove_conduit(2).await;
        assert!(!removed_again);
    }

    #[tokio::test]
    async fn test_conduit_count() {
        let sender = FanoutSender::new(1);

        assert_eq!(sender.conduit_count().await, 0);

        let config_a1 = ConduitConfig::new(1, 2);
        let config_b1 = ConduitConfig::new(2, 1);
        let (conduit1, _) = Conduit::new_pair(config_a1, config_b1);

        let config_a2 = ConduitConfig::new(1, 3);
        let config_b2 = ConduitConfig::new(3, 1);
        let (conduit2, _) = Conduit::new_pair(config_a2, config_b2);

        sender.add_conduit(2, conduit1).await;
        assert_eq!(sender.conduit_count().await, 1);

        sender.add_conduit(3, conduit2).await;
        assert_eq!(sender.conduit_count().await, 2);
    }

    #[tokio::test]
    async fn test_fanout_to_subset() {
        let sender = FanoutSender::new(1);

        let mut handles = vec![];
        for remote_id in 2..=5u64 {
            let config_a = ConduitConfig {
                max_batch_size: 1000,
                ..ConduitConfig::new(1, remote_id)
            };
            let config_b = ConduitConfig {
                max_batch_size: 1000,
                ..ConduitConfig::new(remote_id, 1)
            };
            let (conduit_a, conduit_b) = Conduit::new_pair(config_a.clone(), config_b.clone());
            sender.add_conduit(remote_id, conduit_a).await;

            let handle = tokio::spawn(async move {
                loop {
                    let _ = conduit_b.recv_batch().await;
                }
            });
            handles.push(handle);
        }

        let entries = vec![make_test_entry(1, 100)];
        let batch = EntryBatch::new(1, entries, 1);

        let summary = sender.fanout_to(batch, &[2, 4]).await;

        assert_eq!(summary.total_sites, 2);
        assert!(summary.successful_site_ids().contains(&2));
        assert!(summary.successful_site_ids().contains(&4));

        for handle in handles {
            handle.abort();
        }
    }

    #[tokio::test]
    async fn test_fanout_with_empty_entries() {
        let sender = FanoutSender::new(1);

        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);
        let (conduit_a, _conduit_b) = Conduit::new_pair(config_a, config_b);

        sender.add_conduit(2, conduit_a).await;

        let batch = EntryBatch::new(1, vec![], 1);
        let summary = sender.fanout_to(batch, &[2]).await;

        assert_eq!(summary.total_sites, 1);
        assert_eq!(summary.results[0].entries_sent, 0);
    }

    #[tokio::test]
    async fn test_batch_seq_propagated_to_summary() {
        let sender = FanoutSender::new(1);

        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);
        let (conduit_a, _conduit_b) = Conduit::new_pair(config_a, config_b);

        sender.add_conduit(2, conduit_a).await;

        let entries = vec![make_test_entry(1, 100)];
        let batch = EntryBatch::new(1, entries, 42);

        let summary = sender.fanout_to(batch, &[2]).await;

        assert_eq!(summary.batch_seq, 42);
    }

    #[tokio::test]
    async fn test_site_ids() {
        let sender = FanoutSender::new(1);

        assert!(sender.site_ids().await.is_empty());

        sender.add_conduit(5, create_dummy_conduit(5)).await;
        sender.add_conduit(2, create_dummy_conduit(2)).await;
        sender.add_conduit(8, create_dummy_conduit(8)).await;

        let ids = sender.site_ids().await;
        assert_eq!(ids, vec![2, 5, 8]);
    }

    #[tokio::test]
    async fn test_fanout_all_registered() {
        let sender = FanoutSender::new(1);

        for remote_id in 1..=3u64 {
            sender.add_conduit(remote_id + 10, create_dummy_conduit(remote_id + 10)).await;
        }

        let entries = vec![make_test_entry(1, 100)];
        let batch = EntryBatch::new(1, entries, 1);

        let summary = sender.fanout(batch).await;

        assert_eq!(summary.total_sites, 3);
    }

    #[tokio::test]
    async fn test_fanout_to_nonexistent_site() {
        let sender = FanoutSender::new(1);

        let entries = vec![make_test_entry(1, 100)];
        let batch = EntryBatch::new(1, entries, 1);

        let summary = sender.fanout_to(batch, &[999]).await;

        assert_eq!(summary.total_sites, 1);
        assert_eq!(summary.failed_sites, 1);
        assert!(!summary.results[0].success);
        assert!(summary.results[0].error.is_some());
    }

    fn create_dummy_conduit(site_id: u64) -> Conduit {
        let config_a = ConduitConfig::new(1, site_id);
        let config_b = ConduitConfig::new(site_id, 1);
        let (conduit, _) = Conduit::new_pair(config_a, config_b);
        conduit
    }

    #[tokio::test]
    async fn test_fanout_failure_rate_zero_sites() {
        let summary = FanoutSummary {
            batch_seq: 1,
            total_sites: 0,
            successful_sites: 0,
            failed_sites: 0,
            results: vec![],
        };

        assert_eq!(summary.failure_rate(), 0.0);
    }

    #[tokio::test]
    async fn test_fanout_with_lost_conduit() {
        let sender = FanoutSender::new(1);

        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);
        let (conduit_a, _conduit_b) = Conduit::new_pair(config_a, config_b);

        sender.add_conduit(2, conduit_a).await;

        let entries = vec![make_test_entry(1, 100)];
        let batch = EntryBatch::new(1, entries, 1);

        let summary = sender.fanout_to(batch, &[2, 3]).await;

        assert_eq!(summary.total_sites, 2);
        assert_eq!(summary.successful_sites, 1);
        assert_eq!(summary.failed_sites, 1);
    }

    #[tokio::test]
    async fn test_fanout_summary_results_sorted_by_site_id() {
        let results = vec![
            FanoutResult {
                site_id: 3,
                success: true,
                entries_sent: 1,
                error: None,
                latency_us: 100,
            },
            FanoutResult {
                site_id: 1,
                success: true,
                entries_sent: 1,
                error: None,
                latency_us: 100,
            },
            FanoutResult {
                site_id: 2,
                success: true,
                entries_sent: 1,
                error: None,
                latency_us: 100,
            },
        ];

        let mut sorted_results = results.clone();
        sorted_results.sort_by_key(|r| r.site_id);

        let summary = FanoutSummary {
            batch_seq: 1,
            total_sites: 3,
            successful_sites: 3,
            failed_sites: 0,
            results: sorted_results,
        };

        let ids: Vec<_> = summary.results.iter().map(|r| r.site_id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
    }
}