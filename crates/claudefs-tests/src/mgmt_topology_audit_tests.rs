//! Tests for new management modules: topology, audit_trail, rebalance, perf_report.

use claudefs_mgmt::perf_report::{
    LatencyHistogram, LatencySample, OpKind, PerformanceTracker, SlaThreshold,
};
use claudefs_mgmt::{
    audit_trail::{AuditEvent, AuditEventKind, AuditFilter, AuditTrail},
    rebalance::{JobState, RebalanceJob, RebalanceJobId, RebalanceScheduler},
    topology::{NodeInfo, NodeRole, NodeStatus, TopologyMap},
};

#[cfg(test)]
mod tests {
    use super::*;

    // NodeRole tests
    #[test]
    fn test_node_role_storage() {
        assert!(matches!(NodeRole::Storage, NodeRole::Storage));
    }

    #[test]
    fn test_node_role_client() {
        assert!(matches!(NodeRole::Client, NodeRole::Client));
    }

    #[test]
    fn test_node_role_gateway() {
        assert!(matches!(NodeRole::Gateway, NodeRole::Gateway));
    }

    // NodeStatus tests
    #[test]
    fn test_node_status_online() {
        assert!(matches!(NodeStatus::Online, NodeStatus::Online));
    }

    #[test]
    fn test_node_status_offline() {
        assert!(matches!(NodeStatus::Offline, NodeStatus::Offline));
    }

    #[test]
    fn test_node_status_draining() {
        assert!(matches!(NodeStatus::Draining, NodeStatus::Draining));
    }

    #[test]
    fn test_node_status_degraded() {
        assert!(matches!(NodeStatus::Degraded, NodeStatus::Degraded));
    }

    #[test]
    fn test_node_status_unknown() {
        assert!(matches!(NodeStatus::Unknown, NodeStatus::Unknown));
    }

    // NodeInfo tests
    #[test]
    fn test_node_info_utilization_zero_capacity() {
        let node = NodeInfo {
            id: "node1".into(),
            site_id: "site-a".into(),
            rack_id: "rack-1".into(),
            role: NodeRole::Storage,
            status: NodeStatus::Online,
            ip: "10.0.0.1".into(),
            capacity_bytes: 0,
            used_bytes: 0,
        };
        assert_eq!(node.utilization(), 0.0);
    }

