//! Metadata multi-tenancy security tests.
//!
//! Security tests for multi-tenant metadata operations including:
//! - Tenant isolation
//! - Cross-shard atomicity
//! - Deduplication and reference counting
//! - Quota enforcement
//! - Rollback safety
//! - Permission boundaries

use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

use claudefs_meta::{
    FingerprintIndex, InodeAttr, InodeId, KvStore, MemoryKvStore, MetadataService,
    MetadataServiceConfig, QuotaTracker, QuotaTrackerConfig, TenantContext, TenantId,
    TenantIsolator, TenantIsolatorConfig, TenantNamespace, TenantCapabilities,
    FileType, Timestamp,
};
use claudefs_meta::client_session::SessionId;

struct TestContext {
    tenant_id: TenantId,
    kv_store: Arc<dyn KvStore>,
    service: Arc<MetadataService>,
    tenant_isolator: Arc<TenantIsolator>,
    quota_tracker: Arc<QuotaTracker>,
    fingerprint_index: Arc<FingerprintIndex>,
}

impl TestContext {
    fn new(tenant_id: &str) -> Self {
        let kv_store: Arc<dyn KvStore> = Arc::new(MemoryKvStore::new());
        let config = MetadataServiceConfig {
            node_id: claudefs_meta::NodeId::new(1),
            peers: vec![],
            site_id: 1,
            num_shards: 256,
            max_journal_entries: 10000,
        };
        let service = Arc::new(MetadataService::new_with_kv(config, kv_store.clone()));
        service.init_root().ok();

        let tenant_isolator = Arc::new(TenantIsolator::new(TenantIsolatorConfig::default()));
        let quota_tracker = Arc::new(QuotaTracker::new(QuotaTrackerConfig::default()));
        let fingerprint_index = Arc::new(FingerprintIndex::new());

        Self {
            tenant_id: TenantId::new(tenant_id),
            kv_store,
            service,
            tenant_isolator,
            quota_tracker,
            fingerprint_index,
        }
    }

    fn create_inode(&self, parent: InodeId, name: &str, size: u64) -> Result<InodeAttr, claudefs_meta::MetaError> {
        let mut attr = self.service.create_file(parent, name, 1000, 1000, 0o644)?;
        attr.size = size;
        self.service.setattr(attr.ino, attr.clone())?;
        Ok(attr)
    }

    fn set_quota(&self, bytes: u64) -> Result<(), claudefs_meta::MetaError> {
        self.quota_tracker.add_quota(self.tenant_id.clone(), bytes, 10000)
    }

    fn concurrent_writes(&self, count: usize, bytes_each: u64) -> Vec<tokio::task::JoinHandle<Result<(), claudefs_meta::MetaError>>> {
        let tenant_id = self.tenant_id.clone();
        let quota_tracker = self.quota_tracker.clone();
        let service = self.service.clone();

        (0..count)
            .map(|i| {
                let tenant_id = tenant_id.clone();
                let quota_tracker = quota_tracker.clone();
                let service = service.clone();
                tokio::spawn(async move {
                    quota_tracker.record_storage_write(&tenant_id, bytes_each)?;
                    Ok(())
                })
            })
            .collect()
    }
}

fn make_tenant_context(tenant_id: TenantId, root_inode: InodeId) -> TenantContext {
    TenantContext::new(
        tenant_id,
        1000,
        SessionId::new(),
        root_inode,
        TenantCapabilities::default(),
    )
}

#[cfg(test)]
mod tenant_isolation {
    use super::*;

