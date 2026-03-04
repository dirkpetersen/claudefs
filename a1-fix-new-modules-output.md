[0m> build · accounts/fireworks/models/glm-5
[0m

[0m→ [0mRead crates/claudefs-storage/src/background_scheduler.rs
[0m→ [0mRead crates/claudefs-storage/src/device_health_monitor.rs
[0m→ [0mRead crates/claudefs-storage/src/prefetch_engine.rs
[0m[0m← [0mEdit crates/claudefs-storage/src/background_scheduler.rs


Index: /home/cfs/claudefs/crates/claudefs-storage/src/background_scheduler.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-storage/src/background_scheduler.rs
+++ /home/cfs/claudefs/crates/claudefs-storage/src/background_scheduler.rs
@@ -165,9 +165,9 @@
 }
 
 impl Ord for ScheduledTask {
     fn cmp(&self, other: &Self) -> std::cmp::Ordering {
-        let priority_cmp = self.task.priority.cmp(&other.task.priority);
+        let priority_cmp = other.task.priority.cmp(&self.task.priority);
         if priority_cmp != std::cmp::Ordering::Equal {
             return priority_cmp;
         }
 
@@ -184,9 +184,9 @@
         if type_order != std::cmp::Ordering::Equal {
             return type_order;
         }
 
-        self.inserted_at.cmp(&other.inserted_at)
+        other.inserted_at.cmp(&self.inserted_at)
     }
 }
 
 /// Unified background task scheduler.
[0m
[0m← [0mEdit crates/claudefs-storage/src/background_scheduler.rs

[0mIndex: /home/cfs/claudefs/crates/claudefs-storage/src/background_scheduler.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-storage/src/background_scheduler.rs
+++ /home/cfs/claudefs/crates/claudefs-storage/src/background_scheduler.rs
@@ -371,18 +371,17 @@
     let mut scheduler = BackgroundScheduler::new();
     scheduler.set_io_budget(1000, 100);
 
     let task1 = create_test_task(BackgroundTaskType::Scrub);
-    scheduler.schedule(task1);
-
     let task2 = create_test_task(BackgroundTaskType::Scrub);
+    scheduler.schedule(task1);
     scheduler.schedule(task2);
 
-    scheduler.next_runnable();
-    scheduler.complete_task(BackgroundTaskId(1), 100);
+    let started = scheduler.next_runnable().unwrap();
+    scheduler.complete_task(started.id, 100);
 
     let next = scheduler.next_runnable();
-    assert!(next.is_none(), "Budget exhausted");
+    assert!(next.is_none(), "Budget exhausted after 100 bytes");
 }
 
 #[test]
 fn test_budget_reset() {

[0m← [0mEdit crates/claudefs-storage/src/background_scheduler.rs
Index: /home/cfs/claudefs/crates/claudefs-storage/src/background_scheduler.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-storage/src/background_scheduler.rs
+++ /home/cfs/claudefs/crates/claudefs-storage/src/background_scheduler.rs
@@ -387,11 +387,17 @@
 fn test_budget_reset() {
     let mut scheduler = BackgroundScheduler::new();
     scheduler.set_io_budget(1000, 100);
 
-    scheduler.next_runnable();
-    scheduler.complete_task(BackgroundTaskId(1), 100);
+    let task1 = create_test_task(BackgroundTaskType::Scrub);
+    let task2 = create_test_task(BackgroundTaskType::Scrub);
+    scheduler.schedule(task1);
+    scheduler.schedule(task2);
 
+    let started = scheduler.next_runnable().unwrap();
+    scheduler.complete_task(started.id, 100);
+    assert!(scheduler.next_runnable().is_none());
+
     scheduler.advance_window(1);
 
     let next = scheduler.next_runnable();
     assert!(next.is_some());
[0m

[0m← [0mEdit crates/claudefs-storage/src/background_scheduler.rsIndex: /home/cfs/claudefs/crates/claudefs-storage/src/background_scheduler.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-storage/src/background_scheduler.rs
+++ /home/cfs/claudefs/crates/claudefs-storage/src/background_scheduler.rs
@@ -433,10 +433,10 @@
     let mut scheduler = BackgroundScheduler::new();
     let task = create_test_task(BackgroundTaskType::Scrub);
     scheduler.schedule(task);
 
-    scheduler.next_runnable();
-    scheduler.complete_task(BackgroundTaskId(1), 1000);
+    let started = scheduler.next_runnable().unwrap();
+    scheduler.complete_task(started.id, 1000);
 
     let stats = scheduler.stats();
     assert_eq!(stats.pending_count, 0);
 }

[0m

[0m← [0mEdit crates/claudefs-storage/src/background_scheduler.rs
Index: /home/cfs/claudefs/crates/claudefs-storage/src/background_scheduler.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-storage/src/background_scheduler.rs
+++ /home/cfs/claudefs/crates/claudefs-storage/src/background_scheduler.rs
@@ -445,11 +445,11 @@
 fn test_stats_tracking() {
     let mut scheduler = BackgroundScheduler::new();
 
     let task = create_test_task(BackgroundTaskType::Scrub);
-    scheduler.schedule(task);
-    scheduler.next_runnable();
-    scheduler.complete_task(BackgroundTaskId(1), 500);
+    let _id = scheduler.schedule(task);
+    let started = scheduler.next_runnable().unwrap();
+    scheduler.complete_task(started.id, 500);
 
     let stats = scheduler.stats();
     assert_eq!(stats.tasks_scheduled, 1);
     assert_eq!(stats.tasks_completed, 1);
[0m

I'll fix all three files. Let me apply the fixes now.
Index: /home/cfs/claudefs/crates/claudefs-storage/src/background_scheduler.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-storage/src/background_scheduler.rs
+++ /home/cfs/claudefs/crates/claudefs-storage/src/background_scheduler.rs
@@ -565,12 +565,12 @@
 #[test]
 fn test_estimated_bytes_io_tracked() {
     let mut scheduler = BackgroundScheduler::new();
     let task = BackgroundTask::new(BackgroundTaskType::Scrub, 50000, "test".to_string());
-    scheduler.schedule(task);
+    let _id = scheduler.schedule(task);
 
-    scheduler.next_runnable();
-    scheduler.complete_task(BackgroundTaskId(1), 50000);
+    let started = scheduler.next_runnable().unwrap();
+    scheduler.complete_task(started.id, 50000);
 
     let stats = scheduler.stats();
     assert_eq!(stats.total_bytes_io, 50000);
 }



[0m← [0mEdit crates/claudefs-storage/src/background_scheduler.rs[0m[0m← [0mEdit crates/claudefs-storage/src/device_health_monitor.rs
[0m

Index: /home/cfs/claudefs/crates/claudefs-storage/src/device_health_monitor.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-storage/src/device_health_monitor.rs
+++ /home/cfs/claudefs/crates/claudefs-storage/src/device_health_monitor.rs
@@ -134,13 +134,34 @@
     }
 }
 
 fn compute_health_score(&self) -> f64 {
-    let wear_score = self.compute_wear_score();
-    let capacity_score = self.compute_capacity_score();
-    let smart_score = self.compute_smart_score();
+    let mut total_weight = 0.0f64;
+    let mut weighted_score = 0.0f64;
 
-    (wear_score * 0.35) + (capacity_score * 0.35) + (smart_score * 0.30)
+    if self.wear.is_some() {
+        let w = self.compute_wear_score();
+        weighted_score += w * 0.35;
+        total_weight += 0.35;
+    }
+
+    if self.total_bytes > 0 {
+        let c = self.compute_capacity_score();
+        weighted_score += c * 0.35;
+        total_weight += 0.35;
+    }
+
+    if self.smart.is_some() {
+        let s = self.compute_smart_score();
+        weighted_score += s * 0.30;
+        total_weight += 0.30;
+    }
+
+    if total_weight == 0.0 {
+        1.0
+    } else {
+        weighted_score / total_weight
+    }
 }
 
 fn compute_wear_score(&self) -> f64 {
     match &self.wear {
Now let me fix device_health_monitor.rs:

[0m
Index: /home/cfs/claudefs/crates/claudefs-storage/src/device_health_monitor.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-storage/src/device_health_monitor.rs
+++ /home/cfs/claudefs/crates/claudefs-storage/src/device_health_monitor.rs
@@ -358,9 +358,9 @@
 if capacity_pct < 20.0 {
     alerts.push(HealthAlert {
         device_idx: *idx,
         alert_type: HealthAlertType::LowCapacity,
-        severity: if capacity_pct < 10.0 {
+        severity: if capacity_pct < 5.0 {
             AlertSeverity::Critical
         } else {
             AlertSeverity::Warning
         },

[0m← [0mEdit crates/claudefs-storage/src/device_health_monitor.rsIndex: /home/cfs/claudefs/crates/claudefs-storage/src/prefetch_engine.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-storage/src/prefetch_engine.rs
+++ /home/cfs/claudefs/crates/claudefs-storage/src/prefetch_engine.rs
@@ -101,30 +101,34 @@
     self.detect_pattern(config);
 }
 
 fn detect_pattern(&mut self, config: &PrefetchConfig) {
-    if self.history.len() < 2 {
+    let history_len = self.history.len();
+    if history_len < 2 {
         self.is_sequential = false;
         return;
     }
 
+    let check_window = config.sequential_threshold.min(history_len);
+    let start = history_len - check_window;
+
     let mut is_seq = true;
-    for i in 1..self.history.len() {
-        let expected_offset = self.history[i - 1].offset + self.history[i - 1].size as u64;
-        if self.history[i].offset != expected_offset {
+    for i in (start + 1)..history_len {
+        let expected = self.history[i - 1].offset + self.history[i - 1].size;
+        if self.history[i].offset != expected {
             is_seq = false;
             break;
         }
     }
 
-    if is_seq {
-        if !self.is_sequential && self.history.len() >= config.sequential_threshold {
+    if is_seq && history_len >= config.sequential_threshold {
+        if !self.is_sequential {
             self.confidence = (self.confidence + 0.3).min(1.0);
         }
         self.is_sequential = true;
-    } else {
+    } else if !is_seq {
         if self.is_sequential {
-            self.confidence = (self.confidence - 0.2).max(0.0);
+            self.confidence = (self.confidence - 0.3).max(0.0);
         }
         self.is_sequential = false;
     }
 }
[0m← [0mEdit crates/claudefs-storage/src/prefetch_engine.rs
[0m

Now let me fix prefetch_engine.rs:
[0m← [0mEdit crates/claudefs-storage/src/prefetch_engine.rs
Index: /home/cfs/claudefs/crates/claudefs-storage/src/prefetch_engine.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-storage/src/prefetch_engine.rs
+++ /home/cfs/claudefs/crates/claudefs-storage/src/prefetch_engine.rs
@@ -192,15 +192,18 @@
         .streams
         .entry(stream_id)
         .or_insert_with(StreamState::new);
     let was_sequential = stream.is_sequential;
-
     stream.add_access(block_offset, size, self.access_counter, &self.config);
+    let is_now_sequential = stream.is_sequential;
+    let history_len = stream.history.len();
 
-    if stream.is_sequential && !was_sequential {
+    if is_now_sequential && !was_sequential {
         self.stats.sequential_streams_detected += 1;
-    } else if !stream.is_sequential && was_sequential {
+    } else if !is_now_sequential && was_sequential {
         self.stats.random_streams_detected += 1;
+    } else if !is_now_sequential && !was_sequential && history_len == 2 {
+        self.stats.random_streams_detected += 1;
     }
 }
 
 /// Returns prefetch hints for a stream.

[0m
[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-storage 2>&1 | tail -20test zns::tests::test_zone_manager_creation ... ok
test zns::tests::test_zone_state_transitions ... ok
test device::tests::test_fdp_and_zns_flags ... ok
test integrity_chain::tests::test_integrity_manager_gc_expired_chains ... ok

failures:

---- prefetch_engine::tests::test_prefetch_hints_max_offset_safety stdout ----

thread 'prefetch_engine::tests::test_prefetch_hints_max_offset_safety' (1207233) panicked at crates/claudefs-storage/src/prefetch_engine.rs:146:30:
attempt to add with overflow
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    prefetch_engine::tests::test_prefetch_hints_max_offset_safety

test result: FAILED. 780 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.11s

error: test failed, to rerun pass `-p claudefs-storage --lib`[0m


There's an overflow in the prefetch hint calculation. Let me fix it:
[0m→ [0mRead crates/claudefs-storage/src/prefetch_engine.rs[90m [offset=130, limit=30][0m
[0m
[0m← [0mEdit crates/claudefs-storage/src/prefetch_engine.rsIndex: /home/cfs/claudefs/crates/claudefs-storage/src/prefetch_engine.rs
===================================================================
--- /home/cfs/claudefs/crates/claudefs-storage/src/prefetch_engine.rs
+++ /home/cfs/claudefs/crates/claudefs-storage/src/prefetch_engine.rs
@@ -138,13 +138,14 @@
     return Vec::new();
 }
 
 let last = self.history.last().unwrap();
-let next_offset = last.offset + last.size as u64;
+let next_offset = last.offset.saturating_add(last.size);
 
 (0..config.lookahead_blocks)
     .map(|i| {
-        let offset = next_offset + (i as u64 * last.size as u64);
+        let offset =
+            next_offset.saturating_add(i.saturating_mul(last.size as usize) as u64);
         PrefetchHint {
             offset: offset.min(u64::MAX - last.size as u64),
             size: last.size,
         }

[0m

[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-storage 2>&1 | tail -5

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s[0m


test zns::tests::test_zone_descriptor ... ok
test zns::tests::test_zone_full ... ok
test zns::tests::test_zone_manager_creation ... ok
test zns::tests::test_zone_state_transitions ... ok
test write_journal::tests::test_truncate_with_no_matching_entries ... ok
test device::tests::test_fdp_and_zns_flags ... ok
test integrity_chain::tests::test_integrity_manager_gc_expired_chains ... ok

test result: ok. 781 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.11s[0m
[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-storage --lib 2>&1 | tail -10**All 781 tests pass** (759 previously passing + 22 fixed).
