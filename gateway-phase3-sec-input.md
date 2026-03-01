# A7 Gateway Phase 3 Security Hardening

Fix five security findings (FINDING-16 through FINDING-20) identified by A10 (Security Audit agent)
in the `claudefs-gateway` crate. You MUST modify exactly two files:
1. `crates/claudefs-gateway/Cargo.toml`
2. `crates/claudefs-gateway/src/token_auth.rs`
3. `crates/claudefs-gateway/src/auth.rs`

All 608 existing tests must still pass after your changes. You will need to update some tests
to match the new API. Do NOT break any test — update or add tests as needed.

---

## Changes Required to `crates/claudefs-gateway/Cargo.toml`

Add two workspace dependencies to the `[dependencies]` section:
```toml
rand.workspace = true
sha2.workspace = true
```

The workspace already has `rand = "0.8"` and `sha2 = "0.10"` defined. Just reference them.

---

## Changes Required to `crates/claudefs-gateway/src/token_auth.rs`

### FINDING-16 (HIGH): Replace predictable token generation with CSPRNG

Current bad code at line 163-165:
```rust
pub fn generate_token(uid: u32, counter: u64) -> String {
    format!("{:016x}{:08x}", counter, uid)
}
```

Fix: Replace with CSPRNG-based generation. Remove the `uid` and `counter` parameters entirely
(they are no longer needed). Use `rand::rngs::OsRng` to generate 32 random bytes, then hex-encode:
```rust
pub fn generate_token() -> String {
    use rand::RngCore;
    let mut rng = rand::rngs::OsRng;
    let mut bytes = [0u8; 32];
    rng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
```
The returned token is 64 hex characters long.

Also update the test `test_token_auth_generate_token` to call the new no-argument signature
and verify the token is 64 hex chars and doesn't panic (not a specific hex value):
```rust
#[test]
fn test_token_auth_generate_token() {
    let token = TokenAuth::generate_token();
    assert_eq!(token.len(), 64);
    assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    // Each call should produce a unique token
    let token2 = TokenAuth::generate_token();
    assert_ne!(token, token2);
}
```

### FINDING-18 (MEDIUM): Store hashed tokens instead of plaintext in HashMap

Current bad code: `tokens: Mutex<HashMap<String, AuthToken>>` stores plaintext token strings as keys.

Fix: Store SHA-256 hashes of token strings as HashMap keys. When a caller registers a token,
hash the plaintext and use the hash as the key. When validating, hash the input and look up the hash.

**Changes to `AuthToken`:**
- The `token` field should store the SHA-256 hex hash of the original plaintext token, not the plaintext itself.
- Change `AuthToken::new(token_plaintext: &str, ...)` to compute the hash internally:

```rust
fn sha256_hex(input: &str) -> String {
    use sha2::{Sha256, Digest};
    let hash = Sha256::digest(input.as_bytes());
    hash.iter().map(|b| format!("{:02x}", b)).collect()
}
```

In `AuthToken::new`, store `token: sha256_hex(token)` instead of `token: token.to_string()`.

**Changes to `TokenAuth` methods:**
- `register`: Insert with `token.token.clone()` as key (the hash, not plaintext) — unchanged
- `validate(token_str: &str, now: u64)`: Hash `token_str` before lookup: `tokens.get(&sha256_hex(token_str))`
- `revoke(token_str: &str)`: Hash before remove: `tokens.remove(&sha256_hex(token_str))`
- `exists(token_str: &str)`: Hash before check: `tokens.contains_key(&sha256_hex(token_str))`

The existing tests all use string literals like `auth.exists("token1")`. With the new hashing approach:
- `AuthToken::new("token1", 1000, 100, "user1")` stores `sha256("token1")` as the HashMap key
- `auth.exists("token1")` hashes "token1" and looks it up → works correctly
- All existing behavioral tests should still pass without modification

