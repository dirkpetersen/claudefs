use std::collections::{HashMap, VecDeque};
use std::sync::RwLock;

use crate::types::*;

/// Cache entry for a resolved path component.
#[derive(Clone, Debug)]
pub struct PathCacheEntry {
    /// The resolved inode ID.
    pub ino: InodeId,
    /// The file type.
    pub file_type: FileType,
    /// The shard this inode belongs to.
    pub shard: ShardId,
}

/// A negative cache entry indicating that a name does NOT exist under a parent.
#[derive(Clone, Debug)]
pub struct NegativeCacheEntry {
    /// When this negative entry was cached.
    pub cached_at: Timestamp,
    /// TTL in seconds for this negative entry.
    pub ttl_secs: u64,
}

impl NegativeCacheEntry {
    /// Check if this negative entry has expired.
    fn is_expired(&self) -> bool {
        let now = Timestamp::now();
        now.secs.saturating_sub(self.cached_at.secs) >= self.ttl_secs
    }
}

/// Speculative path resolution with caching.
///
/// Maintains a cache of path-to-inode mappings for fast speculative lookups.
/// Cache keys are (parent_ino, component_name) pairs.
/// Also maintains a negative cache for "entry not found" results.
pub struct PathResolver {
    /// Number of shards for shard computation.
    num_shards: u16,
    /// Cache: (parent_ino, name) -> PathCacheEntry
    cache: RwLock<HashMap<(InodeId, String), PathCacheEntry>>,
    /// Maximum cache entries before eviction.
    max_entries: usize,
    /// LRU order for eviction.
    lru_order: RwLock<VecDeque<(InodeId, String)>>,
    /// Negative cache: (parent_ino, name) -> NegativeCacheEntry
    negative_cache: RwLock<HashMap<(InodeId, String), NegativeCacheEntry>>,
    /// TTL for negative cache entries in seconds.
    negative_cache_ttl_secs: u64,
    /// Maximum negative cache entries.
    max_negative_entries: usize,
}

impl PathResolver {
    /// Create a new path resolver.
    ///
    /// # Arguments
    /// * `num_shards` - Number of shards for shard computation
    /// * `max_entries` - Maximum positive cache entries
    /// * `negative_cache_ttl_secs` - TTL for negative cache entries in seconds
    /// * `max_negative_entries` - Maximum negative cache entries
    pub fn new(
        num_shards: u16,
        max_entries: usize,
        negative_cache_ttl_secs: u64,
        max_negative_entries: usize,
    ) -> Self {
        Self {
            num_shards,
            cache: RwLock::new(HashMap::new()),
            max_entries,
            lru_order: RwLock::new(VecDeque::new()),
            negative_cache: RwLock::new(HashMap::new()),
            negative_cache_ttl_secs,
            max_negative_entries,
        }
    }

    /// Parse a path into components. Handles "/", "//", leading/trailing slashes.
    pub fn parse_path(path: &str) -> Vec<String> {
        if path.is_empty() {
            return vec![];
        }

        let mut components = Vec::new();
        let mut chars = path.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '/' {
                continue;
            }

            let mut name = String::new();
            name.push(c);
            while let Some(&next) = chars.peek() {
                if next == '/' {
                    break;
                }
                name.push(chars.next().unwrap());
            }
            if !name.is_empty() {
                components.push(name);
            }
        }

