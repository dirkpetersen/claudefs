# A2 Phase 3 Production-Hardening Tests

## Context
You are adding production-hardening tests to the `claudefs-meta` crate in the ClaudeFS project.
The crate already has 718 passing tests. We need to add more comprehensive tests to modules
with low test counts that are critical for production deployment.

## Working Directory
/home/cfs/claudefs/crates/claudefs-meta/src/

## Modules to enhance with tests

### 1. inode.rs — Currently 7 tests, needs ~8 more

The file is at `crates/claudefs-meta/src/inode.rs`. Key types:
- `InodeStore` - manages CRUD operations on inodes via KV store
- `InodeId::new(u64)`, `InodeId::ROOT_INODE` (value 1)
- `InodeAttr::new_file(ino, uid, gid, mode, site_id)` - creates file inode
- `InodeAttr::new_directory(ino, uid, gid, mode, site_id)` - creates dir inode
  - Note: `new_directory` sets `nlink = 2` (. and ..)
- `InodeAttr` has fields: `ino`, `file_type`, `mode`, `nlink`, `uid`, `gid`, `size`, `blocks`, etc.
- `FileType::RegularFile`, `FileType::Directory`, `FileType::Symlink`
- `MemoryKvStore::new()` - in-memory KV store for tests
- `MetaError::InodeNotFound(InodeId)`

Tests to add to the `#[cfg(test)] mod tests` block at the bottom of inode.rs:

```rust
    #[test]
    fn test_exists_returns_false_for_nonexistent() {
        let store = make_store();
        assert!(!store.exists(InodeId::new(999)).unwrap());
    }

    #[test]
    fn test_set_inode_nonexistent_returns_error() {
        let store = make_store();
        let attr = InodeAttr::new_file(InodeId::new(999), 1000, 1000, 0o644, 1);
        match store.set_inode(&attr) {
            Err(MetaError::InodeNotFound(id)) => assert_eq!(id.as_u64(), 999),
            other => panic!("expected InodeNotFound, got {:?}", other),
        }
    }

    #[test]
    fn test_multiple_inodes_independent() {
        let store = make_store();
        for i in 2..12u64 {
            let attr = InodeAttr::new_file(InodeId::new(i), 1000 + i as u32, 1000, 0o644, 1);
            store.create_inode(&attr).unwrap();
        }
        for i in 2..12u64 {
            let attr = store.get_inode(InodeId::new(i)).unwrap();
            assert_eq!(attr.uid, 1000 + i as u32);
        }
    }

    #[test]
    fn test_delete_clears_existence() {
        let store = make_store();
        let attr = InodeAttr::new_file(InodeId::new(2), 1000, 1000, 0o644, 1);
        store.create_inode(&attr).unwrap();
        assert!(store.exists(InodeId::new(2)).unwrap());
        store.delete_inode(InodeId::new(2)).unwrap();
        assert!(!store.exists(InodeId::new(2)).unwrap());
    }

    #[test]
    fn test_update_file_size() {
        let store = make_store();
        let mut attr = InodeAttr::new_file(InodeId::new(2), 1000, 1000, 0o644, 1);
        store.create_inode(&attr).unwrap();

        attr.size = 1_073_741_824; // 1 GiB
        attr.blocks = 2097152;     // 2M blocks of 512B
        store.set_inode(&attr).unwrap();

        let retrieved = store.get_inode(InodeId::new(2)).unwrap();
        assert_eq!(retrieved.size, 1_073_741_824);
        assert_eq!(retrieved.blocks, 2097152);
    }

    #[test]
    fn test_allocate_inode_monotonically_increases() {
        let store = make_store();
        let ids: Vec<u64> = (0..100).map(|_| store.allocate_inode().as_u64()).collect();
        for i in 1..ids.len() {
            assert!(ids[i] > ids[i - 1], "IDs should be monotonically increasing");
        }
    }

    #[test]
    fn test_root_inode_id_is_one() {
        assert_eq!(InodeId::ROOT_INODE.as_u64(), 1);
    }

    #[test]
    fn test_symlink_inode() {
        use crate::types::FileType;
        let store = make_store();
        let mut attr = InodeAttr::new_file(InodeId::new(2), 1000, 1000, 0o777, 1);
        attr.file_type = FileType::Symlink;
        attr.symlink_target = Some("/target/path".to_string());
        store.create_inode(&attr).unwrap();

        let retrieved = store.get_inode(InodeId::new(2)).unwrap();
        assert_eq!(retrieved.file_type, FileType::Symlink);
        assert_eq!(retrieved.symlink_target, Some("/target/path".to_string()));
    }
```

### 2. replication.rs — Currently 7 tests, needs ~6 more

The file is at `crates/claudefs-meta/src/replication.rs`. Key types:
- `ReplicationTracker::new(journal: Arc<MetadataJournal>)`
- `compact_batch(entries: Vec<JournalEntry>) -> Vec<JournalEntry>`
- `MetadataJournal::new(node_id: u64, capacity: usize)`
- `MetadataJournal.append(op: MetaOp, log_index: LogIndex) -> Result<(), MetaError>`
- `MetaOp::CreateInode { attr }`, `MetaOp::DeleteInode { ino }`, `MetaOp::SetAttr { ino, attr }`

