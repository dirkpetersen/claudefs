#[cfg(test)]
mod tests {
    use claudefs_meta::types::*;
    use claudefs_meta::{KvStore, MemoryKvStore};
    use serde::{Deserialize, Serialize};

    #[test]
    fn test_inode_id_new() {
        let id = InodeId::new(12345);
        assert_eq!(id.as_u64(), 12345);
    }

    #[test]
    fn test_inode_id_root() {
        assert_eq!(InodeId::ROOT_INODE.as_u64(), 1);
    }

    #[test]
    fn test_inode_id_display() {
        let id = InodeId::new(42);
        assert_eq!(format!("{}", id), "42");
    }

    #[test]
    fn test_node_id_new() {
        let id = NodeId::new(100);
        assert_eq!(id.as_u64(), 100);
    }

    #[test]
    fn test_node_id_display() {
        let id = NodeId::new(7);
        assert_eq!(format!("{}", id), "7");
    }

    #[test]
    fn test_shard_id_new() {
        let id = ShardId::new(42);
        assert_eq!(id.as_u16(), 42);
    }

    #[test]
    fn test_term_new() {
        let term = Term::new(10);
        assert_eq!(term.as_u64(), 10);
    }

    #[test]
    fn test_log_index_new() {
        let index = LogIndex::new(5);
        assert_eq!(index.as_u64(), 5);
    }

    #[test]
    fn test_log_index_zero() {
        assert_eq!(LogIndex::ZERO.as_u64(), 0);
    }

    #[test]
    fn test_timestamp_now() {
        let ts = Timestamp::now();
        assert!(ts.secs > 0);
    }

    #[test]
    fn test_timestamp_ordering() {
        let ts1 = Timestamp {
            secs: 100,
            nanos: 0,
        };
        let ts2 = Timestamp {
            secs: 200,
            nanos: 0,
        };
        assert!(ts1 < ts2);
    }

    #[test]
    fn test_vector_clock_new() {
        let vc = VectorClock::new(1, 100);
        assert_eq!(vc.site_id, 1);
        assert_eq!(vc.sequence, 100);
    }

    #[test]
    fn test_file_type_regular_file() {
        let ft = FileType::RegularFile;
        assert_eq!(ft.mode_bits(), 0o100000);
    }

    #[test]
    fn test_file_type_directory() {
        let ft = FileType::Directory;
        assert_eq!(ft.mode_bits(), 0o040000);
    }

    #[test]
    fn test_file_type_symlink() {
        let ft = FileType::Symlink;
        assert_eq!(ft.mode_bits(), 0o120000);
    }

    #[test]
    fn test_file_type_all_variants() {
        let types = vec![
            FileType::RegularFile,
            FileType::Directory,
            FileType::Symlink,
            FileType::BlockDevice,
            FileType::CharDevice,
            FileType::Fifo,
            FileType::Socket,
        ];

        for ft in types {
            assert!(ft.mode_bits() > 0);
        }
    }

    #[test]
    fn test_replication_state_variants() {
        let _ = ReplicationState::Local;
        let _ = ReplicationState::Pending;
        let _ = ReplicationState::Replicated;
        let _ = ReplicationState::Conflict;
    }

    #[test]
    fn test_inode_attr_new_directory() {
        let attr = InodeAttr::new_directory(InodeId::new(2), 1000, 1000, 0o755, 1);
        assert_eq!(attr.file_type, FileType::Directory);
        assert_eq!(attr.nlink, 2);
    }

    #[test]
    fn test_inode_attr_new_file() {
        let attr = InodeAttr::new_file(InodeId::new(3), 1000, 1000, 0o644, 1);
        assert_eq!(attr.file_type, FileType::RegularFile);
        assert_eq!(attr.nlink, 1);
    }

    #[test]
    fn test_inode_attr_new_symlink() {
        let attr =
            InodeAttr::new_symlink(InodeId::new(4), 1000, 1000, 0o777, 1, "/target".to_string());
        assert_eq!(attr.file_type, FileType::Symlink);
        assert_eq!(attr.symlink_target, Some("/target".to_string()));
    }

    #[test]
    fn test_dir_entry_creation() {
        let entry = DirEntry {
            name: "test.txt".to_string(),
            ino: InodeId::new(10),
            file_type: FileType::RegularFile,
        };

        assert_eq!(entry.name, "test.txt");
        assert_eq!(entry.ino.as_u64(), 10);
    }

    #[test]
    fn test_dir_entry_directory() {
        let entry = DirEntry {
            name: "mydir".to_string(),
            ino: InodeId::new(5),
            file_type: FileType::Directory,
        };

        assert_eq!(entry.file_type, FileType::Directory);
    }

    #[test]
    fn test_inode_id_serialize() {
        let id = InodeId::new(42);
        let serialized = serde_json::to_string(&id).unwrap();
        assert!(serialized.contains("42"));
    }

    #[test]
    fn test_inode_id_deserialize() {
        let json = "12345";
        let id: InodeId = serde_json::from_str(json).unwrap();
        assert_eq!(id.as_u64(), 12345);
    }

