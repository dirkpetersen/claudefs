# Unsafe Code Audit Report

**Auditor:** A10 (Security Audit Agent)
**Date:** 2026-03-01
**Scope:** All `unsafe` blocks in claudefs workspace
**Status:** Phase 2 initial audit

## Summary

| Metric | Value |
|--------|-------|
| Total `.rs` files scanned | 152 |
| Files containing `unsafe` | 3 |
| Total `unsafe` blocks | 8 |
| `unsafe impl` declarations | 2 (Send + Sync) |
| FFI calls (libc) | 4 |
| io_uring SQE pushes | 3 |
| Raw pointer operations | 1 (zerocopy allocator) |

**Risk assessment: LOW** — All unsafe code is confined to well-established FFI boundaries (libc, io-uring). No unsafe code in business logic crates.

## Crates with Unsafe Code

Only 3 of 8 crates contain unsafe code, matching the architecture's `unsafe` budget (A1, A4):

| Crate | File | Unsafe Blocks | Purpose |
|-------|------|--------------|---------|
| claudefs-storage | `uring_engine.rs` | 7 | io_uring ops, libc FFI |
| claudefs-storage | `device.rs` | 1 | libc::close in Drop |
| claudefs-transport | `zerocopy.rs` | 1 | Aligned buffer allocation |

**Clean crates (no unsafe):** claudefs-meta, claudefs-reduce, claudefs-fuse, claudefs-repl, claudefs-gateway, claudefs-mgmt

---

## Detailed Findings

### 1. claudefs-storage/src/uring_engine.rs

#### U1: Send + Sync trait implementations (lines 101-102)

```rust
unsafe impl Send for UringIoEngine {}
unsafe impl Sync for UringIoEngine {}
```

**Justification:** UringIoEngine wraps all mutable state in `Mutex`/`RwLock`, making it thread-safe. The io_uring instance itself is accessed through synchronized paths.

**Verdict:** ACCEPTABLE — Interior mutability properly guarded.

**Recommendation:** Add `// SAFETY:` comment documenting which fields require synchronization.

---

#### U2: libc::open FFI call (line 149)

```rust
let fd = unsafe { libc::open(c_path.as_ptr(), flags, 0o644) };
```

**Context:** Opens NVMe device files with optional O_DIRECT.

**Safety checklist:**
- [x] Path is valid CString (null-terminated)
- [x] Flags are valid POSIX constants (O_RDWR, O_DIRECT)
- [x] Return value checked (fd < 0 → error)
- [x] Mode bits reasonable (0o644)

**Verdict:** SAFE — Standard POSIX FFI pattern with proper error handling.

---

#### U3-U5: io_uring submission queue pushes (lines 209, 241, 268)

```rust
unsafe { sq.push(&read_op).map_err(...)? }   // Read
unsafe { sq.push(&write_op).map_err(...)? }  // Write
unsafe { sq.push(&fsync_op).map_err(...)? }  // Fsync
```

**Safety checklist:**
- [x] File descriptors are valid (from successful libc::open)
- [x] Buffer pointers come from Vec (valid for duration of operation)
- [x] Sizes are bounded by Vec capacity
- [x] Errors from push are propagated

**Concern:** Buffer lifetime — the Vec backing the buffer must outlive the io_uring operation. Current code awaits completion synchronously via `cq.next()`, so the buffer is alive during the entire operation.

**Verdict:** SAFE — Buffers outlive operations due to synchronous completion waiting.

**Recommendation:** If the engine moves to truly async io_uring (multi-CQE), buffer pinning must be added to prevent use-after-free.

---

#### U6: libc::fallocate (line 365-372)

```rust
let ret = unsafe {
    libc::fallocate(fd, libc::FALLOC_FL_PUNCH_HOLE | libc::FALLOC_FL_KEEP_SIZE,
                    byte_offset as libc::off_t, length as libc::off_t)
};
```

**Safety checklist:**
- [x] fd is valid (from register_device)
- [x] Flags are standard POSIX constants
- [x] Offset/length are cast from usize (safe for off_t)
- [x] Return value checked (ret != 0 → error)

**Verdict:** SAFE — Standard POSIX syscall wrapper.

---

#### U7: libc::close in Drop (line 399)

```rust
for (&_device_idx, &fd) in fds.iter() {
    unsafe { libc::close(fd) };
}
```

