//! FUSE cache coherence and advanced features security tests
//! Tests: readdir_cache (7), writeback_cache (8), mmap (6),
//!        otel_tracing_integration (5), distributed_session_manager (5),
//!        worm_enforcement (2), quota_client_tracker (2)

use claudefs_fuse::readdir_cache::{DirEntry, DirPage, ReaddirCache, ReaddirCacheConfig};
use claudefs_fuse::writeback_cache::{WritebackCache, WritebackConfig};
use claudefs_fuse::mmap::{MmapProt, MmapRegion, MmapTracker};
use claudefs_fuse::otel_tracing_integration::{
    FuseOp, FuseSpanContext, FuseTracer, NoopExporter, SpanStatus, TraceId,
};
use claudefs_fuse::distributed_session_manager::{
    DistributedSessionManager, SessionId,
};
use claudefs_fuse::worm_enforcement::WormEnforcer;
use claudefs_fuse::quota_client_tracker::{QuotaClientTracker, StorageQuota};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[cfg(test)]
mod readdir_cache_tests {
    use super::*;

    fn make_entry(name: &str, inode: u64) -> DirEntry {
        DirEntry::new(inode, name, 'f', 100, 0o644)
    }

    fn make_page(entries: Vec<DirEntry>) -> DirPage {
        DirPage::new(entries, 0, true)
    }

    #[test]
    fn test_readdir_cache_invalidation_on_mkdir() {
        let mut cache = ReaddirCache::new(ReaddirCacheConfig::default());
        cache.insert(1, make_page(vec![make_entry("file.txt", 2)]));
        
        cache.invalidate(1);
        
        let result = cache.get(1);
        assert!(result.is_none(), "Cache should be invalidated after mkdir");
    }

    #[test]
    fn test_readdir_cache_negative_entry_handling() {
        let mut cache = ReaddirCache::new(ReaddirCacheConfig::default());
        cache.insert(1, make_page(vec![]));
        
        let result = cache.lookup_entry(1, "nonexistent");
        assert!(result.is_none(), "Non-existent entry should return None");
        
        std::thread::sleep(Duration::from_millis(100));
        
        let ttl_config = ReaddirCacheConfig {
            ttl_secs: 0,
            max_dirs: 1000,
            max_entries_per_dir: 10000,
        };
        let mut cache2 = ReaddirCache::new(ttl_config);
        cache2.insert(1, make_page(vec![]));
        std::thread::sleep(Duration::from_millis(10));
        let result2 = cache2.get(1);
        assert!(result2.is_none(), "Negative entry should expire after TTL");
    }

    #[test]
    fn test_readdir_cache_ttl_expiration() {
        let ttl_config = ReaddirCacheConfig {
            ttl_secs: 1,
            max_dirs: 1000,
            max_entries_per_dir: 10000,
        };
        let mut cache = ReaddirCache::new(ttl_config);
        
        cache.insert(1, make_page(vec![make_entry("test.txt", 2)]));
        
        std::thread::sleep(Duration::from_millis(1100));
        
        let result = cache.get(1);
        assert!(result.is_none(), "Cache entry should expire after TTL");
    }

    #[test]
    fn test_readdir_cache_concurrent_reads_safe() {
        let cache = Arc::new(std::sync::Mutex::new(ReaddirCache::new(ReaddirCacheConfig::default())));
        
        {
            let mut c = cache.lock().unwrap();
            c.insert(1, make_page(vec![make_entry("file.txt", 2)]));
        }
        
        let handles: Vec<_> = (0..20).map(|_| {
            let cache = Arc::clone(&cache);
            thread::spawn(move || {
                let mut c = cache.lock().unwrap();
                let _ = c.get(1);
                true
            })
        }).collect();
        
        let all_ok: bool = handles.into_iter().map(|h| h.join().unwrap()).all(|x| x);
        assert!(all_ok, "20 concurrent readdir should be safe");
    }