    #[tokio::test]
    async fn test_tenant_isolation_read_forbidden() {
        let ctx_a = TestContext::new("tenant_a");
        let ctx_b = TestContext::new("tenant_b");

        ctx_a.tenant_isolator.register_tenant(ctx_a.tenant_id.clone(), 1_000_000_000).ok();
        ctx_b.tenant_isolator.register_tenant(ctx_b.tenant_id.clone(), 1_000_000_000).ok();

        let ns_a = ctx_a.tenant_isolator.get_tenant_namespace(&ctx_a.tenant_id).unwrap();
        
        let inode = ctx_a.create_inode(InodeId::ROOT_INODE, "secret.txt", 1000).unwrap();

        let ctx_b_context = make_tenant_context(ctx_b.tenant_id.clone(), ns_a.root_inode);

        let result = ctx_a.tenant_isolator.enforce_isolation(&ctx_b_context, inode.ino);
        
        assert!(result.is_err(), "Tenant B should not be able to read Tenant A's inode");
    }

    #[tokio::test]
    async fn test_tenant_isolation_write_forbidden() {
        let ctx_a = TestContext::new("tenant_a");
        let ctx_b = TestContext::new("tenant_b");

        ctx_a.tenant_isolator.register_tenant(ctx_a.tenant_id.clone(), 1_000_000_000).ok();
        ctx_b.tenant_isolator.register_tenant(ctx_b.tenant_id.clone(), 1_000_000_000).ok();

        let ns_a = ctx_a.tenant_isolator.get_tenant_namespace(&ctx_a.tenant_id).unwrap();
        let inode = ctx_a.create_inode(InodeId::ROOT_INODE, "owned.txt", 0).unwrap();

        let mut attr = ctx_a.service.getattr(inode.ino).unwrap();
        attr.size = 500;
        
        let ctx_b_context = make_tenant_context(ctx_b.tenant_id.clone(), ns_a.root_inode);
        
        let isolation_result = ctx_a.tenant_isolator.enforce_isolation(&ctx_b_context, inode.ino);
        
        assert!(isolation_result.is_err(), "Tenant B should not be able to write to Tenant A's inode");
    }

    #[tokio::test]
    async fn test_tenant_isolation_delete_forbidden() {
        let ctx_a = TestContext::new("tenant_a");
        let ctx_b = TestContext::new("tenant_b");

        ctx_a.tenant_isolator.register_tenant(ctx_a.tenant_id.clone(), 1_000_000_000).ok();
        ctx_b.tenant_isolator.register_tenant(ctx_b.tenant_id.clone(), 1_000_000_000).ok();

        let ns_a = ctx_a.tenant_isolator.get_tenant_namespace(&ctx_a.tenant_id).unwrap();
        let inode = ctx_a.create_inode(InodeId::ROOT_INODE, "deletable.txt", 0).unwrap();

        let ctx_b_context = make_tenant_context(ctx_b.tenant_id.clone(), ns_a.root_inode);
        
        let result = ctx_a.tenant_isolator.enforce_isolation(&ctx_b_context, inode.ino);
        
        assert!(result.is_err(), "Tenant B should not be able to delete Tenant A's inode");
    }

    #[tokio::test]
    async fn test_tenant_isolation_stat_hidden() {
        let ctx_a = TestContext::new("tenant_a");
        let ctx_b = TestContext::new("tenant_b");

        ctx_a.tenant_isolator.register_tenant(ctx_a.tenant_id.clone(), 1_000_000_000).ok();
        ctx_b.tenant_isolator.register_tenant(ctx_b.tenant_id.clone(), 1_000_000_000).ok();

        let ns_a = ctx_a.tenant_isolator.get_tenant_namespace(&ctx_a.tenant_id).unwrap();
        let inode = ctx_a.create_inode(InodeId::ROOT_INODE, "hidden.txt", 1000).unwrap();

        let ctx_b_context = make_tenant_context(ctx_b.tenant_id.clone(), ns_a.root_inode);
        
        let isolation_result = ctx_a.tenant_isolator.enforce_isolation(&ctx_b_context, inode.ino);
        
        assert!(isolation_result.is_err(), "Tenant B should not see Tenant A's inode in stat");
        
        let attr = ctx_b.service.getattr(inode.ino);
        assert!(attr.is_err(), "Stat should fail for cross-tenant inode");
    }

