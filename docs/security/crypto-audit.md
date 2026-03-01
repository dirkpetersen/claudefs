# Cryptographic Implementation Audit Report

**Auditor:** A10 (Security Audit Agent)
**Date:** 2026-03-01
**Scope:** claudefs-reduce encryption subsystem
**Status:** Phase 2 initial audit

## Summary

| Component | Status | Risk |
|-----------|--------|------|
| AES-256-GCM implementation | SECURE | Low |
| ChaCha20-Poly1305 implementation | SECURE | Low |
| HKDF-SHA256 key derivation | SECURE | Low |
| Nonce generation | SECURE | Low |
| Envelope encryption (KEK/DEK) | SECURE | Low |
| Key rotation | SECURE | Medium |
| Memory zeroization | MISSING | **High** |
| Timing side channels | RESISTANT | Low |

**Overall risk: MEDIUM** — Cryptographic primitives are correctly used. Primary concern is missing memory zeroization of key material.

---

## 1. Algorithm Selection Review

### Approved Algorithms

| Algorithm | Purpose | Key Size | Standard |
|-----------|---------|----------|----------|
| AES-256-GCM | Data encryption (AEAD) | 256-bit | NIST SP 800-38D |
| ChaCha20-Poly1305 | Alternative AEAD | 256-bit | RFC 8439 |
| HKDF-SHA256 | Per-chunk key derivation | 256-bit | RFC 5869 |
| BLAKE3 | Content hashing (CAS) | 256-bit | — |

**Verdict:** All algorithms are industry-standard. AES-GCM and ChaCha20-Poly1305 are the two recommended AEAD ciphers for modern systems.

### Library Dependencies (RustCrypto)

| Crate | Version | Audit Status |
|-------|---------|-------------|
| `aes-gcm` | 0.10 | Well-maintained, constant-time on x86 (AES-NI) |
| `chacha20poly1305` | 0.10 | Constant-time, no hardware dependency |
| `hkdf` | 0.12 | Standard HKDF implementation |
| `sha2` | 0.10 | SHA-256 reference implementation |
| `rand` | 0.8 | ChaCha20-based CSPRNG, seeded from OS |

All from the RustCrypto project — trusted, widely used, and community-audited.

---

## 2. Nonce Generation

**File:** `crates/claudefs-reduce/src/encryption.rs`

```rust
pub fn random_nonce() -> Nonce {
    let mut bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut bytes);
    Nonce(bytes)
}
```

### Analysis

- **Size:** 12 bytes (96 bits) — correct for both AES-GCM and ChaCha20-Poly1305
- **Source:** `rand::thread_rng()` → ChaCha20-based CSPRNG seeded from OS entropy
- **Freshness:** New random nonce generated per `encrypt()` call

### Birthday Bound Risk

For 96-bit random nonces with AES-GCM, the birthday bound is ~2^48 encryptions per key before nonce collision probability exceeds 2^-32. With per-chunk key derivation (different key per chunk), this is effectively unlimited.

**Verdict:** SECURE — Nonce generation is correct and collision risk is negligible.

---

## 3. Key Derivation (HKDF)

**File:** `crates/claudefs-reduce/src/encryption.rs`

```rust
pub fn derive_chunk_key(master_key: &EncryptionKey, chunk_hash: &[u8; 32]) -> EncryptionKey {
    let hk = Hkdf::<Sha256>::new(None, &master_key.0);
    let mut okm = [0u8; 32];
    let mut info = Vec::with_capacity(18 + 32);
    info.extend_from_slice(b"claudefs-chunk-key");
    info.extend_from_slice(chunk_hash);
    hk.expand(&info, &mut okm).expect("HKDF expand failed");
    EncryptionKey(okm)
}
```

### Analysis

- **Salt:** None (acceptable when IKM is already high-entropy master key)
- **Info:** Domain separator `"claudefs-chunk-key"` + 32-byte chunk hash
- **Output:** 32 bytes (256 bits) — matches AES-256 key size

### Security Properties

- [x] Different chunks → different keys (verified by test)
- [x] Domain separation prevents cross-protocol key reuse
- [x] Deterministic: same chunk always derives same key (enables CAS dedup)
- [x] HKDF output is indistinguishable from random

**Verdict:** SECURE — HKDF usage follows RFC 5869 recommendations.

