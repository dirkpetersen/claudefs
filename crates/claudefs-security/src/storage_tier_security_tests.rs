//! Storage S3 tier and tiering policy security tests.
//!
//! Part of A10 Phase 30

#[cfg(test)]
mod tests {
    use claudefs_storage::s3_tier::{
        MockObjectStore, MockObjectStoreStats, ObjectStoreBackend, S3KeyBuilder,
        TieringConfig, TieringEngine, TieringMode, TieringStats,
    };
    use claudefs_storage::tiering_policy::{
        AccessPattern, AccessRecord, TierClass, TierOverridePolicy,
        TieringDecision, TieringPolicyConfig, TieringPolicyEngine, TieringPolicyStats,
    };
    use std::collections::HashMap;

    mod object_store_security {
        use super::*;

        #[tokio::test]
        async fn test_stor_tier_sec_get_nonexistent_returns_error() {
            let store = MockObjectStore::new();
            let result = store.get_segment(999).await;
            assert!(result.is_err(), "Getting non-existent segment should return error");
        }

        #[tokio::test]
        async fn test_stor_tier_sec_delete_nonexistent_succeeds_noop() {
            let store = MockObjectStore::new();
            let result = store.delete_segment(999).await;
            assert!(result.is_ok(), "Deleting non-existent segment should succeed as no-op");
        }

        #[tokio::test]
        async fn test_stor_tier_sec_put_overwrite_changes_data() {
            let store = MockObjectStore::new();
            store.put_segment(1, vec![1, 2, 3]).await.unwrap();
            store.put_segment(1, vec![4, 5, 6, 7]).await.unwrap();
            let data = store.get_segment(1).await.unwrap();
            assert_eq!(data, vec![4, 5, 6, 7], "Overwrite should change data");
        }

        #[tokio::test]
        async fn test_stor_tier_sec_list_returns_sorted_ids() {
            let store = MockObjectStore::new();
            store.put_segment(30, vec![]).await.unwrap();
            store.put_segment(10, vec![]).await.unwrap();
            store.put_segment(20, vec![]).await.unwrap();
            store.put_segment(5, vec![]).await.unwrap();
            let ids = store.list_segments().await.unwrap();
            assert_eq!(ids, vec![5, 10, 20, 30], "List should return sorted IDs");
        }

        #[tokio::test]
        async fn test_stor_tier_sec_stats_track_all_operations() {
            let store = MockObjectStore::new();
            store.put_segment(1, vec![1, 2, 3]).await.unwrap();
            store.put_segment(2, vec![4, 5]).await.unwrap();
            store.get_segment(1).await.unwrap();
            store.exists(1).await.unwrap();
            store.list_segments().await.unwrap();
            store.delete_segment(1).await.unwrap();
            let stats = store.stats();
            assert_eq!(stats.puts, 2, "Should track 2 puts");
            assert_eq!(stats.gets, 1, "Should track 1 get");
            assert_eq!(stats.deletes, 1, "Should track 1 delete");
            assert_eq!(stats.exists_checks, 1, "Should track 1 exists check");
            assert_eq!(stats.list_calls, 1, "Should track 1 list call");
            assert_eq!(stats.total_bytes_stored, 2, "Should track remaining bytes (2 from segment 2)");
        }
    }

    mod tiering_engine_security {
        use super::*;

        #[tokio::test]
        async fn test_stor_tier_sec_upload_verify_succeeds() {
            let store = MockObjectStore::new();
            let config = TieringConfig {
                verify_after_upload: true,
                ..Default::default()
            };
            let engine = TieringEngine::new(config, store);
            let data = vec![9u8; 512];
            let result = engine.upload_segment(100, data.clone()).await.unwrap();
            assert!(result, "Upload with verify should succeed");
            let stats = engine.stats();
            assert_eq!(stats.segments_uploaded, 1);
            assert_eq!(stats.bytes_uploaded, 512);
        }

        #[tokio::test]
        async fn test_stor_tier_sec_download_nonexistent_returns_error() {
            let store = MockObjectStore::new();
            let config = TieringConfig::default();
            let engine = TieringEngine::new(config, store);
            let result = engine.download_segment(999).await;
            assert!(result.is_err(), "Downloading non-existent segment should return error");
        }

        #[tokio::test]
        async fn test_stor_tier_sec_eviction_batch_partial_success() {
            let store = MockObjectStore::new();
            let config = TieringConfig {
                verify_after_upload: false,
                ..Default::default()
            };
            let engine = TieringEngine::new(config, store);
            let mut segment_data = HashMap::new();
            segment_data.insert(1, vec![1u8; 100]);
            segment_data.insert(3, vec![3u8; 300]);
            let segment_ids = vec![1, 2, 3];
            let successful = engine.process_eviction_batch(&segment_ids, &segment_data).await;
            assert_eq!(successful.len(), 2, "Only 2 of 3 should succeed (segment 2 missing)");
            assert!(successful.contains(&1));
            assert!(successful.contains(&3));
            assert!(!successful.contains(&2));
        }

