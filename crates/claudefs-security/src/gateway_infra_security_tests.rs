//! Gateway infrastructure security tests: TLS, circuit breaker, lifecycle, conn pool, quota.
//!
//! Part of A10 Phase 11: Gateway infrastructure security audit

#[cfg(test)]
mod tests {
    use claudefs_gateway::gateway_circuit_breaker::{
        CircuitBreaker, CircuitBreakerConfig, CircuitBreakerError, CircuitBreakerMetrics,
        CircuitBreakerRegistry, CircuitState,
    };
    use claudefs_gateway::gateway_conn_pool::{
        BackendNode, ConnPoolConfig, ConnPoolError, ConnState, GatewayConnPool, NodeConnPool,
        PooledConn,
    };
    use claudefs_gateway::gateway_tls::{
        AlpnProtocol, CertSource, CipherPreference, ClientCertMode, TlsConfig, TlsConfigError,
        TlsConfigValidator, TlsEndpoint, TlsRegistry, TlsVersion,
    };
    use claudefs_gateway::quota::{
        QuotaLimits, QuotaManager, QuotaSubject, QuotaUsage, QuotaViolation,
    };
    use claudefs_gateway::s3_lifecycle::{
        ExpirationAction, LifecycleConfiguration, LifecycleError, LifecycleFilter,
        LifecycleRegistry, LifecycleRule, RuleStatus, StorageClass, TransitionAction,
    };

    fn make_tls_config() -> TlsConfig {
        TlsConfig {
            min_version: TlsVersion::Tls13,
            cipher_pref: CipherPreference::Modern,
            cert_source: CertSource::PemFiles {
                cert_path: "/etc/claudefs/tls/server.crt".into(),
                key_path: "/etc/claudefs/tls/server.key".into(),
            },
            client_cert_mode: ClientCertMode::None,
            alpn_protocols: vec![AlpnProtocol::Http2],
            session_cache_size: 1024,
            handshake_timeout_ms: 5000,
        }
    }

    fn make_circuit_breaker_config() -> CircuitBreakerConfig {
        CircuitBreakerConfig {
            failure_threshold: 3,
            success_threshold: 2,
            open_duration_ms: 30_000,
            timeout_ms: 5_000,
        }
    }

    fn make_backend_node(id: &str) -> BackendNode {
        BackendNode::new(id, format!("10.0.0.1:{}", 9000))
    }

    #[test]
    fn test_tls_config_defaults_are_modern() {
        let config = TlsConfig::default();
        assert_eq!(
            config.min_version,
            TlsVersion::Tls13,
            "FINDING-GW-INFRA-01: min_version should be TLS 1.3 by default"
        );
        assert_eq!(
            config.cipher_pref,
            CipherPreference::Modern,
            "FINDING-GW-INFRA-01: cipher_pref should be Modern by default"
        );
        assert_eq!(
            config.client_cert_mode,
            ClientCertMode::None,
            "FINDING-GW-INFRA-01: client_cert_mode should be None by default"
        );
        assert!(
            config.session_cache_size > 0,
            "FINDING-GW-INFRA-01: session_cache_size should be > 0"
        );
        assert!(
            config.handshake_timeout_ms > 0,
            "FINDING-GW-INFRA-01: handshake_timeout_ms should be > 0"
        );
    }

    #[test]
    fn test_tls_validate_empty_cert_path() {
        let mut config = make_tls_config();
        config.cert_source = CertSource::PemFiles {
            cert_path: "".into(),
            key_path: "/path/to/key".into(),
        };
        let result = TlsConfigValidator::validate(&config);
        assert!(
            matches!(result, Err(TlsConfigError::EmptyCertPath)),
            "FINDING-GW-INFRA-02: Empty cert path should be rejected"
        );
    }

    #[test]
    fn test_tls_validate_empty_key_path() {
        let mut config = make_tls_config();
        config.cert_source = CertSource::PemFiles {
            cert_path: "/path/to/cert".into(),
            key_path: "".into(),
        };
        let result = TlsConfigValidator::validate(&config);
        assert!(
            matches!(result, Err(TlsConfigError::EmptyKeyPath)),
            "FINDING-GW-INFRA-03: Empty key path should be rejected"
        );
    }

