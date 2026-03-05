# A2 Phase 10 Quick Fix — 18 Compilation Errors

The three modules have 18 compilation errors. Read the files and fix all errors in place:

1. crates/claudefs-meta/src/quota_tracker.rs
2. crates/claudefs-meta/src/tenant_isolator.rs
3. crates/claudefs-meta/src/qos_coordinator.rs

## Primary Issues

### Issue 1: Duplicate TenantId type (E0252)
- tenant_isolator.rs and quota_tracker.rs both define TenantId
- SOLUTION: Remove TenantId definitions, use `use crate::tenant::TenantId;` instead

### Issue 2: Duplicate QuotaUsage type (E0252)
- quota_tracker.rs defines QuotaUsage, conflicts with quota.rs
- SOLUTION: Rename in quota_tracker.rs: `QuotaUsage` → `TenantQuotaUsage` (all occurrences)
- Update qos_coordinator.rs: `QuotaUsage` → `TenantQuotaUsage`

### Issue 3: RequestId missing Hash/Eq (E0277)
- qos_coordinator.rs RequestId struct missing derives
- SOLUTION: Change line 55 from `#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]` — already has them, but verify they're present
- If missing: add `Hash` and `Eq` to derive macro

### Issue 4: await without async (E0728)
- tenant_isolator.rs line 268: `.await` on non-async function
- SOLUTION: Change method to async: `pub async fn register_tenant(...)`
- Search for all `.await` calls in tenant_isolator.rs and mark their methods async

### Issue 5: Missing SessionId import (E0432)
- qos_coordinator.rs can't find SessionId
- SOLUTION: Add import: `use crate::client_session::SessionId;`

### Issue 6: Mutable self needed (E0594)
- tenant_isolator.rs: methods try to assign to self but take `&self`
- SOLUTION: Change signatures to `&mut self` for state-modifying methods

## Action

Fix all errors in all three files. Write corrected versions directly to the same file paths:
- crates/claudefs-meta/src/quota_tracker.rs
- crates/claudefs-meta/src/tenant_isolator.rs
- crates/claudefs-meta/src/qos_coordinator.rs

Verify: After writing, `cargo check -p claudefs-meta` should complete with 0 errors.
