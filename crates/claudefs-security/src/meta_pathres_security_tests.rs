use claudefs_meta::pathres::{PathCacheEntry, PathResolver};
use claudefs_meta::types::*;

#[test]
fn test_meta_pr_sec_parse_path_empty_string_returns_empty() {
    let components = PathResolver::parse_path("");
    assert!(components.is_empty());
}

#[test]
fn test_meta_pr_sec_parse_path_single_slash_returns_empty() {
    let components = PathResolver::parse_path("/");
    assert!(components.is_empty());
}

#[test]
fn test_meta_pr_sec_parse_path_double_slash_returns_empty() {
    let components = PathResolver::parse_path("//");
    assert!(components.is_empty());
}

#[test]
fn test_meta_pr_sec_parse_path_embedded_null_characters() {
    let path = "/foo\0bar/baz";
    let components = PathResolver::parse_path(path);
    assert_eq!(components.len(), 2);
    assert_eq!(components[0], "foo\0bar");
    assert_eq!(components[1], "baz");
}

#[test]
fn test_meta_pr_sec_parse_path_very_long_path() {
    let mut path = String::new();
    for i in 0..1000 {
        path.push_str(&format!("/component{}", i));
    }
    let components = PathResolver::parse_path(&path);
    assert_eq!(components.len(), 1000);
    for i in 0..1000 {
        assert_eq!(components[i], format!("component{}", i));
    }
}

#[test]
fn test_meta_pr_sec_speculative_resolve_empty_cache() {
    let resolver = PathResolver::new(256, 1000, 30, 10000);
    let (resolved, remaining) = resolver.speculative_resolve("/a/b/c");
    assert!(resolved.is_empty());
    assert_eq!(remaining, vec!["a", "b", "c"]);
}

#[test]
fn test_meta_pr_sec_speculative_resolve_partial_cache_hit() {
    let resolver = PathResolver::new(256, 1000, 30, 10000);
    let a_ino = InodeId::new(10);
    resolver.cache_resolution(
        InodeId::ROOT_INODE,
        "a",
        PathCacheEntry {
            ino: a_ino,
            file_type: FileType::Directory,
            shard: a_ino.shard(256),
        },
    );
    let (resolved, remaining) = resolver.speculative_resolve("/a/b/c");
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].ino, a_ino);
    assert_eq!(remaining, vec!["b", "c"]);
}

#[test]
fn test_meta_pr_sec_speculative_resolve_full_cache_hit() {
    let resolver = PathResolver::new(256, 1000, 30, 10000);
    let a_ino = InodeId::new(10);
    let b_ino = InodeId::new(20);
    let c_ino = InodeId::new(30);

    resolver.cache_resolution(
        InodeId::ROOT_INODE,
        "a",
        PathCacheEntry {
            ino: a_ino,
            file_type: FileType::Directory,
            shard: a_ino.shard(256),
        },
    );
    resolver.cache_resolution(
        a_ino,
        "b",
        PathCacheEntry {
            ino: b_ino,
            file_type: FileType::Directory,
            shard: b_ino.shard(256),
        },
    );
    resolver.cache_resolution(
        b_ino,
        "c",
        PathCacheEntry {
            ino: c_ino,
            file_type: FileType::RegularFile,
            shard: c_ino.shard(256),
        },
    );

    let (resolved, remaining) = resolver.speculative_resolve("/a/b/c");
    assert_eq!(resolved.len(), 3);
    assert!(remaining.is_empty());
    assert_eq!(resolved[2].ino, c_ino);
}

#[test]
fn test_meta_pr_sec_speculative_resolve_cache_miss_middle() {
    let resolver = PathResolver::new(256, 1000, 30, 10000);
    let a_ino = InodeId::new(10);
    resolver.cache_resolution(
        InodeId::ROOT_INODE,
        "a",
        PathCacheEntry {
            ino: a_ino,
            file_type: FileType::Directory,
            shard: a_ino.shard(256),
        },
    );
    let (resolved, remaining) = resolver.speculative_resolve("/a/b/c/d");
    assert_eq!(resolved.len(), 1);
    assert_eq!(remaining, vec!["b", "c", "d"]);
}

#[test]
fn test_meta_pr_sec_speculative_resolve_root_path() {
    let resolver = PathResolver::new(256, 1000, 30, 10000);
    let (resolved, remaining) = resolver.speculative_resolve("/");
    assert!(resolved.is_empty());
    assert!(remaining.is_empty());
}

