# A4 Phase 7b: Multicast Group Management Module

You are implementing one new module for the `claudefs-transport` crate in the ClaudeFS distributed
filesystem (Rust, Cargo workspace). The crate already has 63 modules and 1013 passing tests.

## Coding Conventions (MANDATORY — follow exactly)

1. **No external async dependencies** — modules must NOT use `tokio`, `async-trait`, `futures`, or
   any async constructs. These are pure sync data-structure/logic modules.
2. **Serde derive** on all public types: `#[derive(Debug, Clone, Serialize, Deserialize)]`
3. **Atomic counters** for stats: `AtomicU64`, `AtomicU32`, `AtomicU8` with `Ordering::Relaxed`
4. **Stats snapshot pattern**: every module has `XxxStats` (atomic fields) + `XxxStatsSnapshot`
5. **Error types** with `thiserror`: `#[derive(Debug, thiserror::Error)]`
6. **No unwrap/expect** in production code — use `?` or return `Result`/`Option`
7. **Tests**: minimum 17 tests in `#[cfg(test)] mod tests` at bottom of file
8. **`std::sync::RwLock`** for interior mutability, NOT `tokio::sync`
9. **Module-level doc comment** `//!` at top of file
10. Do NOT add `pub use` re-exports at top of the file — those go in lib.rs

## Standard imports available

```rust
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicU64, Ordering};
use std::collections::HashMap;
use thiserror::Error;
```

---

## Module: `multicast_group.rs` — Multicast Group Management

### Purpose
Manages named multicast groups for broadcasting control-plane messages (config updates,
membership events, shard rebalancing notifications) to sets of cluster nodes. Used by A2
(Metadata Service) for cluster-wide config propagation and by A6 (Replication) for site
membership announcements. This is a pure protocol-layer abstraction — does NOT do actual
network I/O; callers handle sending to returned member lists.

### Types to implement

