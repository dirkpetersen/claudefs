#![allow(dead_code)]

use crate::inode::InodeId;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::debug;

#[derive(Debug, Clone)]
pub struct ResolvedComponent {
    pub name: String,
    pub ino: InodeId,
    pub parent_ino: InodeId,
    pub generation: u64,
}

#[derive(Debug, Clone)]
pub struct ResolvedPath {
    pub path: String,
    pub components: Vec<ResolvedComponent>,
    pub final_ino: InodeId,
    pub resolved_at: Instant,
}

impl ResolvedPath {
    pub fn is_stale(&self, generations: &GenerationTracker) -> bool {
        for comp in &self.components {
            if generations.get(comp.ino) != comp.generation {
                return true;
            }
        }
        false
    }

    pub fn depth(&self) -> usize {
        self.components.len()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PathResolveError {
    #[error("Component not found: {name} in parent {parent}")]
    ComponentNotFound { name: String, parent: InodeId },
    #[error("Path too deep: depth {depth} exceeds limit {limit}")]
    TooDeep { depth: usize, limit: usize },
    #[error("Path component is stale (TOCTOU): {name}")]
    Stale { name: String },
    #[error("Invalid path: {reason}")]
    InvalidPath { reason: String },
}

pub type PathResolveResult<T> = Result<T, PathResolveError>;

#[derive(Debug, Clone)]
pub struct PathResolverConfig {
    pub max_depth: usize,
    pub cache_capacity: usize,
    pub ttl: Duration,
}

impl Default for PathResolverConfig {
    fn default() -> Self {
        Self {
            max_depth: 64,
            cache_capacity: 1024,
            ttl: Duration::from_secs(60),
        }
    }
}

pub struct GenerationTracker {
    generations: HashMap<InodeId, u64>,
}

impl GenerationTracker {
    pub fn new() -> Self {
        Self {
            generations: HashMap::new(),
        }
    }

    pub fn get(&self, ino: InodeId) -> u64 {
        self.generations.get(&ino).copied().unwrap_or(0)
    }

    pub fn bump(&mut self, ino: InodeId) -> u64 {
        let new_gen = self.generations.get(&ino).copied().unwrap_or(0) + 1;
        self.generations.insert(ino, new_gen);
        new_gen
    }

    pub fn set(&mut self, ino: InodeId, gen: u64) {
        self.generations.insert(ino, gen);
    }

    pub fn remove(&mut self, ino: InodeId) {
        self.generations.remove(&ino);
    }
}

impl Default for GenerationTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Default, Clone)]
pub struct PathResolverStats {
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub stale_hits: u64,
    pub toctou_detected: u64,
    pub invalidations: u64,
}

pub struct PathResolver {
    config: PathResolverConfig,
    cache: HashMap<String, ResolvedPath>,
    generations: GenerationTracker,
    stats: PathResolverStats,
}

impl PathResolver {
    pub fn new(config: PathResolverConfig) -> Self {
        Self {
            config,
            cache: HashMap::new(),
            generations: GenerationTracker::new(),
            stats: PathResolverStats::default(),
        }
    }

    pub fn insert(&mut self, path: &str, resolved: ResolvedPath) {
        if self.cache.len() >= self.config.cache_capacity {
            let keys: Vec<_> = self.cache.keys().cloned().collect();
            if let Some(first) = keys.first() {
                self.cache.remove(first);
            }
        }
        for comp in &resolved.components {
            if self.generations.get(comp.ino) == 0 {
                self.generations.set(comp.ino, comp.generation);
            }
        }
        self.cache.insert(path.to_string(), resolved);
        debug!("path_resolver: cached path {}", path);
    }

    pub fn lookup(&mut self, path: &str) -> Option<ResolvedPath> {
        if let Some(resolved) = self.cache.get(path).cloned() {
            if resolved.is_stale(&self.generations) {
                self.stats.stale_hits += 1;
                self.cache.remove(path);
                debug!("path_resolver: stale hit for {}", path);
                return None;
            }
            self.stats.cache_hits += 1;
            Some(resolved)
        } else {
            self.stats.cache_misses += 1;
            None
        }
    }

    pub fn record_component(&mut self, _component: ResolvedComponent) {}

    pub fn invalidate_prefix(&mut self, path_prefix: &str) {
        let prefix = if path_prefix.ends_with('/') {
            path_prefix.to_string()
        } else {
            format!("{}/", path_prefix)
        };
        let to_remove: Vec<String> = self
            .cache
            .keys()
            .filter(|k| *k == path_prefix || k.starts_with(&prefix))
            .cloned()
            .collect();
        let removed_count = to_remove.len();
        for k in to_remove {
            self.cache.remove(&k);
            self.stats.invalidations += 1;
        }
        if removed_count > 0 {
            debug!(
                "path_resolver: invalidated {} paths with prefix {}",
                removed_count, path_prefix
            );
        }
    }