#[test]
fn test_meta_pr_sec_cache_eviction_when_max_entries_reached() {
    let resolver = PathResolver::new(256, 4, 30, 10000);

    resolver.cache_resolution(
        InodeId::ROOT_INODE,
        "a",
        PathCacheEntry {
            ino: InodeId::new(10),
            file_type: FileType::Directory,
            shard: ShardId::new(0),
        },
    );
    resolver.cache_resolution(
        InodeId::ROOT_INODE,
        "b",
        PathCacheEntry {
            ino: InodeId::new(20),
            file_type: FileType::Directory,
            shard: ShardId::new(0),
        },
    );
    resolver.cache_resolution(
        InodeId::ROOT_INODE,
        "c",
        PathCacheEntry {
            ino: InodeId::new(30),
            file_type: FileType::Directory,
            shard: ShardId::new(0),
        },
    );
    resolver.cache_resolution(
        InodeId::ROOT_INODE,
        "d",
        PathCacheEntry {
            ino: InodeId::new(40),
            file_type: FileType::Directory,
            shard: ShardId::new(0),
        },
    );

    assert_eq!(resolver.cache_size(), 4);

    resolver.cache_resolution(
        InodeId::ROOT_INODE,
        "e",
        PathCacheEntry {
            ino: InodeId::new(50),
            file_type: FileType::Directory,
            shard: ShardId::new(0),
        },
    );

    assert!(resolver.cache_size() <= 4);
}

#[test]
fn test_meta_pr_sec_cache_eviction_removes_oldest_lru() {
    let resolver = PathResolver::new(256, 4, 30, 10000);

    resolver.cache_resolution(
        InodeId::ROOT_INODE,
        "first",
        PathCacheEntry {
            ino: InodeId::new(10),
            file_type: FileType::Directory,
            shard: ShardId::new(0),
        },
    );
    resolver.cache_resolution(
        InodeId::ROOT_INODE,
        "second",
        PathCacheEntry {
            ino: InodeId::new(20),
            file_type: FileType::Directory,
            shard: ShardId::new(0),
        },
    );
    resolver.cache_resolution(
        InodeId::ROOT_INODE,
        "third",
        PathCacheEntry {
            ino: InodeId::new(30),
            file_type: FileType::Directory,
            shard: ShardId::new(0),
        },
    );
    resolver.cache_resolution(
        InodeId::ROOT_INODE,
        "fourth",
        PathCacheEntry {
            ino: InodeId::new(40),
            file_type: FileType::Directory,
            shard: ShardId::new(0),
        },
    );

    resolver.cache_resolution(
        InodeId::ROOT_INODE,
        "new",
        PathCacheEntry {
            ino: InodeId::new(50),
            file_type: FileType::Directory,
            shard: ShardId::new(0),
        },
    );

    let (resolved, _) = resolver.speculative_resolve("/first");
    assert!(
        resolved.is_empty(),
        "oldest entry 'first' should have been evicted"
    );

    let (resolved2, _) = resolver.speculative_resolve("/new");
    assert_eq!(resolved2.len(), 1, "new entry should be present");
}

#[test]
fn test_meta_pr_sec_cache_reaccess_moves_to_lru_back() {
    let resolver = PathResolver::new(256, 4, 30, 10000);

    resolver.cache_resolution(
        InodeId::ROOT_INODE,
        "a",
        PathCacheEntry {
            ino: InodeId::new(10),
            file_type: FileType::Directory,
            shard: ShardId::new(0),
        },
    );
    resolver.cache_resolution(
        InodeId::ROOT_INODE,
        "b",
        PathCacheEntry {
            ino: InodeId::new(20),
            file_type: FileType::Directory,
            shard: ShardId::new(0),
        },
    );
    resolver.cache_resolution(
        InodeId::ROOT_INODE,
        "c",
        PathCacheEntry {
            ino: InodeId::new(30),
            file_type: FileType::Directory,
            shard: ShardId::new(0),
        },
    );
    resolver.cache_resolution(
        InodeId::ROOT_INODE,
        "d",
        PathCacheEntry {
            ino: InodeId::new(40),
            file_type: FileType::Directory,
            shard: ShardId::new(0),
        },
    );

    resolver.cache_resolution(
        InodeId::ROOT_INODE,
        "a",
        PathCacheEntry {
            ino: InodeId::new(10),
            file_type: FileType::Directory,
            shard: ShardId::new(0),
        },
    );

    resolver.cache_resolution(
        InodeId::ROOT_INODE,
        "e",
        PathCacheEntry {
            ino: InodeId::new(50),
            file_type: FileType::Directory,
            shard: ShardId::new(0),
        },
    );

    let (resolved, _) = resolver.speculative_resolve("/a");
    assert_eq!(resolved.len(), 1, "reaccessed 'a' should still be present");
}

