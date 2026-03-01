use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SnapshotError {
    #[error("Snapshot not found: {0}")]
    NotFound(String),
    #[error("Snapshot already exists: {0}")]
    AlreadyExists(String),
    #[error("Cannot delete snapshot: {0} (has active restore)")]
    HasActiveRestore(String),
    #[error("Restore error: {0}")]
    RestoreError(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SnapshotState {
    Creating,
    Available,
    Archiving,
    Archived,
    Restoring,
    Deleting,
}

impl SnapshotState {
    pub fn is_available(&self) -> bool {
        matches!(self, SnapshotState::Available | SnapshotState::Archived)
    }

    pub fn is_on_flash(&self) -> bool {
        matches!(
            self,
            SnapshotState::Available | SnapshotState::Creating | SnapshotState::Archiving
        )
    }

    pub fn is_on_s3(&self) -> bool {
        matches!(self, SnapshotState::Archived | SnapshotState::Restoring)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub name: String,
    pub source_path: String,
    pub state: SnapshotState,
    pub created_at: u64,
    pub size_bytes: u64,
    pub unique_bytes: u64,
    pub inode_count: u64,
    pub s3_key: Option<String>,
    pub tags: HashMap<String, String>,
    pub retention_days: Option<u64>,
}

impl Snapshot {
    pub fn new(name: String, source_path: String, size_bytes: u64, inode_count: u64) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Self {
            name,
            source_path,
            state: SnapshotState::Available,
            created_at: now,
            size_bytes,
            unique_bytes: size_bytes,
            inode_count,
            s3_key: None,
            tags: HashMap::new(),
            retention_days: None,
        }
    }

    pub fn is_expired(&self, current_time: u64) -> bool {
        if let Some(days) = self.retention_days {
            self.age_days(current_time) > days
        } else {
            false
        }
    }

    pub fn age_days(&self, current_time: u64) -> u64 {
        let elapsed = current_time.saturating_sub(self.created_at);
        elapsed / (24 * 60 * 60)
    }

    pub fn dedup_ratio(&self) -> f64 {
        if self.unique_bytes == 0 {
            return 1.0;
        }
        self.size_bytes as f64 / self.unique_bytes as f64
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreJob {
    pub id: String,
    pub snapshot_name: String,
    pub target_path: String,
    pub started_at: u64,
    pub total_bytes: u64,
    pub restored_bytes: u64,
    pub state: RestoreState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RestoreState {
    Running,
    Complete,
    Failed(String),
}

impl RestoreJob {
    pub fn new(id: String, snapshot_name: String, target_path: String, total_bytes: u64) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Self {
            id,
            snapshot_name,
            target_path,
            started_at: now,
            total_bytes,
            restored_bytes: 0,
            state: RestoreState::Running,
        }
    }

    pub fn percent_complete(&self) -> f64 {
        if self.total_bytes == 0 {
            return 100.0;
        }
        (self.restored_bytes as f64 / self.total_bytes as f64) * 100.0
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self.state, RestoreState::Complete | RestoreState::Failed(_))
    }
}

pub struct SnapshotCatalog {
    snapshots: HashMap<String, Snapshot>,
    restore_jobs: HashMap<String, RestoreJob>,
}

impl SnapshotCatalog {
    pub fn new() -> Self {
        Self {
            snapshots: HashMap::new(),
            restore_jobs: HashMap::new(),
        }
    }

    pub fn create_snapshot(
        &mut self,
        name: String,
        source_path: String,
        size_bytes: u64,
        inode_count: u64,
    ) -> Result<&Snapshot, SnapshotError> {
        if self.snapshots.contains_key(&name) {
            return Err(SnapshotError::AlreadyExists(name));
        }
        let snapshot = Snapshot::new(name.clone(), source_path, size_bytes, inode_count);
        self.snapshots.insert(name.clone(), snapshot);
        Ok(self.snapshots.get(&name).unwrap())
    }

    pub fn get_snapshot(&self, name: &str) -> Option<&Snapshot> {
        self.snapshots.get(name)
    }

    pub fn get_snapshot_mut(&mut self, name: &str) -> Option<&mut Snapshot> {
        self.snapshots.get_mut(name)
    }

    pub fn delete_snapshot(&mut self, name: &str) -> Result<Snapshot, SnapshotError> {
        for (_, job) in &self.restore_jobs {
            if job.snapshot_name == name && !job.is_terminal() {
                return Err(SnapshotError::HasActiveRestore(name.to_string()));
            }
        }
        self.snapshots
            .remove(name)
            .ok_or_else(|| SnapshotError::NotFound(name.to_string()))
    }

    pub fn list_for_path(&self, source_path: &str) -> Vec<&Snapshot> {
        self.snapshots
            .values()
            .filter(|s| s.source_path == source_path)
            .collect()
    }

    pub fn list_all(&self) -> Vec<&Snapshot> {
        let mut snapshots: Vec<&Snapshot> = self.snapshots.values().collect();
        snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        snapshots
    }

    pub fn expired_snapshots(&self, current_time: u64) -> Vec<&Snapshot> {
        self.snapshots
            .values()
            .filter(|s| s.is_expired(current_time))
            .collect()
    }

    pub fn start_restore(
        &mut self,
        snapshot_name: &str,
        target_path: String,
        total_bytes: u64,
    ) -> Result<&RestoreJob, SnapshotError> {
        let _snapshot = self
            .snapshots
            .get(snapshot_name)
            .ok_or_else(|| SnapshotError::NotFound(snapshot_name.to_string()))?;

        let id = uuid::Uuid::new_v4().to_string();
        let job = RestoreJob::new(
            id.clone(),
            snapshot_name.to_string(),
            target_path,
            total_bytes,
        );
        self.restore_jobs.insert(id.clone(), job);
        Ok(self.restore_jobs.get(&id).unwrap())
    }

    pub fn get_restore_job(&self, job_id: &str) -> Option<&RestoreJob> {
        self.restore_jobs.get(job_id)
    }

    pub fn update_restore_progress(
        &mut self,
        job_id: &str,
        restored_bytes: u64,
    ) -> Result<(), SnapshotError> {
        let job = self
            .restore_jobs
            .get_mut(job_id)
            .ok_or_else(|| SnapshotError::RestoreError("Job not found".to_string()))?;
        job.restored_bytes = restored_bytes;
        Ok(())
    }

    pub fn complete_restore(&mut self, job_id: &str) -> Result<(), SnapshotError> {
        let job = self
            .restore_jobs
            .get_mut(job_id)
            .ok_or_else(|| SnapshotError::RestoreError("Job not found".to_string()))?;
        job.state = RestoreState::Complete;
        Ok(())
    }

    pub fn snapshot_count(&self) -> usize {
        self.snapshots.len()
    }

    pub fn total_snapshot_bytes(&self) -> u64 {
        self.snapshots.values().map(|s| s.size_bytes).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_state_is_available() {
        assert!(SnapshotState::Available.is_available());
        assert!(SnapshotState::Archived.is_available());
        assert!(!SnapshotState::Creating.is_available());
    }

    #[test]
    fn test_snapshot_state_is_on_flash() {
        assert!(SnapshotState::Available.is_on_flash());
        assert!(SnapshotState::Creating.is_on_flash());
        assert!(SnapshotState::Archiving.is_on_flash());
        assert!(!SnapshotState::Archived.is_on_flash());
    }

    #[test]
    fn test_snapshot_state_is_on_s3() {
        assert!(SnapshotState::Archived.is_on_s3());
        assert!(SnapshotState::Restoring.is_on_s3());
        assert!(!SnapshotState::Available.is_on_s3());
    }

    #[test]
    fn test_snapshot_new() {
        let snapshot = Snapshot::new("snap1".to_string(), "/data".to_string(), 1000, 100);
        assert_eq!(snapshot.name, "snap1");
        assert_eq!(snapshot.state, SnapshotState::Available);
    }

    #[test]
    fn test_is_expired_no_retention() {
        let snapshot = Snapshot::new("snap1".to_string(), "/data".to_string(), 1000, 100);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(!snapshot.is_expired(now + 30 * 24 * 60 * 60));
    }

    #[test]
    fn test_is_expired_within_retention() {
        let mut snapshot = Snapshot::new("snap1".to_string(), "/data".to_string(), 1000, 100);
        snapshot.retention_days = Some(30);
        let now = snapshot.created_at + 10 * 24 * 60 * 60;
        assert!(!snapshot.is_expired(now));
    }

    #[test]
    fn test_is_expired_past_retention() {
        let mut snapshot = Snapshot::new("snap1".to_string(), "/data".to_string(), 1000, 100);
        snapshot.retention_days = Some(30);
        let now = snapshot.created_at + 40 * 24 * 60 * 60;
        assert!(snapshot.is_expired(now));
    }

    #[test]
    fn test_age_days() {
        let snapshot = Snapshot::new("snap1".to_string(), "/data".to_string(), 1000, 100);
        let later = snapshot.created_at + 5 * 24 * 60 * 60;
        assert_eq!(snapshot.age_days(later), 5);
    }

    #[test]
    fn test_dedup_ratio_no_sharing() {
        let snapshot = Snapshot::new("snap1".to_string(), "/data".to_string(), 1000, 100);
        assert!((snapshot.dedup_ratio() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_dedup_ratio_with_sharing() {
        let mut snapshot = Snapshot::new("snap1".to_string(), "/data".to_string(), 1000, 100);
        snapshot.unique_bytes = 500;
        assert!((snapshot.dedup_ratio() - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_restore_job_new() {
        let job = RestoreJob::new(
            "id1".to_string(),
            "snap1".to_string(),
            "/target".to_string(),
            1000,
        );
        assert_eq!(job.state, RestoreState::Running);
        assert_eq!(job.restored_bytes, 0);
    }

    #[test]
    fn test_restore_job_percent_complete_zero() {
        let job = RestoreJob::new(
            "id1".to_string(),
            "snap1".to_string(),
            "/target".to_string(),
            1000,
        );
        assert!((job.percent_complete() - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_restore_job_percent_complete_full() {
        let mut job = RestoreJob::new(
            "id1".to_string(),
            "snap1".to_string(),
            "/target".to_string(),
            1000,
        );
        job.restored_bytes = 1000;
        assert!((job.percent_complete() - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_restore_job_is_terminal() {
        let running = RestoreJob::new(
            "id1".to_string(),
            "snap1".to_string(),
            "/target".to_string(),
            1000,
        );
        assert!(!running.is_terminal());

        let complete = RestoreJob {
            state: RestoreState::Complete,
            id: "id1".to_string(),
            snapshot_name: "snap1".to_string(),
            target_path: "/target".to_string(),
            started_at: running.started_at,
            total_bytes: running.total_bytes,
            restored_bytes: running.restored_bytes,
        };
        assert!(complete.is_terminal());

        let failed = RestoreJob {
            state: RestoreState::Failed("error".to_string()),
            id: "id1".to_string(),
            snapshot_name: "snap1".to_string(),
            target_path: "/target".to_string(),
            started_at: running.started_at,
            total_bytes: running.total_bytes,
            restored_bytes: running.restored_bytes,
        };
        assert!(failed.is_terminal());
    }

    #[test]
    fn test_create_snapshot() {
        let mut catalog = SnapshotCatalog::new();
        let result = catalog.create_snapshot("snap1".to_string(), "/data".to_string(), 1000, 100);
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_snapshot_already_exists() {
        let mut catalog = SnapshotCatalog::new();
        catalog
            .create_snapshot("snap1".to_string(), "/data".to_string(), 1000, 100)
            .unwrap();
        let result = catalog.create_snapshot("snap1".to_string(), "/data".to_string(), 1000, 100);
        assert!(matches!(result, Err(SnapshotError::AlreadyExists(_))));
    }

    #[test]
    fn test_get_snapshot_unknown() {
        let catalog = SnapshotCatalog::new();
        assert!(catalog.get_snapshot("unknown").is_none());
    }

    #[test]
    fn test_delete_snapshot() {
        let mut catalog = SnapshotCatalog::new();
        catalog
            .create_snapshot("snap1".to_string(), "/data".to_string(), 1000, 100)
            .unwrap();
        let result = catalog.delete_snapshot("snap1");
        assert!(result.is_ok());
    }

    #[test]
    fn test_delete_snapshot_not_found() {
        let mut catalog = SnapshotCatalog::new();
        let result = catalog.delete_snapshot("unknown");
        assert!(matches!(result, Err(SnapshotError::NotFound(_))));
    }

    #[test]
    fn test_list_for_path() {
        let mut catalog = SnapshotCatalog::new();
        catalog
            .create_snapshot("snap1".to_string(), "/data".to_string(), 1000, 100)
            .unwrap();
        catalog
            .create_snapshot("snap2".to_string(), "/data".to_string(), 2000, 200)
            .unwrap();
        catalog
            .create_snapshot("snap3".to_string(), "/other".to_string(), 3000, 300)
            .unwrap();

        let list = catalog.list_for_path("/data");
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_list_all() {
        let mut catalog = SnapshotCatalog::new();
        catalog
            .create_snapshot("snap1".to_string(), "/data".to_string(), 1000, 100)
            .unwrap();
        catalog
            .create_snapshot("snap2".to_string(), "/data".to_string(), 2000, 200)
            .unwrap();

        let list = catalog.list_all();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_expired_snapshots() {
        let mut catalog = SnapshotCatalog::new();
        let mut snap = Snapshot::new("snap1".to_string(), "/data".to_string(), 1000, 100);
        snap.retention_days = Some(1);

        let now = snap.created_at + 2 * 24 * 60 * 60;

        catalog.snapshots.insert(snap.name.clone(), snap);

        let expired = catalog.expired_snapshots(now);
        assert_eq!(expired.len(), 1);
    }

    #[test]
    fn test_start_restore() {
        let mut catalog = SnapshotCatalog::new();
        catalog
            .create_snapshot("snap1".to_string(), "/data".to_string(), 1000, 100)
            .unwrap();

        let result = catalog.start_restore("snap1", "/target".to_string(), 1000);
        assert!(result.is_ok());
    }

    #[test]
    fn test_start_restore_not_found() {
        let mut catalog = SnapshotCatalog::new();
        let result = catalog.start_restore("unknown", "/target".to_string(), 1000);
        assert!(matches!(result, Err(SnapshotError::NotFound(_))));
    }

    #[test]
    fn test_update_restore_progress() {
        let mut catalog = SnapshotCatalog::new();
        catalog
            .create_snapshot("snap1".to_string(), "/data".to_string(), 1000, 100)
            .unwrap();

        let job = catalog
            .start_restore("snap1", "/target".to_string(), 1000)
            .unwrap();
        let job_id = job.id.clone();
        catalog.update_restore_progress(&job_id, 500).unwrap();

        let updated = catalog.get_restore_job(&job_id).unwrap();
        assert_eq!(updated.restored_bytes, 500);
    }

    #[test]
    fn test_complete_restore() {
        let mut catalog = SnapshotCatalog::new();
        catalog
            .create_snapshot("snap1".to_string(), "/data".to_string(), 1000, 100)
            .unwrap();

        let job = catalog
            .start_restore("snap1", "/target".to_string(), 1000)
            .unwrap();
        let job_id = job.id.clone();
        catalog.complete_restore(&job_id).unwrap();

        let completed = catalog.get_restore_job(&job_id).unwrap();
        assert!(matches!(completed.state, RestoreState::Complete));
    }

    #[test]
    fn test_snapshot_count() {
        let mut catalog = SnapshotCatalog::new();
        assert_eq!(catalog.snapshot_count(), 0);
        catalog
            .create_snapshot("snap1".to_string(), "/data".to_string(), 1000, 100)
            .unwrap();
        catalog
            .create_snapshot("snap2".to_string(), "/data".to_string(), 2000, 200)
            .unwrap();
        assert_eq!(catalog.snapshot_count(), 2);
    }

    #[test]
    fn test_total_snapshot_bytes() {
        let mut catalog = SnapshotCatalog::new();
        catalog
            .create_snapshot("snap1".to_string(), "/data".to_string(), 1000, 100)
            .unwrap();
        catalog
            .create_snapshot("snap2".to_string(), "/data".to_string(), 2000, 200)
            .unwrap();
        assert_eq!(catalog.total_snapshot_bytes(), 3000);
    }
}
