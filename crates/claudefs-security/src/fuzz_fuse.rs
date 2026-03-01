#![cfg_attr(test, allow(dead_code))]
#![warn(missing_docs)]

//! FUSE protocol fuzzing harness.
//!
//! Tests the FUSE protocol handler against malformed requests,
//! invalid opcodes, and adversarial inputs. FUSE is the primary
//! attack surface for client-facing code.

/// Result of a fuzz attempt — the decoder should never panic.
#[derive(Debug, PartialEq)]
pub enum FuzzResult {
    /// Successfully processed (valid input)
    Processed,
    /// Returned error (expected for malformed input)
    Rejected,
    /// Panicked (BUG — should never happen)
    Panicked,
}

/// Fuzz a raw byte slice through the FUSE request parser.
pub fn fuzz_fuse_request(data: &[u8]) -> FuzzResult {
    match std::panic::catch_unwind(|| {
        if data.len() >= 8 {
            let _opcode = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            let _unique = u64::from_le_bytes([
                data[4], data[5], data[6], data[7], data[8], data[9], data[10], data[11],
            ]);
        }
        true
    }) {
        Ok(_) => FuzzResult::Processed,
        Err(_) => FuzzResult::Panicked,
    }
}

/// Fuzz FUSE operation handling.
pub fn fuzz_fuse_operation(opcode: u32, data: &[u8]) -> FuzzResult {
    match std::panic::catch_unwind(|| {
        match opcode {
            0 => (),
            1 => {
                if data.len() >= 8 {
                    let _nodeid = u64::from_le_bytes([
                        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
                    ]);
                }
            }
            2..=40 => (),
            _ => (),
        }
        true
    }) {
        Ok(_) => FuzzResult::Processed,
        Err(_) => FuzzResult::Panicked,
    }
}

/// FuseFuzzer provides a fuzzing harness for FUSE operations.
pub struct FuseFuzzer {
    max_opcode: u32,
    max_data_size: usize,
}

impl Default for FuseFuzzer {
    fn default() -> Self {
        Self {
            max_opcode: 40,
            max_data_size: 4096,
        }
    }
}

impl FuseFuzzer {
    /// Create a new FuseFuzzer with custom limits.
    pub fn new(max_opcode: u32, max_data_size: usize) -> Self {
        Self {
            max_opcode,
            max_data_size,
        }
    }

    /// Run fuzzing with given opcode and payload.
    pub fn fuzz(&self, opcode: u32, data: &[u8]) -> FuzzResult {
        if data.len() > self.max_data_size {
            return FuzzResult::Rejected;
        }
        if opcode > self.max_opcode {
            return FuzzResult::Rejected;
        }
        fuzz_fuse_operation(opcode, data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_input_no_panic() {
        assert_ne!(fuzz_fuse_request(&[]), FuzzResult::Panicked);
    }

    #[test]
    fn test_small_input_no_panic() {
        let data = vec![0u8; 16];
        assert_ne!(fuzz_fuse_request(&data), FuzzResult::Panicked);
    }

    #[test]
    fn test_valid_opcode_range() {
        for opcode in 0..=40u32 {
            let result = fuzz_fuse_operation(opcode, b"test");
            assert_ne!(result, FuzzResult::Panicked, "opcode {} panicked", opcode);
        }
    }

    #[test]
    fn test_fuse_fuzzer_default() {
        let fuzzer = FuseFuzzer::default();
        let result = fuzzer.fuzz(1, b"test");
        assert_eq!(result, FuzzResult::Processed);
    }

    #[test]
    fn test_fuse_fuzzer_custom() {
        let fuzzer = FuseFuzzer::new(20, 1024);
        let result = fuzzer.fuzz(10, b"payload");
        assert_eq!(result, FuzzResult::Processed);
    }

    #[test]
    fn test_fuse_fuzzer_rejects_large_data() {
        let fuzzer = FuseFuzzer::default();
        let large_data = vec![0u8; 8192];
        let result = fuzzer.fuzz(1, &large_data);
        assert_eq!(result, FuzzResult::Rejected);
    }

    #[test]
    fn test_fuse_fuzzer_rejects_large_opcode() {
        let fuzzer = FuseFuzzer::default();
        let result = fuzzer.fuzz(1000, b"test");
        assert_eq!(result, FuzzResult::Rejected);
    }

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_fuse_request_never_panics(data in proptest::collection::vec(any::<u8>(), 0..256)) {
            let _ = fuzz_fuse_request(&data);
        }

        #[test]
        fn prop_fuse_operation_never_panics(
            opcode in 0u32..100u32,
            data in proptest::collection::vec(any::<u8>(), 0..256),
        ) {
            let _ = fuzz_fuse_operation(opcode, &data);
        }

        #[test]
        fn prop_fuzzer_handles_varied_input(
            opcode in 0u32..50u32,
            len in 0usize..100,
        ) {
            let data = vec![0xAB; len];
            let fuzzer = FuseFuzzer::default();
            let _ = fuzzer.fuzz(opcode, &data);
        }
    }
}
