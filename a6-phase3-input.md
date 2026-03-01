# A6 Phase 3: Replication Production Readiness

## Context

You are implementing Phase 3 production-readiness features for the `claudefs-repl` crate
in the ClaudeFS distributed file system (Rust, Tokio async, MIT license).

The crate currently has 14 modules and 303 tests passing (zero clippy warnings).
Existing modules: error.rs, journal.rs, wal.rs, topology.rs, conduit.rs, sync.rs,
uidmap.rs, engine.rs, checkpoint.rs, fanout.rs, health.rs, report.rs, throttle.rs, pipeline.rs

## Task: Add Three New Modules

Add three new `.rs` files to `crates/claudefs-repl/src/`:

### 1. `batch_auth.rs` — Batch Authentication & Integrity

Implements HMAC-SHA256 batch authentication to address:
- FINDING-06: No sender authentication on entry batches
- FINDING-07: No application-layer batch integrity

**IMPORTANT**: Do NOT use the `hmac` crate or `sha2` crate — they are NOT in the workspace.
Instead implement SHA256 and HMAC-SHA256 from scratch using only `std`.

Here is the SHA256 implementation to use verbatim:

```rust
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
        0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
        0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0x00);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(chunk[i*4..i*4+4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i-15].rotate_right(7) ^ w[i-15].rotate_right(18) ^ (w[i-15] >> 3);
            let s1 = w[i-2].rotate_right(17) ^ w[i-2].rotate_right(19) ^ (w[i-2] >> 10);
            w[i] = w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g; g = f; f = e;
            e = d.wrapping_add(temp1);
            d = c; c = b; b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a); h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c); h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e); h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g); h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, &v) in h.iter().enumerate() {
        out[i*4..i*4+4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

fn hmac_sha256(key: &[u8; 32], message: &[u8]) -> [u8; 32] {
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..32 {
        ipad[i] ^= key[i];
        opad[i] ^= key[i];
    }
    let mut inner_input = Vec::with_capacity(64 + message.len());
    inner_input.extend_from_slice(&ipad);
    inner_input.extend_from_slice(message);
    let inner_hash = sha256(&inner_input);

    let mut outer_input = Vec::with_capacity(64 + 32);
    outer_input.extend_from_slice(&opad);
    outer_input.extend_from_slice(&inner_hash);
    sha256(&outer_input)
}
```

**Types and functions to implement (using the above SHA256/HMAC):**

```rust
use serde::{Deserialize, Serialize};

/// HMAC-SHA256 key for batch authentication (32 bytes).
pub struct BatchAuthKey {
    bytes: [u8; 32],
}

impl BatchAuthKey {
    /// Generate a new random key.
    pub fn generate() -> Self {
        use rand::Rng;
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill(&mut bytes);
        Self { bytes }
    }
    /// Create from raw bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self { Self { bytes } }
    /// Get the raw bytes.
    pub fn as_bytes(&self) -> &[u8; 32] { &self.bytes }
}

impl Drop for BatchAuthKey {
    fn drop(&mut self) {
        // Zero on drop
        for b in self.bytes.iter_mut() { *b = 0; }
    }
}

/// An authenticated batch tag (HMAC-SHA256 output, 32 bytes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchTag {
    pub bytes: [u8; 32],
}

/// Authentication result.
#[derive(Debug, Clone, PartialEq)]
pub enum AuthResult {
    Valid,
    Invalid { reason: String },
}

/// Signs and verifies entry batches.
pub struct BatchAuthenticator {
    key: BatchAuthKey,
    local_site_id: u64,
}

impl BatchAuthenticator {
    pub fn new(key: BatchAuthKey, local_site_id: u64) -> Self;

    // Compute HMAC-SHA256 tag.
    // Message = source_site_id(8 LE) || batch_seq(8 LE) || for_each_entry[seq(8 LE) || inode(8 LE) || payload]
    pub fn sign_batch(&self, source_site_id: u64, batch_seq: u64, entries: &[crate::journal::JournalEntry]) -> BatchTag;

    // Verify: recompute tag and compare with constant-time equality.
    pub fn verify_batch(&self, tag: &BatchTag, source_site_id: u64, batch_seq: u64, entries: &[crate::journal::JournalEntry]) -> AuthResult;
}
```

**Tests (minimum 20):** See requirements section above.

### 2. `failover.rs` — Active-Active Failover

Implements automatic site failover with read-write capability on both sites.
This is Priority 3 in the ClaudeFS feature roadmap.

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use serde::{Deserialize, Serialize};

/// Site role in active-active mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SiteMode {
    /// Site is fully active: accepts reads and writes.
    ActiveReadWrite,
    /// Site is in standby: accepts reads only.
    StandbyReadOnly,
    /// Site is degraded but still accepts writes.
    DegradedAcceptWrites,
    /// Site is offline.
    Offline,
}

/// Failover configuration.
#[derive(Debug, Clone)]
pub struct FailoverConfig {
    pub failure_threshold: u32,
    pub recovery_threshold: u32,
    pub check_interval_ms: u64,
    pub active_active: bool,
}

impl Default for FailoverConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 3,
            recovery_threshold: 2,
            check_interval_ms: 5000,
            active_active: true,
        }
    }
}

/// Failover event.
#[derive(Debug, Clone, PartialEq)]
pub enum FailoverEvent {
    SitePromoted { site_id: u64, new_mode: SiteMode },
    SiteDemoted { site_id: u64, new_mode: SiteMode, reason: String },
    SiteRecovered { site_id: u64 },
    ConflictRequiresResolution { site_id: u64, inode: u64 },
}

