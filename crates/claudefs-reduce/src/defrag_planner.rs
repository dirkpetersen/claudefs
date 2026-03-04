use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlotState {
    Live,
    Fragmented,
    Dead,
    Free,
}

#[derive(Debug, Clone)]
pub struct SegmentSlotInfo {
    pub slot_id: u64,
    pub total_bytes: u32,
    pub live_bytes: u32,
    pub state: SlotState,
}

impl SegmentSlotInfo {
    pub fn live_fraction(&self) -> f32 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        self.live_bytes as f32 / self.total_bytes as f32
    }

    pub fn needs_defrag(&self, threshold: f32) -> bool {
        self.state == SlotState::Fragmented && self.live_fraction() < threshold
    }
}

#[derive(Debug, Clone)]
pub struct DefragAction {
    pub source_slot: u64,
    pub dest_slot: u64,
    pub live_bytes: u32,
}

#[derive(Debug, Clone)]
pub struct DefragPlannerConfig {
    pub defrag_threshold: f32,
    pub max_actions_per_pass: usize,
}

impl Default for DefragPlannerConfig {
    fn default() -> Self {
        Self {
            defrag_threshold: 0.5,
            max_actions_per_pass: 64,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DefragPlannerStats {
    pub plans_generated: u64,
    pub total_actions: u64,
    pub total_bytes_moved: u64,
    pub dead_slots_erased: u64,
}

pub struct DefragPlanner {
    config: DefragPlannerConfig,
    slots: std::collections::HashMap<u64, SegmentSlotInfo>,
    stats: DefragPlannerStats,
}

impl DefragPlanner {
    pub fn new(config: DefragPlannerConfig) -> Self {
        Self {
            config,
            slots: std::collections::HashMap::new(),
            stats: DefragPlannerStats::default(),
        }
    }

    pub fn upsert_slot(&mut self, info: SegmentSlotInfo) {
        self.slots.insert(info.slot_id, info);
    }

    pub fn remove_slot(&mut self, slot_id: u64) -> bool {
        self.slots.remove(&slot_id).is_some()
    }

    pub fn plan_pass(&mut self) -> (Vec<DefragAction>, Vec<u64>) {
        let threshold = self.config.defrag_threshold;
        let max = self.config.max_actions_per_pass;

        let mut candidates: Vec<&SegmentSlotInfo> = self
            .slots
            .values()
            .filter(|s| s.needs_defrag(threshold))
            .collect();
        candidates.sort_by(|a, b| a.live_fraction().partial_cmp(&b.live_fraction()).unwrap());

        let free_slots: Vec<u64> = self
            .slots
            .values()
            .filter(|s| s.state == SlotState::Free)
            .map(|s| s.slot_id)
            .collect();

        let mut actions = Vec::new();
        let mut free_iter = free_slots.into_iter();
        for candidate in candidates.into_iter().take(max) {
            if let Some(dest) = free_iter.next() {
                actions.push(DefragAction {
                    source_slot: candidate.slot_id,
                    dest_slot: dest,
                    live_bytes: candidate.live_bytes,
                });
            }
        }

        let dead_slots: Vec<u64> = self
            .slots
            .values()
            .filter(|s| s.state == SlotState::Dead)
            .map(|s| s.slot_id)
            .collect();

        let bytes_moved: u64 = actions.iter().map(|a| a.live_bytes as u64).sum();
        self.stats.plans_generated += 1;
        self.stats.total_actions += actions.len() as u64;
        self.stats.total_bytes_moved += bytes_moved;
        self.stats.dead_slots_erased += dead_slots.len() as u64;

        (actions, dead_slots)
    }

    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }
    pub fn stats(&self) -> &DefragPlannerStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_state_variants() {
        assert_eq!(SlotState::Live, SlotState::Live);
        assert_eq!(SlotState::Fragmented, SlotState::Fragmented);
        assert_eq!(SlotState::Dead, SlotState::Dead);
        assert_eq!(SlotState::Free, SlotState::Free);
        assert_ne!(SlotState::Live, SlotState::Dead);
    }

    #[test]
    fn live_fraction_full() {
        let slot = SegmentSlotInfo {
            slot_id: 1,
            total_bytes: 100,
            live_bytes: 100,
            state: SlotState::Live,
        };
        assert!((slot.live_fraction() - 1.0).abs() < 0.001);
    }

    #[test]
    fn live_fraction_half() {
        let slot = SegmentSlotInfo {
            slot_id: 1,
            total_bytes: 100,
            live_bytes: 50,
            state: SlotState::Fragmented,
        };
        assert!((slot.live_fraction() - 0.5).abs() < 0.001);
    }

    #[test]
    fn live_fraction_zero_total() {
        let slot = SegmentSlotInfo {
            slot_id: 1,
            total_bytes: 0,
            live_bytes: 0,
            state: SlotState::Free,
        };
        assert!((slot.live_fraction() - 0.0).abs() < 0.001);
    }

    #[test]
    fn needs_defrag_true() {
        let slot = SegmentSlotInfo {
            slot_id: 1,
            total_bytes: 100,
            live_bytes: 30,
            state: SlotState::Fragmented,
        };
        assert!(slot.needs_defrag(0.5));
    }

    #[test]
    fn needs_defrag_false_not_fragmented() {
        let slot = SegmentSlotInfo {
            slot_id: 1,
            total_bytes: 100,
            live_bytes: 30,
            state: SlotState::Live,
        };
        assert!(!slot.needs_defrag(0.5));
    }

    #[test]
    fn needs_defrag_false_above_threshold() {
        let slot = SegmentSlotInfo {
            slot_id: 1,
            total_bytes: 100,
            live_bytes: 60,
            state: SlotState::Fragmented,
        };
        assert!(!slot.needs_defrag(0.5));
    }

    #[test]
    fn config_default() {
        let config = DefragPlannerConfig::default();
        assert!((config.defrag_threshold - 0.5).abs() < 0.001);
        assert_eq!(config.max_actions_per_pass, 64);
    }

    #[test]
    fn new_planner_empty() {
        let planner = DefragPlanner::new(DefragPlannerConfig::default());
        assert_eq!(planner.slot_count(), 0);
    }

    #[test]
    fn upsert_slot_adds() {
        let mut planner = DefragPlanner::new(DefragPlannerConfig::default());
        planner.upsert_slot(SegmentSlotInfo {
            slot_id: 1,
            total_bytes: 100,
            live_bytes: 50,
            state: SlotState::Live,
        });
        assert_eq!(planner.slot_count(), 1);
    }

    #[test]
    fn upsert_slot_updates() {
        let mut planner = DefragPlanner::new(DefragPlannerConfig::default());
        planner.upsert_slot(SegmentSlotInfo {
            slot_id: 1,
            total_bytes: 100,
            live_bytes: 50,
            state: SlotState::Live,
        });
        planner.upsert_slot(SegmentSlotInfo {
            slot_id: 1,
            total_bytes: 200,
            live_bytes: 100,
            state: SlotState::Fragmented,
        });
        assert_eq!(planner.slot_count(), 1);
    }

    #[test]
    fn remove_slot_success() {
        let mut planner = DefragPlanner::new(DefragPlannerConfig::default());
        planner.upsert_slot(SegmentSlotInfo {
            slot_id: 1,
            total_bytes: 100,
            live_bytes: 50,
            state: SlotState::Live,
        });
        let result = planner.remove_slot(1);
        assert!(result);
        assert_eq!(planner.slot_count(), 0);
    }

    #[test]
    fn remove_slot_nonexistent() {
        let mut planner = DefragPlanner::new(DefragPlannerConfig::default());
        let result = planner.remove_slot(999);
        assert!(!result);
    }

    #[test]
    fn plan_pass_no_candidates() {
        let mut planner = DefragPlanner::new(DefragPlannerConfig::default());
        planner.upsert_slot(SegmentSlotInfo {
            slot_id: 1,
            total_bytes: 100,
            live_bytes: 80,
            state: SlotState::Live,
        });
        let (actions, _) = planner.plan_pass();
        assert!(actions.is_empty());
    }

    #[test]
    fn plan_pass_returns_dead_slots() {
        let mut planner = DefragPlanner::new(DefragPlannerConfig::default());
        planner.upsert_slot(SegmentSlotInfo {
            slot_id: 1,
            total_bytes: 100,
            live_bytes: 0,
            state: SlotState::Dead,
        });
        let (_, dead) = planner.plan_pass();
        assert_eq!(dead, vec![1]);
    }

    #[test]
    fn plan_pass_skips_live_slots() {
        let mut planner = DefragPlanner::new(DefragPlannerConfig::default());
        planner.upsert_slot(SegmentSlotInfo {
            slot_id: 1,
            total_bytes: 100,
            live_bytes: 100,
            state: SlotState::Live,
        });
        planner.upsert_slot(SegmentSlotInfo {
            slot_id: 2,
            total_bytes: 100,
            live_bytes: 100,
            state: SlotState::Free,
        });
        let (actions, _) = planner.plan_pass();
        assert!(actions.is_empty());
    }

    #[test]
    fn plan_pass_generates_action() {
        let mut planner = DefragPlanner::new(DefragPlannerConfig::default());
        planner.upsert_slot(SegmentSlotInfo {
            slot_id: 1,
            total_bytes: 100,
            live_bytes: 30,
            state: SlotState::Fragmented,
        });
        planner.upsert_slot(SegmentSlotInfo {
            slot_id: 2,
            total_bytes: 100,
            live_bytes: 0,
            state: SlotState::Free,
        });
        let (actions, _) = planner.plan_pass();
        assert_eq!(actions.len(), 1);
    }

    #[test]
    fn plan_pass_action_source_slot_correct() {
        let mut planner = DefragPlanner::new(DefragPlannerConfig::default());
        planner.upsert_slot(SegmentSlotInfo {
            slot_id: 5,
            total_bytes: 100,
            live_bytes: 30,
            state: SlotState::Fragmented,
        });
        planner.upsert_slot(SegmentSlotInfo {
            slot_id: 10,
            total_bytes: 100,
            live_bytes: 0,
            state: SlotState::Free,
        });
        let (actions, _) = planner.plan_pass();
        assert_eq!(actions[0].source_slot, 5);
    }

    #[test]
    fn plan_pass_action_bytes_correct() {
        let mut planner = DefragPlanner::new(DefragPlannerConfig::default());
        planner.upsert_slot(SegmentSlotInfo {
            slot_id: 1,
            total_bytes: 100,
            live_bytes: 45,
            state: SlotState::Fragmented,
        });
        planner.upsert_slot(SegmentSlotInfo {
            slot_id: 2,
            total_bytes: 100,
            live_bytes: 0,
            state: SlotState::Free,
        });
        let (actions, _) = planner.plan_pass();
        assert_eq!(actions[0].live_bytes, 45);
    }

    #[test]
    fn plan_pass_respects_max_actions() {
        let mut planner = DefragPlanner::new(DefragPlannerConfig {
            defrag_threshold: 0.5,
            max_actions_per_pass: 2,
        });
        for i in 1..=5 {
            planner.upsert_slot(SegmentSlotInfo {
                slot_id: i,
                total_bytes: 100,
                live_bytes: 30,
                state: SlotState::Fragmented,
            });
        }
        for i in 101..=110 {
            planner.upsert_slot(SegmentSlotInfo {
                slot_id: i,
                total_bytes: 100,
                live_bytes: 0,
                state: SlotState::Free,
            });
        }
        let (actions, _) = planner.plan_pass();
        assert_eq!(actions.len(), 2);
    }

    #[test]
    fn plan_pass_updates_stats_plans() {
        let mut planner = DefragPlanner::new(DefragPlannerConfig::default());
        planner.upsert_slot(SegmentSlotInfo {
            slot_id: 1,
            total_bytes: 100,
            live_bytes: 30,
            state: SlotState::Fragmented,
        });
        planner.upsert_slot(SegmentSlotInfo {
            slot_id: 2,
            total_bytes: 100,
            live_bytes: 0,
            state: SlotState::Free,
        });
        planner.plan_pass();
        assert_eq!(planner.stats().plans_generated, 1);
    }

    #[test]
    fn plan_pass_updates_stats_bytes() {
        let mut planner = DefragPlanner::new(DefragPlannerConfig::default());
        planner.upsert_slot(SegmentSlotInfo {
            slot_id: 1,
            total_bytes: 100,
            live_bytes: 30,
            state: SlotState::Fragmented,
        });
        planner.upsert_slot(SegmentSlotInfo {
            slot_id: 2,
            total_bytes: 100,
            live_bytes: 0,
            state: SlotState::Free,
        });
        planner.plan_pass();
        assert_eq!(planner.stats().total_bytes_moved, 30);
    }

    #[test]
    fn plan_pass_dead_slots_erased_stats() {
        let mut planner = DefragPlanner::new(DefragPlannerConfig::default());
        planner.upsert_slot(SegmentSlotInfo {
            slot_id: 1,
            total_bytes: 100,
            live_bytes: 0,
            state: SlotState::Dead,
        });
        planner.upsert_slot(SegmentSlotInfo {
            slot_id: 2,
            total_bytes: 100,
            live_bytes: 0,
            state: SlotState::Dead,
        });
        planner.plan_pass();
        assert_eq!(planner.stats().dead_slots_erased, 2);
    }

    #[test]
    fn plan_pass_sorts_by_live_fraction() {
        let mut planner = DefragPlanner::new(DefragPlannerConfig::default());
        planner.upsert_slot(SegmentSlotInfo {
            slot_id: 1,
            total_bytes: 100,
            live_bytes: 10,
            state: SlotState::Fragmented,
        });
        planner.upsert_slot(SegmentSlotInfo {
            slot_id: 2,
            total_bytes: 100,
            live_bytes: 40,
            state: SlotState::Fragmented,
        });
        planner.upsert_slot(SegmentSlotInfo {
            slot_id: 3,
            total_bytes: 100,
            live_bytes: 0,
            state: SlotState::Free,
        });
        let (actions, _) = planner.plan_pass();
        assert_eq!(actions[0].source_slot, 1);
    }
}
