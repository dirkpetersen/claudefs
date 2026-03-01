# A7 Gateway Phase 3: Production Modules

Add three new production-grade modules to the `claudefs-gateway` crate. Each module goes in
`crates/claudefs-gateway/src/`. Also update `crates/claudefs-gateway/src/lib.rs` to declare
the three new public modules.

All existing 615 tests must continue to pass. Each new module must have comprehensive tests.

---

## Module 1: `nfs_export.rs` — NFSv3 Export Configuration

Create `crates/claudefs-gateway/src/nfs_export.rs`.

This module integrates the `SquashPolicy` from `auth.rs` into a full NFS export configuration.
An NFS export specifies which path is exported, which clients can access it, and with what
permissions and squash policy.

```rust
//! NFSv3 export configuration with security options

use crate::auth::SquashPolicy;
use std::net::IpAddr;

/// Access mode for an NFS export
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExportAccess {
    ReadOnly,
    ReadWrite,
}

impl Default for ExportAccess {
    fn default() -> Self {
        ExportAccess::ReadOnly  // safe default
    }
}

/// A client IP specification (single IP or CIDR range as string)
#[derive(Debug, Clone, PartialEq)]
pub struct ClientSpec {
    /// CIDR range like "192.168.1.0/24" or single IP, or "*" for all
    pub cidr: String,
}

impl ClientSpec {
    pub fn any() -> Self {
        Self { cidr: "*".to_string() }
    }
    pub fn from_cidr(cidr: &str) -> Self {
        Self { cidr: cidr.to_string() }
    }
    /// Check if an IP is allowed by this spec (simplified: * allows all, exact match for single IP)
    pub fn allows(&self, ip: &str) -> bool {
        if self.cidr == "*" {
            return true;
        }
        // Simple exact-match check (production would use CIDR parsing)
        self.cidr == ip || self.cidr.starts_with(&format!("{}/", ip))
    }
}

/// Configuration for a single NFS export
#[derive(Debug, Clone)]
pub struct ExportConfig {
    /// Local filesystem path being exported (e.g., "/data/project1")
    pub path: String,
    /// Exported path as seen by NFS clients (e.g., "/project1")
    pub export_path: String,
    /// Allowed client specs (empty = no access)
    pub clients: Vec<ClientSpec>,
    /// Read-only or read-write
    pub access: ExportAccess,
    /// Root/all squash policy
    pub squash: SquashPolicy,
    /// UID to use for squashed root (default 65534 = nobody)
    pub squash_uid: u32,
    /// GID to use for squashed root (default 65534 = nogroup)
    pub squash_gid: u32,
    /// Whether to hide the export from showmount
    pub hidden: bool,
}

impl ExportConfig {
    pub fn new(path: &str, export_path: &str) -> Self {
        Self {
            path: path.to_string(),
            export_path: export_path.to_string(),
            clients: vec![],
            access: ExportAccess::default(),
            squash: SquashPolicy::default(),  // RootSquash
            squash_uid: 65534,
            squash_gid: 65534,
            hidden: false,
        }
    }
    pub fn with_client(mut self, spec: ClientSpec) -> Self {
        self.clients.push(spec);
        self
    }
    pub fn with_access(mut self, access: ExportAccess) -> Self {
        self.access = access;
        self
    }
    pub fn with_squash(mut self, squash: SquashPolicy) -> Self {
        self.squash = squash;
        self
    }
    pub fn read_write(mut self) -> Self {
        self.access = ExportAccess::ReadWrite;
        self
    }
    pub fn no_squash(mut self) -> Self {
        self.squash = SquashPolicy::None;
        self
    }
    /// Check if a client IP is allowed to access this export
    pub fn allows_client(&self, ip: &str) -> bool {
        if self.clients.is_empty() {
            return false;
        }
        self.clients.iter().any(|c| c.allows(ip))
    }
    pub fn is_read_only(&self) -> bool {
        self.access == ExportAccess::ReadOnly
    }
    pub fn is_read_write(&self) -> bool {
        self.access == ExportAccess::ReadWrite
    }
}

/// Registry of all NFS exports
pub struct ExportRegistry {
    exports: Vec<ExportConfig>,
}

impl ExportRegistry {
    pub fn new() -> Self {
        Self { exports: vec![] }
    }
    pub fn add(&mut self, export: ExportConfig) {
        self.exports.push(export);
    }
    /// Find export by export_path (exact match)
    pub fn find(&self, export_path: &str) -> Option<&ExportConfig> {
        self.exports.iter().find(|e| e.export_path == export_path)
    }
    /// List all non-hidden exports (for showmount)
    pub fn list_visible(&self) -> Vec<&ExportConfig> {
        self.exports.iter().filter(|e| !e.hidden).collect()
    }
    pub fn count(&self) -> usize {
        self.exports.len()
    }
    /// Remove export by export_path; returns true if removed
    pub fn remove(&mut self, export_path: &str) -> bool {
        let before = self.exports.len();
        self.exports.retain(|e| e.export_path != export_path);
        self.exports.len() < before
    }
}

impl Default for ExportRegistry {
    fn default() -> Self {
        Self::new()
    }
}
```

