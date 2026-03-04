[0m
> build · accounts/fireworks/models/minimax-m2p5
[0m
[0m→ [0mRead crates/claudefs-storage/src/node_rebalance.rs[90m [offset=700, limit=30][0m
[0m
[0m← [0mEdit crates/claudefs-storage/src/node_rebalance.rs
Index: /home/cfs/claudefs/crates/claudefs-storage/src/node_rebalance.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-storage/src/node_rebalance.rs
+++ /home/cfs/claudefs/crates/claudefs-storage/src/node_rebalance.rs
@@ -43,14 +43,10 @@
 
 /// Direction of segment migration.
 #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
 pub enum MigrationDirection {
-    Outbound {
-        target_node: NodeId,
-    },
-    Inbound {
-        source_node: NodeId,
-    },
+    Outbound { target_node: NodeId },
+    Inbound { source_node: NodeId },
 }
 
 /// A single segment migration task.
 #[derive(Debug, Clone, Serialize, Deserialize)]
@@ -70,11 +66,9 @@
     Queued,
     Transferring,
     Verifying,
     Completed,
-    Failed {
-        reason: String,
-    },
+    Failed { reason: String },
 }
 
 /// Configuration for the rebalance engine.
 #[derive(Debug, Clone, Serialize, Deserialize)]
@@ -128,9 +122,12 @@
 }
 
 impl RebalanceEngine {
     pub fn new(config: RebalanceConfig, local_node: NodeId) -> Self {
-        info!("Initializing rebalance engine for node {:?} with config: {:?}", local_node, config);
+        info!(
+            "Initializing rebalance engine for node {:?} with config: {:?}",
+            local_node, config
+        );
         Self {
             config,
             state: RebalanceState::Idle,
             local_node,
@@ -142,9 +139,12 @@
         }
     }
 
     pub fn register_segment(&mut self, segment_id: RebalanceSegmentId, shard_id: ShardId) {
-        debug!("Registering segment {:?} for shard {:?}", segment_id, shard_id);
+        debug!(
+            "Registering segment {:?} for shard {:?}",
+            segment_id, shard_id
+        );
         self.local_segments.insert(segment_id, shard_id);
     }
 
     pub fn remove_segment(&mut self, segment_id: RebalanceSegmentId) -> Option<ShardId> {
@@ -152,15 +152,19 @@
         self.local_segments.remove(&segment_id)
     }
 
     pub fn update_shard_map(&mut self, new_map: HashMap<ShardId, NodeId>) {
-        info!("Updating shard map: {} shards, local node is {:?}", new_map.len(), self.local_node);
+        info!(
+            "Updating shard map: {} shards, local node is {:?}",
+            new_map.len(),
+            self.local_node
+        );
         self.shard_map = new_map;
     }
 
     pub fn plan_rebalance(&mut self) -> Vec<MigrationTask> {
         info!("Planning rebalance from state {:?}", self.state);
-        
+
         let mut tasks = Vec::new();
         let now = std::time::SystemTime::now()
             .duration_since(std::time::UNIX_EPOCH)
             .map(|d| d.as_secs())
@@ -179,9 +183,12 @@
                         state: MigrationTaskState::Queued,
                         created_at: now,
                         completed_at: None,
                     };
-                    debug!("Planned outbound migration: segment {:?} shard {:?} -> {:?}", segment_id, shard_id, owner);
+                    debug!(
+                        "Planned outbound migration: segment {:?} shard {:?} -> {:?}",
+                        segment_id, shard_id, owner
+                    );
                     tasks.push(task);
                 }
             }
         }
@@ -197,12 +204,12 @@
         }
 
         info!("Starting rebalance operation");
         self.state = RebalanceState::Planning;
-        
+
         let tasks = self.plan_rebalance();
         let segments_total = tasks.len() as u64;
-        
+
         if segments_total > 0 {
             self.state = RebalanceState::Migrating {
                 segments_total,
                 segments_done: 0,
@@ -218,23 +225,39 @@
         self.stats.total_rebalances += 1;
         Ok(())
     }
 
-    pub fn advance_migration(&mut self, segment_id: RebalanceSegmentId) -> Result<MigrationTaskState, &'static str> {
-        let task = self.migrations.iter_mut().find(|t| t.segment_id == segment_id).ok_or("Migration task not found")?;
+    pub fn advance_migration(
+        &mut self,
+        segment_id: RebalanceSegmentId,
+    ) -> Result<MigrationTaskState, &'static str> {
+        let task = self
+            .migrations
+            .iter_mut()
+            .find(|t| t.segment_id == segment_id)
+            .ok_or("Migration task not found")?;
 
         let new_state = match task.state {
             MigrationTaskState::Queued => {
-                debug!("Advancing segment {:?} from Queued to Transferring", segment_id);
+                debug!(
+                    "Advancing segment {:?} from Queued to Transferring",
+                    segment_id
+                );
                 MigrationTaskState::Transferring
             }
             MigrationTaskState::Transferring => {
-                debug!("Advancing segment {:?} from Transferring to Verifying", segment_id);
+                debug!(
+                    "Advancing segment {:?} from Transferring to Verifying",
+                    segment_id
+                );
                 MigrationTaskState::Verifying
             }
             MigrationTaskState::Verifying => {
-                debug!("Advancing segment {:?} from Verifying to Completed", segment_id);
-                
+                debug!(
+                    "Advancing segment {:?} from Verifying to Completed",
+                    segment_id
+                );
+
                 match &task.direction {
                     MigrationDirection::Outbound { .. } => {
                         self.stats.segments_migrated_out += 1;
                         self.stats.bytes_migrated_out += task.bytes;
@@ -243,13 +266,20 @@
                         self.stats.segments_migrated_in += 1;
                         self.stats.bytes_migrated_in += task.bytes;
                     }
                 }
-                
-                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
+
+                let now = std::time::SystemTime::now()
+                    .duration_since(std::time::UNIX_EPOCH)
+                    .map(|d| d.as_secs())
+                    .unwrap_or(0);
                 task.completed_at = Some(now);
 
-                if let RebalanceState::Migrating { segments_total: _, segments_done } = &mut self.state {
+                if let RebalanceState::Migrating {
+                    segments_total: _,
+                    segments_done,
+                } = &mut self.state
+                {
                     *segments_done += 1;
                 }
 
                 MigrationTaskState::Completed
@@ -266,64 +296,113 @@
         task.state = new_state.clone();
         Ok(new_state)
     }
 
-    pub fn fail_migration(&mut self, segment_id: RebalanceSegmentId, reason: String) -> Result<(), &'static str> {
-        let task = self.migrations.iter_mut().find(|t| t.segment_id == segment_id).ok_or("Migration task not found")?;
+    pub fn fail_migration(
+        &mut self,
+        segment_id: RebalanceSegmentId,
+        reason: String,
+    ) -> Result<(), &'static str> {
+        let task = self
+            .migrations
+            .iter_mut()
+            .find(|t| t.segment_id == segment_id)
+            .ok_or("Migration task not found")?;
 
         warn!("Migration failed for segment {:?}: {}", segment_id, reason);
         task.state = MigrationTaskState::Failed { reason };
         self.stats.failed_migrations += 1;
         Ok(())
     }
 
     pub fn complete_rebalance(&mut self) -> Result<RebalanceStats, &'static str> {
-        let pending = self.migrations.iter().filter(|t| {
-            matches!(t.state, MigrationTaskState::Queued | MigrationTaskState::Transferring | MigrationTaskState::Verifying)
-        }).count();
+        let pending = self
+            .migrations
+            .iter()
+            .filter(|t| {
+                matches!(
+                    t.state,
+                    MigrationTaskState::Queued
+                        | MigrationTaskState::Transferring
+                        | MigrationTaskState::Verifying
+                )
+            })
+            .count();
 
         if pending > 0 {
-            warn!("Cannot complete rebalance: {} migrations still pending", pending);
+            warn!(
+                "Cannot complete rebalance: {} migrations still pending",
+                pending
+            );
             return Err("Cannot complete: migrations still pending");
         }
 
         let segments_moved = self.migrations.len() as u64;
         let bytes_moved: u64 = self.migrations.iter().map(|t| t.bytes).sum();
 
-        self.state = RebalanceState::Completed { segments_moved, bytes_moved, duration_secs: 0 };
+        self.state = RebalanceState::Completed {
+            segments_moved,
+            bytes_moved,
+            duration_secs: 0,
+        };
 
-        info!("Rebalance completed: {} segments, {} bytes", segments_moved, bytes_moved);
+        info!(
+            "Rebalance completed: {} segments, {} bytes",
+            segments_moved, bytes_moved
+        );
 
-        self.last_rebalance_time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
+        self.last_rebalance_time = std::time::SystemTime::now()
+            .duration_since(std::time::UNIX_EPOCH)
+            .map(|d| d.as_secs())
+            .unwrap_or(0);
 
         Ok(self.stats.clone())
     }
 
     pub fn abort_rebalance(&mut self, reason: String) {
         warn!("Aborting rebalance: {}", reason);
         self.state = RebalanceState::Failed { reason };
-        
+
         for task in &mut self.migrations {
-            if !matches!(task.state, MigrationTaskState::Completed | MigrationTaskState::Failed { .. }) {
-                task.state = MigrationTaskState::Failed { reason: "Rebalance aborted".to_string() };
+            if !matches!(
+                task.state,
+                MigrationTaskState::Completed | MigrationTaskState::Failed { .. }
+            ) {
+                task.state = MigrationTaskState::Failed {
+                    reason: "Rebalance aborted".to_string(),
+                };
             }
         }
     }
 
-    pub fn accept_inbound(&mut self, segment_id: RebalanceSegmentId, shard_id: ShardId, source_node: NodeId, bytes: u64) {
-        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
+    pub fn accept_inbound(
+        &mut self,
+        segment_id: RebalanceSegmentId,
+        shard_id: ShardId,
+        source_node: NodeId,
+        bytes: u64,
+    ) {
+        let now = std::time::SystemTime::now()
+            .duration_since(std::time::UNIX_EPOCH)
+            .map(|d| d.as_secs())
+            .unwrap_or(0);
 
         let task = MigrationTask {
             segment_id,
             shard_id,
-            direction: MigrationDirection::Inbound { source_node: source_node.clone() },
+            direction: MigrationDirection::Inbound {
+                source_node: source_node.clone(),
+            },
             bytes,
             state: MigrationTaskState::Queued,
             created_at: now,
             completed_at: None,
         };
 
-        debug!("Accepted inbound migration: segment {:?} shard {:?} from {:?} ({} bytes)", segment_id, shard_id, &source_node, bytes);
+        debug!(
+            "Accepted inbound migration: segment {:?} shard {:?} from {:?} ({} bytes)",
+            segment_id, shard_id, &source_node, bytes
+        );
 
         self.migrations.push(task);
     }
 
@@ -332,18 +411,27 @@
         active < self.config.max_concurrent_migrations
     }
 
     pub fn active_migration_count(&self) -> u32 {
-        self.migrations.iter().filter(|t| {
-            matches!(t.state, MigrationTaskState::Queued | MigrationTaskState::Transferring | MigrationTaskState::Verifying)
-        }).count() as u32
+        self.migrations
+            .iter()
+            .filter(|t| {
+                matches!(
+                    t.state,
+                    MigrationTaskState::Queued
+                        | MigrationTaskState::Transferring
+                        | MigrationTaskState::Verifying
+                )
+            })
+            .count() as u32
     }
 
     pub fn progress_pct(&self) -> f64 {
         match &self.state {
-            RebalanceState::Migrating { segments_total, segments_done } if *segments_total > 0 => {
-                (*segments_done as f64 / *segments_total as f64) * 100.0
-            }
+            RebalanceState::Migrating {
+                segments_total,
+                segments_done,
+            } if *segments_total > 0 => (*segments_done as f64 / *segments_total as f64) * 100.0,
             RebalanceState::Completed { .. } => 100.0,
             _ => 0.0,
         }
     }
@@ -409,9 +497,12 @@
         engine.register_segment(RebalanceSegmentId(1), ShardId(10));
         engine.register_segment(RebalanceSegmentId(2), ShardId(20));
 
         assert_eq!(engine.local_segments().len(), 2);
-        assert_eq!(engine.remove_segment(RebalanceSegmentId(1)), Some(ShardId(10)));
+        assert_eq!(
+            engine.remove_segment(RebalanceSegmentId(1)),
+            Some(ShardId(10))
+        );
         assert_eq!(engine.local_segments().len(), 1);
         assert_eq!(engine.remove_segment(RebalanceSegmentId(999)), None);
     }
 
@@ -425,9 +516,12 @@
         new_map.insert(ShardId(1), new_node("node2"));
         engine.update_shard_map(new_map);
 
         assert_eq!(engine.shard_map().len(), 2);
-        assert_eq!(engine.shard_map().get(&ShardId(0)), Some(&new_node("node1")));
+        assert_eq!(
+            engine.shard_map().get(&ShardId(0)),
+            Some(&new_node("node1"))
+        );
     }
 
     #[test]
     fn test_plan_rebalance_no_changes() {
@@ -466,9 +560,11 @@
 
         let tasks = engine.plan_rebalance();
         assert_eq!(tasks.len(), 1);
         assert_eq!(tasks[0].segment_id, RebalanceSegmentId(1));
-        assert!(matches!(&tasks[0].direction, MigrationDirection::Outbound { target_node } if target_node == &new_node("node2")));
+        assert!(
+            matches!(&tasks[0].direction, MigrationDirection::Outbound { target_node } if target_node == &new_node("node2"))
+        );
     }
 
     #[test]
     fn test_plan_rebalance_node_removed() {
@@ -564,9 +660,9 @@
         engine.start_rebalance().unwrap();
         engine.advance_migration(RebalanceSegmentId(1)).unwrap();
         engine.advance_migration(RebalanceSegmentId(1)).unwrap();
         let new_state = engine.advance_migration(RebalanceSegmentId(1)).unwrap();
-        
+
         assert!(matches!(new_state, MigrationTaskState::Completed));
         assert_eq!(engine.stats().segments_migrated_out, 1);
     }
 
@@ -589,9 +685,11 @@
         engine.update_shard_map(shard_map);
         engine.register_segment(RebalanceSegmentId(1), ShardId(0));
 
         engine.start_rebalance().unwrap();
-        engine.fail_migration(RebalanceSegmentId(1), "Network error".to_string()).unwrap();
+        engine
+            .fail_migration(RebalanceSegmentId(1), "Network error".to_string())
+            .unwrap();
 
         assert_eq!(engine.stats().failed_migrations, 1);
     }
 
@@ -643,33 +741,48 @@
 
         engine.start_rebalance().unwrap();
         engine.abort_rebalance("Manual abort".to_string());
 
-        assert!(matches!(engine.state(), RebalanceState::Failed { reason } if reason == "Manual abort"));
+        assert!(
+            matches!(engine.state(), RebalanceState::Failed { reason } if reason == "Manual abort")
+        );
     }
 
     #[test]
     fn test_accept_inbound() {
         let config = RebalanceConfig::default();
         let mut engine = RebalanceEngine::new(config, new_node("node1"));
 
-        engine.accept_inbound(RebalanceSegmentId(100), ShardId(5), new_node("node2"), 2 * 1024 * 1024);
+        engine.accept_inbound(
+            RebalanceSegmentId(100),
+            ShardId(5),
+            new_node("node2"),
+            2 * 1024 * 1024,
+        );
 
         assert_eq!(engine.migrations().len(), 1);
-        assert!(matches!(&engine.migrations()[0].direction, MigrationDirection::Inbound { source_node } if source_node == &new_node("node2")));
+        assert!(
+            matches!(&engine.migrations()[0].direction, MigrationDirection::Inbound { source_node } if source_node == &new_node("node2"))
+        );
     }
 
     #[test]
     fn test_can_accept_more_under_limit() {
-        let config = RebalanceConfig { max_concurrent_migrations: 4, ..Default::default() };
+        let config = RebalanceConfig {
+            max_concurrent_migrations: 4,
+            ..Default::default()
+        };
         let engine = RebalanceEngine::new(config, new_node("node1"));
 
         assert!(engine.can_accept_more());
     }
 
     #[test]
     fn test_can_accept_more_at_limit() {
-        let config = RebalanceConfig { max_concurrent_migrations: 2, ..Default::default() };
+        let config = RebalanceConfig {
+            max_concurrent_migrations: 2,
+            ..Default::default()
+        };
         let mut engine = RebalanceEngine::new(config, new_node("node1"));
 
         engine.accept_inbound(RebalanceSegmentId(1), ShardId(0), new_node("node2"), 1024);
         engine.accept_inbound(RebalanceSegmentId(2), ShardId(1), new_node("node3"), 1024);
@@ -697,10 +810,14 @@
         engine.register_segment(RebalanceSegmentId(1), ShardId(0));
         engine.register_segment(RebalanceSegmentId(2), ShardId(1));
 
         engine.start_rebalance().unwrap();
-        
-        if let RebalanceState::Migrating { segments_total: _, segments_done } = &mut engine.state {
+
+        if let RebalanceState::Migrating {
+            segments_total: _,
+            segments_done,
+        } = &mut engine.state
+        {
             *segments_done = 1;
         }
 
         assert_eq!(engine.progress_pct(), 50.0);
@@ -716,30 +833,39 @@
         engine.update_shard_map(shard_map);
         engine.register_segment(RebalanceSegmentId(1), ShardId(0));
 
         engine.start_rebalance().unwrap();
+        engine.advance_migration(RebalanceSegmentId(1)).unwrap();
+        engine.advance_migration(RebalanceSegmentId(1)).unwrap();
+        engine.advance_migration(RebalanceSegmentId(1)).unwrap();
         engine.complete_rebalance().unwrap();
 
         assert_eq!(engine.progress_pct(), 100.0);
     }
 
     #[test]
     fn test_cooldown_active() {
-        let config = RebalanceConfig { cooldown_secs: 300, ..Default::default() };
+        let config = RebalanceConfig {
+            cooldown_secs: 300,
+            ..Default::default()
+        };
         let mut engine = RebalanceEngine::new(config, new_node("node1"));
 
         engine.last_rebalance_time = 1000;
-        
+
         assert!(engine.is_cooldown_active(1100));
     }
 
     #[test]
     fn test_cooldown_expired() {
-        let config = RebalanceConfig { cooldown_secs: 300, ..Default::default() };
+        let config = RebalanceConfig {
+            cooldown_secs: 300,
+            ..Default::default()
+        };
         let mut engine = RebalanceEngine::new(config, new_node("node1"));
 
         engine.last_rebalance_time = 1000;
-        
+
         assert!(!engine.is_cooldown_active(1500));
     }
 
     #[test]
@@ -764,9 +890,9 @@
 
     #[test]
     fn test_rebalance_config_default() {
         let config = RebalanceConfig::default();
-        
+
         assert_eq!(config.max_concurrent_migrations, 4);
         assert_eq!(config.max_bandwidth_bytes_per_sec, 100 * 1024 * 1024);
         assert_eq!(config.max_iops, 1000);
         assert_eq!(config.cooldown_secs, 300);
@@ -787,9 +913,9 @@
         engine.update_shard_map(shard_map);
 
         let tasks = engine.plan_rebalance();
         assert_eq!(tasks.len(), 3);
-        
+
         for task in &tasks {
             assert_eq!(task.shard_id, ShardId(5));
         }
     }

[0m
[0m→ [0mRead crates/claudefs-storage/src/node_rebalance.rs[90m [offset=709, limit=20][0m
[0m✱ [0mGrep "test_progress_pct_all_done"[90m in crates/claudefs-storage/src/node_rebalance.rs · 1 match[0m
[0m→ [0mRead crates/claudefs-storage/src/node_rebalance.rs[90m [limit=25, offset=820][0m
Fixed. The test at line 827 now has the three `advance_migration` calls (lines 837-839) between `start_rebalance()` and `complete_rebalance()`.
