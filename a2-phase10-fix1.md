# A2 Phase 10: Fix Compilation Errors

## Overview
The three modules (quota_tracker.rs, tenant_isolator.rs, qos_coordinator.rs) were successfully generated but have 18 compilation errors. These are straightforward fixes, mostly related to:

1. Type name conflicts with existing modules (tenant.rs, quota.rs)
2. Missing trait derives and imports
3. Method signature issues (mutable reference needs)

## Errors to Fix

### 1. Duplicate Type Errors (E0252)

**Error:** TenantId defined multiple times
- **Root cause:** TenantId already exists in crate::tenant module
- **Fix:** In quota_tracker.rs and qos_coordinator.rs, use `crate::tenant::TenantId` instead of defining new TenantId
- **Affected files:** quota_tracker.rs, qos_coordinator.rs

**Error:** QuotaUsage defined multiple times
- **Root cause:** QuotaUsage conflicts with existing quota.rs types
- **Fix:** Rename `QuotaUsage` to `TenantQuotaUsage` in quota_tracker.rs (to distinguish from per-user quotas in quota.rs)
- **Update references:** All references to QuotaUsage in quota_tracker.rs and qos_coordinator.rs → TenantQuotaUsage

### 2. Missing Import (E0432)

**Error:** Unresolved import `crate::types::SessionId`
- **Fix:** SessionId is defined in client_session.rs, NOT in types.rs
- **Solution:** Add import in qos_coordinator.rs: `use crate::client_session::SessionId;`

### 3. Async/Await Without Async Context (E0728)

**Error:** `await` is only allowed inside `async` functions and blocks
- **Root cause:** Methods using .await must be marked `async`
- **Affected:** qos_coordinator.rs methods
- **Fix:** Add `async` keyword to method signatures that contain .await

### 4. Missing Trait Derives (E0277, E0599)

**Error:** RequestId doesn't implement Hash, Eq required for DashMap key
- **Fix:** Add derives to RequestId in qos_coordinator.rs:
  ```rust
  #[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
  pub struct RequestId(String)
  ```

### 5. Mutable Reference Issues (E0594)

**Error:** Cannot assign to `self.next_root_inode` which is behind `&` reference
- **Affected methods:** In tenant_isolator.rs `register_tenant()` and `allocate_shard_range()`
- **Root cause:** Methods taking `&self` but need `&mut self` to modify state
- **Fix:** Change method signatures from `fn register_tenant(&self, ...)` to `fn register_tenant(&mut self, ...)`
- **Same for:** `allocate_shard_range`, or use interior mutability (Arc<Mutex>) for shared state

### 6. DashMap API Mismatch (E0599)

**Error:** `keys()` method not found for DashMap
- **Fix:** DashMap doesn't have a `.keys()` method like HashMap
- **Replacement:** Use `.iter()` to iterate over entries, or use `.contains_key()` for lookups

## Recommended Fixes

### quota_tracker.rs
- Remove local `TenantId` definition; use `crate::tenant::TenantId`
- Rename `QuotaUsage` → `TenantQuotaUsage` throughout
- Ensure all DashMap operations use correct API (no `.keys()`, use `.iter()`)

### tenant_isolator.rs
- Remove local `TenantId` definition; use `crate::tenant::TenantId`
- Change `&self` to `&mut self` for state-modifying methods (`register_tenant`, `allocate_shard_range`)
  - OR use Arc<Mutex<InternalState>> pattern for interior mutability if concurrent register is needed
- Fix DashMap API calls

### qos_coordinator.rs
- Remove local `TenantId` definition; use `crate::tenant::TenantId`
- Rename `QuotaUsage` references to `TenantQuotaUsage`
- Add missing import: `use crate::client_session::SessionId;`
- Add derives to RequestId: `#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]`
- Mark methods with `.await` as `async`
- Fix DashMap API calls

## Testing Strategy

After fixes:
1. Run `cargo check -p claudefs-meta` → should complete with 0 errors (warnings OK)
2. Run `cargo test -p claudefs-meta --lib` → expect 1102+ tests passing
3. If tests fail, identify first failure and provide error context

## Files to Update

1. ✅ quota_tracker.rs — Fix TenantId/QuotaUsage conflicts, DashMap API
2. ✅ tenant_isolator.rs — Fix TenantId conflicts, mutable references, DashMap API
3. ✅ qos_coordinator.rs — Fix TenantId, QuotaUsage, SessionId import, RequestId derives, async methods

## Expected Outcome

- 0 compilation errors
- 1102+ tests passing (1035 from Phase 9 + 67 new)
- 0 failures
- Build clean

## Delivery

Write the corrected versions of the three .rs files directly to:
- crates/claudefs-meta/src/quota_tracker.rs
- crates/claudefs-meta/src/tenant_isolator.rs
- crates/claudefs-meta/src/qos_coordinator.rs

The files are already in place; overwrite with corrected versions.
