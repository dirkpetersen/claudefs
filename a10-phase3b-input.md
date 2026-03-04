# A10 Phase 3b: Security Tests for FUSE and Transport Crates

## Task

Add two new test modules to the `claudefs-security` crate:
1. `fuse_security_tests.rs` — Security tests for the `claudefs-fuse` crate
2. `transport_security_tests.rs` — Security tests for the `claudefs-transport` crate

Both files go in `crates/claudefs-security/src/`.

Also update `crates/claudefs-security/src/lib.rs` to register both new modules.

## Important Constraints

- All tests must compile and pass with `cargo test -p claudefs-security`
- Use only public APIs from `claudefs-fuse` and `claudefs-transport`
- Tests should document security findings (use eprintln for findings detected)
- Use `#[cfg(test)]` module wrapping
- No unsafe code
- When testing for input validation gaps, DON'T assert failure — instead detect whether the issue exists and log it

## Current lib.rs (last 6 lines to show where to add)

The file ends with:
```rust
#[cfg(test)]
pub mod meta_security_tests;
#[cfg(test)]
pub mod gateway_security_tests;
```

Add after these:
```rust
#[cfg(test)]
pub mod fuse_security_tests;
#[cfg(test)]
pub mod transport_security_tests;
```

## Dependencies already available in Cargo.toml

```toml
[dependencies]
claudefs-transport = { path = "../claudefs-transport" }
claudefs-fuse = { path = "../claudefs-fuse" }
# ... others
```

## Public API Reference

### claudefs-fuse::client_auth

```rust
pub struct ClientAuthManager { ... }
impl ClientAuthManager {
    pub fn new(cert_dir: &str) -> Self;
    pub fn state(&self) -> &AuthState;
    pub fn cert(&self) -> Option<&CertRecord>;
    pub fn begin_enrollment(&mut self, token: &str, now_secs: u64) -> Result<()>;
    pub fn complete_enrollment(&mut self, cert_pem: &str, key_pem: &str, now_secs: u64) -> Result<()>;
    pub fn needs_renewal(&self, now_secs: u64, renew_before_secs: u64) -> bool;
    pub fn begin_renewal(&mut self, now_secs: u64) -> Result<()>;
    pub fn complete_renewal(&mut self, cert_pem: &str, key_pem: &str, now_secs: u64) -> Result<()>;
    pub fn revoke(&mut self, reason: &str, now_secs: u64);
    pub fn add_to_crl(&mut self, fingerprint: [u8; 32], reason: &str, revoked_at_secs: u64);
    pub fn is_revoked(&self, fingerprint: &[u8; 32]) -> bool;
    pub fn crl_len(&self) -> usize;
    pub fn compact_crl(&mut self, now_secs: u64, max_age_secs: u64) -> usize;
}

pub enum AuthState {
    Unenrolled,
    Enrolling { token: String, started_at_secs: u64 },
    Enrolled { cert_fingerprint: [u8; 32], expires_at_secs: u64 },
    Renewing { old_fingerprint: [u8; 32], started_at_secs: u64 },
    Revoked { reason: String, revoked_at_secs: u64 },
}

pub struct CertRecord {
    pub fingerprint: [u8; 32],
    pub subject: String,
    pub issued_at_secs: u64,
    pub expires_at_secs: u64,
    pub cert_pem: String,
    pub key_pem: String,
}
impl CertRecord {
    pub fn is_expired(&self, now_secs: u64) -> bool;
    pub fn needs_renewal(&self, now_secs: u64, renew_before_secs: u64) -> bool;
    pub fn days_until_expiry(&self, now_secs: u64) -> i64;
}
```

### claudefs-fuse::path_resolver

