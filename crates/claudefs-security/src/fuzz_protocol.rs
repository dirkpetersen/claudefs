//! Protocol frame fuzzing harness.
//!
//! Tests the transport protocol's frame decoder against malformed, truncated,
//! and adversarial inputs. The frame format is the primary attack surface for
//! network-facing code.

#[allow(unused_imports)]
use claudefs_transport::protocol::{
    Frame, FrameHeader, FRAME_HEADER_SIZE, MAGIC, MAX_PAYLOAD_SIZE, PROTOCOL_VERSION,
};

/// Result of a fuzz attempt — the decoder should never panic.
#[derive(Debug, PartialEq)]
pub enum FuzzResult {
    /// Successfully decoded (valid input)
    Decoded,
    /// Returned error (expected for malformed input)
    Rejected,
    /// Panicked (BUG — should never happen)
    Panicked,
}

/// Fuzz a raw byte slice through the frame header decoder.
pub fn fuzz_frame_header(data: &[u8]) -> FuzzResult {
    match std::panic::catch_unwind(|| FrameHeader::decode(data)) {
        Ok(Ok(_)) => FuzzResult::Decoded,
        Ok(Err(_)) => FuzzResult::Rejected,
        Err(_) => FuzzResult::Panicked,
    }
}

/// Fuzz a raw byte slice through the full frame decoder.
pub fn fuzz_frame_decode(data: &[u8]) -> FuzzResult {
    match std::panic::catch_unwind(|| Frame::decode(data)) {
        Ok(Ok(_)) => FuzzResult::Decoded,
        Ok(Err(_)) => FuzzResult::Rejected,
        Err(_) => FuzzResult::Panicked,
    }
}