**Tests for `nfs_export.rs` (at least 25 tests):**

Write `#[cfg(test)]` module with tests covering:
1. `ExportAccess::default()` is ReadOnly
2. `ClientSpec::any()` allows "*"
3. `ClientSpec::any().allows("1.2.3.4")` returns true
4. `ClientSpec::from_cidr("192.168.1.5").allows("192.168.1.5")` returns true
5. `ClientSpec::from_cidr("192.168.1.5").allows("10.0.0.1")` returns false
6. `ExportConfig::new` creates with default SquashPolicy::RootSquash
7. `ExportConfig::new` default access is ReadOnly
8. `ExportConfig::new` with empty clients `allows_client` returns false
9. `with_client(ClientSpec::any())` then `allows_client("any")` returns true
10. `with_access(ExportAccess::ReadWrite)` then `is_read_write()` true
11. `read_write()` convenience method works
12. `no_squash()` sets SquashPolicy::None
13. `with_squash(SquashPolicy::AllSquash)` sets AllSquash
14. Multiple clients: both IPs allowed
15. ExportRegistry::new() starts empty (count == 0)
16. add() increases count
17. find() returns Some for known path
18. find() returns None for unknown path
19. list_visible() returns non-hidden exports
20. hidden export not in list_visible()
21. remove() returns true for known path
22. remove() returns false for unknown path
23. remove() decreases count
24. ExportConfig squash_uid default is 65534
25. ExportConfig squash_gid default is 65534
26. is_read_only() true for default
27. Export path different from local path

---

## Module 2: `s3_ratelimit.rs` — Per-token S3 API Rate Limiting

Create `crates/claudefs-gateway/src/s3_ratelimit.rs`.

Rate limiting for S3 API requests. Uses a token-bucket algorithm per authentication token
(identified by the SHA-256 hash of the bearer token string, same as token_auth.rs).