**Important:** The test `test_auth_token_new` currently asserts:
```rust
assert_eq!(token.token, "abc123");
```
This test MUST be updated since `token.token` now stores the hash. Change it to:
```rust
let token = AuthToken::new("abc123", 1000, 100, "testuser");
// token.token is now the SHA-256 hash of "abc123", not "abc123" itself
assert_ne!(token.token, "abc123");
assert_eq!(token.token.len(), 64); // SHA-256 hex is 64 chars
assert_eq!(token.uid, 1000);
assert_eq!(token.gid, 100);
assert_eq!(token.name, "testuser");
assert_eq!(token.expires_at, 0);
```

Similarly, `test_auth_token_with_expiry` tests `token.expires_at` which is unchanged.
`test_auth_token_with_permissions` tests permissions, unchanged.
`test_auth_token_is_expired`, `test_auth_token_never_expires` etc. are unchanged.

### FINDING-19 (MEDIUM): Handle Mutex poisoning gracefully

Current bad code: All `.lock().unwrap()` calls panic if the mutex is poisoned.

Fix: Replace all `.lock().unwrap()` with `.lock().unwrap_or_else(|e| e.into_inner())` so
a panic in one thread doesn't permanently disable the entire authentication system.

This change is purely mechanical — no new dependencies required. Update every occurrence of
`self.tokens.lock().unwrap()` in `TokenAuth` impl.

---

## Changes Required to `crates/claudefs-gateway/src/auth.rs`

### FINDING-17/20: Add configurable root squashing for NFS AUTH_SYS

AUTH_SYS allows clients to claim any UID/GID including root (uid=0). Add configurable squashing
so `AuthCred::effective_uid()` and `AuthCred::effective_gid()` return squashed values.

Add a new type `SquashConfig` and new methods on `AuthCred`:

```rust
/// Root squash policy for NFS AUTH_SYS
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SquashPolicy {
    /// No squashing — trust client-supplied UIDs (unsafe on untrusted networks)
    None,
    /// Root squash: map uid=0/gid=0 to nobody:nogroup (default)
    RootSquash,
    /// All squash: map all UIDs/GIDs to nobody:nogroup
    AllSquash,
}

impl Default for SquashPolicy {
    fn default() -> Self {
        SquashPolicy::RootSquash  // safe default
    }
}
```

Add these methods to `AuthCred`:
```rust
/// Effective UID after applying squash policy (default: RootSquash)
pub fn effective_uid(&self, policy: SquashPolicy) -> u32 {
    let raw = self.uid();
    match policy {
        SquashPolicy::None => raw,
        SquashPolicy::RootSquash => if raw == 0 { NOBODY_UID } else { raw },
        SquashPolicy::AllSquash => NOBODY_UID,
    }
}

/// Effective GID after applying squash policy (default: RootSquash)
pub fn effective_gid(&self, policy: SquashPolicy) -> u32 {
    let raw = self.gid();
    match policy {
        SquashPolicy::None => raw,
        SquashPolicy::RootSquash => if raw == 0 { NOBODY_GID } else { raw },
        SquashPolicy::AllSquash => NOBODY_GID,
    }
}
```

**Do NOT change the existing `uid()` and `gid()` methods** — they return the raw values as before.
The new `effective_uid()` and `effective_gid()` are additive.

### Additional: machinename length limit per RFC 1831

In `AuthSysCred::decode_xdr`, after decoding `machinename`, add a length check:
```rust
const AUTH_SYS_MAX_MACHINENAME_LEN: usize = 255;
```
After the `let machinename = dec.decode_string()?;` line, add:
```rust
if machinename.len() > AUTH_SYS_MAX_MACHINENAME_LEN {
    return Err(GatewayError::ProtocolError {
        reason: format!(
            "machinename too long: {} > {}",
            machinename.len(), AUTH_SYS_MAX_MACHINENAME_LEN
        ),
    });
}
```

**New tests to add to `auth.rs`:**

