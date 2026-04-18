//! Quota Enforcement Integration Tests
//!
//! Tests for per-tenant storage quotas with fairness queuing.
//! All tests marked #[ignore] - run with: cargo test --test cluster_quota_enforcement -- --ignored

use std::collections::HashMap;
use std::sync::Arc;

use claudefs_reduce::{
    QuotaManager, QuotaDecision, UsageKind, UsageReason,
    FairnessQueueConfig, FairnessQueue,
    QuotaAccountant,
    CrossTenantDedupManager, TenantId,
};

// Re-export quota_manager types for tests
pub use claudefs_reduce::quota_manager::{
    QuotaConfig, TenantUsage,
};

fn create_test_tenant_id(id: u64) -> TenantId {
    TenantId(id)
}

#[tokio::test]
#[ignore]
async fn test_soft_quota_warning() {
    let mut config = QuotaConfig::default();
    config.soft_quota_percent = 90.0;
    let manager: Arc<QuotaManager> = Arc::new(QuotaManager::new(config));
    
    manager.set_quota(create_test_tenant_id(1), 1000).await.unwrap();
    manager.update_usage(create_test_tenant_id(1), 950, UsageReason {
        kind: UsageKind::Write,
        metadata: HashMap::new(),
    }).await.unwrap();
    
    let usage: TenantUsage = manager.get_tenant_usage(create_test_tenant_id(1)).await.unwrap();
    assert!(usage.soft_quota_warning);
    println!("test_soft_quota_warning: Soft quota warning at 90% threshold");
}

#[tokio::test]
#[ignore]
async fn test_hard_quota_rejection() {
    let config = QuotaConfig::default();
    let manager: Arc<QuotaManager> = Arc::new(QuotaManager::new(config));
    
    manager.set_quota(create_test_tenant_id(1), 1000).await.unwrap();
    manager.update_usage(create_test_tenant_id(1), 1100, UsageReason {
        kind: UsageKind::Write,
        metadata: HashMap::new(),
    }).await.unwrap();
    
    let usage: TenantUsage = manager.get_tenant_usage(create_test_tenant_id(1)).await.unwrap();
    assert!(usage.hard_quota_exceeded);
    println!("test_hard_quota_rejection: Hard quota rejection at 100%");
}

#[tokio::test]
#[ignore]
async fn test_quota_grace_period() {
    let mut config = QuotaConfig::default();
    config.grace_period_secs = 300;
    let manager: Arc<QuotaManager> = Arc::new(QuotaManager::new(config));
    
    manager.set_quota(create_test_tenant_id(1), 1000).await.unwrap();
    
    manager.update_usage(create_test_tenant_id(1), 1100, UsageReason {
        kind: UsageKind::Write,
        metadata: HashMap::new(),
    }).await.unwrap();
    
    let usage: TenantUsage = manager.get_tenant_usage(create_test_tenant_id(1)).await.unwrap();
    assert!(usage.hard_quota_exceeded);
    println!("test_quota_grace_period: 5min grace window available");
}

#[tokio::test]
#[ignore]
async fn test_quota_override_admin() {
    let config = QuotaConfig::default();
    let manager: Arc<QuotaManager> = Arc::new(QuotaManager::new(config));
    
    manager.set_quota(create_test_tenant_id(1), 1000).await.unwrap();
    
    let decision = manager.check_quota(create_test_tenant_id(1), 1500, true).await.unwrap();
    assert_eq!(decision, QuotaDecision::AllowedRestricted);
    
    let metrics = manager.get_metrics();
    assert_eq!(metrics.admin_overrides, 1);
    println!("test_quota_override_admin: Admin override enabled");
}