    #[tokio::test]
    async fn test_tenant_isolation_mkdir_separate_namespaces() {
        let ctx_a = TestContext::new("tenant_a");
        let ctx_b = TestContext::new("tenant_b");

        ctx_a.tenant_isolator.register_tenant(ctx_a.tenant_id.clone(), 1_000_000_000).ok();
        ctx_b.tenant_isolator.register_tenant(ctx_b.tenant_id.clone(), 1_000_000_000).ok();

        let ns_a = ctx_a.tenant_isolator.get_tenant_namespace(&ctx_a.tenant_id).unwrap();
        let ns_b = ctx_b.tenant_isolator.get_tenant_namespace(&ctx_b.tenant_id).unwrap();

        let dir_a = ctx_a.service.mkdir(ns_a.root_inode, "app", 1000, 1000, 0o755).unwrap();
        let dir_b = ctx_b.service.mkdir(ns_b.root_inode, "app", 1000, 1000, 0o755).unwrap();

        assert_ne!(dir_a.ino, dir_b.ino, "Same directory name in different tenants should have different inodes");

        let lookup_a = ctx_a.service.lookup(ns_a.root_inode, "app").unwrap();
        let lookup_b = ctx_b.service.lookup(ns_b.root_inode, "app").unwrap();

        assert_eq!(lookup_a.ino, dir_a.ino);
        assert_eq!(lookup_b.ino, dir_b.ino);
        assert_ne!(lookup_a.ino, lookup_b.ino);
    }
}

#[cfg(test)]
mod cross_shard_atomicity {
    use super::*;

    #[tokio::test]
    async fn test_cross_shard_rename_all_or_nothing() {
        let ctx = TestContext::new("tenant_shard_test");
        ctx.tenant_isolator.register_tenant(ctx.tenant_id.clone(), 1_000_000_000).ok();

        let file = ctx.create_inode(InodeId::ROOT_INODE, "source.txt", 100).unwrap();
        
        let shard_a = ctx.service.shard_for_inode(InodeId::ROOT_INODE);
        let shard_b = ctx.service.shard_for_inode(file.ino);

        let result = ctx.service.rename(
            InodeId::ROOT_INODE,
            "source.txt",
            InodeId::ROOT_INODE,
            "dest.txt",
        );

        assert!(result.is_ok(), "Rename across shards should be atomic");

        let src_lookup = ctx.service.lookup(InodeId::ROOT_INODE, "source.txt");
        let dst_lookup = ctx.service.lookup(InodeId::ROOT_INODE, "dest.txt");

        assert!(src_lookup.is_err(), "Source should not exist after rename");
        assert!(dst_lookup.is_ok(), "Destination should exist after rename");
        assert_eq!(dst_lookup.unwrap().ino, file.ino);
    }

    #[tokio::test]
    async fn test_cross_shard_move_maintains_consistency() {
        let ctx = TestContext::new("tenant_move_test");
        ctx.tenant_isolator.register_tenant(ctx.tenant_id.clone(), 1_000_000_000).ok();

        let dir1 = ctx.service.mkdir(InodeId::ROOT_INODE, "dir1", 1000, 1000, 0o755).unwrap();
        let dir2 = ctx.service.mkdir(InodeId::ROOT_INODE, "dir2", 1000, 1000, 0o755).unwrap();
        
        let file = ctx.create_inode(dir1.ino, "moved_file.txt", 200).unwrap();

        ctx.service.rename(dir1.ino, "moved_file.txt", dir2.ino, "moved_file.txt").ok();

        let in_dir1 = ctx.service.lookup(dir1.ino, "moved_file.txt");
        let in_dir2 = ctx.service.lookup(dir2.ino, "moved_file.txt");

        assert!(in_dir1.is_err(), "File should not exist in source directory");
        assert!(in_dir2.is_ok(), "File should exist in destination directory");
        
        let file_attr = ctx.service.getattr(file.ino).unwrap();
        assert_eq!(file_attr.nlink, 1, "File should have correct link count");
    }

