#![cfg(test)]

use proptest::prelude::*;
use proptest::strategy::ValueTree;
use std::sync::Arc;

use crate::{
    inode::InodeStore,
    journal::MetadataJournal,
    kvstore::{KvStore, MemoryKvStore},
    service::{MetadataService, MetadataServiceConfig},
    types::*,
};

fn is_entry_not_found(err: &MetaError) -> bool {
    matches!(err, MetaError::EntryNotFound { .. })
}

fn is_inode_not_found(err: &MetaError) -> bool {
    matches!(err, MetaError::InodeNotFound(_))
}

proptest! {
    #[test]
    fn prop_inode_shard_always_in_range(ino in 1u64..=u64::MAX, shards in 1u16..=1024u16) {
        let shard = InodeId::new(ino).shard(shards);
        prop_assert!(shard.as_u16() < shards);
    }

    #[test]
    fn prop_inode_shard_deterministic(ino in 1u64..=u64::MAX, shards in 1u16..=1024u16) {
        let shard1 = InodeId::new(ino).shard(shards);
        let shard2 = InodeId::new(ino).shard(shards);
        prop_assert_eq!(shard1, shard2);
    }

    #[test]
    fn prop_inode_shard_uniform(num_shards in 1u16..=16u16) {
        let mut shards_used = vec![0usize; 16];
        let num_shards_usize = num_shards as usize;

        for ino in 1u64..=1000u64 {
            let shard = InodeId::new(ino).shard(num_shards);
            let shard_idx = shard.as_u16() as usize;
            prop_assert!(shard_idx < num_shards_usize, "shard {} out of range for {} shards", shard_idx, num_shards_usize);
            shards_used[shard_idx] += 1;
        }

        for i in 0..num_shards_usize {
            prop_assert!(shards_used[i] > 0, "shard {} never used", i);
        }
    }

    #[test]
    fn prop_inodeattr_bincode_roundtrip(uid in 0u32..=65535u32, gid in 0u32..=65535u32, mode in 0u32..=0o777u32, ino_val in 2u64..=u64::MAX) {
        let attr = InodeAttr::new_file(InodeId::new(ino_val), uid, gid, mode, 1);

        let encoded = bincode::serialize(&attr).unwrap();
        let decoded: InodeAttr = bincode::deserialize(&encoded).unwrap();

        prop_assert_eq!(attr, decoded);
    }

    #[test]
    fn prop_direntry_bincode_roundtrip(name in "\\PC*", ino_val in 1u64..=u64::MAX) {
        let entry = DirEntry {
            name,
            ino: InodeId::new(ino_val),
            file_type: FileType::RegularFile,
        };

        let encoded = bincode::serialize(&entry).unwrap();
        let decoded: DirEntry = bincode::deserialize(&encoded).unwrap();

        prop_assert_eq!(entry, decoded);
    }

    #[test]
    fn prop_metaop_rename_bincode_roundtrip(src_parent_val in 1u64..=u64::MAX, dst_parent_val in 1u64..=u64::MAX, src_name in "\\PC*", dst_name in "\\PC*") {
        let op = MetaOp::Rename {
            src_parent: InodeId::new(src_parent_val),
            src_name,
            dst_parent: InodeId::new(dst_parent_val),
            dst_name,
        };

        let encoded = bincode::serialize(&op).unwrap();
        let decoded: MetaOp = bincode::deserialize(&encoded).unwrap();

        match decoded {
            MetaOp::Rename { src_parent, src_name: _, dst_parent, dst_name: _ } => {
                prop_assert_eq!(src_parent, InodeId::new(src_parent_val));
                prop_assert_eq!(dst_parent, InodeId::new(dst_parent_val));
            }
            _ => prop_assert!(false, "wrong variant after deserialization"),
        }
    }

    #[test]
    fn prop_timestamp_ordering(secs1 in 0u64..=u64::MAX, nanos1 in 0u32..=999999999u32, secs2 in 0u64..=u64::MAX, nanos2 in 0u32..=999999999u32) {
        let t1 = Timestamp { secs: secs1, nanos: nanos1 };
        let t2 = Timestamp { secs: secs2, nanos: nanos2 };

        let ord = t1.cmp(&t2);
        let expected = (secs1, nanos1).cmp(&(secs2, nanos2));

        prop_assert_eq!(ord, expected);
    }

    #[test]
    fn prop_vectorclock_ordering(site1 in 0u64..=u64::MAX, seq1 in 0u64..=u64::MAX, site2 in 0u64..=u64::MAX, seq2 in 0u64..=u64::MAX) {
        let vc1 = VectorClock::new(site1, seq1);
        let vc2 = VectorClock::new(site2, seq2);

        let ord = vc1.cmp(&vc2);
        let expected = (seq1, site1).cmp(&(seq2, site2));

        prop_assert_eq!(ord, expected);
    }

    #[test]
    fn prop_inode_store_create_get_roundtrip(uid in 0u32..=65535u32, gid in 0u32..=65535u32, mode in 0u32..=0o777u32) {
        let kv: Arc<dyn KvStore> = Arc::new(MemoryKvStore::new());
        let store = InodeStore::new(kv);

        let ino = store.allocate_inode();
        let attr = InodeAttr::new_file(ino, uid, gid, mode, 1);

        store.create_inode(&attr).unwrap();

        let retrieved = store.get_inode(ino).unwrap();

        prop_assert_eq!(retrieved.ino, attr.ino);
        prop_assert_eq!(retrieved.uid, attr.uid);
        prop_assert_eq!(retrieved.gid, attr.gid);
        prop_assert_eq!(retrieved.mode, attr.mode);
        prop_assert_eq!(retrieved.file_type, attr.file_type);
        prop_assert_eq!(retrieved.nlink, attr.nlink);
    }

    #[test]
    fn prop_inode_store_set_updates_fields(uid in 0u32..=65535u32, gid in 0u32..=65535u32, mode in 0u32..=0o777u32) {
        let kv: Arc<dyn KvStore> = Arc::new(MemoryKvStore::new());
        let store = InodeStore::new(kv);

        let ino = store.allocate_inode();
        let attr = InodeAttr::new_file(ino, uid, gid, mode, 1);

        store.create_inode(&attr).unwrap();

        let mut updated_attr = attr.clone();
        updated_attr.size = 4096;
        updated_attr.mode = 0o755;

        store.set_inode(&updated_attr).unwrap();

        let retrieved = store.get_inode(ino).unwrap();

        prop_assert_eq!(retrieved.size, 4096);
        prop_assert_eq!(retrieved.mode, 0o755);
    }

    #[test]
    fn prop_inode_store_delete_removes(uid in 0u32..=65535u32, gid in 0u32..=65535u32, mode in 0u32..=0o777u32) {
        let kv: Arc<dyn KvStore> = Arc::new(MemoryKvStore::new());
        let store = InodeStore::new(kv);

        let ino = store.allocate_inode();
        let attr = InodeAttr::new_file(ino, uid, gid, mode, 1);

        store.create_inode(&attr).unwrap();

        let exists_before = store.exists(ino).unwrap();
        prop_assert!(exists_before);

        store.delete_inode(ino).unwrap();

        let exists_after = store.exists(ino).unwrap();
        prop_assert!(!exists_after);

        let result = store.get_inode(ino);
        prop_assert!(matches!(result, Err(ref e) if is_inode_not_found(e)));
    }

    #[test]
    fn prop_service_create_lookup_roundtrip(uid in 0u32..=65535u32, gid in 0u32..=65535u32, mode in 0u32..=0o777u32) {
        let svc = MetadataService::new(MetadataServiceConfig::default());
        svc.init_root().unwrap();

        let filename = "[a-z][a-z0-9_]{0,20}"
            .prop_map(|s: String| s)
            .new_tree(&mut Default::default())
            .unwrap()
            .current();

        let attr = svc.create_file(InodeId::ROOT_INODE, &filename, uid, gid, mode).unwrap();

        let found = svc.lookup(InodeId::ROOT_INODE, &filename).unwrap();

        prop_assert_eq!(found.ino, attr.ino);
        prop_assert_eq!(found.uid, uid);
    }

    #[test]
    fn prop_service_readdir_count(n in 1usize..=20usize, uid in 0u32..=65535u32, gid in 0u32..=65535u32) {
        let svc = MetadataService::new(MetadataServiceConfig::default());
        svc.init_root().unwrap();

        for i in 0..n {
            let name = format!("file_{}", i);
            svc.create_file(InodeId::ROOT_INODE, &name, uid, gid, 0o644).unwrap();
        }

        let entries = svc.readdir(InodeId::ROOT_INODE).unwrap();

        prop_assert_eq!(entries.len(), n);
    }

    #[test]
    fn prop_service_shard_in_range(ino_val in 1u64..=10000u64) {
        let svc = MetadataService::new(MetadataServiceConfig::default());

        let shard = svc.shard_for_inode(InodeId::new(ino_val));

        prop_assert!(shard.as_u16() < svc.num_shards());
    }

    #[test]
    fn prop_logindex_ordering(a in 0u64..=u64::MAX, b in 0u64..=u64::MAX) {
        let idx_a = LogIndex::new(a);
        let idx_b = LogIndex::new(b);

        let ord = idx_a.cmp(&idx_b);
        let expected = a.cmp(&b);

        prop_assert_eq!(ord, expected);
    }

    #[test]
    fn prop_term_ordering(a in 0u64..=u64::MAX, b in 0u64..=u64::MAX) {
        let term_a = Term::new(a);
        let term_b = Term::new(b);

        let ord = term_a.cmp(&term_b);
        let expected = a.cmp(&b);

        prop_assert_eq!(ord, expected);
    }

    #[test]
    fn prop_filetype_mode_bits_nonzero(idx in 0usize..7usize) {
        let ft = match idx {
            0 => FileType::RegularFile,
            1 => FileType::Directory,
            2 => FileType::Symlink,
            3 => FileType::BlockDevice,
            4 => FileType::CharDevice,
            5 => FileType::Fifo,
            _ => FileType::Socket,
        };
        prop_assert!(ft.mode_bits() != 0);
    }

    #[test]
    fn prop_service_unlink_removes_entry(uid in 0u32..=65535u32, gid in 0u32..=65535u32, mode in 0u32..=0o777u32) {
        let svc = MetadataService::new(MetadataServiceConfig::default());
        svc.init_root().unwrap();

        let filename = "testfile";
        svc.create_file(InodeId::ROOT_INODE, filename, uid, gid, mode).unwrap();

        svc.unlink(InodeId::ROOT_INODE, filename).unwrap();

        let result = svc.lookup(InodeId::ROOT_INODE, filename);
        prop_assert!(matches!(result, Err(ref e) if is_entry_not_found(e)));
    }

    #[test]
    fn prop_journal_sequence_monotonic(n in 1usize..=50usize) {
        let journal = MetadataJournal::new(1, 100);

        let mut prev_seq = 0u64;

        for i in 0..n {
            let attr = InodeAttr::new_file(InodeId::new(i as u64 + 100), 1000, 1000, 0o644, 1);
            let op = MetaOp::CreateInode { attr };

            let seq = journal.append(op, LogIndex::new(i as u64 + 1)).unwrap();

            prop_assert!(seq > prev_seq, "sequence {} should be > {}", seq, prev_seq);

            prev_seq = seq;
        }
    }

    #[test]
    fn prop_service_rename_preserves_inode(uid in 0u32..=65535u32, gid in 0u32..=65535u32, mode in 0u32..=0o777u32) {
        let svc = MetadataService::new(MetadataServiceConfig::default());
        svc.init_root().unwrap();

        let old_name = "oldname";
        let attr = svc.create_file(InodeId::ROOT_INODE, old_name, uid, gid, mode).unwrap();
        let original_ino = attr.ino;

        let new_name = "newname";
        svc.rename(InodeId::ROOT_INODE, old_name, InodeId::ROOT_INODE, new_name).unwrap();

        let found = svc.lookup(InodeId::ROOT_INODE, new_name).unwrap();
        prop_assert_eq!(found.ino, original_ino);

        let old_lookup = svc.lookup(InodeId::ROOT_INODE, old_name);
        prop_assert!(matches!(old_lookup, Err(ref e) if is_entry_not_found(e)));
    }
}