```rust
/// Unique group identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GroupId(pub String);

impl GroupId {
    pub fn new(name: impl Into<String>) -> Self;
    pub fn as_str(&self) -> &str;
}

/// A node member of a multicast group.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GroupMember {
    /// Opaque node identifier (16 bytes).
    pub node_id: [u8; 16],
    /// Human-readable label (hostname or address).
    pub label: String,
    /// Timestamp when this member joined (ms since epoch).
    pub joined_at_ms: u64,
}

/// Membership event for group change notifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GroupEvent {
    /// A node joined the group.
    Join { group: GroupId, member: GroupMember },
    /// A node left the group.
    Leave { group: GroupId, node_id: [u8; 16] },
    /// Group was dissolved (all members removed).
    Dissolved { group: GroupId },
}

/// Result of a broadcast operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BroadcastResult {
    /// Number of members in the group at the time of broadcast.
    pub group_size: usize,
    /// Member node_ids that were targeted.
    pub targeted: Vec<[u8; 16]>,
}

/// Configuration for multicast groups.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MulticastGroupConfig {
    /// Maximum number of groups (default: 256).
    pub max_groups: usize,
    /// Maximum members per group (default: 64).
    pub max_members_per_group: usize,
}
impl Default for MulticastGroupConfig { ... }  // max_groups=256, max_members_per_group=64

/// Error type for multicast group operations.
#[derive(Debug, thiserror::Error)]
pub enum MulticastError {
    #[error("group {0:?} not found")]
    GroupNotFound(GroupId),
    #[error("group {0:?} already exists")]
    GroupAlreadyExists(GroupId),
    #[error("member already in group {0:?}")]
    AlreadyMember(GroupId),
    #[error("member not in group {0:?}")]
    NotMember(GroupId),
    #[error("group limit reached ({0})")]
    GroupLimitReached(usize),
    #[error("member limit reached for group {0:?} ({1})")]
    MemberLimitReached(GroupId, usize),
}

/// Manager for named multicast groups.
pub struct MulticastGroupManager {
    config: MulticastGroupConfig,
    groups: RwLock<HashMap<GroupId, Vec<GroupMember>>>,
    stats: Arc<MulticastGroupStats>,
}

impl MulticastGroupManager {
    pub fn new(config: MulticastGroupConfig) -> Self;

    /// Create a new empty group. Error if already exists or group limit reached.
    pub fn create_group(&self, group: GroupId) -> Result<(), MulticastError>;

    /// Add a member to a group. Error if group not found, member already present, or member limit.
    pub fn join(&self, group: &GroupId, member: GroupMember) -> Result<GroupEvent, MulticastError>;

    /// Remove a member from a group. Returns Leave event.
    pub fn leave(&self, group: &GroupId, node_id: &[u8; 16]) -> Result<GroupEvent, MulticastError>;

    /// Dissolve a group — removes all members. Returns Dissolved event.
    pub fn dissolve(&self, group: &GroupId) -> Result<GroupEvent, MulticastError>;

    /// Get all members of a group.
    pub fn members(&self, group: &GroupId) -> Result<Vec<GroupMember>, MulticastError>;

    /// Check if a node is a member of a group.
    pub fn is_member(&self, group: &GroupId, node_id: &[u8; 16]) -> bool;

    /// Prepare a broadcast to a group — returns BroadcastResult with targeted member ids.
    /// Caller is responsible for actually sending the message to each targeted node.
    pub fn prepare_broadcast(&self, group: &GroupId) -> Result<BroadcastResult, MulticastError>;

    /// List all group IDs.
    pub fn list_groups(&self) -> Vec<GroupId>;

    /// Number of groups currently registered.
    pub fn group_count(&self) -> usize;

    /// Stats reference.
    pub fn stats(&self) -> Arc<MulticastGroupStats>;
}

pub struct MulticastGroupStats {
    pub groups_created: AtomicU64,
    pub groups_dissolved: AtomicU64,
    pub joins: AtomicU64,
    pub leaves: AtomicU64,
    pub broadcasts_prepared: AtomicU64,
    pub total_broadcast_targets: AtomicU64,  // sum of group_size at each broadcast
}

pub struct MulticastGroupStatsSnapshot {
    pub groups_created: u64,
    pub groups_dissolved: u64,
    pub joins: u64,
    pub leaves: u64,
    pub broadcasts_prepared: u64,
    pub total_broadcast_targets: u64,
    pub active_groups: usize,
}

impl MulticastGroupStats {
    pub fn new() -> Self;
    pub fn snapshot(&self, active_groups: usize) -> MulticastGroupStatsSnapshot;
}
```

### Tests (minimum 17)
- `test_create_group` — create group, list_groups returns it
- `test_create_group_duplicate` — returns GroupAlreadyExists error
- `test_create_group_limit` — fill to max_groups, next returns GroupLimitReached
- `test_join_success` — join returns Join event with correct member
- `test_join_unknown_group` — returns GroupNotFound
- `test_join_duplicate_member` — returns AlreadyMember
- `test_join_member_limit` — fill group, returns MemberLimitReached
- `test_leave_success` — join then leave, members() is empty
- `test_leave_not_member` — returns NotMember
- `test_dissolve_removes_all` — join 3 members, dissolve, members() returns GroupNotFound
- `test_dissolve_unknown_group` — returns GroupNotFound
- `test_is_member_true` — join member, is_member returns true
- `test_is_member_false` — is_member for non-member returns false
- `test_prepare_broadcast_returns_all_members` — 3 members, targeted.len() == 3
- `test_prepare_broadcast_empty_group` — targeted.len() == 0
- `test_stats_counts` — create/join/leave/dissolve/broadcast, verify counts
- `test_multiple_groups_independent` — two groups, members don't cross

---

## Output Format

Output ONLY the complete file content with this header:

```
=== FILE: crates/claudefs-transport/src/multicast_group.rs ===
<complete file content>
```

## Important
- Produce a complete, compilable Rust file — no TODOs, no unimplemented!()
- Every test must have a descriptive name and assert something meaningful
- Stats snapshot() methods must load atomics with Ordering::Relaxed
- MulticastGroupManager does NOT need Arc<Self> — it uses RwLock internally
