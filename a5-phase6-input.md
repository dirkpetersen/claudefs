# A5 FUSE Client — Phase 6: Advanced Reliability, Observability & Multipath

You are implementing Phase 6 of the ClaudeFS FUSE client crate (`claudefs-fuse`).
The crate already has 37 modules and 641 passing tests.

## Existing Crate State

`Cargo.toml` dependencies already present (do NOT modify Cargo.toml):
```toml
tokio.workspace = true
thiserror.workspace = true
anyhow.workspace = true
serde.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
fuser = "0.15"
libc = "0.2"
lru = "0.12"
```

Existing modules in `crates/claudefs-fuse/src/`:
attr, cache, capability, client_auth, datacache, deleg, dirnotify, error,
fallocate, filesystem, health, inode, interrupt, io_priority, locking, migration,
mmap, mount, openfile, operations, passthrough, perf, posix_acl, prefetch,
quota_enforce, ratelimit, reconnect, server, session, snapshot, symlink,
tiering_hints, tracing_client, transport, worm, writebuf, xattr

The `error` module exports:
```rust
pub type Result<T> = std::result::Result<T, FuseError>;

#[derive(Debug, thiserror::Error)]
pub enum FuseError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),
    #[error("Already exists: {0}")]
    AlreadyExists(String),
    #[error("Resource busy: {0}")]
    ResourceBusy(String),
    #[error("Not supported: {0}")]
    NotSupported(String),
}
```

The `inode` module exports `pub type InodeId = u64;`

The `tracing_client` module exports `TraceId(u128)`, `SpanId(u64)`, `TraceContext { trace_id, span_id, sampled }`.

The existing `locking.rs` implements POSIX fcntl() advisory locks (byte-range locking).

## Task: Add 5 New Modules

Implement these 5 new files. Each file must be self-contained (no cross-module imports except `crate::error::Result` and `crate::inode::InodeId` where needed). Do NOT use any new external crates beyond what's in Cargo.toml.

---

### 1. `crates/claudefs-fuse/src/otel_trace.rs`

OpenTelemetry-compatible trace span collection and export.
Uses `crate::tracing_client::{TraceId, SpanId, TraceContext}` (re-exports them as needed).

**Requirements:**
- `SpanStatus` enum: `Ok`, `Error(String)`, `Unset`
- `SpanKind` enum: `Internal`, `Client`, `Server`, `Producer`, `Consumer`
- `SpanAttribute` struct: `key: String, value: String`
- `OtelSpan` struct:
  - Fields: `trace_id: TraceId`, `span_id: SpanId`, `parent_span_id: Option<SpanId>`,
    `operation: String`, `service: String`, `start_unix_ns: u64`, `end_unix_ns: u64`,
    `status: SpanStatus`, `kind: SpanKind`, `attributes: Vec<SpanAttribute>`
  - Methods: `duration_ns() -> u64`, `is_error() -> bool`, `add_attribute(key, value)`
  - `set_status(status)`, `finish(end_ns: u64)`
- `OtelSpanBuilder` struct for constructing spans:
  - Fields mirroring OtelSpan (most optional), `start_unix_ns: u64`
  - `new(operation, service, start_unix_ns)` constructor
  - `with_parent(parent: &TraceContext)` sets trace_id and parent_span_id
  - `with_trace_id(TraceId)`, `with_kind(SpanKind)`, `with_attribute(key, value)`
  - `build(end_unix_ns: u64) -> OtelSpan` — generates deterministic span_id using hash of operation+start_unix_ns if not set
- `OtelExportBuffer` struct: an in-memory buffer of completed spans
  - `push(span: OtelSpan)` — drops oldest if buffer is full
  - `drain() -> Vec<OtelSpan>` — removes and returns all buffered spans
  - `len() -> usize`, `is_empty() -> bool`
  - `capacity: usize` field, max 10_000 spans
  - `new(capacity: usize) -> Self`
- `SamplingDecision` enum: `RecordAndSample`, `Drop`
- `OtelSampler` struct:
  - `new(sample_rate: f64)` — 0.0=drop all, 1.0=sample all
  - `should_sample(trace_id: TraceId) -> SamplingDecision` — deterministic based on trace_id lower 32 bits mod a large prime compared to threshold
  - `sample_rate() -> f64`

**Tests (20+):** Test `duration_ns`, `is_error`, `add_attribute`, `OtelSpanBuilder::build`,
`with_parent` propagates trace_id correctly, `OtelExportBuffer::push/drain/len`,
buffer drops oldest when full, `OtelSampler::should_sample` at 0.0/1.0/0.5,
sampler determinism (same trace_id gives same decision).

---