    #[test]
    fn test_tls_endpoint_bind_address() {
        let config = make_tls_config();
        let mut endpoint = TlsEndpoint::new("0.0.0.0", 443, config);
        assert_eq!(
            endpoint.bind_address(),
            "0.0.0.0:443",
            "Bind address should be addr:port"
        );

        endpoint.disable();
        assert!(!endpoint.enabled, "Endpoint should be disabled");

        endpoint.enable();
        assert!(endpoint.enabled, "Endpoint should be enabled");
    }

    #[test]
    fn test_tls_registry_management() {
        let mut registry = TlsRegistry::new();

        let config1 = make_tls_config();
        registry.register("s3", TlsEndpoint::new("0.0.0.0", 9000, config1));

        let config2 = make_tls_config();
        registry.register("nfs", TlsEndpoint::new("0.0.0.0", 2049, config2));

        let config3 = make_tls_config();
        registry.register("smb", TlsEndpoint::new("0.0.0.0", 445, config3));

        assert_eq!(
            registry.enabled_count(),
            3,
            "Should have 3 enabled endpoints"
        );

        registry.remove("nfs");
        assert_eq!(
            registry.enabled_count(),
            2,
            "Should have 2 enabled endpoints after removal"
        );

        let names = registry.all_names();
        assert_eq!(names.len(), 2, "Should have 2 names");
    }

    #[test]
    fn test_circuit_breaker_initial_closed() {
        let config = make_circuit_breaker_config();
        let breaker = CircuitBreaker::new("test".to_string(), config);

        assert_eq!(
            breaker.state(),
            CircuitState::Closed,
            "Initial state should be Closed"
        );
        assert_eq!(
            breaker.failure_count(),
            0,
            "Initial failure count should be 0"
        );
        assert_eq!(
            breaker.success_count(),
            0,
            "Initial success count should be 0"
        );

        let metrics = breaker.metrics();
        assert_eq!(
            metrics.successful_calls, 0,
            "Initial successful calls should be 0"
        );
        assert_eq!(metrics.failed_calls, 0, "Initial failed calls should be 0");
    }

    #[test]
    fn test_circuit_breaker_opens_on_failures() {
        let mut config = make_circuit_breaker_config();
        config.failure_threshold = 3;
        let mut breaker = CircuitBreaker::new("test".to_string(), config);

        let _: Result<(), _> =
            breaker.call(|| Err(CircuitBreakerError::OperationFailed("fail".to_string())));
        let _: Result<(), _> =
            breaker.call(|| Err(CircuitBreakerError::OperationFailed("fail".to_string())));
        assert_eq!(
            breaker.state(),
            CircuitState::Closed,
            "State should still be Closed after 2 failures"
        );

        let _: Result<(), _> =
            breaker.call(|| Err(CircuitBreakerError::OperationFailed("fail".to_string())));
        assert_eq!(
            breaker.state(),
            CircuitState::Open,
            "FINDING-GW-INFRA-04: Circuit should open at exactly failure_threshold"
        );
    }

    #[test]
    fn test_circuit_breaker_half_open_recovery() {
        let mut config = make_circuit_breaker_config();
        config.failure_threshold = 1;
        config.success_threshold = 2;
        config.open_duration_ms = 10;
        let mut breaker = CircuitBreaker::new("test".to_string(), config);

        breaker.trip();
        assert_eq!(
            breaker.state(),
            CircuitState::Open,
            "Should be Open after trip"
        );

        std::thread::sleep(std::time::Duration::from_millis(20));

        let _: Result<(), _> = breaker.call(|| Ok(()));
        assert_eq!(
            breaker.state(),
            CircuitState::HalfOpen,
            "Should transition to HalfOpen"
        );

        let _: Result<(), _> = breaker.call(|| Ok(()));
        assert_eq!(
            breaker.state(),
            CircuitState::Closed,
            "Should return to Closed after success_threshold"
        );

        let metrics = breaker.metrics();
        assert!(
            metrics.state_changes >= 3,
            "FINDING-GW-INFRA-05: Should track state_changes in metrics"
        );
    }

    #[test]
    fn test_circuit_breaker_call_rejected_when_open() {
        let mut config = make_circuit_breaker_config();
        config.failure_threshold = 1;
        let mut breaker = CircuitBreaker::new("test".to_string(), config);

        let _: Result<(), _> =
            breaker.call(|| Err(CircuitBreakerError::OperationFailed("fail".to_string())));
        assert_eq!(breaker.state(), CircuitState::Open);

        let result: Result<(), _> = breaker.call(|| Ok(()));
        assert!(
            matches!(result, Err(CircuitBreakerError::CircuitOpen { .. })),
            "FINDING-GW-INFRA-06: Should reject calls when circuit is open"
        );

        let metrics = breaker.metrics();
        assert!(
            metrics.rejected_calls >= 1,
            "FINDING-GW-INFRA-06: rejected_calls metric should increment"
        );
    }

