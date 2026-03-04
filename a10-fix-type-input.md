# Fix type annotation in phase2_audit.rs

Edit the file `crates/claudefs-security/src/phase2_audit.rs`.

On line 7, the import is:
```rust
    use claudefs_reduce::encryption::{
        derive_chunk_key, encrypt, random_nonce, EncryptionAlgorithm, EncryptionKey,
    };
```

Add `Nonce` to this import so it becomes:
```rust
    use claudefs_reduce::encryption::{
        derive_chunk_key, encrypt, random_nonce, EncryptionAlgorithm, EncryptionKey, Nonce,
    };
```

Then on line 79, change:
```rust
        let mut p = None;
```
to:
```rust
        let mut p: Option<Nonce> = None;
```

Make only these two changes. Do not modify anything else.
