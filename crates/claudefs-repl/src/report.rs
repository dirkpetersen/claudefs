// File: crates/claudefs-repl/src/report.rs

//! Conflict and Replication Reports.
//!
//! Generates human-readable reports about replication status for admin consumption.

use crate::checkpoint::ReplicationCheckpoint;
use crate::health::LinkHealthReport;
use crate::sync::Conflict;

/// Summary of replication conflicts for admin reporting.
#[derive(Debug, Clone)]
pub struct ConflictReport {
    /// Site ID.
    pub site_id: u64,
    /// Report generation time (microseconds).
    pub report_time_us: u64,
    /// Number of conflicts.
    pub conflict_count: usize,
    /// The conflicts.
    pub conflicts: Vec<Conflict>,
    /// Unique inodes with conflicts (sorted, deduplicated).
    pub affected_inodes: Vec<u64>,
    /// How many were auto-resolved by LWW.
    pub lww_resolution_count: usize,
}

impl ConflictReport {
    /// Generate a conflict report from a list of conflicts.
    pub fn generate(site_id: u64, conflicts: Vec<Conflict>, report_time_us: u64) -> Self {
        let conflict_count = conflicts.len();
        let mut inodes: Vec<u64> = conflicts.iter().map(|c| c.inode).collect();
        inodes.sort();
        inodes.dedup();

        Self {
            site_id,
            report_time_us,
            conflict_count,
            conflicts,
            affected_inodes: inodes,
            lww_resolution_count: conflict_count,
        }
    }

    /// Returns true if there are any conflicts requiring admin attention.
    pub fn requires_attention(&self) -> bool {
        self.conflict_count > 0
    }

    /// Format as a human-readable summary string.
    pub fn summary(&self) -> String {
        if self.conflict_count == 0 {
            return format!("Site {}: No conflicts", self.site_id);
        }

        format!(
            "Site {}: {} conflicts, {} inodes affected, all resolved by LWW",
            self.site_id,
            self.conflict_count,
            self.affected_inodes.len()
        )
    }
}

/// Full replication status report.
#[derive(Debug, Clone)]
pub struct ReplicationStatusReport {
    /// Report generation time (microseconds).
    pub generated_at_us: u64,
    /// Local site ID.
    pub local_site_id: u64,
    /// Engine state string.
    pub engine_state: String,
    /// Per-link health reports.
    pub link_health: Vec<LinkHealthReport>,
    /// Cluster health as string.
    pub cluster_health: String,
    /// Latest checkpoint (if any).
    pub latest_checkpoint: Option<ReplicationCheckpoint>,
    /// Total conflict count.
    pub conflict_count: usize,
    /// Total entries sent.
    pub total_entries_sent: u64,
    /// Total entries received.
    pub total_entries_received: u64,
}

impl ReplicationStatusReport {
    /// Create a new status report.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        local_site_id: u64,
        generated_at_us: u64,
        engine_state: String,
        link_health: Vec<LinkHealthReport>,
        cluster_health: String,
        latest_checkpoint: Option<ReplicationCheckpoint>,
        conflict_count: usize,
        total_entries_sent: u64,
        total_entries_received: u64,
    ) -> Self {
        Self {
            generated_at_us,
            local_site_id,
            engine_state,
            link_health,
            cluster_health,
            latest_checkpoint,
            conflict_count,
            total_entries_sent,
            total_entries_received,
        }
    }

    /// Format as a one-line summary.
    pub fn one_line_summary(&self) -> String {
        format!(
            "Site {} [{}]: {} | {} links | {} sent | {} recv | {} conflicts",
            self.local_site_id,
            self.engine_state,
            self.cluster_health,
            self.link_health.len(),
            self.total_entries_sent,
            self.total_entries_received,
            self.conflict_count
        )
    }

    /// Returns true if the system is in a degraded state requiring attention.
    pub fn is_degraded(&self) -> bool {
        self.cluster_health != "Healthy"
    }
}

/// Generates replication reports from engine components.
pub struct ReportGenerator {
    /// Site ID.
    site_id: u64,
}

impl ReportGenerator {
    /// Create a new report generator.
    pub fn new(site_id: u64) -> Self {
        Self { site_id }
    }

    /// Generate a conflict report from a conflict list.
    pub fn conflict_report(&self, conflicts: Vec<Conflict>, report_time_us: u64) -> ConflictReport {
        ConflictReport::generate(self.site_id, conflicts, report_time_us)
    }

