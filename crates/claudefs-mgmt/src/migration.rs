use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error("Migration not found: {0}")]
    NotFound(String),
    #[error("Migration already exists: {0}")]
    AlreadyExists(String),
    #[error("Cannot transition from {from:?} to {to:?}")]
    InvalidTransition {
        from: MigrationState,
        to: MigrationState,
    },
    #[error("Migration source not accessible: {0}")]
    SourceError(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MigrationSource {
    Nfs { server: String, export: String },
    Local { path: String },
    ClaudeFs { endpoint: String, path: String },
    S3 { bucket: String, prefix: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MigrationState {
    Pending,
    Scanning,
    Copying,
    Verifying,
    Complete,
    Failed(String),
    Cancelled,
}

impl MigrationState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            MigrationState::Complete | MigrationState::Failed(_) | MigrationState::Cancelled
        )
    }

    pub fn can_transition_to(&self, next: &MigrationState) -> bool {
        match (self, next) {
            (MigrationState::Pending, MigrationState::Scanning) => true,
            (MigrationState::Pending, MigrationState::Copying) => true,
            (MigrationState::Pending, MigrationState::Failed(_)) => true,
            (MigrationState::Pending, MigrationState::Cancelled) => true,

            (MigrationState::Scanning, MigrationState::Copying) => true,
            (MigrationState::Scanning, MigrationState::Failed(_)) => true,
            (MigrationState::Scanning, MigrationState::Cancelled) => true,

            (MigrationState::Copying, MigrationState::Verifying) => true,
            (MigrationState::Copying, MigrationState::Failed(_)) => true,
            (MigrationState::Copying, MigrationState::Cancelled) => true,

            (MigrationState::Verifying, MigrationState::Complete) => true,
            (MigrationState::Verifying, MigrationState::Failed(_)) => true,
            (MigrationState::Verifying, MigrationState::Cancelled) => true,

            (MigrationState::Complete, _) => false,
            (MigrationState::Failed(_), _) => false,
            (MigrationState::Cancelled, _) => false,

            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationJob {
    pub id: String,
    pub name: String,
    pub source: MigrationSource,
    pub destination_path: String,
    pub state: MigrationState,
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub completed_at: Option<u64>,
    pub total_files: u64,
    pub copied_files: u64,
    pub total_bytes: u64,
    pub copied_bytes: u64,
    pub failed_files: u64,
    pub errors: Vec<String>,
    pub dry_run: bool,
}

impl MigrationJob {
    pub fn new(
        id: String,
        name: String,
        source: MigrationSource,
        destination_path: String,
    ) -> Self {
        Self {
            id,
            name,
            source,
            destination_path,
            state: MigrationState::Pending,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            started_at: None,
            completed_at: None,
            total_files: 0,
            copied_files: 0,
            total_bytes: 0,
            copied_bytes: 0,
            failed_files: 0,
            errors: vec![],
            dry_run: false,
        }
    }

    pub fn percent_complete(&self) -> f64 {
        if self.total_files == 0 {
            0.0
        } else {
            (self.copied_files as f64 / self.total_files as f64) * 100.0
        }
    }

    pub fn is_complete(&self) -> bool {
        matches!(self.state, MigrationState::Complete)
    }

    pub fn duration_secs(&self) -> Option<u64> {
        match (self.started_at, self.completed_at) {
            (Some(start), Some(end)) => Some(end - start),
            (Some(_), None) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                Some(now - self.started_at?)
            }
            _ => None,
        }
    }

    pub fn throughput_bps(&self) -> f64 {
        if let Some(dur) = self.duration_secs() {
            if dur > 0 {
                return self.copied_bytes as f64 / dur as f64;
            }
        }
        0.0
    }

    pub fn add_error(&mut self, error: String) {
        self.errors.push(error);
    }

    pub fn transition_to(&mut self, state: MigrationState) -> Result<(), MigrationError> {
        if !self.state.can_transition_to(&state) {
            return Err(MigrationError::InvalidTransition {
                from: self.state.clone(),
                to: state.clone(),
            });
        }

        self.state = state.clone();

        if self.started_at.is_none()
            && matches!(state, MigrationState::Scanning | MigrationState::Copying)
        {
            self.started_at = Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            );
        }

        if state.is_terminal() {
            self.completed_at = Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            );
        }

        Ok(())
    }
}

