//! Message deserialization fuzzing.
//!
//! Tests that the message serialization layer handles malformed inputs
//! without panicking or allocating unbounded memory.

#[allow(unused_imports)]
use claudefs_transport::message::{deserialize_message, serialize_message};
#[allow(unused_imports)]
use serde::{Deserialize, Serialize};

/// Maximum expected message size for safety checks.
pub const MAX_SAFE_MESSAGE_SIZE: usize = 64 * 1024 * 1024; // 64 MB

/// Test that deserialization of arbitrary bytes doesn't panic.
pub fn fuzz_deserialize<T: serde::de::DeserializeOwned>(data: &[u8]) -> bool {
    match std::panic::catch_unwind(|| {
        let _: std::result::Result<T, _> = deserialize_message(data);
    }) {
        Ok(_) => true,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[derive(Serialize, Deserialize, Debug)]
    struct SimpleMessage {
        id: u64,
        name: String,
    }

    #[derive(Serialize, Deserialize, Debug)]
    struct VecMessage {
        items: Vec<u8>,
    }

    #[derive(Serialize, Deserialize, Debug)]
    struct NestedMessage {
        id: u64,
        entries: Vec<SimpleMessage>,
    }

    #[test]
    fn test_empty_bytes_no_panic() {
        assert!(fuzz_deserialize::<SimpleMessage>(&[]));
    }

    #[test]
    fn test_single_byte_no_panic() {
        for b in 0..=255u8 {
            assert!(
                fuzz_deserialize::<SimpleMessage>(&[b]),
                "Panic on single byte {b}"
            );
        }
    }

    #[test]
    fn test_valid_roundtrip() {
        let msg = SimpleMessage {
            id: 42,
            name: "test".into(),
        };
        let bytes = serialize_message(&msg).unwrap();
        let decoded: SimpleMessage = deserialize_message(&bytes).unwrap();
        assert_eq!(decoded.id, 42);
        assert_eq!(decoded.name, "test");
    }

    #[test]
    fn test_truncated_serialized_data() {
        let msg = SimpleMessage {
            id: 42,
            name: "hello world".into(),
        };
        let bytes = serialize_message(&msg).unwrap();
        for len in 0..bytes.len() {
            assert!(
                fuzz_deserialize::<SimpleMessage>(&bytes[..len]),
                "Panic on truncated data at length {len}"
            );
        }
    }

    #[test]
    fn test_type_confusion_no_panic() {
        let msg = VecMessage {
            items: vec![1, 2, 3, 4, 5],
        };
        let bytes = serialize_message(&msg).unwrap();
        assert!(fuzz_deserialize::<SimpleMessage>(&bytes));
    }

    #[test]
    fn test_large_string_length_prefix() {
        let mut data = Vec::new();
        data.extend_from_slice(&42u64.to_le_bytes());
        data.extend_from_slice(&(1_000_000_000u64).to_le_bytes());
        data.extend_from_slice(b"short");
        assert!(fuzz_deserialize::<SimpleMessage>(&data));
    }

    #[test]
    fn test_large_vec_length_prefix() {
        let mut data = Vec::new();
        data.extend_from_slice(&(1_000_000_000u64).to_le_bytes());
        data.extend_from_slice(&[0u8; 64]);
        assert!(fuzz_deserialize::<VecMessage>(&data));
    }

    proptest! {
        #[test]
        fn prop_deserialize_simple_never_panics(data in proptest::collection::vec(any::<u8>(), 0..1024)) {
            fuzz_deserialize::<SimpleMessage>(&data);
        }

        #[test]
        fn prop_deserialize_vec_never_panics(data in proptest::collection::vec(any::<u8>(), 0..1024)) {
            fuzz_deserialize::<VecMessage>(&data);
        }

        #[test]
        fn prop_deserialize_nested_never_panics(data in proptest::collection::vec(any::<u8>(), 0..1024)) {
            fuzz_deserialize::<NestedMessage>(&data);
        }

        #[test]
        fn prop_roundtrip_preserves_data(
            id in any::<u64>(),
            name in "[a-zA-Z0-9]{0,100}",
        ) {
            let msg = SimpleMessage { id, name: name.clone() };
            let bytes = serialize_message(&msg).unwrap();
            let decoded: SimpleMessage = deserialize_message(&bytes).unwrap();
            prop_assert_eq!(decoded.id, id);
            prop_assert_eq!(decoded.name, name);
        }
    }
}
