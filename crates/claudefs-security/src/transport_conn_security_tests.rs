//! Connection security tests for claudefs-transport: migration, mux, keepalive, deadline, cancel.
//!
//! Part of A10 Phase 9: Transport connection security audit

#[cfg(test)]
mod tests {
    use claudefs_transport::{
        batch::{BatchCollector, BatchConfig, BatchEnvelope, BatchRequest, BatchResponse},
        cancel::{new_cancel_pair, CancelReason, CancelRegistry},
        connmigrate::{
            ConnectionId, MigrationConfig, MigrationError, MigrationManager, MigrationReason,
            MigrationState,
        },
        deadline::{decode_deadline, encode_deadline, Deadline, DeadlineContext},
        hedge::{HedgeConfig, HedgePolicy},
        keepalive::{KeepAliveConfig, KeepAliveState, KeepAliveTracker},
        mux::{Multiplexer, MuxConfig},
    };

    fn make_migration_manager(max_concurrent: usize, enabled: bool) -> MigrationManager {
        MigrationManager::new(MigrationConfig {
            max_concurrent_migrations: max_concurrent,
            enabled,
            ..Default::default()
        })
    }

    fn make_mux(max_streams: u32) -> Multiplexer {
        Multiplexer::new(MuxConfig {
            max_concurrent_streams: max_streams,
            ..Default::default()
        })
    }

    fn make_keepalive_tracker(enabled: bool) -> KeepAliveTracker {
        KeepAliveTracker::new(KeepAliveConfig {
            enabled,
            max_missed: 3,
            ..Default::default()
        })
    }

    fn make_hedge_policy(enabled: bool, exclude_writes: bool) -> HedgePolicy {
        HedgePolicy::new(HedgeConfig {
            enabled,
            exclude_writes,
            ..Default::default()
        })
    }

    // ============================================================================
    // Category 1: Connection Migration Security (5 tests)
    // ============================================================================

    #[test]
    fn test_migration_concurrent_limit() {
        let manager = make_migration_manager(2, true);

        let _id1 = manager
            .start_migration(ConnectionId(1), ConnectionId(2), MigrationReason::NodeDrain)
            .unwrap();
        let _id2 = manager
            .start_migration(
                ConnectionId(3),
                ConnectionId(4),
                MigrationReason::LoadBalance,
            )
            .unwrap();

        let result = manager.start_migration(
            ConnectionId(5),
            ConnectionId(6),
            MigrationReason::VersionUpgrade,
        );

        assert!(matches!(
            result,
            Err(MigrationError::TooManyConcurrent { max: 2 })
        ));
    }