**Note:** Deterministic derivation means no forward secrecy at the chunk level. This is an intentional design trade-off for CAS-based deduplication.

---

## 4. Encryption / Decryption

**File:** `crates/claudefs-reduce/src/encryption.rs`

### Encrypt

- Fresh random nonce per call ✓
- AEAD cipher constructed from key ✓
- Ciphertext includes 16-byte auth tag (implicit in RustCrypto) ✓
- Nonce stored alongside ciphertext in `EncryptedChunk` ✓
- Errors propagated via `ReduceError` ✓

### Decrypt

- Auth tag verified automatically by AEAD ✓
- Tampered ciphertext → `DecryptionAuthFailed` error ✓
- Wrong key → `DecryptionAuthFailed` error ✓
- Error message is generic (no information leakage) ✓

**Verdict:** SECURE — Standard AEAD usage with proper error handling.

---

## 5. Envelope Encryption & Key Rotation

**File:** `crates/claudefs-reduce/src/key_manager.rs`

### Architecture

```
Master KEK (Key Encryption Key)
  └── wraps → DEK (Data Encryption Key) [AES-GCM encrypted]
                └── encrypts → chunk data
```

### Key Wrapping

- DEK generated with 32 random bytes ✓
- KEK wraps DEK using AES-256-GCM with fresh random nonce ✓
- KEK version stored with wrapped DEK for rotation ✓

### Key Rotation

- New KEK version increments monotonically ✓
- Old KEKs kept in history for decrypting old data ✓
- History pruned to `max_key_history` (default 10) ✓
- Re-wrapping: unwrap with old KEK, wrap with new KEK ✓

### Concerns

**C1: History pruning may orphan data.** If more than `max_key_history` rotations occur, oldest KEK versions are dropped. Data encrypted with those KEK versions becomes undecryptable.

**Mitigation:** Either increase `max_key_history` or add a migration phase that re-wraps all active data before pruning.

**C2: No key persistence.** KeyManager is in-memory only. After restart, all keys are lost unless externally persisted. This is likely handled by the metadata service (A2) but not visible in this crate.

**Verdict:** SECURE with caveats — Envelope encryption is correctly implemented. Key persistence and history management need integration-level verification.

---

## 6. Timing Side Channel Analysis

### Constant-Time Properties

| Operation | Constant-Time? | Mechanism |
|-----------|---------------|-----------|
| AES-256-GCM encrypt | Yes | AES-NI hardware on x86 |
| AES-256-GCM decrypt | Yes | AES-NI hardware on x86 |
| ChaCha20-Poly1305 | Yes | Inherently constant-time |
| HKDF-SHA256 | Yes | SHA-256 is constant-time |
| Auth tag comparison | Yes | RustCrypto uses `subtle` crate |
| Error handling | Safe | Generic error on all failures |

### Potential Leakage Points

1. **Plaintext length:** Ciphertext length reveals plaintext length (AEAD property). Mitigated by chunking (all chunks are similar size via FastCDC).

2. **Encryption algorithm selection:** The `EncryptionAlgorithm` enum is stored in clear in `EncryptedChunk`. Not a security issue — algorithm is not secret.

3. **Key derivation timing:** HKDF is constant-time regardless of input. No timing leak from chunk hash.

**Verdict:** RESISTANT to timing attacks. No actionable timing channels found.

---

## 7. Critical Findings

### FINDING-01: Missing Memory Zeroization (HIGH)

**Location:** `EncryptionKey`, `DataKey`, `VersionedKey` structs

**Problem:** Key material is not zeroed from memory when structs are dropped. Keys persist in freed memory and can be recovered via:
- Core dumps
- Memory forensics
- Swap file analysis
- Cold boot attacks

**Current state:**
```rust
#[derive(Clone)]
pub struct EncryptionKey(pub [u8; 32]);
// No Drop impl → key bytes remain in memory
```

**Required fix:**
```rust
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct EncryptionKey(pub [u8; 32]);
```

**Note:** `zeroize` is already a transitive dependency of RustCrypto crates. Adding it as a direct dependency has zero cost.

**Risk:** HIGH — Standard requirement for all key management implementations.

---

### FINDING-02: Uninitialized Memory in Zerocopy Allocator (HIGH)

**Location:** `crates/claudefs-transport/src/zerocopy.rs:150-155`