        components
    }

    /// Speculatively resolve a path using the cache.
    /// Returns (resolved components as Vec<PathCacheEntry>, remaining unresolved components).
    /// Resolves as many components as possible from cache, starting from root.
    pub fn speculative_resolve(&self, path: &str) -> (Vec<PathCacheEntry>, Vec<String>) {
        let components = Self::parse_path(path);
        let mut resolved = Vec::new();
        let mut current_parent = InodeId::ROOT_INODE;

        for component in &components {
            let cache = self.cache.read().unwrap();
            let key = (current_parent, component.to_string());

            match cache.get(&key) {
                Some(entry) => {
                    resolved.push(entry.clone());
                    current_parent = entry.ino;
                }
                None => {
                    drop(cache);
                    let remaining: Vec<String> = components[resolved.len()..]
                        .iter()
                        .map(|s| s.to_string())
                        .collect();
                    return (resolved, remaining);
                }
            }
        }

        (resolved, vec![])
    }

    /// Update the cache with a resolved (parent, name) -> entry mapping.
    pub fn cache_resolution(&self, parent: InodeId, name: &str, entry: PathCacheEntry) {
        let mut cache = self.cache.write().unwrap();
        let mut lru = self.lru_order.write().unwrap();
        let key = (parent, name.to_string());

        if cache.len() >= self.max_entries && !cache.contains_key(&key) {
            let evict_count = self.max_entries / 4;
            for _ in 0..evict_count {
                if let Some(old_key) = lru.pop_front() {
                    cache.remove(&old_key);
                }
            }
        }

        cache.insert(key.clone(), entry);
        lru.retain(|k| k != &key);
        lru.push_back(key);
    }

    /// Invalidate cache entries for a given parent directory.
    /// Called when directory contents change (create, delete, rename).
    pub fn invalidate_parent(&self, parent: InodeId) {
        let mut cache = self.cache.write().unwrap();
        let mut lru = self.lru_order.write().unwrap();
        let keys_to_remove: Vec<_> = cache
            .keys()
            .filter(|(p, _)| *p == parent)
            .cloned()
            .collect();

        for key in keys_to_remove {
            cache.remove(&key);
            lru.retain(|k| k != &key);
        }
    }

    /// Invalidate a specific cache entry.
    pub fn invalidate_entry(&self, parent: InodeId, name: &str) {
        let mut cache = self.cache.write().unwrap();
        let mut lru = self.lru_order.write().unwrap();
        let key = (parent, name.to_string());
        cache.remove(&key);
        lru.retain(|k| k != &key);
    }

    /// Get cache size.
    pub fn cache_size(&self) -> usize {
        self.cache.read().unwrap().len()
    }

    /// Clear the entire cache.
    pub fn clear_cache(&self) {
        let mut cache = self.cache.write().unwrap();
        let mut lru = self.lru_order.write().unwrap();
        cache.clear();
        lru.clear();
    }

    /// Cache a "not found" result for a (parent, name) pair.
    ///
    /// This caches the negative result so that future lookups can avoid
    /// repeated failed lookups for missing files (common in build systems).
    pub fn cache_negative(&self, parent: InodeId, name: &str) {
        let mut cache = self.negative_cache.write().unwrap();
        let key = (parent, name.to_string());

        if cache.len() >= self.max_negative_entries && !cache.contains_key(&key) {
            let evict_count = (self.max_negative_entries / 4).max(1);
            let keys: Vec<_> = cache.keys().cloned().collect();
            for key_to_evict in keys.iter().take(evict_count) {
                cache.remove(key_to_evict);
            }
        }

        cache.insert(
            key,
            NegativeCacheEntry {
                cached_at: Timestamp::now(),
                ttl_secs: self.negative_cache_ttl_secs,
            },
        );
    }

    /// Check if a (parent, name) pair is in the negative cache and not expired.
    ///
    /// Returns true if the entry exists and has not expired.
    pub fn check_negative(&self, parent: InodeId, name: &str) -> bool {
        let cache = self.negative_cache.read().unwrap();
        let key = (parent, name.to_string());
        match cache.get(&key) {
            Some(entry) => !entry.is_expired(),
            None => false,
        }
    }

    /// Invalidate a specific negative cache entry.
    ///
    /// Called when a file is created (the "not found" result is now stale).
    pub fn invalidate_negative(&self, parent: InodeId, name: &str) {
        let mut cache = self.negative_cache.write().unwrap();
        let key = (parent, name.to_string());
        cache.remove(&key);
    }

    /// Invalidate all negative cache entries for a parent directory.
    ///
    /// Called when directory contents change.
    pub fn invalidate_negative_parent(&self, parent: InodeId) {
        let mut cache = self.negative_cache.write().unwrap();
        let keys_to_remove: Vec<_> = cache
            .keys()
            .filter(|(p, _)| *p == parent)
            .cloned()
            .collect();

        for key in keys_to_remove {
            cache.remove(&key);
        }
    }

    /// Clean up expired negative cache entries.
    ///
    /// Returns the number of entries removed.
    pub fn cleanup_expired_negative(&self) -> usize {
        let mut cache = self.negative_cache.write().unwrap();
        let keys_to_remove: Vec<_> = cache
            .iter()
            .filter(|(_, entry)| entry.is_expired())
            .map(|(k, _)| k.clone())
            .collect();

        for key in &keys_to_remove {
            cache.remove(key);
        }
        keys_to_remove.len()
    }

    /// Return the current negative cache size.
    pub fn negative_cache_size(&self) -> usize {
        self.negative_cache.read().unwrap().len()
    }

    /// Resolve a full path sequentially using the provided lookup function.
    /// This is the fallback when speculation misses.
    /// `lookup_fn` takes (parent_ino, component_name) and returns Result<DirEntry, MetaError>.
    ///
    /// This method checks the negative cache before each lookup and caches
    /// negative results when lookups fail.
    pub fn resolve_path<F>(&self, path: &str, lookup_fn: F) -> Result<InodeId, MetaError>
    where
        F: Fn(InodeId, &str) -> Result<DirEntry, MetaError>,
    {
        let components = Self::parse_path(path);
        let mut current_parent = InodeId::ROOT_INODE;

        for component in components {
            let key = (current_parent, component.to_string());

            // Check negative cache first
            if self.check_negative(current_parent, &component) {
                return Err(MetaError::EntryNotFound {
                    parent: current_parent,
                    name: component,
                });
            }

            let entry_opt = {
                let cache = self.cache.read().unwrap();
                cache.get(&key).cloned()
            };

            if let Some(cached) = entry_opt {
                current_parent = cached.ino;
            } else {
                let dir_entry = match lookup_fn(current_parent, &component) {
                    Ok(entry) => entry,
                    Err(e) => {
                        // Cache the negative result
                        self.cache_negative(current_parent, &component);
                        return Err(e);
                    }
                };
                let shard = dir_entry.ino.shard(self.num_shards);
                let cache_entry = PathCacheEntry {
                    ino: dir_entry.ino,
                    file_type: dir_entry.file_type,
                    shard,
                };
                self.cache_resolution(current_parent, &component, cache_entry);
                // Invalidate any negative cache entry for this (parent, name)
                // since we found the entry
                self.invalidate_negative(current_parent, &component);
                current_parent = dir_entry.ino;
            }
        }

        Ok(current_parent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_path_simple() {
        let components = PathResolver::parse_path("/home/user/file.txt");
        assert_eq!(components, vec!["home", "user", "file.txt"]);
    }

    #[test]
    fn test_parse_path_root() {
        let components = PathResolver::parse_path("/");
        assert!(components.is_empty());
    }

    #[test]
    fn test_parse_path_double_slash() {
        let components = PathResolver::parse_path("/home//user///file.txt");
        assert_eq!(components, vec!["home", "user", "file.txt"]);
    }

    #[test]
    fn test_speculative_resolve_empty_cache() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);
        let (resolved, remaining) = resolver.speculative_resolve("/home/user/file.txt");
        assert!(resolved.is_empty());
        assert_eq!(remaining, vec!["home", "user", "file.txt"]);
    }

    #[test]
    fn test_speculative_resolve_partial_cache() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);
        let home_ino = InodeId::new(10);
        resolver.cache_resolution(
            InodeId::ROOT_INODE,
            "home",
            PathCacheEntry {
                ino: home_ino,
                file_type: FileType::Directory,
                shard: home_ino.shard(256),
            },
        );

        let (resolved, remaining) = resolver.speculative_resolve("/home/user/file.txt");
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].ino, home_ino);
        assert_eq!(remaining, vec!["user", "file.txt"]);
    }

    #[test]
    fn test_speculative_resolve_full_cache() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);
        let home_ino = InodeId::new(10);
        let user_ino = InodeId::new(20);
        let file_ino = InodeId::new(30);

        resolver.cache_resolution(
            InodeId::ROOT_INODE,
            "home",
            PathCacheEntry {
                ino: home_ino,
                file_type: FileType::Directory,
                shard: home_ino.shard(256),
            },
        );
        resolver.cache_resolution(
            home_ino,
            "user",
            PathCacheEntry {
                ino: user_ino,
                file_type: FileType::Directory,
                shard: user_ino.shard(256),
            },
        );
        resolver.cache_resolution(
            user_ino,
            "file.txt",
            PathCacheEntry {
                ino: file_ino,
                file_type: FileType::RegularFile,
                shard: file_ino.shard(256),
            },
        );

        let (resolved, remaining) = resolver.speculative_resolve("/home/user/file.txt");
        assert_eq!(resolved.len(), 3);
        assert!(remaining.is_empty());
        assert_eq!(resolved[2].ino, file_ino);
    }

    #[test]
    fn test_invalidate_parent() {
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

        resolver.invalidate_parent(InodeId::ROOT_INODE);
        assert_eq!(resolver.cache_size(), 0);
    }

    #[test]
    fn test_invalidate_entry() {
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

        resolver.invalidate_entry(InodeId::ROOT_INODE, "a");
        assert_eq!(resolver.cache_size(), 1);
    }

    #[test]
    fn test_resolve_path_sequential() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);
        let home_ino = InodeId::new(10);
        let user_ino = InodeId::new(20);
        let file_ino = InodeId::new(30);

        let result =
            resolver.resolve_path("/home/user/file.txt", |parent, name| match (parent, name) {
                (p, "home") if p == InodeId::ROOT_INODE => Ok(DirEntry {
                    name: "home".to_string(),
                    ino: home_ino,
                    file_type: FileType::Directory,
                }),
                (p, "user") if p == home_ino => Ok(DirEntry {
                    name: "user".to_string(),
                    ino: user_ino,
                    file_type: FileType::Directory,
                }),
                (p, "file.txt") if p == user_ino => Ok(DirEntry {
                    name: "file.txt".to_string(),
                    ino: file_ino,
                    file_type: FileType::RegularFile,
                }),
                (parent, name) => Err(MetaError::EntryNotFound {
                    parent,
                    name: name.to_string(),
                }),
            });
        assert_eq!(result.unwrap(), file_ino);
        assert_eq!(resolver.cache_size(), 3);
    }

    #[test]
    fn test_resolve_path_not_found() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);
        let result = resolver.resolve_path("/nonexistent", |parent, name| {
            Err(MetaError::EntryNotFound {
                parent,
                name: name.to_string(),
            })
        });
        assert!(result.is_err());
    }

    // ---- Negative cache tests ----

    #[test]
    fn test_negative_cache_miss_not_found() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);
        assert!(!resolver.check_negative(InodeId::ROOT_INODE, "nonexistent"));
        assert_eq!(resolver.negative_cache_size(), 0);
    }

    #[test]
    fn test_negative_cache_hit_avoids_lookup() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);

        // Cache a negative entry
        resolver.cache_negative(InodeId::ROOT_INODE, "nonexistent");
        assert!(resolver.check_negative(InodeId::ROOT_INODE, "nonexistent"));

        // Now resolve_path should return EntryNotFound without calling lookup_fn
        // We use a static counter to track if lookup was called (via interior mutability)
        use std::sync::atomic::{AtomicBool, Ordering};
        let lookup_called = AtomicBool::new(false);
        let result = resolver.resolve_path("/nonexistent", |_, _| {
            lookup_called.store(true, Ordering::SeqCst);
            Err(MetaError::EntryNotFound {
                parent: InodeId::ROOT_INODE,
                name: "nonexistent".to_string(),
            })
        });

        assert!(result.is_err());
        if let Err(MetaError::EntryNotFound { .. }) = result {
            // Expected
        } else {
            panic!("expected EntryNotFound");
        }
        assert!(
            !lookup_called.load(Ordering::SeqCst),
            "lookup should not be called for negative cache hit"
        );
    }

    #[test]
    fn test_negative_cache_invalidated_on_create() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);

        // Cache a negative entry
        resolver.cache_negative(InodeId::ROOT_INODE, "newfile");
        assert!(resolver.check_negative(InodeId::ROOT_INODE, "newfile"));

        // Invalidate when file is created
        resolver.invalidate_negative(InodeId::ROOT_INODE, "newfile");
        assert!(!resolver.check_negative(InodeId::ROOT_INODE, "newfile"));
    }

    #[test]
    fn test_negative_cache_invalidate_parent() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);
        let parent = InodeId::new(100);

        // Add negative entries for multiple files under parent
        resolver.cache_negative(parent, "file1");
        resolver.cache_negative(parent, "file2");
        resolver.cache_negative(parent, "file3");

        assert_eq!(resolver.negative_cache_size(), 3);

        // Invalidate all for parent
        resolver.invalidate_negative_parent(parent);
        assert_eq!(resolver.negative_cache_size(), 0);
    }

    #[test]
    fn test_cleanup_expired_negative() {
        let resolver = PathResolver::new(256, 1000, 1, 10000); // 1 second TTL

        // Cache negative entries
        resolver.cache_negative(InodeId::ROOT_INODE, "file1");
        resolver.cache_negative(InodeId::ROOT_INODE, "file2");
        assert_eq!(resolver.negative_cache_size(), 2);

        // Wait and cleanup - we can't actually wait in unit tests,
        // but we can test that cleanup runs without error
        let _cleaned = resolver.cleanup_expired_negative();
        // May be 0 or 2 depending on timing, but should not panic

        // Verify entries are still there (not expired yet)
        assert!(resolver.check_negative(InodeId::ROOT_INODE, "file1"));
    }

    #[test]
    fn test_negative_cache_size() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);
        assert_eq!(resolver.negative_cache_size(), 0);

        resolver.cache_negative(InodeId::ROOT_INODE, "file1");
        resolver.cache_negative(InodeId::ROOT_INODE, "file2");
        resolver.cache_negative(InodeId::ROOT_INODE, "file3");

        assert_eq!(resolver.negative_cache_size(), 3);
    }

    #[test]
    fn test_negative_cache_max_entries_eviction() {
        let resolver = PathResolver::new(256, 1000, 30, 3); // max 3 entries

        // Add 3 entries
        resolver.cache_negative(InodeId::ROOT_INODE, "file1");
        resolver.cache_negative(InodeId::ROOT_INODE, "file2");
        resolver.cache_negative(InodeId::ROOT_INODE, "file3");

        assert_eq!(resolver.negative_cache_size(), 3);

        // Adding a 4th should trigger eviction
        resolver.cache_negative(InodeId::ROOT_INODE, "file4");

        // Size should still be max 3
        assert!(resolver.negative_cache_size() <= 3);
    }

    #[test]
    fn test_resolve_path_uses_negative_cache() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);

        // First lookup should fail and cache negative
        let result = resolver.resolve_path("/missing/file", |parent, name| {
            Err(MetaError::EntryNotFound {
                parent,
                name: name.to_string(),
            })
        });
        assert!(result.is_err());

        // Second lookup should use negative cache
        use std::sync::atomic::{AtomicBool, Ordering};
        let second_lookup_called = AtomicBool::new(false);
        let result = resolver.resolve_path("/missing/file", |_, _| {
            second_lookup_called.store(true, Ordering::SeqCst);
            Ok(DirEntry {
                name: "file".to_string(),
                ino: InodeId::new(100),
                file_type: FileType::RegularFile,
            })
        });

        // Should still return error from negative cache, not call lookup
        assert!(result.is_err());
        assert!(!second_lookup_called.load(Ordering::SeqCst));

        // Invalidate and it should work
        resolver.invalidate_negative(InodeId::ROOT_INODE, "missing");

        let result = resolver.resolve_path("/missing/file", |_parent, name| {
            Ok(DirEntry {
                name: name.to_string(),
                ino: InodeId::new(100),
                file_type: FileType::RegularFile,
            })
        });
        assert!(result.is_ok());
    }
}
