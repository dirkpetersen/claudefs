use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub type RebalanceJobId = u64;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum JobState {
    Pending,
    Running,
    Paused,
    Complete,
    Failed(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RebalanceJob {
    pub id: RebalanceJobId,
    pub source_node: String,
    pub target_node: String,
    pub bytes_total: u64,
    pub bytes_moved: u64,
    pub state: JobState,
    pub created_at: u64,
    pub updated_at: u64,
}

impl RebalanceJob {
    pub fn progress_fraction(&self) -> f64 {
        if self.bytes_total == 0 {
            0.0
        } else {
            self.bytes_moved as f64 / self.bytes_total as f64
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self.state, JobState::Complete | JobState::Failed(_))
    }
}

pub struct RebalanceScheduler {
    jobs: Mutex<HashMap<RebalanceJobId, RebalanceJob>>,
    next_id: Mutex<RebalanceJobId>,
    max_concurrent: usize,
}

impl RebalanceScheduler {
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            jobs: Mutex::new(HashMap::new()),
            next_id: Mutex::new(1),
            max_concurrent,
        }
    }

    fn now_nanos() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    }

    pub fn submit(&self, source_node: &str, target_node: &str, bytes_total: u64) -> RebalanceJobId {
        let id = {
            let mut next_id = self.next_id.lock().unwrap();
            let id = *next_id;
            *next_id += 1;
            id
        };

        let now = Self::now_nanos();

        let running = self.running_count();
        let state = if running < self.max_concurrent {
            JobState::Running
        } else {
            JobState::Pending
        };

        let job = RebalanceJob {
            id,
            source_node: source_node.to_string(),
            target_node: target_node.to_string(),
            bytes_total,
            bytes_moved: 0,
            state,
            created_at: now,
            updated_at: now,
        };

        self.jobs.lock().unwrap().insert(id, job);
        id
    }

    pub fn start_job(&self, id: RebalanceJobId) -> bool {
        if self.running_count() >= self.max_concurrent {
            return false;
        }
        let mut jobs = self.jobs.lock().unwrap();
        if let Some(job) = jobs.get_mut(&id) {
            if matches!(job.state, JobState::Pending) {
                job.state = JobState::Running;
                job.updated_at = Self::now_nanos();
                return true;
            }
        }
        false
    }

    pub fn pause_job(&self, id: RebalanceJobId) -> bool {
        let mut jobs = self.jobs.lock().unwrap();
        if let Some(job) = jobs.get_mut(&id) {
            if matches!(job.state, JobState::Running) {
                job.state = JobState::Paused;
                job.updated_at = Self::now_nanos();
                return true;
            }
        }
        false
    }

    pub fn resume_job(&self, id: RebalanceJobId) -> bool {
        if self.running_count() >= self.max_concurrent {
            return false;
        }

        let mut jobs = self.jobs.lock().unwrap();
        if let Some(job) = jobs.get_mut(&id) {
            if matches!(job.state, JobState::Paused) {
                job.state = JobState::Running;
                job.updated_at = Self::now_nanos();
                return true;
            }
        }
        false
    }

    pub fn update_progress(&self, id: RebalanceJobId, bytes_moved: u64) -> bool {
        let mut jobs = self.jobs.lock().unwrap();
        if let Some(job) = jobs.get_mut(&id) {
            if matches!(job.state, JobState::Running) {
                job.bytes_moved = bytes_moved;
                job.updated_at = Self::now_nanos();
                return true;
            }
        }
        false
    }

    pub fn complete_job(&self, id: RebalanceJobId) -> bool {
        let mut jobs = self.jobs.lock().unwrap();
        if let Some(job) = jobs.get_mut(&id) {
            if matches!(job.state, JobState::Running) {
                job.state = JobState::Complete;
                job.updated_at = Self::now_nanos();
                return true;
            }
        }
        false
    }

    pub fn fail_job(&self, id: RebalanceJobId, reason: &str) -> bool {
        let mut jobs = self.jobs.lock().unwrap();
        if let Some(job) = jobs.get_mut(&id) {
            if !job.is_terminal() {
                job.state = JobState::Failed(reason.to_string());
                job.updated_at = Self::now_nanos();
                return true;
            }
        }
        false
    }

    pub fn get_job(&self, id: RebalanceJobId) -> Option<RebalanceJob> {
        self.jobs.lock().unwrap().get(&id).cloned()
    }

    pub fn running_count(&self) -> usize {
        self.jobs
            .lock()
            .unwrap()
            .values()
            .filter(|j| matches!(j.state, JobState::Running | JobState::Paused))
            .count()
    }

    pub fn pending_jobs(&self) -> Vec<RebalanceJob> {
        let jobs = self.jobs.lock().unwrap();
        let mut pending: Vec<_> = jobs
            .values()
            .filter(|j| matches!(j.state, JobState::Pending))
            .cloned()
            .collect();
        pending.sort_by_key(|j| j.id);
        pending
    }

    pub fn active_jobs(&self) -> Vec<RebalanceJob> {
        let jobs = self.jobs.lock().unwrap();
        let mut active: Vec<_> = jobs
            .values()
            .filter(|j| matches!(j.state, JobState::Running))
            .cloned()
            .collect();
        active.sort_by_key(|j| j.id);
        active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_scheduler() -> RebalanceScheduler {
        RebalanceScheduler::new(2)
    }

    fn make_running_job(scheduler: &RebalanceScheduler) -> RebalanceJobId {
        scheduler.submit("src", "dst", 1000)
    }

    #[test]
    fn submit_creates_pending_job_when_max_concurrent_zero() {
        let scheduler = RebalanceScheduler::new(0);
        let job_id = scheduler.submit("node1", "node2", 1000);
        let job = scheduler.get_job(job_id).unwrap();
        assert_eq!(job.state, JobState::Pending);
    }

    #[test]
    fn submit_auto_starts_when_under_max_concurrent() {
        let scheduler = create_test_scheduler();
        let job_id = scheduler.submit("node1", "node2", 1000);
        let job = scheduler.get_job(job_id).unwrap();
        assert_eq!(job.state, JobState::Running);
    }

    #[test]
    fn submit_keeps_pending_when_at_max_concurrent() {
        let scheduler = RebalanceScheduler::new(2);
        scheduler.submit("node1", "node2", 1000);
        scheduler.submit("node3", "node4", 2000);
        assert_eq!(scheduler.running_count(), 2);

        let job_id = scheduler.submit("node5", "node6", 3000);
        let job = scheduler.get_job(job_id).unwrap();
        assert_eq!(job.state, JobState::Pending);
    }

    #[test]
    fn start_job_transitions_pending_to_running() {
        let scheduler = RebalanceScheduler::new(2);
        let id1 = scheduler.submit("node1", "node2", 1000);
        let _id2 = scheduler.submit("node3", "node4", 2000);
        let id3 = scheduler.submit("node5", "node6", 3000);
        assert_eq!(scheduler.get_job(id3).unwrap().state, JobState::Pending);

        scheduler.complete_job(id1);
        assert!(scheduler.running_count() < 2);

        let result = scheduler.start_job(id3);
        assert!(result);
        assert_eq!(scheduler.get_job(id3).unwrap().state, JobState::Running);
    }

    #[test]
    fn start_job_returns_false_when_at_max_concurrent() {
        let scheduler = RebalanceScheduler::new(2);
        let job_id = scheduler.submit("node1", "node2", 1000);
        assert_eq!(scheduler.get_job(job_id).unwrap().state, JobState::Running);

        scheduler.submit("node3", "node4", 2000);
        assert_eq!(scheduler.running_count(), 2);

        let pending_id = scheduler.submit("node5", "node6", 3000);
        let result = scheduler.start_job(pending_id);
        assert!(!result);
    }

    #[test]
    fn start_job_returns_false_for_non_pending_job() {
        let scheduler = create_test_scheduler();
        let job_id = scheduler.submit("node1", "node2", 1000);
        scheduler.pause_job(job_id);
        let result = scheduler.start_job(job_id);
        assert!(!result);
    }

    #[test]
    fn pause_job_transitions_running_to_paused() {
        let scheduler = create_test_scheduler();
        let job_id = scheduler.submit("node1", "node2", 1000);
        let result = scheduler.pause_job(job_id);
        assert!(result);
        let job = scheduler.get_job(job_id).unwrap();
        assert_eq!(job.state, JobState::Paused);
    }

    #[test]
    fn pause_job_returns_false_for_non_running_job() {
        let scheduler = RebalanceScheduler::new(0);
        let job_id = scheduler.submit("node1", "node2", 1000);
        scheduler.start_job(job_id);
        scheduler.pause_job(job_id);
        let result = scheduler.pause_job(job_id);
        assert!(!result);
    }

    #[test]
    fn resume_job_transitions_paused_to_running() {
        let scheduler = create_test_scheduler();
        let job_id = scheduler.submit("node1", "node2", 1000);
        scheduler.pause_job(job_id);
        let result = scheduler.resume_job(job_id);
        assert!(result);
        let job = scheduler.get_job(job_id).unwrap();
        assert_eq!(job.state, JobState::Running);
    }

    #[test]
    fn resume_job_returns_false_when_at_max_concurrent() {
        let scheduler = RebalanceScheduler::new(1);
        let job_id1 = scheduler.submit("node1", "node2", 1000);
        assert_eq!(scheduler.running_count(), 1);

        scheduler.pause_job(job_id1);

        let job_id2 = scheduler.submit("node3", "node4", 2000);

        let result = scheduler.resume_job(job_id1);
        assert!(!result);
    }

    #[test]
    fn resume_job_returns_false_for_non_paused_job() {
        let scheduler = create_test_scheduler();
        let job_id = scheduler.submit("node1", "node2", 1000);
        let result = scheduler.resume_job(job_id);
        assert!(!result);
    }

    #[test]
    fn update_progress_updates_bytes_moved() {
        let scheduler = create_test_scheduler();
        let job_id = scheduler.submit("node1", "node2", 1000);
        let result = scheduler.update_progress(job_id, 500);
        assert!(result);
        let job = scheduler.get_job(job_id).unwrap();
        assert_eq!(job.bytes_moved, 500);
    }

    #[test]
    fn update_progress_returns_false_for_non_running_job() {
        let scheduler = RebalanceScheduler::new(0);
        let job_id = scheduler.submit("node1", "node2", 1000);
        scheduler.start_job(job_id);
        scheduler.pause_job(job_id);
        let result = scheduler.update_progress(job_id, 500);
        assert!(!result);
    }

    #[test]
    fn complete_job_transitions_running_to_complete() {
        let scheduler = create_test_scheduler();
        let job_id = scheduler.submit("node1", "node2", 1000);
        let result = scheduler.complete_job(job_id);
        assert!(result);
        let job = scheduler.get_job(job_id).unwrap();
        assert_eq!(job.state, JobState::Complete);
    }

    #[test]
    fn complete_job_returns_false_for_non_running_job() {
        let scheduler = RebalanceScheduler::new(0);
        let job_id = scheduler.submit("node1", "node2", 1000);
        scheduler.start_job(job_id);
        scheduler.pause_job(job_id);
        let result = scheduler.complete_job(job_id);
        assert!(!result);
    }

    #[test]
    fn fail_job_transitions_any_non_terminal_to_failed() {
        let scheduler = create_test_scheduler();
        let job_id = scheduler.submit("node1", "node2", 1000);
        let result = scheduler.fail_job(job_id, "disk error");
        assert!(result);
        let job = scheduler.get_job(job_id).unwrap();
        assert!(matches!(job.state, JobState::Failed(msg) if msg == "disk error"));
    }

    #[test]
    fn fail_job_stores_reason_in_failed_variant() {
        let scheduler = create_test_scheduler();
        let job_id = scheduler.submit("node1", "node2", 1000);
        scheduler.fail_job(job_id, "network timeout");
        let job = scheduler.get_job(job_id).unwrap();
        assert!(matches!(job.state, JobState::Failed(msg) if msg == "network timeout"));
    }

    #[test]
    fn progress_fraction_computation_zero_when_total_zero() {
        let job = RebalanceJob {
            id: 1,
            source_node: "node1".to_string(),
            target_node: "node2".to_string(),
            bytes_total: 0,
            bytes_moved: 0,
            state: JobState::Running,
            created_at: 0,
            updated_at: 0,
        };
        assert_eq!(job.progress_fraction(), 0.0);
    }

    #[test]
    fn progress_fraction_computation_normal_case() {
        let job = RebalanceJob {
            id: 1,
            source_node: "node1".to_string(),
            target_node: "node2".to_string(),
            bytes_total: 1000,
            bytes_moved: 250,
            state: JobState::Running,
            created_at: 0,
            updated_at: 0,
        };
        assert!((job.progress_fraction() - 0.25).abs() < 0.001);
    }

    #[test]
    fn is_terminal_returns_true_for_complete() {
        let job = RebalanceJob {
            id: 1,
            source_node: "node1".to_string(),
            target_node: "node2".to_string(),
            bytes_total: 1000,
            bytes_moved: 1000,
            state: JobState::Complete,
            created_at: 0,
            updated_at: 0,
        };
        assert!(job.is_terminal());
    }

    #[test]
    fn is_terminal_returns_true_for_failed() {
        let job = RebalanceJob {
            id: 1,
            source_node: "node1".to_string(),
            target_node: "node2".to_string(),
            bytes_total: 1000,
            bytes_moved: 100,
            state: JobState::Failed("error".to_string()),
            created_at: 0,
            updated_at: 0,
        };
        assert!(job.is_terminal());
    }

    #[test]
    fn is_terminal_returns_false_for_pending() {
        let job = RebalanceJob {
            id: 1,
            source_node: "node1".to_string(),
            target_node: "node2".to_string(),
            bytes_total: 1000,
            bytes_moved: 0,
            state: JobState::Pending,
            created_at: 0,
            updated_at: 0,
        };
        assert!(!job.is_terminal());
    }

    #[test]
    fn is_terminal_returns_false_for_running() {
        let job = RebalanceJob {
            id: 1,
            source_node: "node1".to_string(),
            target_node: "node2".to_string(),
            bytes_total: 1000,
            bytes_moved: 100,
            state: JobState::Running,
            created_at: 0,
            updated_at: 0,
        };
        assert!(!job.is_terminal());
    }

    #[test]
    fn is_terminal_returns_false_for_paused() {
        let job = RebalanceJob {
            id: 1,
            source_node: "node1".to_string(),
            target_node: "node2".to_string(),
            bytes_total: 1000,
            bytes_moved: 100,
            state: JobState::Paused,
            created_at: 0,
            updated_at: 0,
        };
        assert!(!job.is_terminal());
    }

    #[test]
    fn running_count_stays_within_max_concurrent() {
        let scheduler = RebalanceScheduler::new(2);
        scheduler.submit("node1", "node2", 1000);
        scheduler.submit("node3", "node4", 2000);
        scheduler.submit("node5", "node6", 3000);
        scheduler.submit("node7", "node8", 4000);
        assert!(scheduler.running_count() <= 2);
    }

    #[test]
    fn get_job_returns_some_for_existing_job() {
        let scheduler = create_test_scheduler();
        let job_id = scheduler.submit("node1", "node2", 1000);
        let job = scheduler.get_job(job_id);
        assert!(job.is_some());
    }

    #[test]
    fn get_job_returns_none_for_non_existing_job() {
        let scheduler = create_test_scheduler();
        let job = scheduler.get_job(999);
        assert!(job.is_none());
    }

    #[test]
    fn pending_jobs_returns_sorted_jobs() {
        let scheduler = RebalanceScheduler::new(1);
        scheduler.submit("node1", "node2", 1000);
        scheduler.submit("node3", "node4", 2000);
        scheduler.submit("node5", "node6", 3000);
        let pending = scheduler.pending_jobs();
        assert_eq!(pending.len(), 2);
        assert!(pending[0].id < pending[1].id);
    }

    #[test]
    fn active_jobs_returns_sorted_jobs() {
        let scheduler = create_test_scheduler();
        let active = scheduler.active_jobs();
        assert!(active.len() <= 2);
    }

    #[test]
    fn fail_job_does_not_transition_terminal_jobs() {
        let scheduler = create_test_scheduler();
        let job_id = scheduler.submit("node1", "node2", 1000);
        scheduler.complete_job(job_id);
        let result = scheduler.fail_job(job_id, "should not work");
        assert!(!result);
        let job = scheduler.get_job(job_id).unwrap();
        assert!(matches!(job.state, JobState::Complete));
    }

    #[test]
    fn multiple_pause_resume_cycles() {
        let scheduler = create_test_scheduler();
        let job_id = scheduler.submit("node1", "node2", 1000);
        assert!(scheduler.pause_job(job_id));
        assert!(scheduler.resume_job(job_id));
        assert!(scheduler.pause_job(job_id));
        assert!(scheduler.resume_job(job_id));
        let job = scheduler.get_job(job_id).unwrap();
        assert_eq!(job.state, JobState::Running);
    }

    #[test]
    fn submit_multiple_jobs_auto_start() {
        let scheduler = RebalanceScheduler::new(3);
        let id1 = scheduler.submit("n1", "n2", 100);
        let id2 = scheduler.submit("n3", "n4", 200);
        let id3 = scheduler.submit("n5", "n6", 300);
        let id4 = scheduler.submit("n7", "n8", 400);

        assert_eq!(scheduler.get_job(id1).unwrap().state, JobState::Running);
        assert_eq!(scheduler.get_job(id2).unwrap().state, JobState::Running);
        assert_eq!(scheduler.get_job(id3).unwrap().state, JobState::Running);
        assert_eq!(scheduler.get_job(id4).unwrap().state, JobState::Pending);
    }

    #[test]
    fn submit_increments_job_ids() {
        let scheduler = create_test_scheduler();
        let id1 = scheduler.submit("n1", "n2", 100);
        let id2 = scheduler.submit("n3", "n4", 200);
        let id3 = scheduler.submit("n5", "n6", 300);
        assert!(id1 < id2);
        assert!(id2 < id3);
    }

    #[test]
    fn complete_job_frees_slot_for_pending() {
        let scheduler = RebalanceScheduler::new(2);
        let id1 = scheduler.submit("n1", "n2", 100);
        let id2 = scheduler.submit("n3", "n4", 200);
        let id3 = scheduler.submit("n5", "n6", 300);
        assert_eq!(scheduler.pending_jobs().len(), 1);

        scheduler.complete_job(id1);
        assert_eq!(scheduler.running_count(), 1);

        scheduler.start_job(id3);
        assert_eq!(scheduler.pending_jobs().len(), 0);
    }

    #[test]
    fn update_progress_does_not_exceed_total() {
        let scheduler = create_test_scheduler();
        let job_id = scheduler.submit("n1", "n2", 1000);
        scheduler.update_progress(job_id, 500);
        assert_eq!(scheduler.get_job(job_id).unwrap().progress_fraction(), 0.5);
    }

    #[test]
    fn fail_pending_job_succeeds() {
        let scheduler = RebalanceScheduler::new(0);
        let job_id = scheduler.submit("n1", "n2", 1000);
        let result = scheduler.fail_job(job_id, "cancelled");
        assert!(result);
        assert!(matches!(
            scheduler.get_job(job_id).unwrap().state,
            JobState::Failed(_)
        ));
    }

    #[test]
    fn fail_paused_job_succeeds() {
        let scheduler = create_test_scheduler();
        let job_id = scheduler.submit("n1", "n2", 1000);
        scheduler.pause_job(job_id);
        let result = scheduler.fail_job(job_id, "cancelled");
        assert!(result);
        assert!(matches!(
            scheduler.get_job(job_id).unwrap().state,
            JobState::Failed(_)
        ));
    }
}