    #[test]
    fn test_node_id_serialize_roundtrip() {
        let original = NodeId::new(999);
        let serialized = serde_json::to_string(&original).unwrap();
        let deserialized: NodeId = serde_json::from_str(&serialized).unwrap();
        assert_eq!(original.as_u64(), deserialized.as_u64());
    }

    #[test]
    fn test_file_type_serialize_roundtrip() {
        let original = FileType::Directory;
        let serialized = serde_json::to_string(&original).unwrap();
        let deserialized: FileType = serde_json::from_str(&serialized).unwrap();
        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_timestamp_serialize_roundtrip() {
        let original = Timestamp {
            secs: 1234567890,
            nanos: 123456789,
        };
        let serialized = serde_json::to_string(&original).unwrap();
        let deserialized: Timestamp = serde_json::from_str(&serialized).unwrap();
        assert_eq!(original.secs, deserialized.secs);
    }

    #[test]
    fn test_inode_attr_serialize_roundtrip() {
        let original = InodeAttr::new_file(InodeId::new(100), 1000, 1000, 0o644, 1);
        let serialized = serde_json::to_string(&original).unwrap();
        let deserialized: InodeAttr = serde_json::from_str(&serialized).unwrap();
        assert_eq!(original.ino.as_u64(), deserialized.ino.as_u64());
        assert_eq!(original.file_type, deserialized.file_type);
    }

    #[test]
    fn test_dir_entry_serialize_roundtrip() {
        let original = DirEntry {
            name: "test".to_string(),
            ino: InodeId::new(50),
            file_type: FileType::RegularFile,
        };

        let serialized = serde_json::to_string(&original).unwrap();
        let deserialized: DirEntry = serde_json::from_str(&serialized).unwrap();
        assert_eq!(original.name, deserialized.name);
    }

    #[test]
    fn test_vector_clock_serialize_roundtrip() {
        let original = VectorClock::new(2, 500);
        let serialized = serde_json::to_string(&original).unwrap();
        let deserialized: VectorClock = serde_json::from_str(&serialized).unwrap();
        assert_eq!(original.site_id, deserialized.site_id);
        assert_eq!(original.sequence, deserialized.sequence);
    }

    #[test]
    fn test_log_entry_serialize_roundtrip() {
        let entry = LogEntry {
            index: LogIndex::new(10),
            term: Term::new(5),
            op: MetaOp::DeleteInode {
                ino: InodeId::new(100),
            },
        };

        let serialized = serde_json::to_string(&entry).unwrap();
        let deserialized: LogEntry = serde_json::from_str(&serialized).unwrap();
        assert_eq!(entry.index.as_u64(), deserialized.index.as_u64());
    }

    #[test]
    fn test_memory_kv_store_put_get() {
        let store = MemoryKvStore::new();
        store.put(b"key1".to_vec(), vec![1, 2, 3]).unwrap();

        let value = store.get(b"key1").unwrap();
        assert_eq!(value, Some(vec![1, 2, 3]));
    }

    #[test]
    fn test_memory_kv_store_get_missing() {
        let store = MemoryKvStore::new();
        let value = store.get(b"nonexistent").unwrap();
        assert_eq!(value, None);
    }

    #[test]
    fn test_memory_kv_store_delete() {
        let store = MemoryKvStore::new();
        store.put(b"key1".to_vec(), vec![1, 2, 3]).unwrap();
        store.delete(b"key1").unwrap();

        let value = store.get(b"key1").unwrap();
        assert_eq!(value, None);
    }

    #[test]
    fn test_memory_kv_store_scan_prefix() {
        let store = MemoryKvStore::new();
        store.put(b"a".to_vec(), vec![1]).unwrap();
        store.put(b"ab".to_vec(), vec![2]).unwrap();
        store.put(b"b".to_vec(), vec![3]).unwrap();

        let results = store.scan_prefix(b"a").unwrap();
        assert!(results.len() >= 2);
    }

    #[test]
    fn test_meta_error_display() {
        let err = MetaError::InodeNotFound(InodeId::new(42));
        let _ = format!("{}", err);
    }

    #[test]
    fn test_raft_state_variants() {
        let _ = RaftState::Follower;
        let _ = RaftState::Candidate;
        let _ = RaftState::Leader;
        let _ = RaftState::PreCandidate;
    }

    #[test]
    fn test_inode_id_shard() {
        let id = InodeId::new(1000);
        let shard = id.shard(256);
        assert!(shard.as_u16() < 256);
    }

    #[test]
    fn test_inode_id_shard_deterministic() {
        let id = InodeId::new(1000);
        let shard1 = id.shard(256);
        let shard2 = id.shard(256);
        assert_eq!(shard1.as_u16(), shard2.as_u16());
    }

    #[test]
    fn test_replication_state_serialize() {
        let state = ReplicationState::Replicated;
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("Replicated") || json.contains("replicated"));
    }
}
