//! Transport Integration Tests
//!
//! Deep integration tests for the claudefs-transport crate, testing the real APIs.

use claudefs_transport::protocol::{FRAME_HEADER_SIZE, MAGIC, MAX_PAYLOAD_SIZE, PROTOCOL_VERSION};
use claudefs_transport::{
    CircuitBreaker, CircuitBreakerConfig, CircuitState, CompositeRateLimiter, ConsistentHashRing,
    Frame, FrameFlags, FrameHeader, NodeId, NodeInfo, Opcode, RateLimitConfig, RateLimiter,
    TransportMetrics,
};
use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Duration;

fn make_addr(port: u16) -> SocketAddr {
    SocketAddr::from_str(&format!("192.168.1.1:{}", port)).unwrap()
}

mod circuit_breaker_tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_new_with_config() {
        let config = CircuitBreakerConfig::default();
        let breaker = CircuitBreaker::new(config.clone());
        assert_eq!(breaker.state(), CircuitState::Closed);
    }

    #[test]
    fn test_circuit_breaker_default_config() {
        let config = CircuitBreakerConfig::default();
        assert_eq!(config.failure_threshold, 5);
        assert_eq!(config.success_threshold, 3);
    }

    #[test]
    fn test_record_success_resets_failure_count() {
        let config = CircuitBreakerConfig::default();
        let breaker = CircuitBreaker::new(config);

        breaker.record_failure();
        breaker.record_failure();
        assert_eq!(breaker.failure_count(), 2);

        breaker.record_success();
        assert_eq!(breaker.failure_count(), 0);
    }

    #[test]
    fn test_record_failure_increments_count() {
        let config = CircuitBreakerConfig::default();
        let breaker = CircuitBreaker::new(config);

        breaker.record_failure();
        breaker.record_failure();
        breaker.record_failure();

        assert_eq!(breaker.failure_count(), 3);
    }

    #[test]
    fn test_state_transitions_closed_to_open() {
        let mut config = CircuitBreakerConfig::default();
        config.failure_threshold = 3;
        let breaker = CircuitBreaker::new(config);

        breaker.record_failure();
        assert_eq!(breaker.state(), CircuitState::Closed);

        breaker.record_failure();
        assert_eq!(breaker.state(), CircuitState::Closed);

        breaker.record_failure();
        assert_eq!(breaker.state(), CircuitState::Open);
    }

    #[test]
    fn test_can_execute_false_when_open() {
        let mut config = CircuitBreakerConfig::default();
        config.failure_threshold = 2;
        let breaker = CircuitBreaker::new(config);

        breaker.record_failure();
        breaker.record_failure();

        assert!(!breaker.can_execute());
    }

    #[test]
    fn test_can_execute_true_when_closed() {
        let config = CircuitBreakerConfig::default();
        let breaker = CircuitBreaker::new(config);

        assert!(breaker.can_execute());
    }

    #[test]
    fn test_state_transitions_open_to_half_open() {
        let mut config = CircuitBreakerConfig::default();
        config.failure_threshold = 2;
        config.open_duration = Duration::from_millis(50);
        let breaker = CircuitBreaker::new(config);

        breaker.record_failure();
        breaker.record_failure();
        assert_eq!(breaker.state(), CircuitState::Open);

        std::thread::sleep(Duration::from_millis(60));

        assert!(breaker.can_execute());
    }

    #[test]
    fn test_half_open_to_closed_on_success() {
        let mut config = CircuitBreakerConfig::default();
        config.failure_threshold = 2;
        config.open_duration = Duration::from_millis(50);
        config.success_threshold = 2;
        let breaker = CircuitBreaker::new(config);

        breaker.record_failure();
        breaker.record_failure();

        std::thread::sleep(Duration::from_millis(60));
        breaker.can_execute();

        breaker.record_success();
        assert_eq!(breaker.state(), CircuitState::HalfOpen);

        breaker.record_success();
        assert_eq!(breaker.state(), CircuitState::Closed);
    }

    #[test]
    fn test_half_open_to_open_on_failure() {
        let mut config = CircuitBreakerConfig::default();
        config.failure_threshold = 2;
        config.open_duration = Duration::from_millis(50);
        let breaker = CircuitBreaker::new(config);

        breaker.record_failure();
        breaker.record_failure();

        std::thread::sleep(Duration::from_millis(60));
        breaker.can_execute();

        breaker.record_failure();
        assert_eq!(breaker.state(), CircuitState::Open);
    }

    #[test]
    fn test_reset_clears_state() {
        let config = CircuitBreakerConfig::default();
        let breaker = CircuitBreaker::new(config);

        for _ in 0..5 {
            breaker.record_failure();
        }
        assert_eq!(breaker.state(), CircuitState::Open);

        breaker.reset();
        assert_eq!(breaker.state(), CircuitState::Closed);
        assert!(breaker.can_execute());
    }

    #[test]
    fn test_success_count_tracking() {
        // Test that record_success in closed state resets failure count
        let config = CircuitBreakerConfig::default();
        let breaker = CircuitBreaker::new(config);

        breaker.record_failure();
        assert_eq!(breaker.failure_count(), 1);

        breaker.record_success();
        assert_eq!(breaker.failure_count(), 0);
    }
}

