# A10 Security Audit Report — Phase 1

**Auditor:** A10 (Security Audit Agent)
**Date:** 2026-03-03
**Scope:** Full codebase review — unsafe code, cryptographic implementations, authentication, dependency CVEs
**Codebase:** 210,593 lines of Rust across 10 crates

---

## Executive Summary

The ClaudeFS codebase has a strong security posture overall. Unsafe code is tightly confined to FFI boundaries (8 blocks total), cryptographic operations use well-regarded libraries (aes-gcm, chacha20poly1305, hkdf, rustls), and key material handling includes proper zeroization. However, this audit identified **2 critical**, **3 high**, and **4 medium** severity findings requiring remediation.

### Finding Summary

| ID | Severity | Category | Title | Status |
|----|----------|----------|-------|--------|
| SEC-01 | **CRITICAL** | Crypto | Hand-rolled SHA-256/HMAC-SHA256 in batch_auth.rs | **FIXED** |
| SEC-02 | **CRITICAL** | Crypto | BatchAuthKey zeroization vulnerable to compiler optimization | **FIXED** |
| SEC-03 | **HIGH** | AuthN | TlsAcceptor ignores require_client_auth — mTLS not enforced server-side | **FIXED** |
| SEC-04 | **HIGH** | Memory | File descriptor leak on lock poisoning in uring_engine.rs | **FIXED** |
| SEC-05 | **HIGH** | Memory | Unsafe Send/Sync impls lack safety documentation | Open |
| SEC-06 | **MEDIUM** | Crypto | HKDF uses no salt in key derivation | Open |
| SEC-07 | **MEDIUM** | Dependency | 4 dependency advisories (fuser unsound, lru unsound, bincode/rustls-pemfile unmaintained) | Open |
| SEC-08 | **MEDIUM** | Memory | TOCTOU race in zerocopy allocate_one with Relaxed ordering | Open |
| SEC-09 | **LOW** | Crypto | batch_auth.rs test uses unsafe to read freed memory (UB) | **FIXED** |

---

## Detailed Findings

### SEC-01: Hand-Rolled SHA-256/HMAC-SHA256 (CRITICAL)

**Location:** `crates/claudefs-repl/src/batch_auth.rs:144-251`

**Description:** The batch authentication module contains a complete hand-rolled implementation of SHA-256 (FIPS 180-4) and HMAC-SHA256 (RFC 2104) rather than using the `sha2` and `hmac` crates which are already workspace dependencies.

**Impact:** Custom cryptographic implementations are the #1 security anti-pattern. Even a single-bit error in the round constants, message schedule, or compression function creates a non-compliant hash that:
- May collide more easily than expected, allowing forgery
- May leak information about the key material via timing or output patterns
- Has not undergone the extensive analysis and testing of the `sha2` crate

**Evidence:** The implementation correctly uses the FIPS 180-4 round constants and appears functionally correct based on testing. However, it has NOT been verified against NIST test vectors (the existing tests only check output length, not correctness).

**Recommendation:** Replace with `sha2` and `hmac` crates. The `sha2` crate has been audited and is used by the entire Rust ecosystem including `rustls`.

**Status:** **FIXED** — Replaced with `sha2` + `hmac` crate usage.

---

### SEC-02: BatchAuthKey Zeroization Vulnerable to Compiler Optimization (CRITICAL)

**Location:** `crates/claudefs-repl/src/batch_auth.rs:33-39`

**Description:** The `BatchAuthKey::Drop` implementation uses a manual byte-zeroing loop:
```rust
impl Drop for BatchAuthKey {
    fn drop(&mut self) {
        for b in self.bytes.iter_mut() {
            *b = 0;
        }
    }
}
```

The Rust compiler (and LLVM) may optimize this away because the memory is about to be freed, so the writes are "dead stores" from the optimizer's perspective. The `zeroize` crate uses `volatile` writes specifically to prevent this optimization.

**Impact:** Key material may persist in memory after the `BatchAuthKey` is dropped, creating a window for memory disclosure attacks.

**Recommendation:** Use `zeroize::Zeroize` derive macro or `ZeroizeOnDrop`, which is already a workspace dependency.

**Status:** **FIXED** — Replaced with `Zeroize` + `ZeroizeOnDrop` derives.

---

### SEC-03: TlsAcceptor Ignores require_client_auth (HIGH)

**Location:** `crates/claudefs-transport/src/tls.rs:122-123`

**Description:** The `TlsAcceptor::new()` method always calls `.with_no_client_auth()` regardless of the `config.require_client_auth` flag:
```rust
let server_config = rustls::ServerConfig::builder()
    .with_no_client_auth()  // <-- ignores config.require_client_auth
    .with_single_cert(certs, key)
```

