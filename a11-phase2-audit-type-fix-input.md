# Fix type mismatch in phase2_audit.rs test

## Problem

The test `test_nonce_is_not_counter_based()` in `crates/claudefs-security/src/phase2_audit.rs` has a type mismatch error:

```
error[E0308]: mismatched types
  --> crates/claudefs-security/src/phase2_audit.rs:85:40
   |
85 |                     d = d.wrapping_add(r.nonce.0[i]).wrapping_sub(prev.0[i]);
   |                           ------------ ^^^^^^^^^^^^ expected `u32`, found `u8`
```

The problem is that `r.nonce.0[i]` and `prev.0[i]` are `u8` values, but `d` is `u32`. The bytes need to be cast to `u32` before arithmetic operations.

## Current code (lines 83-86)

```rust
let mut d = 0u32;
for i in 0..12 {
    d = d.wrapping_add(r.nonce.0[i]).wrapping_sub(prev.0[i]);
}
```

## Fix needed

Cast the `u8` values to `u32` before arithmetic:

```rust
let mut d = 0u32;
for i in 0..12 {
    d = d.wrapping_add(r.nonce.0[i] as u32).wrapping_sub(prev.0[i] as u32);
}
```

## Files to fix

- `crates/claudefs-security/src/phase2_audit.rs` — line 85

## Verification

After the fix:
- Run `cargo test -p claudefs-security --lib --no-run` — should succeed without E0308 error
- Then run `cargo test -p claudefs-security` to run the actual tests