mod rate_limiter_tests {
    use super::*;

    #[test]
    fn test_rate_limiter_new_with_config() {
        let config = RateLimitConfig::new(1000, 100);
        let limiter = RateLimiter::new(config);

        assert!(limiter.try_acquire());
    }

    #[test]
    fn test_rate_limiter_default_config() {
        let config = RateLimitConfig::default();
        assert_eq!(config.requests_per_second, 10000);
        assert_eq!(config.burst_size, 1000);
    }

    #[test]
    fn test_try_acquire_returns_true_within_limit() {
        let config = RateLimitConfig::new(1000, 50);
        let limiter = RateLimiter::new(config);

        for _ in 0..50 {
            assert!(limiter.try_acquire());
        }
    }

    #[test]
    fn test_try_acquire_returns_false_when_exhausted() {
        let config = RateLimitConfig::new(1000, 10);
        let limiter = RateLimiter::new(config);

        for _ in 0..10 {
            limiter.try_acquire();
        }

        assert!(!limiter.try_acquire());
    }

    #[test]
    fn test_try_acquire_n() {
        let config = RateLimitConfig::new(1000, 100);
        let limiter = RateLimiter::new(config);

        assert!(limiter.try_acquire_n(50));
        assert!(limiter.try_acquire_n(30));
        assert!(!limiter.try_acquire_n(30));
    }

    #[test]
    fn test_available_tokens() {
        let config = RateLimitConfig::new(1000, 100);
        let limiter = RateLimiter::new(config);

        assert_eq!(limiter.available_tokens(), 100);

        limiter.try_acquire_n(30);
        assert_eq!(limiter.available_tokens(), 70);
    }

    #[test]
    fn test_reset_restores_tokens() {
        let config = RateLimitConfig::new(1000, 100);
        let limiter = RateLimiter::new(config);

        for _ in 0..100 {
            limiter.try_acquire();
        }

        limiter.reset();
        assert_eq!(limiter.available_tokens(), 100);
    }

    #[test]
    fn test_composite_rate_limiter_allows() {
        let config = RateLimitConfig::new(10000, 1000);
        let composite = CompositeRateLimiter::new(config.clone(), config);

        for _ in 0..100 {
            assert!(matches!(
                composite.check(),
                claudefs_transport::RateLimitResult::Allowed
            ));
        }
    }

    #[test]
    fn test_composite_rate_limiter_limits_per_connection() {
        let per_conn = RateLimitConfig::new(1000, 5);
        let global = RateLimitConfig::new(10000, 1000);
        let composite = CompositeRateLimiter::new(per_conn, global);

        for _ in 0..5 {
            composite.check();
        }

        assert!(matches!(
            composite.check(),
            claudefs_transport::RateLimitResult::Limited { .. }
        ));
    }
}

mod consistent_hash_ring_tests {
    use super::*;

    #[test]
    fn test_hash_ring_new() {
        let ring = ConsistentHashRing::new();
        assert_eq!(ring.node_count(), 0);
    }

    #[test]
    fn test_add_node() {
        let mut ring = ConsistentHashRing::new();
        let node = NodeInfo::new(NodeId::new(1), make_addr(9001));
        ring.add_node(node, 50);

        assert_eq!(ring.node_count(), 1);
    }

    #[test]
    fn test_lookup_returns_node_for_key() {
        let mut ring = ConsistentHashRing::new();
        let node = NodeInfo::new(NodeId::new(1), make_addr(9001));
        ring.add_node(node, 50);

        let result = ring.lookup(b"test-key");
        assert!(result.is_some());
    }

    #[test]
    fn test_lookup_returns_none_for_empty_ring() {
        let ring = ConsistentHashRing::new();
        let result = ring.lookup(b"test-key");
        assert!(result.is_none());
    }