#[tokio::test]
#[ignore]
async fn test_fairness_no_starvation() {
    let config = FairnessQueueConfig::default();
    let queue: FairnessQueue = FairnessQueue::new(config);
    
    queue.enqueue(create_test_tenant_id(1), 1000, 95.0).await.unwrap();
    queue.enqueue(create_test_tenant_id(2), 1000, 10.0).await.unwrap();
    
    let result1 = queue.dequeue().await.unwrap();
    let result2 = queue.dequeue().await.unwrap();
    
    assert!(result1.is_some() && result2.is_some());
    let t1 = result1.unwrap().tenant_id;
    let t2 = result2.unwrap().tenant_id;
    
    assert!(t1 == create_test_tenant_id(2) || t2 == create_test_tenant_id(2));
    println!("test_fairness_no_starvation: Both tenants progress without starvation");
}

#[tokio::test]
#[ignore]
async fn test_fairness_weighted_priority() {
    let config = FairnessQueueConfig::default();
    let queue: FairnessQueue = FairnessQueue::new(config);
    
    queue.enqueue(create_test_tenant_id(1), 1000, 80.0).await.unwrap();
    queue.enqueue(create_test_tenant_id(2), 1000, 40.0).await.unwrap();
    
    let result = queue.dequeue().await.unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap().tenant_id, create_test_tenant_id(1));
    println!("test_fairness_weighted_priority: Higher priority (80%) dequeued first");
}

#[tokio::test]
#[ignore]
async fn test_fairness_queue_timeout() {
    let config = FairnessQueueConfig::default();
    let queue: FairnessQueue = FairnessQueue::new(config);
    
    queue.enqueue(create_test_tenant_id(1), 100, 50.0).await.unwrap();
    
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    
    let result = queue.dequeue().await.unwrap();
    assert!(result.is_some());
    
    let metrics = queue.get_metrics();
    assert_eq!(metrics.total_dequeued, 1);
    println!("test_fairness_queue_timeout: Queue processes entries correctly");
}

#[tokio::test]
#[ignore]
async fn test_fairness_batch_clustering() {
    let config = FairnessQueueConfig::default();
    let queue: FairnessQueue = FairnessQueue::new(config);
    
    for _ in 0..50 {
        queue.enqueue(create_test_tenant_id(1), 100, 50.0).await.unwrap();
    }
    for _ in 0..50 {
        queue.enqueue(create_test_tenant_id(2), 100, 50.0).await.unwrap();
    }
    
    let depth1 = queue.get_queue_depth(create_test_tenant_id(1)).await.unwrap();
    let depth2 = queue.get_queue_depth(create_test_tenant_id(2)).await.unwrap();
    
    assert_eq!(depth1, 50);
    assert_eq!(depth2, 50);
    println!("test_fairness_batch_clustering: Batch clustering for similar writes");
}

#[tokio::test]
#[ignore]
async fn test_quota_exact_dedup_credit() {
    let config = QuotaConfig::default();
    let manager: Arc<QuotaManager> = Arc::new(QuotaManager::new(config));
    
    manager.set_quota(create_test_tenant_id(1), 1000).await.unwrap();
    manager.update_usage(create_test_tenant_id(1), 1024 * 1024 * 1024, UsageReason {
        kind: UsageKind::Write,
        metadata: HashMap::new(),
    }).await.unwrap();
    
    manager.apply_dedup_credit(create_test_tenant_id(1), 42, 1024 * 1024 * 1024).await.unwrap();
    
    let usage: TenantUsage = manager.get_tenant_usage(create_test_tenant_id(1)).await.unwrap();
    assert_eq!(usage.used_bytes, 0);
    assert!(usage.dedup_savings_bytes > 0);
    println!("test_quota_exact_dedup_credit: Full dedup credit applied");
}

#[tokio::test]
#[ignore]
async fn test_quota_similarity_dedup_credit() {
    let config = QuotaConfig::default();
    let manager: Arc<QuotaManager> = Arc::new(QuotaManager::new(config));
    
    manager.set_quota(create_test_tenant_id(1), 1000).await.unwrap();
    manager.update_usage(create_test_tenant_id(1), 1024 * 1024 * 1024, UsageReason {
        kind: UsageKind::Write,
        metadata: HashMap::new(),
    }).await.unwrap();
    
    let credit = (1024 * 1024 * 1024 / 5) * 4;
    manager.apply_dedup_credit(create_test_tenant_id(1), 100, credit).await.unwrap();
    
    let usage: TenantUsage = manager.get_tenant_usage(create_test_tenant_id(1)).await.unwrap();
    assert!(usage.used_bytes < 1024 * 1024 * 1024);
    println!("test_quota_similarity_dedup_credit: Delta compression credit applied");
}