```rust
pub struct PathResolver { ... }
impl PathResolver {
    pub fn new(config: PathResolverConfig) -> Self;
    pub fn insert(&self, path: &str, resolved: ResolvedPath);
    pub fn lookup(&self, path: &str) -> Option<ResolvedPath>;
    pub fn validate_path(path: &str) -> PathResolveResult<Vec<&str>>;
    pub fn invalidate_prefix(&self, path_prefix: &str);
    pub fn bump_generation(&self, ino: InodeId) -> u64;
    pub fn is_generation_current(&self, ino: InodeId, gen: u64) -> bool;
    pub fn stats(&self) -> &PathResolverStats;
}

pub struct PathResolverConfig {
    pub max_depth: usize,
    pub cache_capacity: usize,
    pub ttl: Duration,
}
impl Default for PathResolverConfig { fn default() -> Self; }

pub enum PathResolveError {
    ComponentNotFound { name: String, parent: InodeId },
    TooDeep { depth: usize, limit: usize },
    Stale { name: String },
    InvalidPath { reason: String },
}
```

### claudefs-fuse::mount

```rust
pub struct MountOptions {
    pub allow_other: bool,
    pub allow_root: bool,
    pub default_permissions: bool,
    pub auto_unmount: bool,
    pub direct_io: bool,
    pub kernel_cache: bool,
    pub nonempty: bool,
    pub ro: bool,
}
impl Default for MountOptions { fn default() -> Self; }

pub fn parse_mount_options(opts_str: &str) -> Result<MountOptions, MountError>;

pub enum MountError {
    PathNotFound(String),
    NotADirectory(String),
    AlreadyMounted(String),
    PermissionDenied(String),
    InvalidOption(String),
    IoError(String),
}
```

### claudefs-fuse::passthrough

```rust
pub struct PassthroughState { ... }
impl PassthroughState {
    pub fn new(config: &PassthroughConfig) -> Self;
    pub fn is_active(&self) -> bool;
    pub fn register_fd(&mut self, fh: u64, fd: i32);
    pub fn unregister_fd(&mut self, fh: u64) -> Option<i32>;
    pub fn get_fd(&self, fh: u64) -> Option<i32>;
    pub fn fd_count(&self) -> usize;
}

pub struct PassthroughConfig {
    pub enabled: bool,
    pub min_kernel_major: u32,
    pub min_kernel_minor: u32,
}
impl Default for PassthroughConfig { fn default() -> Self; }

pub fn check_kernel_version(major: u32, minor: u32, config: &PassthroughConfig) -> PassthroughStatus;
```

### claudefs-transport::conn_auth

```rust
pub struct ConnectionAuthenticator { ... }
impl ConnectionAuthenticator {
    pub fn new(config: AuthConfig) -> Self;
    pub fn authenticate(&mut self, cert: &CertificateInfo) -> AuthResult;
    pub fn revoke_serial(&mut self, serial: String);
    pub fn revoke_fingerprint(&mut self, fingerprint: String);
    pub fn set_time(&mut self, ms: u64);
    pub fn stats(&self) -> AuthStats;
}

pub struct CertificateInfo {
    pub subject: String,
    pub issuer: String,
    pub serial: String,
    pub fingerprint_sha256: String,
    pub not_before_ms: u64,
    pub not_after_ms: u64,
    pub is_ca: bool,
}

pub struct AuthConfig {
    pub level: AuthLevel,
    pub allowed_subjects: Vec<String>,
    pub allowed_fingerprints: Vec<String>,
    pub max_cert_age_days: u32,
    pub require_cluster_ca: bool,
    pub cluster_ca_fingerprint: Option<String>,
}
impl Default for AuthConfig { fn default() -> Self; }

pub enum AuthLevel { None, TlsOnly, MutualTls, MutualTlsStrict }

pub enum AuthResult {
    Allowed { identity: String },
    Denied { reason: String },
    CertificateExpired { subject: String, expired_at_ms: u64 },
    CertificateRevoked { subject: String, serial: String },
}
```

### claudefs-transport::zerocopy