Tests to add to the `#[cfg(test)] mod tests` block at the bottom of replication.rs:

```rust
    #[test]
    fn test_acknowledge_unregistered_site_is_noop() {
        let journal = Arc::new(MetadataJournal::new(1, 100));
        let tracker = ReplicationTracker::new(journal);
        // Should not error — silently ignores unregistered site
        tracker.acknowledge(999, 5).unwrap();
    }

    #[test]
    fn test_lag_for_unregistered_site_is_journal_length() {
        let journal = Arc::new(MetadataJournal::new(1, 100));
        journal.append(create_op(100), LogIndex::new(1)).unwrap();
        journal.append(create_op(200), LogIndex::new(2)).unwrap();

        let tracker = ReplicationTracker::new(journal.clone());
        // Unregistered site starts at confirmed_sequence=0, so lag = journal length
        let lag = tracker.lag_for_site(999).unwrap();
        assert_eq!(lag, 2);
    }

    #[test]
    fn test_multiple_sites_independent_lag() {
        let journal = Arc::new(MetadataJournal::new(1, 100));
        for i in 1..=5u64 {
            journal.append(create_op(100 + i), LogIndex::new(i)).unwrap();
        }

        let tracker = ReplicationTracker::new(journal.clone());
        tracker.register_site(10).unwrap();
        tracker.register_site(20).unwrap();

        tracker.acknowledge(10, 3).unwrap();
        // site 10 is at seq 3, lag = 5 - 3 = 2
        assert_eq!(tracker.lag_for_site(10).unwrap(), 2);
        // site 20 is at seq 0, lag = 5
        assert_eq!(tracker.lag_for_site(20).unwrap(), 5);
    }

    #[test]
    fn test_compact_batch_delete_before_create_is_preserved() {
        // Delete comes BEFORE create — should NOT be canceled (order matters)
        let entries = vec![
            make_journal_entry(1, delete_op(100)), // Delete first
            make_journal_entry(2, create_op(100)), // Create second
        ];

        let compacted = compact_batch(entries);
        // Both preserved because delete_idx (0) < create_idx (1) — not create-then-delete
        assert_eq!(compacted.len(), 2);
    }

    #[test]
    fn test_compact_batch_multiple_canceled_pairs() {
        let entries = vec![
            make_journal_entry(1, create_op(100)), // canceled
            make_journal_entry(2, create_op(200)), // canceled
            make_journal_entry(3, create_op(300)), // kept
            make_journal_entry(4, delete_op(100)), // cancels create(100)
            make_journal_entry(5, delete_op(200)), // cancels create(200)
            make_journal_entry(6, setattr_op(300)),
        ];

        let compacted = compact_batch(entries);
        assert_eq!(compacted.len(), 2);
        // Only create(300) and setattr(300) survive
        assert!(matches!(&compacted[0].op, MetaOp::CreateInode { attr } if attr.ino.as_u64() == 300));
        assert!(matches!(&compacted[1].op, MetaOp::SetAttr { .. }));
    }

    #[test]
    fn test_pending_entries_limit_honored() {
        let journal = Arc::new(MetadataJournal::new(1, 100));
        for i in 1..=10u64 {
            journal.append(create_op(100 + i), LogIndex::new(i)).unwrap();
        }

        let tracker = ReplicationTracker::new(journal.clone());
        tracker.register_site(10).unwrap();

        let pending = tracker.pending_entries(10, 3).unwrap();
        assert_eq!(pending.len(), 3);
        assert_eq!(pending[0].sequence, 1);
        assert_eq!(pending[2].sequence, 3);
    }
```

### 3. node_snapshot.rs — Currently 6 tests, needs ~6 more

The file is at `crates/claudefs-meta/src/node_snapshot.rs`. Key types:
- `NodeSnapshot::capture(node: &MetadataNode) -> Result<Self, MetaError>`
- `NodeSnapshot::serialize(&self) -> Result<Vec<u8>, MetaError>`
- `NodeSnapshot::deserialize(data: &[u8]) -> Result<Self, MetaError>`
- `MetadataNode::new(config: MetadataNodeConfig)`
- `MetadataNodeConfig { node_id, num_shards, replication_factor, site_id, data_dir, dir_shard_config }`
- `node.create_file(parent, name, uid, gid, mode)` - returns inode ID
- `node.mkdir(parent, name, uid, gid, mode)` - returns inode ID
- `node.set_xattr(ino, name, value)` - sets extended attribute
- `InodeId::ROOT_INODE`
- `SNAPSHOT_VERSION` = 1 (constant)

Tests to add to the `#[cfg(test)] mod tests` block at the bottom of node_snapshot.rs:

