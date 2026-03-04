//! EC repair planning when shards are lost or corrupted.
//!
//! D1: 4+2 EC can tolerate up to 2 shard losses. When a node fails, repair reads
//! surviving shards and reconstructs lost ones on healthy nodes.

/// State of a single shard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShardState {
    /// Shard is available and healthy
    Available,
    /// Shard is lost (node failure, disk loss)
    Lost,
    /// Shard is corrupted (bitrot, checksum failure)
    Corrupted,
}

/// Assessment of a segment's repair needs.
#[derive(Debug, Clone)]
pub struct RepairAssessment {
    /// Segment identifier
    pub segment_id: u64,
    /// Total number of shards
    pub total_shards: u8,
    /// Number of available shards
    pub available: u8,
    /// Number of lost shards
    pub lost: u8,
    /// Number of corrupted shards
    pub corrupted: u8,
}

impl RepairAssessment {
    /// Check if segment is degraded (any lost or corrupted shards)
    pub fn is_degraded(&self) -> bool {
        self.lost + self.corrupted > 0
    }

    /// Check if segment can be recovered
    pub fn can_recover(&self, data_shards: u8) -> bool {
        self.available >= data_shards
    }

    /// Check if segment needs immediate repair (at or above parity threshold)
    pub fn needs_immediate_repair(&self, parity_shards: u8) -> bool {
        self.lost + self.corrupted >= parity_shards
    }
}

/// Plan for repairing a segment.
#[derive(Debug, Clone)]
pub struct RepairPlan {
    /// Segment to repair
    pub segment_id: u64,
    /// Shard indices to read from (sources)
    pub source_shards: Vec<u8>,
    /// Shard indices to reconstruct (targets)
    pub target_shards: Vec<u8>,
    /// Node IDs to place reconstructed shards on
    pub target_nodes: Vec<u64>,
}

impl RepairPlan {
    /// Number of shards to repair
    pub fn repair_count(&self) -> usize {
        self.target_shards.len()
    }
}

/// EC repair planner.
#[derive(Debug, Clone)]
pub struct EcRepair {
    /// Number of data shards
    data_shards: u8,
    /// Number of parity shards
    parity_shards: u8,
}

impl EcRepair {
    /// Create a new EC repair planner
    pub fn new(data_shards: u8, parity_shards: u8) -> Self {
        Self {
            data_shards,
            parity_shards,
        }
    }

    /// Assess the state of a segment
    pub fn assess(&self, segment_id: u64, shard_states: &[(u8, ShardState)]) -> RepairAssessment {
        let total_shards = shard_states.len() as u8;
        let mut available = 0u8;
        let mut lost = 0u8;
        let mut corrupted = 0u8;

        for (_, state) in shard_states {
            match state {
                ShardState::Available => available += 1,
                ShardState::Lost => lost += 1,
                ShardState::Corrupted => corrupted += 1,
            }
        }

        RepairAssessment {
            segment_id,
            total_shards,
            available,
            lost,
            corrupted,
        }
    }

    /// Plan repair for a degraded segment
    pub fn plan_repair(
        &self,
        assessment: &RepairAssessment,
        available_nodes: &[u64],
    ) -> Option<RepairPlan> {
        if !assessment.can_recover(self.data_shards) {
            return None;
        }

        if available_nodes.is_empty() {
            return None;
        }

        let repair_count = (assessment.lost + assessment.corrupted) as usize;
        if repair_count == 0 {
            return None;
        }

        let source_shards: Vec<u8> = (0..self.data_shards).collect();
        let target_shards: Vec<u8> = (self.data_shards..(self.data_shards + self.parity_shards))
            .take(repair_count)
            .collect();

        let target_nodes: Vec<u64> = available_nodes.iter().take(repair_count).copied().collect();

        Some(RepairPlan {
            segment_id: assessment.segment_id,
            source_shards,
            target_shards,
            target_nodes,
        })
    }