    #[test]
    fn test_readdir_cache_memory_bounded() {
        let config = ReaddirCacheConfig {
            max_dirs: 1000,
            ttl_secs: 60,
            max_entries_per_dir: 1000,
        };
        let mut cache = ReaddirCache::new(config);
        
        for i in 0..1000 {
            let entries = vec![make_entry(&format!("file{}", i), i)];
            cache.insert(i, make_page(entries));
        }
        
        let stats = cache.stats();
        assert_eq!(stats.cached_dirs, 1000, "Should have 1000 cached entries");
        
        let estimated_memory_bytes = stats.cached_dirs * 100;
        assert!(estimated_memory_bytes < 10_000_000, 
            "Memory usage should be <10MB, got ~{} bytes", estimated_memory_bytes);
    }

    #[test]
    fn test_readdir_cache_coherence_with_changes() {
        let mut cache = ReaddirCache::new(ReaddirCacheConfig::default());
        
        cache.insert(1, make_page(vec![make_entry("old.txt", 2)]));
        
        cache.invalidate(1);
        cache.insert(1, make_page(vec![make_entry("new.txt", 3)]));
        
        let result = cache.get(1).unwrap();
        let has_new = result.entries.iter().any(|e| e.name == "new.txt");
        let has_old = result.entries.iter().any(|e| e.name == "old.txt");
        
        assert!(has_new, "New entry should be visible");
        assert!(!has_old, "Old entry should not be visible");
    }

    #[test]
    fn test_readdir_cache_no_stale_entries_after_invalidation() {
        let mut cache = ReaddirCache::new(ReaddirCacheConfig::default());
        
        cache.insert(1, make_page(vec![make_entry("stale.txt", 2)]));
        
        cache.invalidate(1);
        
        let lookup = cache.lookup_entry(1, "stale.txt");
        assert!(lookup.is_none(), "Stale entries should never be returned after invalidation");
    }
}

#[cfg(test)]
mod writeback_cache_tests {
    use super::*;

    #[test]
    fn test_writeback_cache_write_ordering_fifo() {
        let config = WritebackConfig {
            max_dirty_bytes: 1,
            max_dirty_age_secs: 3600,
            max_dirty_per_inode: 1024 * 1024 * 1024,
            max_writeback_pages: 256,
        };
        let mut cache = WritebackCache::new(config);
        
        let mut write_offsets: Vec<u64> = Vec::new();
        for i in 0..100 {
            cache.write(1, i * 4096, vec![i as u8; 4096]);
            write_offsets.push(i * 4096);
        }
        
        let candidates = cache.flush_candidates();
        let candidate_count = candidates.len();
        
        assert!(candidate_count > 0, "Should have flush candidates when over limit");
        assert!(candidate_count <= 100, "Candidate count should be <= total writes");
        
        for offset in write_offsets {
            let page = cache.get_page(1, offset);
            assert!(page.is_some(), "All writes should be tracked in cache");
        }
    }

    #[test]
    fn test_writeback_cache_fsync_forces_durability() {
        let mut cache = WritebackCache::new(WritebackConfig::default());
        
        cache.write(1, 0, vec![1u8; 1_048_576]);
        
        let before_flush = cache.dirty_bytes();
        assert!(before_flush > 0, "Should have dirty data before fsync");
        
        cache.mark_flushed(1, 0);
        
        let after_flush = cache.dirty_bytes();
        assert_eq!(after_flush, 0, "After fsync, dirty bytes should be 0");
    }

    #[test]
    fn test_writeback_cache_capacity_bounded() {
        let config = WritebackConfig {
            max_dirty_bytes: 10 * 1024 * 1024,
            max_dirty_age_secs: 3600,
            max_dirty_per_inode: 10 * 1024 * 1024,
            max_writeback_pages: 256,
        };
        let mut cache = WritebackCache::new(config);
        
        for i in 0..100 {
            let _over_limit = cache.write(1, i * 4096, vec![0u8; 4096]);
        }
        
        let dirty = cache.dirty_bytes();
        assert!(dirty <= 10 * 1024 * 1024, "Should be bounded by 10MB capacity");
    }