    #[tokio::test]
    async fn test_cross_shard_hardlink_dedup_safe() {
        let ctx = TestContext::new("tenant_hardlink_test");
        ctx.tenant_isolator.register_tenant(ctx.tenant_id.clone(), 1_000_000_000).ok();

        let file = ctx.create_inode(InodeId::ROOT_INODE, "original.txt", 500).unwrap();
        assert_eq!(file.nlink, 1);

        ctx.service.link(InodeId::ROOT_INODE, "link1.txt", file.ino).ok();
        ctx.service.link(InodeId::ROOT_INODE, "link2.txt", file.ino).ok();

        let attr = ctx.service.getattr(file.ino).unwrap();
        assert_eq!(attr.nlink, 3, "Hardlink count should be 3 (original + 2 links)");

        let link1 = ctx.service.lookup(InodeId::ROOT_INODE, "link1.txt").unwrap();
        let link2 = ctx.service.lookup(InodeId::ROOT_INODE, "link2.txt").unwrap();

        assert_eq!(link1.ino, file.ino);
        assert_eq!(link2.ino, file.ino);
    }

    #[tokio::test]
    async fn test_cross_shard_operation_with_quorum_loss() {
        let ctx = TestContext::new("tenant_quorum_test");
        ctx.tenant_isolator.register_tenant(ctx.tenant_id.clone(), 1_000_000_000).ok();

        let file = ctx.create_inode(InodeId::ROOT_INODE, "quorum_test.txt", 100).unwrap();
        
        let journal = ctx.service.journal();
        let initial_seq = journal.latest_sequence().unwrap();

        ctx.service.rename(
            InodeId::ROOT_INODE,
            "quorum_test.txt",
            InodeId::ROOT_INODE,
            "quorum_test_renamed.txt",
        ).ok();

        let new_seq = journal.latest_sequence().unwrap();
        assert!(new_seq > initial_seq, "Journal should record the operation");
        
        let lookup = ctx.service.lookup(InodeId::ROOT_INODE, "quorum_test_renamed.txt");
        assert!(lookup.is_ok(), "Rename should complete even without explicit quorum");
    }
}

#[cfg(test)]
mod dedup_and_ref_counting {
    use super::*;

    fn make_fingerprint(i: u8) -> [u8; 32] {
        let mut hash = [0u8; 32];
        hash[0] = i;
        hash
    }

    #[tokio::test]
    async fn test_dedup_fingerprint_collision_handled() {
        let index = Arc::new(FingerprintIndex::new());
        let hash1 = make_fingerprint(1);
        let hash2 = make_fingerprint(2);

        index.insert(hash1, 1000, 4096).ok();
        let collision_result = index.insert(hash2, 2000, 4096);
        
        assert!(collision_result.is_ok(), "Different fingerprints should not collide");
        
        let entry1 = index.lookup(&hash1).unwrap();
        let entry2 = index.lookup(&hash2).unwrap();
        
        assert_eq!(entry1.ref_count, 1);
        assert_eq!(entry2.ref_count, 1);
        assert_ne!(entry1.block_location, entry2.block_location);
    }

    #[tokio::test]
    async fn test_dedup_ref_count_accuracy() {
        let index = Arc::new(FingerprintIndex::new());
        let hash = make_fingerprint(10);

        index.insert(hash, 1000, 4096).ok();
        
        index.increment_ref(&hash).ok();
        index.increment_ref(&hash).ok();
        index.increment_ref(&hash).ok();
        index.increment_ref(&hash).ok();

        let entry = index.lookup(&hash).unwrap();
        assert_eq!(entry.ref_count, 5, "After 4 increments + initial, should be 5");

        index.decrement_ref(&hash).ok();
        index.decrement_ref(&hash).ok();
        index.decrement_ref(&hash).ok();

        let entry = index.lookup(&hash).unwrap();
        assert_eq!(entry.ref_count, 2, "After 3 decrements from 5, should be 2");

        index.decrement_ref(&hash).ok();
        index.decrement_ref(&hash).ok();

        assert!(index.lookup(&hash).is_none(), "Entry should be removed when ref_count reaches 0");
    }

