# Authentication & Authorization Security Audit

**Date:** 2026-03-01
**Auditor:** A10 (Security Audit Agent)
**Scope:** A6 conduit auth, A7 gateway auth (NFS + S3 tokens), A8 admin API auth + RBAC
**Status:** Phase 2 initial audit
**Overall Risk:** HIGH — multiple authentication weaknesses across subsystems

## Executive Summary

The authentication layer across ClaudeFS has several design-level weaknesses that must be addressed before production deployment. The most critical issues are:

1. **Conduit (A6):** TLS is optional (violates D7), no sender authentication on batches
2. **Admin API (A8):** Token comparison vulnerable to timing attacks, RBAC exists but isn't wired to the API
3. **Gateway (A7):** Token generation uses predictable patterns (not CSPRNG), NFS AUTH_SYS trusts client-supplied UIDs

16 findings documented (FINDING-05 through FINDING-20), verified by 47 security tests.

---

## A6: Replication Conduit Authentication

### Attack Surface

The conduit is the cross-site replication channel. It carries journal entries between sites via gRPC/mTLS (per D7). A compromised conduit could inject forged journal entries, causing data corruption across sites.

### Findings

#### FINDING-05 (HIGH): TLS is optional on conduit — defaults to plaintext

**Location:** `crates/claudefs-repl/src/conduit.rs:44`
**Description:** `ConduitConfig.tls` is `Option<ConduitTlsConfig>` and defaults to `None`. The conduit operates in plaintext mode when TLS is not configured. Decision D7 requires mTLS for ALL inter-daemon communication.
**Impact:** Journal entries (containing inode operations, write data references) transmitted in cleartext. An attacker on the network can read and modify replication traffic.
**Recommendation:** Make TLS mandatory. Remove `Option` wrapper or require TLS config at construction time. Fail hard if TLS is not configured in production mode.
**Status:** Open

#### FINDING-06 (HIGH): No sender authentication on entry batches

**Location:** `crates/claudefs-repl/src/conduit.rs:80-87`
**Description:** `EntryBatch.source_site_id` is a self-declared field set by the sender. There is no validation that the source_site_id matches the authenticated TLS identity of the sender. A compromised peer can spoof any site ID.
**Impact:** A compromised site B could inject entries claiming to be from site A, causing the receiver to apply forged operations under the wrong site's identity. This could bypass conflict detection.
**Recommendation:** Validate `source_site_id` against the TLS client certificate's site identity. Reject batches where declared site ID doesn't match authenticated identity.
**Status:** Open

#### FINDING-07 (MEDIUM): No application-layer batch integrity

**Location:** `crates/claudefs-repl/src/conduit.rs:78-98`
**Description:** Entry batches have no HMAC, signature, or checksum at the application layer. Integrity relies entirely on transport-level TLS. If TLS is disabled (FINDING-05), batches can be silently modified in transit.
**Recommendation:** Add an HMAC-SHA256 field to EntryBatch, computed over the serialized entries using a shared site key. Verify on receipt regardless of transport security.
**Status:** Open

#### FINDING-08 (MEDIUM): Private key material not zeroized on drop

**Location:** `crates/claudefs-repl/src/conduit.rs:14-21`
**Description:** `ConduitTlsConfig.key_pem` stores the private key as a plain `Vec<u8>`. When the config is dropped, memory is not zeroed. Key material persists in freed memory and can be recovered via memory dumps, swap files, or cold boot attacks.
**Recommendation:** Use `zeroize::Zeroizing<Vec<u8>>` for `key_pem`. Derive `ZeroizeOnDrop` on `ConduitTlsConfig`.
**Status:** Open

#### FINDING-09 (LOW): No rate limiting on conduit receive path

**Location:** `crates/claudefs-repl/src/conduit.rs:232-241`
**Description:** The conduit accepts batches without rate limiting. A compromised peer can flood the conduit with batches, consuming memory and CPU.
**Recommendation:** Add configurable rate limiting (batches/sec, entries/sec) with backpressure. Log and alert on sustained high rates.
**Status:** Open

---

## A8: Management Admin API Authentication

### Attack Surface

The admin API (Axum HTTP) exposes cluster management operations: node status, drain, replication status, capacity metrics. It's intended for administrators and monitoring systems.

### Findings

#### FINDING-10 (HIGH): Token comparison vulnerable to timing attack

**Location:** `crates/claudefs-mgmt/src/api.rs:272`
**Description:** `auth_middleware` compares the provided token against the configured token using `provided_token == token` (standard string equality). This comparison short-circuits on the first mismatched byte, leaking information about the correct token via timing side-channel.
**Impact:** An attacker can determine the correct token byte-by-byte by measuring response times. With ~256 attempts per byte position, a 32-byte token can be recovered in ~8K requests.
**Recommendation:** Use `subtle::ConstantTimeEq` or `ring::constant_time::verify_slices_are_equal` for token comparison.
**Status:** Open

#### FINDING-11 (HIGH): Authentication bypass when admin_token is None