```rust
#[test]
fn test_squash_policy_default_is_root_squash() {
    assert_eq!(SquashPolicy::default(), SquashPolicy::RootSquash);
}

#[test]
fn test_effective_uid_root_squash_squashes_root() {
    let cred = AuthSysCred {
        stamp: 1, machinename: "host".to_string(),
        uid: 0, gid: 0, gids: vec![],
    };
    let opaque = OpaqueAuth { flavor: AUTH_SYS, body: cred.encode_xdr() };
    let auth = AuthCred::from_opaque_auth(&opaque);
    assert_eq!(auth.effective_uid(SquashPolicy::RootSquash), NOBODY_UID);
    assert_eq!(auth.effective_gid(SquashPolicy::RootSquash), NOBODY_GID);
}

#[test]
fn test_effective_uid_root_squash_passes_nonroot() {
    let cred = AuthSysCred {
        stamp: 1, machinename: "host".to_string(),
        uid: 1000, gid: 1000, gids: vec![],
    };
    let opaque = OpaqueAuth { flavor: AUTH_SYS, body: cred.encode_xdr() };
    let auth = AuthCred::from_opaque_auth(&opaque);
    assert_eq!(auth.effective_uid(SquashPolicy::RootSquash), 1000);
    assert_eq!(auth.effective_gid(SquashPolicy::RootSquash), 1000);
}

#[test]
fn test_effective_uid_all_squash() {
    let cred = AuthSysCred {
        stamp: 1, machinename: "host".to_string(),
        uid: 1000, gid: 1000, gids: vec![],
    };
    let opaque = OpaqueAuth { flavor: AUTH_SYS, body: cred.encode_xdr() };
    let auth = AuthCred::from_opaque_auth(&opaque);
    assert_eq!(auth.effective_uid(SquashPolicy::AllSquash), NOBODY_UID);
    assert_eq!(auth.effective_gid(SquashPolicy::AllSquash), NOBODY_GID);
}

#[test]
fn test_effective_uid_none_policy_passes_root() {
    let cred = AuthSysCred {
        stamp: 1, machinename: "host".to_string(),
        uid: 0, gid: 0, gids: vec![],
    };
    let opaque = OpaqueAuth { flavor: AUTH_SYS, body: cred.encode_xdr() };
    let auth = AuthCred::from_opaque_auth(&opaque);
    assert_eq!(auth.effective_uid(SquashPolicy::None), 0);
    assert_eq!(auth.effective_gid(SquashPolicy::None), 0);
}

#[test]
fn test_machinename_length_limit() {
    let long_name = "a".repeat(256);
    let cred = AuthSysCred {
        stamp: 1, machinename: long_name.clone(),
        uid: 1000, gid: 1000, gids: vec![],
    };
    // Encode manually (encode_xdr doesn't check length)
    let encoded = cred.encode_xdr();
    // Decoding should fail because machinename > 255 bytes
    let result = AuthSysCred::decode_xdr(&encoded);
    assert!(result.is_err());
}

#[test]
fn test_machinename_max_length_ok() {
    let max_name = "a".repeat(255);
    let cred = AuthSysCred {
        stamp: 1, machinename: max_name,
        uid: 1000, gid: 1000, gids: vec![],
    };
    let encoded = cred.encode_xdr();
    let result = AuthSysCred::decode_xdr(&encoded);
    assert!(result.is_ok());
}
```

---

## Summary of Changes

1. `Cargo.toml`: Add `rand.workspace = true` and `sha2.workspace = true`
2. `token_auth.rs`:
   - Fix `generate_token()` to use CSPRNG (no args)
   - Fix `AuthToken::new()` to hash the token with SHA-256 on construction
   - Fix all `lock().unwrap()` → `lock().unwrap_or_else(|e| e.into_inner())`
   - Update `validate()`, `revoke()`, `exists()` to hash input before lookup
   - Update test `test_auth_token_new` to not assert `token.token == "abc123"`
   - Update test `test_token_auth_generate_token` to verify format (64 hex chars), not specific value
3. `auth.rs`:
   - Add `SquashPolicy` enum with `None`, `RootSquash` (default), `AllSquash` variants
   - Add `effective_uid(policy)` and `effective_gid(policy)` methods to `AuthCred`
   - Add `AUTH_SYS_MAX_MACHINENAME_LEN = 255` constant and check in `decode_xdr`
   - Add 7 new tests for squashing and machinename length

All 608 existing tests + new tests must pass. Run `cargo test -p claudefs-gateway` to verify.