Per architecture decision D7, all inter-node communication must use mTLS. This bug means server-side certificate verification is never enforced, allowing any client to connect without presenting a valid certificate.

**Impact:** Clients can connect to storage/metadata servers without mTLS authentication. This bypasses the entire trust model described in D7.

**Recommendation:** When `require_client_auth` is true, load the CA certificates and use `.with_client_cert_verifier()` with a `WebPkiClientVerifier`.

**Status:** **FIXED** — TlsAcceptor now enforces client certificates when `require_client_auth` is true.

---

### SEC-04: File Descriptor Leak on Lock Poisoning (HIGH)

**Location:** `crates/claudefs-storage/src/uring_engine.rs:149-167`

**Description:** In `register_device()`, if `libc::open()` succeeds but `self.device_fds.write()` fails (lock poisoned), the file descriptor is leaked:
```rust
let fd = unsafe { libc::open(c_path.as_ptr(), flags, 0o644) };
if fd < 0 { /* handle error */ }
// If this fails, fd is leaked:
let mut fds = self.device_fds.write().map_err(|_| { ... })?;
```

**Impact:** Leaked file descriptors exhaust the process ulimit over time, eventually causing I/O failures across the entire storage engine.

**Recommendation:** Close the fd in the error path before returning the error.

**Status:** **FIXED** — Added fd cleanup in lock poisoning error path.

---

### SEC-05: Unsafe Send/Sync Impls Lack Safety Documentation (HIGH)

**Location:** `crates/claudefs-storage/src/uring_engine.rs:101-102`

**Description:**
```rust
unsafe impl Send for UringIoEngine {}
unsafe impl Sync for UringIoEngine {}
```

These `unsafe impl` blocks have no safety documentation explaining why the type is safe to send/share across threads. The struct contains `Mutex<IoUring>` (which provides interior locking) and `RwLock<HashMap<u16, RawFd>>` (which provides read-write locking), but the safety argument should be explicitly stated.

**Impact:** Without documented safety invariants, future modifications may inadvertently violate thread safety assumptions. `RawFd` is `Copy` and could be used unsafely if the locking discipline is changed.

**Recommendation:** Add `// SAFETY:` comments documenting why Send and Sync are safe for this type.

**Status:** Open — Documented for A1 remediation.

---

### SEC-06: HKDF Uses No Salt in Key Derivation (MEDIUM)

**Location:** `crates/claudefs-reduce/src/encryption.rs:48`

**Description:**
```rust
let hk = Hkdf::<Sha256>::new(None, &master_key.0);
```

HKDF is called without a salt. While HKDF is designed to work without a salt (RFC 5869 Section 2.2), using a per-cluster or per-context salt would strengthen key derivation against pre-computation attacks.

**Impact:** Without a salt, an attacker who knows the master key can pre-compute all possible derived keys for all possible chunk hashes. With a per-cluster salt, the attacker would need to repeat this for each cluster.

**Recommendation:** Use the cluster ID or a random per-cluster salt as the HKDF salt parameter. Store the salt alongside the cluster CA certificate.

**Status:** Open — Low-priority improvement for A3.

---

### SEC-07: Dependency Advisories (MEDIUM)

**Source:** `cargo audit` scan on 2026-03-03

| Crate | Version | Advisory | Severity | Impact |
|-------|---------|----------|----------|--------|
| `fuser` | 0.15.1 | RUSTSEC-2021-0154 | Unsound | Uninitialized memory read & leak in FUSE interface |
| `lru` | 0.12.5 | RUSTSEC-2026-0002 | Unsound | `IterMut` violates Stacked Borrows — potential UB |
| `bincode` | 1.3.3 | RUSTSEC-2025-0141 | Unmaintained | No security patches available |
| `rustls-pemfile` | 2.2.0 | RUSTSEC-2025-0134 | Unmaintained | No security patches available |

**Impact:**
- `fuser`: The FUSE client (A5) may read uninitialized memory, potentially leaking sensitive data to FUSE callers.
- `lru`: The FUSE client's LRU cache may trigger undefined behavior through mutable iteration.
- `bincode`/`rustls-pemfile`: No active maintainers means CVEs will not be patched.

**Recommendation:**
- `fuser`: Monitor for updated version or fork with fix. A5 should audit usage patterns.
- `lru`: Replace with `lru-rs` or a safe alternative. A5 should avoid `IterMut`.
- `bincode`: Evaluate migration to `bincode2`, `postcard`, or `bitcode`.
- `rustls-pemfile`: Functionality now integrated into `rustls` 0.24+; evaluate upgrade.

**Status:** Open — Filed for A5/A11 remediation.

---

### SEC-08: TOCTOU Race in Zerocopy allocate_one (MEDIUM)

**Location:** `crates/claudefs-transport/src/zerocopy.rs:126-139`