    pub fn bump_generation(&mut self, ino: InodeId) -> u64 {
        let gen = self.generations.bump(ino);
        let new_gen = self.generations.get(ino);
        for resolved in self.cache.values_mut() {
            if resolved.final_ino == ino {
                for comp in &resolved.components {
                    if comp.ino == ino && comp.generation != new_gen {
                        self.stats.toctou_detected += 1;
                        break;
                    }
                }
            }
        }
        debug!("path_resolver: bumped generation of ino {} to {}", ino, gen);
        gen
    }

    pub fn is_generation_current(&self, ino: InodeId, gen: u64) -> bool {
        self.generations.get(ino) == gen
    }

    pub fn validate_path(path: &str) -> PathResolveResult<Vec<&str>> {
        if path.is_empty() {
            return Err(PathResolveError::InvalidPath {
                reason: "empty path".to_string(),
            });
        }
        if path.starts_with('/') {
            return Err(PathResolveError::InvalidPath {
                reason: "absolute path".to_string(),
            });
        }
        if path.contains("..") {
            return Err(PathResolveError::InvalidPath {
                reason: "contains ..".to_string(),
            });
        }
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if segments.is_empty() {
            return Err(PathResolveError::InvalidPath {
                reason: "empty path".to_string(),
            });
        }
        Ok(segments)
    }

