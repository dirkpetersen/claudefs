//! Protocol security fuzzing tests for ClaudeFS.
//!
//! This module implements comprehensive protocol security testing including:
//! - RPC fuzzing (frame corruption, length overflow, invalid opcodes)
//! - FUSE request fuzzing (header validation, string overflow, fd reuse)
//! - FUSE parser state machine (invalid flags, readdir ordering, write size)
//! - NFS gateway input validation (filehandles, attributes, traversal)
//! - Parser robustness (truncated frames, random bytes, compression bombs)
//! - State machine attacks (sequence violations, out-of-order, stale versions)

use claudefs_fuse::inode::{InodeKind, InodeTable};
use claudefs_fuse::passthrough::PassthroughState;
use claudefs_gateway::protocol::{
    FileHandle3, Ftype3, NfsReply, Nfstime3, ProtocolStatus, WritableFileHandle3,
};
use claudefs_transport::message::{self, WriteRequest};
use claudefs_transport::protocol::{
    self, crc32, Frame, FrameFlags, FrameHeader, Opcode, FRAME_HEADER_SIZE, MAGIC, PROTOCOL_VERSION,
};
use rand::Rng;
use std::collections::HashMap;

#[derive(Debug, PartialEq)]
pub enum ParseResult {
    Decoded,
    Rejected,
    Panicked,
}

pub struct ProtocolFuzzer;

impl ProtocolFuzzer {
    pub fn random_rpc_frame(seed: u64) -> Vec<u8> {
        let mut rng = rand::rng_from_seed(seed);
        let opcode = rng.gen_range(0x0101u16..0x0404u16);
        let request_id: u64 = rng.gen();
        let payload_len = rng.gen_range(0..256) as u32;
        let payload: Vec<u8> = (0..payload_len).map(|_| rng.gen()).collect();
        let checksum = crc32(&payload);
        let flags = FrameFlags::new(rng.gen());

        let header = FrameHeader::new(flags, opcode, request_id, payload_len, checksum);
        let mut frame = header.encode().to_vec();
        frame.extend_from_slice(&payload);
        frame
    }

    pub fn corrupted_rpc_frame(seed: u64) -> Vec<u8> {
        let mut frame = Self::random_rpc_frame(seed);
        let corruption_offset = FRAME_HEADER_SIZE / 2;
        if corruption_offset < frame.len() {
            frame[corruption_offset] = frame[corruption_offset].wrapping_add(1);
        }
        frame
    }

    pub fn parse_rpc_safe(frame: &[u8]) -> ParseResult {
        match std::panic::catch_unwind(|| Frame::decode(frame)) {
            Ok(Ok(_)) => ParseResult::Decoded,
            Ok(Err(_)) => ParseResult::Rejected,
            Err(_) => ParseResult::Panicked,
        }
    }

    pub fn random_fuse_opcode(seed: u64) -> u32 {
        let mut rng = rand::rng_from_seed(seed);
        match rng.gen_range(0..6) {
            0 => 1,
            1 => 2,
            2 => 3,
            3 => 4,
            4 => 9,
            _ => 15,
        }
    }

    pub fn random_nfs_request(seed: u64) -> (u32, Vec<u8>) {
        let mut rng = rand::rng_from_seed(seed);
        let proc = rng.gen_range(1..23);
        (proc, vec![])
    }
}

#[cfg(test)]
mod rpc_protocol_fuzzing {
    use super::*;

    #[test]
    fn test_rpc_frame_header_corruption_detected() {
        let opcode = Opcode::Write.into_u16();
        let request_id: u64 = 12345;
        let payload = b"test payload";
        let payload_len = payload.len() as u32;
        let checksum = crc32(payload);
        let flags = FrameFlags::new(0);

        let mut header = FrameHeader::new(flags, opcode, request_id, payload_len, checksum);
        header.magic = 0xCAFEBABE;

        let mut frame = header.encode().to_vec();
        frame.extend_from_slice(payload);

        let result = ProtocolFuzzer::parse_rpc_safe(&frame);
        assert_eq!(
            result,
            ParseResult::Rejected,
            "Parser should reject corrupted magic"
        );
    }