**Safety checklist:**
- [x] fd values are from successful register_device calls
- [x] Close is idempotent (double-close is not ideal but not UB)
- [ ] No check that fd hasn't been closed elsewhere

**Verdict:** ACCEPTABLE — Low risk. Consider using `OwnedFd` (Rust std since 1.63) to get automatic close-on-drop with no double-close risk.

**Recommendation:** Migrate to `std::os::fd::OwnedFd` for RAII file descriptor management.

---

### 2. claudefs-storage/src/device.rs

#### U8: libc::close in ManagedDevice::Drop (line 288-290)

```rust
impl Drop for ManagedDevice {
    fn drop(&mut self) {
        if let Some(fd) = self.fd {
            unsafe { libc::close(fd); }
        }
    }
}
```

**Verdict:** SAFE — Guarded by Option, only closes if fd was set.

**Recommendation:** Same as U7 — migrate to `OwnedFd`.

---

### 3. claudefs-transport/src/zerocopy.rs

#### U9: Manual memory allocation (line 150-156)

```rust
let layout = std::alloc::Layout::from_size_align(size, self.config.alignment)
    .expect("Invalid layout");
let data = unsafe {
    let ptr = std::alloc::alloc(layout);
    if ptr.is_null() {
        std::alloc::handle_alloc_error(layout);
    }
    std::slice::from_raw_parts_mut(ptr, size).to_vec()
};
```

**Safety checklist:**
- [x] Layout validated before allocation (from_size_align)
- [x] Null check on returned pointer
- [x] Alignment is configurable (default 4096 = page size)
- [x] Slice immediately converted to Vec (takes ownership)

**Concern:** The `from_raw_parts_mut` creates a mutable slice over uninitialized memory. The `.to_vec()` call reads this uninitialized memory to copy it. This is technically undefined behavior — reading uninitialized memory via `from_raw_parts_mut`.

**Verdict:** POTENTIAL UB — Reading uninitialized memory. Should use `alloc_zeroed` or fill with zeros before creating the slice.

**Recommendation:** Replace `alloc` with `alloc_zeroed` to avoid reading uninitialized bytes:
```rust
let ptr = std::alloc::alloc_zeroed(layout);
```

---

## Cross-Cutting Findings

### F1: No OwnedFd usage for file descriptors
Both `uring_engine.rs` and `device.rs` manage raw `RawFd` values manually. Using `std::os::fd::OwnedFd` (stable since Rust 1.63) would provide automatic close-on-drop and prevent double-close bugs.

### F2: Missing SAFETY comments
Most unsafe blocks lack `// SAFETY:` documentation comments. Rust convention (and clippy lint `undocumented_unsafe_blocks`) requires explaining why each unsafe block is sound.

### F3: Buffer lifetime coupling
io_uring operations assume synchronous completion (buffer alive until CQE). If the engine evolves to batched/async submission, buffer pinning (e.g., via `Pin<Box<[u8]>>`) will be critical to prevent use-after-free.

### F4: Uninitialized memory read in zerocopy allocator
`std::alloc::alloc` returns uninitialized memory. Creating a slice and calling `.to_vec()` reads this memory, which is UB. Use `alloc_zeroed` instead.

---

## Recommendations (Priority Order)

| Priority | Finding | Action | Owner |
|----------|---------|--------|-------|
| HIGH | F4 | Replace `alloc` with `alloc_zeroed` in zerocopy.rs | A4 |
| MEDIUM | F1 | Migrate to `OwnedFd` for RAII fd management | A1 |
| MEDIUM | F2 | Add `// SAFETY:` comments to all unsafe blocks | A1, A4 |
| LOW | F3 | Document buffer lifetime requirements for future async io_uring | A1 |

---

## Methodology

1. Searched all `.rs` files for `unsafe` keyword
2. Searched for `std::mem::transmute`, raw pointer patterns (`*const`, `*mut`), `extern "C"`
3. Read full context around each unsafe block (20+ lines)
4. Verified safety invariants against Rust reference
5. Checked for OWASP-relevant patterns (injection, buffer overflow, use-after-free)
6. Cross-referenced with architecture docs (decisions.md D1-D10)

## Next Steps

- [ ] File GitHub issues for HIGH/MEDIUM findings
- [ ] Re-audit after fixes are applied
- [ ] Expand to fuzzing of io_uring operation sequences
- [ ] Phase 3: Full penetration test of all unsafe boundaries