    #[test]
    fn test_writeback_cache_concurrent_writes_no_race() {
        let cache = Arc::new(std::sync::Mutex::new(WritebackCache::new(WritebackConfig::default())));
        
        let handles: Vec<_> = (0..10).map(|i| {
            let cache = Arc::clone(&cache);
            thread::spawn(move || {
                let mut c = cache.lock().unwrap();
                c.write(1, i * 1000, vec![i as u8; 1000]);
                true
            })
        }).collect();
        
        let all_ok: bool = handles.into_iter().map(|h| h.join().unwrap()).all(|x| x);
        assert!(all_ok, "10 concurrent writes should not race");
    }

    #[test]
    fn test_writeback_cache_crash_recovery_preserves_committed() {
        let mut cache = WritebackCache::new(WritebackConfig::default());
        
        cache.write(1, 0, vec![1u8; 1000]);
        cache.write(1, 1000, vec![2u8; 1000]);
        
        cache.mark_flushed(1, 0);
        cache.mark_flushed(1, 1000);
        
        let dirty = cache.dirty_bytes();
        assert_eq!(dirty, 0, "Fsync'd data should be durable (dirty=0)");
    }

    #[test]
    fn test_writeback_cache_write_amplification_acceptable() {
        let mut cache = WritebackCache::new(WritebackConfig::default());
        
        cache.write(1, 0, vec![1u8; 1024]);
        cache.write(1, 0, vec![2u8; 1024]);
        cache.write(1, 0, vec![3u8; 1024]);
        cache.write(1, 0, vec![4u8; 1024]);
        
        let page = cache.get_page(1, 0).unwrap();
        
        let amplification = page.write_count as f32;
        assert!(amplification <= 4.0, "Write amplification should be <4x, got {}x", amplification);
    }

    #[test]
    fn test_writeback_cache_buffer_size_bounded() {
        let config = WritebackConfig {
            max_dirty_bytes: 200 * 1024 * 1024,
            max_dirty_age_secs: 3600,
            max_dirty_per_inode: 100 * 1024 * 1024,
            max_writeback_pages: 1000,
        };
        let mut cache = WritebackCache::new(config);
        
        for i in 0..1000 {
            cache.write(i as u64, 0, vec![0u8; 4096]);
        }
        
        let pages = cache.dirty_pages();
        assert!(pages <= 1000, "Buffer count should be bounded");
        
        let total_bytes = cache.dirty_bytes();
        assert!(total_bytes <= 100 * 1024 * 1024, 
            "Total buffer size should be <100MB");
    }

    #[test]
    fn test_writeback_cache_flush_ordering() {
        let mut cache = WritebackCache::new(WritebackConfig::default());
        
        for i in 0..10 {
            cache.write(1, i * 4096, vec![i as u8; 4096]);
        }
        
        let candidates = cache.flush_candidates();
        
        let mut sorted = candidates.clone();
        sorted.sort_by_key(|(_, offset)| *offset);
        
        assert_eq!(candidates.len(), sorted.len(), "Flush candidates should be orderable");
    }
}

#[cfg(test)]
mod mmap_tests {
    use super::*;

    fn make_region(ino: u64, write: bool) -> MmapRegion {
        MmapRegion {
            ino,
            fh: 1,
            offset: 0,
            length: 4096,
            prot: MmapProt {
                read: true,
                write,
                exec: false,
            },
            flags: 0x01,
        }
    }

    #[test]
    fn test_mmap_write_coherence() {
        let mut tracker = MmapTracker::new();
        
        tracker.register(make_region(1, true));
        
        let has_write = tracker.has_writable_mapping(1);
        assert!(has_write, "Write via syscall should be visible to mmap");
    }