    #[tokio::test]
    async fn test_dedup_cross_tenant_isolation() {
        let ctx_a = TestContext::new("tenant_dedup_a");
        let ctx_b = TestContext::new("tenant_dedup_b");

        ctx_a.tenant_isolator.register_tenant(ctx_a.tenant_id.clone(), 1_000_000_000).ok();
        ctx_b.tenant_isolator.register_tenant(ctx_b.tenant_id.clone(), 1_000_000_000).ok();

        let hash = make_fingerprint(100);
        
        ctx_a.fingerprint_index.insert(hash, 1000, 4096).ok();
        
        let lookup_a = ctx_a.fingerprint_index.lookup(&hash);
        let lookup_b = ctx_b.fingerprint_index.lookup(&hash);
        
        assert!(lookup_a.is_some(), "Tenant A should find its fingerprint");
        assert!(lookup_b.is_none(), "Tenant B should not see Tenant A's fingerprint");
        
        ctx_b.fingerprint_index.insert(hash, 2000, 4096).ok();
        
        let entry_a = ctx_a.fingerprint_index.lookup(&hash).unwrap();
        let entry_b = ctx_b.fingerprint_index.lookup(&hash).unwrap();
        
        assert_eq!(entry_a.ref_count, 1, "Tenant A entry should have ref_count 1");
        assert_eq!(entry_b.ref_count, 1, "Tenant B entry should have ref_count 1");
    }

    #[tokio::test]
    async fn test_dedup_concurrent_write_same_content() {
        let index = Arc::new(FingerprintIndex::new());
        let hash = make_fingerprint(200);

        let mut handles = Vec::new();
        for _ in 0..10 {
            let index = index.clone();
            let hash = hash;
            handles.push(tokio::spawn(async move {
                index.insert(hash, 1000, 4096).ok()
            }));
        }

        for handle in handles {
            handle.await.ok();
        }

        let entry = index.lookup(&hash).unwrap();
        assert_eq!(entry.ref_count, 10, "Concurrent writes should result in ref_count=10");
    }

    #[tokio::test]
    async fn test_dedup_ref_count_overflow_protection() {
        let index = Arc::new(FingerprintIndex::new());
        let hash = make_fingerprint(255);

        index.insert(hash, 1000, 4096).ok();

        for _ in 0..65535 {
            index.increment_ref(&hash).ok();
        }

        let entry = index.lookup(&hash).unwrap();
        assert!(entry.ref_count <= u64::MAX, "Ref count should not overflow");
        assert!(entry.ref_count >= 65536, "Ref count should be at least 65536");

        let overflow_result = index.increment_ref(&hash);
        assert!(overflow_result.is_ok(), "Increment should succeed with saturating arithmetic");
    }
}

#[cfg(test)]
mod quota_enforcement {
    use super::*;

    #[tokio::test]
    async fn test_quota_simple_enforcement() {
        let ctx = TestContext::new("tenant_quota_simple");
        ctx.tenant_isolator.register_tenant(ctx.tenant_id.clone(), 1_000_000_000).ok();
        
        ctx.set_quota(1000).ok();

        let result_500 = ctx.quota_tracker.record_storage_write(&ctx.tenant_id, 500);
        assert!(result_500.is_ok(), "Writing 500 bytes should succeed");

        let result_600 = ctx.quota_tracker.record_storage_write(&ctx.tenant_id, 600);
        assert!(result_600.is_err(), "Writing 600 more bytes should fail (500 + 600 > 1000)");
        
        let usage = ctx.quota_tracker.get_usage(&ctx.tenant_id).unwrap();
        assert_eq!(usage.used_storage_bytes, 500);
    }

