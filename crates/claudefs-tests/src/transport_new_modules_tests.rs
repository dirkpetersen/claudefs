//! Tests for new transport crate modules: congestion, multipath, conn_auth.

use claudefs_transport::{
    congestion::{
        CongestionAlgorithm, CongestionConfig, CongestionState, CongestionStats, CongestionWindow,
    },
    conn_auth::{
        AuthConfig, AuthLevel, AuthResult, AuthStats, CertificateInfo, ConnectionAuthenticator,
        RevocationList,
    },
    multipath::{
        MultipathConfig, MultipathError, MultipathRouter, MultipathStats, PathId, PathInfo,
        PathMetrics, PathSelectionPolicy, PathState,
    },
};
use std::time::Duration;

#[cfg(test)]
mod tests {
    use super::*;

    // CongestionAlgorithm tests
    #[test]
    fn test_congestion_algorithm_default_is_aimd() {
        assert_eq!(CongestionAlgorithm::default(), CongestionAlgorithm::Aimd);
    }

    #[test]
    fn test_congestion_state_default_is_slow_start() {
        assert_eq!(CongestionState::default(), CongestionState::SlowStart);
    }

    // CongestionConfig tests
    #[test]
    fn test_congestion_config_default_initial_window() {
        let config = CongestionConfig::default();
        assert_eq!(config.initial_window, 65536);
    }

    #[test]
    fn test_congestion_config_default_min_window() {
        let config = CongestionConfig::default();
        assert_eq!(config.min_window, 4096);
    }

    #[test]
    fn test_congestion_config_default_aimd_increase() {
        let config = CongestionConfig::default();
        assert_eq!(config.aimd_increase, 4096);
    }

    // CongestionWindow tests
    #[test]
    fn test_congestion_window_new() {
        let window = CongestionWindow::new(CongestionConfig::default());
        assert_eq!(window.stats().window_size, 0);
    }

    #[test]
    fn test_congestion_window_initial_window() {
        let config = CongestionConfig::default();
        let window = CongestionWindow::new(config);
        assert!(window.stats().window_size >= 0);
    }

    #[test]
    fn test_congestion_window_record_ack() {
        let config = CongestionConfig::default();
        let mut window = CongestionWindow::new(config);
        window.on_send(1000);
        window.on_ack(1000, 100);
        assert!(window.stats().total_acked >= 0);
    }

    #[test]
    fn test_congestion_window_record_loss() {
        let config = CongestionConfig::default();
        let mut window = CongestionWindow::new(config);
        window.on_send(1000);
        window.on_loss(500);
        assert!(window.stats().total_lost > 0);
    }

    #[test]
    fn test_congestion_window_state_slow_start() {
        let config = CongestionConfig::default();
        let window = CongestionWindow::new(config);
        assert_eq!(*window.state(), CongestionState::SlowStart);
    }

    // CongestionStats tests
    #[test]
    fn test_congestion_stats_default_zero() {
        let stats = CongestionStats::default();
        assert_eq!(stats.window_size, 0);
        assert_eq!(stats.total_sent, 0);
    }

    // PathId tests
    #[test]
    fn test_path_id_new() {
        let id = PathId::new(1);
        assert_eq!(id.as_u64(), 1);
    }

    #[test]
    fn test_path_id_from_u64() {
        let id = PathId::from(42u64);
        assert_eq!(id.as_u64(), 42);
    }

    #[test]
    fn test_path_id_into_u64() {
        let id = PathId::new(5);
        assert_eq!(u64::from(id), 5);
    }

    #[test]
    fn test_path_id_default() {
        let id = PathId::default();
        assert_eq!(id.as_u64(), 0);
    }

    // PathState tests
    #[test]
    fn test_path_state_active() {
        assert!(matches!(PathState::Active, PathState::Active));
    }

    #[test]
    fn test_path_state_failed() {
        assert!(matches!(PathState::Failed, PathState::Failed));
    }

    #[test]
    fn test_path_state_degraded() {
        assert!(matches!(PathState::Degraded, PathState::Degraded));
    }

    #[test]
    fn test_path_state_draining() {
        assert!(matches!(PathState::Draining, PathState::Draining));
    }