pub struct MigrationRegistry {
    jobs: HashMap<String, MigrationJob>,
}

impl MigrationRegistry {
    pub fn new() -> Self {
        Self {
            jobs: HashMap::new(),
        }
    }

    pub fn create_job(
        &mut self,
        id: String,
        name: String,
        source: MigrationSource,
        destination_path: String,
    ) -> Result<&MigrationJob, MigrationError> {
        if self.jobs.contains_key(&id) {
            return Err(MigrationError::AlreadyExists(id));
        }

        let job = MigrationJob::new(id.clone(), name, source, destination_path);
        self.jobs.insert(id.clone(), job);
        self.jobs.get(&id).ok_or(MigrationError::NotFound(id))
    }

    pub fn get_job(&self, id: &str) -> Option<&MigrationJob> {
        self.jobs.get(id)
    }

    pub fn get_job_mut(&mut self, id: &str) -> Option<&mut MigrationJob> {
        self.jobs.get_mut(id)
    }

    pub fn transition_job(
        &mut self,
        id: &str,
        state: MigrationState,
    ) -> Result<(), MigrationError> {
        let job = self
            .jobs
            .get_mut(id)
            .ok_or_else(|| MigrationError::NotFound(id.to_string()))?;
        job.transition_to(state)
    }

    pub fn update_progress(
        &mut self,
        id: &str,
        copied_files: u64,
        copied_bytes: u64,
    ) -> Result<(), MigrationError> {
        let job = self
            .jobs
            .get_mut(id)
            .ok_or_else(|| MigrationError::NotFound(id.to_string()))?;
        job.copied_files = copied_files;
        job.copied_bytes = copied_bytes;
        Ok(())
    }

    pub fn cancel_job(&mut self, id: &str) -> Result<(), MigrationError> {
        let job = self
            .jobs
            .get_mut(id)
            .ok_or_else(|| MigrationError::NotFound(id.to_string()))?;
        job.transition_to(MigrationState::Cancelled)
    }

    pub fn list_active(&self) -> Vec<&MigrationJob> {
        self.jobs
            .values()
            .filter(|j| !j.state.is_terminal())
            .collect()
    }

    pub fn list_complete(&self) -> Vec<&MigrationJob> {
        self.jobs
            .values()
            .filter(|j| matches!(j.state, MigrationState::Complete))
            .collect()
    }

    pub fn list_all(&self) -> Vec<&MigrationJob> {
        self.jobs.values().collect()
    }

