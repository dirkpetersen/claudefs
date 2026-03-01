# A10 Phase 4 Priority 2: Supply Chain Security Testing

## Objective

Implement comprehensive supply chain security tests in `crates/claudefs-security/src/supply_chain.rs` to audit:
1. Cryptographic dependency security (RustCrypto stack)
2. Serialization library robustness (bincode, serde)
3. Network library safety (tokio, tower)
4. Platform abstraction correctness (libc syscall bindings)
5. Build artifact reproducibility validation
6. Dependency CVE tracking and remediation

## Context

This is Phase 4 Priority 2 of the A10 Security Audit for ClaudeFS. Phase 3 successfully delivered 318 tests covering unsafe code, crypto zeroization, FUSE fuzzing, and API security. Phase 4 extends security coverage to supply chain risks.

**Project:** ClaudeFS — distributed POSIX file system in Rust
**Security Architecture:** Defense-in-depth with unsafe code isolated to FFI boundaries (A1 io_uring, A4 RDMA, A5 FUSE)

## Requirements

### 1. Module Structure

Create `supply_chain.rs` with the following test groups:

```rust
#[cfg(test)]
mod supply_chain {
    mod crypto_library_security {
        // RustCrypto stack audit
    }
    mod serialization_robustness {
        // bincode/serde edge cases
    }
    mod network_safety {
        // tokio/tower verification
    }
    mod platform_bindings {
        // libc syscall safety
    }
    mod dependency_tracking {
        // CVE registry and remediation
    }
    mod build_reproducibility {
        // Artifact verification
    }
}
```

### 2. Test Categories and Required Tests

#### 2.1 Cryptographic Library Security (15 tests)

**Test Group: `crypto_library_security`**

Tests should verify that RustCrypto dependencies are used correctly:
- `test_aes_gcm_nonce_reuse_prevention()` — Ensure AES-GCM never reuses nonces (critical vulnerability)
- `test_sha2_collision_resistance()` — Verify SHA-256 digest properties
- `test_hkdf_key_derivation_correctness()` — HKDF with salt/info parameters produces different keys
- `test_x509_certificate_validation()` — rustls cert validation with invalid chains
- `test_rsa_signature_verification()` — RSA-PSS signature correctness
- `test_pbkdf2_iteration_verification()` — PBKDF2 with very high iteration counts
- `test_random_number_generation_entropy()` — ChaCha20Rng entropy (min 7 bits per byte)
- `test_timing_side_channel_resistance()` — Constant-time comparison for auth secrets
- `test_memory_zeroization_coverage()` — Zeroize called on all key material (spot checks)
- `test_ecdsa_deterministic_signatures()` — ECDSA signature determinism (RFC 6979)
- `test_poly1305_mac_authentication()` — Poly1305 authentication correctness
- `test_chacha20_stream_cipher_properties()` — ChaCha20 stream properties
- `test_argon2_password_hashing_strong()` — Argon2id with strong parameters (time, memory, parallelism)
- `test_scrypt_derivation_parameters()` — Scrypt parameter validation
- `test_kdf_output_independence()` — KDF outputs independent for different contexts

**Severity & Implementation:**
- Use property-based tests (proptest) where possible
- Mock crypto backends where necessary to test error paths
- Document known CVEs and their remediation status

#### 2.2 Serialization Library Robustness (12 tests)

**Test Group: `serialization_robustness`**

Tests verify bincode and serde don't panic or cause OOM on malformed input:

- `test_bincode_oversized_collection_rejection()` — Reject collections >1GB (prevent OOM)
- `test_bincode_nested_struct_depth_limit()` — Max nesting depth (prevent stack overflow)
- `test_serde_unicode_normalization_safety()` — Unicode in strings doesn't panic
- `test_serde_type_mismatch_error_messages()` — Type confusion produces errors, not panics
- `test_bincode_integer_overflow_safety()` — u32 length fields don't overflow into usize
- `test_serde_borrowed_vs_owned_consistency()` — Borrowed lifetimes don't cause use-after-free
- `test_bincode_checksum_validation()` — Corrupted checksums detected
- `test_serde_default_value_handling()` — Missing fields use defaults correctly
- `test_serde_unknown_field_tolerance()` — Extra fields ignored or error appropriately
- `test_bincode_versioning_compatibility()` — Old/new format compatibility
- `test_serde_enum_discriminant_validation()` — Invalid enum discriminants rejected
- `test_serde_string_escape_sequence_safety()` — Escape sequences don't cause injection