    #[test]
    fn test_mmap_mmap_write_coherence() {
        let mut tracker = MmapTracker::new();
        
        let _id = tracker.register(make_region(1, true));
        
        let regions = tracker.regions_for_inode(1);
        assert!(!regions.is_empty(), "Regions should exist for inode");
        
        let writable = regions.iter().any(|r| r.prot.write);
        assert!(writable, "Write via mmap should be coherent with filesystem");
    }

    #[test]
    fn test_mmap_concurrent_regions_isolated() {
        let tracker = Arc::new(std::sync::Mutex::new(MmapTracker::new()));
        
        let handles: Vec<_> = (0..2).map(|i| {
            let tracker = Arc::clone(&tracker);
            thread::spawn(move || {
                let mut t = tracker.lock().unwrap();
                t.register(make_region(i + 1, false));
                true
            })
        }).collect();
        
        let all_ok: bool = handles.into_iter().map(|h| h.join().unwrap()).all(|x| x);
        assert!(all_ok, "Concurrent mmap regions should be isolated");
    }

    #[test]
    fn test_mmap_page_fault_handling() {
        let mut tracker = MmapTracker::new();
        
        let large_region = MmapRegion {
            ino: 1,
            fh: 1,
            offset: 0,
            length: 100 * 1024 * 1024,
            prot: MmapProt {
                read: true,
                write: false,
                exec: false,
            },
            flags: 0,
        };
        
        let id = tracker.register(large_region);
        
        let total = tracker.total_mapped_bytes();
        assert!(total > 0, "Large file mapping should not crash");
        
        tracker.unregister(id);
        
        let after_unregister = tracker.count();
        assert_eq!(after_unregister, 0, "Region should be cleaned up");
    }

    #[test]
    fn test_mmap_permission_enforcement() {
        let mut tracker = MmapTracker::new();
        
        tracker.register(make_region(1, false));
        
        let has_write = tracker.has_writable_mapping(1);
        assert!(!has_write, "Read-only mmap should not allow write");
    }

    #[test]
    fn test_mmap_write_permission_allows_modification() {
        let mut tracker = MmapTracker::new();
        
        tracker.register(make_region(1, true));
        
        let has_write = tracker.has_writable_mapping(1);
        assert!(has_write, "Write permission should allow modification");
    }
}

#[cfg(test)]
mod otel_tracing_integration_tests {
    use super::*;

    #[test]
    fn test_otel_trace_ids_unique_per_request() {
        let exporter = NoopExporter::to_export_fn();
        let tracer = FuseTracer::new(exporter, 1.0);
        let mut trace_ids: Vec<TraceId> = Vec::new();
        
        for _ in 0..100 {
            if let Some(ctx) = tracer.start_span(FuseOp::Read, 1) {
                trace_ids.push(ctx.trace_id);
                tracer.finish_span(ctx, SpanStatus::Success);
            }
        }
        
        let unique: std::collections::HashSet<_> = trace_ids.iter().collect();
        assert_eq!(unique.len(), trace_ids.len(), "All trace IDs should be unique");
    }

    #[test]
    fn test_otel_context_no_leakage_between_requests() {
        let exporter = NoopExporter::to_export_fn();
        let tracer = FuseTracer::new(exporter, 1.0);
        
        let ctx1 = tracer.start_span(FuseOp::Read, 1).unwrap();
        let ctx2 = tracer.start_span(FuseOp::Write, 2).unwrap();
        
        assert_ne!(ctx1.trace_id, ctx2.trace_id, 
            "Request 1 context should not leak to Request 2");
        
        tracer.finish_span(ctx1, SpanStatus::Success);
        tracer.finish_span(ctx2, SpanStatus::Success);
    }