```rust
//! Per-token S3 API rate limiting using token bucket algorithm

use std::collections::HashMap;
use std::sync::Mutex;

/// Rate limit configuration
#[derive(Debug, Clone, Copy)]
pub struct RateLimitConfig {
    /// Maximum requests per second (bucket capacity)
    pub requests_per_second: u32,
    /// Burst capacity (max tokens in bucket)
    pub burst_capacity: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_second: 1000,
            burst_capacity: 5000,
        }
    }
}

impl RateLimitConfig {
    pub fn new(requests_per_second: u32, burst_capacity: u32) -> Self {
        Self { requests_per_second, burst_capacity }
    }
    pub fn conservative() -> Self {
        Self { requests_per_second: 100, burst_capacity: 500 }
    }
    pub fn generous() -> Self {
        Self { requests_per_second: 10000, burst_capacity: 50000 }
    }
}

/// Token bucket state for one client
#[derive(Debug)]
struct TokenBucket {
    /// Current tokens available
    tokens: f64,
    /// Last refill timestamp (Unix seconds as f64)
    last_refill: f64,
    /// Total requests processed
    total_requests: u64,
    /// Total requests rejected (rate limited)
    rejected_requests: u64,
}

impl TokenBucket {
    fn new(initial_tokens: f64) -> Self {
        Self {
            tokens: initial_tokens,
            last_refill: 0.0,
            total_requests: 0,
            rejected_requests: 0,
        }
    }

    /// Try to consume one token. Returns true if allowed, false if rate limited.
    fn try_consume(&mut self, now: f64, config: &RateLimitConfig) -> bool {
        // Refill tokens based on time elapsed
        let elapsed = (now - self.last_refill).max(0.0);
        let new_tokens = elapsed * config.requests_per_second as f64;
        self.tokens = (self.tokens + new_tokens).min(config.burst_capacity as f64);
        self.last_refill = now;

        self.total_requests += 1;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            self.rejected_requests += 1;
            false
        }
    }
}

/// Per-token rate limiter statistics
#[derive(Debug, Clone)]
pub struct RateLimiterStats {
    pub total_requests: u64,
    pub rejected_requests: u64,
    pub current_tokens: f64,
    pub rejection_rate: f64,
}

/// Per-token S3 API rate limiter
pub struct S3RateLimiter {
    buckets: Mutex<HashMap<String, TokenBucket>>,
    config: RateLimitConfig,
}

impl S3RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            config,
        }
    }

    /// Try to process a request for the given token hash.
    /// Returns true if allowed, false if rate limited.
    /// `now` is the current time in seconds (f64 for sub-second precision).
    pub fn try_request(&self, token_hash: &str, now: f64) -> bool {
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        let bucket = buckets
            .entry(token_hash.to_string())
            .or_insert_with(|| TokenBucket::new(self.config.burst_capacity as f64));
        bucket.try_consume(now, &self.config)
    }

    /// Get statistics for a specific token
    pub fn stats(&self, token_hash: &str) -> Option<RateLimiterStats> {
        let buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        buckets.get(token_hash).map(|b| RateLimiterStats {
            total_requests: b.total_requests,
            rejected_requests: b.rejected_requests,
            current_tokens: b.tokens,
            rejection_rate: if b.total_requests > 0 {
                b.rejected_requests as f64 / b.total_requests as f64
            } else {
                0.0
            },
        })
    }

    /// Remove stale entries for tokens not seen recently
    pub fn evict_stale(&self, now: f64, max_idle_seconds: f64) -> usize {
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        let before = buckets.len();
        buckets.retain(|_, b| (now - b.last_refill) < max_idle_seconds);
        before - buckets.len()
    }

    /// Count tracked tokens
    pub fn tracked_count(&self) -> usize {
        self.buckets.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }
}

impl Default for S3RateLimiter {
    fn default() -> Self {
        Self::new(RateLimitConfig::default())
    }
}
```

**Tests for `s3_ratelimit.rs` (at least 20 tests):**

1. `RateLimitConfig::default()` has 1000 rps and 5000 burst
2. `RateLimitConfig::conservative()` has 100 rps
3. `RateLimitConfig::generous()` has 10000 rps
4. First request is always allowed (bucket starts full)
5. Multiple requests within burst are allowed
6. Requests exceeding burst are rejected (at t=0, send burst_capacity+1 requests)
7. `try_request` returns true for first N requests (N = burst_capacity)
8. After burst exhausted, next request is rejected
9. stats() returns None for unknown token
10. stats() total_requests tracks correctly
11. stats() rejected_requests tracks correctly
12. stats() rejection_rate is 0.0 when no rejections
13. stats() current_tokens decreases per request
14. tracked_count() increases with new tokens
15. evict_stale() removes old entries
16. evict_stale() keeps recent entries
17. Two different tokens have independent buckets
18. After time passes (simulated), tokens refill
19. S3RateLimiter::default() creates with default config
20. evict_stale() returns count of removed entries