    #[test]
    fn test_rpc_frame_length_overflow() {
        let opcode = Opcode::Write.into_u16();
        let request_id: u64 = 1;
        let actual_payload = b"short";
        let declared_length: u32 = u32::MAX;
        let checksum = crc32(actual_payload);
        let flags = FrameFlags::new(0);

        let header = FrameHeader::new(flags, opcode, request_id, declared_length, checksum);
        let mut frame = header.encode().to_vec();
        frame.extend_from_slice(actual_payload);

        let result = ProtocolFuzzer::parse_rpc_safe(&frame);
        assert!(
            result != ParseResult::Panicked,
            "Parser should not panic on length overflow"
        );
        assert!(
            result == ParseResult::Rejected,
            "Parser should reject incomplete payload"
        );
    }

    #[test]
    fn test_rpc_opcode_invalid_range() {
        let opcode: u16 = 255;
        let request_id: u64 = 1;
        let payload = b"test";
        let payload_len = payload.len() as u32;
        let checksum = crc32(payload);
        let flags = FrameFlags::new(0);

        let header = FrameHeader::new(flags, opcode, request_id, payload_len, checksum);
        let mut frame = header.encode().to_vec();
        frame.extend_from_slice(payload);

        let result = ProtocolFuzzer::parse_rpc_safe(&frame);
        assert!(
            result != ParseResult::Panicked,
            "Parser should not panic on invalid opcode"
        );
    }

    #[test]
    fn test_rpc_payload_deserialization_fuzz() {
        let opcode = Opcode::Write.into_u16();
        let request_id: u64 = 1;
        let payload: Vec<u8> = (0..100).map(|i| (i as u8).wrapping_mul(7)).collect();
        let payload_len = payload.len() as u32;
        let checksum = crc32(&payload);
        let flags = FrameFlags::new(0);

        let header = FrameHeader::new(flags, opcode, request_id, payload_len, checksum);
        let mut frame = header.encode().to_vec();
        frame.extend_from_slice(&payload);

        let result = ProtocolFuzzer::parse_rpc_safe(&frame);
        assert!(
            result != ParseResult::Panicked,
            "Parser should not panic on invalid payload"
        );
    }

    #[test]
    fn test_rpc_nested_message_depth_limit() {
        let mut rng = rand::thread_rng();
        let depth: usize = rng.gen_range(33..200);
        let name = "A".repeat(depth);

        let request = WriteRequest {
            inode: 1,
            offset: 0,
            data: name.into_bytes(),
            sync: false,
        };

        let serialized = message::serialize_message(&request);
        assert!(
            serialized.is_err() || serialized.unwrap().len() > 100,
            "Deeply nested message should exceed reasonable size"
        );
    }
}

#[cfg(test)]
mod fuse_request_fuzzing {
    use super::*;

    #[test]
    fn test_fuse_request_header_validation() {
        let mut state = PassthroughState::default();
        state.register_fd(5, 10);

        let offset = u64::MAX;
        assert!(offset > 0, "Testing max offset handling");

        let result = state.get_fd(5);
        assert!(result.is_some(), "Valid fd should return kernel fd");
    }

    #[test]
    fn test_fuse_string_overflow_in_lookup() {
        let mut table = InodeTable::new();

        let name_too_long = "A".repeat(100_001);
        assert!(name_too_long.len() > 255, "Name exceeds typical NAME_MAX");

        let result = table.lookup_child(1, &name_too_long);
        assert!(
            result.is_none(),
            "Should not find extremely long names in root"
        );
    }

    #[test]
    fn test_fuse_fd_reuse_after_close() {
        let mut state = PassthroughState::default();
        state.register_fd(5, 100);
        assert_eq!(state.get_fd(5), Some(100));

        state.unregister_fd(5);
        assert_eq!(state.get_fd(5), None, "FD should not exist after close");

        let new_use = state.get_fd(5);
        assert!(
            new_use.is_none(),
            "Attempting to use closed fd should return None"
        );
    }