    pub fn job_count(&self) -> usize {
        self.jobs.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migration_state_is_terminal_complete() {
        assert!(MigrationState::Complete.is_terminal());
    }

    #[test]
    fn test_migration_state_is_terminal_failed() {
        assert!(MigrationState::Failed("error".to_string()).is_terminal());
    }

    #[test]
    fn test_migration_state_is_terminal_cancelled() {
        assert!(MigrationState::Cancelled.is_terminal());
    }

    #[test]
    fn test_migration_state_is_terminal_false_pending() {
        assert!(!MigrationState::Pending.is_terminal());
    }

    #[test]
    fn test_migration_state_is_terminal_false_copying() {
        assert!(!MigrationState::Copying.is_terminal());
    }

    #[test]
    fn test_migration_state_can_transition_pending_to_scanning() {
        assert!(MigrationState::Pending.can_transition_to(&MigrationState::Scanning));
    }

    #[test]
    fn test_migration_state_can_transition_pending_to_complete() {
        assert!(!MigrationState::Pending.can_transition_to(&MigrationState::Complete));
    }

    #[test]
    fn test_migration_state_can_transition_copying_to_verifying() {
        assert!(MigrationState::Copying.can_transition_to(&MigrationState::Verifying));
    }

    #[test]
    fn test_migration_state_can_transition_complete_to_anything() {
        assert!(!MigrationState::Complete.can_transition_to(&MigrationState::Pending));
        assert!(!MigrationState::Complete.can_transition_to(&MigrationState::Scanning));
    }

    #[test]
    fn test_migration_job_new() {
        let source = MigrationSource::Local {
            path: "/data".to_string(),
        };
        let job = MigrationJob::new(
            "job1".to_string(),
            "Test migration".to_string(),
            source,
            "/dest".to_string(),
        );
        assert_eq!(job.id, "job1");
        assert!(matches!(job.state, MigrationState::Pending));
    }

    #[test]
    fn test_migration_job_percent_complete_zero() {
        let source = MigrationSource::Local {
            path: "/data".to_string(),
        };
        let job = MigrationJob::new(
            "job1".to_string(),
            "Test".to_string(),
            source,
            "/dest".to_string(),
        );
        assert_eq!(job.percent_complete(), 0.0);
    }

    #[test]
    fn test_migration_job_percent_complete_full() {
        let mut source = MigrationSource::Local {
            path: "/data".to_string(),
        };
        let mut job = MigrationJob::new(
            "job1".to_string(),
            "Test".to_string(),
            source,
            "/dest".to_string(),
        );
        job.total_files = 100;
        job.copied_files = 100;
        assert_eq!(job.percent_complete(), 100.0);
    }

    #[test]
    fn test_migration_job_percent_complete_half() {
        let source = MigrationSource::Local {
            path: "/data".to_string(),
        };
        let mut job = MigrationJob::new(
            "job1".to_string(),
            "Test".to_string(),
            source,
            "/dest".to_string(),
        );
        job.total_files = 100;
        job.copied_files = 50;
        assert_eq!(job.percent_complete(), 50.0);
    }

    #[test]
    fn test_migration_job_is_complete_false_pending() {
        let source = MigrationSource::Local {
            path: "/data".to_string(),
        };
        let job = MigrationJob::new(
            "job1".to_string(),
            "Test".to_string(),
            source,
            "/dest".to_string(),
        );
        assert!(!job.is_complete());
    }

    #[test]
    fn test_migration_job_is_complete_true() {
        let source = MigrationSource::Local {
            path: "/data".to_string(),
        };
        let job = MigrationJob {
            state: MigrationState::Complete,
            ..MigrationJob::new(
                "job1".to_string(),
                "Test".to_string(),
                source,
                "/dest".to_string(),
            )
        };
        assert!(job.is_complete());
    }

    #[test]
    fn test_migration_job_duration_secs_none_when_not_started() {
        let source = MigrationSource::Local {
            path: "/data".to_string(),
        };
        let job = MigrationJob::new(
            "job1".to_string(),
            "Test".to_string(),
            source,
            "/dest".to_string(),
        );
        assert_eq!(job.duration_secs(), None);
    }

    #[test]
    fn test_migration_job_add_error() {
        let source = MigrationSource::Local {
            path: "/data".to_string(),
        };
        let mut job = MigrationJob::new(
            "job1".to_string(),
            "Test".to_string(),
            source,
            "/dest".to_string(),
        );
        job.add_error("Test error".to_string());
        assert_eq!(job.errors.len(), 1);
    }

    #[test]
    fn test_migration_job_transition_pending_to_scanning() {
        let source = MigrationSource::Local {
            path: "/data".to_string(),
        };
        let mut job = MigrationJob::new(
            "job1".to_string(),
            "Test".to_string(),
            source,
            "/dest".to_string(),
        );
        let result = job.transition_to(MigrationState::Scanning);
        assert!(result.is_ok());
    }

    #[test]
    fn test_migration_job_transition_invalid() {
        let source = MigrationSource::Local {
            path: "/data".to_string(),
        };
        let mut job = MigrationJob::new(
            "job1".to_string(),
            "Test".to_string(),
            source,
            "/dest".to_string(),
        );
        let result = job.transition_to(MigrationState::Complete);
        assert!(result.is_err());
    }

    #[test]
    fn test_migration_registry_create_job() {
        let mut registry = MigrationRegistry::new();
        let source = MigrationSource::Local {
            path: "/data".to_string(),
        };
        let result = registry.create_job(
            "job1".to_string(),
            "Test".to_string(),
            source,
            "/dest".to_string(),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_migration_registry_create_job_already_exists() {
        let mut registry = MigrationRegistry::new();
        let source = MigrationSource::Local {
            path: "/data".to_string(),
        };
        registry
            .create_job(
                "job1".to_string(),
                "Test".to_string(),
                source.clone(),
                "/dest".to_string(),
            )
            .unwrap();

        let result = registry.create_job(
            "job1".to_string(),
            "Test2".to_string(),
            source,
            "/dest".to_string(),
        );
        assert!(matches!(result, Err(MigrationError::AlreadyExists(_))));
    }

    #[test]
    fn test_migration_registry_get_job_unknown() {
        let registry = MigrationRegistry::new();
        let result = registry.get_job("unknown");
        assert!(result.is_none());
    }

    #[test]
    fn test_migration_registry_transition_job() {
        let mut registry = MigrationRegistry::new();
        let source = MigrationSource::Local {
            path: "/data".to_string(),
        };
        registry
            .create_job(
                "job1".to_string(),
                "Test".to_string(),
                source,
                "/dest".to_string(),
            )
            .unwrap();

        let result = registry.transition_job("job1", MigrationState::Scanning);
        assert!(result.is_ok());
    }

    #[test]
    fn test_migration_registry_update_progress() {
        let mut registry = MigrationRegistry::new();
        let source = MigrationSource::Local {
            path: "/data".to_string(),
        };
        registry
            .create_job(
                "job1".to_string(),
                "Test".to_string(),
                source,
                "/dest".to_string(),
            )
            .unwrap();

        let result = registry.update_progress("job1", 50, 5000);
        assert!(result.is_ok());

        let job = registry.get_job("job1").unwrap();
        assert_eq!(job.copied_files, 50);
        assert_eq!(job.copied_bytes, 5000);
    }

    #[test]
    fn test_migration_registry_cancel_job() {
        let mut registry = MigrationRegistry::new();
        let source = MigrationSource::Local {
            path: "/data".to_string(),
        };
        registry
            .create_job(
                "job1".to_string(),
                "Test".to_string(),
                source,
                "/dest".to_string(),
            )
            .unwrap();

        let result = registry.cancel_job("job1");
        assert!(result.is_ok());

        let job = registry.get_job("job1").unwrap();
        assert!(matches!(job.state, MigrationState::Cancelled));
    }

    #[test]
    fn test_migration_registry_list_active() {
        let mut registry = MigrationRegistry::new();
        let source = MigrationSource::Local {
            path: "/data".to_string(),
        };
        registry
            .create_job(
                "job1".to_string(),
                "Test".to_string(),
                source.clone(),
                "/dest".to_string(),
            )
            .unwrap();
        registry
            .transition_job("job1", MigrationState::Copying)
            .unwrap();

        registry
            .create_job(
                "job2".to_string(),
                "Test2".to_string(),
                source.clone(),
                "/dest2".to_string(),
            )
            .unwrap();
        let job2 = registry.get_job_mut("job2").unwrap();
        job2.state = MigrationState::Complete;

        let active = registry.list_active();
        assert_eq!(active.len(), 1);
    }

    #[test]
    fn test_migration_registry_list_complete() {
        let mut registry = MigrationRegistry::new();
        let source = MigrationSource::Local {
            path: "/data".to_string(),
        };
        registry
            .create_job(
                "job1".to_string(),
                "Test".to_string(),
                source.clone(),
                "/dest".to_string(),
            )
            .unwrap();
        registry
            .transition_job("job1", MigrationState::Scanning)
            .unwrap();
        registry
            .transition_job("job1", MigrationState::Copying)
            .unwrap();
        registry
            .transition_job("job1", MigrationState::Verifying)
            .unwrap();
        registry
            .transition_job("job1", MigrationState::Complete)
            .unwrap();

        let complete = registry.list_complete();
        assert_eq!(complete.len(), 1);
    }

    #[test]
    fn test_migration_registry_job_count() {
        let mut registry = MigrationRegistry::new();
        let source = MigrationSource::Local {
            path: "/data".to_string(),
        };
        registry
            .create_job(
                "job1".to_string(),
                "Test".to_string(),
                source.clone(),
                "/dest".to_string(),
            )
            .unwrap();
        registry
            .create_job(
                "job2".to_string(),
                "Test2".to_string(),
                source.clone(),
                "/dest2".to_string(),
            )
            .unwrap();

        assert_eq!(registry.job_count(), 2);
    }
}
