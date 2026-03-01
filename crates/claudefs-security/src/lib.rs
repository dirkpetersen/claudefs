#![warn(missing_docs)]

//! ClaudeFS security audit crate: fuzzing harnesses, crypto property tests,
//! transport validation, and audit tooling.
//!
//! This crate is owned by A10 (Security Audit Agent) and provides:
//! - Protocol frame fuzzing (malformed frames, oversized payloads, invalid opcodes)
//! - Message deserialization fuzzing (unbounded strings, OOM vectors, type confusion)
//! - Cryptographic security property tests (nonce uniqueness, key isolation, timing)
//! - Transport validation tests (frame boundaries, checksum bypass, flag abuse)
//! - Audit report types for tracking findings

pub mod audit;
#[cfg(test)]
pub mod api_security_tests;
#[cfg(test)]
pub mod conduit_auth_tests;
#[cfg(test)]
pub mod crypto_tests;
pub mod fuzz_message;
pub mod fuzz_protocol;
#[cfg(test)]
pub mod gateway_auth_tests;
#[cfg(test)]
pub mod transport_tests;