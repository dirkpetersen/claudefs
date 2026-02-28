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

/// Speculative path resolution with caching.
///
/// Maintains a cache of path-to-inode mappings for fast speculative lookups.
/// Cache keys are (parent_ino, component_name) pairs.
pub struct PathResolver {
    /// Number of shards for shard computation.
    num_shards: u16,
    /// Cache: (parent_ino, name) -> PathCacheEntry
    cache: RwLock<HashMap<(InodeId, String), PathCacheEntry>>,
    /// Maximum cache entries before eviction.
    max_entries: usize,
    /// LRU order for eviction.
    lru_order: RwLock<VecDeque<(InodeId, String)>>,
}

impl PathResolver {
    /// Create a new path resolver.
    pub fn new(num_shards: u16, max_entries: usize) -> Self {
        Self {
            num_shards,
            cache: RwLock::new(HashMap::new()),
            max_entries,
            lru_order: RwLock::new(VecDeque::new()),
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

    /// Resolve a full path sequentially using the provided lookup function.
    /// This is the fallback when speculation misses.
    /// `lookup_fn` takes (parent_ino, component_name) and returns Result<DirEntry, MetaError>.
    pub fn resolve_path<F>(&self, path: &str, lookup_fn: F) -> Result<InodeId, MetaError>
    where
        F: Fn(InodeId, &str) -> Result<DirEntry, MetaError>,
    {
        let components = Self::parse_path(path);
        let mut current_parent = InodeId::ROOT_INODE;

        for component in components {
            let key = (current_parent, component.to_string());

            let entry_opt = {
                let cache = self.cache.read().unwrap();
                cache.get(&key).cloned()
            };

            if let Some(cached) = entry_opt {
                current_parent = cached.ino;
            } else {
                let dir_entry = lookup_fn(current_parent, &component)?;
                let shard = dir_entry.ino.shard(self.num_shards);
                let cache_entry = PathCacheEntry {
                    ino: dir_entry.ino,
                    file_type: dir_entry.file_type,
                    shard,
                };
                self.cache_resolution(current_parent, &component, cache_entry);
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
        let resolver = PathResolver::new(256, 1000);
        let (resolved, remaining) = resolver.speculative_resolve("/home/user/file.txt");
        assert!(resolved.is_empty());
        assert_eq!(remaining, vec!["home", "user", "file.txt"]);
    }

    #[test]
    fn test_speculative_resolve_partial_cache() {
        let resolver = PathResolver::new(256, 1000);
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
        let resolver = PathResolver::new(256, 1000);
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
        let resolver = PathResolver::new(256, 1000);
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
        let resolver = PathResolver::new(256, 1000);
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
        let resolver = PathResolver::new(256, 1000);
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
        let resolver = PathResolver::new(256, 1000);
        let result = resolver.resolve_path("/nonexistent", |parent, name| {
            Err(MetaError::EntryNotFound {
                parent,
                name: name.to_string(),
            })
        });
        assert!(result.is_err());
    }
}