For the "after time passes" test: create limiter with conservative config, exhaust burst,
then call try_request with a `now` value 10 seconds later — it should be allowed again.

---

## Module 3: `s3_presigned.rs` — S3 Presigned URL Generation and Validation

Create `crates/claudefs-gateway/src/s3_presigned.rs`.

S3-compatible presigned URL generation and validation. Uses HMAC-SHA256 (via sha2 + hmac crates).
Since `hmac` is NOT in the workspace dependencies, use sha2 directly with a manual HMAC implementation
or use a simple approach: HMAC-like signing using `sha2` + a secret key by computing
SHA256(secret + ":" + message) as a simplified signature (NOT RFC 2104 HMAC, but sufficient
for the structural implementation without adding dependencies).

Actually, to avoid adding `hmac` dependency, use a structure where the signature is computed as:
`sig = hex(sha256(key + ":" + canonical_string))` where key is the S3 secret access key.

```rust
//! S3 presigned URL generation and validation

use sha2::{Sha256, Digest};
use std::collections::HashMap;

const PRESIGNED_URL_ALGO: &str = "CFSV1-HMAC-SHA256";

/// A presigned URL request
#[derive(Debug, Clone)]
pub struct PresignedRequest {
    /// HTTP method (GET, PUT, DELETE, etc.)
    pub method: String,
    /// Bucket name
    pub bucket: String,
    /// Object key
    pub key: String,
    /// Expiry duration in seconds (max 7 days = 604800)
    pub expires_in: u32,
    /// Additional query parameters to include in the signature
    pub extra_params: HashMap<String, String>,
}

impl PresignedRequest {
    pub fn new(method: &str, bucket: &str, key: &str, expires_in: u32) -> Self {
        Self {
            method: method.to_uppercase(),
            bucket: bucket.to_string(),
            key: key.to_string(),
            expires_in: expires_in.min(604800),  // max 7 days
            extra_params: HashMap::new(),
        }
    }
    pub fn get(bucket: &str, key: &str, expires_in: u32) -> Self {
        Self::new("GET", bucket, key, expires_in)
    }
    pub fn put(bucket: &str, key: &str, expires_in: u32) -> Self {
        Self::new("PUT", bucket, key, expires_in)
    }
}

/// A generated presigned URL and its components
#[derive(Debug, Clone)]
pub struct PresignedUrl {
    /// The signed URL (path + query params)
    pub url_path: String,
    /// Access key ID used for signing
    pub access_key_id: String,
    /// Unix timestamp when the URL was created
    pub created_at: u64,
    /// Unix timestamp when the URL expires
    pub expires_at: u64,
    /// HMAC signature hex
    pub signature: String,
    /// Canonical string that was signed
    pub canonical_string: String,
}

impl PresignedUrl {
    pub fn is_expired(&self, now: u64) -> bool {
        now > self.expires_at
    }
}

/// Signer: generates and validates presigned URLs
pub struct PresignedSigner {
    /// Access key ID (public identifier)
    access_key_id: String,
    /// Secret access key (private signing key)
    secret_access_key: String,
}

impl PresignedSigner {
    pub fn new(access_key_id: &str, secret_access_key: &str) -> Self {
        Self {
            access_key_id: access_key_id.to_string(),
            secret_access_key: secret_access_key.to_string(),
        }
    }

    /// Compute a signing key from secret + canonical string (simplified, no hmac dep)
    fn sign(&self, canonical: &str) -> String {
        let input = format!("{}:{}", self.secret_access_key, canonical);
        let hash = Sha256::digest(input.as_bytes());
        hash.iter().map(|b| format!("{:02x}", b)).collect()
    }

    /// Build the canonical string for signing
    fn canonical_string(method: &str, bucket: &str, key: &str, access_key_id: &str, expires_at: u64) -> String {
        format!("{}/{}/{}/{}/{}", method, bucket, key, access_key_id, expires_at)
    }

    /// Generate a presigned URL
    /// `now` is the current Unix timestamp in seconds
    pub fn sign_request(&self, req: &PresignedRequest, now: u64) -> PresignedUrl {
        let expires_at = now + req.expires_in as u64;
        let canonical = Self::canonical_string(
            &req.method,
            &req.bucket,
            &req.key,
            &self.access_key_id,
            expires_at,
        );
        let sig = self.sign(&canonical);

        let url_path = format!(
            "/{}/{}?X-CFS-Algorithm={}&X-CFS-AccessKeyId={}&X-CFS-Expires={}&X-CFS-Signature={}",
            req.bucket, req.key,
            PRESIGNED_URL_ALGO,
            self.access_key_id,
            expires_at,
            sig
        );

        PresignedUrl {
            url_path,
            access_key_id: self.access_key_id.clone(),
            created_at: now,
            expires_at,
            signature: sig,
            canonical_string: canonical,
        }
    }

    /// Validate a presigned URL.
    /// Returns Ok(()) if valid and not expired, Err with reason if invalid.
    pub fn validate_url(&self, method: &str, bucket: &str, key: &str, expires_at: u64, signature: &str, now: u64) -> Result<(), &'static str> {
        if now > expires_at {
            return Err("URL expired");
        }
        let canonical = Self::canonical_string(method, bucket, key, &self.access_key_id, expires_at);
        let expected_sig = self.sign(&canonical);
        if signature != expected_sig {
            return Err("Invalid signature");
        }
        Ok(())
    }

    pub fn access_key_id(&self) -> &str {
        &self.access_key_id
    }
}

/// Parse query parameters from a presigned URL path
pub fn parse_presigned_params(url_path: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    if let Some(query) = url_path.split('?').nth(1) {
        for pair in query.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                params.insert(k.to_string(), v.to_string());
            }
        }
    }
    params
}
```