    #[test]
    fn test_circuit_breaker_registry_reset_all() {
        let mut registry = CircuitBreakerRegistry::new();

        let config = make_circuit_breaker_config();
        {
            let cb = registry.get_or_create("backend1", config.clone());
            cb.trip();
        }
        {
            let cb = registry.get_or_create("backend2", config.clone());
            cb.trip();
        }
        {
            let cb = registry.get_or_create("backend3", config.clone());
            cb.trip();
        }

        {
            let cb = registry.get("backend1").unwrap();
            assert_eq!(cb.state(), CircuitState::Open);
        }

        registry.reset_all();

        {
            let cb = registry.get("backend1").unwrap();
            assert_eq!(
                cb.state(),
                CircuitState::Closed,
                "FINDING-GW-INFRA-07: All breakers should be reset to Closed"
            );
        }
        {
            let cb = registry.get("backend2").unwrap();
            assert_eq!(cb.state(), CircuitState::Closed);
        }
        {
            let cb = registry.get("backend3").unwrap();
            assert_eq!(cb.state(), CircuitState::Closed);
        }
    }

    #[test]
    fn test_lifecycle_rule_validation() {
        let mut config = LifecycleConfiguration::new();

        let mut rule_no_actions = LifecycleRule::new("rule-no-actions");
        let result = config.add_rule(rule_no_actions);
        assert!(
            matches!(result, Err(LifecycleError::NoActions(_))),
            "FINDING-GW-INFRA-08: Rule with no transitions or expiration should be rejected"
        );

        let mut config2 = LifecycleConfiguration::new();
        let mut rule_zero_days = LifecycleRule::new("rule-zero-days");
        rule_zero_days.transitions.push(TransitionAction {
            days: 0,
            storage_class: StorageClass::StandardIa,
        });
        let result2 = config2.add_rule(rule_zero_days);
        assert!(
            matches!(result2, Err(LifecycleError::InvalidDays(0))),
            "FINDING-GW-INFRA-08: Rule with days=0 transition should be rejected"
        );
    }

    #[test]
    fn test_lifecycle_duplicate_rule_id() {
        let mut config = LifecycleConfiguration::new();

        let mut rule1 = LifecycleRule::new("rule1");
        rule1.transitions.push(TransitionAction {
            days: 30,
            storage_class: StorageClass::StandardIa,
        });
        config.add_rule(rule1).unwrap();

        let mut rule2 = LifecycleRule::new("rule1");
        rule2.expiration = Some(ExpirationAction::new(90));
        let result = config.add_rule(rule2);
        assert!(
            matches!(result, Err(LifecycleError::DuplicateRuleId(_))),
            "FINDING-GW-INFRA-09: Duplicate rule ID should be rejected"
        );
    }

    #[test]
    fn test_lifecycle_max_rules() {
        let mut config = LifecycleConfiguration::new();

        for i in 0..1000 {
            let mut rule = LifecycleRule::new(format!("rule-{}", i));
            rule.transitions.push(TransitionAction {
                days: 30,
                storage_class: StorageClass::StandardIa,
            });
            config.add_rule(rule).unwrap();
        }

        let mut rule_1001 = LifecycleRule::new("rule-1001");
        rule_1001.transitions.push(TransitionAction {
            days: 30,
            storage_class: StorageClass::StandardIa,
        });
        let result = config.add_rule(rule_1001);
        assert!(
            matches!(result, Err(LifecycleError::TooManyRules(1001))),
            "FINDING-GW-INFRA-10: Adding 1001st rule should be rejected (DoS prevention)"
        );
    }

    #[test]
    fn test_lifecycle_filter_matching() {
        let filter = LifecycleFilter::with_prefix("data/");
        assert!(
            filter.matches("data/file.txt", 100, &[]),
            "Should match prefix"
        );
        assert!(
            !filter.matches("other/file.txt", 100, &[]),
            "Should not match non-prefix"
        );

        let mut size_filter = LifecycleFilter::new();
        size_filter.object_size_greater_than = Some(100);
        size_filter.object_size_less_than = Some(1000);
        assert!(
            size_filter.matches("file.txt", 500, &[]),
            "Should match size range"
        );
        assert!(
            !size_filter.matches("file.txt", 50, &[]),
            "Should not match below min"
        );
        assert!(
            !size_filter.matches("file.txt", 1500, &[]),
            "Should not match above max"
        );
    }