```rust
pub struct RegionPool { ... }
impl RegionPool {
    pub fn new(config: ZeroCopyConfig) -> Self;
    pub fn acquire(&self) -> Option<MemoryRegion>;
    pub fn release(&self, region: MemoryRegion);
    pub fn available(&self) -> usize;
    pub fn total(&self) -> usize;
    pub fn in_use(&self) -> usize;
    pub fn grow(&self, count: usize) -> usize;
    pub fn shrink(&self, count: usize) -> usize;
}

pub struct ZeroCopyConfig {
    pub region_size: usize,
    pub max_regions: usize,
    pub alignment: usize,
    pub preregister: usize,
}
impl Default for ZeroCopyConfig { fn default() -> Self; }
```

### claudefs-transport::flowcontrol

```rust
pub struct FlowController { ... }
impl FlowController {
    pub fn new(config: FlowControlConfig) -> Self;
    pub fn try_acquire(&self, bytes: u64) -> Option<FlowPermit>;
    pub fn state(&self) -> FlowControlState;
    pub fn inflight_requests(&self) -> u32;
    pub fn inflight_bytes(&self) -> u64;
    pub fn release(&self, bytes: u64);
}

pub struct FlowControlConfig {
    pub max_inflight_requests: u32,
    pub max_inflight_bytes: u64,
    pub window_size: u32,
    pub high_watermark_pct: u8,
    pub low_watermark_pct: u8,
}
impl Default for FlowControlConfig { fn default() -> Self; }

pub enum FlowControlState { Open, Throttled, Blocked }
```

## fuse_security_tests.rs — Required Tests (20 tests)

### Category 1: Client Authentication (8 tests)

1. **test_enrollment_empty_token** — Begin enrollment with empty token. Should detect if accepted. FINDING-FUSE-01: No token validation.

2. **test_enrollment_trivial_token** — Begin enrollment with "a" (1-char token). Document if accepted. FINDING-FUSE-01.

3. **test_enrollment_while_enrolled** — Try to begin enrollment when already enrolled. Should be rejected. FINDING-FUSE-02: State machine bypass.

4. **test_double_enrollment_complete** — Complete enrollment twice. Should be rejected second time. State machine correctness.

5. **test_revoked_then_re_enroll** — Revoke certificate, try to enroll again. Document behavior. FINDING-FUSE-03: Post-revocation re-enrollment.

6. **test_crl_growth_unbounded** — Add many entries (1000+) to CRL. Verify grows without limit (document finding). FINDING-FUSE-04: CRL unbounded growth.

7. **test_crl_compact_removes_old** — Add CRL entries, compact with max_age_secs. Verify old entries removed. Correctness test.

8. **test_fingerprint_collision_weakness** — Create two distinct PEM strings that might produce same fingerprint via weak hash. FINDING-FUSE-05: Weak fingerprint.

### Category 2: Path Resolution Security (6 tests)

9. **test_validate_path_dotdot** — Call validate_path with "a/../b". Should be rejected due to "..". FINDING-FUSE-06: Path traversal.

10. **test_validate_path_empty** — Call validate_path with "". Should be rejected. Edge case.

11. **test_validate_path_absolute** — Call validate_path with "/absolute/path". Should be rejected. FINDING-FUSE-07: Absolute path injection.

12. **test_validate_path_deeply_nested** — Call validate_path with 200+ components. Verify behavior with very deep paths.

13. **test_cache_invalidation_prefix** — Insert entries, invalidate_prefix, verify lookup returns None for invalidated entries.

14. **test_generation_tracking_bump** — Bump generation for inode, verify old generation is stale. TOCTOU protection.

### Category 3: Mount Options Security (3 tests)

15. **test_mount_allow_other_default** — Check that allow_other is false by default. FINDING-FUSE-08: Security-critical defaults.

16. **test_mount_parse_invalid_option** — Parse unknown option (e.g., "foobar"). Should return InvalidOption error.

