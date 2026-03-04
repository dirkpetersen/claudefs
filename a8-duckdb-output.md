[0m[0m> build · accounts/fireworks/models/minimax-m2p5


[0m→ [0mRead crates/claudefs-mgmt/Cargo.toml
[0m→ [0mRead crates/claudefs-mgmt/src/analytics.rs
[0m✱ [0mGlob "crates/claudefs-mgmt/src/error*.rs"[90m 0 matches[0m
[0m
Index: /home/cfs/claudefs/crates/claudefs-mgmt/Cargo.toml
===================================================================
--- /home/cfs/claudefs/crates/claudefs-mgmt/Cargo.toml
+++ /home/cfs/claudefs/crates/claudefs-mgmt/Cargo.toml
@@ -47,7 +47,10 @@
 subtle = "2.5"
 rand.workspace = true
 hex = "0.4"
 
+# Analytics
+duckdb = { version = "1.0", features = ["bundled"] }
+
 [dev-dependencies]
 tokio-test = "0.4"
 tempfile = "3.10"
\ No newline at end of file
[0m← [0mEdit crates/claudefs-mgmt/Cargo.toml

[0m
