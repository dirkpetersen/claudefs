# Task: Fix mTLS enforcement in claudefs-transport/src/tls.rs

## Context
This is the ClaudeFS distributed filesystem. Per architecture decision D7, all inter-node communication uses mTLS with cluster-issued certificates.

**SECURITY FINDING SEC-03 (HIGH):** The `TlsAcceptor::new()` method always calls `.with_no_client_auth()` regardless of the `config.require_client_auth` flag. This means mTLS is never enforced on the server side.

## Current TlsAcceptor::new() (lines 117-131)

```rust
pub fn new(config: &TlsConfig) -> Result<Self> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let certs = load_certs_from_pem(&config.cert_chain_pem)?;
    let key = load_private_key_from_pem(&config.private_key_pem)?;

    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()  // <-- BUG: ignores config.require_client_auth
        .with_single_cert(certs, key)
        .map_err(|e| TransportError::TlsError {
            reason: format!("failed to set server cert: {}", e),
        })?;

    let inner = TlsAcceptorInner::from(Arc::new(server_config));
    Ok(Self { inner })
}
```

## Dependencies already available

- `rustls = "0.23"`
- `rustls::server::WebPkiClientVerifier`
- `rustls::RootCertStore`

## Requirements

Fix the `TlsAcceptor::new()` method to:

1. When `config.require_client_auth` is `true`:
   - Load CA certificates from `config.ca_cert_pem`
   - Build a `WebPkiClientVerifier` with those roots
   - Use `.with_client_cert_verifier(verifier)` instead of `.with_no_client_auth()`

2. When `config.require_client_auth` is `false`:
   - Keep existing behavior with `.with_no_client_auth()`

3. Keep all other code unchanged (TlsConnector, TlsStream, helpers, tests).

## Also fix SEC-04: File descriptor leak in uring_engine.rs

**File:** `crates/claudefs-storage/src/uring_engine.rs`

In `register_device()` (lines 135-168), if `libc::open()` succeeds but `self.device_fds.write()` fails (lock poisoned), the file descriptor is leaked.

Fix by closing the fd before returning the error:

```rust
// After opening fd successfully, if the lock fails:
let mut fds = self.device_fds.write().map_err(|_| {
    unsafe { libc::close(fd); }
    StorageError::AllocatorError("Failed to acquire device_fds write lock".to_string())
})?;
```

## Output Format

Provide the complete contents of TWO files:

1. `crates/claudefs-transport/src/tls.rs` — complete file with the TlsAcceptor fix
2. `crates/claudefs-storage/src/uring_engine.rs` — complete file with the fd leak fix

Mark file boundaries clearly with `// FILE: path/to/file` comments.