17. **test_mount_default_permissions** — Check that default_permissions is set by default (security-critical). FINDING-FUSE-09.

### Category 4: Passthrough FD Security (3 tests)

18. **test_passthrough_fd_overwrite** — Register same fh twice with different fd values. Document that old fd is silently overwritten. FINDING-FUSE-10: FD leak.

19. **test_passthrough_get_nonexistent** — Get fd for unregistered fh. Should return None. Correctness.

20. **test_passthrough_unregister_twice** — Unregister same fh twice. Second should return None. Correctness.

## transport_security_tests.rs — Required Tests (20 tests)

### Category 1: Certificate Authentication (8 tests)

1. **test_expired_cert_with_unset_time** — Create authenticator without calling set_time(), authenticate cert with past expiry. Document if accepted. FINDING-TRANS-01: Time validation bypass.

2. **test_expired_cert_with_correct_time** — Set time, authenticate expired cert. Should be rejected. Correctness.

3. **test_not_yet_valid_cert** — Set time before cert's not_before. Should be rejected. Correctness.

4. **test_revoked_serial_rejected** — Revoke a serial, authenticate cert with that serial. Should be rejected.

5. **test_revoked_fingerprint_rejected** — Revoke a fingerprint, authenticate cert with that fingerprint. Should be rejected.

6. **test_ca_fingerprint_substring_match** — Set cluster_ca_fingerprint to "CA". Create cert with issuer containing "MyCAInfo". Document if accepted (substring match vulnerability). FINDING-TRANS-02: Weak CA validation.

7. **test_is_ca_field_ignored** — Create cert with is_ca=true. Verify it has no effect on auth decision. FINDING-TRANS-03: is_ca not checked.

8. **test_strict_mode_empty_allowed** — MutualTlsStrict with empty allowed_subjects and allowed_fingerprints. Verify behavior.

### Category 2: Zero-Copy Pool Security (6 tests)

9. **test_pool_exhaustion_returns_none** — Acquire all regions, verify next acquire returns None. DoS protection.

10. **test_released_region_data_zeroed** — Acquire region, write data, release, re-acquire. Verify data is zeroed. FINDING-TRANS-04: Info leak prevention.

11. **test_pool_grow_within_limits** — Grow pool, verify doesn't exceed max_regions. Correctness.

12. **test_pool_shrink_safety** — Shrink pool, verify can still acquire remaining regions.

13. **test_pool_concurrent_acquire_release** — Multiple threads acquiring and releasing. Verify no data corruption. Thread safety.

14. **test_pool_stats_accurate** — Verify pool stats match actual state after operations. Correctness.

### Category 3: Flow Control Security (6 tests)

15. **test_flow_control_blocks_over_limit** — Exceed max_inflight_requests. Verify try_acquire returns None. DoS protection.

16. **test_flow_control_byte_limit** — Exceed max_inflight_bytes. Verify blocking. DoS protection.

17. **test_flow_control_release_restores** — Acquire, release. Verify more can be acquired. Correctness.

18. **test_flow_permit_drop_releases** — Acquire FlowPermit via try_acquire, drop it. Verify resources released. RAII correctness.

19. **test_flow_control_state_transitions** — Exercise Open → Throttled → Blocked transitions. Verify state machine.

20. **test_flow_control_zero_config** — FlowControlConfig with max_inflight_requests=0. Verify behavior (should block all). Edge case.

## Output Format

Generate THREE files:

### File 1: Updated `lib.rs`
Full content of the updated `crates/claudefs-security/src/lib.rs` with four new module declarations added (the two from before plus these two new ones).

### File 2: `fuse_security_tests.rs`
Full content of `crates/claudefs-security/src/fuse_security_tests.rs`.

### File 3: `transport_security_tests.rs`
Full content of `crates/claudefs-security/src/transport_security_tests.rs`.

Mark each file clearly with `// FILE: filename.rs` at the top.
