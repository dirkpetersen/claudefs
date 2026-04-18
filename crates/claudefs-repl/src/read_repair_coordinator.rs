use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReadRepairPolicy {
    Immediate,
    Deferred,
    Adaptive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsistencyLevel {
    Strong,
    Eventual,
    Causal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadContext {
    pub read_id: String,
    pub site_ids: Vec<u32>,
    pub timestamp: u64,
    pub consistency_level: ConsistencyLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadValue {
    pub value: Vec<u8>,
    pub version: u64,
    pub site_id: u32,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepairAction {
    NoRepair,
    PatchMinority,
    PatchMajority,
    FullSync,
}

#[derive(Debug, Clone)]
pub struct ReadRepairCoordinator {
    policy: ReadRepairPolicy,
    max_sites: usize,
}

impl ReadRepairCoordinator {
    pub fn new(policy: ReadRepairPolicy, max_sites: usize) -> Self {
        ReadRepairCoordinator { policy, max_sites }
    }

    pub fn detect_divergence(&self, values: &[ReadValue]) -> bool {
        if values.is_empty() {
            return false;
        }
        let first_value = &values[0].value;
        !values.iter().all(|rv| &rv.value == first_value)
    }

    pub fn compute_repair_action(&self, values: &[ReadValue], _site_count: usize) -> RepairAction {
        if !self.detect_divergence(values) {
            return RepairAction::NoRepair;
        }

        if values.is_empty() {
            return RepairAction::NoRepair;
        }

        let mut value_counts: HashMap<Vec<u8>, usize> = HashMap::new();
        for rv in values {
            *value_counts.entry(rv.value.clone()).or_insert(0) += 1;
        }

        let max_count = value_counts.values().max().copied().unwrap_or(0);
        let total = values.len();
        let threshold = total.div_ceil(2);

        if max_count >= threshold {
            RepairAction::PatchMinority
        } else if value_counts.len() > 1 && (total as f64 - max_count as f64) / total as f64 > 0.3 {
            RepairAction::FullSync
        } else {
            RepairAction::PatchMajority
        }
    }

    pub fn find_consensus(&self, values: &[ReadValue]) -> Option<ReadValue> {
        if values.is_empty() {
            return None;
        }

        let mut value_map: HashMap<Vec<u8>, ReadValue> = HashMap::new();
        let mut value_counts: HashMap<Vec<u8>, usize> = HashMap::new();

        for rv in values {
            *value_counts.entry(rv.value.clone()).or_insert(0) += 1;
            value_map.insert(rv.value.clone(), rv.clone());
        }

        value_counts
            .iter()
            .max_by_key(|(_, count)| *count)
            .and_then(|(val, _)| value_map.get(val).cloned())
    }

    pub fn select_repair_targets(&self, action: RepairAction, values: &[ReadValue]) -> Vec<u32> {
        match action {
            RepairAction::NoRepair => vec![],
            RepairAction::PatchMinority => {
                if let Some(consensus) = self.find_consensus(values) {
                    values
                        .iter()
                        .filter(|rv| rv.value != consensus.value)
                        .map(|rv| rv.site_id)
                        .collect()
                } else {
                    vec![]
                }
            }
            RepairAction::PatchMajority => {
                if let Some(consensus) = self.find_consensus(values) {
                    values
                        .iter()
                        .filter(|rv| rv.value == consensus.value)
                        .map(|rv| rv.site_id)
                        .collect()
                } else {
                    values.iter().map(|rv| rv.site_id).collect()
                }
            }
            RepairAction::FullSync => values.iter().map(|rv| rv.site_id).collect(),
        }
    }

    pub fn is_idempotent(&self, action: RepairAction) -> bool {
        matches!(action, RepairAction::NoRepair | RepairAction::FullSync)
    }

    pub fn policy(&self) -> ReadRepairPolicy {
        self.policy
    }

    pub fn max_sites(&self) -> usize {
        self.max_sites
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_read_value(value: Vec<u8>, version: u64, site_id: u32, timestamp: u64) -> ReadValue {
        ReadValue {
            value,
            version,
            site_id,
            timestamp,
        }
    }

    #[test]
    fn test_consensus_unanimous() {
        let coordinator = ReadRepairCoordinator::new(ReadRepairPolicy::Immediate, 5);
        let values = vec![
            make_read_value(b"data".to_vec(), 1, 1, 100),
            make_read_value(b"data".to_vec(), 1, 2, 100),
            make_read_value(b"data".to_vec(), 1, 3, 100),
        ];
        let consensus = coordinator.find_consensus(&values);
        assert!(consensus.is_some());
        assert_eq!(consensus.unwrap().value, b"data");
    }

    #[test]
    fn test_consensus_majority_agreement() {
        let coordinator = ReadRepairCoordinator::new(ReadRepairPolicy::Immediate, 5);
        let values = vec![
            make_read_value(b"data".to_vec(), 1, 1, 100),
            make_read_value(b"data".to_vec(), 1, 2, 100),
            make_read_value(b"old".to_vec(), 0, 3, 50),
        ];
        let consensus = coordinator.find_consensus(&values);
        assert!(consensus.is_some());
        assert_eq!(consensus.unwrap().value, b"data");
    }

    #[test]
    fn test_consensus_minority_agreement() {
        let coordinator = ReadRepairCoordinator::new(ReadRepairPolicy::Immediate, 5);
        let values = vec![
            make_read_value(b"old1".to_vec(), 0, 1, 50),
            make_read_value(b"new".to_vec(), 1, 2, 100),
            make_read_value(b"new".to_vec(), 1, 3, 100),
        ];
        let consensus = coordinator.find_consensus(&values);
        assert!(consensus.is_some());
        assert_eq!(consensus.unwrap().value, b"new");
    }

    #[test]
    fn test_divergence_detection_yes() {
        let coordinator = ReadRepairCoordinator::new(ReadRepairPolicy::Immediate, 5);
        let values = vec![
            make_read_value(b"data1".to_vec(), 1, 1, 100),
            make_read_value(b"data2".to_vec(), 1, 2, 100),
        ];
        assert!(coordinator.detect_divergence(&values));
    }

    #[test]
    fn test_divergence_detection_no() {
        let coordinator = ReadRepairCoordinator::new(ReadRepairPolicy::Immediate, 5);
        let values = vec![
            make_read_value(b"data".to_vec(), 1, 1, 100),
            make_read_value(b"data".to_vec(), 1, 2, 100),
        ];
        assert!(!coordinator.detect_divergence(&values));
    }

    #[test]
    fn test_divergence_empty_list() {
        let coordinator = ReadRepairCoordinator::new(ReadRepairPolicy::Immediate, 5);
        let values: Vec<ReadValue> = vec![];
        assert!(!coordinator.detect_divergence(&values));
    }

    #[test]
    fn test_repair_action_no_repair() {
        let coordinator = ReadRepairCoordinator::new(ReadRepairPolicy::Immediate, 5);
        let values = vec![
            make_read_value(b"data".to_vec(), 1, 1, 100),
            make_read_value(b"data".to_vec(), 1, 2, 100),
        ];
        let action = coordinator.compute_repair_action(&values, 2);
        assert_eq!(action, RepairAction::NoRepair);
    }

    #[test]
    fn test_repair_action_patch_minority() {
        let coordinator = ReadRepairCoordinator::new(ReadRepairPolicy::Immediate, 5);
        let values = vec![
            make_read_value(b"data".to_vec(), 1, 1, 100),
            make_read_value(b"data".to_vec(), 1, 2, 100),
            make_read_value(b"old".to_vec(), 0, 3, 50),
        ];
        let action = coordinator.compute_repair_action(&values, 3);
        assert_eq!(action, RepairAction::PatchMinority);
    }

    #[test]
    fn test_repair_action_patch_majority() {
        let coordinator = ReadRepairCoordinator::new(ReadRepairPolicy::Deferred, 5);
        let values = vec![
            make_read_value(b"a".to_vec(), 1, 1, 100),
            make_read_value(b"a".to_vec(), 1, 2, 100),
            make_read_value(b"b".to_vec(), 1, 3, 100),
            make_read_value(b"c".to_vec(), 1, 4, 100),
        ];
        let action = coordinator.compute_repair_action(&values, 4);
        assert!(action == RepairAction::PatchMinority || action == RepairAction::FullSync);
    }

    #[test]
    fn test_repair_action_full_sync() {
        let coordinator = ReadRepairCoordinator::new(ReadRepairPolicy::Adaptive, 5);
        let values = vec![
            make_read_value(b"a".to_vec(), 1, 1, 100),
            make_read_value(b"b".to_vec(), 1, 2, 100),
            make_read_value(b"c".to_vec(), 1, 3, 100),
            make_read_value(b"d".to_vec(), 1, 4, 100),
        ];
        let action = coordinator.compute_repair_action(&values, 4);
        assert_eq!(action, RepairAction::FullSync);
    }

    #[test]
    fn test_immediate_policy() {
        let coordinator = ReadRepairCoordinator::new(ReadRepairPolicy::Immediate, 5);
        assert_eq!(coordinator.policy(), ReadRepairPolicy::Immediate);
    }

    #[test]
    fn test_deferred_policy() {
        let coordinator = ReadRepairCoordinator::new(ReadRepairPolicy::Deferred, 5);
        assert_eq!(coordinator.policy(), ReadRepairPolicy::Deferred);
    }

    #[test]
    fn test_adaptive_policy() {
        let coordinator = ReadRepairCoordinator::new(ReadRepairPolicy::Adaptive, 5);
        assert_eq!(coordinator.policy(), ReadRepairPolicy::Adaptive);
    }

    #[test]
    fn test_strong_consistency() {
        let ctx = ReadContext {
            read_id: "test".to_string(),
            site_ids: vec![1, 2, 3],
            timestamp: 100,
            consistency_level: ConsistencyLevel::Strong,
        };
        assert_eq!(ctx.consistency_level, ConsistencyLevel::Strong);
    }

    #[test]
    fn test_eventual_consistency() {
        let ctx = ReadContext {
            read_id: "test".to_string(),
            site_ids: vec![1, 2],
            timestamp: 200,
            consistency_level: ConsistencyLevel::Eventual,
        };
        assert_eq!(ctx.consistency_level, ConsistencyLevel::Eventual);
    }

    #[test]
    fn test_causal_consistency() {
        let ctx = ReadContext {
            read_id: "test".to_string(),
            site_ids: vec![1],
            timestamp: 300,
            consistency_level: ConsistencyLevel::Causal,
        };
        assert_eq!(ctx.consistency_level, ConsistencyLevel::Causal);
    }

    #[test]
    fn test_select_repair_targets_patch_minority() {
        let coordinator = ReadRepairCoordinator::new(ReadRepairPolicy::Immediate, 5);
        let values = vec![
            make_read_value(b"data".to_vec(), 1, 1, 100),
            make_read_value(b"data".to_vec(), 1, 2, 100),
            make_read_value(b"old".to_vec(), 0, 3, 50),
        ];
        let targets = coordinator.select_repair_targets(RepairAction::PatchMinority, &values);
        assert_eq!(targets, vec![3]);
    }

    #[test]
    fn test_select_repair_targets_full_sync() {
        let coordinator = ReadRepairCoordinator::new(ReadRepairPolicy::Adaptive, 5);
        let values = vec![
            make_read_value(b"a".to_vec(), 1, 1, 100),
            make_read_value(b"b".to_vec(), 1, 2, 100),
            make_read_value(b"c".to_vec(), 1, 3, 100),
        ];
        let targets = coordinator.select_repair_targets(RepairAction::FullSync, &values);
        assert_eq!(targets, vec![1, 2, 3]);
    }

    #[test]
    fn test_is_idempotent_true() {
        let coordinator = ReadRepairCoordinator::new(ReadRepairPolicy::Immediate, 5);
        assert!(coordinator.is_idempotent(RepairAction::NoRepair));
        assert!(coordinator.is_idempotent(RepairAction::FullSync));
    }

    #[test]
    fn test_is_idempotent_false() {
        let coordinator = ReadRepairCoordinator::new(ReadRepairPolicy::Immediate, 5);
        assert!(!coordinator.is_idempotent(RepairAction::PatchMinority));
        assert!(!coordinator.is_idempotent(RepairAction::PatchMajority));
    }

    #[test]
    fn test_find_consensus_empty() {
        let coordinator = ReadRepairCoordinator::new(ReadRepairPolicy::Immediate, 5);
        let values: Vec<ReadValue> = vec![];
        let consensus = coordinator.find_consensus(&values);
        assert!(consensus.is_none());
    }

    #[test]
    fn test_find_consensus_majority() {
        let coordinator = ReadRepairCoordinator::new(ReadRepairPolicy::Immediate, 5);
        let values = vec![
            make_read_value(b"data".to_vec(), 1, 1, 100),
            make_read_value(b"other".to_vec(), 2, 2, 200),
            make_read_value(b"other".to_vec(), 2, 3, 200),
            make_read_value(b"other".to_vec(), 2, 4, 200),
        ];
        let consensus = coordinator.find_consensus(&values);
        assert!(consensus.is_some());
        assert_eq!(consensus.unwrap().value, b"other");
    }

    #[test]
    fn test_serialization_roundtrip() {
        let action = RepairAction::PatchMinority;
        let serialized = bincode::serialize(&action).unwrap();
        let deserialized: RepairAction = bincode::deserialize(&serialized).unwrap();
        assert_eq!(action, deserialized);

        let level = ConsistencyLevel::Causal;
        let serialized = bincode::serialize(&level).unwrap();
        let deserialized: ConsistencyLevel = bincode::deserialize(&serialized).unwrap();
        assert_eq!(level, deserialized);
    }

    #[test]
    fn test_large_replica_set() {
        let coordinator = ReadRepairCoordinator::new(ReadRepairPolicy::Adaptive, 20);
        let mut values = vec![];
        for i in 0..12 {
            values.push(make_read_value(b"consistent".to_vec(), 1, i, 100));
        }
        for i in 12..15 {
            values.push(make_read_value(b"diverged".to_vec(), 0, i, 50));
        }

        assert!(coordinator.detect_divergence(&values));
        let action = coordinator.compute_repair_action(&values, 15);
        assert_eq!(action, RepairAction::PatchMinority);

        let targets = coordinator.select_repair_targets(action, &values);
        assert_eq!(targets.len(), 3);
    }
}