**Location:** `crates/claudefs-mgmt/src/api.rs:261-263`
**Description:** When `admin_token` is not configured (`None`), the auth middleware returns immediately, allowing ALL requests through without any authentication. This is the default configuration.
**Impact:** If an administrator deploys the management API without setting a token, all admin endpoints are publicly accessible. There is no warning or fail-safe.
**Recommendation:** Require `admin_token` in production mode. Log a WARNING on startup if no token is configured. Consider generating a random token and writing it to a secure file if none is provided.
**Status:** Open

#### FINDING-12 (MEDIUM): RBAC not integrated with API middleware

**Location:** `crates/claudefs-mgmt/src/rbac.rs` (entire module), `crates/claudefs-mgmt/src/api.rs:256-296`
**Description:** A comprehensive RBAC system exists (`RbacRegistry` with users, roles, and permissions) but it is not wired into the API's `auth_middleware`. The middleware only checks a single static bearer token — all authenticated requests get the same access level.
**Impact:** There is no way to grant limited access (e.g., read-only monitoring) vs full admin access. The RBAC module is dead code from a security perspective.
**Recommendation:** Wire RBAC into the middleware: map bearer tokens to user identities, check permissions per endpoint. The plumbing is already built — it just needs integration.
**Status:** Open

#### FINDING-13 (MEDIUM): No rate limiting on authentication failures

**Location:** `crates/claudefs-mgmt/src/api.rs:256-296`
**Description:** Failed authentication attempts return 401 immediately with no rate limiting, lockout, or progressive delay. An attacker can brute-force the bearer token at network speed.
**Recommendation:** Implement rate limiting on auth failures: progressive delays (1s, 2s, 4s...) or IP-based lockout after N failures. Log all authentication failures with source IP.
**Status:** Open

#### FINDING-14 (LOW): Health endpoint exposes version — accepted risk

