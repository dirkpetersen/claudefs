# claudefs-gateway Phase 2: proptest for XDR + Async TCP NFS Listener

You are working on the `claudefs-gateway` crate at
`/home/cfs/claudefs/crates/claudefs-gateway/`.

The gateway crate currently builds cleanly with 1107 tests passing. Phase 2 adds:
1. **proptest-based round-trip tests** for XDR encoding/decoding
2. **An async TCP NFS listener** using tokio that dispatches incoming RPC calls
3. **A new `nfs_listener.rs` module** for the async TCP server

---

## Task 1: Add proptest dev-dependency to Cargo.toml

In `crates/claudefs-gateway/Cargo.toml`, add proptest as a dev dependency.

**Current `Cargo.toml`:**
```toml
[package]
name = "claudefs-gateway"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
description = "ClaudeFS subsystem: NFSv3 gateway, pNFS layouts, S3 API endpoint"

[[bin]]
name = "cfs-gateway"
path = "src/main.rs"

[dependencies]
tokio.workspace = true
thiserror.workspace = true
anyhow.workspace = true
serde.workspace = true
prost.workspace = true
tonic.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
rand.workspace = true
sha2.workspace = true
base64 = "0.22"

[lib]
name = "claudefs_gateway"
path = "src/lib.rs"
```

**Required addition** — add after the `[dependencies]` block:
```toml
[dev-dependencies]
proptest = "1.4"
```

---

## Task 2: Add proptest-based tests to `xdr.rs`

The XDR module (`src/xdr.rs`) already has `XdrEncoder` and `XdrDecoder` structs.
Add a new `#[cfg(test)]` proptest block at the bottom of the file with comprehensive
round-trip property tests.