**Tests for `s3_presigned.rs` (at least 20 tests):**

1. `PresignedRequest::new` sets method to uppercase
2. `PresignedRequest::get` creates GET request
3. `PresignedRequest::put` creates PUT request
4. expires_in is capped at 604800 (7 days)
5. expires_in below max stays unchanged
6. `PresignedSigner::new` stores access key id and secret
7. `sign_request` returns url with correct bucket and key in path
8. `sign_request` url_path contains X-CFS-Algorithm
9. `sign_request` url_path contains X-CFS-AccessKeyId
10. `sign_request` url_path contains X-CFS-Expires
11. `sign_request` url_path contains X-CFS-Signature
12. `sign_request` created_at matches `now` parameter
13. `sign_request` expires_at = now + expires_in
14. `PresignedUrl::is_expired` returns false before expiry
15. `PresignedUrl::is_expired` returns true after expiry
16. `validate_url` returns Ok for valid, non-expired URL
17. `validate_url` returns Err("URL expired") for expired URL
18. `validate_url` returns Err("Invalid signature") for wrong signature
19. Same request signed twice produces same signature (deterministic)
20. Different signer (different secret) produces different signature for same request
21. `parse_presigned_params` extracts key-value pairs from URL query string
22. `parse_presigned_params` returns empty map for URL without query string
23. Signature contains only hex chars (64 chars)
24. `access_key_id()` getter works

---

## lib.rs Update

Add these three lines to `crates/claudefs-gateway/src/lib.rs` (in alphabetical order with existing modules):

```rust
pub mod nfs_export;
pub mod s3_presigned;
pub mod s3_ratelimit;
```

---

## Summary

Create three new files:
1. `crates/claudefs-gateway/src/nfs_export.rs` (~27 tests)
2. `crates/claudefs-gateway/src/s3_ratelimit.rs` (~20 tests)
3. `crates/claudefs-gateway/src/s3_presigned.rs` (~24 tests)

Update:
- `crates/claudefs-gateway/src/lib.rs`: add 3 module declarations

Total new tests: ~71. Total gateway tests should reach ~686+.

After making all changes, run `cargo test -p claudefs-gateway` to verify all tests pass.
