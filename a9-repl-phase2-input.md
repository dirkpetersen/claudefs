# A9 Phase 2: Replication Module Tests

You are writing Rust test code for the `claudefs-tests` crate in the ClaudeFS project. Write one new test module: `repl_phase2_tests.rs`.

## Context

A9 (Test & Validation) is adding external integration tests for A6 (Replication) Phase 2 modules. The following modules from `claudefs-repl` need test coverage from the external test crate.

## Public APIs to Test

### 1. `claudefs_repl::journal` — JournalEntry, OpKind

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpKind {
    Create, Unlink, Rename, Write, Truncate, SetAttr, Link, Symlink, MkDir, SetXattr, RemoveXattr,
}

pub struct JournalEntry {
    pub seq: u64,
    pub shard_id: u32,
    pub site_id: u64,
    pub timestamp_us: u64,
    pub inode: u64,
    pub op: OpKind,
    pub payload: Vec<u8>,
    pub crc32: u32,
}

impl JournalEntry {
    pub fn new(seq, shard_id, site_id, timestamp_us, inode, op, payload) -> Self;
    pub fn compute_crc(&self) -> u32;
    pub fn validate_crc(&self) -> bool;
}
```

### 2. `claudefs_repl::batch_auth` — BatchAuthKey, BatchTag, BatchAuthenticator

```rust
pub struct BatchAuthKey { bytes: [u8; 32] }  // Zeroize+ZeroizeOnDrop
impl BatchAuthKey {
    pub fn generate() -> Self;
    pub fn from_bytes(bytes: [u8; 32]) -> Self;
    pub fn as_bytes(&self) -> &[u8; 32];
}

pub struct BatchTag { pub bytes: [u8; 32] }
impl BatchTag {
    pub fn new(bytes: [u8; 32]) -> Self;
    pub fn zero() -> Self;
}

pub enum AuthResult { Valid, Invalid { reason: String } }

pub struct BatchAuthenticator { key: BatchAuthKey, local_site_id: u64 }
impl BatchAuthenticator {
    pub fn new(key: BatchAuthKey, local_site_id: u64) -> Self;
    pub fn local_site_id(&self) -> u64;
    pub fn sign_batch(&self, source_site_id: u64, batch_seq: u64, entries: &[JournalEntry]) -> BatchTag;
    pub fn verify_batch(&self, tag: &BatchTag, source_site_id: u64, batch_seq: u64, entries: &[JournalEntry]) -> AuthResult;
}
```

### 3. `claudefs_repl::active_active` — ActiveActiveController

```rust
pub enum SiteRole { Primary, Secondary }
pub enum LinkStatus { Up, Degraded, Down }