    #[test]
    fn test_lifecycle_expiration_evaluation() {
        let mut config = LifecycleConfiguration::new();

        let mut rule = LifecycleRule::new("expire-rule");
        rule.expiration = Some(ExpirationAction::new(30));
        config.add_rule(rule).unwrap();

        assert!(
            !config.is_object_expired("data/file.txt", 100, &[], 29),
            "Should NOT be expired at 29 days"
        );
        assert!(
            config.is_object_expired("data/file.txt", 100, &[], 30),
            "FINDING-GW-INFRA-11: Should be expired at exactly 30 days"
        );
        assert!(
            config.is_object_expired("data/file.txt", 100, &[], 31),
            "Should be expired at 31 days"
        );
    }

    #[test]
    fn test_conn_pool_config_defaults() {
        let config = ConnPoolConfig::default();
        assert!(config.min_per_node >= 1, "min_per_node should be >= 1");
        assert!(
            config.max_per_node > config.min_per_node,
            "max_per_node should be > min_per_node"
        );
        assert!(config.max_idle_ms > 0, "max_idle_ms should be > 0");
        assert!(
            config.connect_timeout_ms > 0,
            "connect_timeout_ms should be > 0"
        );
    }

    #[test]
    fn test_conn_pool_checkout_checkin() {
        let config = ConnPoolConfig::new(1, 5, 60000);
        let node1 = make_backend_node("node1");
        let node2 = make_backend_node("node2");
        let mut pool = GatewayConnPool::new(vec![node1, node2], config);

        if let Some(pool) = pool.get_pool_mut("node1") {
            pool.add_conn();
        }
        if let Some(pool) = pool.get_pool_mut("node2") {
            pool.add_conn();
        }

        let result1 = pool.checkout();
        assert!(result1.is_some(), "First checkout should succeed");
        let (node_id1, conn_id1) = result1.unwrap();

        pool.checkin(&node_id1, conn_id1);

        let result2 = pool.checkout();
        assert!(result2.is_some(), "Second checkout should succeed");
        let (node_id2, _conn_id2) = result2.unwrap();

        assert_ne!(
            node_id1, node_id2,
            "FINDING-GW-INFRA-12: Should show round-robin behavior"
        );
    }

    #[test]
    fn test_conn_pool_exhaustion() {
        let config = ConnPoolConfig::new(1, 2, 60000);
        let node = make_backend_node("node1");
        let mut pool = NodeConnPool::new(node, config);

        let conn1 = pool.checkout();
        assert!(conn1.is_some(), "First checkout should succeed");

        let conn2 = pool.checkout();
        assert!(conn2.is_some(), "Second checkout should succeed");

        let conn3 = pool.checkout();
        assert!(
            conn3.is_none(),
            "FINDING-GW-INFRA-13: Third checkout should return None (pool exhausted)"
        );

        if let Some(id) = conn1 {
            pool.checkin(id);
        }

        let conn4 = pool.checkout();
        assert!(conn4.is_some(), "Checkout after checkin should succeed");
    }

    #[test]
    fn test_conn_pool_unhealthy_marking() {
        let config = ConnPoolConfig::new(1, 5, 60000);
        let node = make_backend_node("node1");
        let mut pool = NodeConnPool::new(node, config);

        let conn_id = pool.checkout().unwrap();
        pool.checkin(conn_id);

        let healthy_before = pool.healthy_count();

        pool.mark_unhealthy(conn_id, "connection reset by peer");

        let conn = pool.get_conn(conn_id).unwrap();
        assert!(
            !conn.is_healthy(),
            "FINDING-GW-INFRA-14: Connection should be unhealthy after marking"
        );

        let healthy_after = pool.healthy_count();
        assert!(
            healthy_after < healthy_before,
            "healthy_count should decrease"
        );
    }