#[test]
fn test_meta_pr_sec_cache_max_entries_one_evicts_on_insert() {
    let resolver = PathResolver::new(256, 4, 30, 10000);

    for i in 0..4 {
        resolver.cache_resolution(
            InodeId::ROOT_INODE,
            &format!("entry{}", i),
            PathCacheEntry {
                ino: InodeId::new((i + 1) as u64 * 10),
                file_type: FileType::Directory,
                shard: ShardId::new(0),
            },
        );
    }
    assert_eq!(resolver.cache_size(), 4);

    resolver.cache_resolution(
        InodeId::ROOT_INODE,
        "new",
        PathCacheEntry {
            ino: InodeId::new(50),
            file_type: FileType::Directory,
            shard: ShardId::new(0),
        },
    );

    assert!(resolver.cache_size() <= 4);
}

#[test]
fn test_meta_pr_sec_negative_cache_miss_returns_false() {
    let resolver = PathResolver::new(256, 1000, 30, 10000);
    assert!(!resolver.check_negative(InodeId::ROOT_INODE, "nonexistent"));
}

#[test]
fn test_meta_pr_sec_negative_cache_hit_blocks_resolve() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let resolver = PathResolver::new(256, 1000, 30, 10000);

    resolver.cache_negative(InodeId::ROOT_INODE, "nonexistent");
    assert!(resolver.check_negative(InodeId::ROOT_INODE, "nonexistent"));

    let lookup_called = AtomicBool::new(false);
    let result = resolver.resolve_path("/nonexistent", |_, _| {
        lookup_called.store(true, Ordering::SeqCst);
        Ok(DirEntry {
            name: "nonexistent".to_string(),
            ino: InodeId::new(100),
            file_type: FileType::RegularFile,
        })
    });

    assert!(result.is_err());
    assert!(
        !lookup_called.load(Ordering::SeqCst),
        "lookup_fn should not be called for negative cache hit"
    );
}

#[test]
fn test_meta_pr_sec_negative_cache_invalidate_removes_entry() {
    let resolver = PathResolver::new(256, 1000, 30, 10000);

    resolver.cache_negative(InodeId::ROOT_INODE, "testfile");
    assert!(resolver.check_negative(InodeId::ROOT_INODE, "testfile"));

    resolver.invalidate_negative(InodeId::ROOT_INODE, "testfile");
    assert!(!resolver.check_negative(InodeId::ROOT_INODE, "testfile"));
}

#[test]
fn test_meta_pr_sec_negative_cache_invalidate_parent_removes_all() {
    let resolver = PathResolver::new(256, 1000, 30, 10000);
    let parent = InodeId::new(100);

    resolver.cache_negative(parent, "file1");
    resolver.cache_negative(parent, "file2");
    resolver.cache_negative(parent, "file3");
    resolver.cache_negative(InodeId::ROOT_INODE, "otherfile");

    assert_eq!(resolver.negative_cache_size(), 4);

    resolver.invalidate_negative_parent(parent);

    assert!(!resolver.check_negative(parent, "file1"));
    assert!(!resolver.check_negative(parent, "file2"));
    assert!(!resolver.check_negative(parent, "file3"));
    assert!(
        resolver.check_negative(InodeId::ROOT_INODE, "otherfile"),
        "unrelated entry should remain"
    );
}

#[test]
fn test_meta_pr_sec_negative_cache_max_entries_eviction() {
    let resolver = PathResolver::new(256, 1000, 30, 3);

    resolver.cache_negative(InodeId::ROOT_INODE, "file1");
    resolver.cache_negative(InodeId::ROOT_INODE, "file2");
    resolver.cache_negative(InodeId::ROOT_INODE, "file3");

    assert_eq!(resolver.negative_cache_size(), 3);

    resolver.cache_negative(InodeId::ROOT_INODE, "file4");

    assert!(
        resolver.negative_cache_size() <= 3,
        "negative cache should not exceed max_entries"
    );
}

#[test]
fn test_meta_pr_sec_path_traversal_dotdot_as_literal() {
    let components = PathResolver::parse_path("/a/../b");
    assert_eq!(components, vec!["a", "..", "b"]);
}