**Severity & Implementation:**
- Fuzz against random byte inputs
- Set resource limits (stack size, heap) during testing
- Track RUSTSEC-2025-0141 (bincode message length overflow)

#### 2.3 Network Library Safety (10 tests)

**Test Group: `network_safety`**

Tests verify tokio and tower are used correctly for networking:

- `test_tokio_runtime_single_threaded_safety()` — Single-threaded runtime Send+Sync bounds
- `test_tokio_spawn_unbounded_task_queue_limits()` — Task queue has size limits
- `test_tower_service_timeout_enforcement()` — Timeout middleware actually times out
- `test_tower_rate_limit_correctness()` — Rate limiter doesn't allow burst >limit
- `test_tokio_buffer_overflow_protection()` — Large packets don't overflow buffers
- `test_tower_error_handling_no_panics()` — Service layer errors don't panic
- `test_tokio_connection_pool_exhaustion()` — Pool doesn't exhaust system file descriptors
- `test_tower_retry_loop_termination()` — Retry logic terminates (no infinite loops)
- `test_tokio_io_uring_integration_safety()` — io_uring submission ring doesn't overflow
- `test_tower_middleware_composition_correctness()` — Stacked middleware order correct

**Severity & Implementation:**
- Use timeout test harness to detect infinite loops
- Monitor OS resources (fd count) during tests
- Verify tower middleware composition doesn't reorder operations

#### 2.4 Platform Abstraction Correctness (8 tests)

**Test Group: `platform_bindings`**

Tests verify libc syscall bindings are used safely:

- `test_libc_file_descriptor_lifecycle()` — Opened fds are closed (no leaks)
- `test_libc_memory_alignment_requirements()` — Memory aligned for direct I/O
- `test_libc_signal_handler_safety()` — Signal handlers don't call async-unsafe functions
- `test_libc_errno_thread_local_correctness()` — errno not corrupted across threads
- `test_libc_io_uring_completion_queue_sync()` — CQ ring properly synchronized
- `test_libc_mmap_protection_bits_validation()` — PROT_* flags correct
- `test_libc_struct_layout_parity()` — C struct layouts match expectations (pod safety)
- `test_libc_constant_values_verification()` — Constants (O_NONBLOCK, etc.) correct

**Severity & Implementation:**
- Use zerocopy crate to verify memory layout assumptions
- Run under strace to validate syscall sequences
- Test on multiple architectures (x86_64, arm64) if possible

#### 2.5 Dependency CVE Tracking (20 tests)

**Test Group: `dependency_tracking`**

Tests track known CVEs and verify mitigations:

- `test_cve_rustsec_2025_0141_bincode_message_length()` — Bincode message length overflow detection
- `test_cve_rustsec_2025_0134_rustls_pemfile_parsing()` — PKCS#8 parsing edge case handling
- `test_cve_rustsec_2021_0154_fuser_protocol_handling()` — FUSE protocol robustness
- `test_cve_rustsec_2026_0002_lru_unsync_safety()` — LRU cache thread safety
- `test_cve_registry_versions_current()` — All transitive deps are at latest patch version
- `test_cve_dependency_audits_passing()` — `cargo audit` passes with no warnings
- `test_cve_cryptographic_libs_on_data_path()` — Only RustCrypto on critical path (not untrusted libs)
- `test_cve_network_isolation_data_path()` — Network crates not in data transform pipeline
- `test_cve_serialization_bounds_enforcement()` — Serde bounded by message size limits
- `test_cve_async_runtime_bounds()` — Tokio spawn bounded by queue size
- `test_cve_memory_exhaustion_protection()` — DoS attacks can't cause OOM
- `test_cve_stack_exhaustion_protection()` — Recursive input bounded
- `test_cve_library_update_compatibility()` — New versions compatible with current usage
- `test_cve_pinning_strategy_documentation()` — Dependencies pinned with rationale
- `test_cve_vulnerability_notification_integration()` — Tools integrated for real-time alerts
- `test_dev_dependencies_isolated()` — Dev deps not used in production code
- `test_optional_features_minimal()` — Unused optional features disabled
- `test_proc_macro_crates_sandboxed()` — Proc macros don't execute untrusted code
- `test_build_script_safety()` — build.rs scripts don't cause issues
- `test_license_compliance_checking()` — No GPL/AGPL in MIT binary (except Samba VFS)

