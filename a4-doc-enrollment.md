Add missing Rust doc comments to the enrollment.rs module for the claudefs-transport crate.

The crate has #![warn(missing_docs)] enabled. The file is MOSTLY documented already. The 19 missing_docs warnings are specifically for:

1. The variants of EnrollmentError enum (each variant needs a `///` doc comment)
2. The named fields within each EnrollmentError variant (each struct field needs a `///` doc comment)

Here are the items that need docs added:

EnrollmentError enum variants and their fields:
```rust
pub enum EnrollmentError {
    // Add: /// CA certificate or key generation failed.
    CaGenerationFailed {
        // Add: /// Description of what went wrong.
        reason: String
    },
    // Add: /// Failed to sign or issue a certificate.
    CertSigningFailed {
        // Add: /// Description of what went wrong.
        reason: String
    },
    // Add: /// The provided enrollment token is invalid.
    InvalidToken {
        // Add: /// Description of why the token is invalid.
        reason: String
    },
    // Add: /// The enrollment token has expired.
    TokenExpired {
        // Add: /// The expired token value.
        token: String
    },
    // Add: /// The enrollment token was already used.
    TokenAlreadyUsed {
        // Add: /// The already-used token value.
        token: String
    },
    // Add: /// The certificate has been revoked.
    CertificateRevoked {
        // Add: /// Certificate serial number that was revoked.
        serial: String
    },
    // Add: /// The certificate has expired.
    CertificateExpired {
        // Add: /// Certificate serial number that expired.
        serial: String
    },
    // Add: /// The certificate does not need renewal yet.
    RenewalNotNeeded {
        // Add: /// Certificate serial number that is still valid.
        serial: String
    },
    // Add: /// Maximum number of active tokens for this node has been reached.
    MaxTokensExceeded {
        // Add: /// Node identifier that exceeded the token limit.
        node_id: String,
        // Add: /// Maximum allowed number of tokens per node.
        max: usize
    },
}
```

Please output the COMPLETE enrollment.rs file with ONLY those doc comments added to EnrollmentError variants and their fields. Do not change anything else. Output ONLY the Rust source code, no markdown fences.

For reference, the enrollment.rs file starts with this header (use the actual file content):

//! Certificate enrollment for mTLS authentication per architecture decision D7.