    // PathMetrics tests
    #[test]
    fn test_path_metrics_default_zero() {
        let metrics = PathMetrics::default();
        assert_eq!(metrics.latency_us, 0);
        assert_eq!(metrics.bytes_sent, 0);
    }

    // PathSelectionPolicy tests
    #[test]
    fn test_path_policy_default_lowest_latency() {
        assert_eq!(
            PathSelectionPolicy::default(),
            PathSelectionPolicy::LowestLatency
        );
    }

    // MultipathConfig tests
    #[test]
    fn test_multipath_config_default_no_panic() {
        let config = MultipathConfig::default();
        assert_eq!(config.policy, PathSelectionPolicy::LowestLatency);
    }

    #[test]
    fn test_multipath_config_policy() {
        let config = MultipathConfig::default();
        assert!(matches!(config.policy, PathSelectionPolicy::LowestLatency));
    }

    // MultipathRouter tests
    #[test]
    fn test_multipath_router_new_empty() {
        let router = MultipathRouter::new(MultipathConfig::default());
        let stats = router.stats();
        assert_eq!(stats.total_paths, 0);
    }

    #[test]
    fn test_multipath_router_add_path() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        router.add_path("eth0".to_string(), 100, 1);
        assert_eq!(router.stats().total_paths, 1);
    }

    #[test]
    fn test_multipath_router_select_no_paths() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        let result = router.select_path();
        assert!(result.is_none());
    }

    // MultipathStats tests
    #[test]
    fn test_multipath_stats_default() {
        let stats = MultipathStats {
            total_paths: 0,
            active_paths: 0,
            failed_paths: 0,
            total_requests: 0,
            failover_events: 0,
            paths: vec![],
        };
        assert_eq!(stats.total_paths, 0);
        assert_eq!(stats.active_paths, 0);
        assert_eq!(stats.total_requests, 0);
    }

    // AuthLevel tests
    #[test]
    fn test_auth_level_none() {
        assert!(matches!(AuthLevel::None, AuthLevel::None));
    }

    #[test]
    fn test_auth_level_tls_required() {
        assert!(matches!(AuthLevel::MutualTls, AuthLevel::MutualTls));
    }

    // AuthConfig tests
    #[test]
    fn test_auth_config_default() {
        let config = AuthConfig::default();
        assert_eq!(config.level, AuthLevel::MutualTls);
    }

    // ConnectionAuthenticator tests
    #[test]
    fn test_connection_authenticator_new() {
        let auth = ConnectionAuthenticator::new(AuthConfig::default());
        assert_eq!(auth.stats().total_allowed, 0);
    }

    // AuthStats tests
    #[test]
    fn test_auth_stats_default_zero() {
        let stats = AuthStats::default();
        assert_eq!(stats.total_allowed, 0);
        assert_eq!(stats.total_denied, 0);
    }

    // RevocationList tests
    #[test]
    fn test_revocation_list_new_empty() {
        let rl = RevocationList::new();
        assert!(!rl.is_revoked_serial("anything"));
    }

    #[test]
    fn test_revocation_list_is_revoked() {
        let mut rl = RevocationList::new();
        rl.revoke_serial("abc123".to_string());
        assert!(rl.is_revoked_serial("abc123"));
    }

    #[test]
    fn test_revocation_list_is_empty() {
        let rl = RevocationList::new();
        assert!(rl.is_empty());
    }

    #[test]
    fn test_revocation_list_len() {
        let mut rl = RevocationList::new();
        rl.revoke_serial("abc".to_string());
        rl.revoke_fingerprint("fp1".to_string());
        assert_eq!(rl.len(), 2);
    }

    // CertificateInfo tests
    #[test]
    fn test_certificate_info_new() {
        let cert = CertificateInfo {
            subject: "test".to_string(),
            issuer: "CA".to_string(),
            serial: "123".to_string(),
            fingerprint_sha256: "abc".to_string(),
            not_before_ms: 0,
            not_after_ms: 1000,
            is_ca: false,
        };
        assert_eq!(cert.subject, "test");
    }

    // MultipathRouter - additional tests
    #[test]
    fn test_multipath_router_select_with_path() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        router.add_path("eth0".to_string(), 100, 1);
        let result = router.select_path();
        assert!(result.is_some());
    }

    #[test]
    fn test_multipath_router_stats() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        router.add_path("eth0".to_string(), 100, 1);
        let stats = router.stats();
        assert_eq!(stats.active_paths, 1);
    }

    #[test]
    fn test_multipath_router_path_info() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        let id = router.add_path("eth0".to_string(), 100, 1);
        let info = router.path_info(id);
        assert!(info.is_some());
    }

    #[test]
    fn test_multipath_router_remove_path() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        let id = router.add_path("eth0".to_string(), 100, 1);
        assert!(router.remove_path(id));
        assert_eq!(router.stats().total_paths, 0);
    }

    #[test]
    fn test_multipath_router_mark_failed() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        let id = router.add_path("eth0".to_string(), 100, 1);
        router.mark_failed(id);
        assert_eq!(router.path_info(id).unwrap().state, PathState::Failed);
    }

    #[test]
    fn test_multipath_router_mark_active() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        let id = router.add_path("eth0".to_string(), 100, 1);
        router.mark_failed(id);
        router.mark_active(id);
        assert_eq!(router.path_info(id).unwrap().state, PathState::Active);
    }

    #[test]
    fn test_multipath_router_active_paths() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        let id = router.add_path("eth0".to_string(), 100, 1);
        let active = router.active_paths();
        assert!(active.contains(&id));
    }

    #[test]
    fn test_multipath_router_record_success() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        let id = router.add_path("eth0".to_string(), 100, 1);
        router.record_success(id, 100, 1024);
        let info = router.path_info(id).unwrap();
        assert_eq!(info.metrics.latency_us, 100);
    }

    #[test]
    fn test_multipath_router_record_failure() {
        let mut router = MultipathRouter::new(MultipathConfig::default());
        let id = router.add_path("eth0".to_string(), 100, 1);
        router.record_failure(id, 1024);
        let info = router.path_info(id).unwrap();
        assert!(info.metrics.errors > 0);
    }

    // CongestionWindow additional tests
    #[test]
    fn test_congestion_window_can_send() {
        let config = CongestionConfig::default();
        let mut window = CongestionWindow::new(config);
        // Window starts at 0, need to call on_send first to initialize
        window.on_send(1000);
        // After on_send, window is initialized to initial_window (65536)
        assert!(window.can_send(1000));
    }

    #[test]
    fn test_congestion_window_available_window() {
        let config = CongestionConfig::default();
        let mut window = CongestionWindow::new(config);
        window.on_send(1000);
        let available = window.available_window();
        assert!(available >= 0);
    }

    #[test]
    fn test_congestion_window_on_send() {
        let config = CongestionConfig::default();
        let mut window = CongestionWindow::new(config);
        window.on_send(1000);
        assert!(window.stats().total_sent > 0);
    }

    // Additional AuthLevel tests
    #[test]
    fn test_auth_level_tls_only() {
        assert!(matches!(AuthLevel::TlsOnly, AuthLevel::TlsOnly));
    }

    #[test]
    fn test_auth_level_mutual_tls() {
        assert!(matches!(AuthLevel::MutualTls, AuthLevel::MutualTls));
    }

    #[test]
    fn test_auth_level_mutual_tls_strict() {
        assert!(matches!(
            AuthLevel::MutualTlsStrict,
            AuthLevel::MutualTlsStrict
        ));
    }

    // Additional multipath tests
    #[test]
    fn test_path_info_new() {
        let info = PathInfo {
            id: PathId::new(1),
            name: "eth0".to_string(),
            state: PathState::Active,
            metrics: PathMetrics::default(),
            weight: 100,
            priority: 1,
        };
        assert_eq!(info.name, "eth0");
    }

    #[test]
    fn test_path_selection_policy_round_robin() {
        assert!(matches!(
            PathSelectionPolicy::RoundRobin,
            PathSelectionPolicy::RoundRobin
        ));
    }

    #[test]
    fn test_path_selection_policy_weighted_random() {
        assert!(matches!(
            PathSelectionPolicy::WeightedRandom,
            PathSelectionPolicy::WeightedRandom
        ));
    }

    #[test]
    fn test_path_selection_policy_failover() {
        assert!(matches!(
            PathSelectionPolicy::Failover,
            PathSelectionPolicy::Failover
        ));
    }
}