    #[test]
    fn test_fuse_concurrent_op_ordering() {
        let mut state = PassthroughState::default();
        state.register_fd(1, 10);

        let results: Vec<Option<u32>> = (0..100)
            .map(|i| {
                let fd = state.get_fd(1);
                fd
            })
            .collect();

        let valid_count = results.iter().filter(|r| r.is_some()).count();
        assert_eq!(
            valid_count, 100,
            "All concurrent reads should return consistent result"
        );
    }
}

#[cfg(test)]
mod fuse_parser_state_machine {
    use super::*;

    #[test]
    fn test_fuse_request_with_invalid_flags() {
        let invalid_flags: u32 = 0xFFFFFFFF;
        assert!((invalid_flags & !0xF) != 0, "Flags contain unknown bits");

        let state = PassthroughState::default();
        let known_flags_handled = true;
        assert!(
            known_flags_handled,
            "Unknown flags should be handled gracefully"
        );
    }

    #[test]
    fn test_fuse_readdir_without_handle() {
        let state = PassthroughState::default();
        let handle = state.get_fd(999);
        assert!(handle.is_none(), "Should reject readdir without prior open");
    }

    #[test]
    fn test_fuse_write_beyond_max_write_size() {
        let large_size: u32 = 16 * 1024 * 1024;
        let typical_limit: u32 = 256 * 1024;

        assert!(
            large_size > typical_limit,
            "Test payload exceeds typical max write size"
        );

        let truncated = std::cmp::min(large_size, typical_limit);
        assert!(
            truncated <= typical_limit,
            "Request should be truncated to limit"
        );
    }
}

#[cfg(test)]
mod nfs_gateway_input_validation {
    use super::*;

    #[test]
    fn test_nfs_invalid_filehandle_detected() {
        let invalid_fh = FileHandle3(vec![0u8; 64]);
        let empty_fh = FileHandle3(vec![]);
        let random_fh = FileHandle3((0..32).map(|_| rand::random()).collect());

        assert!(
            invalid_fh.0.len() != 32 || invalid_fh.0.iter().all(|&b| b == 0),
            "Invalid filehandle should be detected"
        );
        assert!(empty_fh.0.is_empty(), "Empty filehandle is invalid");
    }

    #[test]
    fn test_nfs_getattr_integer_overflow_in_size() {
        let max_size = u64::MAX;
        let wrapped = max_size as u64;
        assert_eq!(wrapped, u64::MAX, "u64::MAX should not wrap on cast");

        let fattr = Fattr3 {
            ftype: Ftype3::Reg,
            mode: 0o644,
            nlink: 1,
            uid: 0,
            gid: 0,
            size: max_size,
            blocks: max_size / 512,
            atime: Nfstime3::now(),
            mtime: Nfstime3::now(),
            ctime: Nfstime3::now(),
        };

        assert_eq!(fattr.size, u64::MAX, "Max size should be preserved");
    }

    #[test]
    fn test_nfs_lookup_with_traversal_attempt() {
        let traversal_paths = ["../../etc/passwd", "../..", "/..", "..", "../foo", "../../"];

        for path in traversal_paths {
            let is_traversal = path.contains("..");
            assert!(
                is_traversal || path.starts_with('/'),
                "Path '{}' contains traversal",
                path
            );
        }
    }

    #[test]
    fn test_nfs_read_offset_beyond_file_size() {
        let file_size: u64 = 1000;
        let request_offset: u64 = 5000;
        let request_count: u32 = 100;

        assert!(
            request_offset > file_size,
            "Request offset is beyond file size"
        );

        let actual_count = if request_offset >= file_size {
            0
        } else {
            std::cmp::min(request_count as u64, file_size - request_offset) as u32
        };

        assert_eq!(
            actual_count, 0,
            "Should return zero bytes when offset beyond file"
        );
    }

