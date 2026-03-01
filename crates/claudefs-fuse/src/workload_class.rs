use std::collections::HashMap;
use tracing::{debug, trace};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum WorkloadType {
    AiTraining,
    AiInference,
    WebServing,
    Database,
    Backup,
    Interactive,
    Streaming,
    Unknown,
}

impl WorkloadType {
    pub fn is_latency_sensitive(&self) -> bool {
        matches!(
            self,
            WorkloadType::Interactive
                | WorkloadType::AiInference
                | WorkloadType::WebServing
                | WorkloadType::Database
        )
    }

    pub fn is_throughput_oriented(&self) -> bool {
        matches!(
            self,
            WorkloadType::AiTraining | WorkloadType::Backup | WorkloadType::Streaming
        )
    }

    pub fn suggested_read_ahead_kb(&self) -> u64 {
        match self {
            WorkloadType::AiTraining => 2048,
            WorkloadType::AiInference => 512,
            WorkloadType::Backup => 4096,
            WorkloadType::Streaming => 1024,
            WorkloadType::Interactive => 64,
            WorkloadType::WebServing => 128,
            WorkloadType::Database => 256,
            WorkloadType::Unknown => 128,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AccessProfile {
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub read_ops: u64,
    pub write_ops: u64,
    pub sequential_reads: u64,
    pub random_reads: u64,
    pub avg_read_size_bytes: u64,
}

impl AccessProfile {
    pub fn new() -> Self {
        Self {
            read_bytes: 0,
            write_bytes: 0,
            read_ops: 0,
            write_ops: 0,
            sequential_reads: 0,
            random_reads: 0,
            avg_read_size_bytes: 0,
        }
    }

    pub fn record_read(&mut self, bytes: u64, is_sequential: bool) {
        self.read_bytes += bytes;
        self.read_ops += 1;
        if is_sequential {
            self.sequential_reads += 1;
        } else {
            self.random_reads += 1;
        }
        if self.read_ops > 0 {
            self.avg_read_size_bytes = self.read_bytes / self.read_ops;
        }
    }

    pub fn record_write(&mut self, bytes: u64) {
        self.write_bytes += bytes;
        self.write_ops += 1;
    }

    pub fn read_write_ratio(&self) -> f64 {
        let total = self.read_bytes + self.write_bytes;
        if total == 0 {
            1.0
        } else {
            self.read_bytes as f64 / total as f64
        }
    }

    pub fn sequential_ratio(&self) -> f64 {
        let total = self.sequential_reads + self.random_reads;
        if total == 0 {
            0.0
        } else {
            self.sequential_reads as f64 / total as f64
        }
    }

    pub fn total_ops(&self) -> u64 {
        self.read_ops + self.write_ops
    }

    pub fn is_read_heavy(&self) -> bool {
        if self.read_bytes == 0 && self.write_bytes == 0 {
            return false;
        }
        self.read_write_ratio() > 0.75
    }
}

impl Default for AccessProfile {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TuningHint {
    IncreaseReadAhead,
    DecreaseReadAhead,
    UseDirectIo,
    EnableCompression,
    DisableCompression,
    PrioritizeLatency,
    PrioritizeThroughput,
    IncreaseCache,
    ReduceCache,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkloadSignature {
    pub read_write_ratio: f64,
    pub sequential_ratio: f64,
    pub avg_io_size_kb: f64,
    pub ops_per_second: f64,
}

impl WorkloadSignature {
    pub fn from_profile(profile: &AccessProfile, elapsed_secs: f64) -> Self {
        let total_ops = profile.total_ops();
        let ops_per_second = if elapsed_secs > 0.0 {
            total_ops as f64 / elapsed_secs
        } else {
            0.0
        };

        let total_io = profile.read_bytes + profile.write_bytes;
        let avg_io_size_kb = if total_ops > 0 {
            (total_io as f64 / total_ops as f64) / 1024.0
        } else {
            0.0
        };

        Self {
            read_write_ratio: profile.read_write_ratio(),
            sequential_ratio: profile.sequential_ratio(),
            avg_io_size_kb,
            ops_per_second,
        }
    }

    pub fn matches_ai_training(&self) -> bool {
        self.sequential_ratio > 0.8 && self.avg_io_size_kb >= 256.0
    }

    pub fn matches_database(&self) -> bool {
        self.sequential_ratio < 0.3 && self.avg_io_size_kb < 16.0 && self.ops_per_second < 500.0
    }

    pub fn matches_backup(&self) -> bool {
        self.read_write_ratio < 0.1 && (self.sequential_ratio > 0.9 || self.sequential_ratio == 0.0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassificationResult {
    pub workload_type: WorkloadType,
    pub confidence: f64,
    pub hints: Vec<TuningHint>,
}

impl ClassificationResult {
    pub fn new(workload_type: WorkloadType, confidence: f64) -> Self {
        Self {
            workload_type,
            confidence,
            hints: Vec::new(),
        }
    }

    pub fn add_hint(&mut self, hint: TuningHint) {
        self.hints.push(hint);
    }

    pub fn is_high_confidence(&self) -> bool {
        self.confidence >= 0.7
    }
}

pub struct WorkloadClassifier {
    min_ops_for_classification: u64,
}

impl WorkloadClassifier {
    pub fn new() -> Self {
        Self {
            min_ops_for_classification: 100,
        }
    }

    pub fn classify(&self, profile: &AccessProfile, elapsed_secs: f64) -> ClassificationResult {
        let total_ops = profile.total_ops();
        trace!(total_ops, elapsed_secs, "classifying workload");

        if total_ops < self.min_ops_for_classification {
            debug!(
                total_ops,
                threshold = self.min_ops_for_classification,
                "insufficient ops for classification"
            );
            return ClassificationResult::new(WorkloadType::Unknown, 0.0);
        }

        let signature = WorkloadSignature::from_profile(profile, elapsed_secs);
        trace!(
            rwr = signature.read_write_ratio,
            seq = signature.sequential_ratio,
            avg_kb = signature.avg_io_size_kb,
            ops_sec = signature.ops_per_second,
            "workload signature"
        );

        if signature.matches_ai_training() {
            debug!("classified as AiTraining");
            let mut result = ClassificationResult::new(WorkloadType::AiTraining, 0.85);
            result.add_hint(TuningHint::IncreaseReadAhead);
            result.add_hint(TuningHint::PrioritizeThroughput);
            result.add_hint(TuningHint::EnableCompression);
            return result;
        }

        if signature.matches_database() {
            debug!("classified as Database");
            let mut result = ClassificationResult::new(WorkloadType::Database, 0.80);
            result.add_hint(TuningHint::UseDirectIo);
            result.add_hint(TuningHint::PrioritizeLatency);
            result.add_hint(TuningHint::DisableCompression);
            return result;
        }

        if signature.matches_backup() {
            debug!("classified as Backup");
            let mut result = ClassificationResult::new(WorkloadType::Backup, 0.75);
            result.add_hint(TuningHint::IncreaseReadAhead);
            result.add_hint(TuningHint::PrioritizeThroughput);
            return result;
        }

        if signature.read_write_ratio > 0.8 && signature.sequential_ratio > 0.6 {
            debug!("classified as Streaming");
            let mut result = ClassificationResult::new(WorkloadType::Streaming, 0.70);
            result.add_hint(TuningHint::IncreaseReadAhead);
            return result;
        }

        if signature.read_write_ratio > 0.7 && signature.ops_per_second > 1000.0 {
            debug!("classified as WebServing");
            let mut result = ClassificationResult::new(WorkloadType::WebServing, 0.65);
            result.add_hint(TuningHint::PrioritizeLatency);
            result.add_hint(TuningHint::IncreaseCache);
            return result;
        }

        debug!("classified as Unknown");
        ClassificationResult::new(WorkloadType::Unknown, 0.3)
    }
}

impl Default for WorkloadClassifier {
    fn default() -> Self {
        Self::new()
    }
}

pub struct AdaptiveTuner {
    policies: HashMap<u64, ClassificationResult>,
    classifier: WorkloadClassifier,
    profiles: HashMap<u64, AccessProfile>,
    window_start: std::time::Instant,
}

impl AdaptiveTuner {
    pub fn new() -> Self {
        Self {
            policies: HashMap::new(),
            classifier: WorkloadClassifier::new(),
            profiles: HashMap::new(),
            window_start: std::time::Instant::now(),
        }
    }

    pub fn record_read(&mut self, inode: u64, bytes: u64, is_sequential: bool) {
        trace!(inode, bytes, is_sequential, "recording read");
        let profile = self
            .profiles
            .entry(inode)
            .or_insert_with(AccessProfile::new);
        profile.record_read(bytes, is_sequential);
    }

    pub fn record_write(&mut self, inode: u64, bytes: u64) {
        trace!(inode, bytes, "recording write");
        let profile = self
            .profiles
            .entry(inode)
            .or_insert_with(AccessProfile::new);
        profile.record_write(bytes);
    }

    pub fn classify_inode(&mut self, inode: u64) -> &ClassificationResult {
        let elapsed = self.window_start.elapsed().as_secs_f64();
        let profile = match self.profiles.get(&inode) {
            Some(p) => p,
            None => {
                let unknown = ClassificationResult::new(WorkloadType::Unknown, 0.0);
                return self.policies.entry(inode).or_insert(unknown);
            }
        };

        let result = self.classifier.classify(profile, elapsed);
        trace!(inode, ?result.workload_type, confidence = result.confidence, "classification complete");
        self.policies.entry(inode).or_insert(result)
    }

    pub fn get_read_ahead_kb(&self, inode: u64) -> u64 {
        self.policies
            .get(&inode)
            .map(|r| r.workload_type.suggested_read_ahead_kb())
            .unwrap_or(128)
    }

    pub fn tracked_inodes(&self) -> usize {
        self.profiles.len()
    }

    pub fn evict_inode(&mut self, inode: u64) {
        trace!(inode, "evicting inode");
        self.policies.remove(&inode);
        self.profiles.remove(&inode);
    }
}

impl Default for AdaptiveTuner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workload_type_latency_sensitive() {
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
    fn test_workload_type_throughput_oriented() {
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
    fn test_workload_type_suggested_read_ahead() {
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
    fn test_access_profile_new() {
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
    fn test_access_profile_record_read() {
        let mut profile = AccessProfile::new();
        profile.record_read(4096, true);
        assert_eq!(profile.read_bytes, 4096);
        assert_eq!(profile.read_ops, 1);
        assert_eq!(profile.sequential_reads, 1);
        assert_eq!(profile.random_reads, 0);
        assert_eq!(profile.avg_read_size_bytes, 4096);
    }

    #[test]
    fn test_access_profile_record_read_random() {
        let mut profile = AccessProfile::new();
        profile.record_read(4096, false);
        assert_eq!(profile.read_bytes, 4096);
        assert_eq!(profile.read_ops, 1);
        assert_eq!(profile.sequential_reads, 0);
        assert_eq!(profile.random_reads, 1);
    }

    #[test]
    fn test_access_profile_record_write() {
        let mut profile = AccessProfile::new();
        profile.record_write(8192);
        assert_eq!(profile.write_bytes, 8192);
        assert_eq!(profile.write_ops, 1);
    }

    #[test]
    fn test_access_profile_read_write_ratio() {
        let mut profile = AccessProfile::new();
        assert_eq!(profile.read_write_ratio(), 1.0);

        profile.record_read(100, true);
        assert!((profile.read_write_ratio() - 1.0).abs() < 0.001);

        profile.record_write(100);
        assert!((profile.read_write_ratio() - 0.5).abs() < 0.001);

        profile.record_read(300, true);
        assert!((profile.read_write_ratio() - 0.8).abs() < 0.001);
    }

    #[test]
    fn test_access_profile_sequential_ratio() {
        let mut profile = AccessProfile::new();
        assert_eq!(profile.sequential_ratio(), 0.0);

        profile.record_read(4096, true);
        assert!((profile.sequential_ratio() - 1.0).abs() < 0.001);

        profile.record_read(4096, false);
        assert!((profile.sequential_ratio() - 0.5).abs() < 0.001);

        profile.record_read(4096, false);
        assert!((profile.sequential_ratio() - 0.333).abs() < 0.01);
    }

    #[test]
    fn test_access_profile_total_ops() {
        let mut profile = AccessProfile::new();
        assert_eq!(profile.total_ops(), 0);

        profile.record_read(4096, true);
        profile.record_write(8192);
        profile.record_read(1024, false);

        assert_eq!(profile.total_ops(), 3);
    }

    #[test]
    fn test_access_profile_is_read_heavy() {
        let mut profile = AccessProfile::new();
        assert!(!profile.is_read_heavy());

        profile.record_read(100, true);
        assert!(profile.is_read_heavy());

        profile.record_write(100);
        assert!(!profile.is_read_heavy());

        profile.record_read(300, true);
        assert!(profile.is_read_heavy());
    }

    #[test]
    fn test_workload_signature_from_profile() {
        let mut profile = AccessProfile::new();
        for _ in 0..10 {
            profile.record_read(262144, true);
        }
        profile.record_write(8192);

        let sig = WorkloadSignature::from_profile(&profile, 1.0);
        assert!((sig.read_write_ratio - 0.97).abs() < 0.03);
        assert!((sig.sequential_ratio - 1.0).abs() < 0.01);
        assert!(sig.avg_io_size_kb > 230.0);
        assert!(sig.ops_per_second > 0.0);
    }

    #[test]
    fn test_workload_signature_matches_ai_training() {
        let mut profile = AccessProfile::new();
        for _ in 0..10 {
            profile.record_read(262144, true);
        }

        let sig = WorkloadSignature::from_profile(&profile, 1.0);
        assert!(sig.matches_ai_training());
        assert!(!sig.matches_database());
        assert!(!sig.matches_backup());
    }

    #[test]
    fn test_workload_signature_matches_database() {
        let mut profile = AccessProfile::new();
        for _ in 0..10 {
            profile.record_read(4096, false);
        }

        let sig = WorkloadSignature::from_profile(&profile, 1.0);
        assert!(sig.matches_database());
        assert!(!sig.matches_ai_training());
        assert!(!sig.matches_backup());
    }

    #[test]
    fn test_workload_signature_matches_backup() {
        let mut profile = AccessProfile::new();
        for _ in 0..10 {
            profile.record_write(262144);
        }

        let sig = WorkloadSignature::from_profile(&profile, 1.0);
        assert!(sig.matches_backup());
        assert!(!sig.matches_ai_training());
        assert!(!sig.matches_database());
    }

    #[test]
    fn test_classification_result_new() {
        let result = ClassificationResult::new(WorkloadType::AiTraining, 0.85);
        assert_eq!(result.workload_type, WorkloadType::AiTraining);
        assert!((result.confidence - 0.85).abs() < 0.001);
        assert!(result.hints.is_empty());
    }

    #[test]
    fn test_classification_result_add_hint() {
        let mut result = ClassificationResult::new(WorkloadType::Unknown, 0.5);
        result.add_hint(TuningHint::IncreaseReadAhead);
        result.add_hint(TuningHint::PrioritizeLatency);

        assert_eq!(result.hints.len(), 2);
        assert!(matches!(result.hints[0], TuningHint::IncreaseReadAhead));
    }

    #[test]
    fn test_classification_result_is_high_confidence() {
        let mut result = ClassificationResult::new(WorkloadType::Unknown, 0.5);
        assert!(!result.is_high_confidence());

        result.confidence = 0.7;
        assert!(result.is_high_confidence());

        result.confidence = 0.9;
        assert!(result.is_high_confidence());
    }

    #[test]
    fn test_workload_classifier_insufficient_ops() {
        let classifier = WorkloadClassifier::new();
        let profile = AccessProfile::new();
        let result = classifier.classify(&profile, 1.0);

        assert_eq!(result.workload_type, WorkloadType::Unknown);
        assert!((result.confidence - 0.0).abs() < 0.001);
        assert!(result.hints.is_empty());
    }

    #[test]
    fn test_workload_classifier_ai_training() {
        let classifier = WorkloadClassifier::new();
        let mut profile = AccessProfile::new();

        for _ in 0..150 {
            profile.record_read(262144, true);
        }

        let result = classifier.classify(&profile, 1.0);
        assert_eq!(result.workload_type, WorkloadType::AiTraining);
        assert!((result.confidence - 0.85).abs() < 0.001);
        assert!(result.hints.contains(&TuningHint::IncreaseReadAhead));
        assert!(result.hints.contains(&TuningHint::PrioritizeThroughput));
        assert!(result.hints.contains(&TuningHint::EnableCompression));
    }

    #[test]
    fn test_workload_classifier_database() {
        let classifier = WorkloadClassifier::new();
        let mut profile = AccessProfile::new();

        for _ in 0..150 {
            profile.record_read(4096, false);
        }

        let result = classifier.classify(&profile, 1.0);
        assert_eq!(result.workload_type, WorkloadType::Database);
        assert!((result.confidence - 0.80).abs() < 0.001);
        assert!(result.hints.contains(&TuningHint::UseDirectIo));
        assert!(result.hints.contains(&TuningHint::PrioritizeLatency));
        assert!(result.hints.contains(&TuningHint::DisableCompression));
    }

    #[test]
    fn test_workload_classifier_backup() {
        let classifier = WorkloadClassifier::new();
        let mut profile = AccessProfile::new();

        for _ in 0..150 {
            profile.record_write(262144);
        }

        let result = classifier.classify(&profile, 1.0);
        assert_eq!(result.workload_type, WorkloadType::Backup);
        assert!((result.confidence - 0.75).abs() < 0.001);
        assert!(result.hints.contains(&TuningHint::IncreaseReadAhead));
        assert!(result.hints.contains(&TuningHint::PrioritizeThroughput));
    }

    #[test]
    fn test_workload_classifier_streaming() {
        let classifier = WorkloadClassifier::new();
        let mut profile = AccessProfile::new();

        for _ in 0..150 {
            profile.record_read(131072, true);
        }

        let result = classifier.classify(&profile, 1.0);
        assert_eq!(result.workload_type, WorkloadType::Streaming);
        assert!((result.confidence - 0.70).abs() < 0.001);
    }

    #[test]
    fn test_workload_classifier_web_serving() {
        let classifier = WorkloadClassifier::new();
        let mut profile = AccessProfile::new();

        for _ in 0..1500 {
            profile.record_read(8192, false);
        }

        let result = classifier.classify(&profile, 1.0);
        assert_eq!(result.workload_type, WorkloadType::WebServing);
    }

    #[test]
    fn test_adaptive_tuner_new() {
        let tuner = AdaptiveTuner::new();
        assert_eq!(tuner.tracked_inodes(), 0);
    }

    #[test]
    fn test_adaptive_tuner_record_read() {
        let mut tuner = AdaptiveTuner::new();
        tuner.record_read(100, 4096, true);
        assert_eq!(tuner.tracked_inodes(), 1);

        tuner.record_read(200, 8192, false);
        assert_eq!(tuner.tracked_inodes(), 2);
    }

    #[test]
    fn test_adaptive_tuner_record_write() {
        let mut tuner = AdaptiveTuner::new();
        tuner.record_write(100, 8192);
        assert_eq!(tuner.tracked_inodes(), 1);
    }

    #[test]
    fn test_adaptive_tuner_classify_inode() {
        let mut tuner = AdaptiveTuner::new();

        for _ in 0..150 {
            tuner.record_read(100, 262144, true);
        }

        let result = tuner.classify_inode(100);
        assert_eq!(result.workload_type, WorkloadType::AiTraining);
    }

    #[test]
    fn test_adaptive_tuner_get_read_ahead_kb() {
        let mut tuner = AdaptiveTuner::new();

        for _ in 0..150 {
            tuner.record_read(100, 262144, true);
        }

        tuner.classify_inode(100);
        let read_ahead = tuner.get_read_ahead_kb(100);
        assert_eq!(read_ahead, 2048);
    }

    #[test]
    fn test_adaptive_tuner_get_read_ahead_kb_default() {
        let tuner = AdaptiveTuner::new();
        let read_ahead = tuner.get_read_ahead_kb(999);
        assert_eq!(read_ahead, 128);
    }

    #[test]
    fn test_adaptive_tuner_evict_inode() {
        let mut tuner = AdaptiveTuner::new();
        tuner.record_read(100, 4096, true);
        assert_eq!(tuner.tracked_inodes(), 1);

        tuner.evict_inode(100);
        assert_eq!(tuner.tracked_inodes(), 0);
        assert!(tuner.get_read_ahead_kb(100) == 128);
    }
}