    #[test]
    fn test_conn_pool_node_removal() {
        let config = ConnPoolConfig::new(1, 5, 60000);
        let node1 = make_backend_node("node1");
        let node2 = make_backend_node("node2");
        let node3 = make_backend_node("node3");
        let mut pool = GatewayConnPool::new(vec![node1, node2, node3], config);

        if let Some(pool) = pool.get_pool_mut("node1") {
            pool.add_conn();
        }
        if let Some(pool) = pool.get_pool_mut("node2") {
            pool.add_conn();
        }
        if let Some(pool) = pool.get_pool_mut("node3") {
            pool.add_conn();
        }

        assert_eq!(pool.node_count(), 3, "Should have 3 nodes");

        pool.remove_node("node2");

        assert_eq!(
            pool.node_count(),
            2,
            "FINDING-GW-INFRA-15: Should have 2 nodes after removal"
        );

        let result = pool.checkout();
        assert!(
            result.is_some(),
            "Checkout should work with remaining nodes"
        );
        let (node_id, _) = result.unwrap();
        assert!(node_id != "node2", "Should not return removed node");
    }

    #[test]
    fn test_quota_write_hard_limit() {
        let manager = QuotaManager::new();
        let subject = QuotaSubject::User(1000);
        manager.set_limits(subject, QuotaLimits::new(1000, 10));

        let violation1 = manager.record_write(subject, 500);
        assert_eq!(
            violation1,
            QuotaViolation::None,
            "500 bytes should be allowed"
        );

        let violation2 = manager.record_write(subject, 600);
        assert_eq!(
            violation2,
            QuotaViolation::HardLimitExceeded,
            "FINDING-GW-INFRA-16: 1100 bytes should exceed hard limit"
        );

        let usage = manager.get_usage(subject);
        assert_eq!(
            usage.bytes_used, 500,
            "Usage should not be updated on hard limit exceeded"
        );
    }

    #[test]
    fn test_quota_soft_limit_warning() {
        let manager = QuotaManager::new();
        let subject = QuotaSubject::User(1000);
        manager.set_limits(subject, QuotaLimits::with_soft(1000, 500, 10, 5));

        let violation = manager.record_write(subject, 600);
        assert_eq!(
            violation,
            QuotaViolation::SoftLimitExceeded,
            "FINDING-GW-INFRA-17: Soft limit exceeded should warn but allow"
        );

        let usage = manager.get_usage(subject);
        assert_eq!(
            usage.bytes_used, 0,
            "Soft limit exceeded - write NOT recorded (current impl)"
        );
    }

    #[test]
    fn test_quota_inode_enforcement() {
        let manager = QuotaManager::new();
        let subject = QuotaSubject::User(1000);
        manager.set_limits(subject, QuotaLimits::new(1000000, 5));

        for _ in 0..5 {
            let violation = manager.record_create(subject);
            assert_eq!(
                violation,
                QuotaViolation::None,
                "First 5 creates should succeed"
            );
        }

        let violation = manager.record_create(subject);
        assert_eq!(
            violation,
            QuotaViolation::HardLimitExceeded,
            "FINDING-GW-INFRA-18: 6th create should exceed inode hard limit"
        );
    }

    #[test]
    fn test_quota_delete_reclaims() {
        let manager = QuotaManager::new();
        let subject = QuotaSubject::User(1000);
        manager.set_limits(subject, QuotaLimits::new(1000, 100));

        manager.record_write(subject, 500);
        let usage_after_write = manager.get_usage(subject);
        assert_eq!(
            usage_after_write.bytes_used, 500,
            "Usage should be 500 after write"
        );

        manager.record_delete(subject, 200);
        let usage_after_delete = manager.get_usage(subject);
        assert_eq!(
            usage_after_delete.bytes_used, 300,
            "FINDING-GW-INFRA-19: Usage should be 300 after delete"
        );
    }

    #[test]
    fn test_quota_check_without_recording() {
        let manager = QuotaManager::new();
        let subject = QuotaSubject::User(1000);
        manager.set_limits(subject, QuotaLimits::new(100, 10));

        manager.record_write(subject, 50);
        let usage_before = manager.get_usage(subject);
        assert_eq!(usage_before.bytes_used, 50, "Usage should be 50");

        let violation = manager.check_write(subject, 60);
        assert_eq!(
            violation,
            QuotaViolation::HardLimitExceeded,
            "FINDING-GW-INFRA-20: check_write should return violation without recording"
        );

        let usage_after = manager.get_usage(subject);
        assert_eq!(
            usage_after.bytes_used, 50,
            "Usage should still be 50 (check didn't record)"
        );
    }
}