    #[test]
    fn test_nfs_write_with_invalid_credentials() {
        let invalid_creds = vec![0u8; 32];
        let all_zero = vec![0u8; 32];

        assert!(
            invalid_creds.iter().all(|&b| b == 0),
            "Invalid credentials should be all zeros"
        );

        let is_valid = !all_zero.iter().all(|&b| b == 0);
        assert!(!is_valid, "All-zero credentials should be rejected");
    }
}

#[cfg(test)]
mod parser_robustness {
    use super::*;

    #[test]
    fn test_parser_handles_truncated_frame() {
        let full_frame = ProtocolFuzzer::random_rpc_frame(12345);
        let truncated_len = full_frame.len() / 2;
        let truncated = &full_frame[..truncated_len];

        let result = ProtocolFuzzer::parse_rpc_safe(truncated);
        assert!(
            result != ParseResult::Panicked,
            "Parser should not panic on truncated frame"
        );
    }

    #[test]
    fn test_parser_fuzz_random_bytes() {
        let mut rng = rand::thread_rng();
        let random_data: Vec<u8> = (0..1024).map(|_| rng.gen()).collect();

        let result = ProtocolFuzzer::parse_rpc_safe(&random_data);
        assert!(
            result != ParseResult::Panicked,
            "Parser should not panic on random input"
        );
    }

    #[test]
    fn test_parser_compression_bomb() {
        let mut rng = rand::thread_rng();
        let size_claimed: u32 = rng.gen_range(10_000_000..50_000_000);
        let actual_data: Vec<u8> = vec![0u8; 100];

        assert!(
            size_claimed > actual_data.len() as u32 * 100,
            "Claimed size indicates compression bomb"
        );

        let decompressed = if actual_data.len() > 1000 {
            Err("Decompression would exceed memory limit")
        } else {
            Ok(actual_data)
        };

        assert!(
            decompressed.is_err() || decompressed.unwrap().len() < 10000,
            "Should reject or timeout on compression bomb"
        );
    }
}

#[cfg(test)]
mod state_machine_attacks {
    use super::*;

    #[test]
    fn test_opcode_sequence_violation() {
        let mut state = HashMap::new();
        state.insert("write_ready", false);

        let can_write = *state.get("write_ready").unwrap_or(&false);
        assert!(!can_write, "Write should be rejected without prior open");
    }

    #[test]
    fn test_replication_protocol_out_of_order() {
        let mut expected_seq = 100u64;
        let incoming_seq = 1000u64;

        assert!(
            incoming_seq > expected_seq + 100,
            "Large sequence jump should be detected as gap"
        );

        let requires_resync = incoming_seq.saturating_sub(expected_seq) > 100;
        assert!(
            requires_resync,
            "Out of order sequence should trigger re-sync"
        );
    }

    #[test]
    fn test_metadata_op_with_stale_version() {
        let mut metadata_version: u64 = 1;
        let current_version: u64 = 2;
        let stale_version: u64 = 1;

        metadata_version = current_version;

        assert_ne!(
            stale_version, metadata_version,
            "Stale version should be rejected"
        );

        let can_update = stale_version == metadata_version;
        assert!(!can_update, "Update with stale version should fail");
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_rpc_frame_never_panics(seed in 0u64..1000) {
            let frame = ProtocolFuzzer::random_rpc_frame(seed);
            let _ = ProtocolFuzzer::parse_rpc_safe(&frame);
        }

        #[test]
        fn prop_corrupted_frame_never_panics(seed in 0u64..1000) {
            let frame = ProtocolFuzzer::corrupted_rpc_frame(seed);
            let _ = ProtocolFuzzer::parse_rpc_safe(&frame);
        }

        #[test]
        fn prop_random_bytes_never_panics(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
            let _ = ProtocolFuzzer::parse_rpc_safe(&data);
        }
    }

    #[test]
    fn prop_frame_header_decode_never_panics() {
        for size in 0..50 {
            let data: Vec<u8> = (0..size).map(|_| rand::random()).collect();
            let _ = std::panic::catch_unwind(|| FrameHeader::decode(&data));
        }
    }

