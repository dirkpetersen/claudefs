# Dependency Audit Report

**Auditor:** A10 (Security Audit Agent)
**Date:** 2026-03-01
**Tool:** `cargo audit` (RustSec advisory database, 939 advisories)
**Scope:** All 360 transitive dependencies in Cargo.lock

## Summary

| Severity | Count |
|----------|-------|
| Vulnerabilities | 0 |
| Unsound | 2 |
| Unmaintained | 2 |
| **Total advisories** | **4** |

No active security vulnerabilities found. Two soundness issues and two unmaintained crates require attention.

---

## Advisory Details

### ADV-1: fuser 0.15.1 — Uninitialized Memory Read & Leak (UNSOUND)

| Field | Value |
|-------|-------|
| Advisory | RUSTSEC-2021-0154 |
| Severity | Unsound |
| Affected crate | `claudefs-fuse` |
| Description | Uninitialized memory read and leak in fuser crate |

**Impact:** The `fuser` crate (FUSE v3 bindings) has a known soundness issue with uninitialized memory. This affects the claudefs-fuse crate directly.

**Recommendation:** Monitor for updated fuser versions. Since fuser is the primary Rust FUSE library, there is no drop-in replacement. The unsoundness is in internal buffer handling and is mitigated by the kernel FUSE protocol guarantees.

**Risk:** MEDIUM — Affects FUSE client hot path. Mitigated by kernel FUSE protocol structure.

---

### ADV-2: lru 0.12.5 — IterMut Stacked Borrows Violation (UNSOUND)

| Field | Value |
|-------|-------|
| Advisory | RUSTSEC-2026-0002 |
| Severity | Unsound |
| Affected crate | `claudefs-fuse` (via lru cache) |
| Description | `IterMut` invalidates internal pointer, violating Stacked Borrows |

**Impact:** The `lru` crate used for FUSE client-side metadata caching has a soundness issue with mutable iterators. This could cause UB under Miri's Stacked Borrows model.

**Recommendation:** Upgrade to a patched version when available, or replace with `lru` fork / `quick_cache` / manual `HashMap + LinkedList`.

**Risk:** LOW — Affects mutable iteration only. Typical LRU usage (get/put) is unaffected.

---

### ADV-3: bincode 1.3.3 — Unmaintained (WARNING)

| Field | Value |
|-------|-------|
| Advisory | RUSTSEC-2025-0141 |
| Severity | Warning (unmaintained) |
| Affected crates | claudefs-transport, claudefs-storage, claudefs-meta, claudefs-repl, claudefs-tests |
| Description | bincode 1.x is no longer maintained |

**Impact:** bincode 1.x is a workspace dependency used for internal wire format serialization. It works correctly but will not receive bug fixes or security patches.

**Recommendation:** Plan migration to `bincode 2.x` (maintained) or `postcard` (no_std-friendly). This is a workspace-wide change requiring coordination across agents.

**Risk:** LOW — No known vulnerabilities. Unmaintained status is a long-term concern.

---

### ADV-4: rustls-pemfile 2.2.0 — Unmaintained (WARNING)

| Field | Value |
|-------|-------|
| Advisory | RUSTSEC-2025-0134 |
| Severity | Warning (unmaintained) |
| Affected crate | `claudefs-transport` |
| Description | rustls-pemfile is no longer maintained separately |

**Impact:** PEM file parsing for TLS certificates. Functionality has been absorbed into the `rustls` crate itself.

**Recommendation:** Migrate PEM parsing to `rustls::pem` when upgrading to rustls 0.24+. Minor change.

**Risk:** LOW — PEM parsing is simple and unlikely to have security issues.

---

## Action Items

| # | Priority | Advisory | Action | Owner |
|---|----------|----------|--------|-------|
| 1 | MEDIUM | ADV-1 (fuser) | Monitor for patched fuser version; add to A5 backlog | A5, A10 |
| 2 | LOW | ADV-2 (lru) | Upgrade or replace lru crate in claudefs-fuse | A5 |
| 3 | LOW | ADV-3 (bincode) | Plan migration to bincode 2.x or postcard | All agents |
| 4 | LOW | ADV-4 (rustls-pemfile) | Migrate to rustls built-in PEM parsing | A4 |

---

## Positive Findings

- **Zero active CVEs** across 360 dependencies
- **All crypto crates** (aes-gcm, chacha20poly1305, hkdf, sha2, rand) are current and well-maintained
- **TLS stack** (rustls 0.23) is current with no advisories
- **Core dependencies** (tokio, serde, thiserror, tracing) are all current
