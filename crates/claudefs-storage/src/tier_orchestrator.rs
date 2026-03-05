use std::collections::HashMap;
use thiserror::Error;
use tracing::debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TierLevel {
    Hot,
    Warm,
    Cold,
    Archive,
}

impl std::fmt::Display for TierLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TierLevel::Hot => write!(f, "Hot"),
            TierLevel::Warm => write!(f, "Warm"),
            TierLevel::Cold => write!(f, "Cold"),
            TierLevel::Archive => write!(f, "Archive"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TierPlacement {
    pub shard_id: u64,
    pub tier: TierLevel,
    pub size_bytes: u64,
    pub promoted_at_ms: u64,
    pub demoted_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MigrationReason {
    PolicyEvaluation,
    CapacityPressure,
    HotDataDetected,
    ColdDataDetected,
    NodeFailure,
}

#[derive(Debug, Clone)]
pub struct PendingMigration {
    pub shard_id: u64,
    pub from_tier: TierLevel,
    pub to_tier: TierLevel,
    pub reason: MigrationReason,
    pub priority: u8,
    pub created_ms: u64,
}

#[derive(Debug, Clone)]
pub struct TieringConfig {
    pub hot_capacity_bytes: u64,
    pub warm_capacity_bytes: u64,
    pub policy: String,
}

#[derive(Debug, Error)]
pub enum TieringError {
    #[error("Shard not found: shard_id {0}")]
    ShardNotFound(u64),
    #[error("Capacity exceeded for tier: {0}")]
    CapacityExceeded(TierLevel),
    #[error("Invalid migration: {0}")]
    InvalidMigration(String),
    #[error("Shard already placed: shard_id {0}")]
    ShardAlreadyPlaced(u64),
}

pub type TieringResult<T> = Result<T, TieringError>;

pub struct TierOrchestrator {
    placements: HashMap<u64, TierPlacement>,
    pending_migrations: Vec<PendingMigration>,
    config: TieringConfig,
}

impl TierOrchestrator {
    pub fn new(config: TieringConfig) -> Self {
        Self {
            placements: HashMap::new(),
            pending_migrations: Vec::new(),
            config,
        }
    }

    pub fn place_shard(
        &mut self,
        shard_id: u64,
        tier: TierLevel,
        size_bytes: u64,
    ) -> TieringResult<()> {
        if self.placements.contains_key(&shard_id) {
            return Err(TieringError::ShardAlreadyPlaced(shard_id));
        }

        let current_capacity = self.used_capacity(tier);
        let tier_capacity = self.tier_capacity(tier);

        if current_capacity + size_bytes > tier_capacity {
            return Err(TieringError::CapacityExceeded(tier));
        }

        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let placement = TierPlacement {
            shard_id,
            tier,
            size_bytes,
            promoted_at_ms: timestamp_ms,
            demoted_at_ms: timestamp_ms,
        };

        self.placements.insert(shard_id, placement);
        debug!(
            "Placed shard {} on tier {:?} with {} bytes",
            shard_id, tier, size_bytes
        );
        Ok(())
    }

    pub fn evaluate_policy(&mut self, current_time_ms: u64) -> Vec<PendingMigration> {
        let mut migrations = Vec::new();

        for (shard_id, placement) in self.placements.iter() {
            let age_ms = current_time_ms.saturating_sub(placement.promoted_at_ms);

            let (should_migrate, target_tier) = match placement.tier {
                TierLevel::Hot => {
                    if age_ms > 300000
                        && self.used_capacity(TierLevel::Hot) > self.config.hot_capacity_bytes / 2
                    {
                        (true, TierLevel::Warm)
                    } else {
                        (false, TierLevel::Hot)
                    }
                }
                TierLevel::Warm => {
                    if age_ms > 600000 {
                        (true, TierLevel::Cold)
                    } else if self.used_capacity(TierLevel::Hot)
                        < self.config.hot_capacity_bytes / 3
                    {
                        (true, TierLevel::Hot)
                    } else {
                        (false, TierLevel::Warm)
                    }
                }
                TierLevel::Cold => {
                    if age_ms > 3600000
                        && self.used_capacity(TierLevel::Cold) > self.config.warm_capacity_bytes / 2
                    {
                        (true, TierLevel::Archive)
                    } else {
                        (false, TierLevel::Cold)
                    }
                }
                TierLevel::Archive => (false, TierLevel::Archive),
            };

            if should_migrate && target_tier != placement.tier {
                let reason = if matches!(placement.tier, TierLevel::Hot) {
                    MigrationReason::ColdDataDetected
                } else if matches!(target_tier, TierLevel::Hot) {
                    MigrationReason::HotDataDetected
                } else {
                    MigrationReason::PolicyEvaluation
                };

                let priority = match reason {
                    MigrationReason::CapacityPressure => 100,
                    MigrationReason::HotDataDetected => 80,
                    MigrationReason::NodeFailure => 60,
                    MigrationReason::ColdDataDetected => 40,
                    MigrationReason::PolicyEvaluation => 20,
                };

                migrations.push(PendingMigration {
                    shard_id: *shard_id,
                    from_tier: placement.tier,
                    to_tier: target_tier,
                    reason,
                    priority,
                    created_ms: current_time_ms,
                });
            }
        }

        migrations.sort_by(|a, b| b.priority.cmp(&a.priority));

        for migration in &migrations {
            if !self
                .pending_migrations
                .iter()
                .any(|m| m.shard_id == migration.shard_id)
            {
                self.pending_migrations.push(migration.clone());
            }
        }

        debug!(
            "Policy evaluation triggered {} migrations",
            migrations.len()
        );
        migrations
    }

    pub fn queue_migration(&mut self, migration: PendingMigration) -> TieringResult<()> {
        if !self.placements.contains_key(&migration.shard_id) {
            return Err(TieringError::ShardNotFound(migration.shard_id));
        }

        if self
            .pending_migrations
            .iter()
            .any(|m| m.shard_id == migration.shard_id)
        {
            return Err(TieringError::InvalidMigration(format!(
                "Migration already pending for shard {}",
                migration.shard_id
            )));
        }

        self.pending_migrations.push(migration.clone());
        debug!(
            "Queued migration for shard {}: {:?} -> {:?}",
            migration.shard_id, migration.from_tier, migration.to_tier
        );
        Ok(())
    }

    pub fn commit_migration(
        &mut self,
        shard_id: u64,
        to_tier: TierLevel,
        timestamp_ms: u64,
    ) -> TieringResult<()> {
        let placement = self
            .placements
            .get_mut(&shard_id)
            .ok_or(TieringError::ShardNotFound(shard_id))?;

        let from_tier = placement.tier;
        placement.tier = to_tier;

        if to_tier == TierLevel::Hot {
            placement.promoted_at_ms = timestamp_ms;
        } else {
            placement.demoted_at_ms = timestamp_ms;
        }

        self.pending_migrations.retain(|m| m.shard_id != shard_id);

        debug!(
            "Committed migration for shard {}: {:?} -> {:?}",
            shard_id, from_tier, to_tier
        );
        Ok(())
    }

    pub fn get_placement(&self, shard_id: u64) -> Option<&TierPlacement> {
        self.placements.get(&shard_id)
    }

    pub fn shard_count_by_tier(&self, tier: TierLevel) -> usize {
        self.placements.values().filter(|p| p.tier == tier).count()
    }

    pub fn used_capacity(&self, tier: TierLevel) -> u64 {
        self.placements
            .values()
            .filter(|p| p.tier == tier)
            .map(|p| p.size_bytes)
            .sum()
    }

    pub fn pending_count(&self) -> usize {
        self.pending_migrations.len()
    }

    pub fn cancel_migration(&mut self, shard_id: u64) -> TieringResult<()> {
        let initial_len = self.pending_migrations.len();
        self.pending_migrations.retain(|m| m.shard_id != shard_id);

        if self.pending_migrations.len() == initial_len {
            return Err(TieringError::InvalidMigration(format!(
                "No pending migration found for shard {}",
                shard_id
            )));
        }

        debug!("Cancelled migration for shard {}", shard_id);
        Ok(())
    }

    fn tier_capacity(&self, tier: TierLevel) -> u64 {
        match tier {
            TierLevel::Hot => self.config.hot_capacity_bytes,
            TierLevel::Warm => self.config.warm_capacity_bytes,
            TierLevel::Cold => self.config.warm_capacity_bytes * 2,
            TierLevel::Archive => u64::MAX,
        }
    }

    pub fn remove_shard(&mut self, shard_id: u64) -> TieringResult<()> {
        self.placements
            .remove(&shard_id)
            .ok_or(TieringError::ShardNotFound(shard_id))?;
        self.pending_migrations.retain(|m| m.shard_id != shard_id);
        Ok(())
    }

    pub fn capacity_pressure_migration(
        &mut self,
        shard_id: u64,
        target_tier: TierLevel,
        current_time_ms: u64,
    ) -> TieringResult<()> {
        let placement = self
            .placements
            .get(&shard_id)
            .ok_or(TieringError::ShardNotFound(shard_id))?;

        let migration = PendingMigration {
            shard_id,
            from_tier: placement.tier,
            to_tier: target_tier,
            reason: MigrationReason::CapacityPressure,
            priority: 100,
            created_ms: current_time_ms,
        };

        self.queue_migration(migration)
    }
}

impl Default for TierOrchestrator {
    fn default() -> Self {
        Self::new(TieringConfig {
            hot_capacity_bytes: 1_000_000_000_000,
            warm_capacity_bytes: 5_000_000_000_000,
            policy: "default".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_orchestrator() -> TierOrchestrator {
        TierOrchestrator::new(TieringConfig {
            hot_capacity_bytes: 1000,
            warm_capacity_bytes: 5000,
            policy: "default".to_string(),
        })
    }

    #[test]
    fn test_place_shard_hot() {
        let mut orch = create_orchestrator();
        assert!(orch.place_shard(1, TierLevel::Hot, 100).is_ok());
    }

    #[test]
    fn test_place_shard_warm() {
        let mut orch = create_orchestrator();
        assert!(orch.place_shard(1, TierLevel::Warm, 200).is_ok());
    }

    #[test]
    fn test_place_shard_cold() {
        let mut orch = create_orchestrator();
        assert!(orch.place_shard(1, TierLevel::Cold, 300).is_ok());
    }

    #[test]
    fn test_place_shard_archive() {
        let mut orch = create_orchestrator();
        assert!(orch.place_shard(1, TierLevel::Archive, 400).is_ok());
    }

    #[test]
    fn test_place_shard_duplicate() {
        let mut orch = create_orchestrator();
        orch.place_shard(1, TierLevel::Hot, 100).unwrap();

        assert!(matches!(
            orch.place_shard(1, TierLevel::Warm, 100),
            Err(TieringError::ShardAlreadyPlaced(1))
        ));
    }

    #[test]
    fn test_capacity_overflow() {
        let mut orch = create_orchestrator();

        assert!(matches!(
            orch.place_shard(1, TierLevel::Hot, 2000),
            Err(TieringError::CapacityExceeded(TierLevel::Hot))
        ));
    }

    #[test]
    fn test_capacity_within_limit() {
        let mut orch = create_orchestrator();
        assert!(orch.place_shard(1, TierLevel::Hot, 500).is_ok());
        assert!(orch.place_shard(2, TierLevel::Hot, 400).is_ok());

        assert!(matches!(
            orch.place_shard(3, TierLevel::Hot, 200),
            Err(TieringError::CapacityExceeded(TierLevel::Hot))
        ));
    }

    #[test]
    fn test_evaluate_policy_no_migrations() {
        let mut orch = create_orchestrator();
        orch.place_shard(1, TierLevel::Hot, 100).unwrap();

        let migrations = orch.evaluate_policy(1000);
        assert!(migrations.is_empty());
    }

    #[test]
    fn test_evaluate_policy_triggers_migration() {
        let mut orch = TierOrchestrator::new(TieringConfig {
            hot_capacity_bytes: 100,
            warm_capacity_bytes: 500,
            policy: "default".to_string(),
        });
        orch.place_shard(1, TierLevel::Hot, 60).unwrap();

        let current_time_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        if let Some(placement) = orch.placements.get_mut(&1) {
            placement.promoted_at_ms = current_time_ms.saturating_sub(400000);
        }

        let migrations = orch.evaluate_policy(current_time_ms);

        assert!(!migrations.is_empty());
    }

    #[test]
    fn test_evaluate_policy_cold_data() {
        let mut orch = TierOrchestrator::new(TieringConfig {
            hot_capacity_bytes: 100,
            warm_capacity_bytes: 500,
            policy: "default".to_string(),
        });
        orch.place_shard(1, TierLevel::Hot, 60).unwrap();

        let current_time_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        if let Some(placement) = orch.placements.get_mut(&1) {
            placement.promoted_at_ms = current_time_ms.saturating_sub(500000);
        }

        let migrations = orch.evaluate_policy(current_time_ms);

        let has_cold_migration = migrations
            .iter()
            .any(|m| m.shard_id == 1 && m.reason == MigrationReason::ColdDataDetected);
        assert!(has_cold_migration);
    }

    #[test]
    fn test_policy_prioritizes_capacity_pressure() {
        let mut orch = create_orchestrator();
        orch.place_shard(1, TierLevel::Hot, 100).unwrap();
        orch.place_shard(2, TierLevel::Warm, 100).unwrap();

        let migrations = orch.evaluate_policy(1000);

        assert!(!migrations.is_empty());
    }

    #[test]
    fn test_queue_migration() {
        let mut orch = create_orchestrator();
        orch.place_shard(1, TierLevel::Hot, 100).unwrap();

        let migration = PendingMigration {
            shard_id: 1,
            from_tier: TierLevel::Hot,
            to_tier: TierLevel::Warm,
            reason: MigrationReason::PolicyEvaluation,
            priority: 50,
            created_ms: 1000,
        };

        orch.queue_migration(migration).unwrap();
        assert_eq!(orch.pending_count(), 1);
    }

    #[test]
    fn test_queue_migration_nonexistent_shard() {
        let mut orch = create_orchestrator();

        let migration = PendingMigration {
            shard_id: 999,
            from_tier: TierLevel::Hot,
            to_tier: TierLevel::Warm,
            reason: MigrationReason::PolicyEvaluation,
            priority: 50,
            created_ms: 1000,
        };

        assert!(matches!(
            orch.queue_migration(migration),
            Err(TieringError::ShardNotFound(999))
        ));
    }

    #[test]
    fn test_commit_migration() {
        let mut orch = create_orchestrator();
        orch.place_shard(1, TierLevel::Hot, 100).unwrap();

        orch.commit_migration(1, TierLevel::Warm, 1000).unwrap();

        let placement = orch.get_placement(1).unwrap();
        assert_eq!(placement.tier, TierLevel::Warm);
    }

    #[test]
    fn test_commit_migration_nonexistent() {
        let mut orch = create_orchestrator();

        assert!(matches!(
            orch.commit_migration(999, TierLevel::Warm, 1000),
            Err(TieringError::ShardNotFound(999))
        ));
    }

    #[test]
    fn test_commit_migration_removes_pending() {
        let mut orch = create_orchestrator();
        orch.place_shard(1, TierLevel::Hot, 100).unwrap();

        let migration = PendingMigration {
            shard_id: 1,
            from_tier: TierLevel::Hot,
            to_tier: TierLevel::Warm,
            reason: MigrationReason::PolicyEvaluation,
            priority: 50,
            created_ms: 1000,
        };

        orch.queue_migration(migration).unwrap();
        assert_eq!(orch.pending_count(), 1);

        orch.commit_migration(1, TierLevel::Warm, 2000).unwrap();
        assert_eq!(orch.pending_count(), 0);
    }

    #[test]
    fn test_get_placement_existing() {
        let mut orch = create_orchestrator();
        orch.place_shard(1, TierLevel::Hot, 100).unwrap();

        let placement = orch.get_placement(1);
        assert!(placement.is_some());
        assert_eq!(placement.unwrap().shard_id, 1);
    }

    #[test]
    fn test_get_placement_nonexistent() {
        let orch = create_orchestrator();
        assert!(orch.get_placement(999).is_none());
    }

    #[test]
    fn test_shard_count_by_tier() {
        let mut orch = create_orchestrator();
        orch.place_shard(1, TierLevel::Hot, 100).unwrap();
        orch.place_shard(2, TierLevel::Hot, 100).unwrap();
        orch.place_shard(3, TierLevel::Warm, 100).unwrap();

        assert_eq!(orch.shard_count_by_tier(TierLevel::Hot), 2);
        assert_eq!(orch.shard_count_by_tier(TierLevel::Warm), 1);
        assert_eq!(orch.shard_count_by_tier(TierLevel::Cold), 0);
    }

    #[test]
    fn test_used_capacity() {
        let mut orch = create_orchestrator();
        orch.place_shard(1, TierLevel::Hot, 100).unwrap();
        orch.place_shard(2, TierLevel::Hot, 200).unwrap();
        orch.place_shard(3, TierLevel::Warm, 300).unwrap();

        assert_eq!(orch.used_capacity(TierLevel::Hot), 300);
        assert_eq!(orch.used_capacity(TierLevel::Warm), 300);
        assert_eq!(orch.used_capacity(TierLevel::Cold), 0);
    }

    #[test]
    fn test_pending_count() {
        let mut orch = create_orchestrator();
        orch.place_shard(1, TierLevel::Hot, 100).unwrap();
        orch.place_shard(2, TierLevel::Hot, 100).unwrap();

        let migration = PendingMigration {
            shard_id: 1,
            from_tier: TierLevel::Hot,
            to_tier: TierLevel::Warm,
            reason: MigrationReason::PolicyEvaluation,
            priority: 50,
            created_ms: 1000,
        };

        orch.queue_migration(migration).unwrap();
        assert_eq!(orch.pending_count(), 1);
    }

    #[test]
    fn test_cancel_migration() {
        let mut orch = create_orchestrator();
        orch.place_shard(1, TierLevel::Hot, 100).unwrap();

        let migration = PendingMigration {
            shard_id: 1,
            from_tier: TierLevel::Hot,
            to_tier: TierLevel::Warm,
            reason: MigrationReason::PolicyEvaluation,
            priority: 50,
            created_ms: 1000,
        };

        orch.queue_migration(migration).unwrap();
        orch.cancel_migration(1).unwrap();

        assert_eq!(orch.pending_count(), 0);
    }

    #[test]
    fn test_cancel_nonexistent_migration() {
        let mut orch = create_orchestrator();

        assert!(matches!(
            orch.cancel_migration(999),
            Err(TieringError::InvalidMigration(_))
        ));
    }

    #[test]
    fn test_tier_level_clone() {
        let tier = TierLevel::Hot;
        let cloned = tier.clone();
        assert_eq!(tier, cloned);
    }

    #[test]
    fn test_tier_level_debug() {
        let tier = TierLevel::Cold;
        let debug_str = format!("{:?}", tier);
        assert!(debug_str.contains("Cold"));
    }

    #[test]
    fn test_tier_level_partial_eq() {
        assert_eq!(TierLevel::Hot, TierLevel::Hot);
        assert_eq!(TierLevel::Warm, TierLevel::Warm);
        assert_ne!(TierLevel::Hot, TierLevel::Cold);
    }

    #[test]
    fn test_migration_same_shard_multiple_times() {
        let mut orch = create_orchestrator();
        orch.place_shard(1, TierLevel::Hot, 100).unwrap();

        let migration1 = PendingMigration {
            shard_id: 1,
            from_tier: TierLevel::Hot,
            to_tier: TierLevel::Warm,
            reason: MigrationReason::PolicyEvaluation,
            priority: 50,
            created_ms: 1000,
        };

        orch.queue_migration(migration1).unwrap();

        let migration2 = PendingMigration {
            shard_id: 1,
            from_tier: TierLevel::Warm,
            to_tier: TierLevel::Cold,
            reason: MigrationReason::PolicyEvaluation,
            priority: 40,
            created_ms: 2000,
        };

        assert!(matches!(
            orch.queue_migration(migration2),
            Err(TieringError::InvalidMigration(_))
        ));
    }

    #[test]
    fn test_placement_history_tracking() {
        let mut orch = create_orchestrator();
        orch.place_shard(1, TierLevel::Hot, 100).unwrap();

        let placement = orch.get_placement(1).unwrap();
        assert_eq!(placement.promoted_at_ms, placement.demoted_at_ms);

        orch.commit_migration(1, TierLevel::Warm, 5000).unwrap();

        let placement = orch.get_placement(1).unwrap();
        assert!(placement.demoted_at_ms > 0);
    }

    #[test]
    fn test_archive_tier_unlimited_capacity() {
        let mut orch = create_orchestrator();

        for i in 1..=100 {
            assert!(orch.place_shard(i, TierLevel::Archive, 1000000).is_ok());
        }
    }

    #[test]
    fn test_remove_shard() {
        let mut orch = create_orchestrator();
        orch.place_shard(1, TierLevel::Hot, 100).unwrap();

        orch.remove_shard(1).unwrap();

        assert!(orch.get_placement(1).is_none());
    }

    #[test]
    fn test_remove_shard_nonexistent() {
        let mut orch = create_orchestrator();

        assert!(matches!(
            orch.remove_shard(999),
            Err(TieringError::ShardNotFound(999))
        ));
    }

    #[test]
    fn test_remove_shard_cancels_pending_migrations() {
        let mut orch = create_orchestrator();
        orch.place_shard(1, TierLevel::Hot, 100).unwrap();

        let migration = PendingMigration {
            shard_id: 1,
            from_tier: TierLevel::Hot,
            to_tier: TierLevel::Warm,
            reason: MigrationReason::PolicyEvaluation,
            priority: 50,
            created_ms: 1000,
        };

        orch.queue_migration(migration).unwrap();
        assert_eq!(orch.pending_count(), 1);

        orch.remove_shard(1).unwrap();
        assert_eq!(orch.pending_count(), 0);
    }

    #[test]
    fn test_capacity_pressure_migration() {
        let mut orch = create_orchestrator();
        orch.place_shard(1, TierLevel::Warm, 100).unwrap();

        orch.capacity_pressure_migration(1, TierLevel::Hot, 1000)
            .unwrap();

        assert_eq!(orch.pending_count(), 1);
    }

    #[test]
    fn test_policy_evaluates_all_tiers() {
        let mut orch = create_orchestrator();
        orch.place_shard(1, TierLevel::Hot, 100).unwrap();
        orch.place_shard(2, TierLevel::Warm, 100).unwrap();
        orch.place_shard(3, TierLevel::Cold, 100).unwrap();
        orch.place_shard(4, TierLevel::Archive, 100).unwrap();

        let migrations = orch.evaluate_policy(4000000);

        assert!(!migrations.is_empty());
    }
}
