//! Connection migration for seamless request handoff during rolling upgrades or node failures.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Unique connection identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionId(pub u64);

/// State of a migration operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationState {
    /// Not migrating
    Idle,
    /// Migration is being prepared (negotiating with target)
    Preparing,
    /// Actively migrating in-flight requests
    Migrating,
    /// Migration completed successfully
    Completed,
    /// Migration failed
    Failed,
}

impl std::fmt::Display for MigrationState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MigrationState::Idle => write!(f, "Idle"),
            MigrationState::Preparing => write!(f, "Preparing"),
            MigrationState::Migrating => write!(f, "Migrating"),
            MigrationState::Completed => write!(f, "Completed"),
            MigrationState::Failed => write!(f, "Failed"),
        }
    }
}

/// Reason for initiating migration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationReason {
    /// Node is being drained for maintenance
    NodeDrain,
    /// Connection has degraded health
    HealthDegraded,
    /// Connection lost (unplanned)
    ConnectionLost,
    /// Load rebalancing
    LoadBalance,
    /// Protocol version upgrade
    VersionUpgrade,
}

impl std::fmt::Display for MigrationReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MigrationReason::NodeDrain => write!(f, "NodeDrain"),
            MigrationReason::HealthDegraded => write!(f, "HealthDegraded"),
            MigrationReason::ConnectionLost => write!(f, "ConnectionLost"),
            MigrationReason::LoadBalance => write!(f, "LoadBalance"),
            MigrationReason::VersionUpgrade => write!(f, "VersionUpgrade"),
        }
    }
}

impl std::fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ConnectionId({})", self.0)
    }
}

/// A single migration operation tracking
#[derive(Debug, Clone)]
pub struct MigrationRecord {
    pub id: u64,
    pub source: ConnectionId,
    pub target: ConnectionId,
    pub reason: MigrationReason,
    pub state: MigrationState,
    pub requests_migrated: u64,
    pub requests_failed: u64,
    pub started_at_ms: u64,
    pub completed_at_ms: Option<u64>,
}

/// Configuration for connection migration
#[derive(Debug, Clone)]
pub struct MigrationConfig {
    pub max_concurrent_migrations: usize,
    pub migration_timeout_ms: u64,
    pub retry_failed_requests: bool,
    pub max_retries: u32,
    pub quiesce_timeout_ms: u64,
    pub enabled: bool,
}

impl Default for MigrationConfig {
    fn default() -> Self {
        Self {
            max_concurrent_migrations: 4,
            migration_timeout_ms: 10000,
            retry_failed_requests: true,
            max_retries: 3,
            quiesce_timeout_ms: 5000,
            enabled: true,
        }
    }
}

/// Errors that can occur during migration operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationError {
    /// Too many concurrent migrations
    TooManyConcurrent { max: usize },
    /// Connection is already being migrated
    AlreadyMigrating { connection: ConnectionId },
    /// Migration not found
    MigrationNotFound { id: u64 },
    /// Migration is disabled
    Disabled,
}

impl std::fmt::Display for MigrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MigrationError::TooManyConcurrent { max } => {
                write!(f, "too many concurrent migrations (max: {})", max)
            }
            MigrationError::AlreadyMigrating { connection } => {
                write!(f, "connection {} is already migrating", connection)
            }
            MigrationError::MigrationNotFound { id } => {
                write!(f, "migration {} not found", id)
            }
            MigrationError::Disabled => {
                write!(f, "migration is disabled")
            }
        }
    }
}

/// Stats tracking for migration operations
pub struct MigrationStats {
    total_migrations: AtomicU64,
    successful_migrations: AtomicU64,
    failed_migrations: AtomicU64,
    requests_migrated: AtomicU64,
    requests_failed: AtomicU64,
}

impl MigrationStats {
    pub fn new() -> Self {
        Self {
            total_migrations: AtomicU64::new(0),
            successful_migrations: AtomicU64::new(0),
            failed_migrations: AtomicU64::new(0),
            requests_migrated: AtomicU64::new(0),
            requests_failed: AtomicU64::new(0),
        }
    }

