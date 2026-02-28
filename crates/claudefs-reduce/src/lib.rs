#![warn(missing_docs)]

//! ClaudeFS reduction subsystem: Inline dedupe (BLAKE3), compression (LZ4/Zstd), encryption (AES-GCM)

pub mod compression;
pub mod dedupe;
pub mod encryption;
pub mod fingerprint;
pub mod pipeline;