        #[test]
        fn test_stor_tier_sec_tiering_stats_initially_zero() {
            let stats = TieringStats::default();
            assert_eq!(stats.segments_uploaded, 0);
            assert_eq!(stats.segments_downloaded, 0);
            assert_eq!(stats.segments_deleted, 0);
            assert_eq!(stats.bytes_uploaded, 0);
            assert_eq!(stats.bytes_downloaded, 0);
            assert_eq!(stats.upload_errors, 0);
            assert_eq!(stats.download_errors, 0);
        }

        #[test]
        fn test_stor_tier_sec_tiering_mode_variants_comparison() {
            assert_eq!(TieringMode::Cache, TieringMode::Cache);
            assert_eq!(TieringMode::Tiered, TieringMode::Tiered);
            assert_eq!(TieringMode::Disabled, TieringMode::Disabled);
            assert_ne!(TieringMode::Cache, TieringMode::Tiered);
            assert_ne!(TieringMode::Tiered, TieringMode::Disabled);
            assert_ne!(TieringMode::Cache, TieringMode::Disabled);
        }
    }

    mod s3_key_builder_security {
        use super::*;

        #[test]
        fn test_stor_tier_sec_key_builder_empty_prefix() {
            let builder = S3KeyBuilder::new(String::new());
            let key = builder.segment_key(123);
            assert_eq!(key, "123");
            let parsed = builder.parse_segment_id("456");
            assert_eq!(parsed, Some(456));
        }

        #[test]
        fn test_stor_tier_sec_parse_invalid_suffix_returns_none() {
            let builder = S3KeyBuilder::new("segments/".to_string());
            let parsed = builder.parse_segment_id("segments/abc");
            assert_eq!(parsed, None, "Non-numeric suffix should return None");
        }

        #[test]
        fn test_stor_tier_sec_parse_wrong_prefix_returns_none() {
            let builder = S3KeyBuilder::new("segments/".to_string());
            let parsed = builder.parse_segment_id("other/123");
            assert_eq!(parsed, None, "Wrong prefix should return None");
        }
    }

    mod policy_classification_edge_cases {
        use super::*;

        #[test]
        fn test_stor_tier_sec_classify_unknown_returns_cold() {
            let config = TieringPolicyConfig::default();
            let engine = TieringPolicyEngine::new(config);
            let class = engine.classify_segment(999, 1000);
            assert_eq!(class, TierClass::Cold, "Unknown segment should be Cold");
        }

        #[test]
        fn test_stor_tier_sec_classify_exact_hot_threshold() {
            let mut config = TieringPolicyConfig::default();
            config.hot_threshold = 100;
            let mut engine = TieringPolicyEngine::new(config);
            engine.register_segment(1, 4096, 0);
            for _ in 0..100 {
                engine.record_access(1, 1024, true, false, 100);
            }
            let class = engine.classify_segment(1, 200);
            assert_eq!(class, TierClass::Hot, "Exact hot_threshold access should be Hot");
        }

        #[test]
        fn test_stor_tier_sec_classify_frozen_by_age() {
            let mut config = TieringPolicyConfig::default();
            config.frozen_after_secs = 100;
            let mut engine = TieringPolicyEngine::new(config);
            engine.register_segment(1, 4096, 0);
            engine.record_access(1, 1024, true, false, 50);
            for _ in 0..200 {
                engine.record_access(1, 1024, true, false, 100);
            }
            let class = engine.classify_segment(1, 300);
            assert_eq!(class, TierClass::Frozen, "Age > frozen_after_secs should be Frozen");
        }

        #[test]
        fn test_stor_tier_sec_classify_warm_between_thresholds() {
            let mut config = TieringPolicyConfig::default();
            config.hot_threshold = 100;
            config.warm_threshold = 10;
            let mut engine = TieringPolicyEngine::new(config);
            engine.register_segment(1, 4096, 0);
            for _ in 0..50 {
                engine.record_access(1, 1024, true, false, 100);
            }
            let class = engine.classify_segment(1, 200);
            assert_eq!(class, TierClass::Warm, "Between warm and hot threshold should be Warm");
        }

        #[test]
        fn test_stor_tier_sec_frozen_overrides_hot() {
            let mut config = TieringPolicyConfig::default();
            config.frozen_after_secs = 100;
            config.hot_threshold = 10;
            let mut engine = TieringPolicyEngine::new(config);
            engine.register_segment(1, 4096, 0);
            for _ in 0..200 {
                engine.record_access(1, 1024, true, false, 100);
            }
            let class = engine.classify_segment(1, 500);
            assert_eq!(class, TierClass::Frozen, "Frozen check comes first, should override hot");
        }
    }

    mod pattern_detection_security {
        use super::*;

        #[test]
        fn test_stor_tier_sec_pattern_unknown_segment() {
            let config = TieringPolicyConfig::default();
            let engine = TieringPolicyEngine::new(config);
            let pattern = engine.detect_pattern(999);
            assert_eq!(pattern, AccessPattern::Unknown, "Unknown segment should be Unknown");
        }