    pub fn snapshot(&self) -> MigrationStatsSnapshot {
        MigrationStatsSnapshot {
            total_migrations: self.total_migrations.load(Ordering::Relaxed),
            successful_migrations: self.successful_migrations.load(Ordering::Relaxed),
            failed_migrations: self.failed_migrations.load(Ordering::Relaxed),
            requests_migrated: self.requests_migrated.load(Ordering::Relaxed),
            requests_failed: self.requests_failed.load(Ordering::Relaxed),
            active_migrations: 0,
        }
    }

    pub fn increment_total(&self) {
        self.total_migrations.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_successful(&self) {
        self.successful_migrations.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_failed(&self) {
        self.failed_migrations.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_requests_migrated(&self, count: u64) {
        self.requests_migrated.fetch_add(count, Ordering::Relaxed);
    }

    pub fn add_requests_failed(&self, count: u64) {
        self.requests_failed.fetch_add(count, Ordering::Relaxed);
    }
}

impl Default for MigrationStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot of migration statistics
#[derive(Debug, Clone)]
pub struct MigrationStatsSnapshot {
    pub total_migrations: u64,
    pub successful_migrations: u64,
    pub failed_migrations: u64,
    pub requests_migrated: u64,
    pub requests_failed: u64,
    pub active_migrations: usize,
}

/// Manages connection migrations
pub struct MigrationManager {
    config: MigrationConfig,
    active_migrations: Mutex<Vec<MigrationRecord>>,
    next_migration_id: AtomicU64,
    stats: MigrationStats,
}

impl MigrationManager {
    pub fn new(config: MigrationConfig) -> Self {
        Self {
            config,
            active_migrations: Mutex::new(Vec::new()),
            next_migration_id: AtomicU64::new(1),
            stats: MigrationStats::new(),
        }
    }

    fn current_time_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    fn is_connection_migrating(
        &self,
        conn_id: ConnectionId,
        migrations: &[MigrationRecord],
    ) -> bool {
        migrations.iter().any(|m| {
            (m.source == conn_id || m.target == conn_id)
                && (m.state == MigrationState::Preparing || m.state == MigrationState::Migrating)
        })
    }

    pub fn start_migration(
        &self,
        source: ConnectionId,
        target: ConnectionId,
        reason: MigrationReason,
    ) -> Result<u64, MigrationError> {
        if !self.config.enabled {
            return Err(MigrationError::Disabled);
        }

        let mut migrations = self.active_migrations.lock().unwrap();

        if migrations.len() >= self.config.max_concurrent_migrations {
            return Err(MigrationError::TooManyConcurrent {
                max: self.config.max_concurrent_migrations,
            });
        }

        if self.is_connection_migrating(source, &migrations) {
            return Err(MigrationError::AlreadyMigrating { connection: source });
        }

        let id = self.next_migration_id.fetch_add(1, Ordering::SeqCst);
        let started_at_ms = Self::current_time_ms();

        let record = MigrationRecord {
            id,
            source,
            target,
            reason,
            state: MigrationState::Preparing,
            requests_migrated: 0,
            requests_failed: 0,
            started_at_ms,
            completed_at_ms: None,
        };

        migrations.push(record);
        self.stats.increment_total();

        Ok(id)
    }

    pub fn record_request_migrated(&self, migration_id: u64) -> bool {
        let mut migrations = self.active_migrations.lock().unwrap();
        if let Some(record) = migrations.iter_mut().find(|m| m.id == migration_id) {
            record.requests_migrated += 1;
            if record.state == MigrationState::Preparing {
                record.state = MigrationState::Migrating;
            }
            self.stats.add_requests_migrated(1);
            true
        } else {
            false
        }
    }

    pub fn record_request_failed(&self, migration_id: u64) -> bool {
        let mut migrations = self.active_migrations.lock().unwrap();
        if let Some(record) = migrations.iter_mut().find(|m| m.id == migration_id) {
            record.requests_failed += 1;
            self.stats.add_requests_failed(1);
            true
        } else {
            false
        }
    }

    pub fn complete_migration(&self, migration_id: u64) -> bool {
        let mut migrations = self.active_migrations.lock().unwrap();
        if let Some(record) = migrations.iter_mut().find(|m| m.id == migration_id) {
            record.state = MigrationState::Completed;
            record.completed_at_ms = Some(Self::current_time_ms());
            self.stats.increment_successful();
            true
        } else {
            false
        }
    }

    pub fn fail_migration(&self, migration_id: u64) -> bool {
        let mut migrations = self.active_migrations.lock().unwrap();
        if let Some(record) = migrations.iter_mut().find(|m| m.id == migration_id) {
            record.state = MigrationState::Failed;
            record.completed_at_ms = Some(Self::current_time_ms());
            self.stats.increment_failed();
            true
        } else {
            false
        }
    }

    pub fn get_migration(&self, migration_id: u64) -> Option<MigrationRecord> {
        let migrations = self.active_migrations.lock().unwrap();
        migrations.iter().find(|m| m.id == migration_id).cloned()
    }

    pub fn active_count(&self) -> usize {
        let migrations = self.active_migrations.lock().unwrap();
        migrations
            .iter()
            .filter(|m| {
                m.state == MigrationState::Preparing || m.state == MigrationState::Migrating
            })
            .count()
    }

    pub fn is_migrating(&self, conn_id: ConnectionId) -> bool {
        let migrations = self.active_migrations.lock().unwrap();
        self.is_connection_migrating(conn_id, &migrations)
    }

    /// Returns a snapshot of migration statistics.
    pub fn stats(&self) -> MigrationStatsSnapshot {
        let mut snapshot = self.stats.snapshot();
        snapshot.active_migrations = self.active_count();
        snapshot
    }
}

impl Default for MigrationManager {
    fn default() -> Self {
        Self::new(MigrationConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = MigrationConfig::default();
        assert_eq!(config.max_concurrent_migrations, 4);
        assert_eq!(config.migration_timeout_ms, 10000);
        assert_eq!(config.retry_failed_requests, true);
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.quiesce_timeout_ms, 5000);
        assert_eq!(config.enabled, true);
    }

    #[test]
    fn test_connection_id() {
        let id1 = ConnectionId(42);
        let id2 = ConnectionId(42);
        let id3 = ConnectionId(100);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
        assert_eq!(id1.0, 42);
    }

    #[test]
    fn test_migration_state_values() {
        let states = [
            MigrationState::Idle,
            MigrationState::Preparing,
            MigrationState::Migrating,
            MigrationState::Completed,
            MigrationState::Failed,
        ];

        for (i, s1) in states.iter().enumerate() {
            for (j, s2) in states.iter().enumerate() {
                if i == j {
                    assert_eq!(s1, s2);
                } else {
                    assert_ne!(s1, s2);
                }
            }
        }
    }

    #[test]
    fn test_migration_reason_values() {
        let reasons = [
            MigrationReason::NodeDrain,
            MigrationReason::HealthDegraded,
            MigrationReason::ConnectionLost,
            MigrationReason::LoadBalance,
            MigrationReason::VersionUpgrade,
        ];

        for (i, r1) in reasons.iter().enumerate() {
            for (j, r2) in reasons.iter().enumerate() {
                if i == j {
                    assert_eq!(r1, r2);
                } else {
                    assert_ne!(r1, r2);
                }
            }
        }
    }

    #[test]
    fn test_manager_initial_state() {
        let manager = MigrationManager::new(MigrationConfig::default());
        assert_eq!(manager.active_count(), 0);
        assert!(manager.get_migration(1).is_none());
    }

    #[test]
    fn test_start_migration() {
        let config = MigrationConfig {
            max_concurrent_migrations: 4,
            enabled: true,
            ..Default::default()
        };
        let manager = MigrationManager::new(config);

        let id = manager
            .start_migration(ConnectionId(1), ConnectionId(2), MigrationReason::NodeDrain)
            .unwrap();

        assert_eq!(id, 1);
        let record = manager.get_migration(id).unwrap();
        assert_eq!(record.source, ConnectionId(1));
        assert_eq!(record.target, ConnectionId(2));
        assert_eq!(record.reason, MigrationReason::NodeDrain);
        assert_eq!(record.state, MigrationState::Preparing);
    }

    #[test]
    fn test_start_migration_returns_unique_ids() {
        let config = MigrationConfig {
            max_concurrent_migrations: 10,
            enabled: true,
            ..Default::default()
        };
        let manager = MigrationManager::new(config);

        let id1 = manager
            .start_migration(ConnectionId(1), ConnectionId(2), MigrationReason::NodeDrain)
            .unwrap();
        let id2 = manager
            .start_migration(
                ConnectionId(3),
                ConnectionId(4),
                MigrationReason::LoadBalance,
            )
            .unwrap();
        let id3 = manager
            .start_migration(
                ConnectionId(5),
                ConnectionId(6),
                MigrationReason::VersionUpgrade,
            )
            .unwrap();

        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id3, 3);
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
    }

    #[test]
    fn test_start_migration_too_many() {
        let config = MigrationConfig {
            max_concurrent_migrations: 2,
            enabled: true,
            ..Default::default()
        };
        let manager = MigrationManager::new(config);

        manager
            .start_migration(ConnectionId(1), ConnectionId(2), MigrationReason::NodeDrain)
            .unwrap();
        manager
            .start_migration(ConnectionId(3), ConnectionId(4), MigrationReason::NodeDrain)
            .unwrap();

        let result =
            manager.start_migration(ConnectionId(5), ConnectionId(6), MigrationReason::NodeDrain);

        assert!(matches!(
            result,
            Err(MigrationError::TooManyConcurrent { max: 2 })
        ));
    }

    #[test]
    fn test_start_migration_already_migrating() {
        let config = MigrationConfig {
            max_concurrent_migrations: 4,
            enabled: true,
            ..Default::default()
        };
        let manager = MigrationManager::new(config);

        manager
            .start_migration(ConnectionId(1), ConnectionId(2), MigrationReason::NodeDrain)
            .unwrap();

        let result = manager.start_migration(
            ConnectionId(1),
            ConnectionId(3),
            MigrationReason::LoadBalance,
        );

        assert!(matches!(
            result,
            Err(MigrationError::AlreadyMigrating {
                connection: ConnectionId(1)
            })
        ));
    }

    #[test]
    fn test_start_migration_disabled() {
        let config = MigrationConfig {
            enabled: false,
            ..Default::default()
        };
        let manager = MigrationManager::new(config);

        let result =
            manager.start_migration(ConnectionId(1), ConnectionId(2), MigrationReason::NodeDrain);

        assert!(matches!(result, Err(MigrationError::Disabled)));
    }

    #[test]
    fn test_record_request_migrated() {
        let manager = MigrationManager::new(MigrationConfig::default());
        let id = manager
            .start_migration(ConnectionId(1), ConnectionId(2), MigrationReason::NodeDrain)
            .unwrap();

        assert!(manager.record_request_migrated(id));
        assert!(manager.record_request_migrated(id));

        let record = manager.get_migration(id).unwrap();
        assert_eq!(record.requests_migrated, 2);
        assert_eq!(record.state, MigrationState::Migrating);
    }

    #[test]
    fn test_record_request_failed() {
        let manager = MigrationManager::new(MigrationConfig::default());
        let id = manager
            .start_migration(ConnectionId(1), ConnectionId(2), MigrationReason::NodeDrain)
            .unwrap();

        assert!(manager.record_request_failed(id));
        assert!(manager.record_request_failed(id));

        let record = manager.get_migration(id).unwrap();
        assert_eq!(record.requests_failed, 2);
    }

    #[test]
    fn test_complete_migration() {
        let manager = MigrationManager::new(MigrationConfig::default());
        let id = manager
            .start_migration(ConnectionId(1), ConnectionId(2), MigrationReason::NodeDrain)
            .unwrap();

        assert!(manager.complete_migration(id));

        let record = manager.get_migration(id).unwrap();
        assert_eq!(record.state, MigrationState::Completed);
        assert!(record.completed_at_ms.is_some());
    }

    #[test]
    fn test_fail_migration() {
        let manager = MigrationManager::new(MigrationConfig::default());
        let id = manager
            .start_migration(ConnectionId(1), ConnectionId(2), MigrationReason::NodeDrain)
            .unwrap();

        assert!(manager.fail_migration(id));

        let record = manager.get_migration(id).unwrap();
        assert_eq!(record.state, MigrationState::Failed);
        assert!(record.completed_at_ms.is_some());
    }

    #[test]
    fn test_get_migration() {
        let manager = MigrationManager::new(MigrationConfig::default());
        let id = manager
            .start_migration(ConnectionId(1), ConnectionId(2), MigrationReason::NodeDrain)
            .unwrap();

        let record = manager.get_migration(id).unwrap();
        assert_eq!(record.id, id);
    }

    #[test]
    fn test_get_nonexistent_migration() {
        let manager = MigrationManager::new(MigrationConfig::default());
        let result = manager.get_migration(999);
        assert!(result.is_none());
    }

    #[test]
    fn test_active_count() {
        let config = MigrationConfig {
            max_concurrent_migrations: 10,
            enabled: true,
            ..Default::default()
        };
        let manager = MigrationManager::new(config);

        let id1 = manager
            .start_migration(ConnectionId(1), ConnectionId(2), MigrationReason::NodeDrain)
            .unwrap();
        let id2 = manager
            .start_migration(
                ConnectionId(3),
                ConnectionId(4),
                MigrationReason::LoadBalance,
            )
            .unwrap();

        assert_eq!(manager.active_count(), 2);

        manager.complete_migration(id1);
        assert_eq!(manager.active_count(), 1);

        manager.fail_migration(id2);
        assert_eq!(manager.active_count(), 0);
    }

    #[test]
    fn test_is_migrating() {
        let manager = MigrationManager::new(MigrationConfig::default());

        assert!(!manager.is_migrating(ConnectionId(1)));

        manager
            .start_migration(ConnectionId(1), ConnectionId(2), MigrationReason::NodeDrain)
            .unwrap();

        assert!(manager.is_migrating(ConnectionId(1)));
        assert!(manager.is_migrating(ConnectionId(2)));
        assert!(!manager.is_migrating(ConnectionId(3)));

        manager.complete_migration(1);
        assert!(!manager.is_migrating(ConnectionId(1)));
    }

    #[test]
    fn test_stats() {
        let config = MigrationConfig {
            max_concurrent_migrations: 10,
            enabled: true,
            ..Default::default()
        };
        let manager = MigrationManager::new(config);

        let id1 = manager
            .start_migration(ConnectionId(1), ConnectionId(2), MigrationReason::NodeDrain)
            .unwrap();
        manager.record_request_migrated(id1);
        manager.record_request_migrated(id1);
        manager.record_request_failed(id1);
        manager.complete_migration(id1);

        let id2 = manager
            .start_migration(
                ConnectionId(3),
                ConnectionId(4),
                MigrationReason::HealthDegraded,
            )
            .unwrap();
        manager.fail_migration(id2);

        let stats = manager.stats();
        assert_eq!(stats.total_migrations, 2);
        assert_eq!(stats.successful_migrations, 1);
        assert_eq!(stats.failed_migrations, 1);
        assert_eq!(stats.requests_migrated, 2);
        assert_eq!(stats.requests_failed, 1);
    }

    #[test]
    fn test_complete_removes_from_active() {
        let config = MigrationConfig {
            max_concurrent_migrations: 10,
            enabled: true,
            ..Default::default()
        };
        let manager = MigrationManager::new(config);

        let id1 = manager
            .start_migration(ConnectionId(1), ConnectionId(2), MigrationReason::NodeDrain)
            .unwrap();
        let id2 = manager
            .start_migration(
                ConnectionId(3),
                ConnectionId(4),
                MigrationReason::LoadBalance,
            )
            .unwrap();

        assert_eq!(manager.active_count(), 2);

        manager.complete_migration(id1);
        assert_eq!(manager.active_count(), 1);
        assert!(manager.get_migration(id1).is_some());

        manager.complete_migration(id2);
        assert_eq!(manager.active_count(), 0);
    }

    #[test]
    fn test_migration_record_fields() {
        let manager = MigrationManager::new(MigrationConfig::default());
        let id = manager
            .start_migration(
                ConnectionId(100),
                ConnectionId(200),
                MigrationReason::VersionUpgrade,
            )
            .unwrap();

        let record = manager.get_migration(id).unwrap();

        assert_eq!(record.id, id);
        assert_eq!(record.source, ConnectionId(100));
        assert_eq!(record.target, ConnectionId(200));
        assert_eq!(record.reason, MigrationReason::VersionUpgrade);
        assert_eq!(record.state, MigrationState::Preparing);
        assert_eq!(record.requests_migrated, 0);
        assert_eq!(record.requests_failed, 0);
        assert!(record.completed_at_ms.is_none());
        assert!(record.started_at_ms > 0);
    }
}