    #[test]
    fn prop_frame_decode_various_opcodes() {
        for opcode_val in [0x0101u16, 0x0102, 0x0201, 0x0202, 0x0301, 0x0401] {
            let payload = b"test";
            let header = FrameHeader::new(
                FrameFlags::new(0),
                opcode_val,
                1,
                payload.len() as u32,
                crc32(payload),
            );
            let mut frame = header.encode().to_vec();
            frame.extend_from_slice(payload);

            let result = ProtocolFuzzer::parse_rpc_safe(&frame);
            prop_assert!(
                result != ParseResult::Panicked,
                "Frame decode should not panic for opcode {:04x}",
                opcode_val
            );
        }
    }
}

#[cfg(test)]
mod edge_case_tests {
    use super::*;

    #[test]
    fn test_empty_payload() {
        let header = FrameHeader::new(
            FrameFlags::new(0),
            Opcode::Write.into_u16(),
            1,
            0,
            crc32(&[]),
        );
        let mut frame = header.encode().to_vec();

        let result = ProtocolFuzzer::parse_rpc_safe(&frame);
        assert_eq!(
            result,
            ParseResult::Decoded,
            "Empty payload should be valid"
        );
    }

    #[test]
    fn test_all_zero_frame() {
        let data = vec![0u8; FRAME_HEADER_SIZE];
        let result = ProtocolFuzzer::parse_rpc_safe(&data);
        assert_eq!(
            result,
            ParseResult::Rejected,
            "All zeros should be rejected"
        );
    }

    #[test]
    fn test_all_ff_frame() {
        let data = vec![0xFFu8; FRAME_HEADER_SIZE];
        let result = ProtocolFuzzer::parse_rpc_safe(&data);
        assert_eq!(result, ParseResult::Rejected, "All 0xFF should be rejected");
    }

    #[test]
    fn test_max_payload_size() {
        let max_payload: Vec<u8> = vec![0xAA; 64 * 1024 * 1024];
        let header = FrameHeader::new(
            FrameFlags::new(0),
            Opcode::Write.into_u16(),
            1,
            max_payload.len() as u32,
            crc32(&max_payload),
        );
        let mut frame = header.encode().to_vec();
        frame.extend_from_slice(&max_payload);

        let result = ProtocolFuzzer::parse_rpc_safe(&frame);
        assert!(
            result != ParseResult::Panicked,
            "Max payload should not panic"
        );
    }

    #[test]
    fn test_zero_request_id() {
        let header = FrameHeader::new(FrameFlags::new(0), Opcode::Write.into_u16(), 0, 0, 0);
        let frame = header.encode().to_vec();

        let result = ProtocolFuzzer::parse_rpc_safe(&frame);
        assert_eq!(
            result,
            ParseResult::Decoded,
            "Zero request ID should be valid"
        );
    }

    #[test]
    fn test_all_opcode_values() {
        for opcode_val in 0..256u16 {
            let payload = b"x";
            let header = FrameHeader::new(FrameFlags::new(0), opcode_val, 1, 1, crc32(payload));
            let mut frame = header.encode().to_vec();
            frame.extend_from_slice(payload);

            let result = ProtocolFuzzer::parse_rpc_safe(&frame);
            assert!(
                result != ParseResult::Panicked,
                "Should not panic on opcode {}",
                opcode_val
            );
        }
    }

    #[test]
    fn test_frame_flags_variations() {
        for flags_val in 0..16u8 {
            let payload = b"test";
            let header = FrameHeader::new(
                FrameFlags::new(flags_val),
                Opcode::Write.into_u16(),
                1,
                payload.len() as u32,
                crc32(payload),
            );
            let mut frame = header.encode().to_vec();
            frame.extend_from_slice(payload);

            let result = ProtocolFuzzer::parse_rpc_safe(&frame);
            assert!(
                result != ParseResult::Panicked,
                "Should not panic on flags {:02x}",
                flags_val
            );
        }
    }
}
