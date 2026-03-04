[0m
> build · accounts/fireworks/models/minimax-m2p5

[0m[0m→ [0mRead crates/claudefs-gateway/Cargo.toml

[0m→ [0mRead crates/claudefs-gateway/src/xdr.rs
[0m→ [0mRead crates/claudefs-gateway/src/lib.rs[0m→ [0mRead crates/claudefs-gateway/src/server.rs
[0m
[0m← [0mEdit crates/claudefs-gateway/Cargo.toml
Index: /home/cfs/claudefs/crates/claudefs-gateway/Cargo.toml
===================================================================
--- /home/cfs/claudefs/crates/claudefs-gateway/Cargo.toml
+++ /home/cfs/claudefs/crates/claudefs-gateway/Cargo.toml
@@ -22,7 +22,10 @@
 rand.workspace = true
 sha2.workspace = true
 base64 = "0.22"
 
+[dev-dependencies]
+proptest = "1.4"
+
 [lib]
 name = "claudefs_gateway"
 path = "src/lib.rs"
\ No newline at end of file

[0m
Now I'll implement the four tasks. Let me start:
[0m← [0mEdit crates/claudefs-gateway/src/xdr.rs
[0m

Index: /home/cfs/claudefs/crates/claudefs-gateway/src/xdr.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-gateway/src/xdr.rs
+++ /home/cfs/claudefs/crates/claudefs-gateway/src/xdr.rs
@@ -343,5 +343,126 @@
 
         let mut dec = XdrDecoder::new(buf);
         assert_eq!(dec.decode_string().unwrap(), s);
     }
+
+    mod proptest_tests {
+        use super::*;
+        use proptest::prelude::*;
+
+        proptest! {
+            #[test]
+            fn prop_u32_roundtrip(v in any::<u32>()) {
+                let mut enc = XdrEncoder::new();
+                enc.encode_u32(v);
+                let buf = enc.finish();
+                let mut dec = XdrDecoder::new(buf);
+                prop_assert_eq!(dec.decode_u32().unwrap(), v);
+                prop_assert_eq!(dec.remaining(), 0);
+            }
+
+            #[test]
+            fn prop_i32_roundtrip(v in any::<i32>()) {
+                let mut enc = XdrEncoder::new();
+                enc.encode_i32(v);
+                let buf = enc.finish();
+                let mut dec = XdrDecoder::new(buf);
+                prop_assert_eq!(dec.decode_i32().unwrap(), v);
+                prop_assert_eq!(dec.remaining(), 0);
+            }
+
+            #[test]
+            fn prop_u64_roundtrip(v in any::<u64>()) {
+                let mut enc = XdrEncoder::new();
+                enc.encode_u64(v);
+                let buf = enc.finish();
+                let mut dec = XdrDecoder::new(buf);
+                prop_assert_eq!(dec.decode_u64().unwrap(), v);
+                prop_assert_eq!(dec.remaining(), 0);
+            }
+
+            #[test]
+            fn prop_i64_roundtrip(v in any::<i64>()) {
+                let mut enc = XdrEncoder::new();
+                enc.encode_i64(v);
+                let buf = enc.finish();
+                let mut dec = XdrDecoder::new(buf);
+                prop_assert_eq!(dec.decode_i64().unwrap(), v);
+                prop_assert_eq!(dec.remaining(), 0);
+            }
+
+            #[test]
+            fn prop_bool_roundtrip(v in any::<bool>()) {
+                let mut enc = XdrEncoder::new();
+                enc.encode_bool(v);
+                let buf = enc.finish();
+                let mut dec = XdrDecoder::new(buf);
+                prop_assert_eq!(dec.decode_bool().unwrap(), v);
+                prop_assert_eq!(dec.remaining(), 0);
+            }
+
+            #[test]
+            fn prop_opaque_variable_roundtrip(data in proptest::collection::vec(any::<u8>(), 0..256)) {
+                let mut enc = XdrEncoder::new();
+                enc.encode_opaque_variable(&data);
+                let buf = enc.finish();
+                let mut dec = XdrDecoder::new(buf);
+                prop_assert_eq!(dec.decode_opaque_variable().unwrap(), data);
+                prop_assert_eq!(dec.remaining(), 0);
+            }
+
+            #[test]
+            fn prop_string_roundtrip(s in "\PC{0,200}") {
+                let mut enc = XdrEncoder::new();
+                enc.encode_string(&s);
+                let buf = enc.finish();
+                let mut dec = XdrDecoder::new(buf);
+                prop_assert_eq!(dec.decode_string().unwrap(), s);
+                prop_assert_eq!(dec.remaining(), 0);
+            }
+
+            #[test]
+            fn prop_sequence_roundtrip(
+                a in any::<u32>(),
+                b in any::<u64>(),
+                c in any::<bool>(),
+                s in "\PC{0,100}"
+            ) {
+                let mut enc = XdrEncoder::new();
+                enc.encode_u32(a);
+                enc.encode_u64(b);
+                enc.encode_bool(c);
+                enc.encode_string(&s);
+                let buf = enc.finish();
+
+                let mut dec = XdrDecoder::new(buf);
+                prop_assert_eq!(dec.decode_u32().unwrap(), a);
+                prop_assert_eq!(dec.decode_u64().unwrap(), b);
+                prop_assert_eq!(dec.decode_bool().unwrap(), c);
+                prop_assert_eq!(dec.decode_string().unwrap(), s);
+                prop_assert_eq!(dec.remaining(), 0);
+            }
+
+            #[test]
+            fn prop_encoding_alignment(data in proptest::collection::vec(any::<u8>(), 0..100)) {
+                let mut enc = XdrEncoder::new();
+                enc.encode_opaque_variable(&data);
+                let buf = enc.finish();
+                let expected_len = 4 + data.len() + ((4 - (data.len() % 4)) % 4);
+                prop_assert_eq!(buf.len(), expected_len);
+            }
+
+            #[test]
+            fn prop_truncated_returns_error(v in any::<u64>()) {
+                let mut enc = XdrEncoder::new();
+                enc.encode_u64(v);
+                let full_buf = enc.finish();
+                if full_buf.len() > 1 {
+                    let truncated = full_buf.slice(..full_buf.len() - 1);
+                    let mut dec = XdrDecoder::new(truncated);
+                    let result = dec.decode_u64();
+                    prop_assert!(result.is_err(), "Expected error for truncated buffer");
+                }
+            }
+        }
+    }
 }
[0m
Index: /home/cfs/claudefs/crates/claudefs-gateway/src/server.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-gateway/src/server.rs
+++ /home/cfs/claudefs/crates/claudefs-gateway/src/server.rs
@@ -64,10 +64,16 @@
         config,
     }
 }
 
+/// Dispatches raw RPC call bytes to the appropriate handler.
+/// Returns the encoded RPC reply bytes.
+pub fn dispatch(&self, buf: &[u8]) -> Vec<u8> {
+    self.process_tcp(buf)
+}
+
 /// Dispatches an RPC call to the appropriate NFS or MOUNT handler.
-pub fn dispatch(&self, call: &RpcCall) -> Vec<u8> {
+pub fn dispatch_call(&self, call: &RpcCall) -> Vec<u8> {
     let auth = AuthCred::from_opaque_auth(&call.cred);
 
     match (call.prog, call.vers) {
         (NFS_PROGRAM, NFS_VERSION) => self.dispatch_nfs(call, &auth),

[0m← [0mEdit crates/claudefs-gateway/src/server.rs
Now I need to update the internal calls in the same file to use `dispatch_call`:


[0mIndex: /home/cfs/claudefs/crates/claudefs-gateway/src/server.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-gateway/src/server.rs
+++ /home/cfs/claudefs/crates/claudefs-gateway/src/server.rs
@@ -349,9 +349,9 @@
 let rpc_data = &data[4..4 + fragment_len as usize];
 
 match RpcCall::decode(rpc_data) {
     Ok(call) => {
-        let reply = self.dispatch(&call);
+        let reply = self.dispatch_call(&call);
         crate::rpc::TcpRecordMark::encode(&reply)
     }
     Err(_) => vec![],
 }

[0m← [0mEdit crates/claudefs-gateway/src/server.rs
[0m
Wrote file successfully.
[0m← [0mWrite crates/claudefs-gateway/src/nfs_listener.rs[0m← [0mEdit crates/claudefs-gateway/src/lib.rs
[0m
Index: /home/cfs/claudefs/crates/claudefs-gateway/src/lib.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-gateway/src/lib.rs
+++ /home/cfs/claudefs/crates/claudefs-gateway/src/lib.rs
@@ -45,8 +45,10 @@
 /// NFSv4 session management.
 pub mod nfs_v4_session;
 /// NFS write operations.
 pub mod nfs_write;
+/// Async TCP listener for NFSv3 RPC connections.
+pub mod nfs_listener;
 /// Performance configuration for gateway.
 pub mod perf_config;
 /// pNFS layout handling.
 pub mod pnfs;

[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-gateway 2>&1[0mBlocking waiting for file lock on artifact directory
   Compiling claudefs-gateway v0.1.0 (/home/cfs/claudefs/crates/claudefs-gateway)
error: unknown character escape: `P`
   --> crates/claudefs-gateway/src/xdr.rs:414:45
    |
414 |             fn prop_string_roundtrip(s in "\PC{0,200}") {
    |                                             ^ unknown character escape
    |
    = help: for more information, visit <https://doc.rust-lang.org/reference/tokens.html#literals>
help: if you meant to write a literal backslash (perhaps escaping in a regular expression), consider a raw string literal
    |
414 |             fn prop_string_roundtrip(s in r"\PC{0,200}") {
    |                                           +

error: unknown character escape: `P`
   --> crates/claudefs-gateway/src/xdr.rs:428:24
    |
428 |                 s in "\PC{0,100}"
    |                        ^ unknown character escape
    |
    = help: for more information, visit <https://doc.rust-lang.org/reference/tokens.html#literals>
help: if you meant to write a literal backslash (perhaps escaping in a regular expression), consider a raw string literal
    |
428 |                 s in r"\PC{0,100}"
    |                      +

warning: unused import: `prost::bytes::Bytes`
   --> crates/claudefs-gateway/src/protocol.rs:415:9
    |
415 |     use prost::bytes::Bytes;
    |         ^^^^^^^^^^^^^^^^^^^
    |
    = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

error[E0308]: mismatched types
   --> crates/claudefs-gateway/src/server.rs:405:35
    |
405 |         let reply = disp.dispatch(&call);
    |                          -------- ^^^^^ expected `&[u8]`, found `&RpcCall`
    |                          |
    |                          arguments to this method are incorrect
    |
    = note: expected reference `&[u8]`
               found reference `&rpc::RpcCall`
note: method defined here
   --> crates/claudefs-gateway/src/server.rs:70:12
    |
 70 |     pub fn dispatch(&self, buf: &[u8]) -> Vec<u8> {
    |            ^^^^^^^^        ----------

error[E0308]: mismatched types
   --> crates/claudefs-gateway/src/server.rs:428:35
    |
428 |         let reply = disp.dispatch(&call);
    |                          -------- ^^^^^ expected `&[u8]`, found `&RpcCall`
    |                          |
    |                          arguments to this method are incorrect
    |
    = note: expected reference `&[u8]`
               found reference `&rpc::RpcCall`
note: method defined here
   --> crates/claudefs-gateway/src/server.rs:70:12
    |
 70 |     pub fn dispatch(&self, buf: &[u8]) -> Vec<u8> {
    |            ^^^^^^^^        ----------

error[E0308]: mismatched types
   --> crates/claudefs-gateway/src/server.rs:463:35
    |
463 |         let reply = disp.dispatch(&call);
    |                          -------- ^^^^^ expected `&[u8]`, found `&RpcCall`
    |                          |
    |                          arguments to this method are incorrect
    |
    = note: expected reference `&[u8]`
               found reference `&rpc::RpcCall`
note: method defined here
   --> crates/claudefs-gateway/src/server.rs:70:12
    |
 70 |     pub fn dispatch(&self, buf: &[u8]) -> Vec<u8> {
    |            ^^^^^^^^        ----------

error[E0308]: mismatched types
   --> crates/claudefs-gateway/src/server.rs:487:35
    |
487 |         let reply = disp.dispatch(&call);
    |                          -------- ^^^^^ expected `&[u8]`, found `&RpcCall`
    |                          |
    |                          arguments to this method are incorrect
    |
    = note: expected reference `&[u8]`
               found reference `&rpc::RpcCall`
note: method defined here
   --> crates/claudefs-gateway/src/server.rs:70:12
    |
 70 |     pub fn dispatch(&self, buf: &[u8]) -> Vec<u8> {
    |            ^^^^^^^^        ----------

error[E0308]: mismatched types
   --> crates/claudefs-gateway/src/server.rs:542:35
    |
542 |         let reply = disp.dispatch(&call);
    |                          -------- ^^^^^ expected `&[u8]`, found `&RpcCall`
    |                          |
    |                          arguments to this method are incorrect
    |
    = note: expected reference `&[u8]`
               found reference `&rpc::RpcCall`
note: method defined here
   --> crates/claudefs-gateway/src/server.rs:70:12
    |
 70 |     pub fn dispatch(&self, buf: &[u8]) -> Vec<u8> {
    |            ^^^^^^^^        ----------

error[E0308]: mismatched types
   --> crates/claudefs-gateway/src/server.rs:567:35
    |
567 |         let reply = disp.dispatch(&call);
    |                          -------- ^^^^^ expected `&[u8]`, found `&RpcCall`
    |                          |
    |                          arguments to this method are incorrect
    |
    = note: expected reference `&[u8]`
               found reference `&rpc::RpcCall`
note: method defined here
   --> crates/claudefs-gateway/src/server.rs:70:12
    |
 70 |     pub fn dispatch(&self, buf: &[u8]) -> Vec<u8> {
    |            ^^^^^^^^        ----------

error[E0308]: mismatched types
   --> crates/claudefs-gateway/src/server.rs:590:35
    |
590 |         let reply = disp.dispatch(&call);
    |                          -------- ^^^^^ expected `&[u8]`, found `&RpcCall`
    |                          |
    |                          arguments to this method are incorrect
    |
    = note: expected reference `&[u8]`
               found reference `&rpc::RpcCall`
note: method defined here
   --> crates/claudefs-gateway/src/server.rs:70:12
    |
 70 |     pub fn dispatch(&self, buf: &[u8]) -> Vec<u8> {
    |            ^^^^^^^^        ----------

error[E0308]: mismatched types
   --> crates/claudefs-gateway/src/server.rs:626:35
    |
626 |         let reply = disp.dispatch(&call);
    |                          -------- ^^^^^ expected `&[u8]`, found `&RpcCall`
    |                          |
    |                          arguments to this method are incorrect
    |
    = note: expected reference `&[u8]`
               found reference `&rpc::RpcCall`
note: method defined here
   --> crates/claudefs-gateway/src/server.rs:70:12
    |
 70 |     pub fn dispatch(&self, buf: &[u8]) -> Vec<u8> {
    |            ^^^^^^^^        ----------

error[E0308]: mismatched types
   --> crates/claudefs-gateway/src/server.rs:650:35
    |
650 |         let reply = disp.dispatch(&call);
    |                          -------- ^^^^^ expected `&[u8]`, found `&RpcCall`
    |                          |
    |                          arguments to this method are incorrect
    |
    = note: expected reference `&[u8]`
               found reference `&rpc::RpcCall`
note: method defined here
   --> crates/claudefs-gateway/src/server.rs:70:12
    |
 70 |     pub fn dispatch(&self, buf: &[u8]) -> Vec<u8> {
    |            ^^^^^^^^        ----------

error[E0308]: mismatched types
   --> crates/claudefs-gateway/src/server.rs:686:35
    |
686 |         let reply = disp.dispatch(&call);
    |                          -------- ^^^^^ expected `&[u8]`, found `&RpcCall`
    |                          |
    |                          arguments to this method are incorrect
    |
    = note: expected reference `&[u8]`
               found reference `&rpc::RpcCall`
note: method defined here
   --> crates/claudefs-gateway/src/server.rs:70:12
    |
 70 |     pub fn dispatch(&self, buf: &[u8]) -> Vec<u8> {
    |            ^^^^^^^^        ----------

error[E0308]: mismatched types
   --> crates/claudefs-gateway/src/server.rs:723:35
    |
723 |         let reply = disp.dispatch(&call);
    |                          -------- ^^^^^ expected `&[u8]`, found `&RpcCall`
    |                          |
    |                          arguments to this method are incorrect
    |
    = note: expected reference `&[u8]`
               found reference `&rpc::RpcCall`
note: method defined here
   --> crates/claudefs-gateway/src/server.rs:70:12
    |
 70 |     pub fn dispatch(&self, buf: &[u8]) -> Vec<u8> {
    |            ^^^^^^^^        ----------

error[E0308]: mismatched types
   --> crates/claudefs-gateway/src/server.rs:748:35
    |
748 |         let reply = disp.dispatch(&call);
    |                          -------- ^^^^^ expected `&[u8]`, found `&RpcCall`
    |                          |
    |                          arguments to this method are incorrect
    |
    = note: expected reference `&[u8]`
               found reference `&rpc::RpcCall`
note: method defined here
   --> crates/claudefs-gateway/src/server.rs:70:12
    |
 70 |     pub fn dispatch(&self, buf: &[u8]) -> Vec<u8> {
    |            ^^^^^^^^        ----------

error[E0308]: mismatched types
   --> crates/claudefs-gateway/src/server.rs:785:35
    |
785 |         let reply = disp.dispatch(&call);
    |                          -------- ^^^^^ expected `&[u8]`, found `&RpcCall`
    |                          |
    |                          arguments to this method are incorrect
    |
    = note: expected reference `&[u8]`
               found reference `&rpc::RpcCall`
note: method defined here
   --> crates/claudefs-gateway/src/server.rs:70:12
    |
 70 |     pub fn dispatch(&self, buf: &[u8]) -> Vec<u8> {
    |            ^^^^^^^^        ----------

error[E0277]: the size for values of type `str` cannot be known at compilation time
   --> crates/claudefs-gateway/src/xdr.rs:414:38
    |
414 |             fn prop_string_roundtrip(s in "\PC{0,200}") {
    |                                      ^ doesn't have a size known at compile-time
    |
    = help: the trait `Sized` is not implemented for `str`
    = note: all local variables must have a statically known size

error[E0277]: the size for values of type `str` cannot be known at compilation time
    --> crates/claudefs-gateway/src/xdr.rs:352:9
     |
 352 | /         proptest! {
 353 | |             #[test]
 354 | |             fn prop_u32_roundtrip(v in any::<u32>()) {
 355 | |                 let mut enc = XdrEncoder::new();
...    |
 466 | |         }
     | |_________^ doesn't have a size known at compile-time
     |
     = help: the trait `Sized` is not implemented for `str`
note: required by a bound in `NamedArguments`
    --> /home/cfs/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/proptest-1.10.0/src/sugar.rs:1074:30
     |
1074 | pub struct NamedArguments<N, V>(#[doc(hidden)] pub N, #[doc(hidden)] pub V);
     |                              ^ required by this bound in `NamedArguments`
     = note: this error originates in the macro `$crate::proptest_helper` which comes from the expansion of the macro `proptest` (in Nightly builds, run with -Z macro-backtrace for more info)

error[E0277]: the size for values of type `str` cannot be known at compilation time
   --> crates/claudefs-gateway/src/xdr.rs:419:17
    |
419 |                 prop_assert_eq!(dec.decode_string().unwrap(), s);
    |                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ doesn't have a size known at compile-time
    |
    = help: the trait `Sized` is not implemented for `str`
    = note: all local variables must have a statically known size
    = note: this error originates in the macro `prop_assert_eq` (in Nightly builds, run with -Z macro-backtrace for more info)

error[E0277]: the size for values of type `str` cannot be known at compilation time
   --> crates/claudefs-gateway/src/xdr.rs:419:17
    |
419 |                 prop_assert_eq!(dec.decode_string().unwrap(), s);
    |                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    |                 |
    |                 doesn't have a size known at compile-time
    |                 required by this formatting parameter
    |
    = help: the trait `Sized` is not implemented for `str`
    = note: this error originates in the macro `$crate::__export::format_args` which comes from the expansion of the macro `prop_assert_eq` (in Nightly builds, run with -Z macro-backtrace for more info)

error[E0277]: the size for values of type `str` cannot be known at compilation time
   --> crates/claudefs-gateway/src/xdr.rs:428:17
    |
428 |                 s in "\PC{0,100}"
    |                 ^ doesn't have a size known at compile-time
    |
    = help: the trait `Sized` is not implemented for `str`
    = note: all local variables must have a statically known size

error[E0277]: the size for values of type `str` cannot be known at compilation time
    --> crates/claudefs-gateway/src/xdr.rs:352:9
     |
 352 | /         proptest! {
 353 | |             #[test]
 354 | |             fn prop_u32_roundtrip(v in any::<u32>()) {
 355 | |                 let mut enc = XdrEncoder::new();
...    |
 466 | |         }
     | |_________^ doesn't have a size known at compile-time
     |
     = help: within `(u32, u64, bool, str)`, the trait `Sized` is not implemented for `str`
     = note: required because it appears within the type `(u32, u64, bool, str)`
note: required by a bound in `NamedArguments`
    --> /home/cfs/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/proptest-1.10.0/src/sugar.rs:1074:30
     |
1074 | pub struct NamedArguments<N, V>(#[doc(hidden)] pub N, #[doc(hidden)] pub V);
     |                              ^ required by this bound in `NamedArguments`
     = note: this error originates in the macro `$crate::proptest_helper` which comes from the expansion of the macro `proptest` (in Nightly builds, run with -Z macro-backtrace for more info)

error[E0277]: the size for values of type `str` cannot be known at compilation time
   --> crates/claudefs-gateway/src/xdr.rs:441:17
    |
441 |                 prop_assert_eq!(dec.decode_string().unwrap(), s);
    |                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ doesn't have a size known at compile-time
    |
    = help: the trait `Sized` is not implemented for `str`
    = note: all local variables must have a statically known size
    = note: this error originates in the macro `prop_assert_eq` (in Nightly builds, run with -Z macro-backtrace for more info)

error[E0277]: the size for values of type `str` cannot be known at compilation time
   --> crates/claudefs-gateway/src/xdr.rs:441:17
    |
441 |                 prop_assert_eq!(dec.decode_string().unwrap(), s);
    |                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    |                 |
    |                 doesn't have a size known at compile-time
    |                 required by this formatting parameter
    |
    = help: the trait `Sized` is not implemented for `str`
    = note: this error originates in the macro `$crate::__export::format_args` which comes from the expansion of the macro `prop_assert_eq` (in Nightly builds, run with -Z macro-backtrace for more info)

error: could not compile `claudefs-gateway` (lib) due to 2 previous errors
warning: build failed, waiting for other jobs to finish...
warning: variable does not need to be mutable
   --> crates/claudefs-gateway/src/gateway_tls.rs:450:13
    |
450 |         let mut endpoint1 = TlsEndpoint::new("0.0.0.0", 9000, config.clone());
    |             ----^^^^^^^^^
    |             |
    |             help: remove this `mut`
    |
    = note: `#[warn(unused_mut)]` (part of `#[warn(unused)]`) on by default

Some errors have detailed explanations: E0277, E0308.
For more information about an error, try `rustc --explain E0277`.
warning: `claudefs-gateway` (lib test) generated 2 warnings
error: could not compile `claudefs-gateway` (lib test) due to 23 previous errors; 2 warnings emitted


[0m← [0mEdit crates/claudefs-gateway/src/xdr.rsIndex: /home/cfs/claudefs/crates/claudefs-gateway/src/xdr.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-gateway/src/xdr.rs
+++ /home/cfs/claudefs/crates/claudefs-gateway/src/xdr.rs
@@ -410,9 +410,9 @@
     prop_assert_eq!(dec.remaining(), 0);
 }
 
 #[test]
-fn prop_string_roundtrip(s in "\PC{0,200}") {
+fn prop_string_roundtrip(s in r"\PC{0,200}") {
     let mut enc = XdrEncoder::new();
     enc.encode_string(&s);
     let buf = enc.finish();
     let mut dec = XdrDecoder::new(buf);
@@ -424,9 +424,9 @@
 fn prop_sequence_roundtrip(
     a in any::<u32>(),
     b in any::<u64>(),
     c in any::<bool>(),
-    s in "\PC{0,100}"
+    s in r"\PC{0,100}"
 ) {
     let mut enc = XdrEncoder::new();
     enc.encode_u32(a);
     enc.encode_u64(b);

[0m

[0m✱ [0mGrep "let reply = disp\.dispatch\(&call\)"[90m in crates/claudefs-gateway/src · 13 matches[0m
[0m
[0m← [0mEdit crates/claudefs-gateway/src/server.rs
[0mIndex: /home/cfs/claudefs/crates/claudefs-gateway/src/server.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-gateway/src/server.rs
+++ /home/cfs/claudefs/crates/claudefs-gateway/src/server.rs
@@ -401,9 +401,9 @@
     OpaqueAuth::none().encode_xdr(&mut enc);
     let call_data = enc.finish().to_vec();
 
     let call = RpcCall::decode(&call_data).unwrap();
-    let reply = disp.dispatch(&call);
+    let reply = disp.dispatch_call(&call);
 
     let mut dec = XdrDecoder::new(prost::bytes::Bytes::from(reply));
     assert_eq!(dec.decode_u32().unwrap(), 1);
     assert_eq!(dec.decode_u32().unwrap(), 1);
@@ -424,9 +424,9 @@
     OpaqueAuth::none().encode_xdr(&mut enc);
     let call_data = enc.finish().to_vec();
 
     let call = RpcCall::decode(&call_data).unwrap();
-    let reply = disp.dispatch(&call);
+    let reply = disp.dispatch_call(&call);
 
     let mut dec = XdrDecoder::new(prost::bytes::Bytes::from(reply));
     assert_eq!(dec.decode_u32().unwrap(), 2);
     assert_eq!(dec.decode_u32().unwrap(), 1);
@@ -459,9 +459,9 @@
 
     let call_data = enc.finish().to_vec();
 
     let call = RpcCall::decode(&call_data).unwrap();
-    let reply = disp.dispatch(&call);
+    let reply = disp.dispatch_call(&call);
 
     let mut dec = XdrDecoder::new(prost::bytes::Bytes::from(reply));
     assert_eq!(dec.decode_u32().unwrap(), 3);
     assert_eq!(dec.decode_u32().unwrap(), 1);
@@ -483,9 +483,9 @@
     OpaqueAuth::none().encode_xdr(&mut enc);
     let call_data = enc.finish().to_vec();
 
     let call = RpcCall::decode(&call_data).unwrap();
-    let reply = disp.dispatch(&call);
+    let reply = disp.dispatch_call(&call);
 
     let mut dec = XdrDecoder::new(prost::bytes::Bytes::from(reply));
     assert_eq!(dec.decode_u32().unwrap(), 4);
     assert_eq!(dec.decode_u32().unwrap(), 1);
@@ -538,9 +538,9 @@
     enc.encode_string(".");
 
     let call_data = enc.finish().to_vec();
     let call = RpcCall::decode(&call_data).unwrap();
-    let reply = disp.dispatch(&call);
+    let reply = disp.dispatch_call(&call);
 
     let mut dec = XdrDecoder::new(prost::bytes::Bytes::from(reply));
     assert_eq!(dec.decode_u32().unwrap(), 6);
     assert_eq!(dec.decode_u32().unwrap(), 1);
@@ -563,9 +563,9 @@
     enc.encode_string("/");
     let call_data = enc.finish().to_vec();
 
     let call = RpcCall::decode(&call_data).unwrap();
-    let reply = disp.dispatch(&call);
+    let reply = disp.dispatch_call(&call);
 
     let mut dec = XdrDecoder::new(prost::bytes::Bytes::from(reply));
     assert_eq!(dec.decode_u32().unwrap(), 7);
     assert_eq!(dec.decode_u32().unwrap(), 1);
@@ -586,9 +586,9 @@
     OpaqueAuth::none().encode_xdr(&mut enc);
     let call_data = enc.finish().to_vec();
 
     let call = RpcCall::decode(&call_data).unwrap();
-    let reply = disp.dispatch(&call);
+    let reply = disp.dispatch_call(&call);
 
     let mut dec = XdrDecoder::new(prost::bytes::Bytes::from(reply));
     assert_eq!(dec.decode_u32().unwrap(), 8);
     assert_eq!(dec.decode_u32().unwrap(), 1);
@@ -622,9 +622,9 @@
     enc.encode_u32(0);
 
     let call_data = enc.finish().to_vec();
     let call = RpcCall::decode(&call_data).unwrap();
-    let reply = disp.dispatch(&call);
+    let reply = disp.dispatch_call(&call);
 
     let mut dec = XdrDecoder::new(prost::bytes::Bytes::from(reply));
     assert_eq!(dec.decode_u32().unwrap(), 9);
     assert_eq!(dec.decode_u32().unwrap(), 1);
@@ -646,9 +646,9 @@
     OpaqueAuth::none().encode_xdr(&mut enc);
     let call_data = enc.finish().to_vec();
 
     let call = RpcCall::decode(&call_data).unwrap();
-    let reply = disp.dispatch(&call);
+    let reply = disp.dispatch_call(&call);
 
     let mut dec = XdrDecoder::new(prost::bytes::Bytes::from(reply));
     assert_eq!(dec.decode_u32().unwrap(), 10);
     assert_eq!(dec.decode_u32().unwrap(), 1);
@@ -682,9 +682,9 @@
     enc.encode_u32(4096);
 
     let call_data = enc.finish().to_vec();
     let call = RpcCall::decode(&call_data).unwrap();
-    let reply = disp.dispatch(&call);
+    let reply = disp.dispatch_call(&call);
 
     let mut dec = XdrDecoder::new(prost::bytes::Bytes::from(reply));
     assert_eq!(dec.decode_u32().unwrap(), 11);
     assert_eq!(dec.decode_u32().unwrap(), 1);
@@ -719,9 +719,9 @@
     enc.encode_opaque_variable(b"hello");
 
     let call_data = enc.finish().to_vec();
     let call = RpcCall::decode(&call_data).unwrap();
-    let reply = disp.dispatch(&call);
+    let reply = disp.dispatch_call(&call);
 
     assert!(reply.len() > 4);
 }
 
@@ -744,9 +744,9 @@
     enc.encode_string("/");
     let call_data = enc.finish().to_vec();
 
     let call = RpcCall::decode(&call_data).unwrap();
-    let reply = disp.dispatch(&call);
+    let reply = disp.dispatch_call(&call);
 
     let mut dec = XdrDecoder::new(prost::bytes::Bytes::from(reply));
     assert_eq!(dec.decode_u32().unwrap(), 13);
     assert_eq!(dec.decode_u32().unwrap(), 1);
@@ -781,9 +781,9 @@
     enc.encode_u32(0o755);
 
     let call_data = enc.finish().to_vec();
     let call = RpcCall::decode(&call_data).unwrap();
-    let reply = disp.dispatch(&call);
+    let reply = disp.dispatch_call(&call);
 
     let mut dec = XdrDecoder::new(prost::bytes::Bytes::from(reply));
     assert_eq!(dec.decode_u32().unwrap(), 14);
     assert_eq!(dec.decode_u32().unwrap(), 1);


[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-gateway 2>&1


[0mBlocking waiting for file lock on artifact directory
   Compiling claudefs-gateway v0.1.0 (/home/cfs/claudefs/crates/claudefs-gateway)
warning: unused import: `prost::bytes::Bytes`
   --> crates/claudefs-gateway/src/protocol.rs:415:9
    |
415 |     use prost::bytes::Bytes;
    |         ^^^^^^^^^^^^^^^^^^^
    |
    = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: variable does not need to be mutable
   --> crates/claudefs-gateway/src/gateway_tls.rs:450:13
    |
450 |         let mut endpoint1 = TlsEndpoint::new("0.0.0.0", 9000, config.clone());
    |             ----^^^^^^^^^
    |             |
    |             help: remove this `mut`
    |
    = note: `#[warn(unused_mut)]` (part of `#[warn(unused)]`) on by default

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/gateway_circuit_breaker.rs:309:5
    |
309 |     pub fn set_opened_at(&mut self, time: Instant) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    |
note: the lint level is defined here
   --> crates/claudefs-gateway/src/lib.rs:1:9
    |
  1 | #![warn(missing_docs)]
    |         ^^^^^^^^^^^^

warning: comparison is useless due to type limits
   --> crates/claudefs-gateway/src/gateway_conn_pool.rs:486:17
    |
486 |         assert!(evicted >= 0);
    |                 ^^^^^^^^^^^^
    |
    = note: `#[warn(unused_comparisons)]` on by default

warning: calls to `std::mem::drop` with a value that implements `Copy` does nothing
   --> crates/claudefs-gateway/src/gateway_conn_pool.rs:615:13
    |
615 |             drop(conn_id); // keep checked out
    |             ^^^^^-------^
    |                  |
    |                  argument has type `u64`
    |
    = note: `#[warn(dropping_copy_types)]` on by default
help: use `let _ = ...` to ignore the expression or result
    |
615 -             drop(conn_id); // keep checked out
615 +             let _ = conn_id; // keep checked out
    |

warning: comparison is useless due to type limits
   --> crates/claudefs-gateway/src/stats.rs:330:17
    |
330 |         assert!(uptime >= 0);
    |                 ^^^^^^^^^^^

warning: `claudefs-gateway` (lib test) generated 6 warnings (run `cargo fix --lib -p claudefs-gateway --tests` to apply 2 suggestions)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 32.23s
     Running unittests src/lib.rs (target/debug/deps/claudefs_gateway-3f336f00af2ad3cc)

running 1121 tests
test access_log::tests::test_access_log_append ... ok
test access_log::tests::test_access_log_capacity_eviction ... ok
test access_log::tests::test_access_log_entry_format_csv ... ok
test access_log::tests::test_access_log_entry_is_error ... ok
test access_log::tests::test_access_log_entry_with_bytes ... ok
test access_log::tests::test_access_log_entry_format_structured ... ok
test access_log::tests::test_access_log_entry_new ... ok
test access_log::tests::test_access_log_entry_with_duration ... ok
test access_log::tests::test_access_log_entry_with_status ... ok
test access_log::tests::test_access_log_entry_with_uid ... ok
test access_log::tests::test_access_log_filter_client ... ok
test access_log::tests::test_access_log_filter_protocol ... ok
test access_log::tests::test_access_log_is_empty ... ok
test access_log::tests::test_access_log_len ... ok
test access_log::tests::test_access_log_recent ... ok
test access_log::tests::test_access_log_reset_stats ... ok
test access_log::tests::test_access_log_stats ... ok
test access_log::tests::test_access_log_stats_add_entry ... ok
test access_log::tests::test_access_log_stats_avg_duration ... ok
test access_log::tests::test_access_log_stats_avg_duration_empty ... ok
test access_log::tests::test_access_log_stats_error_count ... ok
test access_log::tests::test_access_log_stats_error_rate ... ok
test access_log::tests::test_access_log_stats_requests_per_sec ... ok
test access_log::tests::test_access_log_stats_requests_per_sec_zero_window ... ok
test auth::tests::test_auth_cred_from_opaque_auth_none ... ok
test auth::tests::test_auth_cred_from_opaque_auth_unknown ... ok
test auth::tests::test_auth_cred_from_opaque_auth_sys ... ok
test auth::tests::test_auth_cred_gid ... ok
test auth::tests::test_auth_cred_is_root ... ok
test auth::tests::test_auth_cred_uid ... ok
test auth::tests::test_auth_none ... ok
test auth::tests::test_auth_sys_cred_build ... ok
test auth::tests::test_auth_sys_cred_encode_decode_roundtrip ... ok
test auth::tests::test_auth_sys_cred_has_gid_primary ... ok
test auth::tests::test_auth_sys_cred_has_gid_supplementary ... ok
test auth::tests::test_auth_sys_cred_has_uid ... ok
test auth::tests::test_auth_sys_cred_is_root ... ok
test auth::tests::test_auth_sys_from_opaque_auth ... ok
test auth::tests::test_auth_sys_from_opaque_auth_truncated ... ok
test auth::tests::test_effective_uid_all_squash ... ok
test auth::tests::test_effective_uid_none_policy_passes_root ... ok
test auth::tests::test_effective_uid_root_squash_passes_nonroot ... ok
test auth::tests::test_effective_uid_root_squash_squashes_root ... ok
test auth::tests::test_machinename_length_limit ... ok
test auth::tests::test_machinename_max_length_ok ... ok
test auth::tests::test_squash_policy_default_is_root_squash ... ok
test config::tests::test_bind_addr_mount_default ... ok
test config::tests::test_bind_addr_new ... ok
test config::tests::test_bind_addr_nfs_default ... ok
test config::tests::test_bind_addr_s3_default ... ok
test config::tests::test_bind_addr_to_socket_addr_string ... ok
test config::tests::test_export_config_default_ro ... ok
test config::tests::test_export_config_default_rw ... ok
test config::tests::test_export_config_to_export_entry ... ok
test config::tests::test_gateway_config_any_enabled ... ok
test config::tests::test_gateway_config_default ... ok
test config::tests::test_gateway_config_validate_empty_export_path ... ok
test config::tests::test_gateway_config_validate_empty_exports ... ok
test config::tests::test_gateway_config_validate_nfs_mount_port_conflict ... ok
test config::tests::test_gateway_config_validate_no_protocols ... ok
test config::tests::test_gateway_config_validate_ok ... ok
test config::tests::test_gateway_config_validate_pnfs_no_data_servers ... ok
test config::tests::test_gateway_config_validate_pnfs_ok ... ok
test config::tests::test_gateway_config_validate_port_conflict ... ok
test config::tests::test_nfs_config_default_with_export ... ok
test config::tests::test_s3_config_default ... ok
test error::tests::test_nfs3acces_error ... ok
test error::tests::test_nfs3badhandle_error ... ok
test error::tests::test_nfs3exist_error ... ok
test error::tests::test_nfs3fbig_error ... ok
test error::tests::test_nfs3inval_error ... ok
test error::tests::test_nfs3io_error ... ok
test error::tests::test_nfs3isdir_error ... ok
test error::tests::test_nfs3noent_error ... ok
test error::tests::test_nfs3nospc_error ... ok
test error::tests::test_nfs3notdir_error ... ok
test error::tests::test_nfs3notsupp_error ... ok
test error::tests::test_nfs3rofs_error ... ok
test error::tests::test_nfs3serverfault_error ... ok
test error::tests::test_nfs3stale_error ... ok
test error::tests::test_s3_bucket_not_found_error ... ok
test export_manager::tests::test_add_duplicate_export_error ... ok
test export_manager::tests::test_add_export ... ok
test export_manager::tests::test_decrement_clients ... ok
test export_manager::tests::test_export_active_status ... ok
test export_manager::tests::test_export_can_remove ... ok
test export_manager::tests::test_export_can_remove_after_clients_disconnect ... ok
test export_manager::tests::test_export_draining_status ... ok
test export_manager::tests::test_export_manager_new ... ok
test export_manager::tests::test_export_paths ... ok
test export_manager::tests::test_force_remove_export ... ok
test export_manager::tests::test_get_export ... ok
test export_manager::tests::test_get_nonexistent_export ... ok
test export_manager::tests::test_increment_clients ... ok
test export_manager::tests::test_increment_nonexistent ... ok
test export_manager::tests::test_is_exported ... ok
test export_manager::tests::test_list_exports ... ok
test export_manager::tests::test_reload_adds_new_exports ... ok
test export_manager::tests::test_reload_removes_old_exports ... ok
test export_manager::tests::test_remove_export ... ok
test export_manager::tests::test_remove_nonexistent_export ... ok
test export_manager::tests::test_root_fh ... ok
test export_manager::tests::test_root_fh_nonexistent ... ok
test export_manager::tests::test_total_clients ... ok
test gateway_audit::tests::test_audit_event_type_severity_acl_denied ... ok
test gateway_audit::tests::test_audit_event_type_severity_auth_failure ... ok
test gateway_audit::tests::test_audit_event_type_severity_auth_success ... ok
test gateway_audit::tests::test_audit_event_type_severity_export_violation ... ok
test gateway_audit::tests::test_audit_event_type_severity_rate_limit ... ok
test gateway_audit::tests::test_audit_event_type_severity_tls_handshake_failed ... ok
test gateway_audit::tests::test_audit_event_type_severity_token_revoked ... ok
test gateway_audit::tests::test_audit_event_type_severity_unauthorized_operation ... ok
test gateway_audit::tests::test_audit_record_new ... ok
test gateway_audit::tests::test_audit_severity_critical_greater_than_warning ... ok
test gateway_audit::tests::test_audit_severity_ordering ... ok
test gateway_audit::tests::test_audit_trail_clear ... ok
test gateway_audit::tests::test_audit_trail_critical_count ... ok
test gateway_audit::tests::test_audit_trail_is_empty ... ok
test gateway_audit::tests::test_audit_trail_len ... ok
test gateway_audit::tests::test_audit_trail_min_severity_filters_events ... ok
test gateway_audit::tests::test_audit_trail_new_is_empty ... ok
test gateway_audit::tests::test_audit_trail_record_increments_id ... ok
test gateway_audit::tests::test_audit_trail_record_returns_id ... ok
test gateway_audit::tests::test_audit_trail_record_returns_none_below_min_severity ... ok
test gateway_audit::tests::test_audit_trail_record_returns_none_when_disabled ... ok
test gateway_audit::tests::test_audit_trail_record_stores_critical_when_min_info ... ok
test gateway_audit::tests::test_audit_trail_records_by_severity ... ok
test gateway_audit::tests::test_audit_trail_records_by_type ... ok
test gateway_audit::tests::test_audit_trail_records_by_type_unknown ... ok
test gateway_audit::tests::test_audit_trail_ring_buffer_eviction ... ok
test gateway_audit::tests::test_audit_trail_warning_count ... ok
test gateway_circuit_breaker::tests::test_circuit_breaker_initial_state ... ok
test gateway_circuit_breaker::tests::test_default_config ... ok
test gateway_circuit_breaker::tests::test_failure_accumulation_opens_circuit ... ok
test gateway_circuit_breaker::tests::test_halfopen_to_closed_on_success_threshold ... ok
test gateway_circuit_breaker::tests::test_halfopen_to_open_on_failure ... ok
test gateway_circuit_breaker::tests::test_metrics_tracking ... ok
test gateway_circuit_breaker::tests::test_name_accessor ... ok
test gateway_circuit_breaker::tests::test_normal_call_flow ... ok
test gateway_circuit_breaker::tests::test_open_state_rejects_calls ... ok
test gateway_circuit_breaker::tests::test_open_to_halfopen_after_duration ... ok
test gateway_circuit_breaker::tests::test_operation_failed_in_closed_state ... ok
test gateway_circuit_breaker::tests::test_registry_all_metrics ... ok
test gateway_circuit_breaker::tests::test_registry_get ... ok
test gateway_circuit_breaker::tests::test_registry_get_mut ... ok
test gateway_circuit_breaker::tests::test_registry_get_or_create ... ok
test gateway_circuit_breaker::tests::test_registry_reset_all ... ok
test gateway_circuit_breaker::tests::test_reset_forces_closed_state ... ok
test gateway_circuit_breaker::tests::test_state_changes_metric ... ok
test gateway_circuit_breaker::tests::test_successful_call_clears_failure_count ... ok
test gateway_circuit_breaker::tests::test_trip_forces_open_state ... ok
test gateway_conn_pool::tests::test_backend_node_builder ... ok
test gateway_conn_pool::tests::test_checkin_increments_requests_served ... ok
test gateway_conn_pool::tests::test_checkin_nonexistent_connection ... ok
test gateway_conn_pool::tests::test_checkout_creates_new_when_under_max ... ok
test gateway_conn_pool::tests::test_config_default_values ... ok
test gateway_conn_pool::tests::test_conn_id_uniqueness ... ok
test gateway_conn_pool::tests::test_evict_idle ... ok
test gateway_conn_pool::tests::test_mark_unhealthy ... ok
test gateway_conn_pool::tests::test_mark_unhealthy_nonexistent_connection ... ok
test gateway_conn_pool::tests::test_multi_pool_add_remove_node ... ok
test gateway_conn_pool::tests::test_pool_stats ... ok
test gateway_conn_pool::tests::test_no_healthy_nodes_returns_none ... ok
test gateway_conn_pool::tests::test_pooled_conn_is_healthy ... ok
test gateway_conn_pool::tests::test_pooled_conn_is_idle ... ok
test gateway_conn_pool::tests::test_remove_nonexistent_node ... ok
test gateway_conn_pool::tests::test_single_node_checkout_checkin ... ok
test gateway_conn_pool::tests::test_total_and_active_conns ... ok
test gateway_conn_pool::tests::test_weighted_checkout ... ok
test gateway_metrics::tests::test_backend_errors_tracking ... ok
test gateway_metrics::tests::test_circuit_breakers_tracking ... ok
test gateway_metrics::tests::test_empty_histogram_percentiles ... ok
test gateway_metrics::tests::test_empty_metrics_error_rate ... ok
test gateway_metrics::tests::test_gateway_metrics_active_connections ... ok
test gateway_audit::tests::test_audit_config_default ... ok
test gateway_metrics::tests::test_gateway_metrics_export_text ... ok
test gateway_metrics::tests::test_gateway_metrics_overall_error_rate ... ok
test gateway_metrics::tests::test_gateway_metrics_record_op ... ok
test gateway_metrics::tests::test_gateway_metrics_reset ... ok
test gateway_metrics::tests::test_gateway_metrics_total_errors ... ok
test gateway_metrics::tests::test_gateway_metrics_total_requests ... ok
test gateway_metrics::tests::test_gateway_metrics_uptime ... ok
test gateway_metrics::tests::test_get_op_metrics ... ok
test gateway_metrics::tests::test_latency_histogram_mean ... ok
test gateway_metrics::tests::test_latency_histogram_observe ... ok
test gateway_metrics::tests::test_latency_histogram_p50 ... ok
test gateway_metrics::tests::test_latency_histogram_p99 ... ok
test gateway_metrics::tests::test_latency_histogram_reset ... ok
test gateway_metrics::tests::test_metric_operation_as_str ... ok
test gateway_metrics::tests::test_latency_histogram_p999 ... ok
test gateway_metrics::tests::test_metric_operation_from_str ... ok
test gateway_metrics::tests::test_metric_protocol_as_str ... ok
test gateway_metrics::tests::test_operation_metrics_error_rate ... ok
test gateway_metrics::tests::test_operation_metrics_record_error ... ok
test gateway_metrics::tests::test_operation_metrics_record_success ... ok
test gateway_tls::tests::test_cipher_preference_variants ... ok
test gateway_tls::tests::test_client_cert_mode_required_validation_path ... ok
test gateway_tls::tests::test_in_memory_cert_source_validation ... ok
test gateway_tls::tests::test_tls_config_default_values ... ok
test gateway_tls::tests::test_tls_config_validator_accepts_valid_config ... ok
test gateway_tls::tests::test_tls_config_validator_rejects_empty_alpn_protocols ... ok
test gateway_tls::tests::test_tls_config_validator_rejects_empty_cert_path ... ok
test gateway_tls::tests::test_tls_config_validator_rejects_empty_key_path ... ok
test gateway_tls::tests::test_tls_config_validator_rejects_handshake_timeout_zero ... ok
test gateway_tls::tests::test_tls_config_validator_rejects_session_cache_size_zero ... ok
test gateway_tls::tests::test_tls_endpoint_bind_address_returns_addr_port ... ok
test gateway_tls::tests::test_tls_endpoint_disable_sets_enabled_false ... ok
test gateway_tls::tests::test_tls_endpoint_enable_sets_enabled_true ... ok
test gateway_tls::tests::test_tls_endpoint_new_creates_enabled_endpoint ... ok
test gateway_tls::tests::test_tls_registry_all_names_returns_all_names ... ok
test gateway_tls::tests::test_tls_registry_enabled_count_counts_only_enabled ... ok
test gateway_tls::tests::test_tls_registry_get_returns_none_for_unknown_name ... ok
test gateway_tls::tests::test_tls_registry_get_returns_some_for_registered_name ... ok
test gateway_tls::tests::test_tls_registry_new_is_empty ... ok
test gateway_tls::tests::test_tls_registry_register_adds_endpoint ... ok
test gateway_tls::tests::test_tls_registry_remove_returns_the_endpoint ... ok
test gateway_tls::tests::test_tls_version_variants ... ok
test health::tests::test_check_result_degraded ... ok
test health::tests::test_check_result_ok ... ok
test health::tests::test_check_result_unhealthy ... ok
test health::tests::test_health_checker_clear ... ok
test health::tests::test_health_checker_is_healthy ... ok
test health::tests::test_health_checker_is_healthy_empty ... ok
test health::tests::test_health_checker_is_healthy_with_unhealthy ... ok
test health::tests::test_health_checker_is_ready ... ok
test health::tests::test_health_checker_is_ready_not ... ok
test health::tests::test_health_checker_new ... ok
test health::tests::test_health_checker_register_result ... ok
test health::tests::test_health_checker_remove_check ... ok
test health::tests::test_health_checker_remove_check_not_found ... ok
test health::tests::test_health_checker_report ... ok
test health::tests::test_health_checker_update_result ... ok
test health::tests::test_health_checker_update_result_not_found ... ok
test health::tests::test_health_report_failed_count ... ok
test health::tests::test_health_report_is_ready ... ok
test health::tests::test_health_report_is_ready_not ... ok
test health::tests::test_health_report_new_all_healthy ... ok
test health::tests::test_health_report_new_empty ... ok
test health::tests::test_health_report_new_with_degraded ... ok
test health::tests::test_health_report_new_with_unhealthy ... ok
test health::tests::test_health_report_passed_count ... ok
test health::tests::test_health_status_is_ok ... ok
test health::tests::test_health_status_to_str ... ok
test mount::tests::test_create_handler ... ok
test mount::tests::test_dump ... ok
test mount::tests::test_export_list ... ok
test mount::tests::test_is_allowed ... ok
test mount::tests::test_is_allowed_empty_groups ... ok
test mount::tests::test_is_exported ... ok
test mount::tests::test_mnt_allowed_client ... ok
test mount::tests::test_mnt_auth_flavors ... ok
test mount::tests::test_mnt_invalid_path ... ok
test mount::tests::test_mnt_registers_mount ... ok
test mount::tests::test_mnt_valid_path ... ok
test mount::tests::test_mnt_wrong_client ... ok
test mount::tests::test_null ... ok
test mount::tests::test_umnt ... ok
test mount::tests::test_umntall ... ok
test nfs::tests::test_access ... ok
test nfs::tests::test_create_file ... ok
test nfs::tests::test_fsinfo ... ok
test nfs::tests::test_fsstat ... ok
test nfs::tests::test_getattr ... ok
test nfs::tests::test_lookup_root_dot ... ok
test nfs::tests::test_lookup_root_dotdot ... ok
test nfs::tests::test_mkdir ... ok
test nfs::tests::test_pathconf ... ok
test nfs::tests::test_readdir ... ok
test nfs::tests::test_remove_file ... ok
test nfs::tests::test_rename_directory ... ok
test nfs::tests::test_symlink_readlink ... ok
test nfs::tests::test_write_and_read ... ok
test nfs_acl::tests::test_acl_entry_applies_to_group ... ok
test nfs_acl::tests::test_acl_entry_applies_to_group_obj ... ok
test nfs_acl::tests::test_acl_entry_applies_to_mask ... ok
test nfs_acl::tests::test_acl_entry_applies_to_user ... ok
test nfs_acl::tests::test_acl_entry_applies_to_user_obj ... ok
test nfs_acl::tests::test_acl_entry_group ... ok
test nfs_acl::tests::test_acl_entry_group_obj ... ok
test nfs_acl::tests::test_acl_entry_mask ... ok
test nfs_acl::tests::test_acl_entry_other ... ok
test nfs_acl::tests::test_acl_entry_user ... ok
test nfs_acl::tests::test_acl_entry_user_obj ... ok
test nfs_acl::tests::test_acl_perms_from_bits ... ok
test nfs_acl::tests::test_acl_perms_new ... ok
test nfs_acl::tests::test_acl_perms_none ... ok
test nfs_acl::tests::test_acl_perms_r_only ... ok
test nfs_acl::tests::test_acl_perms_rw ... ok
test nfs_acl::tests::test_acl_perms_rwx ... ok
test nfs_acl::tests::test_acl_perms_rx ... ok
test nfs_acl::tests::test_acl_perms_to_bits ... ok
test nfs_acl::tests::test_nfs4_access_mask_from_u32 ... ok
test nfs_acl::tests::test_nfs4_access_mask_full_control ... ok
test nfs_acl::tests::test_nfs4_access_mask_read_only ... ok
test nfs_acl::tests::test_nfs4_access_mask_read_write ... ok
test nfs_acl::tests::test_nfs4_access_mask_to_u32 ... ok
test nfs_acl::tests::test_nfs4_ace_allow_everyone ... ok
test nfs_acl::tests::test_nfs4_ace_allow_owner ... ok
test nfs_acl::tests::test_nfs4_ace_deny_everyone ... ok
test nfs_acl::tests::test_posix_acl_add ... ok
test nfs_acl::tests::test_posix_acl_by_tag ... ok
test nfs_acl::tests::test_posix_acl_check_access ... ok
test nfs_acl::tests::test_posix_acl_is_valid ... ok
test nfs_acl::tests::test_posix_acl_is_valid_missing_mask ... ok
test nfs_acl::tests::test_posix_acl_is_valid_with_named ... ok
test nfs_acl::tests::test_posix_acl_new ... ok
test nfs_acl::tests::test_posix_acl_remove_tag ... ok
test nfs_acl::tests::test_posix_acl_to_mode_bits ... ok
test nfs_acl::tests::test_posix_acl_to_mode_bits_with_mask ... ok
test nfs_cache::tests::test_attr_cache_capacity_limit ... ok
test nfs_cache::tests::test_attr_cache_hit_rate ... ok
test nfs_cache::tests::test_attr_cache_insert_get ... ok
test nfs_cache::tests::test_attr_cache_invalidate ... ok
test nfs_cache::tests::test_attr_cache_invalidate_all ... ok
test nfs_cache::tests::test_attr_cache_is_empty ... ok
test nfs_cache::tests::test_attr_cache_len ... ok
test nfs_cache::tests::test_attr_cache_miss ... ok
test nfs_cache::tests::test_attr_cache_stats ... ok
test nfs_cache::tests::test_cached_attr_new ... ok
test nfs_copy_offload::tests::test_async_copy_handle_progress ... ok
test nfs_copy_offload::tests::test_already_complete_cannot_fail ... ok
test nfs_copy_offload::tests::test_cancel_copy ... ok
test nfs_copy_offload::tests::test_clone_request_builder ... ok
test nfs_copy_offload::tests::test_clone_request_default ... ok
test nfs_copy_offload::tests::test_complete_copy ... ok
test nfs_copy_offload::tests::test_complete_non_existent_copy ... ok
test nfs_copy_offload::tests::test_fail_copy ... ok
test nfs_copy_offload::tests::test_copy_result_builder ... ok
test nfs_copy_offload::tests::test_copy_state_values ... ok
test nfs_copy_offload::tests::test_fail_non_existent_copy ... ok
test nfs_copy_offload::tests::test_max_concurrent_limit ... ok
test nfs_copy_offload::tests::test_multiple_segments ... ok
test nfs_copy_offload::tests::test_poll_copy_not_found ... ok
test nfs_copy_offload::tests::test_purge_finished ... ok
test nfs_copy_offload::tests::test_segment_validation ... ok
test nfs_copy_offload::tests::test_start_copy ... ok
test nfs_copy_offload::tests::test_write_stable_values ... ok
test nfs_copy_offload::tests::test_zero_total_bytes_progress ... ok
test nfs_delegation::tests::test_delegation_id_hex_length ... ok
test nfs_delegation::tests::test_active_count_only_granted ... ok
test nfs_delegation::tests::test_delegation_initiate_recall ... ok
test nfs_delegation::tests::test_delegation_is_active_false_after_recall ... ok
test nfs_delegation::tests::test_delegation_manager_new_empty ... ok
test nfs_delegation::tests::test_delegation_id_unique ... ok
test nfs_delegation::tests::test_delegation_is_active_true_for_granted ... ok
test nfs_delegation::tests::test_delegation_mark_returned ... ok
test nfs_delegation::tests::test_delegation_new_creates_granted ... ok
test nfs_delegation::tests::test_delegation_revoke ... ok
test nfs_delegation::tests::test_file_delegations ... ok
test nfs_delegation::tests::test_grant_read_after_write_fails ... ok
test nfs_delegation::tests::test_grant_read_delegation_succeeds ... ok
test nfs_delegation::tests::test_grant_second_write_fails ... ok
test nfs_delegation::tests::test_grant_write_after_read_succeeds ... ok
test nfs_delegation::tests::test_grant_write_delegation_succeeds ... ok
test nfs_delegation::tests::test_recall_file_no_delegations ... ok
test nfs_delegation::tests::test_multiple_read_delegations ... ok
test nfs_delegation::tests::test_recall_file_sets_recall_pending ... ok
test nfs_delegation::tests::test_recall_file_returns_ids ... ok
test nfs_delegation::tests::test_return_delegation_already_returned ... ok
test nfs_delegation::tests::test_return_delegation_not_found ... ok
test nfs_delegation::tests::test_return_delegation_success ... ok
test nfs_delegation::tests::test_revoke_client ... ok
test nfs_delegation::tests::test_revoke_client_sets_revoked ... ok
test nfs_export::tests::test_client_spec_any_allows_any_ip ... ok
test nfs_delegation::tests::test_total_count_all_states ... ok
test nfs_export::tests::test_client_spec_any_allows_asterisk ... ok
test nfs_export::tests::test_client_spec_from_cidr_exact_match ... ok
test nfs_export::tests::test_client_spec_from_cidr_no_match_different_ip ... ok
test nfs_export::tests::test_export_access_default_is_read_only ... ok
test nfs_export::tests::test_export_config_is_read_only_default ... ok
test nfs_export::tests::test_export_config_multiple_clients_both_allowed ... ok
test nfs_export::tests::test_export_config_new_default_access_is_read_only ... ok
test nfs_export::tests::test_export_config_new_empty_clients_allows_no_client ... ok
test nfs_export::tests::test_export_config_new_has_default_squash_policy ... ok
test nfs_export::tests::test_export_config_no_squash_sets_policy ... ok
test nfs_export::tests::test_export_config_read_write_convenience ... ok
test nfs_export::tests::test_export_config_squash_gid_default ... ok
test nfs_export::tests::test_export_config_squash_uid_default ... ok
test nfs_export::tests::test_export_config_with_access_read_write ... ok
test nfs_export::tests::test_export_config_with_client_allows_any ... ok
test nfs_export::tests::test_export_config_with_squash_all_squash ... ok
test nfs_export::tests::test_export_path_different_from_local_path ... ok
test nfs_export::tests::test_export_registry_add_increases_count ... ok
test nfs_export::tests::test_export_registry_find_returns_none_unknown ... ok
test nfs_export::tests::test_export_registry_find_returns_some ... ok
test nfs_export::tests::test_export_registry_hidden_export_not_in_list_visible ... ok
test nfs_export::tests::test_export_registry_list_visible_returns_non_hidden ... ok
test nfs_export::tests::test_export_registry_new_starts_empty ... ok
test nfs_export::tests::test_export_registry_remove_decreases_count ... ok
test nfs_export::tests::test_export_registry_remove_returns_false_unknown_path ... ok
test nfs_export::tests::test_export_registry_remove_returns_true_known_path ... ok
test nfs_listener::tests::test_max_rpc_record_constant ... ok
test nfs_listener::tests::test_nfs_listener_new ... ok
test nfs_listener::tests::test_nfs_shutdown_signal ... ok
test nfs_listener::tests::test_record_mark_parsing ... ok
test nfs_readdirplus::tests::test_encode_fsstat_ok ... ok
test nfs_readdirplus::tests::test_encode_getattr_err ... ok
test nfs_readdirplus::tests::test_encode_getattr_ok ... ok
test nfs_readdirplus::tests::test_encode_lookup_ok ... ok
test nfs_readdirplus::tests::test_encode_read_ok ... ok
test nfs_readdirplus::tests::test_encode_read_ok_with_eof_false ... ok
test nfs_readdirplus::tests::test_encode_readdirplus_err ... ok
test nfs_readdirplus::tests::test_encode_readdirplus_ok_empty ... ok
test nfs_readdirplus::tests::test_encode_readdirplus_ok_multiple_entries ... ok
test nfs_readdirplus::tests::test_encode_readdirplus_ok_single_entry ... ok
test nfs_readdirplus::tests::test_encode_write_ok ... ok
test nfs_referral::tests::test_add_referral_duplicate ... ok
test nfs_referral::tests::test_add_referral_success ... ok
test nfs_referral::tests::test_disable_referral ... ok
test nfs_referral::tests::test_disable_referral_not_exists ... ok
test nfs_referral::tests::test_empty_database_operations ... ok
test nfs_referral::tests::test_enable_referral ... ok
test nfs_referral::tests::test_enable_referral_not_exists ... ok
test nfs_referral::tests::test_list_referrals ... ok
test nfs_referral::tests::test_lookup_by_prefix_exact_match ... ok
test nfs_referral::tests::test_lookup_by_prefix_longest_match ... ok
test nfs_referral::tests::test_lookup_by_prefix_nested_paths ... ok
test nfs_referral::tests::test_lookup_by_prefix_root_match ... ok
test nfs_referral::tests::test_lookup_by_prefix_with_disabled_entry ... ok
test nfs_referral::tests::test_lookup_exact_match ... ok
test nfs_referral::tests::test_lookup_not_found ... ok
test nfs_referral::tests::test_lookup_returns_disabled_referral ... ok
test nfs_referral::tests::test_multiple_referrals_different_paths ... ok
test nfs_referral::tests::test_referral_entry_validation_double_slash ... ok
test nfs_referral::tests::test_referral_entry_validation_empty_targets ... ok
test nfs_referral::tests::test_referral_entry_validation_invalid_path_not_absolute ... ok
test nfs_referral::tests::test_referral_entry_validation_valid ... ok
test nfs_referral::tests::test_referral_target_validation_empty_server ... ok
test nfs_referral::tests::test_referral_target_validation_invalid_port ... ok
test nfs_referral::tests::test_referral_target_validation_valid ... ok
test nfs_referral::tests::test_referral_type_default ... ok
test nfs_referral::tests::test_remove_referral_exists ... ok
test nfs_referral::tests::test_remove_referral_not_exists ... ok
test nfs_referral::tests::test_root_path_referral ... ok
test nfs_referral::tests::test_to_fs_locations_conversion ... ok
test nfs_referral::tests::test_to_fs_locations_multiple_targets ... ok
test nfs_referral::tests::test_to_fs_locations_nested_path ... ok
test nfs_v4_session::tests::test_nfs_client_confirm ... ok
test nfs_v4_session::tests::test_nfs_client_lease_expiry ... ok
test nfs_v4_session::tests::test_nfs_client_renew_lease ... ok
test nfs_v4_session::tests::test_nfs_session_destroy ... ok
test nfs_v4_session::tests::test_nfs_session_drain ... ok
test nfs_v4_session::tests::test_nfs_session_fore_slot ... ok
test nfs_v4_session::tests::test_nfs_session_idle_secs ... ok
test nfs_v4_session::tests::test_nfs_session_is_active ... ok
test nfs_v4_session::tests::test_nfs_session_new ... ok
test nfs_v4_session::tests::test_nfs_session_state_display ... ok
test nfs_v4_session::tests::test_nfs_session_update_last_used ... ok
test nfs_v4_session::tests::test_session_error_display ... ok
test nfs_v4_session::tests::test_session_id_new ... ok
test nfs_v4_session::tests::test_session_id_to_hex ... ok
test nfs_v4_session::tests::test_session_manager_active_session_count ... ok
test nfs_v4_session::tests::test_session_manager_client_not_found ... ok
test nfs_v4_session::tests::test_session_manager_confirm_client ... ok
test nfs_v4_session::tests::test_session_manager_create_client ... ok
test nfs_v4_session::tests::test_session_manager_create_session ... ok
test nfs_v4_session::tests::test_session_manager_destroy_session ... ok
test nfs_v4_session::tests::test_session_manager_expire_stale_clients ... ok
test nfs_v4_session::tests::test_session_manager_session_not_found ... ok
test nfs_v4_session::tests::test_session_manager_unconfirmed_client_error ... ok
test nfs_v4_session::tests::test_slot_acquire_release ... ok
test nfs_v4_session::tests::test_slot_new ... ok
test nfs_v4_session::tests::test_slot_release_caches_reply ... ok
test nfs_v4_session::tests::test_slot_validate_after_acquire ... ok
test nfs_v4_session::tests::test_slot_validate_sequence_invalid ... ok
test nfs_v4_session::tests::test_slot_validate_sequence_new_request ... ok
test nfs_v4_session::tests::test_slot_validate_sequence_replay ... ok
test nfs_v4_session::tests::test_slot_validate_sequence_wrapping ... ok
test nfs_write::tests::test_commit ... ok
test nfs_write::tests::test_commit_all ... ok
test nfs_write::tests::test_commit_nonexistent ... ok
test nfs_write::tests::test_has_pending_writes ... ok
test nfs_write::tests::test_pending_count ... ok
test nfs_write::tests::test_pending_write_fields ... ok
test nfs_write::tests::test_pending_writes_empty ... ok
test nfs_write::tests::test_record_write ... ok
test nfs_write::tests::test_record_write_multiple ... ok
test nfs_write::tests::test_remove_file ... ok
test nfs_write::tests::test_total_pending ... ok
test nfs_write::tests::test_write_stability_ordering ... ok
test nfs_write::tests::test_write_tracker_new ... ok
test nfs_write::tests::test_write_verf ... ok
test perf_config::tests::auto_tune_config_default_is_conservative ... ok
test perf_config::tests::auto_tune_mode_variants_exist ... ok
test perf_config::tests::buffer_config_default_values ... ok
test perf_config::tests::connection_config_default_values ... ok
test perf_config::tests::perf_config_for_protocol_nfs_has_larger_buffers ... ok
test perf_config::tests::perf_config_for_protocol_pnfs_matches_nfs_buffer_size ... ok
test perf_config::tests::perf_config_for_protocol_s3_has_smaller_buffers ... ok
test perf_config::tests::perf_config_protocol_returns_the_protocol ... ok
test perf_config::tests::perf_config_validator_accepts_valid_default_nfs ... ok
test perf_config::tests::perf_config_validator_accepts_valid_default_s3 ... ok
test perf_config::tests::timeout_config_default_values ... ok
test perf_config::tests::validator_accepts_target_cpu_percent_100 ... ok
test perf_config::tests::validator_rejects_max_connections_zero ... ok
test perf_config::tests::validator_rejects_max_per_client_exceeds_max ... ok
test perf_config::tests::validator_rejects_max_per_client_zero ... ok
test perf_config::tests::validator_rejects_max_request_size_zero ... ok
test perf_config::tests::validator_rejects_measurement_window_zero ... ok
test perf_config::tests::validator_rejects_read_timeout_zero ... ok
test perf_config::tests::validator_rejects_recv_buf_size_zero ... ok
test perf_config::tests::validator_rejects_send_buf_size_zero ... ok
test perf_config::tests::validator_rejects_target_cpu_percent_101 ... ok
test perf_config::tests::validator_rejects_write_timeout_zero ... ok
test pnfs::tests::test_add_server ... ok
test pnfs::tests::test_empty_server ... ok
test pnfs::tests::test_iomode_from_u32 ... ok
test pnfs::tests::test_layout_offset_length ... ok
test pnfs::tests::test_layout_stateid ... ok
test pnfs::tests::test_layout_type_files ... ok
test pnfs::tests::test_multiple_servers_stripe ... ok
test pnfs::tests::test_new_server ... ok
test pnfs::tests::test_remove_server_existing ... ok
test pnfs::tests::test_remove_server_not_existing ... ok
test pnfs::tests::test_single_server_layout ... ok
test pnfs::tests::test_stripe_unit ... ok
test pnfs_flex::tests::test_flex_file_layout_add_segment ... ok
test pnfs_flex::tests::test_flex_file_layout_new ... ok
test pnfs_flex::tests::test_flex_file_layout_segments_for_range ... ok
test pnfs_flex::tests::test_flex_file_layout_server_add_server ... ok
test pnfs_flex::tests::test_flex_file_layout_server_get_layout ... ok
test pnfs_flex::tests::test_flex_file_layout_server_invalid_mirror_count ... ok
test pnfs_flex::tests::test_flex_file_layout_server_invalid_stripe_unit ... ok
test pnfs_flex::tests::test_flex_file_layout_server_new ... ok
test pnfs_flex::tests::test_flex_file_layout_server_no_servers ... ok
test pnfs_flex::tests::test_flex_file_layout_server_remove_server ... ok
test pnfs_flex::tests::test_flex_file_layout_total_bytes ... ok
test pnfs_flex::tests::test_flex_file_mirror_is_valid_stripe_unit ... ok
test pnfs_flex::tests::test_flex_file_mirror_new ... ok
test pnfs_flex::tests::test_flex_file_segment_contains_offset ... ok
test pnfs_flex::tests::test_flex_file_segment_contains_offset_unlimited ... ok
test pnfs_flex::tests::test_flex_file_segment_new ... ok
test pnfs_flex::tests::test_flex_file_segment_total_server_count ... ok
test portmap::tests::test_clear ... ok
test portmap::tests::test_count ... ok
test portmap::tests::test_dump ... ok
test portmap::tests::test_get_port_not_registered ... ok
test portmap::tests::test_new_portmapper ... ok
test portmap::tests::test_register_defaults ... ok
test portmap::tests::test_register_replace ... ok
test portmap::tests::test_unregister ... ok
test protocol::tests::test_fattr3_default_dir ... ok
test protocol::tests::test_fattr3_default_file ... ok
test protocol::tests::test_fattr3_xdr_roundtrip ... ok
test protocol::tests::test_filehandle_as_inode_invalid ... ok
test protocol::tests::test_filehandle_from_inode ... ok
test protocol::tests::test_filehandle_new_empty_error ... ok
test protocol::tests::test_filehandle_new_too_long_error ... ok
test protocol::tests::test_filehandle_new_valid ... ok
test protocol::tests::test_filehandle_xdr_roundtrip ... ok
test protocol::tests::test_fsinfo_defaults ... ok
test protocol::tests::test_ftype3_from_u32 ... ok
test protocol::tests::test_ftype3_from_u32_invalid ... ok
test protocol::tests::test_ftype3_xdr_roundtrip ... ok
test protocol::tests::test_nfstime3_now ... ok
test protocol::tests::test_nfstime3_xdr_roundtrip ... ok
test protocol::tests::test_nfstime3_zero ... ok
test protocol::tests::test_pathconf_defaults ... ok
test quota::tests::test_check_write ... ok
test quota::tests::test_get_limits_none ... ok
test quota::tests::test_quota_limits_new ... ok
test quota::tests::test_quota_limits_unlimited ... ok
test quota::tests::test_quota_limits_with_soft ... ok
test quota::tests::test_quota_usage_add_bytes ... ok
test quota::tests::test_quota_usage_add_inodes ... ok
test quota::tests::test_quota_usage_new ... ok
test quota::tests::test_quota_usage_sub_bytes ... ok
test quota::tests::test_quota_usage_sub_inodes ... ok
test quota::tests::test_record_create_at_limit ... ok
test quota::tests::test_record_create_below_limit ... ok
test quota::tests::test_record_delete ... ok
test quota::tests::test_record_write_above_hard_limit ... ok
test quota::tests::test_record_write_above_soft_limit ... ok
test quota::tests::test_record_write_at_limit ... ok
test quota::tests::test_record_write_below_limit ... ok
test quota::tests::test_remove_limits ... ok
test quota::tests::test_reset_usage ... ok
test quota::tests::test_set_get_limits ... ok
test quota::tests::test_subjects ... ok
test rpc::tests::test_opaque_auth_encode_decode ... ok
test rpc::tests::test_opaque_auth_encode_decode_roundtrip ... ok
test rpc::tests::test_opaque_auth_none ... ok
test rpc::tests::test_opaque_auth_with_gss ... ok
test rpc::tests::test_rpccall_decode_truncated ... ok
test rpc::tests::test_rpccall_decode_valid_call ... ok
test rpc::tests::test_rpccall_decode_wrong_msg_type ... ok
test rpc::tests::test_rpccall_with_auth_sys ... ok
test rpc::tests::test_rpcreply_encode_auth_error ... ok
test rpc::tests::test_rpcreply_encode_garbage_args ... ok
test rpc::tests::test_rpcreply_encode_garbage_args_verification ... ok
test rpc::tests::test_rpcreply_encode_proc_unavail ... ok
test rpc::tests::test_rpcreply_encode_proc_unavail_verification ... ok
test rpc::tests::test_rpcreply_encode_prog_mismatch ... ok
test rpc::tests::test_rpcreply_encode_success ... ok
test rpc::tests::test_rpcreply_roundtrip ... ok
test rpc::tests::test_tcp_record_mark_decode_last_fragment ... ok
test rpc::tests::test_tcp_record_mark_decode_not_last_fragment ... ok
test rpc::tests::test_tcp_record_mark_encode ... ok
test rpc::tests::test_tcp_record_mark_fragment_boundary ... ok
test s3::tests::test_bucket_name_validation ... ok
test s3::tests::test_bucket_not_empty_on_delete ... ok
test s3::tests::test_bucket_not_found ... ok
test s3::tests::test_bucket_size ... ok
test s3::tests::test_copy_object ... ok
test s3::tests::test_create_and_list_bucket ... ok
test s3::tests::test_delete_bucket ... ok
test s3::tests::test_delete_object ... ok
test s3::tests::test_etag_generation ... ok
test s3::tests::test_head_object ... ok
test s3::tests::test_list_objects_with_delimiter ... ok
test s3::tests::test_list_objects_with_prefix ... ok
test s3::tests::test_multiple_buckets ... ok
test s3::tests::test_object_count ... ok
test s3::tests::test_object_not_found ... ok
test s3::tests::test_overwrite_object ... ok
test s3::tests::test_put_and_get_object ... ok
test s3_bucket_policy::tests::test_allow_all_public ... ok
test s3_bucket_policy::tests::test_allow_user_read ... ok
test s3_bucket_policy::tests::test_allow_user_write ... ok
test s3_bucket_policy::tests::test_bucket_policy_add_statement ... ok
test s3_bucket_policy::tests::test_bucket_policy_default_deny ... ok
test s3_bucket_policy::tests::test_bucket_policy_deny_overrides_allow ... ok
test s3_bucket_policy::tests::test_bucket_policy_is_allowed ... ok
test s3_bucket_policy::tests::test_bucket_policy_new ... ok
test s3_bucket_policy::tests::test_bucket_policy_registry_bucket_count ... ok
test s3_bucket_policy::tests::test_bucket_policy_registry_open_access ... ok
test s3_bucket_policy::tests::test_bucket_policy_registry_remove ... ok
test s3_bucket_policy::tests::test_bucket_policy_registry_set_get ... ok
test s3_bucket_policy::tests::test_bucket_policy_to_json ... ok
test s3_bucket_policy::tests::test_deny_all ... ok
test s3_bucket_policy::tests::test_policy_statement_applies_mismatch ... ok
test s3_bucket_policy::tests::test_resource_all_buckets ... ok
test s3_bucket_policy::tests::test_resource_bucket_only ... ok
test s3_bucket_policy::tests::test_resource_matches_all_buckets ... ok
test s3_bucket_policy::tests::test_resource_matches_prefix ... ok
test s3_bucket_policy::tests::test_resource_matches_wildcard ... ok
test s3_bucket_policy::tests::test_resource_new ... ok
test s3_bucket_policy::tests::test_s3action_from_str ... ok
test s3_bucket_policy::tests::test_s3action_from_str_invalid ... ok
test s3_bucket_policy::tests::test_s3action_to_str ... ok
test s3_cors::tests::test_cors_config_add_rule ... ok
test s3_cors::tests::test_cors_config_matching_rule ... ok
test s3_cors::tests::test_cors_config_new ... ok
test s3_cors::tests::test_cors_registry_get_config_none ... ok
test s3_cors::tests::test_cors_registry_handle_preflight ... ok
test s3_cors::tests::test_cors_registry_handle_preflight_no_bucket ... ok
test s3_cors::tests::test_cors_registry_remove_config ... ok
test s3_cors::tests::test_cors_registry_remove_config_not_found ... ok
test s3_cors::tests::test_cors_registry_set_get_config ... ok
test s3_cors::tests::test_cors_response_headers_no_match ... ok
test s3_cors::tests::test_cors_response_headers ... ok
test s3_cors::tests::test_cors_rule_allow_all ... ok
test s3_cors::tests::test_cors_rule_allows_headers_exact ... ok
test s3_cors::tests::test_cors_rule_allows_headers_wildcard ... ok
test s3_cors::tests::test_cors_rule_allows_method ... ok
test s3_cors::tests::test_cors_rule_is_valid ... ok
test s3_cors::tests::test_cors_rule_is_valid_invalid ... ok
test s3_cors::tests::test_cors_rule_matches_origin_exact ... ok
test s3_cors::tests::test_cors_rule_matches_origin_wildcard ... ok
test s3_cors::tests::test_cors_rule_new ... ok
test s3_cors::tests::test_handle_preflight_allowed ... ok
test s3_cors::tests::test_handle_preflight_denied_no_config ... ok
test s3_cors::tests::test_handle_preflight_denied_no_matching_rule ... ok
test s3_encryption::tests::test_generate_response_headers_none ... ok
test s3_encryption::tests::test_generate_response_headers_sse_kms ... ok
test s3_encryption::tests::test_generate_response_headers_sse_s3 ... ok
test s3_encryption::tests::test_resolve_sse_bucket_key_enabled_from_bucket ... ok
test s3_encryption::tests::test_resolve_sse_enforce_encryption_rejects_none ... ok
test s3_encryption::tests::test_resolve_sse_enforce_kms_requires_key ... ok
test s3_encryption::tests::test_resolve_sse_kms_with_key_succeeds ... ok
test s3_encryption::tests::test_resolve_sse_no_bucket_config ... ok
test s3_encryption::tests::test_resolve_sse_request_overrides_bucket_default ... ok
test s3_encryption::tests::test_resolve_sse_uses_bucket_default ... ok
test s3_encryption::tests::test_sse_algorithm_from_str ... ok
test s3_encryption::tests::test_sse_algorithm_is_kms ... ok
test s3_encryption::tests::test_sse_algorithm_to_string ... ok
test s3_encryption::tests::test_sse_bucket_config_builder ... ok
test s3_encryption::tests::test_sse_bucket_config_response_conversion ... ok
test s3_encryption::tests::test_sse_context_builder ... ok
test s3_encryption::tests::test_sse_context_default ... ok
test s3_encryption::tests::test_sse_manager_configure_bucket ... ok
test s3_encryption::tests::test_sse_manager_new ... ok
test s3_encryption::tests::test_sse_manager_remove_bucket ... ok
test s3_encryption::tests::test_sse_object_metadata_builder ... ok
test s3_encryption::tests::test_validate_kms_key_id_alias ... ok
test s3_encryption::tests::test_validate_kms_key_id_arn ... ok
test s3_encryption::tests::test_validate_kms_key_id_empty ... ok
test s3_encryption::tests::test_validate_kms_key_id_invalid ... ok
test s3_encryption::tests::test_validate_sse_headers_bucket_key_enabled ... ok
test s3_encryption::tests::test_validate_sse_headers_empty ... ok
test s3_encryption::tests::test_validate_sse_headers_invalid_algorithm ... ok
test s3_encryption::tests::test_validate_sse_headers_parsing ... ok
test s3_lifecycle::tests::test_config_add_rule ... ok
test s3_encryption::tests::test_validate_sse_headers_kms_with_key ... ok
test s3_lifecycle::tests::test_config_applicable_transitions ... ok
test s3_lifecycle::tests::test_config_duplicate_id_error ... ok
test s3_lifecycle::tests::test_config_enabled_rules_filter ... ok
test s3_lifecycle::tests::test_config_is_object_expired ... ok
test s3_lifecycle::tests::test_config_remove_rule ... ok
test s3_lifecycle::tests::test_filter_matches_all_objects ... ok
test s3_lifecycle::tests::test_filter_prefix_match ... ok
test s3_lifecycle::tests::test_filter_prefix_no_match ... ok
test s3_lifecycle::tests::test_filter_size_range ... ok
test s3_lifecycle::tests::test_filter_tag_match ... ok
test s3_lifecycle::tests::test_filter_tag_no_match ... ok
test s3_lifecycle::tests::test_lifecycle_rule_no_actions_error ... ok
test s3_lifecycle::tests::test_registry_delete_config ... ok
test s3_lifecycle::tests::test_registry_set_get ... ok
test s3_lifecycle::tests::test_rule_enabled_disabled ... ok
test s3_lifecycle::tests::test_rule_is_expired_false ... ok
test s3_lifecycle::tests::test_rule_is_expired_true ... ok
test s3_lifecycle::tests::test_rule_next_transition_first ... ok
test s3_lifecycle::tests::test_rule_next_transition_none ... ok
test s3_lifecycle::tests::test_rule_next_transition_not_yet ... ok
test s3_lifecycle::tests::test_storage_class_variants ... ok
test s3_multipart::tests::test_multipart_manager_abort ... ok
test s3_multipart::tests::test_multipart_manager_active_count ... ok
test s3_multipart::tests::test_multipart_manager_create ... ok
test s3_multipart::tests::test_multipart_manager_complete ... ok
test s3_multipart::tests::test_multipart_manager_list_uploads ... ok
test s3_multipart::tests::test_multipart_manager_upload_part ... ok
test s3_multipart::tests::test_multipart_manager_upload_part_unknown_upload ... ok
test s3_multipart::tests::test_multipart_upload_abort ... ok
test s3_multipart::tests::test_multipart_upload_abort_completed ... ok
test s3_multipart::tests::test_multipart_upload_add_part ... ok
test s3_multipart::tests::test_multipart_upload_add_part_invalid_number ... ok
test s3_multipart::tests::test_multipart_upload_add_part_invalid_state ... ok
test s3_multipart::tests::test_multipart_upload_mark_completed ... ok
test s3_multipart::tests::test_multipart_upload_mark_completed_wrong_state ... ok
test s3_multipart::tests::test_multipart_upload_new ... ok
test s3_multipart::tests::test_multipart_upload_sorted_parts ... ok
test s3_multipart::tests::test_multipart_upload_start_complete ... ok
test s3_multipart::tests::test_multipart_upload_start_complete_wrong_state ... ok
test s3_multipart::tests::test_multipart_upload_total_size ... ok
test s3_multipart::tests::test_multipart_upload_validate_completion ... ok
test s3_multipart::tests::test_multipart_upload_validate_completion_empty ... ok
test s3_multipart::tests::test_multipart_upload_validate_completion_non_contiguous ... ok
test s3_notification::tests::test_configs_for_returns_empty_slice_for_unknown_bucket ... ok
test s3_notification::tests::test_configs_for_returns_registered_configs ... ok
test s3_notification::tests::test_enabled_config_count_counts_only_enabled_configs ... ok
test s3_notification::tests::test_matching_configs_does_not_return_disabled_configs ... ok
test s3_notification::tests::test_matching_configs_finds_configs_matching_event_key ... ok
test s3_notification::tests::test_notification_config_disable_sets_enabled_false ... ok
test s3_notification::tests::test_notification_config_enable_sets_enabled_true ... ok
test s3_notification::tests::test_notification_config_new_creates_enabled_config ... ok
test s3_notification::tests::test_notification_event_event_name_object_created ... ok
test s3_notification::tests::test_notification_event_event_name_object_removed ... ok
test s3_notification::tests::test_notification_event_event_name_object_restored ... ok
test s3_notification::tests::test_notification_event_event_name_reduced_redundancy ... ok
test s3_notification::tests::test_notification_filter_default_has_no_prefix_suffix ... ok
test s3_notification::tests::test_notification_filter_matches_returns_false_when_key_doesnt_match ... ok
test s3_notification::tests::test_notification_filter_matches_returns_true_when_key_has_matching_prefix ... ok
test s3_notification::tests::test_notification_filter_matches_returns_true_when_key_has_matching_suffix ... ok
test s3_notification::tests::test_notification_filter_matches_returns_true_when_no_filter_set ... ok
test s3_notification::tests::test_notification_filter_with_prefix_sets_prefix ... ok
test s3_notification::tests::test_notification_filter_with_suffix_sets_suffix ... ok
test s3_notification::tests::test_notification_manager_new_is_empty ... ok
test s3_notification::tests::test_record_delivery_increments_counter ... ok
test s3_notification::tests::test_register_adds_config_to_bucket ... ok
test s3_notification::tests::test_remove_deletes_config_by_id ... ok
test s3_notification::tests::test_remove_returns_not_found_for_unknown_id ... ok
test s3_object_lock::tests::test_bucket_lock_not_enabled_for_legal_hold ... ok
test s3_object_lock::tests::test_bucket_lock_not_enabled_for_retention ... ok
test s3_object_lock::tests::test_can_delete_compliance_mode_active ... ok
test s3_object_lock::tests::test_can_delete_governance_mode_bypass_false ... ok
test s3_object_lock::tests::test_can_delete_governance_mode_bypass_true ... ok
test s3_object_lock::tests::test_can_delete_legal_hold_on ... ok
test s3_object_lock::tests::test_can_overwrite_governance_mode_bypass_true ... ok
test s3_object_lock::tests::test_can_overwrite_legal_hold_on ... ok
test s3_object_lock::tests::test_can_overwrite_no_lock ... ok

...22144 bytes truncated...

The tool call succeeded but the output was truncated. Full output saved to: /home/cfs/.local/share/opencode/tool-output/tool_cb7bacc10001zA6jLg1J3y4k9j
Use the Task tool to have explore agent process this file with Grep and Read (with offset/limit). Do NOT read the full file yourself - delegate to save context.[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-gateway 2>&1 | tail -20[0m

test xdr::tests::proptest_tests::prop_u64_roundtrip ... ok
test xdr::tests::proptest_tests::prop_encoding_alignment ... ok
test xdr::tests::proptest_tests::prop_opaque_variable_roundtrip ... ok
test xdr::tests::proptest_tests::prop_sequence_roundtrip ... ok
test xdr::tests::proptest_tests::prop_string_roundtrip ... ok

test result: ok. 1121 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.63s

     Running unittests src/main.rs (target/debug/deps/cfs_gateway-998a62f92c329b09)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests claudefs_gateway

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
All tasks completed successfully:

1. **Added proptest dev-dependency** to `Cargo.toml`
2. **Added proptest tests** to `xdr.rs` (10 new round-trip property tests)
3. **Created `nfs_listener.rs`** with async TCP listener using tokio
4. **Registered module** in `lib.rs`

Test results: **1121 tests passed** (original 1107 + 14 new: 10 proptest + 4 nfs_listener unit tests)