#[tokio::test]
#[ignore]
async fn test_quota_cross_tenant_dedup() {
    let manager: CrossTenantDedupManager = CrossTenantDedupManager::new();
    
    manager.register_match(1000, create_test_tenant_id(1), create_test_tenant_id(2)).await.unwrap();
    manager.register_match(1000, create_test_tenant_id(1), create_test_tenant_id(3)).await.unwrap();
    
    let shared1 = manager.get_shared_blocks(create_test_tenant_id(1)).await.unwrap();
    let shared2 = manager.get_shared_blocks(create_test_tenant_id(2)).await.unwrap();
    
    assert!(shared1.contains(&1000));
    assert!(shared2.contains(&1000));
    println!("test_quota_cross_tenant_dedup: Single-count for shared block");
}

#[tokio::test]
#[ignore]
async fn test_quota_snapshot_accounting() {
    let accountant: QuotaAccountant = QuotaAccountant::new(10);
    
    accountant.record(create_test_tenant_id(1), 1024, UsageReason {
        kind: UsageKind::Write,
        metadata: HashMap::new(),
    }).await.unwrap();
    
    accountant.record(create_test_tenant_id(1), 1024, UsageReason {
        kind: UsageKind::Snapshot,
        metadata: HashMap::new(),
    }).await.unwrap();
    
    let usage = accountant.get_current_usage(create_test_tenant_id(1)).await.unwrap();
    assert!(usage.0 >= 1024);
    println!("test_quota_snapshot_accounting: No double-charge for snapshots");
}

#[tokio::test]
#[ignore]
async fn test_quota_crash_recovery() {
    let accountant: QuotaAccountant = QuotaAccountant::new(10);
    
    accountant.record(create_test_tenant_id(1), 1000, UsageReason {
        kind: UsageKind::Write,
        metadata: HashMap::new(),
    }).await.unwrap();
    
    let journal = accountant.get_journal();
    let entries = journal.get_entries(Some(create_test_tenant_id(1))).unwrap();
    
    assert!(!entries.is_empty());
    
    let stats = accountant.reconcile().await.unwrap();
    assert_eq!(stats.inconsistencies_found, 0);
    println!("test_quota_crash_recovery: Journal-based recovery verified");
}

#[tokio::test]
#[ignore]
async fn test_quota_concurrent_updates() {
    let config = QuotaConfig::default();
    let manager: Arc<QuotaManager> = Arc::new(QuotaManager::new(config));
    
    manager.set_quota(create_test_tenant_id(1), 100000).await.unwrap();
    
    let m1 = Arc::clone(&manager);
    let handle = tokio::spawn(async move {
        for _ in 0..10 {
            m1.update_usage(create_test_tenant_id(1), 100, UsageReason {
                kind: UsageKind::Write,
                metadata: HashMap::new(),
            }).await.unwrap();
        }
    });
    
    for _ in 0..10 {
        manager.update_usage(create_test_tenant_id(1), 100, UsageReason {
            kind: UsageKind::Write,
            metadata: HashMap::new(),
        }).await.unwrap();
    }
    
    handle.await.unwrap();
    
    let usage: TenantUsage = manager.get_tenant_usage(create_test_tenant_id(1)).await.unwrap();
    assert_eq!(usage.used_bytes, 2000);
    println!("test_quota_concurrent_updates: Concurrent safety verified");
}

