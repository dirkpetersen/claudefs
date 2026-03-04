# Task: Fix Security Findings in batch_auth.rs

Fix the following critical security findings in `crates/claudefs-repl/src/batch_auth.rs`.

## Current File Content

The file currently contains:
1. A hand-rolled SHA-256 implementation (FIPS 180-4) at lines 143-230 
2. A hand-rolled HMAC-SHA256 implementation (RFC 2104) at lines 232-251
3. A BatchAuthKey Drop impl that uses a manual byte-zeroing loop (lines 33-38) which can be optimized away by the compiler
4. A test at lines 294-301 that uses unsafe to read freed memory (UB)

## Required Changes

### SEC-01: Replace hand-rolled SHA-256 and HMAC-SHA256 with crate implementations

Replace the `sha256()` and `hmac_sha256()` functions with the `sha2` and `hmac` crates.

The new `hmac_sha256` function should use:
```rust
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

fn hmac_sha256(key: &[u8; 32], message: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC key length is always valid");
    mac.update(message);
    let result = mac.finalize();
    let bytes = result.into_bytes();
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    out
}
```

Remove the entire `sha256()` function (lines 143-230) and the old `hmac_sha256()` function (lines 232-251).

### SEC-02: Replace manual zeroization with zeroize crate

Replace the Drop impl (lines 33-38):
```rust
impl Drop for BatchAuthKey {
    fn drop(&mut self) {
        for b in self.bytes.iter_mut() {
            *b = 0;
        }
    }
}
```

With the `zeroize` crate's `Zeroize` and `ZeroizeOnDrop` derives:
```rust
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct BatchAuthKey {
    bytes: [u8; 32],
}
```

Remove the manual Drop impl entirely.

### SEC-09: Remove unsafe test

Replace the `test_batch_key_zero_on_drop` test (lines 294-301) which reads freed memory. Replace it with a test that verifies the `Zeroize` trait is implemented:
```rust
#[test]
fn test_batch_key_zeroize() {
    use zeroize::Zeroize;
    let mut key = BatchAuthKey::from_bytes([0x55; 32]);
    key.zeroize();
    assert_eq!(*key.as_bytes(), [0u8; 32]);
}
```

## Important Constraints

1. The `hmac` and `sha2` and `zeroize` crates are already workspace dependencies — do NOT add new Cargo.toml entries
2. Keep ALL existing tests that don't involve UB (test_sha256_known_hash, test_sha256_empty_string etc. can be removed since we're removing the sha256 function)
3. Keep the `constant_time_compare` function — it's still needed
4. Keep the `BatchAuthenticator`, `BatchTag`, `AuthResult` types unchanged
5. Keep the `sign_batch` and `verify_batch` methods unchanged
6. Update the `test_sha256_*` and `test_hmac_*` tests to reference the new hmac_sha256 function
7. The file should still compile and all tests should pass

## Output

Output the COMPLETE replacement file content for `crates/claudefs-repl/src/batch_auth.rs`. Include ALL the existing code (BatchTag, AuthResult, BatchAuthenticator, all tests) with only the changes described above.
