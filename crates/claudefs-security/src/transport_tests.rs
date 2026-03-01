//! Transport layer security tests.
//!
//! Tests transport components against adversarial conditions:
//! - Frame validation bypass attempts
//! - Checksum collision attacks
//! - Rate limiter exhaustion
//! - Circuit breaker manipulation
//! - Connection pool exhaustion
//! - TLS configuration validation

use claudefs_transport::circuitbreaker::{CircuitBreaker, CircuitBreakerConfig, CircuitState};
use claudefs_transport::loadshed::{LoadShedConfig, LoadShedder};
use claudefs_transport::protocol::{
    Frame, FrameFlags, FrameHeader, Opcode, FRAME_HEADER_SIZE, MAGIC, MAX_PAYLOAD_SIZE,
    PROTOCOL_VERSION,
};
use claudefs_transport::ratelimit::{RateLimitConfig, RateLimiter};

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_frame_with_mismatched_payload_length() {
        let payload = b"real payload data";
        let crc = claudefs_transport::protocol::crc32(payload);

        let mut header = FrameHeader {
            magic: MAGIC,
            version: PROTOCOL_VERSION,
            flags: FrameFlags::default(),
            opcode: 0x0101,
            request_id: 1,
            payload_length: payload.len() as u32 + 100,
            checksum: crc,
        };

        let frame = Frame {
            header,
            payload: payload.to_vec(),
        };

        assert!(
            frame.validate().is_err(),
            "Mismatched payload_length must be rejected"
        );
    }

    #[test]
    fn test_frame_checksum_zero() {
        let frame = Frame::new(Opcode::Lookup, 1, b"test".to_vec());
        assert_ne!(
            frame.header.checksum, 0,
            "CRC32 of non-empty payload should not be zero"
        );
    }

    #[test]
    fn test_frame_empty_payload_checksum() {
        let frame = Frame::new(Opcode::Lookup, 1, Vec::new());
        assert!(
            frame.validate().is_ok(),
            "Empty payload frame should validate"
        );
    }

    #[test]
    fn test_opcode_from_u16_unknown_returns_none() {
        for opcode in [0x0000u16, 0x0199, 0x0299, 0x0399, 0x0499, 0xFFFF] {
            assert!(
                Opcode::from_u16(opcode).is_none(),
                "Unknown opcode 0x{opcode:04x} should return None"
            );
        }
    }

    #[test]
    fn test_rate_limiter_burst_enforcement() {
        let config = RateLimitConfig::new(10, 5);
        let limiter = RateLimiter::new(config);

        let mut allowed = 0;
        for _ in 0..20 {
            if limiter.try_acquire() {
                allowed += 1;
            }
        }

        assert!(
            allowed <= 10,
            "Rate limiter allowed {allowed} requests, expected <= 10"
        );
        assert!(allowed >= 1, "Rate limiter should allow at least 1 request");
    }

    #[test]
    fn test_circuit_breaker_opens_on_failures() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            success_threshold: 2,
            open_duration: Duration::from_secs(60),
            half_open_max_requests: 1,
        };
        let cb = CircuitBreaker::new(config);

        for _ in 0..3 {
            cb.record_failure();
        }

        assert_eq!(
            cb.state(),
            CircuitState::Open,
            "Circuit breaker must open after failure_threshold failures"
        );
    }

    #[test]
    fn test_circuit_breaker_blocks_when_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            success_threshold: 1,
            open_duration: Duration::from_secs(3600),
            half_open_max_requests: 1,
        };
        let cb = CircuitBreaker::new(config);
        cb.record_failure();

        assert!(
            !cb.can_execute(),
            "Open circuit breaker must not allow requests"
        );
    }

    #[test]
    fn test_load_shedder_rejects_under_pressure() {
        let config = LoadShedConfig::new(100, 5, 90, 1.0, 1000, true);
        let shedder = LoadShedder::new(config);

        for _ in 0..10 {
            shedder.record_queue_depth(10);
        }
        shedder.record_cpu_usage(95);

        let total = 100;
        let mut admitted = 0;
        for _ in 0..total {
            if !shedder.should_shed() {
                admitted += 1;
            }
        }
        assert!(
            admitted < total,
            "Load shedder should reject some requests under load"
        );
    }

    #[test]
    fn test_all_flag_combinations() {
        for flags_raw in 0..=0x0F_u8 {
            let flags = FrameFlags::new(flags_raw);
            let frame = Frame {
                header: FrameHeader {
                    magic: MAGIC,
                    version: PROTOCOL_VERSION,
                    flags,
                    opcode: 0x0101,
                    request_id: 1,
                    payload_length: 4,
                    checksum: claudefs_transport::protocol::crc32(b"test"),
                },
                payload: b"test".to_vec(),
            };
            assert!(
                frame.validate().is_ok(),
                "Flag combination 0x{flags_raw:02x} should validate"
            );
        }
    }

    #[test]
    fn test_self_signed_ca_generation() {
        use claudefs_transport::tls::{generate_node_cert, generate_self_signed_ca};
        let (ca_cert, ca_key) = generate_self_signed_ca().unwrap();

        assert!(!ca_cert.is_empty(), "CA cert must not be empty");
        assert!(!ca_key.is_empty(), "CA key must not be empty");
        assert!(
            ca_cert.starts_with(b"-----BEGIN CERTIFICATE-----"),
            "CA cert must be PEM format"
        );
        assert!(
            ca_key.starts_with(b"-----BEGIN PRIVATE KEY-----"),
            "CA key must be PEM format"
        );
    }

    #[test]
    fn test_node_cert_generation() {
        use claudefs_transport::tls::{generate_node_cert, generate_self_signed_ca};
        let (ca_cert, ca_key) = generate_self_signed_ca().unwrap();
        let (node_cert, node_key) = generate_node_cert(&ca_cert, &ca_key, "test-node").unwrap();

        assert!(!node_cert.is_empty(), "Node cert must not be empty");
        assert!(!node_key.is_empty(), "Node key must not be empty");
        assert!(
            node_cert.starts_with(b"-----BEGIN CERTIFICATE-----"),
            "Node cert must be PEM format"
        );
    }

    #[test]
    fn test_tls_connector_with_valid_certs() {
        use claudefs_transport::tls::{
            generate_node_cert, generate_self_signed_ca, TlsConfig, TlsConnector,
        };
        let (ca_cert, ca_key) = generate_self_signed_ca().unwrap();
        let (node_cert, node_key) = generate_node_cert(&ca_cert, &ca_key, "client-node").unwrap();

        let config = TlsConfig::new(ca_cert, node_cert, node_key, true);

        let connector = TlsConnector::new(&config);
        assert!(connector.is_ok(), "TLS connector should accept valid certs");
    }

    #[test]
    fn test_tls_rejects_invalid_pem() {
        use claudefs_transport::tls::{TlsConfig, TlsConnector};
        let config = TlsConfig::new(
            b"not a real cert".to_vec(),
            b"also fake".to_vec(),
            b"nope".to_vec(),
            false,
        );

        let result = TlsConnector::new(&config);
        assert!(result.is_err(), "TLS connector should reject invalid PEM");
    }
}