pub struct ForwardedWrite {
    pub origin_site_id: String,
    pub logical_time: u64,
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

pub struct WriteConflict {
    pub key: Vec<u8>,
    pub local_time: u64,
    pub remote_time: u64,
    pub winner: SiteRole,
}

pub struct ActiveActiveStats {
    pub writes_forwarded: u64,
    pub conflicts_resolved: u64,
    pub link_flaps: u64,
}

pub struct ActiveActiveController {
    pub site_id: String,
    pub role: SiteRole,
    pub link_status: LinkStatus,
    // private fields
}

impl ActiveActiveController {
    pub fn new(site_id: String, role: SiteRole) -> Self;
    pub fn local_write(&mut self, key: Vec<u8>, value: Vec<u8>) -> ForwardedWrite;
    pub fn apply_remote_write(&mut self, fw: ForwardedWrite) -> Option<WriteConflict>;
    pub fn set_link_status(&mut self, status: LinkStatus);
    pub fn stats(&self) -> &ActiveActiveStats;
    pub fn drain_pending(&mut self) -> Vec<ForwardedWrite>;
}
```

### 4. `claudefs_repl::failover` — FailoverManager

```rust
pub enum SiteMode { ActiveReadWrite, StandbyReadOnly, DegradedAcceptWrites, Offline }

pub struct FailoverConfig {
    pub failure_threshold: u32,
    pub recovery_threshold: u32,
    pub check_interval_ms: u64,
    pub active_active: bool,
}
impl Default for FailoverConfig {  // threshold=3, recovery=2, interval=5000, active_active=true }

pub enum FailoverEvent {
    SitePromoted { site_id: u64, new_mode: SiteMode },
    SiteDemoted { site_id: u64, new_mode: SiteMode, reason: String },
    SiteRecovered { site_id: u64 },
    ConflictRequiresResolution { site_id: u64, inode: u64 },
}

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

pub struct FailoverManager { /* private */ }

impl FailoverManager {
    pub fn new(config: FailoverConfig, local_site_id: u64) -> Self;
    pub async fn register_site(&self, site_id: u64);
    pub async fn record_health(&self, site_id: u64, healthy: bool) -> Vec<FailoverEvent>;
    pub async fn get_site_mode(&self, site_id: u64) -> Option<SiteMode>;
}
```

## Requirements

Write `repl_phase2_tests.rs` with at least 50 tests organized into sections:

### Section 1: JournalEntry (10 tests)
1. `test_journal_entry_new_sets_crc` — JournalEntry::new auto-computes crc32
2. `test_journal_entry_validate_crc_true` — new entry validates
3. `test_journal_entry_validate_crc_false_after_tamper` — modify payload, validate fails
4. `test_journal_entry_all_op_kinds` — create entries with each OpKind
5. `test_journal_entry_zero_payload` — empty payload works
6. `test_journal_entry_serde_roundtrip` — serialize/deserialize with serde_json
7. `test_op_kind_serde` — all OpKind variants roundtrip through serde_json
8. `test_journal_entry_compute_crc_deterministic` — same entry computes same crc
9. `test_journal_entry_crc_changes_with_payload` — different payload = different crc
10. `prop_journal_entry_crc_roundtrip` — proptest: random payload always validates

### Section 2: BatchAuthentication (15 tests)
1. `test_batch_key_generate_is_32_bytes`
2. `test_batch_key_from_bytes`
3. `test_batch_key_two_generates_differ` — two generated keys are different (with high probability)
4. `test_batch_tag_zero`
5. `test_batch_tag_new`
6. `test_batch_tag_equality`
7. `test_authenticator_local_site_id`
8. `test_sign_batch_empty_entries` — sign with no entries
9. `test_sign_batch_nonempty_entries`
10. `test_verify_batch_valid` — sign then verify with same key
11. `test_verify_batch_wrong_key` — verify with different key fails
12. `test_verify_batch_tampered_payload` — modify entry payload, verify fails
13. `test_verify_batch_wrong_source_site` — different source_site_id fails verify
14. `test_verify_batch_wrong_seq` — different batch_seq fails verify
15. `prop_batch_auth_roundtrip` — proptest: random payload signs and verifies

### Section 3: ActiveActiveController (15 tests)
1. `test_controller_new_initial_state`
2. `test_controller_link_starts_down`
3. `test_local_write_increments_logical_time`
4. `test_local_write_returns_forwarded_write`
5. `test_local_write_forwards_origin_site_id`
6. `test_stats_writes_forwarded_after_write`
7. `test_drain_pending_clears_queue`
8. `test_drain_pending_empty_initially`
9. `test_apply_remote_write_no_conflict`
10. `test_apply_remote_write_conflict_same_timestamp`
11. `test_conflict_winner_primary_site_id_lower`
12. `test_set_link_status_up_increments_flaps`
13. `test_set_link_status_down_no_flap`
14. `test_stats_conflicts_resolved_after_conflict`
15. `test_forwarded_write_serde`

### Section 4: FailoverManager (15 tests)
1. `test_failover_config_defaults`
2. `test_site_failover_state_new`
3. `test_site_readable_active`
4. `test_site_writable_active`
5. `test_site_offline_not_readable`
6. `test_site_offline_not_writable`
7. `test_failover_manager_new`
8. `test_register_site` — register and get_site_mode returns ActiveReadWrite
9. `test_record_health_healthy_no_transition` — 1 success, no transition
10. `test_record_health_failures_trigger_demote` — failure_threshold consecutive failures
11. `test_record_health_recovery_after_demotion` — enough successes recovers
12. `test_site_mode_default_is_active` — SiteMode::default() == ActiveReadWrite
13. `test_failover_event_variants` — create each FailoverEvent variant
14. `test_unregistered_site_auto_registers` — calling record_health on unknown site works
15. `prop_failover_mode_writable` — proptest: only ActiveReadWrite and DegradedAcceptWrites are writable

## Imports to use

```rust
use claudefs_repl::journal::{JournalEntry, OpKind};
use claudefs_repl::batch_auth::{AuthResult, BatchAuthKey, BatchTag, BatchAuthenticator};
use claudefs_repl::active_active::{
    ActiveActiveController, ActiveActiveStats, ForwardedWrite, LinkStatus, SiteRole, WriteConflict,
};
use claudefs_repl::failover::{
    FailoverConfig, FailoverEvent, FailoverManager, SiteFailoverState, SiteMode,
};
use proptest::prelude::*;
```

## Helper

```rust
fn make_journal_entry(seq: u64, op: OpKind, payload: Vec<u8>) -> JournalEntry {
    JournalEntry::new(seq, 0, 1, 1_000_000, 42, op, payload)
}
```

## Output format

Output the complete file content starting with:
```
//! Replication Phase 2 integration tests
//! ...
```

Wrap all tests in a `#[cfg(test)]` module called `tests` and proptest tests in a separate `proptest_tests` module. Use `#[test]` for sync tests and `#[tokio::test]` for async tests.
