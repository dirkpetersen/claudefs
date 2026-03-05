# Supervisor: Fix Test Compilation Errors

## Context
The ClaudeFS test suite has 8 compilation errors across mgmt, security, and transport test files. All are in test/validation code, not production code. The underlying modules are correct—only the test assertions and method calls need fixes.

## Errors to Fix

### 1. claudefs-mgmt/src/event_sink.rs (3 errors)
- **Line 565:** `assert_eq!(path, PathBuf::from(...))` — comparing `&PathBuf` with `PathBuf`
  - Fix: Change to `assert_eq!(*path, PathBuf::from("/first.log"))`
- **Lines 597-599:** EventSeverity comparison operators — enum lacks `PartialOrd`
  - These three assertions compare EventSeverity variants with `>` operator
  - Fix: EventSeverity struct needs `#[derive(PartialOrd, Ord, PartialEq, Eq)]`

### 2. claudefs-mgmt/src/resource_limiter.rs (1 error)
- **Line 598:** Use of moved value `enforcer`
  - Line 594 gets `enforcer` as `Option<&mut QuotaEnforcer>`, then it's moved
  - Fix: Call methods in sequence without re-binding between uses

### 3. claudefs-security/src/storage_allocator_uring_security_tests.rs (1 error)
- **Line 246:** Type mismatch in `handles.push(handle1)` — JoinHandle<Vec<BlockRef>> vs expected type
  - Context shows handles is wrong type
  - Fix: Check handles vec declaration and align types

### 4. claudefs-security/src/transport_auth_tls_security_tests.rs (3 errors)
- **Line 369:** Cannot access private field `service.tokens`
  - EnrollmentService::tokens is private
  - Fix: Add public accessor method to EnrollmentService, OR restructure test to use public API
- **Line 440:** Cannot borrow `rl` as mutable (not declared mutable)
  - Fix: Change to `let mut rl = RevocationList::new();`

## Implementation Requirements

1. **Do NOT modify test structure** — only fix type mismatches and missing derives
2. **Add derives to structs if needed** (e.g., PartialOrd, Eq for EventSeverity)
3. **Fix variable declarations** (add `mut` where needed)
4. **For private field access**: Add public methods to EnrollmentService or RevocationList
5. **Run `cargo check` after to verify all 8 errors are gone**

## Expected Result
- `cargo check` passes with only warnings (unused functions/variables)
- `cargo test --lib` runs to completion (some tests may fail, but no compilation errors)
- All 8 errors resolved

## Files to modify
- crates/claudefs-mgmt/src/event_sink.rs
- crates/claudefs-mgmt/src/resource_limiter.rs
- crates/claudefs-security/src/storage_allocator_uring_security_tests.rs
- crates/claudefs-security/src/transport_auth_tls_security_tests.rs
- crates/claudefs-transport/src/conn_auth.rs (if adding public accessors to RevocationList/EnrollmentService)
- crates/claudefs-transport/src/enrollment.rs (if adding public accessors)