**Description:** The `allocate_one` method uses `compare_exchange` with `Relaxed` ordering:
```rust
let current = self.total_allocated.load(Ordering::Relaxed);
if current >= self.config.max_regions { return None; }
match self.total_allocated.compare_exchange(
    current, current + 1, Ordering::Relaxed, Ordering::Relaxed,
) { ... }
```

While the CAS prevents the counter from exceeding max_regions in most cases, `Relaxed` ordering provides no happens-before guarantees. On weakly-ordered architectures, this could theoretically allow over-allocation.

**Impact:** On x86 (which is the target platform), this is benign because x86 provides total store ordering. On ARM, this could cause one extra allocation beyond max_regions.

**Recommendation:** Use `Ordering::AcqRel` for the success case and `Ordering::Acquire` for the failure case to be correct on all architectures.

**Status:** Open — Low-priority for A4.

---

### SEC-09: Test Uses Unsafe to Read Freed Memory (LOW)

**Location:** `crates/claudefs-repl/src/batch_auth.rs:296-301`

**Description:** The `test_batch_key_zero_on_drop` test calls `std::mem::forget(key)` then reads the raw pointer, which is technically undefined behavior even in tests.

**Impact:** Only affects test code, no production impact.

**Status:** **FIXED** — Removed UB test.

---

## Positive Findings

The audit also identified several areas of strong security practice:

1. **Crypto library selection:** AES-GCM and ChaCha20-Poly1305 via the `aes-gcm` and `chacha20poly1305` crates are excellent choices. Both are AEAD ciphers providing confidentiality and integrity.

2. **Key material redaction:** All key types (`EncryptionKey`, `DataKey`, `VersionedKey`) implement `Debug` with `[REDACTED]` output, preventing accidental key leakage via logging.

3. **Envelope encryption:** The key management system properly implements envelope encryption with versioned KEKs, wrapped DEKs, key rotation, and history retention.

4. **TLS via rustls:** Using `rustls` (pure Rust, no OpenSSL) eliminates an entire class of C memory safety vulnerabilities.

5. **Constant-time comparison:** The `batch_auth.rs` module correctly uses constant-time comparison for HMAC verification.

6. **Zero-on-release:** The zerocopy `RegionPool` zeroes memory regions before returning them to the free list, preventing data leakage between users.

7. **Certificate generation:** The CA and node certificate generation uses `rcgen` properly with separate key pairs per node.

8. **Unsafe confinement:** Only 8 unsafe blocks in the entire codebase, all at FFI boundaries (io_uring, libc). No unsafe in crypto paths.

---

## Recommendations for Phase 2

1. **Fuzzing infrastructure:** Set up `cargo-fuzz` targets for the RPC protocol parser, FUSE message handler, and NFS gateway.
2. **NIST test vectors:** Validate all crypto implementations against official test vectors (AES-GCM: NIST SP 800-38D, ChaCha20: RFC 8439).
3. **CRL distribution:** The certificate revocation mechanism via SWIM gossip (D7) needs implementation and testing.
4. **Rate limiting:** The management API (A8) and enrollment endpoint (A4) need rate limiting to prevent brute-force attacks.
5. **Audit logging:** Security-relevant events (auth failures, key rotations, CRL updates) should be logged with structured tracing.

---

## Appendix: Files Reviewed

### Unsafe Code (Complete Review)
- `crates/claudefs-storage/src/uring_engine.rs` — 6 unsafe blocks (io_uring FFI)
- `crates/claudefs-transport/src/zerocopy.rs` — 1 unsafe block (aligned allocation)
- `crates/claudefs-repl/src/batch_auth.rs` — 1 unsafe block (test only, UB)

### Cryptographic Implementation (Complete Review)
- `crates/claudefs-reduce/src/encryption.rs` — AES-GCM, ChaCha20, HKDF
- `crates/claudefs-reduce/src/key_manager.rs` — Envelope encryption, key rotation
- `crates/claudefs-reduce/src/key_rotation_scheduler.rs` — Key lifecycle
- `crates/claudefs-repl/src/batch_auth.rs` — HMAC-SHA256 batch authentication
- `crates/claudefs-storage/src/integrity_chain.rs` — Hash chain integrity
- `crates/claudefs-storage/src/checksum.rs` — CRC32C/CRC64/xxHash64

### Authentication (Complete Review)
- `crates/claudefs-transport/src/tls.rs` — mTLS, CA generation, node certs
- `crates/claudefs-transport/src/tls_tcp.rs` — TLS over TCP
- `crates/claudefs-transport/src/conn_auth.rs` — Connection authentication
- `crates/claudefs-transport/src/enrollment.rs` — Client enrollment

### Dependency Audit
- `cargo audit` — 4 advisories (2 unsound, 2 unmaintained)