    #[test]
    fn test_node_info_utilization_half() {
        let node = NodeInfo {
            id: "node1".into(),
            site_id: "site-a".into(),
            rack_id: "rack-1".into(),
            role: NodeRole::Storage,
            status: NodeStatus::Online,
            ip: "10.0.0.1".into(),
            capacity_bytes: 1024 * 1024 * 1024,
            used_bytes: 512 * 1024 * 1024,
        };
        assert!((node.utilization() - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_node_info_utilization_full() {
        let node = NodeInfo {
            id: "node1".into(),
            site_id: "site-a".into(),
            rack_id: "rack-1".into(),
            role: NodeRole::Storage,
            status: NodeStatus::Online,
            ip: "10.0.0.1".into(),
            capacity_bytes: 1024,
            used_bytes: 1024,
        };
        assert!((node.utilization() - 1.0).abs() < 0.001);
    }

    // TopologyMap tests
    #[test]
    fn test_topology_map_new_empty() {
        let topology = TopologyMap::new();
        assert!(topology.get_node("x").is_none());
    }

    #[test]
    fn test_topology_map_upsert_get() {
        let topology = TopologyMap::new();
        let node = NodeInfo {
            id: "node1".into(),
            site_id: "site-a".into(),
            rack_id: "rack-1".into(),
            role: NodeRole::Storage,
            status: NodeStatus::Online,
            ip: "10.0.0.1".into(),
            capacity_bytes: 1000,
            used_bytes: 500,
        };
        topology.upsert_node(node);
        assert!(topology.get_node("node1").is_some());
    }

    #[test]
    fn test_topology_map_remove_existing() {
        let topology = TopologyMap::new();
        let node = NodeInfo {
            id: "node1".into(),
            site_id: "site-a".into(),
            rack_id: "rack-1".into(),
            role: NodeRole::Storage,
            status: NodeStatus::Online,
            ip: "10.0.0.1".into(),
            capacity_bytes: 1000,
            used_bytes: 500,
        };
        topology.upsert_node(node);
        assert!(topology.remove_node("node1"));
    }

    #[test]
    fn test_topology_map_remove_nonexistent() {
        let topology = TopologyMap::new();
        assert!(!topology.remove_node("xyz"));
    }

    #[test]
    fn test_topology_map_get_after_remove() {
        let topology = TopologyMap::new();
        let node = NodeInfo {
            id: "node1".into(),
            site_id: "site-a".into(),
            rack_id: "rack-1".into(),
            role: NodeRole::Storage,
            status: NodeStatus::Online,
            ip: "10.0.0.1".into(),
            capacity_bytes: 1000,
            used_bytes: 500,
        };
        topology.upsert_node(node);
        topology.remove_node("node1");
        assert!(topology.get_node("node1").is_none());
    }

    #[test]
    fn test_topology_map_nodes_by_role_empty() {
        let topology = TopologyMap::new();
        assert!(topology.nodes_by_role(NodeRole::Storage).is_empty());
    }

    #[test]
    fn test_topology_map_nodes_by_role_single() {
        let topology = TopologyMap::new();
        let node = NodeInfo {
            id: "node1".into(),
            site_id: "site-a".into(),
            rack_id: "rack-1".into(),
            role: NodeRole::Storage,
            status: NodeStatus::Online,
            ip: "10.0.0.1".into(),
            capacity_bytes: 1000,
            used_bytes: 500,
        };
        topology.upsert_node(node);
        assert_eq!(topology.nodes_by_role(NodeRole::Storage).len(), 1);
    }

    #[test]
    fn test_topology_map_nodes_by_role_filter() {
        let topology = TopologyMap::new();
        let node1 = NodeInfo {
            id: "node1".into(),
            site_id: "site-a".into(),
            rack_id: "rack-1".into(),
            role: NodeRole::Storage,
            status: NodeStatus::Online,
            ip: "10.0.0.1".into(),
            capacity_bytes: 1000,
            used_bytes: 500,
        };
        let node2 = NodeInfo {
            id: "node2".into(),
            site_id: "site-a".into(),
            rack_id: "rack-1".into(),
            role: NodeRole::Client,
            status: NodeStatus::Online,
            ip: "10.0.0.2".into(),
            capacity_bytes: 0,
            used_bytes: 0,
        };
        topology.upsert_node(node1);
        topology.upsert_node(node2);
        assert_eq!(topology.nodes_by_role(NodeRole::Storage).len(), 1);
    }

    #[test]
    fn test_topology_map_nodes_by_status() {
        let topology = TopologyMap::new();
        let node = NodeInfo {
            id: "node1".into(),
            site_id: "site-a".into(),
            rack_id: "rack-1".into(),
            role: NodeRole::Storage,
            status: NodeStatus::Online,
            ip: "10.0.0.1".into(),
            capacity_bytes: 1000,
            used_bytes: 500,
        };
        topology.upsert_node(node);
        assert_eq!(topology.nodes_by_status(NodeStatus::Online).len(), 1);
    }

    #[test]
    fn test_topology_map_nodes_in_site() {
        let topology = TopologyMap::new();
        let node = NodeInfo {
            id: "node1".into(),
            site_id: "site-a".into(),
            rack_id: "rack-1".into(),
            role: NodeRole::Storage,
            status: NodeStatus::Online,
            ip: "10.0.0.1".into(),
            capacity_bytes: 1000,
            used_bytes: 500,
        };
        topology.upsert_node(node);
        assert_eq!(topology.nodes_in_site("site-a").len(), 1);
    }

    #[test]
    fn test_topology_map_nodes_in_rack() {
        let topology = TopologyMap::new();
        let node = NodeInfo {
            id: "node1".into(),
            site_id: "site-a".into(),
            rack_id: "rack-1".into(),
            role: NodeRole::Storage,
            status: NodeStatus::Online,
            ip: "10.0.0.1".into(),
            capacity_bytes: 1000,
            used_bytes: 500,
        };
        topology.upsert_node(node);
        assert_eq!(topology.nodes_in_rack("rack-1").len(), 1);
    }

    // AuditEvent tests
    #[test]
    fn test_audit_event_new() {
        let event = AuditEvent::new(
            1,
            0,
            "admin".into(),
            "1.2.3.4".into(),
            AuditEventKind::Login,
            "/".into(),
            "ok".into(),
            true,
        );
        assert_eq!(event.id, 1);
    }

    #[test]
    fn test_audit_event_success_true() {
        let event = AuditEvent::new(
            1,
            0,
            "admin".into(),
            "1.2.3.4".into(),
            AuditEventKind::Login,
            "/".into(),
            "ok".into(),
            true,
        );
        assert!(event.success);
    }

    #[test]
    fn test_audit_event_success_false() {
        let event = AuditEvent::new(
            1,
            0,
            "admin".into(),
            "1.2.3.4".into(),
            AuditEventKind::Login,
            "/".into(),
            "failed".into(),
            false,
        );
        assert!(!event.success);
    }

    #[test]
    fn test_audit_event_kind_login() {
        assert!(matches!(AuditEventKind::Login, AuditEventKind::Login));
    }

    // AuditEventKind tests
    #[test]
    fn test_audit_event_kind_logout() {
        assert!(matches!(AuditEventKind::Logout, AuditEventKind::Logout));
    }

    #[test]
    fn test_audit_event_kind_token_create() {
        assert!(matches!(
            AuditEventKind::TokenCreate,
            AuditEventKind::TokenCreate
        ));
    }

    #[test]
    fn test_audit_event_kind_config_change() {
        assert!(matches!(
            AuditEventKind::ConfigChange,
            AuditEventKind::ConfigChange
        ));
    }

    // AuditFilter tests
    #[test]
    fn test_audit_filter_new_default() {
        let filter = AuditFilter::new();
        assert!(filter.user.is_none());
        assert!(!filter.success_only);
    }

    #[test]
    fn test_audit_filter_matches_all() {
        let filter = AuditFilter::new();
        let event = AuditEvent::new(
            1,
            0,
            "admin".into(),
            "1.2.3.4".into(),
            AuditEventKind::Login,
            "/".into(),
            "ok".into(),
            true,
        );
        assert!(filter.matches(&event));
    }

    #[test]
    fn test_audit_filter_success_only_matches_success() {
        let mut filter = AuditFilter::new();
        filter.success_only = true;
        let event = AuditEvent::new(
            1,
            0,
            "admin".into(),
            "1.2.3.4".into(),
            AuditEventKind::Login,
            "/".into(),
            "ok".into(),
            true,
        );
        assert!(filter.matches(&event));
    }

    #[test]
    fn test_audit_filter_success_only_rejects_failure() {
        let mut filter = AuditFilter::new();
        filter.success_only = true;
        let event = AuditEvent::new(
            1,
            0,
            "admin".into(),
            "1.2.3.4".into(),
            AuditEventKind::Login,
            "/".into(),
            "failed".into(),
            false,
        );
        assert!(!filter.matches(&event));
    }

    // AuditTrail tests
    #[test]
    fn test_audit_trail_new_empty() {
        let trail = AuditTrail::new();
        assert!(trail.query(&AuditFilter::new()).is_empty());
    }

    #[test]
    fn test_audit_trail_append_one() {
        let trail = AuditTrail::new();
        trail.record("admin", "1.2.3.4", AuditEventKind::Login, "/", "ok", true);
        assert_eq!(trail.query(&AuditFilter::new()).len(), 1);
    }

    #[test]
    fn test_audit_trail_append_multiple() {
        let trail = AuditTrail::new();
        trail.record("admin", "1.2.3.4", AuditEventKind::Login, "/", "ok", true);
        trail.record("admin", "1.2.3.4", AuditEventKind::Logout, "/", "bye", true);
        trail.record(
            "admin",
            "1.2.3.4",
            AuditEventKind::ConfigChange,
            "/config",
            "change",
            true,
        );
        assert_eq!(trail.query(&AuditFilter::new()).len(), 3);
    }

    #[test]
    fn test_audit_trail_filter_by_user() {
        let trail = AuditTrail::new();
        trail.record("alice", "1.2.3.4", AuditEventKind::Login, "/", "ok", true);
        trail.record("bob", "1.2.3.4", AuditEventKind::Login, "/", "ok", true);
        let mut filter = AuditFilter::new();
        filter.user = Some("alice".to_string());
        assert_eq!(trail.query(&filter).len(), 1);
    }

    // RebalanceJob and JobState tests
    #[test]
    fn test_job_state_variants() {
        assert!(matches!(JobState::Pending, JobState::Pending));
        assert!(matches!(JobState::Running, JobState::Running));
        assert!(matches!(JobState::Paused, JobState::Paused));
        assert!(matches!(JobState::Complete, JobState::Complete));
    }

    #[test]
    fn test_job_state_failed() {
        assert!(matches!(
            JobState::Failed("error".to_string()),
            JobState::Failed(_)
        ));
    }

    #[test]
    fn test_job_is_not_terminal_pending() {
        let job = RebalanceJob {
            id: 1,
            source_node: "n1".to_string(),
            target_node: "n2".to_string(),
            bytes_total: 1000,
            bytes_moved: 0,
            state: JobState::Pending,
            created_at: 0,
            updated_at: 0,
        };
        assert!(!job.is_terminal());
    }

    #[test]
    fn test_job_is_not_terminal_running() {
        let job = RebalanceJob {
            id: 1,
            source_node: "n1".to_string(),
            target_node: "n2".to_string(),
            bytes_total: 1000,
            bytes_moved: 500,
            state: JobState::Running,
            created_at: 0,
            updated_at: 0,
        };
        assert!(!job.is_terminal());
    }

    #[test]
    fn test_job_is_terminal_complete() {
        let job = RebalanceJob {
            id: 1,
            source_node: "n1".to_string(),
            target_node: "n2".to_string(),
            bytes_total: 1000,
            bytes_moved: 1000,
            state: JobState::Complete,
            created_at: 0,
            updated_at: 0,
        };
        assert!(job.is_terminal());
    }

    #[test]
    fn test_job_is_terminal_failed() {
        let job = RebalanceJob {
            id: 1,
            source_node: "n1".to_string(),
            target_node: "n2".to_string(),
            bytes_total: 1000,
            bytes_moved: 100,
            state: JobState::Failed("error".to_string()),
            created_at: 0,
            updated_at: 0,
        };
        assert!(job.is_terminal());
    }

    #[test]
    fn test_job_progress_fraction_zero_total() {
        let job = RebalanceJob {
            id: 1,
            source_node: "n1".to_string(),
            target_node: "n2".to_string(),
            bytes_total: 0,
            bytes_moved: 0,
            state: JobState::Running,
            created_at: 0,
            updated_at: 0,
        };
        assert_eq!(job.progress_fraction(), 0.0);
    }

    #[test]
    fn test_job_progress_fraction_half() {
        let job = RebalanceJob {
            id: 1,
            source_node: "n1".to_string(),
            target_node: "n2".to_string(),
            bytes_total: 100,
            bytes_moved: 50,
            state: JobState::Running,
            created_at: 0,
            updated_at: 0,
        };
        assert!((job.progress_fraction() - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_job_progress_fraction_complete() {
        let job = RebalanceJob {
            id: 1,
            source_node: "n1".to_string(),
            target_node: "n2".to_string(),
            bytes_total: 1000,
            bytes_moved: 1000,
            state: JobState::Running,
            created_at: 0,
            updated_at: 0,
        };
        assert!((job.progress_fraction() - 1.0).abs() < 0.001);
    }

    // RebalanceScheduler tests
    #[test]
    fn test_rebalance_scheduler_new() {
        let scheduler = RebalanceScheduler::new(2);
        assert_eq!(scheduler.running_count(), 0);
    }

    #[test]
    fn test_rebalance_scheduler_submit_returns_id() {
        let scheduler = RebalanceScheduler::new(2);
        let id = scheduler.submit("n1", "n2", 1000);
        assert!(id > 0);
    }

    #[test]
    fn test_rebalance_scheduler_submit_creates_job() {
        let scheduler = RebalanceScheduler::new(2);
        let id = scheduler.submit("n1", "n2", 1000);
        assert!(scheduler.get_job(id).is_some());
    }

    #[test]
    fn test_rebalance_scheduler_running_count_initial() {
        let scheduler = RebalanceScheduler::new(2);
        assert_eq!(scheduler.running_count(), 0);
    }

    #[test]
    fn test_rebalance_scheduler_pending_jobs() {
        let scheduler = RebalanceScheduler::new(0);
        scheduler.submit("n1", "n2", 1000);
        assert!(!scheduler.pending_jobs().is_empty());
    }

    #[test]
    fn test_rebalance_scheduler_active_jobs() {
        let scheduler = RebalanceScheduler::new(2);
        scheduler.submit("n1", "n2", 1000);
        assert!(!scheduler.active_jobs().is_empty());
    }

    // Performance tracking tests
    #[test]
    fn test_latency_histogram_new() {
        let hist = LatencyHistogram::new();
        assert_eq!(hist.count(), 0);
    }

    #[test]
    fn test_latency_histogram_record() {
        let mut hist = LatencyHistogram::new();
        hist.record(100);
        assert_eq!(hist.count(), 1);
    }

    #[test]
    fn test_latency_histogram_percentile() {
        let mut hist = LatencyHistogram::new();
        hist.record(100);
        hist.record(200);
        hist.record(300);
        assert_eq!(hist.percentile(50.0), 200);
    }

    #[test]
    fn test_latency_histogram_mean() {
        let mut hist = LatencyHistogram::new();
        hist.record(100);
        hist.record(200);
        assert!((hist.mean_us() - 150.0).abs() < 0.001);
    }

    #[test]
    fn test_performance_tracker_new() {
        let tracker = PerformanceTracker::new();
        assert!(tracker.histogram_for(OpKind::Read).is_none());
    }

    #[test]
    fn test_performance_tracker_record_sample() {
        let tracker = PerformanceTracker::new();
        tracker.record_sample(LatencySample {
            op: OpKind::Read,
            latency_us: 500,
            timestamp_ns: 1000,
            node_id: "node1".to_string(),
        });
        assert!(tracker.histogram_for(OpKind::Read).is_some());
    }

    #[test]
    fn test_op_kind_variants() {
        assert!(matches!(OpKind::Read, OpKind::Read));
        assert!(matches!(OpKind::Write, OpKind::Write));
        assert!(matches!(OpKind::Stat, OpKind::Stat));
    }

    #[test]
    fn test_sla_threshold_new() {
        let threshold = SlaThreshold {
            op: OpKind::Read,
            p99_target_us: 1000,
            p50_target_us: 500,
        };
        assert_eq!(threshold.op, OpKind::Read);
    }

    #[test]
    fn test_performance_tracker_set_threshold() {
        let tracker = PerformanceTracker::new();
        tracker.set_threshold(SlaThreshold {
            op: OpKind::Read,
            p99_target_us: 1000,
            p50_target_us: 500,
        });
        let violations = tracker.check_violations(0);
        assert!(violations.is_empty());
    }

    #[test]
    fn test_performance_tracker_p99_us() {
        let tracker = PerformanceTracker::new();
        for _ in 0..100 {
            tracker.record_sample(LatencySample {
                op: OpKind::Read,
                latency_us: 1000,
                timestamp_ns: 0,
                node_id: "node1".to_string(),
            });
        }
        assert!(tracker.p99_us(OpKind::Read) > 0);
    }

    #[test]
    fn test_performance_tracker_p50_us() {
        let tracker = PerformanceTracker::new();
        for _ in 0..100 {
            tracker.record_sample(LatencySample {
                op: OpKind::Write,
                latency_us: 500,
                timestamp_ns: 0,
                node_id: "node1".to_string(),
            });
        }
        assert!(tracker.p50_us(OpKind::Write) > 0);
    }
}