### 2. `crates/claudefs-fuse/src/idmap.rs`

UID/GID identity mapping for user namespace virtualization.
Used to remap client UIDs/GIDs when the FUSE client crosses user-namespace boundaries.

**Requirements:**
- `IdMapMode` enum: `Identity` (no mapping), `Squash { nobody_uid: u32, nobody_gid: u32 }`,
  `RangeShift { host_base: u32, local_base: u32, count: u32 }`,
  `Table` (uses explicit mapping table)
- `IdMapEntry` struct: `host_id: u32, local_id: u32` — one entry in the Table mode
- `IdMapper` struct:
  - `new(mode: IdMapMode) -> Self`
  - `add_uid_entry(entry: IdMapEntry) -> Result<()>` — max 65_536 entries, returns error if exceeded or duplicate host_id
  - `add_gid_entry(entry: IdMapEntry) -> Result<()>` — same constraints
  - `map_uid(host_uid: u32) -> u32` — maps host→local. In Identity: returns host_uid.
    In Squash: always returns nobody_uid. In RangeShift: if `host_uid` in `[host_base, host_base+count)`,
    returns `local_base + (host_uid - host_base)`, else returns host_uid unchanged.
    In Table: looks up entry, returns host_uid if not found.
  - `map_gid(host_gid: u32) -> u32` — same logic for GIDs
  - `reverse_map_uid(local_uid: u32) -> Option<u32>` — Table only: finds host_id for local_id
  - `reverse_map_gid(local_gid: u32) -> Option<u32>` — same for GIDs
  - `uid_entry_count() -> usize`, `gid_entry_count() -> usize`
  - `mode() -> &IdMapMode`
- Root preservation: `map_uid(0)` always returns 0 in Identity and RangeShift modes (root is not remapped unless explicitly in Table)
- `IdMapStats` struct: `uid_lookups: u64, gid_lookups: u64, uid_hits: u64, gid_hits: u64`

**Tests (20+):** Test Identity pass-through, Squash maps all UIDs to nobody,
RangeShift in-range remapping, RangeShift out-of-range pass-through,
Table mode lookup hit and miss, reverse_map_uid, add_entry duplicate returns error,
max entries limit, root preservation in Identity/RangeShift, GID variants mirror UID.

---

### 3. `crates/claudefs-fuse/src/flock.rs`

BSD `flock(2)` advisory lock support for the FUSE client.
Complements the existing `locking.rs` which handles POSIX `fcntl()` byte-range locks.
`flock()` locks are whole-file, owned by an open file description (fd+pid combination).

**Requirements:**
- `FlockType` enum: `Shared`, `Exclusive`, `Unlock`
- `FlockHandle` struct:
  - Fields: `fd: u64` (file descriptor token), `ino: InodeId`, `pid: u32`,
    `lock_type: FlockType`, `nonblocking: bool`
  - `new(fd, ino, pid, lock_type, nonblocking) -> Self`
  - `is_blocking() -> bool` (inverse of nonblocking)
  - `is_shared() -> bool`, `is_exclusive() -> bool`
- `FlockConflict` enum: `None`, `WouldBlock { holder_pid: u32 }`, `Deadlock`
- `FlockEntry` struct: `fd: u64, ino: InodeId, pid: u32, lock_type: FlockType`
- `FlockRegistry` struct:
  - `new() -> Self`
  - `try_acquire(handle: FlockHandle) -> FlockConflict` — returns `None` on success (lock recorded),
    `WouldBlock` if conflicting lock held, applies upgrade/downgrade if same fd already holds a lock
  - `release(fd: u64, ino: InodeId)` — removes the lock for this fd+ino combination
  - `release_all_for_pid(pid: u32)` — clean up all locks held by a crashed/exited process
  - `has_lock(fd: u64, ino: InodeId) -> bool`
  - `lock_type_for(fd: u64, ino: InodeId) -> Option<FlockType>`
  - `holder_count(ino: InodeId) -> usize` — how many fds hold a lock on this inode
  - Conflict rules: Shared + Shared = OK, Shared + Exclusive = conflict, Exclusive + anything = conflict,
    same fd upgrading from Shared to Exclusive = allowed only if no other shared holders,
    same fd downgrading from Exclusive to Shared = always allowed
- `FlockStats` struct: `acquires: u64, releases: u64, conflicts: u64, upgrades: u64, downgrades: u64`

**Tests (22+):** Shared+Shared succeeds, Exclusive blocks Shared, Exclusive blocks Exclusive,
Shared blocks Exclusive, acquire returns WouldBlock not deadlock for nonblocking,
release removes lock, has_lock after acquire/release, upgrade Shared→Exclusive when alone,
upgrade blocked when another shared holder, downgrade Exclusive→Shared always works,
release_all_for_pid cleans up, holder_count, lock_type_for returns None after release.