    #[test]
    fn test_lookup_n_returns_distinct_nodes() {
        let mut ring = ConsistentHashRing::new();

        let node1 = NodeInfo::new(NodeId::new(1), make_addr(9001));
        let node2 = NodeInfo::new(NodeId::new(2), make_addr(9002));
        let node3 = NodeInfo::new(NodeId::new(3), make_addr(9003));

        ring.add_node(node1, 50);
        ring.add_node(node2, 50);
        ring.add_node(node3, 50);

        let result = ring.lookup_n(b"test-key", 3);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_remove_node() {
        let mut ring = ConsistentHashRing::new();
        let node = NodeInfo::new(NodeId::new(1), make_addr(9001));
        ring.add_node(node, 50);

        ring.remove_node(NodeId::new(1));
        assert_eq!(ring.node_count(), 0);
    }

    #[test]
    fn test_consistent_mapping_same_key() {
        let mut ring1 = ConsistentHashRing::new();
        let mut ring2 = ConsistentHashRing::new();

        let node = NodeInfo::new(NodeId::new(1), make_addr(9001));

        ring1.add_node(node.clone(), 50);
        ring2.add_node(node, 50);

        let key = b"consistent-key";
        assert_eq!(ring1.lookup(key), ring2.lookup(key));
    }
}

mod transport_metrics_tests {
    use super::*;

    #[test]
    fn test_metrics_new() {
        let metrics = TransportMetrics::new();
        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.requests_sent, 0);
        assert_eq!(snapshot.requests_received, 0);
        assert_eq!(snapshot.bytes_sent, 0);
        assert_eq!(snapshot.bytes_received, 0);
        assert_eq!(snapshot.errors_total, 0);
    }

    #[test]
    fn test_record_requests_sent() {
        let metrics = TransportMetrics::new();

        metrics.inc_requests_sent();
        metrics.inc_requests_sent();

        assert_eq!(metrics.snapshot().requests_sent, 2);
    }

    #[test]
    fn test_record_requests_received() {
        let metrics = TransportMetrics::new();

        metrics.inc_requests_received();

        assert_eq!(metrics.snapshot().requests_received, 1);
    }

    #[test]
    fn test_add_bytes_sent() {
        let metrics = TransportMetrics::new();

        metrics.add_bytes_sent(1024);
        metrics.add_bytes_sent(512);

        assert_eq!(metrics.snapshot().bytes_sent, 1536);
    }

    #[test]
    fn test_add_bytes_received() {
        let metrics = TransportMetrics::new();

        metrics.add_bytes_received(2048);

        assert_eq!(metrics.snapshot().bytes_received, 2048);
    }

    #[test]
    fn test_record_errors() {
        let metrics = TransportMetrics::new();

        metrics.inc_errors_total();

        assert_eq!(metrics.snapshot().errors_total, 1);
    }

    #[test]
    fn test_record_retries() {
        let metrics = TransportMetrics::new();

        metrics.inc_retries_total();

        assert_eq!(metrics.snapshot().retries_total, 1);
    }

    #[test]
    fn test_record_timeouts() {
        let metrics = TransportMetrics::new();

        metrics.inc_timeouts_total();

        assert_eq!(metrics.snapshot().timeouts_total, 1);
    }

    #[test]
    fn test_connection_tracking() {
        let metrics = TransportMetrics::new();

        metrics.connection_opened();
        metrics.connection_opened();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.connections_opened, 2);
        assert_eq!(snapshot.active_connections, 2);

        metrics.connection_closed();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.connections_closed, 1);
        assert_eq!(snapshot.active_connections, 1);
    }

    #[test]
    fn test_health_check_metrics() {
        let metrics = TransportMetrics::new();

        metrics.inc_health_checks_total();
        metrics.inc_health_checks_failed();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.health_checks_total, 1);
        assert_eq!(snapshot.health_checks_failed, 1);
    }
}

mod protocol_tests {
    use super::*;

    #[test]
    fn test_frame_header_constants() {
        assert_eq!(FRAME_HEADER_SIZE, 24);
        assert_eq!(MAGIC, 0xCF5F0001);
        assert_eq!(PROTOCOL_VERSION, 1);
        assert_eq!(MAX_PAYLOAD_SIZE, 64 * 1024 * 1024);
    }

    #[test]
    fn test_frame_header_encode() {
        let header = FrameHeader::new(FrameFlags::default(), 0x0101, 12345, 100, 0xDEADBEEF);
        let encoded = header.encode();

        assert_eq!(encoded.len(), FRAME_HEADER_SIZE);
    }

    #[test]
    fn test_frame_header_decode() {
        let header = FrameHeader::new(FrameFlags::RESPONSE, 0x0101, 12345, 100, 0xDEADBEEF);
        let encoded = header.encode();

        let decoded = FrameHeader::decode(&encoded).unwrap();

        assert_eq!(decoded.magic, MAGIC);
        assert_eq!(decoded.version, PROTOCOL_VERSION);
        assert_eq!(decoded.request_id, 12345);
        assert_eq!(decoded.payload_length, 100);
    }