**Severity & Implementation:**
- Integrate with cargo-audit for real-time CVE tracking
- Document remediation status for each CVE
- Maintain a VULNERABILITY_AUDIT.md file tracking findings
- Define SLA for security updates (critical: 24h, high: 1 week)

#### 2.6 Build Reproducibility Validation (8 tests)

**Test Group: `build_reproducibility`**

Tests verify CI/CD artifacts are deterministic:

- `test_cargo_lock_file_consistency()` — Lock file stable across builds
- `test_build_timestamp_independence()` — Binaries identical regardless of build time
- `test_build_path_independence()` — Binaries identical from different directories
- `test_compiler_flag_determinism()` — Debug info stripped consistently
- `test_artifact_hash_consistency()` — SHA-256 hash same for identical source/compiler
- `test_linker_reproducibility()` — Link order deterministic
- `test_dependency_version_locking()` — All deps pinned to exact versions
- `test_build_artifact_signing_verification()` — Release artifacts signed correctly

**Severity & Implementation:**
- Use SLSA provenance format for artifact metadata
- Document build environment reproducibility requirements
- Test on CI/CD to catch environment drift

### 3. Integration Requirements

**File Location:** `crates/claudefs-security/src/supply_chain.rs`

**Module Exports:**
- Export all test groups in `lib.rs` as public test modules
- Add to `mod supply_chain` in `lib.rs` module tree

**Dependencies:**
- Add to `crates/claudefs-security/Cargo.toml` if needed:
  - `proptest` (for property-based testing)
  - `rand` (for entropy testing)
  - `nix` (for libc bindings verification)
  - `zerocopy` (for memory layout verification)
  - Keep existing: tokio, serde, bincode, tower, etc.

**Test Isolation:**
- All tests must be runnable with `cargo test supply_chain::`
- No external dependencies (no actual network calls)
- All crypto tests use mocked/deterministic RNGs for reproducibility
- Resource-heavy tests wrapped in `#[ignore]` with documentation

### 4. Testing Requirements

- **All tests must pass** on Linux x86_64 (target platform)
- **Zero panics** on malformed input
- **No clippy warnings** in the module
- **Execution time:** <30 seconds total (supply_chain tests alone)
- **Documentation:** Each test has a doc comment explaining CVE/vulnerability it covers

### 5. Success Criteria

✅ 73 total tests implemented (15+12+10+8+20+8)
✅ All tests passing with 100% pass rate
✅ Zero clippy warnings
✅ Every critical vulnerability (CRITICAL/HIGH) has corresponding test
✅ Build artifact reproducibility verified
✅ Supply chain audit report generated with findings

## Output Format

Return complete Rust code for `supply_chain.rs` that:
1. Implements all 73 tests organized into 6 submodules
2. Includes proper error handling (use `thiserror` or `anyhow` as appropriate)
3. Has comprehensive doc comments on each test explaining the vulnerability
4. Uses property-based testing (proptest) where suitable
5. Passes all tests with `cargo test supply_chain::` (in claudefs-security crate)
6. Produces zero clippy warnings

## Additional Context

**Previous Phase 3 Deliverables:**
- 318 tests passing
- 50+ security findings tracked
- Unsafe code audit complete (A1/A4/A5)
- Crypto zeroization audit complete (A3)
- FUSE/RPC protocol fuzzing complete

**Phase 4 Roadmap:**
- Priority 1 ✅: DoS Resilience (27 tests, complete)
- Priority 2 (THIS): Supply Chain Security (73 tests)
- Priority 3: Operational Security (secrets, audit logs, compliance)
- Priority 4: Advanced Fuzzing (protocol expansion, crash consistency)

## References

- RUSTSEC-2025-0141: bincode message length overflow
- RUSTSEC-2025-0134: rustls-pemfile PKCS#8 parsing
- RUSTSEC-2021-0154: fuser FUSE protocol handling
- RUSTSEC-2026-0002: lru unsync impl
- RustCrypto: https://www.rust-crypto.org/
- SLSA: https://slsa.dev/ (artifact provenance)
