//! Tests for PathResolver in claudefs-fuse

use claudefs_fuse::path_resolver::{
    PathResolveError, PathResolver, PathResolverConfig, PathResolverStats, ResolvedComponent,
    ResolvedPath,
};
use std::time::Instant;

fn make_resolved(path: &str, ino: u64, gen: u64) -> ResolvedPath {
    ResolvedPath {
        path: path.to_string(),
        components: vec![ResolvedComponent {
            name: path.to_string(),
            ino,
            parent_ino: 1,
            generation: gen,
        }],
        final_ino: ino,
        resolved_at: Instant::now(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_max_depth() {
        let config = PathResolverConfig::default();
        assert_eq!(config.max_depth, 64);
    }

    #[test]
    fn test_default_config_cache_capacity() {
        let config = PathResolverConfig::default();
        assert_eq!(config.cache_capacity, 1000);
    }

    #[test]
    fn test_default_config_ttl() {
        let config = PathResolverConfig::default();
        assert_eq!(config.ttl.as_secs(), 30);
    }

    #[test]
    fn test_custom_config() {
        let config = PathResolverConfig {
            max_depth: 32,
            cache_capacity: 500,
            ttl: std::time::Duration::from_secs(60),
        };
        assert_eq!(config.max_depth, 32);
        assert_eq!(config.cache_capacity, 500);
        assert_eq!(config.ttl.as_secs(), 60);
    }

    #[test]
    fn test_generation_tracker_new() {
        let tracker = claudefs_fuse::path_resolver::GenerationTracker::new();
        assert_eq!(tracker.get(999), 0);
    }

    #[test]
    fn test_generation_tracker_get_unknown() {
        let tracker = claudefs_fuse::path_resolver::GenerationTracker::new();
        assert_eq!(tracker.get(12345), 0);
    }

    #[test]
    fn test_generation_tracker_bump() {
        let mut tracker = claudefs_fuse::path_resolver::GenerationTracker::new();
        let gen = tracker.bump(1);
        assert_eq!(gen, 1);
        assert_eq!(tracker.get(1), 1);
    }

    #[test]
    fn test_generation_tracker_bump_multiple() {
        let mut tracker = claudefs_fuse::path_resolver::GenerationTracker::new();
        tracker.bump(1);
        let gen = tracker.bump(1);
        assert_eq!(gen, 2);
    }

    #[test]
    fn test_generation_tracker_set_get() {
        let mut tracker = claudefs_fuse::path_resolver::GenerationTracker::new();
        tracker.set(1, 100);
        assert_eq!(tracker.get(1), 100);
    }

    #[test]
    fn test_generation_tracker_remove() {
        let mut tracker = claudefs_fuse::path_resolver::GenerationTracker::new();
        tracker.set(1, 100);
        tracker.remove(1);
        assert_eq!(tracker.get(1), 0);
    }

    #[test]
    fn test_resolved_path_is_stale_not_in_tracker() {
        let mut tracker = claudefs_fuse::path_resolver::GenerationTracker::new();
        let resolved = make_resolved("a/b", 2, 1);
        tracker.set(2, 1);
        assert!(!resolved.is_stale(&tracker));
    }

    #[test]
    fn test_resolved_path_is_stale_when_bumped() {
        let mut tracker = claudefs_fuse::path_resolver::GenerationTracker::new();
        let resolved = make_resolved("a/b", 2, 1);
        tracker.set(2, 1);
        tracker.bump(2);
        assert!(resolved.is_stale(&tracker));
    }

    #[test]
    fn test_resolved_path_depth_single() {
        let resolved = make_resolved("a", 1, 1);
        assert_eq!(resolved.depth(), 1);
    }

    #[test]
    fn test_resolved_path_depth_multiple() {
        let resolved = ResolvedPath {
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
        assert_eq!(resolved.depth(), 3);
    }

    #[test]
    fn test_validate_path_valid_simple() {
        let result = PathResolver::validate_path("foo");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec!["foo"]);
    }

    #[test]
    fn test_validate_path_valid_multiple() {
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
    fn test_validate_path_dotdot() {
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
    fn test_validate_path_trailing_slash() {
        let result = PathResolver::validate_path("a/");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_path_single_component() {
        let result = PathResolver::validate_path("file");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec!["file"]);
    }

    #[test]
    fn test_validate_path_multiple_components() {
        let result = PathResolver::validate_path("dir/subdir/file");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec!["dir", "subdir", "file"]);
    }

    #[test]
    fn test_validate_path_whitespace() {
        let result = PathResolver::validate_path("hello world");
        assert!(result.is_ok());
    }

    #[test]
    fn test_insert_and_lookup_cache_hit() {
        let mut resolver = PathResolver::new(PathResolverConfig::default());
        resolver.insert("a/b", make_resolved("a/b", 2, 1));

        let result = resolver.lookup("a/b");
        assert!(result.is_some());
        assert_eq!(resolver.stats().cache_hits, 1);
    }

    #[test]
    fn test_insert_and_lookup_cache_miss() {
        let mut resolver = PathResolver::new(PathResolverConfig::default());

        let result = resolver.lookup("nonexistent");
        assert!(result.is_none());
        assert_eq!(resolver.stats().cache_misses, 1);
    }

    #[test]
    fn test_lookup_stale_after_bump() {
        let mut resolver = PathResolver::new(PathResolverConfig::default());
        resolver.insert("a/b", make_resolved("a/b", 2, 1));

        resolver.bump_generation(2);

        let result = resolver.lookup("a/b");
        assert!(result.is_none());
        assert_eq!(resolver.stats().stale_hits, 1);
    }

    #[test]
    fn test_invalidate_prefix_exact_match() {
        let mut resolver = PathResolver::new(PathResolverConfig::default());
        resolver.insert("a", make_resolved("a", 1, 1));

        resolver.invalidate_prefix("a");

        assert!(resolver.lookup("a").is_none());
        assert_eq!(resolver.stats().invalidations, 1);
    }

    #[test]
    fn test_invalidate_prefix_sub_paths() {
        let mut resolver = PathResolver::new(PathResolverConfig::default());
        resolver.insert("a/b", make_resolved("a/b", 2, 1));
        resolver.insert("a/c", make_resolved("a/c", 3, 1));
        resolver.insert("x/y", make_resolved("x/y", 4, 1));

        resolver.invalidate_prefix("a");

        assert!(resolver.lookup("a/b").is_none());
        assert!(resolver.lookup("a/c").is_none());
        assert!(resolver.lookup("x/y").is_some());
    }

    #[test]
    fn test_invalidate_prefix_preserves_unrelated() {
        let mut resolver = PathResolver::new(PathResolverConfig::default());
        resolver.insert("a/b", make_resolved("a/b", 2, 1));
        resolver.insert("x/y", make_resolved("x/y", 3, 1));

        resolver.invalidate_prefix("a");

        assert!(resolver.lookup("x/y").is_some());
    }

    #[test]
    fn test_invalidate_prefix_stats_incremented() {
        let mut resolver = PathResolver::new(PathResolverConfig::default());
        resolver.insert("a/b", make_resolved("a/b", 2, 1));
        resolver.insert("a/c", make_resolved("a/c", 3, 1));

        resolver.invalidate_prefix("a");

        assert_eq!(resolver.stats().invalidations, 2);
    }

    #[test]
    fn test_bump_generation_updates() {
        let mut resolver = PathResolver::new(PathResolverConfig::default());

        let gen = resolver.bump_generation(1);
        assert_eq!(gen, 1);
    }

    #[test]
    fn test_is_generation_current_before_bump() {
        let resolver = PathResolver::new(PathResolverConfig::default());
        assert!(resolver.is_generation_current(1, 0));
    }

    #[test]
    fn test_is_generation_current_after_bump() {
        let mut resolver = PathResolver::new(PathResolverConfig::default());
        resolver.bump_generation(1);
        assert!(!resolver.is_generation_current(1, 0));
        assert!(resolver.is_generation_current(1, 1));
    }

    #[test]
    fn test_stats_initial_zeros() {
        let resolver = PathResolver::new(PathResolverConfig::default());
        let stats = resolver.stats();
        assert_eq!(stats.cache_hits, 0);
        assert_eq!(stats.cache_misses, 0);
        assert_eq!(stats.stale_hits, 0);
        assert_eq!(stats.toctou_detected, 0);
        assert_eq!(stats.invalidations, 0);
    }

    #[test]
    fn test_stats_hits_and_misses() {
        let mut resolver = PathResolver::new(PathResolverConfig::default());

        resolver.lookup("a");
        assert_eq!(resolver.stats().cache_misses, 1);

        resolver.insert("b", make_resolved("b", 2, 1));
        resolver.lookup("b");
        assert_eq!(resolver.stats().cache_hits, 1);
    }

    #[test]
    fn test_cache_capacity_eviction() {
        let mut resolver = PathResolver::new(PathResolverConfig {
            cache_capacity: 2,
            ..Default::default()
        });

        resolver.insert("a", make_resolved("a", 1, 1));
        resolver.insert("b", make_resolved("b", 2, 1));
        resolver.insert("c", make_resolved("c", 3, 1));

        resolver.lookup("a");
        resolver.lookup("b");
        resolver.lookup("c");

        let stats = resolver.stats();
        assert!(stats.cache_misses >= 1);
    }

    #[test]
    fn test_record_component_no_panic() {
        let mut resolver = PathResolver::new(PathResolverConfig::default());
        let component = ResolvedComponent {
            name: "test".to_string(),
            ino: 1,
            parent_ino: 0,
            generation: 1,
        };
        resolver.record_component(component);
    }
}

mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_validate_path_non_empty(s in "[a-zA-Z0-9_]+") {
            let result = PathResolver::validate_path(&s);
            assert!(result.is_ok());
        }

        #[test]
        fn prop_validate_path_no_slash_start(s in "[a-zA-Z0-9_]+") {
            let result = PathResolver::validate_path(&s);
            assert!(result.is_ok());
        }

        #[test]
        fn prop_validate_path_no_dotdot(s in "[a-zA-Z0-9_]+") {
            let result = PathResolver::validate_path(&s);
            assert!(result.is_ok());
        }

        #[test]
        fn prop_validate_path_complex(s in "[a-zA-Z0-9_]+/[a-zA-Z0-9_]+") {
            let result = PathResolver::validate_path(&s);
            assert!(result.is_ok());
        }
    }
}
