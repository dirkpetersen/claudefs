//! FUSE workload classification security tests.

#[cfg(test)]
mod tests {
    use claudefs_fuse::workload_class::{
        AccessProfile, AdaptiveTuner, ClassificationResult, TuningHint, WorkloadClassifier,
        WorkloadSignature, WorkloadType,
    };

    #[test]
    fn test_fuse_wl_sec_workload_type_latency_sensitive_all() {
        assert!(WorkloadType::Interactive.is_latency_sensitive());
        assert!(WorkloadType::AiInference.is_latency_sensitive());
        assert!(WorkloadType::WebServing.is_latency_sensitive());
        assert!(WorkloadType::Database.is_latency_sensitive());
        assert!(!WorkloadType::AiTraining.is_latency_sensitive());
        assert!(!WorkloadType::Backup.is_latency_sensitive());
        assert!(!WorkloadType::Streaming.is_latency_sensitive());
        assert!(!WorkloadType::Unknown.is_latency_sensitive());
    }

    #[test]
    fn test_fuse_wl_sec_workload_type_throughput_oriented_all() {
        assert!(WorkloadType::AiTraining.is_throughput_oriented());
        assert!(WorkloadType::Backup.is_throughput_oriented());
        assert!(WorkloadType::Streaming.is_throughput_oriented());
        assert!(!WorkloadType::Interactive.is_throughput_oriented());
        assert!(!WorkloadType::AiInference.is_throughput_oriented());
        assert!(!WorkloadType::WebServing.is_throughput_oriented());
        assert!(!WorkloadType::Database.is_throughput_oriented());
        assert!(!WorkloadType::Unknown.is_throughput_oriented());
    }

    #[test]
    fn test_fuse_wl_sec_workload_type_read_ahead_values() {
        assert_eq!(WorkloadType::AiTraining.suggested_read_ahead_kb(), 2048);
        assert_eq!(WorkloadType::AiInference.suggested_read_ahead_kb(), 512);
        assert_eq!(WorkloadType::Backup.suggested_read_ahead_kb(), 4096);
        assert_eq!(WorkloadType::Streaming.suggested_read_ahead_kb(), 1024);
        assert_eq!(WorkloadType::Interactive.suggested_read_ahead_kb(), 64);
        assert_eq!(WorkloadType::WebServing.suggested_read_ahead_kb(), 128);
        assert_eq!(WorkloadType::Database.suggested_read_ahead_kb(), 256);
        assert_eq!(WorkloadType::Unknown.suggested_read_ahead_kb(), 128);
    }

    #[test]
    fn test_fuse_wl_sec_workload_type_no_overlap() {
        let all_types = [
            WorkloadType::AiTraining,
            WorkloadType::AiInference,
            WorkloadType::WebServing,
            WorkloadType::Database,
            WorkloadType::Backup,
            WorkloadType::Interactive,
            WorkloadType::Streaming,
            WorkloadType::Unknown,
        ];
        for wt in &all_types {
            assert!(
                !(wt.is_latency_sensitive() && wt.is_throughput_oriented()),
                "{:?} is both latency-sensitive and throughput-oriented",
                wt
            );
        }
    }

    #[test]
    fn test_fuse_wl_sec_unknown_neither_sensitive_nor_throughput() {
        assert!(!WorkloadType::Unknown.is_latency_sensitive());
        assert!(!WorkloadType::Unknown.is_throughput_oriented());
    }

    #[test]
    fn test_fuse_wl_sec_access_profile_new_zeros() {
        let profile = AccessProfile::new();
        assert_eq!(profile.read_bytes, 0);
        assert_eq!(profile.write_bytes, 0);
        assert_eq!(profile.read_ops, 0);
        assert_eq!(profile.write_ops, 0);
        assert_eq!(profile.sequential_reads, 0);
        assert_eq!(profile.random_reads, 0);
        assert_eq!(profile.avg_read_size_bytes, 0);
    }

