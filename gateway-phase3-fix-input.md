# Fix Two Broken Security Tests in claudefs-security

The file `crates/claudefs-security/src/gateway_auth_tests.rs` has two tests that were
documenting pre-fix behavior but are now failing because the security fixes have been applied.

## Context

A7 fixed FINDING-16 through FINDING-20. Two tests in gateway_auth_tests.rs now fail:

1. `finding_18_token_enumeration_possible` — was documenting that token strings were returned
   in cleartext. Now tokens are stored as SHA-256 hashes, so `tokens[0].token` is the hash,
   NOT the original string "token-a".

2. `auth_sys_long_machinename` — was documenting that long machinenames were NOT rejected.
   Now machinenames > 255 bytes ARE rejected with a ProtocolError. The test needs to verify
   the NEW behavior (rejection of overly long names).

## Required Changes

Please modify ONLY `crates/claudefs-security/src/gateway_auth_tests.rs`.

### Fix 1: `finding_18_token_enumeration_possible`

Current (failing) code:
```rust
#[test]
fn finding_18_token_enumeration_possible() {
    let auth = TokenAuth::new();
    auth.register(AuthToken::new("token-a", 1000, 100, "user1"));
    auth.register(AuthToken::new("token-b", 2000, 200, "user2"));

    let tokens = auth.tokens_for_user(1000);
    assert_eq!(tokens.len(), 1);
    assert_eq!(
        tokens[0].token, "token-a",
        "Token string returned in cleartext"
    );
}
```

New code (documents FIXED behavior — tokens stored as hashes, not cleartext):
```rust
#[test]
fn finding_18_token_stored_as_hash() {
    // FINDING-18 FIXED: tokens are now stored as SHA-256 hashes, not cleartext
    let auth = TokenAuth::new();
    auth.register(AuthToken::new("token-a", 1000, 100, "user1"));
    auth.register(AuthToken::new("token-b", 2000, 200, "user2"));

    let tokens = auth.tokens_for_user(1000);
    assert_eq!(tokens.len(), 1);
    // Token field now stores the SHA-256 hash (64 hex chars), not the plaintext
    assert_eq!(tokens[0].token.len(), 64, "Token stored as 64-char SHA-256 hex hash");
    assert_ne!(tokens[0].token, "token-a", "Token NOT stored in cleartext");
    // Validate still works with plaintext
    assert!(auth.validate("token-a", 0).is_some());
}
```

### Fix 2: `auth_sys_long_machinename`

Current (failing) code:
```rust
#[test]
fn auth_sys_long_machinename() {
    let long_name = "a".repeat(10000);
    let cred = AuthSysCred {
        stamp: 1,
        machinename: long_name.clone(),
        uid: 1000,
        gid: 1000,
        gids: vec![],
    };
    let encoded = cred.encode_xdr();
    let decoded = AuthSysCred::decode_xdr(&encoded).unwrap();
    assert_eq!(
        decoded.machinename.len(),
        10000,
        "No limit on machinename length — potential DoS"
    );
}
```

New code (documents FIXED behavior — names > 255 bytes now rejected):
```rust
#[test]
fn auth_sys_long_machinename_rejected() {
    // FIXED: machinenames > 255 bytes now rejected per RFC 1831
    let long_name = "a".repeat(10000);
    let cred = AuthSysCred {
        stamp: 1,
        machinename: long_name.clone(),
        uid: 1000,
        gid: 1000,
        gids: vec![],
    };
    let encoded = cred.encode_xdr();
    // decode_xdr now rejects machinenames > 255 bytes with a ProtocolError
    let result = AuthSysCred::decode_xdr(&encoded);
    assert!(result.is_err(), "Machinename > 255 bytes should be rejected");

    // Exactly 255 bytes is OK
    let ok_name = "b".repeat(255);
    let ok_cred = AuthSysCred {
        stamp: 1,
        machinename: ok_name,
        uid: 1000,
        gid: 1000,
        gids: vec![],
    };
    let ok_encoded = ok_cred.encode_xdr();
    assert!(AuthSysCred::decode_xdr(&ok_encoded).is_ok());
}
```

## Summary

- Rename `finding_18_token_enumeration_possible` → `finding_18_token_stored_as_hash` and update assertions
- Rename `auth_sys_long_machinename` → `auth_sys_long_machinename_rejected` and update assertions

All other tests in the file should remain unchanged.

After making changes, run `cargo test -p claudefs-security -- gateway_auth_tests` to verify
both updated tests pass. The total test count for claudefs-security should stay at ≥ 202 passing
(these 2 tests change from failing to passing).
