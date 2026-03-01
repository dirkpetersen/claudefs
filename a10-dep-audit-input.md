# Task: Create dependency audit module for claudefs-security

## Context
You are working on the `claudefs-security` crate at `crates/claudefs-security/src/dep_audit.rs`.
This is a NEW file to add a dependency audit module as part of the Phase 3 security audit.

## Requirements

Create a module that tests for dependency security issues discovered via `cargo audit`:

### Findings to encode as tests:

1. **RUSTSEC-2025-0141**: `bincode 1.3.3` is unmaintained — used by storage, transport, repl, meta
2. **RUSTSEC-2025-0134**: `rustls-pemfile 2.2.0` is unmaintained — used by transport
3. **RUSTSEC-2021-0154**: `fuser 0.15.1` has uninitialized memory read & leak (unsound)
4. **RUSTSEC-2026-0002**: `lru 0.12.5` IterMut invalidates internal pointer (unsound)

### Additional dependency checks to implement:

5. Verify no `openssl` dependency exists (we use RustCrypto/rustls)
6. Verify `rand` uses a CSPRNG (OsRng or ThreadRng, not StdRng with fixed seed)
7. Verify `aes-gcm` and `chacha20poly1305` versions are current
8. Verify total number of unique transitive dependencies is within acceptable bounds
9. Verify no known-bad crates are in the dependency tree (e.g., `remove_dir_all` older than 0.8)
10. Verify unsafe code in dependencies is bounded — check `fuser` as the main unsafe dep

### Module structure:

```rust
//! Phase 3 dependency CVE sweep and supply chain audit.
//!
//! Findings: FINDING-DEP-01 through FINDING-DEP-10
//!
//! This module documents dependency security findings and verifies
//! that critical security properties of the dependency tree are maintained.

#[cfg(test)]
mod tests {
    // Group 1: Unmaintained dependencies (tracked advisories)
    // Group 2: Unsound dependencies (tracked advisories)
    // Group 3: Supply chain properties
}
```

### Implementation notes:

- Tests should be `#[test]` functions (not async)
- For unmaintained/unsound crates, write tests that document the advisory and verify we're aware of it. Mark them as `#[ignore]` with comments explaining the advisory status.
- For supply chain property tests, verify things like: crate versions match expected, no unexpected crates present, etc.
- Use `std::process::Command` to run `cargo metadata` for dependency tree inspection
- Include at least 15 tests total

### Dependencies available:
- serde_json (for parsing cargo metadata output)
- std::process::Command (for running cargo commands)

## Output
Output ONLY the complete Rust file content for `crates/claudefs-security/src/dep_audit.rs`.
Do not include markdown code fences or explanations.