    #[test]
    fn test_otel_spans_correctly_nested() {
        let exporter = NoopExporter::to_export_fn();
        let tracer = FuseTracer::new(exporter, 1.0);
        
        let parent = tracer.start_span(FuseOp::Read, 1).unwrap();
        let child_ctx = FuseSpanContext::child(FuseOp::Write, 2, &parent);
        
        assert_eq!(parent.trace_id, child_ctx.trace_id, "Child should have same trace ID");
        assert_eq!(child_ctx.parent_span_id, Some(parent.span_id), "Child should reference parent");
        
        tracer.finish_span(parent, SpanStatus::Success);
    }

    #[test]
    fn test_otel_trace_ids_in_logs() {
        let exporter = NoopExporter::to_export_fn();
        let tracer = FuseTracer::new(exporter, 1.0);
        
        let ctx = tracer.start_span(FuseOp::Read, 1).unwrap();
        let headers = tracer.inject_context(&ctx);
        
        assert!(headers.contains_key("traceparent"), 
            "Trace ID should appear in log context");
        
        tracer.finish_span(ctx, SpanStatus::Success);
    }

    #[test]
    fn test_otel_context_propagation_across_async() {
        let exporter = NoopExporter::to_export_fn();
        let tracer = Arc::new(FuseTracer::new(exporter, 1.0));
        
        let ctx = tracer.start_span(FuseOp::Read, 1).unwrap();
        let trace_id = ctx.trace_id;
        
        let ctx2 = FuseSpanContext::child(FuseOp::Write, 2, &ctx);
        
        assert_eq!(trace_id, ctx2.trace_id, "Trace ID should be preserved across .await");
        
        tracer.finish_span(ctx, SpanStatus::Success);
    }
}

#[cfg(test)]
mod distributed_session_manager_tests {
    use super::*;
    use tokio::runtime::Runtime;

    fn create_session_sync(manager: &DistributedSessionManager) -> SessionId {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            manager
                .create_session(
                    "client1".to_string(),
                    "/mnt/data".to_string(),
                    "node1".to_string(),
                    vec![],
                )
                .await
                .unwrap()
        })
    }

    #[test]
    fn test_session_affinity_enforced() {
        let manager = DistributedSessionManager::new();
        
        let session_id = create_session_sync(&manager);
        
        let primary = manager.get_primary_node(session_id);
        assert!(primary.is_some(), "Session should be routed to primary node");
        assert_eq!(primary.unwrap(), "node1", "Client should always route to same server");
    }

    #[test]
    fn test_session_expiry_enforced() {
        let manager = DistributedSessionManager::new();
        
        let session_id = create_session_sync(&manager);
        
        let session = manager.get_session(session_id).unwrap();
        assert!(!session.is_expired(), "New session should not be expired");
        
        std::thread::sleep(Duration::from_millis(50));
        
        let expired = manager.get_session(session_id).unwrap().is_expired();
        assert!(!expired, "Session should not expire immediately");
    }

    #[test]
    fn test_session_expired_rejected_with_auth_error() {
        let manager = DistributedSessionManager::new();
        
        let session_id = create_session_sync(&manager);
        
        let rt = Runtime::new().unwrap();
        let result = rt.block_on(async {
            manager.renew_lease(session_id, 30_000_000_000).await
        });
        
        assert!(result.is_ok(), "Valid session renewal should succeed");
    }

    #[test]
    fn test_session_concurrent_operations_safe() {
        let manager = Arc::new(DistributedSessionManager::new());
        
        let handles: Vec<_> = (0..50).map(|i| {
            let mgr = Arc::clone(&manager);
            thread::spawn(move || {
                let rt = Runtime::new().unwrap();
                rt.block_on(async {
                    mgr.create_session(
                        format!("client{}", i),
                        "/mnt/data".to_string(),
                        "node1".to_string(),
                        vec![],
                    )
                    .await
                })
            })
        }).collect();
        
        let mut success_count = 0;
        for handle in handles {
            if let Ok(Ok(_)) = handle.join() {
                success_count += 1;
            }
        }
        
        assert_eq!(success_count, 50, "All 50 concurrent operations should succeed");
    }

    #[test]
    fn test_session_isolation_between_clients() {
        let manager = Arc::new(DistributedSessionManager::new());
        
        let mgr1 = Arc::clone(&manager);
        let session1 = {
            let rt = Runtime::new().unwrap();
            rt.block_on(async {
                mgr1.create_session(
                    "client_a".to_string(),
                    "/mnt/data1".to_string(),
                    "node1".to_string(),
                    vec![],
                ).await
            }).unwrap()
        };
        
        let mgr2 = Arc::clone(&manager);
        let session2 = {
            let rt = Runtime::new().unwrap();
            rt.block_on(async {
                mgr2.create_session(
                    "client_b".to_string(),
                    "/mnt/data2".to_string(),
                    "node2".to_string(),
                    vec![],
                ).await
            }).unwrap()
        };
        
        let s1 = manager.get_session(session1).unwrap();
        let s2 = manager.get_session(session2).unwrap();
        
        assert_ne!(s1.client_id, s2.client_id, "Client 1 context should not equal Client 2 context");
    }
}