/// Per-site failover state.
#[derive(Debug, Clone)]
pub struct SiteFailoverState {
    pub site_id: u64,
    pub mode: SiteMode,
    pub consecutive_failures: u32,
    pub consecutive_successes: u32,
    pub last_check_us: u64,
    pub failover_count: u64,
}

impl SiteFailoverState {
    pub fn new(site_id: u64) -> Self;
    pub fn is_writable(&self) -> bool;
    pub fn is_readable(&self) -> bool;
}

/// The failover manager.
pub struct FailoverManager {
    config: FailoverConfig,
    local_site_id: u64,
    sites: Arc<Mutex<HashMap<u64, SiteFailoverState>>>,
    events: Arc<Mutex<Vec<FailoverEvent>>>,
}

impl FailoverManager {
    pub fn new(config: FailoverConfig, local_site_id: u64) -> Self;
    pub async fn register_site(&self, site_id: u64);
    pub async fn record_health(&self, site_id: u64, healthy: bool) -> Vec<FailoverEvent>;
    pub async fn site_mode(&self, site_id: u64) -> Option<SiteMode>;
    pub async fn writable_sites(&self) -> Vec<u64>;
    pub async fn readable_sites(&self) -> Vec<u64>;
    pub async fn force_mode(&self, site_id: u64, mode: SiteMode) -> Result<(), crate::error::ReplError>;
    pub async fn drain_events(&self) -> Vec<FailoverEvent>;
    pub async fn all_states(&self) -> Vec<SiteFailoverState>;
    pub async fn failover_counts(&self) -> HashMap<u64, u64>;
}
```

**State transitions:**
- N consecutive failures from ActiveReadWrite → DegradedAcceptWrites → emit SiteDemoted
- N more failures from DegradedAcceptWrites → Offline → emit SiteDemoted
- M consecutive successes from Offline → StandbyReadOnly → emit SitePromoted
- M consecutive successes from StandbyReadOnly → ActiveReadWrite → emit SiteRecovered
- Failures from StandbyReadOnly → Offline → emit SiteDemoted
- reset consecutive counters on each health change (failure resets success_count, success resets failure_count)

**Tests (minimum 25):** See requirements section above.

### 3. `auth_ratelimit.rs` — Authentication Rate Limiting

Implements rate limiting for conduit connections (addresses FINDING-09).

```rust
use std::collections::HashMap;

/// Rate limit configuration.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    pub max_auth_attempts_per_minute: u32,
    pub max_batches_per_second: u32,
    pub max_global_bytes_per_second: u64,
    pub lockout_duration_secs: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_auth_attempts_per_minute: 60,
            max_batches_per_second: 1000,
            max_global_bytes_per_second: 0, // unlimited
            lockout_duration_secs: 300,     // 5 min
        }
    }
}

/// Rate limit check result.
#[derive(Debug, Clone, PartialEq)]
pub enum RateLimitResult {
    Allowed,
    Throttled { wait_ms: u64 },
    Blocked { reason: String, until_us: u64 },
}

/// Per-site rate limit state (private).
struct SiteRateState {
    auth_attempts: Vec<u64>,   // timestamps in microseconds of recent auth attempts
    batch_tokens: f64,          // remaining batch tokens (token bucket)
    batch_last_refill_us: u64,
    locked_until_us: u64,       // 0 = not locked
}

/// Rate limiter for conduit authentication and batch throughput.
pub struct AuthRateLimiter {
    config: RateLimitConfig,
    per_site: HashMap<u64, SiteRateState>,
    global_bytes_tokens: f64,
    global_last_refill_us: u64,
}

impl AuthRateLimiter {
    pub fn new(config: RateLimitConfig) -> Self;

    // Check auth attempt: records timestamp, checks lockout, checks rate.
    // Window = 60 seconds. If attempts in window >= max_auth_attempts_per_minute:
    //   → lock site for lockout_duration_secs, return Blocked
    // If site is locked: return Blocked with remaining time
    // Otherwise: record attempt, return Allowed
    pub fn check_auth_attempt(&mut self, site_id: u64, now_us: u64) -> RateLimitResult;

    // Check batch send rate using token bucket.
    // Refill rate = max_batches_per_second tokens/sec.
    // Also check global bytes limit if configured.
    pub fn check_batch_send(&mut self, site_id: u64, byte_count: u64, now_us: u64) -> RateLimitResult;

    // Reset rate limit for a site (admin unblock).
    pub fn reset_site(&mut self, site_id: u64);

    // Count auth attempts in the last 60 seconds.
    pub fn auth_attempt_count(&self, site_id: u64, now_us: u64) -> u32;

    // Check if site is currently locked out.
    pub fn is_locked_out(&self, site_id: u64, now_us: u64) -> bool;
}
```

**Tests (minimum 15):** See requirements section above.

## Output Format

Output each file surrounded by clear markers:

```
=== FILE: crates/claudefs-repl/src/batch_auth.rs ===
<full file content>
=== END FILE ===

=== FILE: crates/claudefs-repl/src/failover.rs ===
<full file content>
=== END FILE ===

=== FILE: crates/claudefs-repl/src/auth_ratelimit.rs ===
<full file content>
=== END FILE ===

=== FILE: crates/claudefs-repl/src/lib.rs ===
<full file content>
=== END FILE ===
```

## Requirements Summary

1. Zero compiler errors, zero clippy warnings
2. All pub items have doc comments
3. All #[derive] includes appropriate traits
4. Tests are comprehensive (≥20 for batch_auth, ≥25 for failover, ≥15 for auth_ratelimit)
5. No new dependencies — use only: tokio, thiserror, serde, rand, std
6. The SHA256 and HMAC implementations are self-contained in batch_auth.rs
7. FailoverManager uses Arc<Mutex<...>> and async methods
8. AuthRateLimiter is sync (not async), all methods take &mut self and now_us: u64