    /// Generate a basic status report (without engine integration).
    #[allow(clippy::too_many_arguments)]
    pub fn status_report(
        &self,
        generated_at_us: u64,
        engine_state: &str,
        link_health: Vec<LinkHealthReport>,
        cluster_health: &str,
        latest_checkpoint: Option<ReplicationCheckpoint>,
        conflict_count: usize,
        total_sent: u64,
        total_received: u64,
    ) -> ReplicationStatusReport {
        ReplicationStatusReport::new(
            self.site_id,
            generated_at_us,
            engine_state.to_string(),
            link_health,
            cluster_health.to_string(),
            latest_checkpoint,
            conflict_count,
            total_sent,
            total_received,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoint::ReplicationCheckpoint;
    use crate::sync::Conflict;
    use crate::wal::ReplicationCursor;

    #[test]
    fn test_conflict_report_generation_0_conflicts() {
        let report = ConflictReport::generate(1, vec![], 1000000);

        assert_eq!(report.site_id, 1);
        assert_eq!(report.conflict_count, 0);
        assert!(report.conflicts.is_empty());
        assert!(report.affected_inodes.is_empty());
        assert_eq!(report.lww_resolution_count, 0);
    }

    #[test]
    fn test_conflict_report_generation_multiple_conflicts() {
        let conflicts = vec![
            Conflict {
                inode: 100,
                local_site_id: 1,
                remote_site_id: 2,
                local_ts: 1000,
                remote_ts: 2000,
                winner_site_id: 2,
                detected_at_us: 3000000,
            },
            Conflict {
                inode: 200,
                local_site_id: 1,
                remote_site_id: 2,
                local_ts: 1000,
                remote_ts: 2000,
                winner_site_id: 2,
                detected_at_us: 3000000,
            },
            Conflict {
                inode: 100,
                local_site_id: 1,
                remote_site_id: 3,
                local_ts: 1500,
                remote_ts: 2500,
                winner_site_id: 3,
                detected_at_us: 4000000,
            },
        ];

        let report = ConflictReport::generate(2, conflicts, 5000000);

        assert_eq!(report.site_id, 2);
        assert_eq!(report.conflict_count, 3);
        assert_eq!(report.conflicts.len(), 3);
    }

    #[test]
    fn test_affected_inodes_sorted_deduplicated() {
        let conflicts = vec![
            Conflict {
                inode: 300,
                local_site_id: 1,
                remote_site_id: 2,
                local_ts: 1000,
                remote_ts: 2000,
                winner_site_id: 2,
                detected_at_us: 3000000,
            },
            Conflict {
                inode: 100,
                local_site_id: 1,
                remote_site_id: 2,
                local_ts: 1000,
                remote_ts: 2000,
                winner_site_id: 2,
                detected_at_us: 3000000,
            },
            Conflict {
                inode: 200,
                local_site_id: 1,
                remote_site_id: 2,
                local_ts: 1000,
                remote_ts: 2000,
                winner_site_id: 2,
                detected_at_us: 3000000,
            },
        ];

        let report = ConflictReport::generate(1, conflicts, 5000000);

        assert_eq!(report.affected_inodes, vec![100, 200, 300]);
    }

    #[test]
    fn test_requires_attention_true_when_conflicts_exist() {
        let conflicts = vec![Conflict {
            inode: 100,
            local_site_id: 1,
            remote_site_id: 2,
            local_ts: 1000,
            remote_ts: 2000,
            winner_site_id: 2,
            detected_at_us: 3000000,
        }];

        let report = ConflictReport::generate(1, conflicts, 5000000);

        assert!(report.requires_attention());
    }

    #[test]
    fn test_requires_attention_false_when_no_conflicts() {
        let report = ConflictReport::generate(1, vec![], 5000000);

        assert!(!report.requires_attention());
    }

    #[test]
    fn test_summary_returns_non_empty_string() {
        let conflicts = vec![Conflict {
            inode: 100,
            local_site_id: 1,
            remote_site_id: 2,
            local_ts: 1000,
            remote_ts: 2000,
            winner_site_id: 2,
            detected_at_us: 3000000,
        }];

        let report = ConflictReport::generate(1, conflicts, 5000000);

        let summary = report.summary();
        assert!(!summary.is_empty());
        assert!(summary.contains("conflicts"));
    }

    #[test]
    fn test_summary_no_conflicts() {
        let report = ConflictReport::generate(1, vec![], 5000000);

        let summary = report.summary();
        assert!(!summary.is_empty());
        assert!(summary.contains("No conflicts"));
    }

    #[test]
    fn test_replication_status_report_creation() {
        let report = ReplicationStatusReport::new(
            1,
            1000000,
            "Running".to_string(),
            vec![],
            "Healthy".to_string(),
            None,
            0,
            100,
            200,
        );

        assert_eq!(report.local_site_id, 1);
        assert_eq!(report.generated_at_us, 1000000);
        assert_eq!(report.engine_state, "Running");
        assert_eq!(report.cluster_health, "Healthy");
        assert_eq!(report.total_entries_sent, 100);
        assert_eq!(report.total_entries_received, 200);
    }

    #[test]
    fn test_one_line_summary_returns_non_empty_string() {
        let report = ReplicationStatusReport::new(
            1,
            1000000,
            "Running".to_string(),
            vec![],
            "Healthy".to_string(),
            None,
            0,
            100,
            200,
        );

        let summary = report.one_line_summary();
        assert!(!summary.is_empty());
        assert!(summary.contains("Site 1"));
        assert!(summary.contains("Running"));
        assert!(summary.contains("Healthy"));
    }

    #[test]
    fn test_is_degraded_when_cluster_health_degraded() {
        let report = ReplicationStatusReport::new(
            1,
            1000000,
            "Running".to_string(),
            vec![],
            "Degraded".to_string(),
            None,
            0,
            100,
            200,
        );

        assert!(report.is_degraded());
    }

    #[test]
    fn test_is_degraded_when_cluster_health_critical() {
        let report = ReplicationStatusReport::new(
            1,
            1000000,
            "Running".to_string(),
            vec![],
            "Critical".to_string(),
            None,
            0,
            100,
            200,
        );

        assert!(report.is_degraded());
    }

    #[test]
    fn test_is_not_degraded_when_healthy() {
        let report = ReplicationStatusReport::new(
            1,
            1000000,
            "Running".to_string(),
            vec![],
            "Healthy".to_string(),
            None,
            0,
            100,
            200,
        );

        assert!(!report.is_degraded());
    }

    #[test]
    fn test_report_generator_conflict_report() {
        let generator = ReportGenerator::new(1);

        let conflicts = vec![Conflict {
            inode: 100,
            local_site_id: 1,
            remote_site_id: 2,
            local_ts: 1000,
            remote_ts: 2000,
            winner_site_id: 2,
            detected_at_us: 3000000,
        }];

        let report = generator.conflict_report(conflicts, 5000000);

        assert_eq!(report.site_id, 1);
        assert_eq!(report.conflict_count, 1);
    }

    #[test]
    fn test_report_generator_status_report() {
        let generator = ReportGenerator::new(1);

        let cursors = vec![ReplicationCursor::new(2, 0, 100)];
        let checkpoint = ReplicationCheckpoint::new(1, 1, 1000000, cursors);

        let report = generator.status_report(
            1000000,
            "Running",
            vec![],
            "Healthy",
            Some(checkpoint),
            0,
            100,
            200,
        );

        assert_eq!(report.local_site_id, 1);
        assert_eq!(report.engine_state, "Running");
        assert!(report.latest_checkpoint.is_some());
    }

    #[test]
    fn test_conflict_report_lww_resolution_count() {
        let conflicts = vec![
            Conflict {
                inode: 100,
                local_site_id: 1,
                remote_site_id: 2,
                local_ts: 1000,
                remote_ts: 2000,
                winner_site_id: 2,
                detected_at_us: 3000000,
            },
            Conflict {
                inode: 200,
                local_site_id: 1,
                remote_site_id: 2,
                local_ts: 1000,
                remote_ts: 2000,
                winner_site_id: 2,
                detected_at_us: 3000000,
            },
        ];

        let report = ConflictReport::generate(1, conflicts, 5000000);

        assert_eq!(report.lww_resolution_count, 2);
    }

    #[test]
    fn test_replication_status_report_with_checkpoint() {
        let cursors = vec![
            ReplicationCursor::new(2, 0, 100),
            ReplicationCursor::new(2, 1, 200),
        ];
        let checkpoint = ReplicationCheckpoint::new(1, 1, 1000000, cursors);

        let report = ReplicationStatusReport::new(
            1,
            1000000,
            "Running".to_string(),
            vec![],
            "Healthy".to_string(),
            Some(checkpoint),
            0,
            100,
            200,
        );

        let cp = report.latest_checkpoint.unwrap();
        assert_eq!(cp.cursor_count, 2);
    }

    #[test]
    fn test_conflict_report_report_time() {
        let report = ConflictReport::generate(1, vec![], 1234567890000000);

        assert_eq!(report.report_time_us, 1234567890000000);
    }

    #[test]
    fn test_replication_status_report_with_link_health() {
        let health_report = LinkHealthReport {
            site_id: 2,
            site_name: "site2".to_string(),
            health: crate::health::LinkHealth::Healthy,
            last_successful_batch_us: Some(1000000),
            entries_behind: 0,
            consecutive_errors: 0,
        };

        let report = ReplicationStatusReport::new(
            1,
            1000000,
            "Running".to_string(),
            vec![health_report.clone()],
            "Healthy".to_string(),
            None,
            0,
            100,
            200,
        );

        assert_eq!(report.link_health.len(), 1);
        assert_eq!(report.link_health[0].site_id, 2);
    }

    #[test]
    fn test_conflict_report_debug_format() {
        let conflicts = vec![Conflict {
            inode: 100,
            local_site_id: 1,
            remote_site_id: 2,
            local_ts: 1000,
            remote_ts: 2000,
            winner_site_id: 2,
            detected_at_us: 3000000,
        }];

        let report = ConflictReport::generate(1, conflicts, 5000000);

        let debug_str = format!("{:?}", report);
        assert!(!debug_str.is_empty());
        assert!(debug_str.contains("ConflictReport"));
    }

    #[test]
    fn test_replication_status_report_debug_format() {
        let report = ReplicationStatusReport::new(
            1,
            1000000,
            "Running".to_string(),
            vec![],
            "Healthy".to_string(),
            None,
            0,
            100,
            200,
        );

        let debug_str = format!("{:?}", report);
        assert!(!debug_str.is_empty());
        assert!(debug_str.contains("ReplicationStatusReport"));
    }
}