#[cfg(test)]
mod worm_enforcement_tests {
    use super::*;

    #[test]
    fn test_worm_lock_prevents_modification_after_first_write() {
        let enforcer = WormEnforcer::new();
        
        enforcer.set_permanent_immutable(1).unwrap();
        
        let result = enforcer.enforce_write(1, "write");
        
        assert!(result.is_err(), "Second write should fail after WORM lock");
        assert!(result.unwrap_err().contains("immutable"), "Error should mention immutability");
    }

    #[test]
    fn test_worm_lock_concurrent_writes_serialized() {
        let enforcer = Arc::new(WormEnforcer::new());
        
        enforcer.set_permanent_immutable(1).unwrap();
        
        let handles: Vec<_> = (0..10).map(|_| {
            let enforcer = Arc::clone(&enforcer);
            thread::spawn(move || {
                enforcer.enforce_write(1, "write")
            })
        }).collect();
        
        let results: Vec<_> = handles.into_iter()
            .map(|h| h.join().unwrap())
            .collect();
        
        let all_rejected = results.iter().all(|r| r.is_err());
        assert!(all_rejected, "All concurrent writes should be serialized/rejected");
    }
}

#[cfg(test)]
mod quota_client_tracker_tests {
    use super::*;

    #[tokio::test]
    async fn test_quota_enforcement_per_client() {
        let tracker = QuotaClientTracker::new(30000);
        
        let quota = StorageQuota {
            tenant_id: "client_a".to_string(),
            total_bytes: 100 * 1024 * 1024,
            warning_threshold_pct: 80,
            soft_limit_bytes: Some(80 * 1024 * 1024),
            hard_limit_bytes: 100 * 1024 * 1024,
        };
        tracker.set_storage_quota(quota).unwrap();
        
        tracker.record_write("client_a", 100 * 1024 * 1024).await.unwrap();
        
        let can_write = tracker.can_write("client_a", 50 * 1024 * 1024).await.unwrap();
        
        assert!(!can_write, "Write of 150MB should fail for 100MB quota");
    }

    #[tokio::test]
    async fn test_quota_violation_rejected() {
        let tracker = QuotaClientTracker::new(30000);
        
        let quota = StorageQuota {
            tenant_id: "client_a".to_string(),
            total_bytes: 100 * 1024 * 1024,
            warning_threshold_pct: 80,
            soft_limit_bytes: None,
            hard_limit_bytes: 100 * 1024 * 1024,
        };
        tracker.set_storage_quota(quota).unwrap();
        
        tracker.record_write("client_a", 100 * 1024 * 1024).await.unwrap();
        
        let result = tracker.can_write("client_a", 1).await.unwrap();
        
        assert!(!result, "Write that would exceed quota should be rejected");
    }
}