---

### 4. `crates/claudefs-fuse/src/multipath.rs`

Multi-path I/O management for the FUSE client.
Manages multiple concurrent transport paths to storage nodes with load balancing,
health monitoring, and automatic failover. This is a routing/selection layer — it does
not implement actual I/O, it decides *which path* to use.

**Requirements:**
- `PathId(u64)` newtype
- `PathState` enum: `Active`, `Degraded`, `Failed`, `Reconnecting`
  - `is_usable() -> bool` — Active or Degraded
- `PathPriority(u8)` with associated const `DEFAULT: u8 = 100`
- `PathMetrics` struct:
  - Fields: `latency_us: u64` (exponential moving average), `error_count: u64`,
    `bytes_sent: u64`, `bytes_recv: u64`, `last_error_at_secs: u64`
  - `new() -> Self` — initializes latency_us to 1000 (1ms baseline)
  - `record_success(latency_us: u64)` — updates EMA: `new_latency = (7 * old + latency_us) / 8`
  - `record_error(now_secs: u64)` — increments error_count, records timestamp
  - `error_rate_recent(now_secs: u64, window_secs: u64) -> f64` — returns 1.0 if last_error_at_secs >= now_secs.saturating_sub(window_secs) and error_count > 0, else 0.0
  - `score() -> u64` — `latency_us + error_count * 1000`
- `PathInfo` struct:
  - Fields: `id: PathId`, `state: PathState`, `priority: u8`,
    `remote_addr: String`, `metrics: PathMetrics`
  - Methods: `new(id: PathId, remote_addr: String, priority: u8) -> Self`,
    `mark_degraded(&mut self)`, `mark_failed(&mut self)`,
    `mark_reconnecting(&mut self)`, `mark_active(&mut self)`, `is_usable() -> bool`
- `LoadBalancePolicy` enum: `RoundRobin`, `LeastLatency`, `Primary`
  - `Primary` uses highest-priority path exclusively, falls back to next if failed
- `MultipathRouter` struct:
  - `new(policy: LoadBalancePolicy) -> Self`
  - `add_path(&mut self, info: PathInfo) -> Result<()>` — max 16 paths, error if duplicate PathId
  - `remove_path(&mut self, id: PathId) -> Result<()>` — error if not found
  - `select_path(&mut self) -> Option<PathId>` — selects according to policy among usable paths
  - `record_success(&mut self, id: PathId, latency_us: u64) -> Result<()>`
  - `record_error(&mut self, id: PathId, now_secs: u64) -> Result<()>` — marks Degraded after 3 errors, Failed after 10
  - `mark_reconnecting(&mut self, id: PathId) -> Result<()>`, `mark_active(&mut self, id: PathId) -> Result<()>`
  - `path_count(&self) -> usize`, `usable_path_count(&self) -> usize`
  - `all_paths_failed(&self) -> bool`
  - For `RoundRobin`: cycles through usable paths in insertion order using a round-robin index
  - For `LeastLatency`: picks usable path with lowest `score()`
  - For `Primary`: picks usable path with highest `priority`; if tie, picks lowest score

**Tests (20+):** add_path/remove_path, select_path RoundRobin cycles correctly,
select_path LeastLatency picks lowest score, Primary picks highest priority,
Primary falls back when primary fails, record_error increments, degraded after 3 errors,
failed after 10 errors, usable_path_count excludes Failed, all_paths_failed,
max 16 paths limit enforced, select_path returns None when no usable paths,
duplicate PathId returns error on add.

---

### 5. `crates/claudefs-fuse/src/crash_recovery.rs`

FUSE client crash recovery: reconstructs open-file state and outstanding locks
after a daemon crash and restart.

**Requirements:**
- `RecoveryState` enum: `Idle`, `Scanning`, `Replaying { replayed: u32, total: u32 }`,
  `Complete { recovered: u32, orphaned: u32 }`, `Failed(String)`
  - `is_in_progress() -> bool` — Scanning or Replaying
  - `is_complete() -> bool` — Complete or Failed
- `OpenFileRecord` struct: `ino: InodeId, fd: u64, pid: u32, flags: u32, path_hint: String`
  - `is_writable() -> bool` — flags & 1 != 0 (O_WRONLY) or flags & 2 != 0 (O_RDWR)
  - `is_append_only() -> bool` — flags & 1024 != 0 (O_APPEND)