#[test]
fn test_meta_pr_sec_path_traversal_dot_as_literal() {
    let components = PathResolver::parse_path("/a/./b");
    assert_eq!(components, vec!["a", ".", "b"]);
}

#[test]
fn test_meta_pr_sec_path_embedded_slash_in_component() {
    let components = PathResolver::parse_path("/a/b/c");
    assert_eq!(components.len(), 3);
    assert_eq!(components[0], "a");
    assert_eq!(components[1], "b");
    assert_eq!(components[2], "c");
}

#[test]
fn test_meta_pr_sec_path_unicode_components() {
    let components = PathResolver::parse_path("/日本語/🔥/Émoji");
    assert_eq!(components, vec!["日本語", "🔥", "Émoji"]);
}

#[test]
fn test_meta_pr_sec_path_very_long_component_name() {
    let long_name = "a".repeat(4096);
    let path = format!("/{}", long_name);
    let components = PathResolver::parse_path(&path);
    assert_eq!(components.len(), 1);
    assert_eq!(components[0].len(), 4096);
}

#[test]
fn test_meta_pr_sec_clear_cache_empties_positive_and_negative() {
    let resolver = PathResolver::new(256, 1000, 30, 10000);

    resolver.cache_resolution(
        InodeId::ROOT_INODE,
        "a",
        PathCacheEntry {
            ino: InodeId::new(10),
            file_type: FileType::Directory,
            shard: ShardId::new(0),
        },
    );
    resolver.cache_negative(InodeId::ROOT_INODE, "b");

    assert_eq!(resolver.cache_size(), 1);
    assert_eq!(resolver.negative_cache_size(), 1);

    resolver.clear_cache();

    assert_eq!(resolver.cache_size(), 0);
}

#[test]
fn test_meta_pr_sec_invalidate_parent_only_affects_target() {
    let resolver = PathResolver::new(256, 1000, 30, 10000);
    let parent_a = InodeId::new(10);
    let parent_b = InodeId::new(20);

    resolver.cache_resolution(
        parent_a,
        "x",
        PathCacheEntry {
            ino: InodeId::new(100),
            file_type: FileType::RegularFile,
            shard: ShardId::new(0),
        },
    );
    resolver.cache_resolution(
        parent_b,
        "y",
        PathCacheEntry {
            ino: InodeId::new(200),
            file_type: FileType::RegularFile,
            shard: ShardId::new(0),
        },
    );

    assert_eq!(resolver.cache_size(), 2);

    resolver.invalidate_parent(parent_a);

    assert_eq!(resolver.cache_size(), 1);
}

#[test]
fn test_meta_pr_sec_invalidate_entry_only_affects_specific() {
    let resolver = PathResolver::new(256, 1000, 30, 10000);

    resolver.cache_resolution(
        InodeId::ROOT_INODE,
        "a",
        PathCacheEntry {
            ino: InodeId::new(10),
            file_type: FileType::Directory,
            shard: ShardId::new(0),
        },
    );
    resolver.cache_resolution(
        InodeId::ROOT_INODE,
        "b",
        PathCacheEntry {
            ino: InodeId::new(20),
            file_type: FileType::Directory,
            shard: ShardId::new(0),
        },
    );

    assert_eq!(resolver.cache_size(), 2);

    resolver.invalidate_entry(InodeId::ROOT_INODE, "a");

    assert_eq!(resolver.cache_size(), 1);
    let (resolved, _) = resolver.speculative_resolve("/b");
    assert_eq!(resolved.len(), 1);
}

#[test]
fn test_meta_pr_sec_cache_update_existing_key_no_size_increase() {
    let resolver = PathResolver::new(256, 1000, 30, 10000);

    resolver.cache_resolution(
        InodeId::ROOT_INODE,
        "a",
        PathCacheEntry {
            ino: InodeId::new(10),
            file_type: FileType::Directory,
            shard: ShardId::new(0),
        },
    );
    assert_eq!(resolver.cache_size(), 1);

    resolver.cache_resolution(
        InodeId::ROOT_INODE,
        "a",
        PathCacheEntry {
            ino: InodeId::new(999),
            file_type: FileType::RegularFile,
            shard: ShardId::new(0),
        },
    );

    assert_eq!(
        resolver.cache_size(),
        1,
        "cache size should not increase when updating existing key"
    );
    let (resolved, _) = resolver.speculative_resolve("/a");
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].ino, InodeId::new(999));
}