/// Build a valid frame header as bytes, then corrupt specific fields.
pub fn build_frame_bytes(
    magic: u32,
    version: u8,
    flags: u8,
    opcode: u16,
    request_id: u64,
    payload: &[u8],
    checksum: Option<u32>,
) -> Vec<u8> {
    let payload_len = payload.len() as u32;
    let crc = checksum.unwrap_or_else(|| claudefs_transport::protocol::crc32(payload));

    let mut buf = Vec::with_capacity(FRAME_HEADER_SIZE + payload.len());
    buf.extend_from_slice(&magic.to_be_bytes());
    buf.push(version);
    buf.push(flags);
    buf.extend_from_slice(&opcode.to_be_bytes());
    buf.extend_from_slice(&request_id.to_be_bytes());
    buf.extend_from_slice(&payload_len.to_be_bytes());
    buf.extend_from_slice(&crc.to_be_bytes());
    buf.extend_from_slice(payload);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_input_rejected() {
        assert_eq!(fuzz_frame_header(&[]), FuzzResult::Rejected);
    }

    #[test]
    fn test_short_input_rejected() {
        for len in 1..FRAME_HEADER_SIZE {
            let data = vec![0u8; len];
            assert_eq!(
                fuzz_frame_header(&data),
                FuzzResult::Rejected,
                "Header decoder should reject {len}-byte input"
            );
        }
    }

    #[test]
    fn test_wrong_magic_rejected() {
        let frame = build_frame_bytes(0xDEADBEEF, PROTOCOL_VERSION, 0, 0x0101, 1, b"test", None);
        assert_eq!(fuzz_frame_decode(&frame), FuzzResult::Rejected);
    }

    #[test]
    fn test_wrong_version_rejected() {
        let frame = build_frame_bytes(MAGIC, 99, 0, 0x0101, 1, b"test", None);
        assert_eq!(fuzz_frame_decode(&frame), FuzzResult::Rejected);
    }

    #[test]
    fn test_valid_frame_accepted() {
        let payload = b"hello world";
        let frame = build_frame_bytes(MAGIC, PROTOCOL_VERSION, 0, 0x0101, 1, payload, None);
        assert_eq!(fuzz_frame_decode(&frame), FuzzResult::Decoded);
    }

    #[test]
    fn test_corrupted_checksum_rejected() {
        let payload = b"hello world";
        let bad_crc = 0xDEADBEEF;
        let frame = build_frame_bytes(
            MAGIC,
            PROTOCOL_VERSION,
            0,
            0x0101,
            1,
            payload,
            Some(bad_crc),
        );
        assert_eq!(fuzz_frame_decode(&frame), FuzzResult::Rejected);
    }

    #[test]
    fn test_truncated_payload_rejected() {
        let mut frame = build_frame_bytes(MAGIC, PROTOCOL_VERSION, 0, 0x0101, 1, &[0u8; 10], None);
        let fake_len: u32 = 100;
        frame[16..20].copy_from_slice(&fake_len.to_be_bytes());
        assert_eq!(fuzz_frame_decode(&frame), FuzzResult::Rejected);
    }

    #[test]
    fn test_zero_length_payload_accepted() {
        let frame = build_frame_bytes(MAGIC, PROTOCOL_VERSION, 0, 0x0101, 1, &[], None);
        assert_eq!(fuzz_frame_decode(&frame), FuzzResult::Decoded);
    }

    #[test]
    fn test_max_flags_no_panic() {
        let frame = build_frame_bytes(MAGIC, PROTOCOL_VERSION, 0xFF, 0x0101, 1, b"test", None);
        let result = fuzz_frame_decode(&frame);
        assert_ne!(result, FuzzResult::Panicked);
    }

    #[test]
    fn test_max_request_id_no_panic() {
        let frame = build_frame_bytes(MAGIC, PROTOCOL_VERSION, 0, 0x0101, u64::MAX, b"test", None);
        let result = fuzz_frame_decode(&frame);
        assert_ne!(result, FuzzResult::Panicked);
    }

    #[test]
    fn test_unknown_opcode_no_panic() {
        let frame = build_frame_bytes(MAGIC, PROTOCOL_VERSION, 0, 0xFFFF, 1, b"test", None);
        let result = fuzz_frame_decode(&frame);
        assert_ne!(result, FuzzResult::Panicked);
    }

    #[test]
    fn test_all_zeros_rejected() {
        let data = vec![0u8; 256];
        assert_eq!(fuzz_frame_decode(&data), FuzzResult::Rejected);
    }

    #[test]
    fn test_all_ones_rejected() {
        let data = vec![0xFF; 256];
        assert_eq!(fuzz_frame_decode(&data), FuzzResult::Rejected);
    }

    #[test]
    fn test_payload_length_overflow_no_panic() {
        let mut frame = build_frame_bytes(MAGIC, PROTOCOL_VERSION, 0, 0x0101, 1, b"test", None);
        let max_len: u32 = u32::MAX;
        frame[16..20].copy_from_slice(&max_len.to_be_bytes());
        let result = fuzz_frame_decode(&frame);
        assert_ne!(
            result,
            FuzzResult::Panicked,
            "u32::MAX payload_length must not panic"
        );
    }

    #[test]
    fn test_payload_at_max_size_boundary() {
        let mut frame = build_frame_bytes(MAGIC, PROTOCOL_VERSION, 0, 0x0101, 1, b"test", None);
        let over_max: u32 = MAX_PAYLOAD_SIZE + 1;
        frame[16..20].copy_from_slice(&over_max.to_be_bytes());
        let result = fuzz_frame_decode(&frame);
        assert_ne!(result, FuzzResult::Panicked);
    }

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_header_decode_never_panics(data in proptest::collection::vec(any::<u8>(), 0..512)) {
            let _ = fuzz_frame_header(&data);
        }

        #[test]
        fn prop_frame_decode_never_panics(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
            let _ = fuzz_frame_decode(&data);
        }

        #[test]
        fn prop_valid_frame_roundtrip(
            opcode_raw in 0x0101u16..0x0110u16,
            request_id in any::<u64>(),
            payload in proptest::collection::vec(any::<u8>(), 0..1024),
        ) {
            let frame_bytes = build_frame_bytes(
                MAGIC, PROTOCOL_VERSION, 0, opcode_raw, request_id, &payload, None,
            );
            let result = fuzz_frame_decode(&frame_bytes);
            prop_assert_eq!(result, FuzzResult::Decoded);
        }

        #[test]
        fn prop_corrupted_magic_always_rejected(
            magic in any::<u32>().prop_filter("not real magic", |m| *m != MAGIC),
            payload in proptest::collection::vec(any::<u8>(), 0..64),
        ) {
            let frame_bytes = build_frame_bytes(
                magic, PROTOCOL_VERSION, 0, 0x0101, 1, &payload, None,
            );
            prop_assert_eq!(fuzz_frame_decode(&frame_bytes), FuzzResult::Rejected);
        }

        #[test]
        fn prop_corrupted_checksum_always_rejected(
            payload in proptest::collection::vec(any::<u8>(), 1..256),
            bad_crc in any::<u32>(),
        ) {
            let real_crc = claudefs_transport::protocol::crc32(&payload);
            if bad_crc != real_crc {
                let frame_bytes = build_frame_bytes(
                    MAGIC, PROTOCOL_VERSION, 0, 0x0101, 1, &payload, Some(bad_crc),
                );
                prop_assert_eq!(fuzz_frame_decode(&frame_bytes), FuzzResult::Rejected);
            }
        }
    }
}