    pub fn stats(&self) -> &PathResolverStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn test_validate_path_valid() {
        let result = PathResolver::validate_path("a/b/c");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec!["a", "b", "c"]);
    }

    #[test]
    fn test_validate_path_empty() {
        let result = PathResolver::validate_path("");
        assert!(result.is_err());
        match result.unwrap_err() {
            PathResolveError::InvalidPath { reason } => {
                assert!(reason.contains("empty"));
            }
            _ => panic!("expected InvalidPath"),
        }
    }

    #[test]
    fn test_validate_path_contains_dotdot() {
        let result = PathResolver::validate_path("a/../b");
        assert!(result.is_err());
        match result.unwrap_err() {
            PathResolveError::InvalidPath { reason } => {
                assert!(reason.contains(".."));
            }
            _ => panic!("expected InvalidPath"),
        }
    }

    #[test]
    fn test_validate_path_absolute() {
        let result = PathResolver::validate_path("/a/b");
        assert!(result.is_err());
        match result.unwrap_err() {
            PathResolveError::InvalidPath { reason } => {
                assert!(reason.contains("absolute"));
            }
            _ => panic!("expected InvalidPath"),
        }
    }

    #[test]
    fn test_validate_path_relative() {
        let result = PathResolver::validate_path("a/b");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_path_single_segment() {
        let result = PathResolver::validate_path("file.txt");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec!["file.txt"]);
    }

    #[test]
    fn test_generation_tracker_get_default() {
        let tracker = GenerationTracker::new();
        assert_eq!(tracker.get(999), 0);
    }

    #[test]
    fn test_generation_tracker_bump() {
        let mut tracker = GenerationTracker::new();
        let gen = tracker.bump(1);
        assert_eq!(gen, 1);
        assert_eq!(tracker.get(1), 1);
    }

    #[test]
    fn test_generation_tracker_bump_multiple() {
        let mut tracker = GenerationTracker::new();
        tracker.bump(1);
        let gen = tracker.bump(1);
        assert_eq!(gen, 2);
    }

    #[test]
    fn test_generation_tracker_set() {
        let mut tracker = GenerationTracker::new();
        tracker.set(1, 100);
        assert_eq!(tracker.get(1), 100);
    }

    #[test]
    fn test_generation_tracker_remove() {
        let mut tracker = GenerationTracker::new();
        tracker.set(1, 100);
        tracker.remove(1);
        assert_eq!(tracker.get(1), 0);
    }

    #[test]
    fn test_insert_and_lookup_cache() {
        let mut resolver = PathResolver::new(PathResolverConfig::default());
        let resolved = ResolvedPath {
            path: "a/b".to_string(),
            components: vec![ResolvedComponent {
                name: "b".to_string(),
                ino: 2,
                parent_ino: 1,
                generation: 1,
            }],
            final_ino: 2,
            resolved_at: Instant::now(),
        };
        resolver.insert("a/b", resolved);

        let result = resolver.lookup("a/b");
        assert!(result.is_some());
    }

    #[test]
    fn test_lookup_miss() {
        let mut resolver = PathResolver::new(PathResolverConfig::default());

        let result = resolver.lookup("nonexistent");
        assert!(result.is_none());
        assert_eq!(resolver.stats().cache_misses, 1);
    }

    #[test]
    fn test_stale_detection_via_bump_generation() {
        let mut resolver = PathResolver::new(PathResolverConfig::default());

        let resolved = ResolvedPath {
            path: "a/b".to_string(),
            components: vec![ResolvedComponent {
                name: "b".to_string(),
                ino: 2,
                parent_ino: 1,
                generation: 1,
            }],
            final_ino: 2,
            resolved_at: Instant::now(),
        };
        resolver.insert("a/b", resolved);

        resolver.bump_generation(2);

        let result = resolver.lookup("a/b");
        assert!(result.is_none());
        assert_eq!(resolver.stats().stale_hits, 1);
    }

    #[test]
    fn test_invalidate_prefix() {
        let mut resolver = PathResolver::new(PathResolverConfig::default());

        resolver.insert(
            "a/b",
            ResolvedPath {
                path: "a/b".to_string(),
                components: vec![],
                final_ino: 2,
                resolved_at: Instant::now(),
            },
        );
        resolver.insert(
            "a/c",
            ResolvedPath {
                path: "a/c".to_string(),
                components: vec![],
                final_ino: 3,
                resolved_at: Instant::now(),
            },
        );
        resolver.insert(
            "x/y",
            ResolvedPath {
                path: "x/y".to_string(),
                components: vec![],
                final_ino: 4,
                resolved_at: Instant::now(),
            },
        );

        resolver.invalidate_prefix("a");

        assert!(resolver.lookup("a/b").is_none());
        assert!(resolver.lookup("a/c").is_none());
        assert!(resolver.lookup("x/y").is_some());
    }

    #[test]
    fn test_invalidate_prefix_exact_match() {
        let mut resolver = PathResolver::new(PathResolverConfig::default());

        resolver.insert(
            "a",
            ResolvedPath {
                path: "a".to_string(),
                components: vec![],
                final_ino: 1,
                resolved_at: Instant::now(),
            },
        );

        resolver.invalidate_prefix("a");

        assert!(resolver.lookup("a").is_none());
    }

    #[test]
    fn test_record_component() {
        let mut resolver = PathResolver::new(PathResolverConfig::default());

        let component = ResolvedComponent {
            name: "test".to_string(),
            ino: 1,
            parent_ino: 0,
            generation: 1,
        };
        resolver.record_component(component);
    }

    #[test]
    fn test_stats_tracking() {
        let mut resolver = PathResolver::new(PathResolverConfig::default());

        resolver.lookup("a");
        assert_eq!(resolver.stats().cache_misses, 1);

        resolver.insert(
            "b",
            ResolvedPath {
                path: "b".to_string(),
                components: vec![],
                final_ino: 2,
                resolved_at: Instant::now(),
            },
        );
        resolver.lookup("b");
        assert_eq!(resolver.stats().cache_hits, 1);
    }

    #[test]
    fn test_default_config() {
        let config = PathResolverConfig::default();
        assert_eq!(config.max_depth, 64);
        assert_eq!(config.cache_capacity, 1024);
        assert_eq!(config.ttl, Duration::from_secs(60));
    }

    #[test]
    fn test_resolved_path_depth() {
        let path = ResolvedPath {
            path: "a/b/c".to_string(),
            components: vec![
                ResolvedComponent {
                    name: "a".to_string(),
                    ino: 1,
                    parent_ino: 0,
                    generation: 1,
                },
                ResolvedComponent {
                    name: "b".to_string(),
                    ino: 2,
                    parent_ino: 1,
                    generation: 1,
                },
                ResolvedComponent {
                    name: "c".to_string(),
                    ino: 3,
                    parent_ino: 2,
                    generation: 1,
                },
            ],
            final_ino: 3,
            resolved_at: Instant::now(),
        };
        assert_eq!(path.depth(), 3);
    }

    #[test]
    fn test_is_generation_current() {
        let resolver = PathResolver::new(PathResolverConfig::default());

        assert!(resolver.is_generation_current(1, 0));
    }

    #[test]
    fn test_bump_generation_updates_tracker() {
        let mut resolver = PathResolver::new(PathResolverConfig::default());

        resolver.bump_generation(1);

        assert!(!resolver.is_generation_current(1, 0));
        assert!(resolver.is_generation_current(1, 1));
    }

    #[test]
    fn test_toctou_detected() {
        let mut resolver = PathResolver::new(PathResolverConfig::default());

        resolver.insert(
            "a/b",
            ResolvedPath {
                path: "a/b".to_string(),
                components: vec![ResolvedComponent {
                    name: "b".to_string(),
                    ino: 2,
                    parent_ino: 1,
                    generation: 1,
                }],
                final_ino: 2,
                resolved_at: Instant::now(),
            },
        );

        resolver.bump_generation(2);

        let _ = resolver.lookup("a/b");

        assert_eq!(resolver.stats().toctou_detected, 1);
    }

    #[test]
    fn test_validate_path_multiple_slashes() {
        let result = PathResolver::validate_path("a//b");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_path_trailing_slash() {
        let result = PathResolver::validate_path("a/");
        assert!(result.is_ok());
    }
}
