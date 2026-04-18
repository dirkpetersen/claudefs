//! Cross-site quota configuration replication with eventual consistency.
//!
//! This module handles quota limit synchronization between sites with conflict
//! detection and resolution using Lamport clocks for ordering.

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};

use dashmap::DashMap;
use thiserror::Error;
use uuid::Uuid;

pub use crate::quota_tracker::{QuotaType, TenantQuota};
use crate::tenant::TenantId;

#[derive(Error, Debug)]
pub enum QuotaReplicationError {
    #[error("invalid quota request: {0}")]
    InvalidRequest(String),

    #[error("quota not found: tenant={tenant_id}, type={quota_type}")]
    QuotaNotFound { tenant_id: String, quota_type: String },

    #[error("replication timeout after {0}ms")]
    ReplicationTimeout(u64),

    #[error("soft limit must be <= hard limit")]
    InvalidLimits,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplicationStatus {
    Pending,
    Applied,
    Conflict,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolutionStrategy {
    MaxWins,
    TimestampWins,
    AdminReview,
}

#[derive(Clone, Debug)]
pub struct QuotaReplicationRequest {
    pub request_id: String,
    pub tenant_id: String,
    pub quota_type: QuotaType,
    pub soft_limit: u64,
    pub hard_limit: u64,
    pub timestamp: u64,
    pub generation: u64,
    pub source_site: String,
}

#[derive(Clone, Debug)]
pub struct QuotaReplicationAck {
    pub request_id: String,
    pub status: ReplicationStatus,
    pub destination_site: String,
    pub applied_at: u64,
}

#[derive(Clone, Debug)]
pub struct QuotaReplicationConflict {
    pub tenant_id: String,
    pub quota_type: QuotaType,
    pub site_a_limit: u64,
    pub site_b_limit: u64,
    pub resolution_strategy: ResolutionStrategy,
    pub detected_at: u64,
}

#[derive(Clone, Default, Debug)]
pub struct QuotaReplicationMetrics {
    pub requests_sent: u64,
    pub requests_received: u64,
    pub acks_received: u64,
    pub conflicts_detected: u64,
    pub replication_lag_ms: u64,
    pub pending_requests: usize,
}

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn replicate_quota_config(
    request_id: &str,
    tenant_id: &str,
    quota_type: QuotaType,
    soft_limit: u64,
    hard_limit: u64,
    source_site: &str,
    pending_requests: &Arc<DashMap<String, QuotaReplicationRequest>>,
    generation: &Arc<AtomicU64>,
) -> QuotaReplicationRequest {
    let gen = generation.fetch_add(1, Ordering::SeqCst);
    
    let request = QuotaReplicationRequest {
        request_id: request_id.to_string(),
        tenant_id: tenant_id.to_string(),
        quota_type,
        soft_limit,
        hard_limit,
        timestamp: current_time_ms(),
        generation: gen,
        source_site: source_site.to_string(),
    };
    
    pending_requests.insert(request.request_id.clone(), request.clone());
    request
}

pub fn apply_remote_quota_update(
    request: &QuotaReplicationRequest,
    quota_tracker: &crate::quota_tracker::QuotaTracker,
    pending_requests: &Arc<DashMap<String, QuotaReplicationRequest>>,
    acks: &Arc<DashMap<String, QuotaReplicationAck>>,
    local_generation: &Arc<AtomicU64>,
    conflict_log: &Arc<DashMap<String, QuotaReplicationConflict>>,
    metrics: &Arc<Mutex<QuotaReplicationMetrics>>,
    local_site: &str,
) -> QuotaReplicationAck {
    if request.tenant_id.is_empty() {
        let ack = QuotaReplicationAck {
            request_id: request.request_id.clone(),
            status: ReplicationStatus::Failed("tenant_id cannot be empty".to_string()),
            destination_site: local_site.to_string(),
            applied_at: current_time_ms(),
        };
        acks.insert(request.request_id.clone(), ack.clone());
        return ack;
    }

    if request.soft_limit > request.hard_limit {
        let ack = QuotaReplicationAck {
            request_id: request.request_id.clone(),
            status: ReplicationStatus::Failed("soft_limit must be <= hard_limit".to_string()),
            destination_site: local_site.to_string(),
            applied_at: current_time_ms(),
        };
        acks.insert(request.request_id.clone(), ack.clone());
        return ack;
    }

    let local_gen = local_generation.load(Ordering::SeqCst);
    let mut is_conflict = false;

    if local_gen < request.generation {
    } else if local_gen > request.generation {
        is_conflict = true;
    } else {
        if local_gen == request.generation && request.generation != 0 {
            if let Some(local_quota) = quota_tracker.get_quota(&TenantId::new(&request.tenant_id)) {
                let local_limit = match request.quota_type {
                    QuotaType::Storage(_) => local_quota.storage_limit_bytes,
                    QuotaType::Iops(_) => local_quota.iops_limit,
                };
                if local_limit != request.hard_limit {
                    is_conflict = true;
                }
            }
        }
    }

    if is_conflict {
        let local_quota = quota_tracker.get_quota(&TenantId::new(&request.tenant_id));
        let site_a_limit = match &local_quota {
            Some(q) => match request.quota_type {
                QuotaType::Storage(_) => q.storage_limit_bytes,
                QuotaType::Iops(_) => q.iops_limit,
            },
            None => 0,
        };
        let site_b_limit = request.hard_limit;

        let conflict = QuotaReplicationConflict {
            tenant_id: request.tenant_id.clone(),
            quota_type: request.quota_type.clone(),
            site_a_limit,
            site_b_limit,
            resolution_strategy: ResolutionStrategy::MaxWins,
            detected_at: current_time_ms(),
        };

        let winning_limit = std::cmp::max(site_a_limit, site_b_limit);

        let tenant_id_owned = request.tenant_id.clone();
        match request.quota_type {
            QuotaType::Storage(_) => {
                let _ = quota_tracker.update_quota(TenantId::new(tenant_id_owned.clone()), winning_limit, 0);
            }
            QuotaType::Iops(_) => {
                let _ = quota_tracker.update_quota(TenantId::new(tenant_id_owned.clone()), 0, winning_limit);
            }
        }

        let conflict_key = format!("{}:{}:{}", request.tenant_id, format!("{:?}", request.quota_type), current_time_ms());
        conflict_log.insert(conflict_key, conflict.clone());

        {
            let mut m = metrics.lock().unwrap();
            m.conflicts_detected += 1;
        }

        pending_requests.remove(&request.request_id);

        let ack = QuotaReplicationAck {
            request_id: request.request_id.clone(),
            status: ReplicationStatus::Conflict,
            destination_site: local_site.to_string(),
            applied_at: current_time_ms(),
        };
        acks.insert(request.request_id.clone(), ack.clone());
        
        {
            let mut m = metrics.lock().unwrap();
            m.acks_received += 1;
        }
        
        return ack;
    }

    let tenant_id_owned = request.tenant_id.clone();
    match request.quota_type {
        QuotaType::Storage(_) => {
            let _ = quota_tracker.update_quota(TenantId::new(tenant_id_owned.clone()), request.hard_limit, 0);
        }
        QuotaType::Iops(_) => {
            let _ = quota_tracker.update_quota(TenantId::new(tenant_id_owned.clone()), 0, request.hard_limit);
        }
    }

    pending_requests.remove(&request.request_id);

    let ack = QuotaReplicationAck {
        request_id: request.request_id.clone(),
        status: ReplicationStatus::Applied,
        destination_site: local_site.to_string(),
        applied_at: current_time_ms(),
    };
    acks.insert(request.request_id.clone(), ack.clone());

    {
        let mut m = metrics.lock().unwrap();
        m.acks_received += 1;
    }

    ack
}

pub fn handle_quota_conflict(
    conflict: &QuotaReplicationConflict,
    strategy: ResolutionStrategy,
    quota_tracker: &crate::quota_tracker::QuotaTracker,
    audit_log: &Arc<DashMap<String, String>>,
) -> u64 {
    let winning_limit = match strategy {
        ResolutionStrategy::MaxWins => std::cmp::max(conflict.site_a_limit, conflict.site_b_limit),
        ResolutionStrategy::TimestampWins => {
            if conflict.detected_at > 0 {
                std::cmp::min(conflict.site_a_limit, conflict.site_b_limit)
            } else {
                std::cmp::max(conflict.site_a_limit, conflict.site_b_limit)
            }
        }
        ResolutionStrategy::AdminReview => {
            let audit_entry = format!(
                "{}:{:?}:{}:resolved:admin_review_required",
                conflict.tenant_id,
                conflict.quota_type,
                conflict.detected_at
            );
            audit_log.insert(conflict.detected_at.to_string(), audit_entry);
            return 0;
        }
    };

    if winning_limit > 0 {
        let tenant_id_owned = conflict.tenant_id.clone();
        match conflict.quota_type {
            QuotaType::Storage(_) => {
                let _ = quota_tracker.update_quota(TenantId::new(tenant_id_owned.clone()), winning_limit, 0);
            }
            QuotaType::Iops(_) => {
                let _ = quota_tracker.update_quota(TenantId::new(tenant_id_owned.clone()), 0, winning_limit);
            }
        }
    }

    let audit_entry = format!(
        "{}:{:?}:{}:resolved:{:?}",
        conflict.tenant_id,
        conflict.quota_type,
        conflict.detected_at,
        strategy
    );
    audit_log.insert(conflict.detected_at.to_string(), audit_entry);

    winning_limit
}

pub async fn sync_quota_state(
    quota_tracker: &crate::quota_tracker::QuotaTracker,
    pending_requests: &Arc<DashMap<String, QuotaReplicationRequest>>,
    acks: &Arc<DashMap<String, QuotaReplicationAck>>,
    source_site: &str,
    destination_site: &str,
    local_generation: &Arc<AtomicU64>,
) -> Result<(usize, usize), String> {
    let mut total_synced = 0;
    let mut total_failed = 0;
    let mut request_ids = Vec::new();

    for (tenant_id, quota) in quota_tracker.iter_quotas() {

        let storage_req = QuotaReplicationRequest {
            request_id: Uuid::new_v4().to_string(),
            tenant_id: tenant_id.to_string(),
            quota_type: QuotaType::Storage(quota.storage_limit_bytes),
            soft_limit: (quota.storage_limit_bytes as f64 * 0.8) as u64,
            hard_limit: quota.storage_limit_bytes,
            timestamp: current_time_ms(),
            generation: 0,
            source_site: source_site.to_string(),
        };
        
        let request_id = storage_req.request_id.clone();
        pending_requests.insert(request_id.clone(), storage_req);
        request_ids.push(request_id);
        
        local_generation.fetch_add(1, Ordering::SeqCst);

        let iops_req = QuotaReplicationRequest {
            request_id: Uuid::new_v4().to_string(),
            tenant_id: tenant_id.to_string(),
            quota_type: QuotaType::Iops(quota.iops_limit),
            soft_limit: (quota.iops_limit as f64 * 0.8) as u64,
            hard_limit: quota.iops_limit,
            timestamp: current_time_ms(),
            generation: 0,
            source_site: source_site.to_string(),
        };
        
        let request_id = iops_req.request_id.clone();
        pending_requests.insert(request_id.clone(), iops_req);
        request_ids.push(request_id);
        
        local_generation.fetch_add(1, Ordering::SeqCst);
    }

    let timeout_duration = std::time::Duration::from_secs(30);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout_duration {
        let mut all_done = true;
        for rid in &request_ids {
            if !acks.contains_key(rid) {
                all_done = false;
                break;
            }
        }

        if all_done {
            break;
        }

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    for rid in &request_ids {
        if let Some(ack) = acks.get(rid) {
            match &ack.status {
                ReplicationStatus::Applied | ReplicationStatus::Conflict => {
                    total_synced += 1;
                }
                ReplicationStatus::Failed(_) => {
                    total_failed += 1;
                }
                ReplicationStatus::Pending => {
                    total_failed += 1;
                }
            }
        } else {
            total_failed += 1;
        }
    }

    Ok((total_synced, total_failed))
}

pub fn get_replication_metrics(
    pending_requests: &Arc<DashMap<String, QuotaReplicationRequest>>,
    acks: &Arc<DashMap<String, QuotaReplicationAck>>,
    metrics: &Arc<Mutex<QuotaReplicationMetrics>>,
) -> QuotaReplicationMetrics {
    let pending_count = pending_requests.len();
    
    let replication_lag_ms = if pending_count > 0 {
        let oldest = pending_requests.iter()
            .min_by_key(|r| r.timestamp)
            .map(|r| r.timestamp)
            .unwrap_or(0);
        if oldest > 0 {
            current_time_ms().saturating_sub(oldest)
        } else {
            0
        }
    } else {
        0
    };

    let mut m = metrics.lock().unwrap();
    m.pending_requests = pending_count;
    m.replication_lag_ms = replication_lag_ms;

    m.clone()
}

pub fn batch_replicate_quotas(
    requests: Vec<QuotaReplicationRequest>,
    pending_requests: &Arc<DashMap<String, QuotaReplicationRequest>>,
    metrics: &Arc<Mutex<QuotaReplicationMetrics>>,
) -> usize {
    let mut count = 0;
    
    {
        let mut m = metrics.lock().unwrap();
        for req in &requests {
            pending_requests.insert(req.request_id.clone(), req.clone());
            m.requests_sent += 1;
            count += 1;
        }
    }

    count
}

pub fn clear_acked_requests(
    pending_requests: &Arc<DashMap<String, QuotaReplicationRequest>>,
    acks: &Arc<DashMap<String, QuotaReplicationAck>>,
) -> usize {
    let mut removed = 0;
    
    let to_remove: Vec<String> = pending_requests.iter()
        .filter_map(|entry| {
            let rid = entry.key().clone();
            if let Some(ack) = acks.get(&rid) {
                match &ack.status {
                    ReplicationStatus::Applied | ReplicationStatus::Conflict => {
                        return Some(rid);
                    }
                    _ => None,
                }
            } else {
                None
            }
        })
        .collect();

    for rid in to_remove {
        pending_requests.remove(&rid);
        removed += 1;
    }

    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_tracker() -> crate::quota_tracker::QuotaTracker {
        crate::quota_tracker::QuotaTracker::new(crate::quota_tracker::QuotaTrackerConfig::default())
    }

    #[test]
    fn test_replicate_storage_limit() {
        let pending = Arc::new(DashMap::new());
        let generation = Arc::new(AtomicU64::new(1));
        
        let req = replicate_quota_config(
            "req-001",
            "tenant-a",
            QuotaType::Storage(0),
            80_000,
            100_000,
            "site-a",
            &pending,
            &generation,
        );
        
        assert_eq!(req.tenant_id, "tenant-a");
        assert!(matches!(req.quota_type, QuotaType::Storage(_)));
        assert_eq!(req.hard_limit, 100_000);
        assert!(pending.contains_key("req-001"));
    }

    #[test]
    fn test_replicate_iops_limit() {
        let pending = Arc::new(DashMap::new());
        let generation = Arc::new(AtomicU64::new(1));
        
        let req = replicate_quota_config(
            "req-002",
            "tenant-b",
            QuotaType::Iops(0),
            800,
            1000,
            "site-b",
            &pending,
            &generation,
        );
        
        assert_eq!(req.tenant_id, "tenant-b");
        assert!(matches!(req.quota_type, QuotaType::Iops(_)));
        assert_eq!(req.hard_limit, 1000);
    }

    #[test]
    fn test_replicate_batch_updates() {
        let pending = Arc::new(DashMap::new());
        let generation = Arc::new(AtomicU64::new(1));
        
        let requests = vec![
            replicate_quota_config("req-1", "t1", QuotaType::Storage(0), 80, 100, "site-a", &pending, &generation),
            replicate_quota_config("req-2", "t2", QuotaType::Storage(0), 160, 200, "site-a", &pending, &generation),
            replicate_quota_config("req-3", "t3", QuotaType::Iops(0), 800, 1000, "site-a", &pending, &generation),
        ];
        
        assert_eq!(requests.len(), 3);
        assert_eq!(pending.len(), 3);
    }

    #[test]
    fn test_replication_ordering_by_generation() {
        let pending = Arc::new(DashMap::new());
        let generation = Arc::new(AtomicU64::new(0));
        
        let req1 = replicate_quota_config("req-1", "t1", QuotaType::Storage(0), 80, 100, "site-a", &pending, &generation);
        let req2 = replicate_quota_config("req-2", "t1", QuotaType::Storage(0), 160, 200, "site-a", &pending, &generation);
        
        assert!(req2.generation > req1.generation);
    }

    #[test]
    fn test_apply_remote_update_success() {
        let tracker = create_test_tracker();
        tracker.add_quota("tenant-a".into(), 1000, 1000).unwrap();
        
        let pending = Arc::new(DashMap::new());
        let acks = Arc::new(DashMap::new());
        let local_gen = Arc::new(AtomicU64::new(0));
        let conflict_log = Arc::new(DashMap::new());
        let metrics = Arc::new(Mutex::new(QuotaReplicationMetrics::default()));
        
        let request = QuotaReplicationRequest {
            request_id: "req-001".to_string(),
            tenant_id: "tenant-a".to_string(),
            quota_type: QuotaType::Storage(0),
            soft_limit: 800,
            hard_limit: 2000,
            timestamp: current_time_ms(),
            generation: 10,
            source_site: "site-a".to_string(),
        };
        
        let ack = apply_remote_quota_update(
            &request,
            &tracker,
            &pending,
            &acks,
            &local_gen,
            &conflict_log,
            &metrics,
            "site-b",
        );
        
        assert!(matches!(ack.status, ReplicationStatus::Applied));
        assert_eq!(ack.destination_site, "site-b");
        
        let updated = tracker.get_quota(&"tenant-a".into()).unwrap();
        assert_eq!(updated.storage_limit_bytes, 2000);
    }

    #[test]
    fn test_apply_remote_update_validation() {
        let tracker = create_test_tracker();
        tracker.add_quota("tenant-a".into(), 1000, 1000).unwrap();
        
        let pending = Arc::new(DashMap::new());
        let acks = Arc::new(DashMap::new());
        let local_gen = Arc::new(AtomicU64::new(0));
        let conflict_log = Arc::new(DashMap::new());
        let metrics = Arc::new(Mutex::new(QuotaReplicationMetrics::default()));
        
        let request = QuotaReplicationRequest {
            request_id: "req-002".to_string(),
            tenant_id: "tenant-a".to_string(),
            quota_type: QuotaType::Storage(0),
            soft_limit: 2000,
            hard_limit: 1000,
            timestamp: current_time_ms(),
            generation: 10,
            source_site: "site-a".to_string(),
        };
        
        let ack = apply_remote_quota_update(
            &request,
            &tracker,
            &pending,
            &acks,
            &local_gen,
            &conflict_log,
            &metrics,
            "site-b",
        );
        
        assert!(matches!(ack.status, ReplicationStatus::Failed(_)));
    }

    #[test]
    fn test_conflict_max_wins_strategy() {
        let tracker = create_test_tracker();
        tracker.add_quota("tenant-a".into(), 90_000_000_000, 10000).unwrap();
        
        let conflict = QuotaReplicationConflict {
            tenant_id: "tenant-a".to_string(),
            quota_type: QuotaType::Storage(0),
            site_a_limit: 90_000_000_000,
            site_b_limit: 100_000_000_000,
            resolution_strategy: ResolutionStrategy::MaxWins,
            detected_at: current_time_ms(),
        };
        
        let audit_log = Arc::new(DashMap::new());
        let result = handle_quota_conflict(&conflict, ResolutionStrategy::MaxWins, &tracker, &audit_log);
        
        assert_eq!(result, 100_000_000_000);
    }

    #[test]
    fn test_conflict_timestamp_wins_strategy() {
        let tracker = create_test_tracker();
        
        let conflict = QuotaReplicationConflict {
            tenant_id: "tenant-a".to_string(),
            quota_type: QuotaType::Storage(0),
            site_a_limit: 90_000_000_000,
            site_b_limit: 100_000_000_000,
            resolution_strategy: ResolutionStrategy::TimestampWins,
            detected_at: 1000,
        };
        
        let audit_log = Arc::new(DashMap::new());
        let result = handle_quota_conflict(&conflict, ResolutionStrategy::TimestampWins, &tracker, &audit_log);
        
        assert!(result > 0);
    }

    #[test]
    fn test_conflict_admin_review_strategy() {
        let tracker = create_test_tracker();
        
        let conflict = QuotaReplicationConflict {
            tenant_id: "tenant-a".to_string(),
            quota_type: QuotaType::Storage(0),
            site_a_limit: 90_000_000_000,
            site_b_limit: 100_000_000_000,
            resolution_strategy: ResolutionStrategy::AdminReview,
            detected_at: current_time_ms(),
        };
        
        let audit_log = Arc::new(DashMap::new());
        let result = handle_quota_conflict(&conflict, ResolutionStrategy::AdminReview, &tracker, &audit_log);
        
        assert_eq!(result, 0);
        assert!(!audit_log.is_empty());
    }

    #[tokio::test]
    async fn test_sync_after_recovery() {
        let tracker = create_test_tracker();
        tracker.add_quota("tenant-a".into(), 1000, 100).unwrap();
        tracker.add_quota("tenant-b".into(), 2000, 200).unwrap();
        
        let pending = Arc::new(DashMap::new());
        let acks = Arc::new(DashMap::new());
        let generation = Arc::new(AtomicU64::new(0));
        
        let (synced, failed) = sync_quota_state(
            &tracker,
            &pending,
            &acks,
            "site-a",
            "site-b",
            &generation,
        ).await.unwrap();
        
        assert_eq!(pending.len(), 4);
    }

    #[tokio::test]
    async fn test_sync_partial_failure() {
        let tracker = create_test_tracker();
        
        for i in 0..5 {
            let tid = format!("tenant-{}", i);
            tracker.add_quota(tid.into(), 1000, 100).unwrap();
        }
        
        let pending = Arc::new(DashMap::new());
        let acks = Arc::new(DashMap::new());
        let generation = Arc::new(AtomicU64::new(0));
        
        let (synced, failed) = sync_quota_state(
            &tracker,
            &pending,
            &acks,
            "site-a",
            "site-b",
            &generation,
        ).await.unwrap();
        
        assert_eq!(synced, 0);
        assert_eq!(failed, 10);
    }

    #[test]
    fn test_replication_idempotency() {
        let tracker = create_test_tracker();
        tracker.add_quota("tenant-a".into(), 1000, 1000).unwrap();
        
        let pending = Arc::new(DashMap::new());
        let acks = Arc::new(DashMap::new());
        let local_gen = Arc::new(AtomicU64::new(0));
        let conflict_log = Arc::new(DashMap::new());
        let metrics = Arc::new(Mutex::new(QuotaReplicationMetrics::default()));
        
        let request = QuotaReplicationRequest {
            request_id: "req-001".to_string(),
            tenant_id: "tenant-a".to_string(),
            quota_type: QuotaType::Storage(0),
            soft_limit: 800,
            hard_limit: 2000,
            timestamp: current_time_ms(),
            generation: 10,
            source_site: "site-a".to_string(),
        };
        
        let _ = apply_remote_quota_update(&request, &tracker, &pending, &acks, &local_gen, &conflict_log, &metrics, "site-b");
        
        let quota1 = tracker.get_quota(&"tenant-a".into()).unwrap();
        
        let _ = apply_remote_quota_update(&request, &tracker, &pending, &acks, &local_gen, &conflict_log, &metrics, "site-b");
        
        let quota2 = tracker.get_quota(&"tenant-a".into()).unwrap();
        assert_eq!(quota1.storage_limit_bytes, quota2.storage_limit_bytes);
    }

    #[test]
    fn test_replication_lag_measurement() {
        let pending = Arc::new(DashMap::new());
        let acks = Arc::new(DashMap::new());
        let metrics = Arc::new(Mutex::new(QuotaReplicationMetrics::default()));
        
        let req = QuotaReplicationRequest {
            request_id: "req-001".to_string(),
            tenant_id: "tenant-a".to_string(),
            quota_type: QuotaType::Storage(0),
            soft_limit: 800,
            hard_limit: 1000,
            timestamp: current_time_ms() - 5000,
            generation: 1,
            source_site: "site-a".to_string(),
        };
        pending.insert(req.request_id.clone(), req);
        
        let result = get_replication_metrics(&pending, &acks, &metrics);
        
        assert!(result.pending_requests >= 1);
        assert!(result.replication_lag_ms >= 5000);
    }

    #[test]
    fn test_pending_requests_cleanup() {
        let pending = Arc::new(DashMap::new());
        let acks = Arc::new(DashMap::new());
        
        pending.insert("req-001".to_string(), QuotaReplicationRequest {
            request_id: "req-001".to_string(),
            tenant_id: "t1".to_string(),
            quota_type: QuotaType::Storage(0),
            soft_limit: 80,
            hard_limit: 100,
            timestamp: 0,
            generation: 1,
            source_site: "site-a".to_string(),
        });
        
        acks.insert("req-001".to_string(), QuotaReplicationAck {
            request_id: "req-001".to_string(),
            status: ReplicationStatus::Applied,
            destination_site: "site-b".to_string(),
            applied_at: current_time_ms(),
        });
        
        let removed = clear_acked_requests(&pending, &acks);
        
        assert_eq!(removed, 1);
        assert!(pending.is_empty());
    }

    #[test]
    fn test_concurrent_updates_same_tenant() {
        let tracker = create_test_tracker();
        tracker.add_quota("tenant-a".to_string(), 1000, 1000).unwrap();
        
        let pending = Arc::new(DashMap::new());
        let acks = Arc::new(DashMap::new());
        let local_gen = Arc::new(AtomicU64::new(0));
        let conflict_log = Arc::new(DashMap::new());
        let metrics = Arc::new(Mutex::new(QuotaReplicationMetrics::default()));
        
        for i in 0..5 {
            let request = QuotaReplicationRequest {
                request_id: format!("req-{}", i),
                tenant_id: "tenant-a".to_string(),
                quota_type: QuotaType::Storage(0),
                soft_limit: 800,
                hard_limit: 1000 + (i as u64 * 100),
                timestamp: current_time_ms(),
                generation: 10 + i as u64,
                source_site: "site-a".to_string(),
            };
            
            let _ = apply_remote_quota_update(&request, &tracker, &pending, &acks, &local_gen, &conflict_log, &metrics, "site-b");
        }
        
        let quota = tracker.get_quota(&"tenant-a".into()).unwrap();
        assert_eq!(quota.storage_limit_bytes, 1400);
    }

    #[test]
    fn test_generation_counter_increment() {
        let pending = Arc::new(DashMap::new());
        let generation = Arc::new(AtomicU64::new(0));
        
        let req1 = replicate_quota_config("req-1", "t1", QuotaType::Storage(0), 80, 100, "site-a", &pending, &generation);
        let req2 = replicate_quota_config("req-2", "t1", QuotaType::Storage(0), 80, 100, "site-a", &pending, &generation);
        let req3 = replicate_quota_config("req-3", "t1", QuotaType::Storage(0), 80, 100, "site-a", &pending, &generation);
        
        assert_eq!(req2.generation, req1.generation + 1);
        assert_eq!(req3.generation, req2.generation + 1);
    }

    #[test]
    fn test_metrics_accuracy() {
        let pending = Arc::new(DashMap::new());
        let acks = Arc::new(DashMap::new());
        let metrics = Arc::new(Mutex::new(QuotaReplicationMetrics::default()));
        
        let requests = vec![
            QuotaReplicationRequest {
                request_id: "req-1".to_string(),
                tenant_id: "t1".to_string(),
                quota_type: QuotaType::Storage(0),
                soft_limit: 80,
                hard_limit: 100,
                timestamp: 0,
                generation: 1,
                source_site: "site-a".to_string(),
            },
            QuotaReplicationRequest {
                request_id: "req-2".to_string(),
                tenant_id: "t2".to_string(),
                quota_type: QuotaType::Iops(0),
                soft_limit: 800,
                hard_limit: 1000,
                timestamp: 0,
                generation: 2,
                source_site: "site-a".to_string(),
            },
        ];
        
        let inserted = batch_replicate_quotas(requests, &pending, &metrics);
        assert_eq!(inserted, 2);
        
        acks.insert("req-1".to_string(), QuotaReplicationAck {
            request_id: "req-1".to_string(),
            status: ReplicationStatus::Applied,
            destination_site: "site-b".to_string(),
            applied_at: current_time_ms(),
        });
        
        let m = get_replication_metrics(&pending, &acks, &metrics);
        assert_eq!(m.requests_sent, 2);
    }

    #[test]
    fn test_replication_with_quota_tracker() {
        let tracker = create_test_tracker();
        tracker.add_quota("tenant-a".into(), 1000, 100).unwrap();
        
        let pending = Arc::new(DashMap::new());
        let acks = Arc::new(DashMap::new());
        let local_gen = Arc::new(AtomicU64::new(5));
        let conflict_log = Arc::new(DashMap::new());
        let metrics = Arc::new(Mutex::new(QuotaReplicationMetrics::default()));
        
        let request = QuotaReplicationRequest {
            request_id: "req-001".to_string(),
            tenant_id: "tenant-a".to_string(),
            quota_type: QuotaType::Storage(0),
            soft_limit: 800,
            hard_limit: 2000,
            timestamp: current_time_ms(),
            generation: 10,
            source_site: "site-a".to_string(),
        };
        
        let ack = apply_remote_quota_update(&request, &tracker, &pending, &acks, &local_gen, &conflict_log, &metrics, "site-b");
        
        assert!(matches!(ack.status, ReplicationStatus::Applied));
    }

    #[test]
    fn test_large_batch_100_tenants() {
        let pending = Arc::new(DashMap::new());
        let metrics = Arc::new(Mutex::new(QuotaReplicationMetrics::default()));
        
        let mut requests = Vec::with_capacity(100);
        for i in 0..100 {
            requests.push(QuotaReplicationRequest {
                request_id: format!("req-{}", i),
                tenant_id: format!("tenant-{}", i),
                quota_type: if i % 2 == 0 { QuotaType::Storage(0) } else { QuotaType::Iops(0) },
                soft_limit: 800,
                hard_limit: 1000,
                timestamp: 0,
                generation: i as u64,
                source_site: "site-a".to_string(),
            });
        }
        
        let inserted = batch_replicate_quotas(requests, &pending, &metrics);
        
        assert_eq!(inserted, 100);
        assert_eq!(pending.len(), 100);
        
        let m = metrics.lock().unwrap();
        assert_eq!(m.requests_sent, 100);
    }

    #[test]
    fn test_replication_conflict_audit_trail() {
        let tracker = create_test_tracker();
        
        let conflict = QuotaReplicationConflict {
            tenant_id: "tenant-a".to_string(),
            quota_type: QuotaType::Storage(0),
            site_a_limit: 90_000_000_000,
            site_b_limit: 100_000_000_000,
            resolution_strategy: ResolutionStrategy::MaxWins,
            detected_at: 1234567890000,
        };
        
        let audit_log = Arc::new(DashMap::new());
        let _ = handle_quota_conflict(&conflict, ResolutionStrategy::MaxWins, &tracker, &audit_log);
        
        assert!(!audit_log.is_empty());
        
        let audit_entry = audit_log.iter().next().unwrap();
        assert!(audit_entry.value().contains("tenant-a"));
        assert!(audit_entry.value().contains("Storage"));
    }
}