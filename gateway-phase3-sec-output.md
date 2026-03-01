[0m
[0m

> build · accounts/fireworks/models/minimax-m2p5[0m→ [0mRead crates/claudefs-gateway/Cargo.toml
[0m→ [0mRead crates/claudefs-gateway/src/token_auth.rs
[0m→ [0mRead crates/claudefs-gateway/src/auth.rs
[0m[0m← [0mEdit crates/claudefs-gateway/Cargo.toml

[0mIndex: /home/cfs/claudefs/crates/claudefs-gateway/Cargo.toml
===================================================================
--- /home/cfs/claudefs/crates/claudefs-gateway/Cargo.toml
+++ /home/cfs/claudefs/crates/claudefs-gateway/Cargo.toml
@@ -18,8 +18,10 @@
 prost.workspace = true
 tonic.workspace = true
 tracing.workspace = true
 tracing-subscriber.workspace = true
+rand.workspace = true
+sha2.workspace = true
 
 [lib]
 name = "claudefs_gateway"
 path = "src/lib.rs"
\ No newline at end of file


[0m← [0mEdit crates/claudefs-gateway/src/token_auth.rsIndex: /home/cfs/claudefs/crates/claudefs-gateway/src/token_auth.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-gateway/src/token_auth.rs
+++ /home/cfs/claudefs/crates/claudefs-gateway/src/token_auth.rs
@@ -2,8 +2,16 @@
 
 use std::collections::HashMap;
 use std::sync::Mutex;
 
+use rand::RngCore;
+use sha2::{Digest, Sha256};
+
+fn sha256_hex(input: &str) -> String {
+    let hash = Sha256::digest(input.as_bytes());
+    hash.iter().map(|b| format!("{:02x}", b)).collect()
+}
+
 /// Token permissions bitmask
 #[derive(Debug, Clone, Copy, Default)]
 pub struct TokenPermissions {
     pub read: bool,


[0m

[0m
[0m← [0mEdit crates/claudefs-gateway/src/token_auth.rsIndex: /home/cfs/claudefs/crates/claudefs-gateway/src/token_auth.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-gateway/src/token_auth.rs
+++ /home/cfs/claudefs/crates/claudefs-gateway/src/token_auth.rs
@@ -64,9 +64,9 @@
 
 impl AuthToken {
     pub fn new(token: &str, uid: u32, gid: u32, name: &str) -> Self {
         Self {
-            token: token.to_string(),
+            token: sha256_hex(token),
             uid,
             gid,
             name: name.to_string(),
             permissions: TokenPermissions::default(),

[0m← [0mEdit crates/claudefs-gateway/src/token_auth.rs
[0mIndex: /home/cfs/claudefs/crates/claudefs-gateway/src/token_auth.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-gateway/src/token_auth.rs
+++ /home/cfs/claudefs/crates/claudefs-gateway/src/token_auth.rs
@@ -116,16 +116,16 @@
     /// Register a token
     pub fn register(&self, token: AuthToken) {
         self.tokens
             .lock()
-            .unwrap()
+            .unwrap_or_else(|e| e.into_inner())
             .insert(token.token.clone(), token);
     }
 
     /// Validate a token string, return the token if valid and not expired
     pub fn validate(&self, token: &str, now: u64) -> Option<AuthToken> {
-        let tokens = self.tokens.lock().unwrap();
-        tokens.get(token).and_then(|t| {
+        let tokens = self.tokens.lock().unwrap_or_else(|e| e.into_inner());
+        tokens.get(&sha256_hex(token)).and_then(|t| {
             if t.is_expired(now) {
                 None
             } else {
                 Some(t.clone())
@@ -134,16 +134,20 @@
     }
 
     /// Revoke a token by string
     pub fn revoke(&self, token: &str) -> bool {
-        self.tokens.lock().unwrap().remove(token).is_some()
+        self.tokens
+            .lock()
+            .unwrap_or_else(|e| e.into_inner())
+            .remove(&sha256_hex(token))
+            .is_some()
     }
 
     /// List all tokens for a user (by uid)
     pub fn tokens_for_user(&self, uid: u32) -> Vec<AuthToken> {
         self.tokens
             .lock()
-            .unwrap()
+            .unwrap_or_else(|e| e.into_inner())
             .values()
             .filter(|t| t.uid == uid)
             .cloned()
             .collect()
@@ -152,30 +156,36 @@
     /// Count valid (non-expired) tokens at a given time
     pub fn valid_count(&self, now: u64) -> usize {
         self.tokens
             .lock()
-            .unwrap()
+            .unwrap_or_else(|e| e.into_inner())
             .values()
             .filter(|t| !t.is_expired(now))
             .count()
     }
 
     /// Remove all expired tokens
     pub fn cleanup_expired(&self, now: u64) -> usize {
-        let mut tokens = self.tokens.lock().unwrap();
+        let mut tokens = self.tokens.lock().unwrap_or_else(|e| e.into_inner());
         let before = tokens.len();
         tokens.retain(|_, t| !t.is_expired(now));
         before - tokens.len()
     }
 
-    /// Generate a simple token string (hex of counter + uid)
-    pub fn generate_token(uid: u32, counter: u64) -> String {
-        format!("{:016x}{:08x}", counter, uid)
+    /// Generate a cryptographically secure token string (64 hex chars)
+    pub fn generate_token() -> String {
+        let mut rng = rand::rngs::OsRng;
+        let mut bytes = [0u8; 32];
+        rng.fill_bytes(&mut bytes);
+        bytes.iter().map(|b| format!("{:02x}", b)).collect()
     }
 
     /// Check if a token string exists (regardless of expiry)
     pub fn exists(&self, token: &str) -> bool {
-        self.tokens.lock().unwrap().contains_key(token)
+        self.tokens
+            .lock()
+            .unwrap_or_else(|e| e.into_inner())
+            .contains_key(&sha256_hex(token))
     }
 }
 
 impl Default for TokenAuth {


[0m✗ [0medit failed
[91m[1mError: [0mFile /home/cfs/claudefs/crates/claudefs-gateway/src/token_auth.rs has been modified since it was last read.
Last modification: 2026-03-01T12:04:32.693Z
Last read: 2026-03-01T12:04:21.652Z

Please read the file again before modifying it.
[0m→ [0mRead crates/claudefs-gateway/src/token_auth.rs[90m [offset=175, limit=100][0m
[0m
[0m← [0mEdit crates/claudefs-gateway/src/token_auth.rs[0m


Index: /home/cfs/claudefs/crates/claudefs-gateway/src/token_auth.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-gateway/src/token_auth.rs
+++ /home/cfs/claudefs/crates/claudefs-gateway/src/token_auth.rs
@@ -224,9 +224,10 @@
 
 #[test]
 fn test_auth_token_new() {
     let token = AuthToken::new("abc123", 1000, 100, "testuser");
-    assert_eq!(token.token, "abc123");
+    assert_ne!(token.token, "abc123");
+    assert_eq!(token.token.len(), 64);
     assert_eq!(token.uid, 1000);
     assert_eq!(token.gid, 100);
     assert_eq!(token.name, "testuser");
     assert_eq!(token.expires_at, 0);
[0m→ [0mRead crates/claudefs-gateway/src/token_auth.rs[90m [limit=30, offset=350][0m
[0m[0m← [0mEdit crates/claudefs-gateway/src/token_auth.rs



[0mIndex: /home/cfs/claudefs/crates/claudefs-gateway/src/token_auth.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-gateway/src/token_auth.rs
+++ /home/cfs/claudefs/crates/claudefs-gateway/src/token_auth.rs
@@ -366,10 +366,13 @@
 }
 
 #[test]
 fn test_token_auth_generate_token() {
-    let token = TokenAuth::generate_token(1000, 12345);
-    assert_eq!(token, "0000000000003039000003e8");
+    let token = TokenAuth::generate_token();
+    assert_eq!(token.len(), 64);
+    assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
+    let token2 = TokenAuth::generate_token();
+    assert_ne!(token, token2);
 }
 
 #[test]
 fn test_token_auth_exists() {
Index: /home/cfs/claudefs/crates/claudefs-gateway/src/auth.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-gateway/src/auth.rs
+++ /home/cfs/claudefs/crates/claudefs-gateway/src/auth.rs
@@ -9,8 +9,18 @@
 
 const NOBODY_UID: u32 = 65534;
 const NOBODY_GID: u32 = 65534;
 
+const AUTH_SYS_MAX_MACHINENAME_LEN: usize = 255;
+
+#[derive(Debug, Clone, Copy, PartialEq, Default)]
+pub enum SquashPolicy {
+    #[default]
+    RootSquash,
+    AllSquash,
+    None,
+}
+
 #[derive(Debug, Clone)]
 pub struct AuthSysCred {
     pub stamp: u32,
     pub machinename: String,


[0m← [0mEdit crates/claudefs-gateway/src/auth.rs[0m
[0m→ [0mRead crates/claudefs-gateway/src/auth.rs[90m [limit=50, offset=35][0m
[0m
[0m← [0mEdit crates/claudefs-gateway/src/auth.rsIndex: /home/cfs/claudefs/crates/claudefs-gateway/src/auth.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-gateway/src/auth.rs
+++ /home/cfs/claudefs/crates/claudefs-gateway/src/auth.rs
@@ -43,8 +43,17 @@
 let mut dec = XdrDecoder::new(prost::bytes::Bytes::copy_from_slice(body));
 
 let stamp = dec.decode_u32()?;
 let machinename = dec.decode_string()?;
+if machinename.len() > AUTH_SYS_MAX_MACHINENAME_LEN {
+    return Err(GatewayError::ProtocolError {
+        reason: format!(
+            "machinename too long: {} > {}",
+            machinename.len(),
+            AUTH_SYS_MAX_MACHINENAME_LEN
+        ),
+    });
+}
 let uid = dec.decode_u32()?;
 let gid = dec.decode_u32()?;
 
 let gids_count = dec.decode_u32()? as usize;


[0m
[0m→ [0mRead crates/claudefs-gateway/src/auth.rs[90m [limit=50, offset=120][0m
[0m[0m← [0mEdit crates/claudefs-gateway/src/auth.rs

[0m

Index: /home/cfs/claudefs/crates/claudefs-gateway/src/auth.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-gateway/src/auth.rs
+++ /home/cfs/claudefs/crates/claudefs-gateway/src/auth.rs
@@ -155,8 +155,38 @@
             AuthCred::Sys(cred) => cred.is_root(),
             _ => false,
         }
     }
+
+    pub fn effective_uid(&self, policy: SquashPolicy) -> u32 {
+        let raw = self.uid();
+        match policy {
+            SquashPolicy::None => raw,
+            SquashPolicy::RootSquash => {
+                if raw == 0 {
+                    NOBODY_UID
+                } else {
+                    raw
+                }
+            }
+            SquashPolicy::AllSquash => NOBODY_UID,
+        }
+    }
+
+    pub fn effective_gid(&self, policy: SquashPolicy) -> u32 {
+        let raw = self.gid();
+        match policy {
+            SquashPolicy::None => raw,
+            SquashPolicy::RootSquash => {
+                if raw == 0 {
+                    NOBODY_GID
+                } else {
+                    raw
+                }
+            }
+            SquashPolicy::AllSquash => NOBODY_GID,
+        }
+    }
 }
 
 #[cfg(test)]
 mod tests {
[0m→ [0mRead crates/claudefs-gateway/src/auth.rs[90m [limit=40, offset=395][0m
[0m

[0m← [0mEdit crates/claudefs-gateway/src/auth.rs[0m

Index: /home/cfs/claudefs/crates/claudefs-gateway/src/auth.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-gateway/src/auth.rs
+++ /home/cfs/claudefs/crates/claudefs-gateway/src/auth.rs
@@ -422,5 +422,112 @@
         };
         let cred_sys = AuthCred::from_opaque_auth(&opaque);
         assert!(cred_sys.is_root());
     }
+
+    #[test]
+    fn test_squash_policy_default_is_root_squash() {
+        assert_eq!(SquashPolicy::default(), SquashPolicy::RootSquash);
+    }
+
+    #[test]
+    fn test_effective_uid_root_squash_squashes_root() {
+        let cred = AuthSysCred {
+            stamp: 1,
+            machinename: "host".to_string(),
+            uid: 0,
+            gid: 0,
+            gids: vec![],
+        };
+        let opaque = OpaqueAuth {
+            flavor: AUTH_SYS,
+            body: cred.encode_xdr(),
+        };
+        let auth = AuthCred::from_opaque_auth(&opaque);
+        assert_eq!(auth.effective_uid(SquashPolicy::RootSquash), NOBODY_UID);
+        assert_eq!(auth.effective_gid(SquashPolicy::RootSquash), NOBODY_GID);
+    }
+
+    #[test]
+    fn test_effective_uid_root_squash_passes_nonroot() {
+        let cred = AuthSysCred {
+            stamp: 1,
+            machinename: "host".to_string(),
+            uid: 1000,
+            gid: 1000,
+            gids: vec![],
+        };
+        let opaque = OpaqueAuth {
+            flavor: AUTH_SYS,
+            body: cred.encode_xdr(),
+        };
+        let auth = AuthCred::from_opaque_auth(&opaque);
+        assert_eq!(auth.effective_uid(SquashPolicy::RootSquash), 1000);
+        assert_eq!(auth.effective_gid(SquashPolicy::RootSquash), 1000);
+    }
+
+    #[test]
+    fn test_effective_uid_all_squash() {
+        let cred = AuthSysCred {
+            stamp: 1,
+            machinename: "host".to_string(),
+            uid: 1000,
+            gid: 1000,
+            gids: vec![],
+        };
+        let opaque = OpaqueAuth {
+            flavor: AUTH_SYS,
+            body: cred.encode_xdr(),
+        };
+        let auth = AuthCred::from_opaque_auth(&opaque);
+        assert_eq!(auth.effective_uid(SquashPolicy::AllSquash), NOBODY_UID);
+        assert_eq!(auth.effective_gid(SquashPolicy::AllSquash), NOBODY_GID);
+    }
+
+    #[test]
+    fn test_effective_uid_none_policy_passes_root() {
+        let cred = AuthSysCred {
+            stamp: 1,
+            machinename: "host".to_string(),
+            uid: 0,
+            gid: 0,
+            gids: vec![],
+        };
+        let opaque = OpaqueAuth {
+            flavor: AUTH_SYS,
+            body: cred.encode_xdr(),
+        };
+        let auth = AuthCred::from_opaque_auth(&opaque);
+        assert_eq!(auth.effective_uid(SquashPolicy::None), 0);
+        assert_eq!(auth.effective_gid(SquashPolicy::None), 0);
+    }
+
+    #[test]
+    fn test_machinename_length_limit() {
+        let long_name = "a".repeat(256);
+        let cred = AuthSysCred {
+            stamp: 1,
+            machinename: long_name.clone(),
+            uid: 1000,
+            gid: 1000,
+            gids: vec![],
+        };
+        let encoded = cred.encode_xdr();
+        let result = AuthSysCred::decode_xdr(&encoded);
+        assert!(result.is_err());
+    }
+
+    #[test]
+    fn test_machinename_max_length_ok() {
+        let max_name = "a".repeat(255);
+        let cred = AuthSysCred {
+            stamp: 1,
+            machinename: max_name,
+            uid: 1000,
+            gid: 1000,
+            gids: vec![],
+        };
+        let encoded = cred.encode_xdr();
+        let result = AuthSysCred::decode_xdr(&encoded);
+        assert!(result.is_ok());
+    }
 }
[0m$ [0mcargo test -p claudefs-gateway 2>&1
[0m
Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
   Compiling crypto-common v0.1.7
   Compiling digest v0.10.7
   Compiling sha2 v0.10.9
   Compiling claudefs-gateway v0.1.0 (/home/cfs/claudefs/crates/claudefs-gateway)
warning: unused import: `prost::bytes::Bytes`
   --> crates/claudefs-gateway/src/protocol.rs:324:9
    |
324 |     use prost::bytes::Bytes;
    |         ^^^^^^^^^^^^^^^^^^^
    |
    = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/access_log.rs:11:5
   |
11 |     Nfs3,
   |     ^^^^
   |
note: the lint level is defined here
  --> crates/claudefs-gateway/src/lib.rs:1:9
   |
 1 | #![warn(missing_docs)]
   |         ^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/access_log.rs:12:5
   |
12 |     Nfs4,
   |     ^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/access_log.rs:13:5
   |
13 |     S3,
   |     ^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/access_log.rs:14:5
   |
14 |     Smb3,
   |     ^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-gateway/src/access_log.rs:41:5
   |
41 | /     pub fn new(
42 | |         client_ip: &str,
43 | |         protocol: GatewayProtocol,
44 | |         operation: &str,
45 | |         resource: &str,
46 | |     ) -> Self {
   | |_____________^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/access_log.rs:64:5
   |
64 |     pub fn with_status(mut self, status: u32) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/access_log.rs:69:5
   |
69 |     pub fn with_bytes(mut self, bytes: u64) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/access_log.rs:74:5
   |
74 |     pub fn with_duration_us(mut self, duration_us: u64) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/access_log.rs:79:5
   |
79 |     pub fn with_uid(mut self, uid: u32) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-gateway/src/access_log.rs:125:5
    |
125 |     pub total_requests: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-gateway/src/access_log.rs:126:5
    |
126 |     pub error_count: u64,
    |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-gateway/src/access_log.rs:127:5
    |
127 |     pub total_bytes: u64,
    |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-gateway/src/access_log.rs:128:5
    |
128 |     pub total_duration_us: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/access_log.rs:132:5
    |
132 |     pub fn add_entry(&mut self, entry: &AccessLogEntry) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/access_log.rs:141:5
    |
141 |     pub fn avg_duration_us(&self) -> u64 {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/access_log.rs:149:5
    |
149 |     pub fn error_rate(&self) -> f64 {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/access_log.rs:157:5
    |
157 |     pub fn requests_per_sec(&self, window_secs: u64) -> f64 {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-gateway/src/access_log.rs:174:5
    |
174 |     pub fn new(capacity: usize) -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-gateway/src/auth.rs:16:1
   |
16 | pub enum SquashPolicy {
   | ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/auth.rs:18:5
   |
18 |     RootSquash,
   |     ^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/auth.rs:19:5
   |
19 |     AllSquash,
   |     ^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/auth.rs:20:5
   |
20 |     None,
   |     ^^^^

warning: missing documentation for a struct
  --> crates/claudefs-gateway/src/auth.rs:24:1
   |
24 | pub struct AuthSysCred {
   | ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/auth.rs:25:5
   |
25 |     pub stamp: u32,
   |     ^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/auth.rs:26:5
   |
26 |     pub machinename: String,
   |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/auth.rs:27:5
   |
27 |     pub uid: u32,
   |     ^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/auth.rs:28:5
   |
28 |     pub gid: u32,
   |     ^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/auth.rs:29:5
   |
29 |     pub gids: Vec<u32>,
   |     ^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-gateway/src/auth.rs:33:5
   |
33 |     pub fn from_opaque_auth(auth: &OpaqueAuth) -> Result<Self> {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-gateway/src/auth.rs:42:5
   |
42 |     pub fn decode_xdr(body: &[u8]) -> Result<Self> {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/auth.rs:80:5
   |
80 |     pub fn encode_xdr(&self) -> Vec<u8> {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/auth.rs:93:5
   |
93 |     pub fn has_uid(&self, uid: u32) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/auth.rs:97:5
   |
97 |     pub fn has_gid(&self, gid: u32) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/auth.rs:104:5
    |
104 |     pub fn is_root(&self) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-gateway/src/auth.rs:110:1
    |
110 | pub struct AuthNone;
    | ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-gateway/src/auth.rs:113:5
    |
113 |     pub fn to_opaque_auth() -> OpaqueAuth {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
   --> crates/claudefs-gateway/src/auth.rs:119:1
    |
119 | pub enum AuthCred {
    | ^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/auth.rs:120:5
    |
120 |     None,
    |     ^^^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/auth.rs:121:5
    |
121 |     Sys(AuthSysCred),
    |     ^^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/auth.rs:122:5
    |
122 |     Unknown(u32),
    |     ^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-gateway/src/auth.rs:126:5
    |
126 |     pub fn from_opaque_auth(auth: &OpaqueAuth) -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/auth.rs:137:5
    |
137 |     pub fn uid(&self) -> u32 {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/auth.rs:145:5
    |
145 |     pub fn gid(&self) -> u32 {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/auth.rs:153:5
    |
153 |     pub fn is_root(&self) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/auth.rs:160:5
    |
160 |     pub fn effective_uid(&self, policy: SquashPolicy) -> u32 {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/auth.rs:175:5
    |
175 |     pub fn effective_gid(&self, policy: SquashPolicy) -> u32 {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
 --> crates/claudefs-gateway/src/config.rs:9:1
  |
9 | pub struct BindAddr {
  | ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/config.rs:10:5
   |
10 |     pub addr: String,
   |     ^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/config.rs:11:5
   |
11 |     pub port: u16,
   |     ^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-gateway/src/config.rs:15:5
   |
15 |     pub fn new(addr: &str, port: u16) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-gateway/src/config.rs:22:5
   |
22 |     pub fn nfs_default() -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-gateway/src/config.rs:29:5
   |
29 |     pub fn mount_default() -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-gateway/src/config.rs:36:5
   |
36 |     pub fn s3_default() -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/config.rs:43:5
   |
43 |     pub fn to_socket_addr_string(&self) -> String {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-gateway/src/config.rs:55:1
   |
55 | pub struct ExportConfig {
   | ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/config.rs:56:5
   |
56 |     pub path: String,
   |     ^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/config.rs:57:5
   |
57 |     pub allowed_clients: Vec<String>,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/config.rs:58:5
   |
58 |     pub read_only: bool,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/config.rs:59:5
   |
59 |     pub root_squash: bool,
   |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/config.rs:60:5
   |
60 |     pub anon_uid: u32,
   |     ^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/config.rs:61:5
   |
61 |     pub anon_gid: u32,
   |     ^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-gateway/src/config.rs:65:5
   |
65 |     pub fn default_rw(path: &str) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-gateway/src/config.rs:76:5
   |
76 |     pub fn default_ro(path: &str) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/config.rs:87:5
   |
87 |     pub fn to_export_entry(&self) -> ExportEntry {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-gateway/src/config.rs:96:1
   |
96 | pub struct S3Config {
   | ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/config.rs:97:5
   |
97 |     pub bind: BindAddr,
   |     ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/config.rs:98:5
   |
98 |     pub region: String,
   |     ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/config.rs:99:5
   |
99 |     pub max_object_size: u64,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-gateway/src/config.rs:100:5
    |
100 |     pub multipart_chunk_min: u64,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-gateway/src/config.rs:101:5
    |
101 |     pub enable_versioning: bool,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-gateway/src/config.rs:105:5
    |
105 |     pub fn new() -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-gateway/src/config.rs:123:1
    |
123 | pub struct NfsConfig {
    | ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-gateway/src/config.rs:124:5
    |
124 |     pub bind: BindAddr,
    |     ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-gateway/src/config.rs:125:5
    |
125 |     pub mount_bind: BindAddr,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-gateway/src/config.rs:126:5
    |
126 |     pub exports: Vec<ExportConfig>,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-gateway/src/config.rs:127:5
    |
127 |     pub fsid: u64,
    |     ^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-gateway/src/config.rs:128:5
    |
128 |     pub max_read_size: u32,
    |     ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-gateway/src/config.rs:129:5
    |
129 |     pub max_write_size: u32,
    |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-gateway/src/config.rs:130:5
    |
130 |     pub enable_pnfs: bool,
    |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-gateway/src/config.rs:131:5
    |
131 |     pub pnfs_data_servers: Vec<String>,
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-gateway/src/config.rs:135:5
    |
135 |     pub fn default_with_export(path: &str) -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-gateway/src/config.rs:156:1
    |
156 | pub struct GatewayConfig {
    | ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-gateway/src/config.rs:157:5
    |
157 |     pub nfs: NfsConfig,
    |     ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-gateway/src/config.rs:158:5
    |
158 |     pub s3: S3Config,
    |     ^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-gateway/src/config.rs:159:5
    |
159 |     pub enable_nfs: bool,
    |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-gateway/src/config.rs:160:5
    |
160 |     pub enable_s3: bool,
    |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-gateway/src/config.rs:161:5
    |
161 |     pub enable_smb: bool,
    |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
   --> crates/claudefs-gateway/src/config.rs:162:5
    |
162 |     pub log_level: String,
    |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-gateway/src/config.rs:166:5
    |
166 |     pub fn default_with_export(path: &str) -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/config.rs:177:5
    |
177 |     pub fn any_enabled(&self) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/config.rs:181:5
    |
181 |     pub fn validate(&self) -> Result<()> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
  --> crates/claudefs-gateway/src/error.rs:39:1
   |
39 | pub enum GatewayError {
   | ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/error.rs:41:5
   |
41 |     Nfs3NoEnt,
   |     ^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/error.rs:43:5
   |
43 |     Nfs3Io,
   |     ^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/error.rs:45:5
   |
45 |     Nfs3Acces,
   |     ^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/error.rs:47:5
   |
47 |     Nfs3Exist,
   |     ^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/error.rs:49:5
   |
49 |     Nfs3NotDir,
   |     ^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/error.rs:51:5
   |
51 |     Nfs3IsDir,
   |     ^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/error.rs:53:5
   |
53 |     Nfs3Inval,
   |     ^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/error.rs:55:5
   |
55 |     Nfs3FBig,
   |     ^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/error.rs:57:5
   |
57 |     Nfs3NoSpc,
   |     ^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/error.rs:59:5
   |
59 |     Nfs3ROfs,
   |     ^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/error.rs:61:5
   |
61 |     Nfs3Stale,
   |     ^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/error.rs:63:5
   |
63 |     Nfs3BadHandle,
   |     ^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/error.rs:65:5
   |
65 |     Nfs3NotSupp,
   |     ^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/error.rs:67:5
   |
67 |     Nfs3ServerFault,
   |     ^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/error.rs:69:5
   |
69 |     S3BucketNotFound { bucket: String },
   |     ^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/error.rs:69:24
   |
69 |     S3BucketNotFound { bucket: String },
   |                        ^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/error.rs:71:5
   |
71 |     S3ObjectNotFound { key: String },
   |     ^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/error.rs:71:24
   |
71 |     S3ObjectNotFound { key: String },
   |                        ^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/error.rs:73:5
   |
73 |     S3InvalidBucketName { name: String },
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/error.rs:73:27
   |
73 |     S3InvalidBucketName { name: String },
   |                           ^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/error.rs:75:5
   |
75 |     S3AccessDenied,
   |     ^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/error.rs:77:5
   |
77 |     XdrDecodeError { reason: String },
   |     ^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/error.rs:77:22
   |
77 |     XdrDecodeError { reason: String },
   |                      ^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/error.rs:79:5
   |
79 |     XdrEncodeError { reason: String },
   |     ^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/error.rs:79:22
   |
79 |     XdrEncodeError { reason: String },
   |                      ^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/error.rs:81:5
   |
81 |     ProtocolError { reason: String },
   |     ^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/error.rs:81:21
   |
81 |     ProtocolError { reason: String },
   |                     ^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/error.rs:83:5
   |
83 |     BackendError { reason: String },
   |     ^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/error.rs:83:20
   |
83 |     BackendError { reason: String },
   |                    ^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/error.rs:85:5
   |
85 |     NotImplemented { feature: String },
   |     ^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/error.rs:85:22
   |
85 |     NotImplemented { feature: String },
   |                      ^^^^^^^^^^^^^^^

warning: missing documentation for a variant
  --> crates/claudefs-gateway/src/error.rs:87:5
   |
87 |     IoError(#[from] std::io::Error),
   |     ^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/error.rs:91:5
   |
91 |     pub fn nfs3_status(&self) -> u32 {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a type alias
   --> crates/claudefs-gateway/src/error.rs:127:1
    |
127 | pub type Result<T> = std::result::Result<T, GatewayError>;
    | ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/export_manager.rs:23:5
   |
23 |     pub config: ExportConfig,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/export_manager.rs:24:5
   |
24 |     pub status: ExportStatus,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/export_manager.rs:25:5
   |
25 |     pub client_count: u32,
   |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/export_manager.rs:26:5
   |
26 |     pub root_fh: FileHandle3,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/export_manager.rs:27:5
   |
27 |     pub root_inode: u64,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-gateway/src/export_manager.rs:31:5
   |
31 |     pub fn new(config: ExportConfig, root_fh: FileHandle3, root_inode: u64) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/export_manager.rs:41:5
   |
41 |     pub fn is_active(&self) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/export_manager.rs:45:5
   |
45 |     pub fn can_remove(&self) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-gateway/src/export_manager.rs:57:5
   |
57 |     pub fn new() -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/export_manager.rs:64:5
   |
64 | /     pub fn add_export(
65 | |         &self,
66 | |         config: ExportConfig,
67 | |         root_inode: u64,
68 | |     ) -> crate::error::Result<FileHandle3> {
   | |__________________________________________^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/export_manager.rs:100:5
    |
100 |     pub fn remove_export(&self, path: &str) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/export_manager.rs:119:5
    |
119 |     pub fn force_remove_export(&self, path: &str) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/export_manager.rs:133:5
    |
133 |     pub fn get_export(&self, path: &str) -> Option<ActiveExport> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/export_manager.rs:142:5
    |
142 |     pub fn list_exports(&self) -> Vec<ActiveExport> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/export_manager.rs:151:5
    |
151 |     pub fn export_paths(&self) -> Vec<String> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/export_manager.rs:160:5
    |
160 |     pub fn is_exported(&self, path: &str) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/export_manager.rs:169:5
    |
169 |     pub fn increment_clients(&self, path: &str) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/export_manager.rs:183:5
    |
183 |     pub fn decrement_clients(&self, path: &str) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/export_manager.rs:199:5
    |
199 |     pub fn root_fh(&self, path: &str) -> Option<FileHandle3> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/export_manager.rs:208:5
    |
208 |     pub fn reload(&self, configs: Vec<ExportConfig>) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/export_manager.rs:241:5
    |
241 |     pub fn count(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/export_manager.rs:250:5
    |
250 |     pub fn total_clients(&self) -> u32 {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/health.rs:19:5
   |
19 |     pub fn is_ok(&self) -> bool {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/health.rs:23:5
   |
23 |     pub fn to_str(&self) -> &'static str {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/health.rs:36:5
   |
36 |     pub name: String,
   |     ^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/health.rs:37:5
   |
37 |     pub status: HealthStatus,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/health.rs:38:5
   |
38 |     pub message: String,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/health.rs:39:5
   |
39 |     pub duration_ms: u64,
   |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-gateway/src/health.rs:43:5
   |
43 |     pub fn ok(name: &str, duration_ms: u64) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-gateway/src/health.rs:52:5
   |
52 |     pub fn degraded(name: &str, message: &str, duration_ms: u64) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-gateway/src/health.rs:61:5
   |
61 |     pub fn unhealthy(name: &str, message: &str, duration_ms: u64) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/health.rs:74:5
   |
74 |     pub overall: HealthStatus,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/health.rs:75:5
   |
75 |     pub checks: Vec<CheckResult>,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/health.rs:76:5
   |
76 |     pub timestamp: u64,
   |     ^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-gateway/src/health.rs:80:5
   |
80 |     pub fn new(checks: Vec<CheckResult>, timestamp: u64) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/health.rs:103:5
    |
103 |     pub fn is_ready(&self) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/health.rs:107:5
    |
107 |     pub fn passed_count(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/health.rs:114:5
    |
114 |     pub fn failed_count(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-gateway/src/health.rs:128:5
    |
128 |     pub fn new() -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/health.rs:134:5
    |
134 |     pub fn register_result(&self, result: CheckResult) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/health.rs:143:5
    |
143 |     pub fn update_result(&self, name: &str, status: HealthStatus, message: &str) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/health.rs:154:5
    |
154 |     pub fn report(&self, timestamp: u64) -> HealthReport {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/health.rs:159:5
    |
159 |     pub fn is_healthy(&self) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/health.rs:167:5
    |
167 |     pub fn is_ready(&self) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/health.rs:175:5
    |
175 |     pub fn check_count(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/health.rs:179:5
    |
179 |     pub fn remove_check(&self, name: &str) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/health.rs:186:5
    |
186 |     pub fn clear(&self) {
    |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-gateway/src/mount.rs:42:1
   |
42 | pub struct MountEntry {
   | ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/mount.rs:43:5
   |
43 |     pub hostname: String,
   |     ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/mount.rs:44:5
   |
44 |     pub dirpath: String,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-gateway/src/mount.rs:48:1
   |
48 | pub struct ExportEntry {
   | ^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/mount.rs:49:5
   |
49 |     pub dirpath: String,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/mount.rs:50:5
   |
50 |     pub groups: Vec<String>,
   |     ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-gateway/src/mount.rs:54:1
   |
54 | pub struct MntResult {
   | ^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/mount.rs:55:5
   |
55 |     pub status: u32,
   |     ^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/mount.rs:56:5
   |
56 |     pub filehandle: Option<FileHandle3>,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/mount.rs:57:5
   |
57 |     pub auth_flavors: Vec<u32>,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-gateway/src/mount.rs:60:1
   |
60 | pub struct MountHandler {
   | ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
  --> crates/claudefs-gateway/src/mount.rs:67:5
   |
67 |     pub fn new(exports: Vec<ExportEntry>, root_fh: FileHandle3) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/mount.rs:75:5
   |
75 |     pub fn null(&self) {}
   |     ^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/mount.rs:77:5
   |
77 |     pub fn mnt(&self, path: &str, client_host: &str) -> MntResult {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/mount.rs:113:5
    |
113 |     pub fn dump(&self) -> Vec<MountEntry> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/mount.rs:117:5
    |
117 |     pub fn umnt(&self, path: &str) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/mount.rs:123:5
    |
123 |     pub fn umntall(&self) {
    |     ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/mount.rs:129:5
    |
129 |     pub fn export(&self) -> Vec<ExportEntry> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/mount.rs:133:5
    |
133 |     pub fn is_exported(&self, path: &str) -> Option<&ExportEntry> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/mount.rs:137:5
    |
137 |     pub fn is_allowed(&self, export: &ExportEntry, client_host: &str) -> bool {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-gateway/src/mount.rs:147:5
    |
147 |     pub fn encode_mnt_result(result: &MntResult, enc: &mut XdrEncoder) {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/mount.rs:166:5
    |
166 |     pub fn mount_count(&self) -> usize {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a trait
  --> crates/claudefs-gateway/src/nfs.rs:12:1
   |
12 | pub trait VfsBackend: Send + Sync {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/nfs.rs:13:5
   |
13 |     fn getattr(&self, fh: &FileHandle3) -> Result<Fattr3>;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/nfs.rs:14:5
   |
14 |     fn lookup(&self, dir_fh: &FileHandle3, name: &str) -> Result<LookupResult>;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/nfs.rs:15:5
   |
15 |     fn read(&self, fh: &FileHandle3, offset: u64, count: u32) -> Result<(Vec<u8>, bool)>;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/nfs.rs:16:5
   |
16 |     fn write(&self, fh: &FileHandle3, offset: u64, data: &[u8]) -> Result<u32>;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/nfs.rs:17:5
   |
17 |     fn readdir(&self, dir_fh: &FileHandle3, cookie: u64, count: u32) -> Result<ReadDirResult>;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/nfs.rs:18:5
   |
18 |     fn mkdir(&self, dir_fh: &FileHandle3, name: &str, mode: u32) -> Result<(FileHandle3, Fattr3)>;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/nfs.rs:19:5
   |
19 |     fn create(&self, dir_fh: &FileHandle3, name: &str, mode: u32) -> Result<(FileHandle3, Fattr3)>;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/nfs.rs:20:5
   |
20 |     fn remove(&self, dir_fh: &FileHandle3, name: &str) -> Result<()>;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/nfs.rs:21:5
   |
21 | /     fn rename(
22 | |         &self,
23 | |         from_dir: &FileHandle3,
24 | |         from_name: &str,
25 | |         to_dir: &FileHandle3,
26 | |         to_name: &str,
27 | |     ) -> Result<()>;
   | |____________________^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/nfs.rs:28:5
   |
28 |     fn readlink(&self, fh: &FileHandle3) -> Result<String>;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/nfs.rs:29:5
   |
29 | /     fn symlink(
30 | |         &self,
31 | |         dir_fh: &FileHandle3,
32 | |         name: &str,
33 | |         target: &str,
34 | |     ) -> Result<(FileHandle3, Fattr3)>;
   | |_______________________________________^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/nfs.rs:35:5
   |
35 |     fn fsstat(&self, fh: &FileHandle3) -> Result<FsStatResult>;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/nfs.rs:36:5
   |
36 |     fn fsinfo(&self, fh: &FileHandle3) -> Result<FsInfoResult>;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/nfs.rs:37:5
   |
37 |     fn pathconf(&self, fh: &FileHandle3) -> Result<PathConfResult>;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
  --> crates/claudefs-gateway/src/nfs.rs:38:5
   |
38 |     fn access(&self, fh: &FileHandle3, uid: u32, gid: u32, access_bits: u32) -> Result<u32>;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct
  --> crates/claudefs-gateway/src/nfs.rs:42:1
   |
42 | pub struct InodeEntry {
   | ^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/nfs.rs:43:5
   |
43 |     pub ftype: Ftype3,
   |     ^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/nfs.rs:44:5
   |
44 |     pub mode: u32,
   |     ^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/nfs.rs:45:5
   |
45 |     pub size: u64,
   |     ^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/nfs.rs:46:5
   |
46 |     pub data: Vec<u8>,
   |     ^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/nfs.rs:47:5
   |
47 |     pub children: HashMap<String, u64>,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/nfs.rs:48:5
   |
48 |     pub link_target: Option<String>,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/nfs.rs:49:5
   |
49 |     pub uid: u32,
   |     ^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/nfs.rs:50:5
   |
50 |     pub gid: u32,
   |     ^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/nfs.rs:51:5
   |
51 |     pub atime: Nfstime3,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/nfs.rs:52:5
   |
52 |     pub mtime: Nfstime3,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/nfs.rs:53:5
   |
53 |     pub ctime: Nfstime3,
   |     ^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a struct field
  --> crates/claudefs-gateway/src/nfs.rs:54:5
   |
54 |     pub nlink: u32,
   |     ^^^^^^^^^^^^^^

warning: missing documentation for a struct
   --> crates/claudefs-gateway/src/nfs.rs:118:1
    |
118 | pub struct MockVfsBackend {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-gateway/src/nfs.rs:125:5
    |
125 |     pub fn new(fsid: u64) -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an enum
   --> crates/claudefs-gateway/src/nfs.rs:515:1
    |
515 | pub enum Nfs3GetAttrResult {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:516:5
    |
516 |     Ok(Fattr3),
    |     ^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:517:5
    |
517 |     Err(u32),
    |     ^^^

warning: missing documentation for an enum
   --> crates/claudefs-gateway/src/nfs.rs:520:1
    |
520 | pub enum Nfs3LookupResult {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:521:5
    |
521 |     Ok(LookupResult),
    |     ^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:522:5
    |
522 |     Err(u32),
    |     ^^^

warning: missing documentation for an enum
   --> crates/claudefs-gateway/src/nfs.rs:525:1
    |
525 | pub enum Nfs3ReadResult {
    | ^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:526:5
    |
526 |     Ok(Vec<u8>, bool),
    |     ^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:527:5
    |
527 |     Err(u32),
    |     ^^^

warning: missing documentation for an enum
   --> crates/claudefs-gateway/src/nfs.rs:530:1
    |
530 | pub enum Nfs3WriteResult {
    | ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:531:5
    |
531 |     Ok(u32, u32),
    |     ^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:532:5
    |
532 |     Err(u32),
    |     ^^^

warning: missing documentation for an enum
   --> crates/claudefs-gateway/src/nfs.rs:535:1
    |
535 | pub enum Nfs3CreateResult {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:536:5
    |
536 |     Ok(FileHandle3, Fattr3),
    |     ^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:537:5
    |
537 |     Err(u32),
    |     ^^^

warning: missing documentation for an enum
   --> crates/claudefs-gateway/src/nfs.rs:540:1
    |
540 | pub enum Nfs3MkdirResult {
    | ^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:541:5
    |
541 |     Ok(FileHandle3, Fattr3),
    |     ^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:542:5
    |
542 |     Err(u32),
    |     ^^^

warning: missing documentation for an enum
   --> crates/claudefs-gateway/src/nfs.rs:545:1
    |
545 | pub enum Nfs3RemoveResult {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:546:5
    |
546 |     Ok,
    |     ^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:547:5
    |
547 |     Err(u32),
    |     ^^^

warning: missing documentation for an enum
   --> crates/claudefs-gateway/src/nfs.rs:550:1
    |
550 | pub enum Nfs3RenameResult {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:551:5
    |
551 |     Ok,
    |     ^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:552:5
    |
552 |     Err(u32),
    |     ^^^

warning: missing documentation for an enum
   --> crates/claudefs-gateway/src/nfs.rs:555:1
    |
555 | pub enum Nfs3ReadDirResult {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:556:5
    |
556 |     Ok(ReadDirResult),
    |     ^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:557:5
    |
557 |     Err(u32),
    |     ^^^

warning: missing documentation for an enum
   --> crates/claudefs-gateway/src/nfs.rs:560:1
    |
560 | pub enum Nfs3FsStatResult {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:561:5
    |
561 |     Ok(FsStatResult),
    |     ^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:562:5
    |
562 |     Err(u32),
    |     ^^^

warning: missing documentation for an enum
   --> crates/claudefs-gateway/src/nfs.rs:565:1
    |
565 | pub enum Nfs3FsInfoResult {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:566:5
    |
566 |     Ok(FsInfoResult),
    |     ^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:567:5
    |
567 |     Err(u32),
    |     ^^^

warning: missing documentation for an enum
   --> crates/claudefs-gateway/src/nfs.rs:570:1
    |
570 | pub enum Nfs3PathConfResult {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:571:5
    |
571 |     Ok(PathConfResult),
    |     ^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:572:5
    |
572 |     Err(u32),
    |     ^^^

warning: missing documentation for an enum
   --> crates/claudefs-gateway/src/nfs.rs:575:1
    |
575 | pub enum Nfs3AccessResult {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:576:5
    |
576 |     Ok(u32),
    |     ^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:577:5
    |
577 |     Err(u32),
    |     ^^^

warning: missing documentation for an enum
   --> crates/claudefs-gateway/src/nfs.rs:580:1
    |
580 | pub enum Nfs3ReadLinkResult {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:581:5
    |
581 |     Ok(String),
    |     ^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:582:5
    |
582 |     Err(u32),
    |     ^^^

warning: missing documentation for an enum
   --> crates/claudefs-gateway/src/nfs.rs:585:1
    |
585 | pub enum Nfs3SymLinkResult {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:586:5
    |
586 |     Ok(FileHandle3, Fattr3),
    |     ^^

warning: missing documentation for a variant
   --> crates/claudefs-gateway/src/nfs.rs:587:5
    |
587 |     Err(u32),
    |     ^^^

warning: missing documentation for a struct
   --> crates/claudefs-gateway/src/nfs.rs:590:1
    |
590 | pub struct Nfs3Handler<B: VfsBackend> {
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for an associated function
   --> crates/claudefs-gateway/src/nfs.rs:597:5
    |
597 |     pub fn new(backend: Arc<B>, fsid: u64) -> Self {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/nfs.rs:601:5
    |
601 |     pub fn handle_getattr(&self, fh: &FileHandle3) -> Nfs3GetAttrResult {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: missing documentation for a method
   --> crates/claudefs-gateway/src/nfs.rs:608:5
    |
608 |     pub fn handle_lookup(&self, dir_fh: &FileHandle3, name: &str) -> Nfs3LookupResult {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^


...167635 bytes truncated...

The tool call succeeded but the output was truncated. Full output saved to: /home/cfs/.local/share/opencode/tool-output/tool_ca94a307d001nu45whKmSP2h27
Use the Task tool to have explore agent process this file with Grep and Read (with offset/limit). Do NOT read the full file yourself - delegate to save context.
[0m$ [0mcargo test -p claudefs-gateway 2>&1 | tail -20
test xdr::tests::test_roundtrip_multiple_values ... ok
test nfs_cache::tests::test_cached_attr_is_expired ... ok
test nfs_cache::tests::test_attr_cache_ttl_expiry ... ok
test nfs_cache::tests::test_attr_cache_evict_expired ... ok
test nfs_cache::tests::test_cached_attr_age_ms ... ok

test result: ok. 615 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s

     Running unittests src/main.rs (target/debug/deps/cfs_gateway-74fe45d4f67ffeda)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests claudefs_gateway

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s[0m

All 615 tests passed (original 608 + 7 new tests). Security fixes complete:

1. **Cargo.toml**: Added `rand.workspace = true` and `sha2.workspace = true`
2. **token_auth.rs**:
   - FINDING-16: `generate_token()` now uses CSPRNG (OsRng), returns 64-char hex
   - FINDING-18: Tokens stored as SHA-256 hashes in HashMap
   - FINDING-19: All `.lock().unwrap()` replaced with `.lock().unwrap_or_else(|e| e.into_inner())`
   - Updated tests for new API
3. **auth.rs**:
   - FINDING-17/20: Added `SquashPolicy` enum with `RootSquash` (default), `AllSquash`, `None`
   - Added `effective_uid()` and `effective_gid()` methods to `AuthCred`
   - Added `AUTH_SYS_MAX_MACHINENAME_LEN = 255` check in `decode_xdr()`
   - Added 7 new tests for squashing and machinename length