    /// Plan preventive repairs for multiple degraded segments
    pub fn plan_preventive_repair(&self, assessments: &[RepairAssessment]) -> Vec<RepairPlan> {
        let mut degraded: Vec<&RepairAssessment> = assessments
            .iter()
            .filter(|a| a.is_degraded() && a.can_recover(self.data_shards))
            .collect();

        degraded.sort_by(|a, b| {
            let a_severity = a.lost + a.corrupted;
            let b_severity = b.lost + b.corrupted;
            b_severity.cmp(&a_severity)
        });

        degraded
            .into_iter()
            .filter_map(|a| self.plan_repair(a, &[1, 2, 3, 4, 5, 6]))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ec_repair_new_4_2() {
        let repair = EcRepair::new(4, 2);
        assert_eq!(repair.data_shards, 4);
        assert_eq!(repair.parity_shards, 2);
    }

    #[test]
    fn assess_all_available() {
        let repair = EcRepair::new(4, 2);
        let states = vec![
            (0u8, ShardState::Available),
            (1u8, ShardState::Available),
            (2u8, ShardState::Available),
            (3u8, ShardState::Available),
            (4u8, ShardState::Available),
            (5u8, ShardState::Available),
        ];
        let assessment = repair.assess(1, &states);
        assert_eq!(assessment.available, 6);
        assert_eq!(assessment.lost, 0);
        assert_eq!(assessment.corrupted, 0);
        assert!(!assessment.is_degraded());
    }

    #[test]
    fn assess_some_lost() {
        let repair = EcRepair::new(4, 2);
        let states = vec![
            (0u8, ShardState::Available),
            (1u8, ShardState::Available),
            (2u8, ShardState::Available),
            (3u8, ShardState::Available),
            (4u8, ShardState::Lost),
            (5u8, ShardState::Available),
        ];
        let assessment = repair.assess(1, &states);
        assert_eq!(assessment.available, 5);
        assert_eq!(assessment.lost, 1);
        assert_eq!(assessment.corrupted, 0);
    }

    #[test]
    fn assess_some_corrupted() {
        let repair = EcRepair::new(4, 2);
        let states = vec![
            (0u8, ShardState::Available),
            (1u8, ShardState::Corrupted),
            (2u8, ShardState::Available),
            (3u8, ShardState::Available),
            (4u8, ShardState::Available),
            (5u8, ShardState::Available),
        ];
        let assessment = repair.assess(1, &states);
        assert_eq!(assessment.available, 5);
        assert_eq!(assessment.lost, 0);
        assert_eq!(assessment.corrupted, 1);
    }

    #[test]
    fn is_degraded_false() {
        let assessment = RepairAssessment {
            segment_id: 1,
            total_shards: 6,
            available: 6,
            lost: 0,
            corrupted: 0,
        };
        assert!(!assessment.is_degraded());
    }

    #[test]
    fn is_degraded_true() {
        let assessment = RepairAssessment {
            segment_id: 1,
            total_shards: 6,
            available: 5,
            lost: 1,
            corrupted: 0,
        };
        assert!(assessment.is_degraded());
    }

    #[test]
    fn can_recover_true() {
        let assessment = RepairAssessment {
            segment_id: 1,
            total_shards: 6,
            available: 4,
            lost: 2,
            corrupted: 0,
        };
        assert!(assessment.can_recover(4));
    }

    #[test]
    fn can_recover_false() {
        let assessment = RepairAssessment {
            segment_id: 1,
            total_shards: 6,
            available: 3,
            lost: 3,
            corrupted: 0,
        };
        assert!(!assessment.can_recover(4));
    }

    #[test]
    fn needs_immediate_repair_false() {
        let assessment = RepairAssessment {
            segment_id: 1,
            total_shards: 6,
            available: 5,
            lost: 1,
            corrupted: 0,
        };
        assert!(!assessment.needs_immediate_repair(2));
    }

    #[test]
    fn needs_immediate_repair_true() {
        let assessment = RepairAssessment {
            segment_id: 1,
            total_shards: 6,
            available: 4,
            lost: 2,
            corrupted: 0,
        };
        assert!(assessment.needs_immediate_repair(2));
    }

    #[test]
    fn plan_repair_returns_none_if_unrecoverable() {
        let repair = EcRepair::new(4, 2);
        let assessment = RepairAssessment {
            segment_id: 1,
            total_shards: 6,
            available: 3,
            lost: 3,
            corrupted: 0,
        };
        let nodes = vec![10u64, 20u64];
        assert!(repair.plan_repair(&assessment, &nodes).is_none());
    }

    #[test]
    fn plan_repair_returns_plan_if_recoverable() {
        let repair = EcRepair::new(4, 2);
        let assessment = RepairAssessment {
            segment_id: 1,
            total_shards: 6,
            available: 5,
            lost: 1,
            corrupted: 0,
        };
        let nodes = vec![10u64, 20u64];
        let plan = repair.plan_repair(&assessment, &nodes);
        assert!(plan.is_some());
    }

    #[test]
    fn plan_repair_source_has_data_shards() {
        let repair = EcRepair::new(4, 2);
        let assessment = RepairAssessment {
            segment_id: 1,
            total_shards: 6,
            available: 5,
            lost: 1,
            corrupted: 0,
        };
        let nodes = vec![10u64, 20u64];
        let plan = repair.plan_repair(&assessment, &nodes).unwrap();
        assert_eq!(plan.source_shards.len(), 4);
    }

    #[test]
    fn plan_preventive_repair_prioritizes_most_degraded() {
        let repair = EcRepair::new(4, 2);
        let assessments = vec![
            RepairAssessment {
                segment_id: 1,
                total_shards: 6,
                available: 5,
                lost: 1,
                corrupted: 0,
            },
            RepairAssessment {
                segment_id: 2,
                total_shards: 6,
                available: 4,
                lost: 2,
                corrupted: 0,
            },
        ];
        let plans = repair.plan_preventive_repair(&assessments);
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].segment_id, 2);
        assert_eq!(plans[1].segment_id, 1);
    }

    #[test]
    fn plan_preventive_repair_empty() {
        let repair = EcRepair::new(4, 2);
        let assessments: Vec<RepairAssessment> = vec![];
        let plans = repair.plan_preventive_repair(&assessments);
        assert!(plans.is_empty());
    }

    #[test]
    fn repair_plan_repair_count() {
        let plan = RepairPlan {
            segment_id: 1,
            source_shards: vec![0, 1, 2, 3],
            target_shards: vec![4, 5],
            target_nodes: vec![10, 20],
        };
        assert_eq!(plan.repair_count(), 2);
    }
}
