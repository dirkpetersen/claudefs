I'll first read the current lib.rs to understand its structure, then create the required files.
Let me check the actual APIs in the claudefs-reduce and claudefs-repl crates:
Now I need to update the test files with correct imports. The modules are accessible but need explicit paths:
All tests pass. Here's a summary of what was created:

**Files Created/Updated:**

1. **`crates/claudefs-security/src/lib.rs`** - Added two new module declarations:
   - `reduce_security_tests`
   - `repl_security_tests`

2. **`crates/claudefs-security/src/reduce_security_tests.rs`** - 20 security tests covering:
   - GC Safety (5 tests): sweep behavior, clear marks, TOCTOU, refcount underflow, stats accuracy
   - Key Management Security (6 tests): generate DEK without key, wrap/unwrap roundtrip, clear history, scheduler behavior, double schedule, tampered ciphertext
   - Encryption Security (4 tests): nonce uniqueness, empty plaintext, wrong key, deterministic key derivation
   - Checksum and Segment Security (5 tests): tampered data, segment integrity, snapshot limits, clone nonexistent, compression roundtrip

3. **`crates/claudefs-security/src/repl_security_tests.rs`** - 20 security tests covering:
   - Journal Integrity (5 tests): CRC validation, CRC collisions, empty payload, position tracking, shard filtering
   - Batch Authentication (5 tests): sign/verify roundtrip, tampered entry, replay protection, wrong key, zero tag
   - Site Identity and TLS (5 tests): fingerprint mismatch, fingerprint bypass, TLS required, TLS testonly, empty certs
   - Conflict Resolution and Failover (5 tests): LWW resolution, equal timestamps, fencing token, WAL reset, rate limiter

**Test Results:**
- `cargo test -p claudefs-security reduce_security_tests`: 20 passed
- `cargo test -p claudefs-security repl_security_tests`: 20 passed
- Full test suite: 698 passed, 0 failed
