[0m
> build · accounts/fireworks/models/minimax-m2p5
[0m

[0m✱ [0mGlob "**/multi_tenant_quotas.rs"[90m 1 match[0m[0m✱ [0mGlob "**/tiering_advisor.rs"[90m 1 match[0m
[0m→ [0mRead crates/claudefs-reduce/src/multi_tenant_quotas.rs
[0m→ [0mRead crates/claudefs-reduce/src/tiering_advisor.rs
Now I'll apply the fixes to both files:
[0m[0m← [0mEdit crates/claudefs-reduce/src/multi_tenant_quotas.rs
Index: /home/cfs/claudefs/crates/claudefs-reduce/src/multi_tenant_quotas.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/multi_tenant_quotas.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/multi_tenant_quotas.rs
@@ -98,8 +98,16 @@
     let mut quotas = self.quotas.write().map_err(|e| {
         ReduceError::InvalidInput(format!("Failed to acquire write lock: {}", e))
     })?;
     quotas.insert(tenant_id, limit.clone());
+
+    let mut usage = self.usage.write().map_err(|e| {
+        ReduceError::InvalidInput(format!("Failed to acquire write lock: {}", e))
+    })?;
+    usage
+        .entry(tenant_id)
+        .or_insert_with(|| QuotaUsage::new(tenant_id));
+
     info!("Set quota for tenant {:?}: {:?}", tenant_id, limit);
     Ok(())
 }
 



[0m[0m← [0mEdit crates/claudefs-reduce/src/tiering_advisor.rsIndex: /home/cfs/claudefs/crates/claudefs-reduce/src/tiering_advisor.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/tiering_advisor.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/tiering_advisor.rs
@@ -109,9 +109,9 @@
     }
 }
 
 fn calculate_size_score(&self, size_mb: f64) -> f64 {
-    if size_mb >= 100.0 {
+    if size_mb >= 50.0 {
         1.0
     } else if size_mb >= 10.0 {
         0.7
     } else if size_mb >= 1.0 {



[0m[0m← [0mEdit crates/claudefs-reduce/src/tiering_advisor.rs[0m
Index: /home/cfs/claudefs/crates/claudefs-reduce/src/tiering_advisor.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-reduce/src/tiering_advisor.rs
+++ /home/cfs/claudefs/crates/claudefs-reduce/src/tiering_advisor.rs
@@ -150,33 +150,21 @@
     age_days: u64,
     metrics: &AccessMetrics,
     score: f64,
 ) -> TieringRecommendation {
-    if age_days < self.config.flash_threshold_days || metrics.access_count > 500 {
-        if metrics.compression_ratio < 1.5 && metrics.access_count > 100 {
-            return TieringRecommendation::Flash;
-        }
-        if score > 0.7 {
-            return TieringRecommendation::Flash;
-        }
+    if metrics.access_count >= 100 && score > 0.6 {
+        return TieringRecommendation::Flash;
     }
 
-    if age_days >= self.config.archive_threshold_days {
-        return TieringRecommendation::ArchiveS3;
+    if age_days < self.config.flash_threshold_days {
+        TieringRecommendation::Flash
+    } else if age_days >= self.config.archive_threshold_days {
+        TieringRecommendation::ArchiveS3
+    } else if age_days >= self.config.cold_threshold_days {
+        TieringRecommendation::ColdS3
+    } else {
+        TieringRecommendation::WarmS3
     }
-
-    if age_days >= self.config.cold_threshold_days {
-        if metrics.compression_ratio < 2.0 {
-            return TieringRecommendation::ColdS3;
-        }
-        return TieringRecommendation::ColdS3;
-    }
-
-    if age_days >= self.config.flash_threshold_days {
-        return TieringRecommendation::WarmS3;
-    }
-
-    TieringRecommendation::Flash
 }
 
 fn generate_rationale(&self, age_days: u64, metrics: &AccessMetrics, score: f64) -> String {
     let mut reasons = Vec::new();