The existing test module is already present. Add a new module `proptest_tests` inside
the existing `#[cfg(test)] mod tests` block, like this:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    // ... existing tests ...

    // ADD AFTER existing tests:
    mod proptest_tests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// u32 round-trip: any u32 encodes and decodes to the same value
            #[test]
            fn prop_u32_roundtrip(v in any::<u32>()) {
                let mut enc = XdrEncoder::new();
                enc.encode_u32(v);
                let buf = enc.finish();
                let mut dec = XdrDecoder::new(buf);
                prop_assert_eq!(dec.decode_u32().unwrap(), v);
                prop_assert_eq!(dec.remaining(), 0);
            }

            /// i32 round-trip: any i32 encodes and decodes to the same value
            #[test]
            fn prop_i32_roundtrip(v in any::<i32>()) {
                let mut enc = XdrEncoder::new();
                enc.encode_i32(v);
                let buf = enc.finish();
                let mut dec = XdrDecoder::new(buf);
                prop_assert_eq!(dec.decode_i32().unwrap(), v);
                prop_assert_eq!(dec.remaining(), 0);
            }

            /// u64 round-trip: any u64 encodes and decodes to the same value
            #[test]
            fn prop_u64_roundtrip(v in any::<u64>()) {
                let mut enc = XdrEncoder::new();
                enc.encode_u64(v);
                let buf = enc.finish();
                let mut dec = XdrDecoder::new(buf);
                prop_assert_eq!(dec.decode_u64().unwrap(), v);
                prop_assert_eq!(dec.remaining(), 0);
            }

            /// i64 round-trip: any i64 encodes and decodes to the same value
            #[test]
            fn prop_i64_roundtrip(v in any::<i64>()) {
                let mut enc = XdrEncoder::new();
                enc.encode_i64(v);
                let buf = enc.finish();
                let mut dec = XdrDecoder::new(buf);
                prop_assert_eq!(dec.decode_i64().unwrap(), v);
                prop_assert_eq!(dec.remaining(), 0);
            }

            /// bool round-trip: any bool encodes and decodes to the same value
            #[test]
            fn prop_bool_roundtrip(v in any::<bool>()) {
                let mut enc = XdrEncoder::new();
                enc.encode_bool(v);
                let buf = enc.finish();
                let mut dec = XdrDecoder::new(buf);
                prop_assert_eq!(dec.decode_bool().unwrap(), v);
                prop_assert_eq!(dec.remaining(), 0);
            }

            /// opaque variable-length round-trip: any byte slice encodes and decodes
            #[test]
            fn prop_opaque_variable_roundtrip(data in proptest::collection::vec(any::<u8>(), 0..256)) {
                let mut enc = XdrEncoder::new();
                enc.encode_opaque_variable(&data);
                let buf = enc.finish();
                let mut dec = XdrDecoder::new(buf);
                prop_assert_eq!(dec.decode_opaque_variable().unwrap(), data);
                prop_assert_eq!(dec.remaining(), 0);
            }

            /// string round-trip: any valid UTF-8 string encodes and decodes
            #[test]
            fn prop_string_roundtrip(s in "\\PC{0,200}") {
                let mut enc = XdrEncoder::new();
                enc.encode_string(&s);
                let buf = enc.finish();
                let mut dec = XdrDecoder::new(buf);
                prop_assert_eq!(dec.decode_string().unwrap(), s);
                prop_assert_eq!(dec.remaining(), 0);
            }

            /// Multi-value sequence: encode multiple values, decode in same order
            #[test]
            fn prop_sequence_roundtrip(
                a in any::<u32>(),
                b in any::<u64>(),
                c in any::<bool>(),
                s in "\\PC{0,100}"
            ) {
                let mut enc = XdrEncoder::new();
                enc.encode_u32(a);
                enc.encode_u64(b);
                enc.encode_bool(c);
                enc.encode_string(&s);
                let buf = enc.finish();

                let mut dec = XdrDecoder::new(buf);
                prop_assert_eq!(dec.decode_u32().unwrap(), a);
                prop_assert_eq!(dec.decode_u64().unwrap(), b);
                prop_assert_eq!(dec.decode_bool().unwrap(), c);
                prop_assert_eq!(dec.decode_string().unwrap(), s);
                prop_assert_eq!(dec.remaining(), 0);
            }

            /// XDR encoding is always a multiple of 4 bytes (XDR alignment rule)
            #[test]
            fn prop_encoding_alignment(data in proptest::collection::vec(any::<u8>(), 0..100)) {
                let mut enc = XdrEncoder::new();
                enc.encode_opaque_variable(&data);
                let buf = enc.finish();
                // Length prefix (4 bytes) + data (padded to 4-byte boundary)
                let expected_len = 4 + data.len() + ((4 - (data.len() % 4)) % 4);
                prop_assert_eq!(buf.len(), expected_len);
            }

            /// Truncated buffer returns an error (not a panic)
            #[test]
            fn prop_truncated_returns_error(v in any::<u64>()) {
                let mut enc = XdrEncoder::new();
                enc.encode_u64(v);
                let full_buf = enc.finish();
                // Try decoding from a buffer missing the last byte
                if full_buf.len() > 1 {
                    let truncated = full_buf.slice(..full_buf.len() - 1);
                    let mut dec = XdrDecoder::new(truncated);
                    let result = dec.decode_u64();
                    prop_assert!(result.is_err(), "Expected error for truncated buffer");
                }
            }
        }
    }
}
```

**IMPORTANT:** Make sure to check what methods `XdrDecoder` actually has. Looking at the file, it has:
- `decode_u32() -> Result<u32>`
- `decode_i32() -> Result<i32>`
- `decode_u64() -> Result<u64>`
- `decode_i64() -> Result<i64>`
- `decode_bool() -> Result<bool>`
- `decode_opaque_fixed(len: usize) -> Result<Vec<u8>>`
- `decode_opaque_variable() -> Result<Vec<u8>>`
- `decode_string() -> Result<String>`
- `remaining() -> usize`

Use only these existing methods. Do NOT add new methods to XdrDecoder.

---

## Task 3: Create `src/nfs_listener.rs` — Async TCP NFS Listener

Create a new file `crates/claudefs-gateway/src/nfs_listener.rs` that implements
an async tokio TCP listener for NFS RPC calls.

The listener:
- Accepts TCP connections on a configurable port
- Reads RPC record marking (RFC 5531 section 11) — 4-byte big-endian length prefix
- Dispatches each RPC message to the `RpcDispatcher`
- Sends the response back with proper record marking
- Handles graceful shutdown via a `tokio::sync::watch` channel
- Uses structured logging via the `tracing` crate

**Implementation requirements:**
- Must use `tokio::net::TcpListener` for accepting connections
- Must use `tokio::io::AsyncReadExt` and `tokio::io::AsyncWriteExt` for I/O
- Each connection is handled in a separate `tokio::spawn`'d task
- `NfsListener` struct holds the bind address and shutdown receiver
- `NfsListener::new(addr: &str) -> (NfsListener, NfsShudown)` where `NfsShutdown` is a handle to send shutdown
- `NfsListener::run<B: VfsBackend + Send + Sync + 'static>(self, dispatcher: Arc<RpcDispatcher<B>>) -> tokio::task::JoinHandle<()>`
- Must handle: connection accept errors, malformed RPC frames, oversized frames (reject > 4MB), partial reads

Here is the complete implementation to write:

```rust
//! Async tokio TCP listener for NFSv3 RPC calls (RFC 5531 record marking)

use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tracing::{debug, error, info, warn};

use crate::nfs::VfsBackend;
use crate::server::RpcDispatcher;

/// Maximum allowed RPC record size (4 MB). Larger frames are rejected.
const MAX_RPC_RECORD: usize = 4 * 1024 * 1024;

/// Listener handle — call `shutdown()` to stop the listener.
pub struct NfsShutdown {
    sender: watch::Sender<bool>,
}

impl NfsShutdown {
    /// Signal the listener to stop accepting new connections.
    pub fn shutdown(&self) {
        let _ = self.sender.send(true);
    }
}

/// Async TCP listener that accepts NFS RPC connections.
pub struct NfsListener {
    /// Bind address (e.g., "0.0.0.0:2049")
    pub bind_addr: String,
    /// Shutdown signal receiver
    shutdown: watch::Receiver<bool>,
}

impl NfsListener {
    /// Create a new `NfsListener` bound to `addr`.
    ///
    /// Returns the listener and a `NfsShutdown` handle.
    pub fn new(addr: &str) -> (Self, NfsShutdown) {
        let (tx, rx) = watch::channel(false);
        let listener = NfsListener {
            bind_addr: addr.to_string(),
            shutdown: rx,
        };
        let shutdown = NfsShutdown { sender: tx };
        (listener, shutdown)
    }

    /// Start listening and dispatching NFS RPC calls.
    ///
    /// Each accepted connection is handled in a separate `tokio::spawn`'d task.
    /// Returns a `JoinHandle` that completes when the listener shuts down.
    pub fn run<B>(self, dispatcher: Arc<RpcDispatcher<B>>) -> tokio::task::JoinHandle<()>
    where
        B: VfsBackend + Send + Sync + 'static,
    {
        tokio::spawn(async move {
            let listener = match TcpListener::bind(&self.bind_addr).await {
                Ok(l) => {
                    info!(addr = %self.bind_addr, "NFS TCP listener started");
                    l
                }
                Err(e) => {
                    error!(addr = %self.bind_addr, error = %e, "Failed to bind NFS listener");
                    return;
                }
            };

            let mut shutdown = self.shutdown;

            loop {
                tokio::select! {
                    accept = listener.accept() => {
                        match accept {
                            Ok((stream, peer)) => {
                                debug!(peer = %peer, "NFS connection accepted");
                                let dispatcher = dispatcher.clone();
                                let shutdown_rx = shutdown.clone();
                                tokio::spawn(async move {
                                    handle_connection(stream, dispatcher, shutdown_rx).await;
                                });
                            }
                            Err(e) => {
                                warn!(error = %e, "NFS accept error");
                            }
                        }
                    }
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() {
                            info!("NFS listener shutting down");
                            break;
                        }
                    }
                }
            }
        })
    }
}

/// Handle a single NFS/TCP connection using RFC 5531 record marking.
async fn handle_connection<B>(
    mut stream: TcpStream,
    dispatcher: Arc<RpcDispatcher<B>>,
    mut shutdown: watch::Receiver<bool>,
) where
    B: VfsBackend + Send + Sync + 'static,
{
    let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or_default();

    loop {
        // Read the 4-byte RPC record mark (RFC 5531 §11)
        let mut mark_buf = [0u8; 4];
        let read_mark = tokio::select! {
            r = stream.read_exact(&mut mark_buf) => r,
            _ = shutdown.changed() => break,
        };

        match read_mark {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                debug!(peer = %peer, "NFS connection closed by client");
                break;
            }
            Err(e) => {
                warn!(peer = %peer, error = %e, "Error reading RPC record mark");
                break;
            }
        }

        // The high bit of the record mark indicates "last fragment"
        let mark = u32::from_be_bytes(mark_buf);
        let _last_fragment = (mark & 0x8000_0000) != 0;
        let fragment_len = (mark & 0x7FFF_FFFF) as usize;

        if fragment_len > MAX_RPC_RECORD {
            warn!(peer = %peer, len = fragment_len, "RPC record too large, closing connection");
            break;
        }

        // Read the RPC message body
        let mut body = vec![0u8; fragment_len];
        if let Err(e) = stream.read_exact(&mut body).await {
            warn!(peer = %peer, error = %e, "Error reading RPC body");
            break;
        }

        // Dispatch the RPC call
        let response = dispatcher.dispatch(&body);

        // Write the response with record marking (last_fragment bit set)
        let resp_len = response.len() as u32;
        let resp_mark: u32 = 0x8000_0000 | resp_len;
        let mut out = Vec::with_capacity(4 + response.len());
        out.extend_from_slice(&resp_mark.to_be_bytes());
        out.extend_from_slice(&response);

        if let Err(e) = stream.write_all(&out).await {
            warn!(peer = %peer, error = %e, "Error writing RPC response");
            break;
        }
    }

    debug!(peer = %peer, "NFS connection handler finished");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nfs_listener_new() {
        let (listener, shutdown) = NfsListener::new("127.0.0.1:0");
        assert_eq!(listener.bind_addr, "127.0.0.1:0");
        // Shutdown signal should work
        shutdown.shutdown();
    }

    #[test]
    fn test_nfs_shutdown_signal() {
        let (listener, shutdown) = NfsListener::new("0.0.0.0:2049");
        assert!(!*listener.shutdown.borrow());
        shutdown.shutdown();
        // After shutdown(), receiver should see true
        // (We can't easily test async behavior without spawning a runtime)
        assert_eq!(listener.bind_addr, "0.0.0.0:2049");
    }

    #[test]
    fn test_max_rpc_record_constant() {
        assert_eq!(MAX_RPC_RECORD, 4 * 1024 * 1024);
    }

    #[test]
    fn test_record_mark_parsing() {
        // Test that we correctly extract last_fragment bit and fragment_len
        let mark: u32 = 0x8000_0005; // last fragment, 5 bytes
        let last_fragment = (mark & 0x8000_0000) != 0;
        let fragment_len = (mark & 0x7FFF_FFFF) as usize;
        assert!(last_fragment);
        assert_eq!(fragment_len, 5);

        let mark2: u32 = 0x0000_0100; // not last fragment, 256 bytes
        let last_fragment2 = (mark2 & 0x8000_0000) != 0;
        let fragment_len2 = (mark2 & 0x7FFF_FFFF) as usize;
        assert!(!last_fragment2);
        assert_eq!(fragment_len2, 256);
    }
}
```