**Problem:** `std::alloc::alloc` returns uninitialized memory. The code creates a slice over this memory and calls `.to_vec()`, which reads uninitialized bytes. This is undefined behavior per the Rust reference.

**Required fix:** Use `std::alloc::alloc_zeroed` instead of `std::alloc::alloc`.

**Risk:** HIGH — Undefined behavior. May also leak sensitive data from previously freed memory.

---

### FINDING-03: Plaintext in EncryptedChunk Type (MEDIUM)

**Location:** `crates/claudefs-reduce/src/pipeline.rs`

**Problem:** When encryption is disabled, plaintext is stored in `EncryptedChunk.ciphertext` with a sentinel zero nonce. This conflates encrypted and unencrypted data in the same type.

**Risk:** MEDIUM — Downstream code may assume all `EncryptedChunk` values are encrypted. Could lead to plaintext being treated as ciphertext or vice versa.

**Recommendation:** Add an explicit `is_encrypted` flag or use a separate type for unencrypted chunks.

---

### FINDING-04: Key History Pruning Without Data Migration (MEDIUM)

**Location:** `crates/claudefs-reduce/src/key_manager.rs`

**Problem:** `rotate_key()` drops oldest KEK versions when history exceeds `max_key_history`. Data encrypted with dropped KEK versions becomes permanently undecryptable.

**Recommendation:** Add a `rewrap_all` phase before pruning, or emit a warning when pruning keys that may still protect active data.

---

## 8. Test Coverage Assessment

| Test | What It Verifies | Status |
|------|-----------------|--------|
| `prop_aesgcm_roundtrip` | Encrypt → decrypt correctness (property-based) | PASS |
| `prop_chacha_roundtrip` | ChaCha20 roundtrip correctness | PASS |
| `tampered_ciphertext_fails` | Auth tag rejection | PASS |
| `wrong_key_fails` | Key isolation | PASS |
| `hkdf_is_deterministic` | Key derivation consistency | PASS |
| `different_chunks_get_different_keys` | Key derivation uniqueness | PASS |
| `test_generate_dek_is_random` | DEK randomness | PASS |
| `test_wrap_unwrap_roundtrip` | Envelope encryption | PASS |
| `test_unwrap_with_wrong_version_fails` | Version check | PASS |
| `test_rotate_key_increments_version` | Version monotonicity | PASS |
| `test_rotate_key_keeps_history` | Key retention | PASS |
| `test_rewrap_dek` | Re-wrapping for rotation | PASS |
| `test_is_current_version` | Version detection | PASS |
| `test_history_pruning` | History limits | PASS |
| `test_no_key_returns_missing_key` | Error handling | PASS |

**Missing tests (recommended):**
- Nonce uniqueness across many encryptions
- Large payload encryption (>1MB)
- Concurrent encryption thread safety
- Key zeroization verification (once implemented)
- Boundary conditions for HKDF info length

---

## 9. Recommendations Summary

| # | Priority | Finding | Owner | Action |
|---|----------|---------|-------|--------|
| 1 | **HIGH** | Missing zeroization | A3 | Add `zeroize` derive to key types |
| 2 | **HIGH** | Uninitialized memory | A4 | Use `alloc_zeroed` in zerocopy.rs |
| 3 | **MEDIUM** | Plaintext in EncryptedChunk | A3 | Add `is_encrypted` flag or separate type |
| 4 | **MEDIUM** | Key history pruning | A3 | Add rewrap phase or warning |
| 5 | **LOW** | Missing security tests | A10 | Add nonce uniqueness / thread safety tests |

---

## Methodology

1. Read all encryption-related source files in claudefs-reduce
2. Traced data flow through the reduction pipeline (write path and read path)
3. Verified algorithm parameters against NIST/RFC standards
4. Analyzed timing properties of all crypto operations
5. Reviewed RustCrypto library versions for known vulnerabilities
6. Cross-referenced with OWASP Cryptographic Failures (A02:2021)
7. Checked test coverage for crypto operations

## Next Steps

- [ ] File GitHub issues for FINDING-01 through FINDING-04
- [ ] Implement additional security property tests in claudefs-security crate
- [ ] Re-audit after zeroization is added
- [ ] Phase 3: Fuzzing of encryption inputs, key rotation sequences