    #[test]
    fn test_migration_already_migrating() {
        let manager = make_migration_manager(4, true);

        let _id1 = manager
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
    fn test_migration_id_uniqueness() {
        let manager = make_migration_manager(10, true);

        let mut ids = Vec::new();
        for i in 1..=10 {
            let id = manager
                .start_migration(
                    ConnectionId(i),
                    ConnectionId(i + 100),
                    MigrationReason::NodeDrain,
                )
                .unwrap();
            ids.push(id);
        }

        let mut sorted_ids = ids.clone();
        sorted_ids.sort();
        sorted_ids.dedup();

        assert_eq!(
            ids.len(),
            sorted_ids.len(),
            "All migration IDs should be unique"
        );
    }

    #[test]
    fn test_migration_state_machine() {
        let manager = make_migration_manager(4, true);

        let id1 = manager
            .start_migration(ConnectionId(1), ConnectionId(2), MigrationReason::NodeDrain)
            .unwrap();

        manager.record_request_migrated(id1);
        manager.complete_migration(id1);

        let record1 = manager.get_migration(id1).unwrap();
        assert_eq!(record1.state, MigrationState::Completed);

        let id2 = manager
            .start_migration(
                ConnectionId(3),
                ConnectionId(4),
                MigrationReason::HealthDegraded,
            )
            .unwrap();

        manager.record_request_failed(id2);
        manager.fail_migration(id2);

        let record2 = manager.get_migration(id2).unwrap();
        assert_eq!(record2.state, MigrationState::Failed);
    }

    #[test]
    fn test_migration_disabled() {
        let manager = make_migration_manager(4, false);

        let result =
            manager.start_migration(ConnectionId(1), ConnectionId(2), MigrationReason::NodeDrain);

        assert!(matches!(result, Err(MigrationError::Disabled)));
    }

    // ============================================================================
    // Category 2: Multiplexing Security (5 tests)
    // ============================================================================

    #[test]
    fn test_mux_max_concurrent_streams() {
        let mux = make_mux(3);

        let _ = mux.open_stream().unwrap();
        let _ = mux.open_stream().unwrap();
        let _ = mux.open_stream().unwrap();

        let result = mux.open_stream();

        assert!(result.is_err());
    }

    #[test]
    fn test_mux_stream_id_uniqueness() {
        let mux = make_mux(200);

        let mut ids = Vec::new();
        for _ in 0..100 {
            let (id, _) = mux.open_stream().unwrap();
            ids.push(id);
        }

        let mut sorted_ids = ids.clone();
        sorted_ids.sort();
        sorted_ids.dedup();

        assert_eq!(
            ids.len(),
            sorted_ids.len(),
            "All stream IDs should be unique"
        );
    }

    #[test]
    fn test_mux_dispatch_unknown_stream() {
        use claudefs_transport::protocol::Frame;
        use claudefs_transport::protocol::Opcode;

        let mux = make_mux(10);

        let frame = Frame::new(Opcode::Read, 999, vec![]);
        let result = mux.dispatch_response(999, frame);

        assert!(!result, "Should return false for unknown stream");
        assert_eq!(mux.active_streams(), 0, "Should not panic or leak");
    }

    #[test]
    fn test_mux_cancel_stream() {
        let mux = make_mux(10);

        let _ = mux.open_stream().unwrap();
        assert_eq!(mux.active_streams(), 1);

        let cancelled = mux.cancel_stream(1);
        assert!(cancelled);
        assert_eq!(mux.active_streams(), 0);

        let _ = mux.open_stream().unwrap();
        assert_eq!(mux.active_streams(), 1);
    }

    #[test]
    fn test_mux_cancel_nonexistent() {
        let mux = make_mux(10);

        let result = mux.cancel_stream(999);

        assert!(!result, "Should return false for non-existent stream");
        assert_eq!(mux.active_streams(), 0);
    }

    // ============================================================================
    // Category 3: Keep-Alive State Machine (5 tests)
    // ============================================================================

    #[test]
    fn test_keepalive_initial_state() {
        let tracker = make_keepalive_tracker(true);

        assert_eq!(tracker.state(), KeepAliveState::Active);
        assert_eq!(tracker.missed_count(), 0);
    }

    #[test]
    fn test_keepalive_timeout_transitions() {
        let tracker = make_keepalive_tracker(true);

        assert_eq!(tracker.state(), KeepAliveState::Active);

        tracker.record_timeout();
        assert_eq!(tracker.state(), KeepAliveState::Warning);
        assert_eq!(tracker.missed_count(), 1);

        tracker.record_timeout();
        assert_eq!(tracker.missed_count(), 2);

        tracker.record_timeout();
        assert_eq!(tracker.state(), KeepAliveState::Dead);
        assert_eq!(tracker.missed_count(), 3);
    }

    #[test]
    fn test_keepalive_reset_recovers() {
        let tracker = make_keepalive_tracker(true);

        tracker.record_timeout();
        tracker.record_timeout();
        tracker.record_timeout();

        assert_eq!(tracker.state(), KeepAliveState::Dead);
        assert_eq!(tracker.missed_count(), 3);

        tracker.reset();

        assert_eq!(tracker.state(), KeepAliveState::Active);
        assert_eq!(tracker.missed_count(), 0);
    }

    #[test]
    fn test_keepalive_disabled_state() {
        let tracker = make_keepalive_tracker(false);

        assert_eq!(tracker.state(), KeepAliveState::Disabled);

        tracker.record_timeout();
        assert_eq!(tracker.state(), KeepAliveState::Disabled);
    }

    #[test]
    fn test_keepalive_is_alive_check() {
        let tracker = make_keepalive_tracker(true);

        assert!(tracker.is_alive(), "Should be alive in Active state");

        tracker.record_timeout();
        assert!(tracker.is_alive(), "Should still be alive in Warning state");

        tracker.record_timeout();
        tracker.record_timeout();
        assert!(!tracker.is_alive(), "Should not be alive in Dead state");
    }

    // ============================================================================
    // Category 4: Deadline & Hedge (5 tests)
    // ============================================================================

    #[test]
    fn test_deadline_zero_duration_expired() {
        let deadline = Deadline::new(std::time::Duration::ZERO);

        assert!(
            deadline.is_expired(),
            "Zero duration deadline should be expired immediately"
        );
        assert!(
            deadline.remaining().is_none(),
            "Should return None for remaining time"
        );
    }

    #[test]
    fn test_deadline_encode_decode_roundtrip() {
        use std::time::Duration;

        let ctx = DeadlineContext::with_timeout(Duration::from_secs(30));

        let encoded = encode_deadline(&ctx);
        let decoded = decode_deadline(&encoded);

        let original_expiry = ctx.deadline().map(|d| d.expiry_ms());
        let decoded_expiry = decoded.deadline().map(|d| d.expiry_ms());

        assert_eq!(original_expiry, decoded_expiry);
    }

    #[test]
    fn test_deadline_no_deadline_check_ok() {
        let ctx = DeadlineContext::new();

        let result = ctx.check();
        assert!(result.is_ok(), "No deadline should return Ok");

        assert!(!ctx.is_expired(), "No deadline should not be expired");
    }

    #[test]
    fn test_hedge_disabled_blocks_all() {
        let policy = make_hedge_policy(false, false);

        let result = policy.should_hedge(1000, false);

        assert!(!result, "Disabled hedging should block all hedge requests");
    }

    #[test]
    fn test_hedge_write_exclusion() {
        let policy = make_hedge_policy(true, true);

        let write_result = policy.should_hedge(1000, true);
        assert!(
            !write_result,
            "Writes should be excluded when exclude_writes=true"
        );

        let policy2 = make_hedge_policy(true, false);
        let read_result = policy2.should_hedge(1000, true);
        assert!(
            read_result,
            "Writes should be allowed when exclude_writes=false"
        );
    }

    // ============================================================================
    // Category 5: Cancellation & Batch (5 tests)
    // ============================================================================

    #[test]
    fn test_cancel_token_propagation() {
        let (token, handle) = new_cancel_pair();

        handle.cancel(CancelReason::ClientDisconnected);

        assert!(token.is_cancelled(), "Token should be cancelled");
        assert_eq!(
            token.cancelled_reason(),
            Some(CancelReason::ClientDisconnected)
        );
    }

    #[test]
    fn test_cancel_registry_cancel_all() {
        let registry = CancelRegistry::new();

        registry.register(1);
        registry.register(2);
        registry.register(3);
        registry.register(4);
        registry.register(5);

        assert_eq!(registry.active_count(), 5);

        registry.cancel_all(CancelReason::ServerShutdown);

        let stats = registry.stats();
        assert_eq!(stats.total_cancelled, 5);
    }

    #[test]
    fn test_cancel_child_independence() {
        let (token, handle) = new_cancel_pair();
        let (child_token, child_handle) = token.child();

        child_handle.cancel(CancelReason::UserRequested);

        assert!(
            !token.is_cancelled(),
            "Parent should NOT be cancelled when child is cancelled"
        );
        assert!(child_token.is_cancelled(), "Child should be cancelled");

        handle.cancel(CancelReason::ServerShutdown);

        assert!(token.is_cancelled(), "Parent should be cancelled");
        assert!(
            child_token.is_cancelled(),
            "Child should also be cancelled when parent is cancelled"
        );
    }

    #[test]
    fn test_batch_envelope_encode_decode() {
        let requests = vec![
            BatchRequest::new(1, 1, vec![1, 2, 3]),
            BatchRequest::new(2, 2, vec![4, 5, 6, 7]),
            BatchRequest::new(3, 3, vec![8, 9]),
        ];

        let envelope = BatchEnvelope::new_request_batch(42, requests);

        let encoded = envelope.encode();
        let decoded = BatchEnvelope::decode(&encoded).unwrap();

        assert_eq!(decoded.len(), 3);
        assert_eq!(
            decoded.total_payload_bytes(),
            envelope.total_payload_bytes()
        );
    }

    #[test]
    fn test_batch_response_error_tracking() {
        let error_response = BatchResponse::error(1, 1, "error message".to_string());
        assert!(
            error_response.is_error(),
            "Error response should report is_error=true"
        );

        let success_response = BatchResponse::success(2, 2, vec![1, 2, 3]);
        assert!(
            !success_response.is_error(),
            "Success response should report is_error=false"
        );
    }
}