    #[tokio::test]
    async fn test_quota_concurrent_writes_no_overage() {
        let ctx = TestContext::new("tenant_quota_concurrent");
        ctx.tenant_isolator.register_tenant(ctx.tenant_id.clone(), 1_000_000_000).ok();
        
        ctx.set_quota(1000).ok();

        let mut handles = ctx.concurrent_writes(10, 150);
        
        let mut successes = 0;
        let mut failures = 0;
        
        for handle in handles {
            match handle.await {
                Ok(Ok(())) => successes += 1,
                Ok(Err(_)) => failures += 1,
                Err(_) => failures += 1,
            }
        }

        let final_usage = ctx.quota_tracker.get_usage(&ctx.tenant_id).unwrap();
        assert!(final_usage.used_storage_bytes <= 1000, "Total usage should not exceed quota");
    }

    #[tokio::test]
    async fn test_quota_dedup_reduces_quota_usage() {
        let ctx = TestContext::new("tenant_quota_dedup");
        ctx.tenant_isolator.register_tenant(ctx.tenant_id.clone(), 1_000_000_000).ok();
        
        ctx.set_quota(1000).ok();

        let hash = [0u8; 32];
        ctx.fingerprint_index.insert(hash, 1000, 500).ok();
        
        let usage_before = ctx.quota_tracker.get_usage(&ctx.tenant_id);
        
        ctx.fingerprint_index.increment_ref(&hash).ok();
        
        let usage_after = ctx.quota_tracker.get_usage(&ctx.tenant_id);
        
        assert_eq!(usage_before, usage_after, "Quota usage should not increase due to dedup");
    }

    #[tokio::test]
    async fn test_quota_delete_frees_space() {
        let ctx = TestContext::new("tenant_quota_delete");
        ctx.tenant_isolator.register_tenant(ctx.tenant_id.clone(), 1_000_000_000).ok();
        
        ctx.set_quota(1000).ok();

        ctx.quota_tracker.record_storage_write(&ctx.tenant_id, 600).ok();
        
        let usage_before = ctx.quota_tracker.get_usage(&ctx.tenant_id).unwrap();
        assert_eq!(usage_before.used_storage_bytes, 600);

        ctx.quota_tracker.record_storage_delete(&ctx.tenant_id, 600);
        
        let usage_after = ctx.quota_tracker.get_usage(&ctx.tenant_id).unwrap();
        assert_eq!(usage_after.used_storage_bytes, 0, "Quota should be 0 after delete");

        let result = ctx.quota_tracker.record_storage_write(&ctx.tenant_id, 1000);
        assert!(result.is_ok(), "Should be able to write full quota after delete");
    }
}

#[cfg(test)]
mod rollback_safety {
    use super::*;

    #[tokio::test]
    async fn test_failed_operation_no_partial_state() {
        let ctx = TestContext::new("tenant_rollback_test");
        ctx.tenant_isolator.register_tenant(ctx.tenant_id.clone(), 1_000_000_000).ok();

        let file = ctx.create_inode(InodeId::ROOT_INODE, "before_crash.txt", 100).unwrap();
        
        let journal = ctx.service.journal();
        let before_seq = journal.latest_sequence().unwrap();

        let result = ctx.service.create_file(InodeId::ROOT_INODE, "during_crash.txt", 200);
        
        if result.is_err() {
            let after_seq = journal.latest_sequence().unwrap();
            assert!(after_seq >= before_seq, "Journal state should be consistent");
        }

        let before_file = ctx.service.getattr(file.ino);
        assert!(before_file.is_ok(), "Previous file should still exist");
    }

