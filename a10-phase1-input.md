# A10 Phase 1: Security Audit Infrastructure & Test Fixes

## Context

ClaudeFS is a distributed POSIX file system in Rust with 8 builder crates (A1-A8). A10 (Security Audit) is responsible for:
- Reviewing unsafe code in A1/A4/A5 (Phase 2+)
- Fuzzing network protocol and FUSE interface (Phase 2+)
- Cryptography implementation audit (Phase 2+)
- Dependency CVE scanning

Currently, the `claudefs-security` crate has extensive test infrastructure but several tests have **compilation errors** due to accessing private struct fields and using methods that don't exist or have changed signatures.

## Problem: Security Test Compilation Errors

### Error 1: Private field access in SessionManager
File: `crates/claudefs-security/src/meta_client_session_security_tests.rs`
Lines: 453, 635, 757-758, 789

The tests try to access `manager.sessions` which is now a private field. Tests need to use public APIs instead.

**Examples:**
```rust
// Line 453 - OLD (doesn't compile):
let s = manager.sessions.get(&session.session_id).unwrap();

// Lines 757-758 - OLD (doesn't compile):
let count1 = manager.sessions.len();
let count2 = manager.sessions.len();

// Line 789 - OLD (doesn't compile):
let sessions: Vec<_> = manager.sessions.iter()...
```

**Fix approach:**
Replace private field access with public API methods. Check if SessionManager exposes methods like `get_session()`, `list_sessions()`, `session_count()`, etc. Use those instead.

### Error 2: TraceLatencyStats missing p95_ns field
File: `crates/claudefs-security/src/transport_trace_aggregator_security_tests.rs`
Lines: 400-401

Tests reference `stats.p95_ns` but this field doesn't exist in `TraceLatencyStats`.

**Fix approach:**
Update the test to use fields that actually exist in `TraceLatencyStats`. Check what percentile fields are available and adjust assertions.

### Error 3: HashMap collection from reference iterator
File: `crates/claudefs-security/src/transport_trace_aggregator_security_tests.rs`
Line: 39

Cannot build HashMap from iterator over references.

**Current code:**
```rust
let unique_count = trace_ids.iter().collect::<HashMap<_, ()>>().len();
```

**Fix approach:**
Convert references to owned values before collecting. Either clone or change to use references directly with a HashSet.

### Error 4: TokenBucket missing refill_at_ns method
File: `crates/claudefs-security/src/transport_bandwidth_shaper_security_tests.rs`
Line: 137

`TokenBucket::refill_at_ns()` method doesn't exist.

**Fix approach:**
Replace with correct method name or use alternative approach for testing token refill at specific time. Check TokenBucket public API.

### Error 5: BandwidthStats is not Option type
File: `crates/claudefs-security/src/transport_bandwidth_shaper_security_tests.rs`
Line: 402

Code calls `.is_some()` on `BandwidthStats` but it's not an Option.

**Current code:**
```rust
assert!(stats1.is_some() && stats2.is_some(), ...);
```

**Fix approach:**
BandwidthStats is likely a direct struct, not wrapped in Option. Remove `.is_some()` calls or check if the structs are non-null directly.

## Task: Fix All Compilation Errors

**Scope:**
1. Fix all 11 compilation errors in claudefs-security tests
2. Ensure tests still validate the intended security properties
3. Use public APIs only (no private field access)
4. Match actual struct signatures from transport and meta crates

**Constraints:**
- Do NOT modify the transport or meta crates themselves
- Only fix tests in `claudefs-security`
- Keep the test logic intact, just fix the compilation errors
- Tests should still verify the security properties they were designed for

**Files to modify:**
1. `crates/claudefs-security/src/meta_client_session_security_tests.rs` — Fix 5 private field access errors
2. `crates/claudefs-security/src/transport_trace_aggregator_security_tests.rs` — Fix HashMap collection and p95_ns errors
3. `crates/claudefs-security/src/transport_bandwidth_shaper_security_tests.rs` — Fix TokenBucket and BandwidthStats errors

**Output format:**
Provide the complete fixed source code for each file. Include inline comments explaining what changed and why.