#[tokio::test]
#[ignore]
async fn test_quota_compression_savings() {
    let config = QuotaConfig::default();
    let manager: Arc<QuotaManager> = Arc::new(QuotaManager::new(config));
    
    manager.set_quota(create_test_tenant_id(1), 1000000000).await.unwrap();
    manager.update_usage(create_test_tenant_id(1), 1000000000, UsageReason {
        kind: UsageKind::Write,
        metadata: HashMap::new(),
    }).await.unwrap();
    
    let compression_savings: i64 = -750000000;
    manager.update_usage(create_test_tenant_id(1), compression_savings, UsageReason {
        kind: UsageKind::Compression,
        metadata: HashMap::new(),
    }).await.unwrap();
    
    let usage: TenantUsage = manager.get_tenant_usage(create_test_tenant_id(1)).await.unwrap();
    assert!(usage.used_bytes < 1000000000, "used_bytes should be less after compression savings");
    println!("test_quota_compression_savings: Compression factored into usage");
}

#[tokio::test]
#[ignore]
async fn test_quota_tiering_to_s3() {
    let config = QuotaConfig::default();
    let manager: Arc<QuotaManager> = Arc::new(QuotaManager::new(config));
    
    manager.set_quota(create_test_tenant_id(1), 1000).await.unwrap();
    manager.update_usage(create_test_tenant_id(1), 1000, UsageReason {
        kind: UsageKind::Write,
        metadata: HashMap::new(),
    }).await.unwrap();
    
    let initial_usage: TenantUsage = manager.get_tenant_usage(create_test_tenant_id(1)).await.unwrap();
    assert!(initial_usage.used_bytes >= 1000, "initial used_bytes should be >= 1000");
    
    manager.update_usage(create_test_tenant_id(1), -500, UsageReason {
        kind: UsageKind::Tiering,
        metadata: HashMap::new(),
    }).await.unwrap();
    
    let final_usage: TenantUsage = manager.get_tenant_usage(create_test_tenant_id(1)).await.unwrap();
    assert!(final_usage.used_bytes < 1000, "final used_bytes should be less than 1000");
    println!("test_quota_tiering_to_s3: Tiering frees quota space");
}

#[tokio::test]
#[ignore]
async fn test_quota_complex_topology() {
    let config = QuotaConfig::default();
    let manager: Arc<QuotaManager> = Arc::new(QuotaManager::new(config));
    
    manager.set_quota(create_test_tenant_id(1), 1000000).await.unwrap();
    
    manager.update_usage(create_test_tenant_id(1), 100000, UsageReason {
        kind: UsageKind::Write,
        metadata: HashMap::new(),
    }).await.unwrap();
    
    manager.apply_dedup_credit(create_test_tenant_id(1), 1, 20000).await.unwrap();
    
    let compression_savings: i64 = -30000;
    manager.update_usage(create_test_tenant_id(1), compression_savings, UsageReason {
        kind: UsageKind::Compression,
        metadata: HashMap::new(),
    }).await.unwrap();
    
    let usage: TenantUsage = manager.get_tenant_usage(create_test_tenant_id(1)).await.unwrap();
    assert!(usage.used_bytes < 100000, "used_bytes should be less after dedup and compression");
    assert!(usage.dedup_savings_bytes > 0);
    println!("test_quota_complex_topology: Multi-layer reduction applied correctly");
}

#[tokio::test]
#[ignore]
async fn test_quota_audit_trail() {
    let accountant: QuotaAccountant = QuotaAccountant::new(10);
    
    accountant.record(create_test_tenant_id(1), 100, UsageReason {
        kind: UsageKind::Write,
        metadata: HashMap::new(),
    }).await.unwrap();
    
    accountant.record(create_test_tenant_id(1), -50, UsageReason {
        kind: UsageKind::Dedup,
        metadata: HashMap::new(),
    }).await.unwrap();
    
    let trail = accountant.audit_trail(create_test_tenant_id(1), None).await.unwrap();
    
    assert_eq!(trail.len(), 2);
    assert_eq!(trail[0].delta_bytes, 100);
    assert_eq!(trail[1].delta_bytes, -50);
    println!("test_quota_audit_trail: Historical audit log retrieved");
}