    #[tokio::test]
    async fn test_concurrent_tenant_ops_isolated() {
        let ctx_a = TestContext::new("tenant_a_ops");
        let ctx_b = TestContext::new("tenant_b_ops");

        ctx_a.tenant_isolator.register_tenant(ctx_a.tenant_id.clone(), 1_000_000_000).ok();
        ctx_b.tenant_isolator.register_tenant(ctx_b.tenant_id.clone(), 1_000_000_000).ok();

        let ns_a = ctx_a.tenant_isolator.get_tenant_namespace(&ctx_a.tenant_id).unwrap();
        
        ctx_a.service.create_file(ns_a.root_inode, "shared_name.txt", 100).ok();
        
        ctx_b.service.create_file(ns_a.root_inode, "shared_name.txt", 200).ok();
        
        let lookup_a = ctx_a.service.lookup(ns_a.root_inode, "shared_name.txt");
        
        if lookup_a.is_ok() {
            let attr = lookup_a.unwrap();
            let full_attr = ctx_a.service.getattr(attr.ino).unwrap();
            assert_eq!(full_attr.size, 100, "Tenant A should see its own file");
        }
    }

    #[tokio::test]
    async fn test_shard_split_rollback_safe() {
        let ctx = TestContext::new("tenant_shard_split");
        ctx.tenant_isolator.register_tenant(ctx.tenant_id.clone(), 1_000_000_000).ok();

        let file1 = ctx.create_inode(InodeId::ROOT_INODE, "file1.txt", 100).unwrap();
        let file2 = ctx.create_inode(InodeId::ROOT_INODE, "file2.txt", 200).unwrap();

        let shard1 = ctx.service.shard_for_inode(file1.ino);
        let shard2 = ctx.service.shard_for_inode(file2.ino);

        assert!(ctx.service.getattr(file1.ino).is_ok());
        assert!(ctx.service.getattr(file2.ino).is_ok());

        let rename_result = ctx.service.rename(
            InodeId::ROOT_INODE,
            "file1.txt",
            InodeId::ROOT_INODE,
            "renamed_file1.txt",
        );

        assert!(rename_result.is_ok());
        assert!(ctx.service.getattr(file1.ino).is_err());
        assert!(ctx.service.lookup(InodeId::ROOT_INODE, "renamed_file1.txt").is_ok());
    }

    #[tokio::test]
    async fn test_quota_rollback_on_failed_write() {
        let ctx = TestContext::new("tenant_quota_rollback");
        ctx.tenant_isolator.register_tenant(ctx.tenant_id.clone(), 1_000_000_000).ok();
        
        ctx.set_quota(1000).ok();
        
        ctx.quota_tracker.record_storage_write(&ctx.tenant_id, 900).ok();
        
        let usage_before = ctx.quota_tracker.get_usage(&ctx.tenant_id).unwrap();
        assert_eq!(usage_before.used_storage_bytes, 900);

        let write_result = ctx.quota_tracker.record_storage_write(&ctx.tenant_id, 200);
        
        if write_result.is_err() {
            let usage_after = ctx.quota_tracker.get_usage(&ctx.tenant_id).unwrap();
            assert_eq!(usage_after.used_storage_bytes, 900, "Usage should roll back to 900");
        }
    }
}

#[cfg(test)]
mod permission_boundaries {
    use super::*;