    #[test]
    fn test_frame_header_roundtrip() {
        let original = FrameHeader::new(
            FrameFlags::COMPRESSED | FrameFlags::ENCRYPTED,
            0x0202,
            0x123456789ABCDEF0,
            1024,
            0xABCD1234,
        );

        let encoded = original.encode();
        let decoded = FrameHeader::decode(&encoded).unwrap();

        assert_eq!(original.magic, decoded.magic);
        assert_eq!(original.version, decoded.version);
        assert_eq!(original.opcode, decoded.opcode);
        assert_eq!(original.request_id, decoded.request_id);
    }

    #[test]
    fn test_frame_header_decode_invalid_length() {
        let result = FrameHeader::decode(&[0u8; 10]);
        assert!(result.is_err());
    }

    #[test]
    fn test_frame_header_decode_invalid_magic() {
        let mut header = FrameHeader::new(FrameFlags::default(), 0x0101, 1, 0, 0);
        header.magic = 0x12345678;

        let encoded = header.encode();
        let result = FrameHeader::decode(&encoded);

        assert!(result.is_err());
    }

    #[test]
    fn test_frame_header_decode_invalid_version() {
        let mut header = FrameHeader::new(FrameFlags::default(), 0x0101, 1, 0, 0);
        header.version = 99;

        let encoded = header.encode();
        let result = FrameHeader::decode(&encoded);

        assert!(result.is_err());
    }

    #[test]
    fn test_opcode_variants() {
        assert_eq!(Opcode::Lookup.into_u16(), 0x0101);
        assert_eq!(Opcode::Create.into_u16(), 0x0102);
        assert_eq!(Opcode::Read.into_u16(), 0x0201);
        assert_eq!(Opcode::Write.into_u16(), 0x0202);
        assert_eq!(Opcode::Heartbeat.into_u16(), 0x0301);
        assert_eq!(Opcode::JournalSync.into_u16(), 0x0401);
    }

    #[test]
    fn test_opcode_from_u16() {
        assert_eq!(Opcode::from_u16(0x0101), Some(Opcode::Lookup));
        assert_eq!(Opcode::from_u16(0x0201), Some(Opcode::Read));
        assert_eq!(Opcode::from_u16(0xFFFF), None);
    }

    #[test]
    fn test_opcode_conversion_from_u16() {
        let opcode: Opcode = 0x0101.into();
        assert_eq!(opcode, Opcode::Lookup);
    }

    #[test]
    fn test_frame_new() {
        let payload = b"test payload".to_vec();
        let frame = Frame::new(Opcode::Read, 42, payload.clone());

        assert_eq!(frame.header.opcode, Opcode::Read as u16);
        assert_eq!(frame.header.request_id, 42);
        assert_eq!(frame.payload, payload);
    }

    #[test]
    fn test_frame_encode_decode_roundtrip() {
        let payload = b"This is a test payload".to_vec();
        let original = Frame::new(Opcode::Write, 999, payload);

        let encoded = original.encode();
        let decoded = Frame::decode(&encoded).unwrap();

        assert_eq!(decoded.header.opcode, original.header.opcode);
        assert_eq!(decoded.header.request_id, original.header.request_id);
        assert_eq!(decoded.payload, original.payload);
    }

    #[test]
    fn test_frame_validate() {
        let payload = b"valid payload".to_vec();
        let frame = Frame::new(Opcode::Read, 1, payload);

        assert!(frame.validate().is_ok());
    }

    #[test]
    fn test_frame_is_response() {
        let payload = b"test".to_vec();
        let frame = Frame::new(Opcode::Lookup, 1, payload);

        assert!(!frame.is_response());
    }

    #[test]
    fn test_frame_make_response() {
        let payload = b"request".to_vec();
        let frame = Frame::new(Opcode::Lookup, 1, payload);

        let response = frame.make_response(b"response".to_vec());

        assert!(response.is_response());
        assert_eq!(response.header.request_id, 1);
    }

    #[test]
    fn test_frame_flags() {
        let flags = FrameFlags::COMPRESSED;
        assert!(flags.contains(FrameFlags::COMPRESSED));
        assert!(!flags.contains(FrameFlags::ENCRYPTED));
    }

    #[test]
    fn test_frame_flags_combined() {
        let flags = FrameFlags::COMPRESSED | FrameFlags::ENCRYPTED;
        assert!(flags.contains(FrameFlags::COMPRESSED));
        assert!(flags.contains(FrameFlags::ENCRYPTED));
    }

    #[test]
    fn test_frame_flags_with() {
        let flags = FrameFlags::default().with(FrameFlags::ONE_WAY);
        assert!(flags.contains(FrameFlags::ONE_WAY));
    }

    #[test]
    fn test_frame_flags_without() {
        let flags = FrameFlags::COMPRESSED.without(FrameFlags::COMPRESSED);
        assert!(!flags.contains(FrameFlags::COMPRESSED));
    }
}