**Location:** `crates/claudefs-mgmt/src/api.rs:153-158`
**Description:** The `/health` endpoint returns the software version without authentication. When `admin_token` is configured, the health endpoint requires auth (it's behind the middleware).
**Impact:** Version disclosure aids targeted attacks. However, health endpoints need to be unauthenticated for load balancer probes.
**Recommendation:** Consider a separate unauthenticated health probe (`/ready` returning only status code) vs an authenticated `/health` with version info. ACCEPTED RISK for now.
**Status:** Accepted

#### FINDING-15 (MEDIUM): Destructive operations have no RBAC

**Location:** `crates/claudefs-mgmt/src/api.rs:197-217`
**Description:** The node drain endpoint (`POST /api/v1/nodes/{id}/drain`) is a destructive admin operation that triggers data evacuation. Any authenticated user can invoke it — there is no role or permission check.
**Impact:** A monitoring system or read-only user with a valid token can accidentally or maliciously drain storage nodes.
**Recommendation:** Require `Permission::DrainNodes` (already defined in RBAC). Block destructive operations behind admin-level authentication.
**Status:** Open

---

## A7: Gateway Authentication (NFS + S3 Tokens)

### Attack Surface

The gateway serves NFS clients (AUTH_SYS credentials) and S3 clients (bearer tokens). NFS AUTH_SYS is inherently trust-based. S3 token auth is custom-built.

### Findings

#### FINDING-16 (HIGH): Token generation uses predictable pattern

**Location:** `crates/claudefs-gateway/src/token_auth.rs:163-165`
**Description:** `TokenAuth::generate_token(uid, counter)` produces tokens by formatting `{counter:016x}{uid:08x}`. This is entirely deterministic — an attacker who knows the uid and can estimate the counter value can predict valid tokens.
**Impact:** Token prediction enables unauthorized S3 API access. The counter is likely sequential, making prediction trivial.
**Recommendation:** Use `rand::rngs::OsRng` with `rand::RngCore::fill_bytes()` to generate 32 bytes of cryptographically random data, then hex-encode. Never use deterministic patterns for security tokens.
**Status:** Open

#### FINDING-17 (HIGH): AUTH_SYS trusts client-supplied UID/GID

**Location:** `crates/claudefs-gateway/src/auth.rs:14-88`
**Description:** NFS AUTH_SYS (sec=sys) accepts whatever UID/GID the client declares. There is no server-side verification. A client can claim to be root (uid=0) or any other user.
**Impact:** Any NFS client can access files as any user, including root. This is a fundamental NFS protocol limitation but must be documented and mitigated.
**Recommendation:** Implement root squashing (map uid=0 to nobody:nogroup) as default. Add IP-based access controls. Document that `sec=krb5p` (D7) is required for untrusted networks. Add `root_squash` option to gateway config.
**Status:** Open

#### FINDING-18 (MEDIUM): Tokens stored in plaintext HashMap

**Location:** `crates/claudefs-gateway/src/token_auth.rs:97-99`
**Description:** Token strings are stored as plaintext keys in a `HashMap<String, AuthToken>`. If the process memory is dumped (core dump, /proc/pid/mem), all active tokens are exposed.
**Impact:** Memory disclosure attacks reveal all active S3 authentication tokens.
**Recommendation:** Store hashed tokens (SHA-256) as HashMap keys. On validation, hash the provided token and look up the hash. The `AuthToken.token` field should store the hash, not the plaintext.
**Status:** Open

#### FINDING-19 (MEDIUM): Mutex poisoning leads to DoS

**Location:** `crates/claudefs-gateway/src/token_auth.rs:110-118`
**Description:** `TokenAuth` uses `std::sync::Mutex` with `.unwrap()` on `lock()`. If any thread panics while holding the lock (e.g., during a register or validate call), the Mutex becomes poisoned and all subsequent lock attempts panic.
**Impact:** A single panic in any token operation permanently disables the entire token authentication system.
**Recommendation:** Use `parking_lot::Mutex` (no poisoning, smaller, faster) or handle `PoisonError` gracefully with recovery.
**Status:** Open

#### FINDING-20 (LOW): No root squashing on NFS AUTH_SYS

**Location:** `crates/claudefs-gateway/src/auth.rs:118-140`
**Description:** `AuthCred::uid()` returns the raw UID from AUTH_SYS credentials, including uid=0 (root). There is no root squashing mechanism.
**Impact:** NFS clients can perform operations as root on exported filesystems.
**Recommendation:** Add root squashing as a configurable gateway option. Default to `root_squash` (map uid/gid 0 to 65534). Add `all_squash` option for maximum security.
**Status:** Open

---

## Additional Observations

### AUTH_SYS machinename has no length limit

**Location:** `crates/claudefs-gateway/src/auth.rs:32-58`
**Description:** The `machinename` field in AUTH_SYS credentials has no length validation. A client can send a 10KB+ machine name, causing memory allocation.
**Recommendation:** Add a maximum length check (e.g., 255 bytes per RFC 1831).

### Token expiry boundary condition

**Location:** `crates/claudefs-gateway/src/token_auth.rs:79-81`
**Description:** `is_expired()` uses `now >= self.expires_at`, meaning the token is already expired at the exact expiry timestamp. This is correct but should be documented clearly.

---

## Summary Table

| ID | Severity | Subsystem | Title | Status |
|----|----------|-----------|-------|--------|
| FINDING-05 | HIGH | A6 Conduit | TLS optional, defaults to plaintext | Open |
| FINDING-06 | HIGH | A6 Conduit | No sender authentication on batches | Open |
| FINDING-07 | MEDIUM | A6 Conduit | No application-layer batch integrity | Open |
| FINDING-08 | MEDIUM | A6 Conduit | Private key not zeroized on drop | Open |
| FINDING-09 | LOW | A6 Conduit | No rate limiting on conduit | Open |
| FINDING-10 | HIGH | A8 Admin API | Timing attack on token comparison | Open |
| FINDING-11 | HIGH | A8 Admin API | Auth bypass when token is None | Open |
| FINDING-12 | MEDIUM | A8 Admin API | RBAC not wired to API middleware | Open |
| FINDING-13 | MEDIUM | A8 Admin API | No rate limiting on auth failures | Open |
| FINDING-14 | LOW | A8 Admin API | Health endpoint leaks version | Accepted |
| FINDING-15 | MEDIUM | A8 Admin API | Drain has no RBAC authorization | Open |
| FINDING-16 | HIGH | A7 Gateway | Predictable token generation | Open |
| FINDING-17 | HIGH | A7 Gateway | AUTH_SYS trusts client UID/GID | Open |
| FINDING-18 | MEDIUM | A7 Gateway | Tokens in plaintext HashMap | Open |
| FINDING-19 | MEDIUM | A7 Gateway | Mutex poisoning → DoS | Open |
| FINDING-20 | LOW | A7 Gateway | No NFS root squashing | Open |

**Severity distribution:** 6 HIGH, 7 MEDIUM, 3 LOW
**Open findings:** 15 of 16
**Accepted risks:** 1 (FINDING-14)

## Recommendations Priority

### Immediate (before any deployment)
1. FINDING-05: Make conduit TLS mandatory
2. FINDING-06: Validate batch sender identity against TLS cert
3. FINDING-10: Use constant-time token comparison
4. FINDING-11: Require admin_token in production config
5. FINDING-16: Replace predictable token generation with CSPRNG

### Before production
6. FINDING-12: Wire RBAC to API middleware
7. FINDING-15: Add RBAC checks to destructive endpoints
8. FINDING-17: Implement root squashing for NFS
9. FINDING-18: Hash tokens at rest
10. FINDING-19: Replace std::sync::Mutex with parking_lot

### Hardening
11. FINDING-07: Add application-layer batch integrity (HMAC)
12. FINDING-08: Zeroize private key material
13. FINDING-09: Add conduit rate limiting
14. FINDING-13: Rate limit auth failures
15. FINDING-20: Add configurable root/all squashing

---

*Next audit phase (Phase 3): Full penetration test of management API, crypto audit completion, dependency CVE sweep, final unsafe review.*