        #[test]
        fn test_stor_tier_sec_pattern_single_write() {
            let config = TieringPolicyConfig::default();
            let mut engine = TieringPolicyEngine::new(config);
            engine.register_segment(1, 4096, 0);
            engine.record_access(1, 4096, true, true, 100);
            let pattern = engine.detect_pattern(1);
            assert_eq!(pattern, AccessPattern::WriteOnceReadMany, "Single write should be WriteOnceReadMany");
        }

        #[test]
        fn test_stor_tier_sec_pattern_single_read() {
            let config = TieringPolicyConfig::default();
            let mut engine = TieringPolicyEngine::new(config);
            engine.register_segment(1, 4096, 0);
            engine.record_access(1, 4096, true, false, 100);
            let pattern = engine.detect_pattern(1);
            assert_eq!(pattern, AccessPattern::ReadOnce, "Single read should be ReadOnce");
        }

        #[test]
        fn test_stor_tier_sec_pattern_sequential_reads() {
            let config = TieringPolicyConfig::default();
            let mut engine = TieringPolicyEngine::new(config);
            engine.register_segment(1, 4096, 0);
            for _ in 0..10 {
                engine.record_access(1, 4096, true, false, 100);
            }
            let pattern = engine.detect_pattern(1);
            assert_eq!(pattern, AccessPattern::Sequential, "10+ sequential reads should be Sequential");
        }

        #[test]
        fn test_stor_tier_sec_pattern_random_reads() {
            let config = TieringPolicyConfig::default();
            let mut engine = TieringPolicyEngine::new(config);
            engine.register_segment(1, 4096, 0);
            for _ in 0..10 {
                engine.record_access(1, 4096, false, false, 100);
            }
            let pattern = engine.detect_pattern(1);
            assert_eq!(pattern, AccessPattern::Random, "10+ random reads should be Random");
        }
    }

    mod override_and_decision_security {
        use super::*;

        #[test]
        fn test_stor_tier_sec_pinflash_override_returns_hot() {
            let config = TieringPolicyConfig::default();
            let mut engine = TieringPolicyEngine::new(config);
            engine.register_segment(1, 4096, 0);
            engine.set_override(1, TierOverridePolicy::PinFlash);
            let decision = engine.make_decision(1, 1000);
            assert_eq!(decision.recommended_tier, TierClass::Hot, "PinFlash should always return Hot");
        }

        #[test]
        fn test_stor_tier_sec_forces3_override_returns_cold() {
            let config = TieringPolicyConfig::default();
            let mut engine = TieringPolicyEngine::new(config);
            engine.register_segment(1, 4096, 0);
            for _ in 0..200 {
                engine.record_access(1, 1024, true, false, 100);
            }
            engine.set_override(1, TierOverridePolicy::ForceS3);
            let decision = engine.make_decision(1, 1000);
            assert_eq!(decision.recommended_tier, TierClass::Cold, "ForceS3 should always return Cold");
        }

        #[test]
        fn test_stor_tier_sec_auto_override_uses_classification() {
            let mut config = TieringPolicyConfig::default();
            config.hot_threshold = 10;
            let mut engine = TieringPolicyEngine::new(config);
            engine.register_segment(1, 4096, 0);
            for _ in 0..50 {
                engine.record_access(1, 1024, true, false, 100);
            }
            let decision = engine.make_decision(1, 200);
            assert_eq!(decision.override_policy, TierOverridePolicy::Auto);
            assert_eq!(decision.recommended_tier, TierClass::Hot, "Auto should use classification");
        }

        #[test]
        fn test_stor_tier_sec_eviction_skips_pinflash() {
            let config = TieringPolicyConfig::default();
            let mut engine = TieringPolicyEngine::new(config);
            engine.register_segment(1, 4096, 0);
            engine.register_segment(2, 4096, 0);
            engine.set_override(1, TierOverridePolicy::PinFlash);
            let candidates = engine.get_eviction_candidates(1000, 10);
            assert!(!candidates.iter().any(|d| d.segment_id == 1), "PinFlash should be skipped");
            assert!(candidates.iter().any(|d| d.segment_id == 2), "Auto segment should be included");
        }

        #[test]
        fn test_stor_tier_sec_eviction_sorted_by_score_desc() {
            let mut config = TieringPolicyConfig::default();
            config.recency_weight = 1.0;
            config.size_weight = 0.5;
            config.frequency_weight = 0.1;
            let mut engine = TieringPolicyEngine::new(config);
            engine.register_segment(1, 4096, 100);
            engine.register_segment(2, 1024 * 1024, 100);
            engine.register_segment(3, 4096, 100);
            engine.record_access(1, 1024, true, false, 200);
            let candidates = engine.get_eviction_candidates(10000, 3);
            assert_eq!(candidates.len(), 3);
            for i in 1..candidates.len() {
                assert!(
                    candidates[i - 1].score >= candidates[i].score,
                    "Candidates should be sorted by score descending"
                );
            }
        }
    }
}