**IMPORTANT NOTE:** The `RpcDispatcher::dispatch(&[u8]) -> Vec<u8>` method may not
exist yet. Check `src/server.rs` first. If `dispatch` doesn't exist, you must also
add a `pub fn dispatch(&self, buf: &[u8]) -> Vec<u8>` method to `RpcDispatcher` in
`server.rs`. This method should parse the RPC call from `buf`, dispatch to the
appropriate handler, and return the encoded response. Here's a simple implementation:

```rust
/// Dispatch a raw RPC call bytes to the appropriate handler.
/// Returns the encoded RPC reply bytes.
pub fn dispatch(&self, buf: &[u8]) -> Vec<u8> {
    use prost::bytes::Bytes;
    use crate::xdr::{XdrDecoder, XdrEncoder};
    use crate::rpc::{RpcCall, RpcReply, ACCEPT_SUCCESS, ACCEPT_PROC_UNAVAIL, AUTH_NONE};

    let mut dec = XdrDecoder::new(Bytes::copy_from_slice(buf));

    // Try to parse as RPC call
    match RpcCall::decode(&mut dec) {
        Ok(call) => {
            // Create a null reply for now (stub)
            let reply = RpcReply::accepted(call.xid, ACCEPT_SUCCESS);
            let mut enc = XdrEncoder::new();
            reply.encode(&mut enc);
            enc.finish().to_vec()
        }
        Err(_) => {
            // Return empty on decode error
            Vec::new()
        }
    }
}
```

---

## Task 4: Register `nfs_listener` module in `lib.rs`

In `crates/claudefs-gateway/src/lib.rs`, add the new module declaration.

Find the existing module list and add:
```rust
/// Async TCP listener for NFSv3 RPC connections.
pub mod nfs_listener;
```

Add it near the other NFS-related modules (after `nfs_write`).

---

## Key Constraints

1. **Do NOT break any existing tests** — all 1107 tests must still pass
2. **Only use existing dependencies** — `tokio` is already in `[dependencies]`, no new runtime deps
3. **Proptest** goes in `[dev-dependencies]` only
4. **Do not rename or move any existing functions/types**
5. **All new public items must have doc comments** (the crate has `#![warn(missing_docs)]`)
6. If `RpcCall::decode()` or `RpcReply::encode()` don't exist in the current RPC module,
   make the `dispatch()` method a simple stub that returns an empty Vec — just get it to compile
7. Check `src/server.rs` for the exact type signature of `RpcDispatcher` before adding the `dispatch` method

Run `cargo test -p claudefs-gateway` to validate. All existing tests must still pass.
The new proptest tests should also pass (usually they do since they're property tests).