```rust
    #[test]
    fn test_snapshot_site_id() {
        let config = MetadataNodeConfig {
            node_id: NodeId::new(1),
            num_shards: 64,
            replication_factor: 3,
            site_id: 42,
            data_dir: None,
            dir_shard_config: DirShardConfig::default(),
        };
        let node = MetadataNode::new(config).unwrap();
        let snapshot = NodeSnapshot::capture(&node).unwrap();
        assert_eq!(snapshot.site_id, 42);
    }

    #[test]
    fn test_snapshot_multiple_files() {
        let node = make_node();
        for i in 0..10u32 {
            let name = format!("file{}.txt", i);
            node.create_file(InodeId::ROOT_INODE, &name, 1000 + i, 1000, 0o644).unwrap();
        }
        let snapshot = NodeSnapshot::capture(&node).unwrap();
        assert!(snapshot.next_inode_id > 10, "next_inode_id should reflect 10 files created");
    }

    #[test]
    fn test_snapshot_dir_entries_captured() {
        let node = make_node();
        node.mkdir(InodeId::ROOT_INODE, "alpha", 1000, 1000, 0o755).unwrap();
        node.mkdir(InodeId::ROOT_INODE, "beta", 1000, 1000, 0o755).unwrap();
        node.create_file(InodeId::ROOT_INODE, "gamma.txt", 1000, 1000, 0o644).unwrap();

        let snapshot = NodeSnapshot::capture(&node).unwrap();
        // Snapshot should have at least some directory entries
        let total_dir_entries: usize = snapshot.dir_entries.iter().map(|(_, e)| e.len()).sum();
        assert!(total_dir_entries >= 3, "should have at least 3 entries under root");
    }

    #[test]
    fn test_deserialize_invalid_data_returns_error() {
        let bad_data = b"this is not valid bincode";
        assert!(NodeSnapshot::deserialize(bad_data).is_err());
    }

    #[test]
    fn test_total_size_increases_with_more_inodes() {
        let node_small = make_node();
        let snapshot_small = NodeSnapshot::capture(&node_small).unwrap();
        let size_small = snapshot_small.total_size_bytes();

        let node_large = make_node();
        for i in 0..100u32 {
            let name = format!("f{}", i);
            node_large.create_file(InodeId::ROOT_INODE, &name, 1000, 1000, 0o644).unwrap();
        }
        let snapshot_large = NodeSnapshot::capture(&node_large).unwrap();
        let size_large = snapshot_large.total_size_bytes();

        assert!(size_large > size_small, "larger snapshot should have larger estimated size");
    }

    #[test]
    fn test_snapshot_bincode_roundtrip_preserves_site_id() {
        let config = MetadataNodeConfig {
            node_id: NodeId::new(7),
            num_shards: 16,
            replication_factor: 3,
            site_id: 99,
            data_dir: None,
            dir_shard_config: DirShardConfig::default(),
        };
        let node = MetadataNode::new(config).unwrap();
        let _ = node.create_file(InodeId::ROOT_INODE, "file.rs", 500, 500, 0o600).unwrap();

        let snapshot = NodeSnapshot::capture(&node).unwrap();
        let bytes = snapshot.serialize().unwrap();
        let restored = NodeSnapshot::deserialize(&bytes).unwrap();

        assert_eq!(restored.site_id, 99);
        assert_eq!(restored.node_id, NodeId::new(7));
        assert_eq!(restored.num_shards, 16);
    }
```

## IMPORTANT INSTRUCTIONS

1. Add the test functions above to the existing `#[cfg(test)] mod tests` blocks in each file.
   - For `inode.rs`: add tests to the `mod tests` at the bottom of the file
   - For `replication.rs`: add tests to the `mod tests` at the bottom of the file
   - For `node_snapshot.rs`: add tests to the `mod tests` at the bottom of the file

2. Do NOT modify any existing code — only add new test functions.

3. Each test function must start with `#[test]` (they are synchronous, not async).

4. Ensure imports match what's already in each test module's `use` statements.
   - For inode.rs tests: `use crate::types::{FileType, InodeAttr, InodeId};` already exists
   - For replication.rs tests: existing imports cover what we need; add `use crate::types::LogIndex;` if not present
   - For node_snapshot.rs tests: `use crate::node::MetadataNodeConfig;` and `use crate::dirshard::DirShardConfig;` exist; also need `use crate::types::NodeId;` which should already be in scope via `use crate::types::*;` in the module

5. Output ONLY the complete modified content of each file. Format:

   === FILE: crates/claudefs-meta/src/inode.rs ===
   <full file content>

   === FILE: crates/claudefs-meta/src/replication.rs ===
   <full file content>

   === FILE: crates/claudefs-meta/src/node_snapshot.rs ===
   <full file content>

6. Pay close attention to the `node_snapshot.rs` tests — `make_node()` helper function is already defined in the test module; use it. The `set_xattr` method may or may not exist on `MetadataNode` — only use `create_file` and `mkdir` which are confirmed to exist.