- `PendingWrite` struct: `ino: InodeId, offset: u64, len: u64, dirty_since_secs: u64`
  - `age_secs(now: u64) -> u64` — now.saturating_sub(dirty_since_secs)
  - `is_stale(now: u64, max_age_secs: u64) -> bool` — age_secs(now) > max_age_secs
- `RecoveryJournal` struct:
  - Tracks open files and pending writes collected during recovery scanning
  - `new() -> Self`
  - `add_open_file(&mut self, record: OpenFileRecord)`
  - `add_pending_write(&mut self, write: PendingWrite)`
  - `open_file_count(&self) -> usize`, `pending_write_count(&self) -> usize`
  - `writable_open_files(&self) -> Vec<&OpenFileRecord>`
  - `stale_pending_writes(&self, now_secs: u64, max_age_secs: u64) -> Vec<&PendingWrite>`
- `RecoveryConfig` struct:
  - Fields: `max_recovery_secs: u64`, `max_open_files: usize`, `stale_write_age_secs: u64`
  - `default_config() -> Self` — max_recovery_secs=30, max_open_files=10_000, stale_write_age_secs=300
- `CrashRecovery` struct:
  - `new(config: RecoveryConfig) -> Self`
  - `state(&self) -> &RecoveryState`
  - `begin_scan(&mut self) -> Result<()>` — transitions Idle → Scanning, errors if not Idle
  - `record_open_file(&mut self, record: OpenFileRecord) -> Result<()>` — errors if not Scanning,
    errors (InvalidArgument) if max_open_files exceeded
  - `record_pending_write(&mut self, write: PendingWrite) -> Result<()>` — errors if not Scanning
  - `begin_replay(&mut self, total: u32) -> Result<()>` — Scanning → Replaying{0, total}
  - `advance_replay(&mut self, count: u32)` — increments replayed field, clamps to total
  - `complete(&mut self, orphaned: u32) -> Result<()>` — Replaying → Complete
  - `fail(&mut self, reason: String)` — any state → Failed
  - `reset(&mut self)` — any state → Idle, clears journal
  - `journal(&self) -> &RecoveryJournal`

**Tests (22+):** begin_scan transitions state, begin_scan errors if not Idle,
record_open_file errors if not Scanning, max_open_files limit enforced,
begin_replay transitions, advance_replay increments, advance_replay clamps at total,
complete transitions, fail from any state, reset clears journal,
is_writable with O_RDWR flags, is_append_only with O_APPEND flag,
stale_pending_writes age filter, writable_open_files filter,
full happy-path recovery sequence, is_in_progress, is_complete.

---

## Deliverables

Provide the complete Rust source for each of these 5 files:
1. `crates/claudefs-fuse/src/otel_trace.rs`
2. `crates/claudefs-fuse/src/idmap.rs`
3. `crates/claudefs-fuse/src/flock.rs`
4. `crates/claudefs-fuse/src/multipath.rs`
5. `crates/claudefs-fuse/src/crash_recovery.rs`

Also provide the updated `crates/claudefs-fuse/src/lib.rs` with 5 new `pub mod` lines added.
The full updated lib.rs should have these modules in alphabetical order:
attr, cache, capability, client_auth, crash_recovery, datacache, deleg, dirnotify, error,
fallocate, filesystem, flock, health, inode, interrupt, io_priority, idmap, locking, migration,
mmap, mount, multipath, openfile, operations, otel_trace, passthrough, perf, posix_acl, prefetch,
quota_enforce, ratelimit, reconnect, server, session, snapshot, symlink, tiering_hints,
tracing_client, transport, worm, writebuf, xattr

## Strict Requirements

- No new external crates. Use only: std, tokio, thiserror, anyhow, serde, tracing, fuser, libc, lru.
- No `use crate::X` for modules that don't exist yet (only import from error and inode).
- All `#[cfg(test)]` test modules at end of each file.
- Each test module starts with: `#[cfg(test)] mod tests { use super::*; ... }`
- All test functions use `#[test]` (not tokio::test — no async needed).
- Tests must compile and pass with only `std` + the types defined in that file.
- Do NOT use `rand` crate — generate pseudo-random span_ids using std::collections::hash_map::DefaultHasher.
- No `unsafe` code.
- All public types derive `Debug`.
- Use `crate::error::Result` (not FuseError directly) for return types.
- Do NOT import `crate::error::FuseError` directly; use `crate::error::Result<T>` only.
- `otel_trace.rs` MUST import `use crate::tracing_client::{TraceId, SpanId, TraceContext};`
- `flock.rs` and `crash_recovery.rs` MUST import `use crate::inode::InodeId;`
- `multipath.rs` does NOT need InodeId.
- `idmap.rs` does NOT need InodeId.
