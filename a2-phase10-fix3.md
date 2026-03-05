# A2 Phase 10: Final Compilation Fixes (3 remaining errors)

## Errors to Fix

1. **E0432: unresolved import `quota_tracker::TenantQuotaUsage`**
   - In lib.rs, `TenantQuotaUsage` is not being exported from quota_tracker.rs
   - Fix: In quota_tracker.rs, add to the `impl` section or ensure `TenantQuotaUsage` is public and uses `Serialize, Deserialize` derives
   - Or: Remove from lib.rs pub use if not meant to be exported

2. **E0425: cannot find type `Ref` in module `dashmap::mapref::multiple`**
   - tenant_isolator.rs or qos_coordinator.rs uses `Ref` type incorrectly
   - Fix: Remove use of `Ref` or use `dashmap::mapref::one::Ref` instead
   - Likely location: methods iterating DashMap entries

3. **E0603: struct import `TenantId` is private**
   - tenant_isolator.rs or qos_coordinator.rs tries to import TenantId but it's private
   - Fix: Use `pub use crate::tenant::TenantId;` at top of files, or make imports use public path

## Action

Read the files and make these three fixes:
1. crates/claudefs-meta/src/quota_tracker.rs
2. crates/claudefs-meta/src/tenant_isolator.rs
3. crates/claudefs-meta/src/qos_coordinator.rs

Ensure:
- `TenantQuotaUsage` is public in quota_tracker.rs (or remove from lib.rs export if internal-only)
- No invalid `Ref` imports; use correct dashmap::mapref types or remove if not needed
- TenantId imports are correct and point to `crate::tenant::TenantId`

Write corrected files directly. After write, `cargo check -p claudefs-meta` should show 0 errors.
