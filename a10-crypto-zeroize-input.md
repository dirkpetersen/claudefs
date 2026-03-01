# Task: Create crypto zeroize audit tests for claudefs-security

## Context
You are working on the `claudefs-security` crate at `crates/claudefs-security/src/crypto_zeroize_audit.rs`.
This is a NEW file for Phase 3 crypto audit.

## Requirements

Create a module that audits sensitive key material handling in the encryption and key management code.

### Security findings to encode:

1. **FINDING-CZ-01**: `EncryptionKey` implements `Clone` but not `Zeroize` — key material may persist in memory after drop
2. **FINDING-CZ-02**: `DataKey` implements `Clone` + `Serialize` but not `Zeroize` — DEK material not wiped
3. **FINDING-CZ-03**: `VersionedKey` implements `Clone` but not `Zeroize` — KEK material not wiped
4. **FINDING-CZ-04**: `KeyManager::kek_history` HashMap not zeroized on clear
5. **FINDING-CZ-05**: `EncryptionKey::Debug` correctly redacts — PASS
6. **FINDING-CZ-06**: `DataKey::Debug` correctly redacts — PASS
7. **FINDING-CZ-07**: `VersionedKey::Debug` correctly redacts — PASS
8. **FINDING-CZ-08**: `WormReducer::register()` allows overwriting existing retention policies — potential compliance weakness

### Test structure:

```rust
//! Phase 3 crypto zeroize and key material handling audit.
//!
//! Findings: FINDING-CZ-01 through FINDING-CZ-15
//!
//! Audits sensitive key material handling in claudefs-reduce encryption pipeline.
//! Verifies that key material debug output is redacted and documents
//! zeroize-on-drop gaps for remediation.

use claudefs_reduce::encryption::{EncryptionKey, Nonce, EncryptionAlgorithm, encrypt, decrypt, derive_chunk_key, random_nonce};
use claudefs_reduce::key_manager::{KeyManager, KeyManagerConfig, KeyVersion, DataKey, WrappedKey};
use claudefs_reduce::worm_reducer::{WormReducer, WormMode, RetentionPolicy};
```

### Tests to write:

1. FINDING-CZ-01: Test that EncryptionKey can be cloned (documents Clone without Zeroize)
2. FINDING-CZ-02: Test that DataKey can be cloned (documents Clone without Zeroize)
3. FINDING-CZ-03: Test that VersionedKey key material is accessible (gap: no Zeroize on drop)
4. FINDING-CZ-04: Test that KeyManager history clear doesn't zeroize old keys
5. FINDING-CZ-05: Verify EncryptionKey Debug output is "[REDACTED]"
6. FINDING-CZ-06: Verify DataKey Debug output is "[REDACTED]"
7. FINDING-CZ-07: Verify VersionedKey Debug output is "[REDACTED]"
8. FINDING-CZ-08: Test WORM policy overwrite behavior (compliance gap)
9. FINDING-CZ-09: Verify nonce generation doesn't leak key material
10. FINDING-CZ-10: Verify encrypted chunk doesn't contain plaintext
11. FINDING-CZ-11: Test that wrapped DEK is longer than raw DEK (includes auth tag)
12. FINDING-CZ-12: Verify key rotation preserves old key accessibility
13. FINDING-CZ-13: Test that encrypt with ChaCha20 doesn't expose key in ciphertext
14. FINDING-CZ-14: Verify HKDF info string is properly namespaced ("claudefs-chunk-key")
15. FINDING-CZ-15: Test that empty key (all zeros) still produces valid encryption

### Constraints:
- Write at least 15 tests
- Use `#[test]` functions (no async needed)
- For zeroize-gap findings (CZ-01 through CZ-04), write tests that pass but include comments documenting the finding for A3 to remediate
- All other tests should verify actual security properties

## Output
Output ONLY the complete Rust file content for `crates/claudefs-security/src/crypto_zeroize_audit.rs`.
Do not include markdown code fences or explanations.
