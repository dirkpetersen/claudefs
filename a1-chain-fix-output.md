[0m
> build · accounts/fireworks/models/minimax-m2p5
[0m
[0m→ [0mRead crates/claudefs-storage/src/integrity_chain.rs
[0m
[0m← [0mEdit crates/claudefs-storage/src/integrity_chain.rs
Index: /home/cfs/claudefs/crates/claudefs-storage/src/integrity_chain.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-storage/src/integrity_chain.rs
+++ /home/cfs/claudefs/crates/claudefs-storage/src/integrity_chain.rs
@@ -237,9 +237,9 @@
     .map_err(|e| StorageError::AllocatorError(e.to_string()))?
     .as_millis() as u64;
 
 let ttl = ttl_seconds.unwrap_or(self.config.chain_ttl_seconds);
-let expires_at = now + ttl * 1000;
+let expires_at = now + ttl * 60_000;
 
 let chain = IntegrityChain {
     id: Uuid::new_v4().to_string(),
     data_id,

[0m
Done. Changed `ttl * 1000` to `ttl * 60_000` at line 241 in the `create_chain` method, so TTL values now represent minutes instead of seconds.