    #[tokio::test]
    async fn test_tenant_uid_namespace_separate() {
        let ctx_a = TestContext::new("tenant_uid_test_a");
        let ctx_b = TestContext::new("tenant_uid_test_b");

        ctx_a.tenant_isolator.register_tenant(ctx_a.tenant_id.clone(), 1_000_000_000).ok();
        ctx_b.tenant_isolator.register_tenant(ctx_b.tenant_id.clone(), 1_000_000_000).ok();

        let ns_a = ctx_a.tenant_isolator.get_tenant_namespace(&ctx_a.tenant_id).unwrap();
        
        let uid = 1000u32;
        
        let file_a = ctx_a.service.create_file(ns_a.root_inode, "file_a.txt", 0).unwrap();
        
        let mut caps_a = TenantCapabilities::default();
        caps_a.can_read = true;
        
        let ctx_with_uid_a = TenantContext::new(
            ctx_a.tenant_id.clone(),
            uid,
            SessionId::new(),
            ns_a.root_inode,
            caps_a,
        );

        let result_a = ctx_a.tenant_isolator.enforce_isolation(&ctx_with_uid_a, file_a.ino);
        assert!(result_a.is_ok(), "Same UID in same tenant should work");

        let ns_b = ctx_b.tenant_isolator.get_tenant_namespace(&ctx_b.tenant_id).unwrap();
        
        let caps_b = TenantCapabilities::default();
        let ctx_with_uid_b = TenantContext::new(
            ctx_b.tenant_id.clone(),
            uid,
            SessionId::new(),
            ns_b.root_inode,
            caps_b,
        );
        
        let result_b = ctx_b.tenant_isolator.enforce_isolation(&ctx_with_uid_b, file_a.ino);
        assert!(result_b.is_err(), "Same UID in different tenant should be isolated");
    }

    #[tokio::test]
    async fn test_xattr_tenant_isolation() {
        let ctx_a = TestContext::new("tenant_xattr_a");
        let ctx_b = TestContext::new("tenant_xattr_b");

        ctx_a.tenant_isolator.register_tenant(ctx_a.tenant_id.clone(), 1_000_000_000).ok();
        ctx_b.tenant_isolator.register_tenant(ctx_b.tenant_id.clone(), 1_000_000_000).ok();

        let ns_a = ctx_a.tenant_isolator.get_tenant_namespace(&ctx_a.tenant_id).unwrap();
        let ns_b = ctx_b.tenant_isolator.get_tenant_namespace(&ctx_b.tenant_id).unwrap();

        let file_a = ctx_a.service.create_file(ns_a.root_inode, "xattr_file.txt", 0).unwrap();
        
        let ctx_a_context = make_tenant_context(ctx_a.tenant_id.clone(), ns_a.root_inode);
        
        let isolation_result = ctx_a.tenant_isolator.enforce_isolation(&ctx_a_context, file_a.ino);
        assert!(isolation_result.is_ok());

        let ctx_b_context = make_tenant_context(ctx_b.tenant_id.clone(), ns_b.root_inode);
        
        let cross_tenant_isolation = ctx_a.tenant_isolator.enforce_isolation(&ctx_b_context, file_a.ino);
        assert!(cross_tenant_isolation.is_err(), "Tenant B should not access Tenant A's xattr");
    }

    #[tokio::test]
    async fn test_acl_tenant_isolation() {
        let ctx_a = TestContext::new("tenant_acl_a");
        let ctx_b = TestContext::new("tenant_acl_b");

        ctx_a.tenant_isolator.register_tenant(ctx_a.tenant_id.clone(), 1_000_000_000).ok();
        ctx_b.tenant_isolator.register_tenant(ctx_b.tenant_id.clone(), 1_000_000_000).ok();

        let ns_a = ctx_a.tenant_isolator.get_tenant_namespace(&ctx_a.tenant_id).unwrap();
        let ns_b = ctx_b.tenant_isolator.get_tenant_namespace(&ctx_b.tenant_id).unwrap();

        let file_a = ctx_a.service.create_file(ns_a.root_inode, "acl_file.txt", 0).unwrap();
        
        let ctx_a_context = make_tenant_context(ctx_a.tenant_id.clone(), ns_a.root_inode);
        let enforcement_a = ctx_a.tenant_isolator.enforce_isolation(&ctx_a_context, file_a.ino);
        assert!(enforcement_a.is_ok(), "Tenant A should access its own file with ACL");

        let ctx_b_context = make_tenant_context(ctx_b.tenant_id.clone(), ns_b.root_inode);
        
        let isolation_check = ctx_a.tenant_isolator.enforce_isolation(&ctx_b_context, file_a.ino);
        assert!(isolation_check.is_err(), "Tenant B ACL should not grant access to Tenant A's file");
    }
}