    #[test]
    fn test_fuse_wl_sec_access_profile_rw_ratio_empty() {
        let profile = AccessProfile::new();
        assert!((profile.read_write_ratio() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_fuse_wl_sec_access_profile_seq_ratio_empty() {
        let profile = AccessProfile::new();
        assert!((profile.sequential_ratio() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_fuse_wl_sec_access_profile_is_read_heavy_edge_cases() {
        let mut profile = AccessProfile::new();
        assert!(!profile.is_read_heavy());
        profile.record_read(100, true);
        assert!(profile.is_read_heavy());
    }

    #[test]
    fn test_fuse_wl_sec_access_profile_avg_read_size() {
        let mut profile = AccessProfile::new();
        profile.record_read(4096, true);
        assert_eq!(profile.avg_read_size_bytes, 4096);
        profile.record_read(8192, true);
        assert_eq!(profile.avg_read_size_bytes, 6144);
        profile.record_read(12288, false);
        assert_eq!(profile.avg_read_size_bytes, 8192);
    }

    #[test]
    fn test_fuse_wl_sec_signature_empty_profile_zeros() {
        let profile = AccessProfile::new();
        let sig = WorkloadSignature::from_profile(&profile, 10.0);
        assert!((sig.read_write_ratio - 1.0).abs() < f64::EPSILON);
        assert!((sig.sequential_ratio - 0.0).abs() < f64::EPSILON);
        assert!((sig.avg_io_size_kb - 0.0).abs() < f64::EPSILON);
        assert!((sig.ops_per_second - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_fuse_wl_sec_signature_matches_ai_training() {
        let mut profile = AccessProfile::new();
        for _ in 0..100 {
            profile.record_read(262144, true);
        }
        let sig = WorkloadSignature::from_profile(&profile, 1.0);
        assert!(sig.matches_ai_training());
    }

    #[test]
    fn test_fuse_wl_sec_signature_matches_database() {
        let mut profile = AccessProfile::new();
        for _ in 0..100 {
            profile.record_read(8192, false);
        }
        let sig = WorkloadSignature::from_profile(&profile, 1.0);
        assert!(sig.matches_database());
    }

    #[test]
    fn test_fuse_wl_sec_signature_matches_backup() {
        let mut profile = AccessProfile::new();
        for _ in 0..100 {
            profile.record_write(262144);
        }
        let sig = WorkloadSignature::from_profile(&profile, 1.0);
        assert!(sig.matches_backup());
    }

    #[test]
    fn test_fuse_wl_sec_signature_zero_elapsed_no_panic() {
        let mut profile = AccessProfile::new();
        profile.record_read(4096, true);
        profile.record_write(8192);
        let sig = WorkloadSignature::from_profile(&profile, 0.0);
        assert!((sig.ops_per_second - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_fuse_wl_sec_classifier_insufficient_ops() {
        let classifier = WorkloadClassifier::new();
        let profile = AccessProfile::new();
        let result = classifier.classify(&profile, 1.0);
        assert_eq!(result.workload_type, WorkloadType::Unknown);
        assert!((result.confidence - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_fuse_wl_sec_classifier_ai_training_with_hints() {
        let classifier = WorkloadClassifier::new();
        let mut profile = AccessProfile::new();
        for _ in 0..150 {
            profile.record_read(262144, true);
        }
        let result = classifier.classify(&profile, 1.0);
        assert_eq!(result.workload_type, WorkloadType::AiTraining);
        assert!(result.hints.contains(&TuningHint::IncreaseReadAhead));
        assert!(result.hints.contains(&TuningHint::PrioritizeThroughput));
        assert!(result.hints.contains(&TuningHint::EnableCompression));
    }

    #[test]
    fn test_fuse_wl_sec_classifier_database_with_hints() {
        let classifier = WorkloadClassifier::new();
        let mut profile = AccessProfile::new();
        for _ in 0..150 {
            profile.record_read(8192, false);
        }
        let result = classifier.classify(&profile, 1.0);
        assert_eq!(result.workload_type, WorkloadType::Database);
        assert!(result.hints.contains(&TuningHint::UseDirectIo));
        assert!(result.hints.contains(&TuningHint::PrioritizeLatency));
        assert!(result.hints.contains(&TuningHint::DisableCompression));
    }

    #[test]
    fn test_fuse_wl_sec_classifier_backup_pattern() {
        let classifier = WorkloadClassifier::new();
        let mut profile = AccessProfile::new();
        for _ in 0..150 {
            profile.record_write(262144);
        }
        let result = classifier.classify(&profile, 1.0);
        assert_eq!(result.workload_type, WorkloadType::Backup);
    }

    #[test]
    fn test_fuse_wl_sec_classifier_mixed_pattern() {
        let classifier = WorkloadClassifier::new();
        let mut profile = AccessProfile::new();
        for _ in 0..150 {
            profile.record_read(131072, true);
        }
        let result = classifier.classify(&profile, 1.0);
        assert_eq!(result.workload_type, WorkloadType::Streaming);
        assert!((result.confidence - 0.70).abs() < f64::EPSILON);
    }

    #[test]
    fn test_fuse_wl_sec_tuner_new_empty() {
        let tuner = AdaptiveTuner::new();
        assert_eq!(tuner.tracked_inodes(), 0);
    }

    #[test]
    fn test_fuse_wl_sec_tuner_record_increases_tracking() {
        let mut tuner = AdaptiveTuner::new();
        tuner.record_read(1, 4096, true);
        assert_eq!(tuner.tracked_inodes(), 1);
        tuner.record_write(2, 8192);
        assert_eq!(tuner.tracked_inodes(), 2);
    }

    #[test]
    fn test_fuse_wl_sec_tuner_classify_unknown_inode() {
        let mut tuner = AdaptiveTuner::new();
        let result = tuner.classify_inode(999);
        assert_eq!(result.workload_type, WorkloadType::Unknown);
        assert!((result.confidence - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_fuse_wl_sec_tuner_evict_removes_data() {
        let mut tuner = AdaptiveTuner::new();
        tuner.record_read(1, 4096, true);
        assert_eq!(tuner.tracked_inodes(), 1);
        tuner.evict_inode(1);
        assert_eq!(tuner.tracked_inodes(), 0);
        assert_eq!(tuner.get_read_ahead_kb(1), 128);
    }

    #[test]
    fn test_fuse_wl_sec_result_high_confidence_threshold() {
        let result_high = ClassificationResult::new(WorkloadType::Unknown, 0.7);
        assert!(result_high.is_high_confidence());
        let result_low = ClassificationResult::new(WorkloadType::Unknown, 0.69);
        assert!(!result_low.is_high_confidence());
    }

    #[test]
    fn test_fuse_wl_sec_result_hints_accumulate() {
        let mut result = ClassificationResult::new(WorkloadType::Unknown, 0.5);
        result.add_hint(TuningHint::IncreaseReadAhead);
        result.add_hint(TuningHint::PrioritizeLatency);
        result.add_hint(TuningHint::EnableCompression);
        assert_eq!(result.hints.len(), 3);
        assert_eq!(result.hints[0], TuningHint::IncreaseReadAhead);
        assert_eq!(result.hints[1], TuningHint::PrioritizeLatency);
        assert_eq!(result.hints[2], TuningHint::EnableCompression);
    }

    #[test]
    fn test_fuse_wl_sec_tuner_default_read_ahead() {
        let tuner = AdaptiveTuner::new();
        assert_eq!(tuner.get_read_ahead_kb(12345), 128);
    }

    #[test]
    fn test_fuse_wl_sec_tuner_ai_training_read_ahead() {
        let mut tuner = AdaptiveTuner::new();
        for _ in 0..150 {
            tuner.record_read(1, 262144, true);
        }
        tuner.classify_inode(1);
        assert_eq!(tuner.get_read_ahead_kb(1), 2048);
    }
}
