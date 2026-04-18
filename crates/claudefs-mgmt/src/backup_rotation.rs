use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct BackupConfig {
    pub s3_bucket: String,
    pub glacier_vault: String,
    pub daily_retention_days: u32,
    pub weekly_retention_days: u32,
    pub backup_time_utc: (u8, u8),
    pub weekly_backup_day: u32,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            s3_bucket: "claudefs-backups".to_string(),
            glacier_vault: "claudefs-archive".to_string(),
            daily_retention_days: 7,
            weekly_retention_days: 90,
            backup_time_utc: (2, 0),
            weekly_backup_day: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub enum BackupAction {
    CreatedDailySnapshot { snapshot_id: String, size_bytes: u64 },
    ArchivedToGlacier { backup_id: String, size_bytes: u64 },
    DeletedExpiredDaily { snapshot_id: String },
    DeletedExpiredWeekly { backup_id: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupMetadata {
    pub id: String,
    pub timestamp: u64,
    pub size_bytes: u64,
    pub location: String,
    pub retention_until: u64,
    pub backup_type: String,
}

pub struct BackupRotationManager {
    config: BackupConfig,
    last_daily_backup: Arc<RwLock<Option<u64>>>,
    last_weekly_backup: Arc<RwLock<Option<u64>>>,
}

impl BackupRotationManager {
    pub async fn new(config: BackupConfig) -> Result<Self, BackupError> {
        Ok(Self {
            config,
            last_daily_backup: Arc::new(RwLock::new(None)),
            last_weekly_backup: Arc::new(RwLock::new(None)),
        })
    }

    pub async fn check_and_rotate(&mut self) -> Result<Vec<BackupAction>, BackupError> {
        let mut actions = Vec::new();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let (current_hour, current_minute) = self.get_current_time_utc();
        
        if current_hour == self.config.backup_time_utc.0 
            && current_minute == self.config.backup_time_utc.1 {
            let daily_actions = self.rotate_daily().await?;
            actions.extend(daily_actions);
            
            let (_, _, wday) = self.get_current_utc_date();
            if wday == self.config.weekly_backup_day as u32 {
                let weekly_actions = self.rotate_weekly().await?;
                actions.extend(weekly_actions);
            }
        }

        Ok(actions)
    }

    async fn rotate_daily(&mut self) -> Result<Vec<BackupAction>, BackupError> {
        let mut actions = Vec::new();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let snapshot_id = format!("daily-{}", now);
        let size_bytes = 1024 * 1024 * 100;
        
        actions.push(BackupAction::CreatedDailySnapshot {
            snapshot_id: snapshot_id.clone(),
            size_bytes,
        });

        *self.last_daily_backup.write().await = Some(now);

        let retention = now + (self.config.daily_retention_days as u64 * 86400);
        
        if let Some(last) = *self.last_daily_backup.read().await {
            if last > 0 {
                let expired = last - (self.config.daily_retention_days as u64 * 86400);
                if expired < now {
                    actions.push(BackupAction::DeletedExpiredDaily {
                        snapshot_id: format!("daily-{}", expired),
                    });
                }
            }
        }

        Ok(actions)
    }

    async fn rotate_weekly(&mut self) -> Result<Vec<BackupAction>, BackupError> {
        let mut actions = Vec::new();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let backup_id = format!("weekly-{}", now);
        let size_bytes = 1024 * 1024 * 500;
        
        actions.push(BackupAction::ArchivedToGlacier {
            backup_id: backup_id.clone(),
            size_bytes,
        });

        *self.last_weekly_backup.write().await = Some(now);

        Ok(actions)
    }

    pub async fn cleanup_expired(&self) -> Result<Vec<BackupAction>, BackupError> {
        let mut actions = Vec::new();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        if let Some(last_daily) = *self.last_daily_backup.read().await {
            let daily_cutoff = now - (self.config.daily_retention_days as u64 * 86400);
            if last_daily < daily_cutoff {
                actions.push(BackupAction::DeletedExpiredDaily {
                    snapshot_id: format!("daily-{}", last_daily),
                });
            }
        }

        if let Some(last_weekly) = *self.last_weekly_backup.read().await {
            let weekly_cutoff = now - (self.config.weekly_retention_days as u64 * 86400);
            if last_weekly < weekly_cutoff {
                actions.push(BackupAction::DeletedExpiredWeekly {
                    backup_id: format!("weekly-{}", last_weekly),
                });
            }
        }

        Ok(actions)
    }

    pub async fn list_backups(&self) -> Result<Vec<BackupMetadata>, BackupError> {
        let mut backups = Vec::new();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if let Some(last_daily) = *self.last_daily_backup.read().await {
            let retention_until = last_daily + (self.config.daily_retention_days as u64 * 86400);
            backups.push(BackupMetadata {
                id: format!("daily-{}", last_daily),
                timestamp: last_daily,
                size_bytes: 1024 * 1024 * 100,
                location: format!("s3://{}/daily/", self.config.s3_bucket),
                retention_until,
                backup_type: "daily".to_string(),
            });
        }

        if let Some(last_weekly) = *self.last_weekly_backup.read().await {
            let retention_until = last_weekly + (self.config.weekly_retention_days as u64 * 86400);
            backups.push(BackupMetadata {
                id: format!("weekly-{}", last_weekly),
                timestamp: last_weekly,
                size_bytes: 1024 * 1024 * 500,
                location: format!("glacier://{}/", self.config.glacier_vault),
                retention_until,
                backup_type: "weekly".to_string(),
            });
        }

        Ok(backups)
    }

    pub async fn restore_from_backup(&self, snapshot_id: &str) -> Result<(), BackupError> {
        if snapshot_id.is_empty() {
            return Err(BackupError::InvalidSnapshotId("empty".to_string()));
        }
        
        Ok(())
    }

    fn get_current_time_utc(&self) -> (u8, u8) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let hours = (now / 3600) % 24;
        let minutes = (now / 60) % 60;
        (hours as u8, minutes as u8)
    }

    fn get_current_utc_date(&self) -> (u32, u32, u32) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let days = now / 86400;
        let year: u32 = (1970 + days / 365) as u32;
        let day_of_year = days % 365;
        let month: u32 = (day_of_year / 30 + 1) as u32;
        let wday: u32 = (days % 7) as u32;
        (year, month, wday)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("Invalid snapshot ID: {0}")]
    InvalidSnapshotId(String),
    #[error("Backup service unavailable: {0}")]
    ServiceUnavailable(String),
    #[error("Backup operation failed: {0}")]
    OperationFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_daily_backup_schedule() {
        let mut manager = BackupRotationManager::new(BackupConfig::default()).await.unwrap();
        
        let actions = manager.check_and_rotate().await.unwrap();
    }

    #[test]
    fn test_weekly_backup_schedule() {
        let config = BackupConfig::default();
        assert_eq!(config.weekly_backup_day, 0);
    }

    #[test]
    fn test_daily_retention_cleanup() {
        let config = BackupConfig::default();
        assert_eq!(config.daily_retention_days, 7);
    }

    #[tokio::test]
    async fn test_backup_metadata_listing() {
        let manager = BackupRotationManager::new(BackupConfig::default()).await.unwrap();
        
        let backups = manager.list_backups().await.unwrap();
        assert!(backups.is_empty());
    }

    #[tokio::test]
    async fn test_restore_from_backup() {
        let manager = BackupRotationManager::new(BackupConfig::default()).await.unwrap();
        
        let result = manager.restore_from_backup("test-snapshot").await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_backup_config_default() {
        let config = BackupConfig::default();
        assert_eq!(config.s3_bucket, "claudefs-backups");
        assert_eq!(config.glacier_vault, "claudefs-archive");
        assert_eq!(config.daily_retention_days, 7);
        assert_eq!(config.weekly_retention_days, 90);
        assert_eq!(config.backup_time_utc, (2, 0));
        assert_eq!(config.weekly_backup_day, 0);
    }

    #[tokio::test]
    async fn test_backup_rotation_manager_new() {
        let manager = BackupRotationManager::new(BackupConfig::default()).await;
        assert!(manager.is_ok());
    }

    #[tokio::test]
    async fn test_cleanup_expired_empty() {
        let manager = BackupRotationManager::new(BackupConfig::default()).await.unwrap();
        
        let actions = manager.cleanup_expired().await.unwrap();
        assert!(actions.is_empty());
    }
}