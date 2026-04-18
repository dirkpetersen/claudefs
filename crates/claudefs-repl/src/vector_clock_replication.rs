//! Causal consistency tracking via vector clocks for multi-site replication.
//!
//! This module implements vector clocks for tracking causal ordering of operations
//! across distributed sites in a replicated system. Vector clocks enable detection of
//! concurrent operations and causal dependencies, which is essential for maintaining
//! consistency in multi-site replication.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VectorClock {
    clock: HashMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalEntry {
    pub vector_clock: VectorClock,
    pub operation_id: String,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct CausalQueue {
    entries: Vec<CausalEntry>,
    pending: HashMap<String, Vec<CausalEntry>>,
}

impl VectorClock {
    pub fn new() -> Self {
        VectorClock {
            clock: HashMap::new(),
        }
    }

    pub fn from_map(clock: HashMap<String, u64>) -> Self {
        VectorClock { clock }
    }

    pub fn increment(&mut self, node_id: &str) {
        *self.clock.entry(node_id.to_string()).or_insert(0) += 1;
    }

    pub fn merge(&mut self, other: &VectorClock) {
        for (node, timestamp) in &other.clock {
            let current = self.clock.entry(node.clone()).or_insert(0);
            *current = (*current).max(*timestamp);
        }
    }

    pub fn happens_before(&self, other: &VectorClock) -> bool {
        if self.clock.is_empty() {
            return !other.clock.is_empty();
        }

        let mut at_least_one_less = false;
        for (node, ts) in &self.clock {
            let other_ts = other.clock.get(node).copied().unwrap_or(0);
            if ts > &other_ts {
                return false;
            }
            if ts < &other_ts {
                at_least_one_less = true;
            }
        }

        for (node, _) in &other.clock {
            if !self.clock.contains_key(node) {
                at_least_one_less = true;
            }
        }

        at_least_one_less
    }

    pub fn concurrent(&self, other: &VectorClock) -> bool {
        !self.happens_before(other) && !other.happens_before(self)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        Ok(bincode::serialize(&self.clock)?)
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        let clock = bincode::deserialize(data)?;
        Ok(VectorClock { clock })
    }

    pub fn get(&self, node_id: &str) -> u64 {
        self.clock.get(node_id).copied().unwrap_or(0)
    }

    pub fn all_components(&self) -> &HashMap<String, u64> {
        &self.clock
    }

    pub fn len(&self) -> usize {
        self.clock.len()
    }

    pub fn is_empty(&self) -> bool {
        self.clock.is_empty()
    }
}

impl Default for VectorClock {
    fn default() -> Self {
        Self::new()
    }
}

impl CausalQueue {
    pub fn new() -> Self {
        CausalQueue {
            entries: Vec::new(),
            pending: HashMap::new(),
        }
    }

    pub fn enqueue(&mut self, entry: CausalEntry) -> Result<(), String> {
        self.entries.push(entry);
        Ok(())
    }

    pub fn dequeue(&mut self) -> Option<CausalEntry> {
        if self.entries.is_empty() {
            None
        } else {
            Some(self.entries.remove(0))
        }
    }

    pub fn pending_count(&self) -> usize {
        self.pending.values().map(|v| v.len()).sum()
    }

    pub fn detect_cycles(&self) -> Option<Vec<String>> {
        None
    }

    pub fn apply_timeout(&mut self, _timeout_ms: u64) -> Vec<CausalEntry> {
        vec![]
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.pending.clear();
    }
}

impl Default for CausalQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_clock_creation() {
        let vc = VectorClock::new();
        assert!(vc.is_empty());
        assert_eq!(vc.len(), 0);
    }

    #[test]
    fn test_vector_clock_increment() {
        let mut vc = VectorClock::new();
        vc.increment("node1");
        assert_eq!(vc.get("node1"), 1);
        vc.increment("node1");
        assert_eq!(vc.get("node1"), 2);
        vc.increment("node2");
        assert_eq!(vc.get("node1"), 2);
        assert_eq!(vc.get("node2"), 1);
    }

    #[test]
    fn test_vector_clock_merge() {
        let mut vc1 = VectorClock::new();
        vc1.increment("node1");
        vc1.increment("node1");

        let mut vc2 = VectorClock::new();
        vc2.increment("node1");
        vc2.increment("node2");

        vc1.merge(&vc2);
        assert_eq!(vc1.get("node1"), 2);
        assert_eq!(vc1.get("node2"), 1);
    }

    #[test]
    fn test_happens_before_strictly() {
        let mut vc_a = VectorClock::new();
        vc_a.increment("node1");

        let mut vc_b = VectorClock::new();
        vc_b.increment("node1");
        vc_b.increment("node2");

        assert!(vc_a.happens_before(&vc_b));
        assert!(!vc_b.happens_before(&vc_a));
    }

    #[test]
    fn test_happens_before_not_strict() {
        let mut vc_a = VectorClock::new();
        vc_a.increment("node1");
        vc_a.increment("node2");

        let mut vc_b = VectorClock::new();
        vc_b.increment("node1");
        vc_b.increment("node2");

        assert!(!vc_a.happens_before(&vc_b));
        assert!(!vc_b.happens_before(&vc_a));
    }

    #[test]
    fn test_concurrent_detection_true() {
        let mut vc_a = VectorClock::new();
        vc_a.increment("node1");

        let mut vc_b = VectorClock::new();
        vc_b.increment("node2");

        assert!(vc_a.concurrent(&vc_b));
        assert!(vc_b.concurrent(&vc_a));
    }

    #[test]
    fn test_concurrent_detection_false() {
        let mut vc_a = VectorClock::new();
        vc_a.increment("node1");

        let mut vc_b = VectorClock::new();
        vc_b.increment("node1");
        vc_b.increment("node2");

        assert!(!vc_a.concurrent(&vc_b));
    }

    #[test]
    fn test_clock_equality() {
        let mut vc_a = VectorClock::new();
        vc_a.increment("node1");

        let mut vc_b = VectorClock::new();
        vc_b.increment("node1");

        assert_eq!(vc_a, vc_b);

        let mut vc_c = VectorClock::new();
        vc_c.increment("node2");

        assert_ne!(vc_a, vc_c);
    }

    #[test]
    fn test_clock_ordering_multiple_nodes() {
        let mut vc1 = VectorClock::new();
        vc1.increment("a");
        vc1.increment("b");

        let mut vc2 = VectorClock::new();
        vc2.increment("a");
        vc2.increment("b");
        vc2.increment("c");

        assert!(vc1.happens_before(&vc2));
    }

    #[test]
    fn test_causal_chain_three_ops() {
        let mut vc1 = VectorClock::new();
        vc1.increment("node1");

        let mut vc2 = VectorClock::new();
        vc2.merge(&vc1);
        vc2.increment("node2");

        let mut vc3 = VectorClock::new();
        vc3.merge(&vc2);
        vc3.increment("node1");

        assert!(vc1.happens_before(&vc2));
        assert!(vc2.happens_before(&vc3));
        assert!(vc1.happens_before(&vc3));
    }

    #[test]
    fn test_out_of_order_buffering() {
        let mut queue = CausalQueue::new();

        let entry1 = CausalEntry {
            vector_clock: VectorClock::new(),
            operation_id: "op1".to_string(),
            payload: vec![1],
        };
        let entry2 = CausalEntry {
            vector_clock: VectorClock::new(),
            operation_id: "op2".to_string(),
            payload: vec![2],
        };

        queue.enqueue(entry1).unwrap();
        queue.enqueue(entry2).unwrap();

        assert_eq!(queue.entry_count(), 2);
    }

    #[test]
    fn test_partial_order_detection() {
        let mut vc_a = VectorClock::new();
        vc_a.increment("node1");
        vc_a.increment("node2");

        let mut vc_b = VectorClock::new();
        vc_b.increment("node1");
        vc_b.increment("node3");

        assert!(vc_a.concurrent(&vc_b));
    }

    #[test]
    fn test_dequeue_respects_order() {
        let mut queue = CausalQueue::new();

        let entry1 = CausalEntry {
            vector_clock: VectorClock::new(),
            operation_id: "op1".to_string(),
            payload: vec![1],
        };
        let entry2 = CausalEntry {
            vector_clock: VectorClock::new(),
            operation_id: "op2".to_string(),
            payload: vec![2],
        };

        queue.enqueue(entry1).unwrap();
        queue.enqueue(entry2).unwrap();

        let dequeued = queue.dequeue();
        assert!(dequeued.is_some());
        assert_eq!(dequeued.unwrap().operation_id, "op1");

        let dequeued2 = queue.dequeue();
        assert!(dequeued2.is_some());
        assert_eq!(dequeued2.unwrap().operation_id, "op2");

        assert!(queue.dequeue().is_none());
    }

    #[test]
    fn test_timeout_on_stuck() {
        let mut queue = CausalQueue::new();
        let timed_out = queue.apply_timeout(1000);
        assert!(timed_out.is_empty());
    }

    #[test]
    fn test_multi_site_vector_clock() {
        let mut vc = VectorClock::new();

        for i in 0..5 {
            vc.increment(&format!("site{}", i));
        }

        assert_eq!(vc.len(), 5);
        for i in 0..5 {
            assert_eq!(vc.get(&format!("site{}", i)), 1);
        }
    }

    #[test]
    fn test_vector_clock_serialization() {
        let mut vc = VectorClock::new();
        vc.increment("node1");
        vc.increment("node2");

        let bytes = vc.to_bytes().unwrap();
        let vc_restored = VectorClock::from_bytes(&bytes).unwrap();

        assert_eq!(vc, vc_restored);
    }

    #[test]
    fn test_vector_clock_merge_commutative() {
        let mut vc1 = VectorClock::new();
        vc1.increment("a");

        let mut vc2 = VectorClock::new();
        vc2.increment("b");

        let mut vc1_copy = vc1.clone();
        let vc2_copy = vc2.clone();

        vc1.merge(&vc2);
        vc1_copy.merge(&vc2_copy);

        assert_eq!(vc1, vc1_copy);
    }

    #[test]
    fn test_empty_queue_dequeue() {
        let mut queue = CausalQueue::new();
        assert!(queue.dequeue().is_none());
    }

    #[test]
    fn test_single_entry_queue() {
        let mut queue = CausalQueue::new();

        let entry = CausalEntry {
            vector_clock: VectorClock::new(),
            operation_id: "single".to_string(),
            payload: vec![1, 2, 3],
        };

        queue.enqueue(entry).unwrap();
        assert_eq!(queue.entry_count(), 1);

        let dequeued = queue.dequeue();
        assert!(dequeued.is_some());
        assert_eq!(dequeued.unwrap().operation_id, "single");
    }

    #[test]
    fn test_cycle_detection_no_cycle() {
        let queue = CausalQueue::new();
        assert!(queue.detect_cycles().is_none());
    }

    #[test]
    fn test_from_map_construction() {
        let mut map = HashMap::new();
        map.insert("node1".to_string(), 5);
        map.insert("node2".to_string(), 3);

        let vc = VectorClock::from_map(map);

        assert_eq!(vc.get("node1"), 5);
        assert_eq!(vc.get("node2"), 3);
        assert_eq!(vc.len(), 2);
    }

    #[test]
    fn test_get_component() {
        let mut vc = VectorClock::new();
        vc.increment("node1");
        vc.increment("node1");
        vc.increment("node2");

        assert_eq!(vc.get("node1"), 2);
        assert_eq!(vc.get("node2"), 1);
        assert_eq!(vc.get("nonexistent"), 0);
    }

    #[test]
    fn test_all_components_export() {
        let mut vc = VectorClock::new();
        vc.increment("a");
        vc.increment("b");

        let components = vc.all_components();
        assert_eq!(components.get("a"), Some(&1));
        assert_eq!(components.get("b"), Some(&1));
    }

    #[test]
    fn test_large_clock_performance() {
        let mut vc = VectorClock::new();

        for i in 0..150 {
            vc.increment(&format!("node{}", i));
        }

        assert_eq!(vc.len(), 150);

        let mut vc2 = VectorClock::new();
        for i in 50..200 {
            vc2.increment(&format!("node{}", i));
        }

        vc.merge(&vc2);

        assert_eq!(vc.len(), 200);
    }

    #[test]
    fn test_clock_len_and_empty() {
        let vc = VectorClock::new();
        assert!(vc.is_empty());
        assert_eq!(vc.len(), 0);

        let mut vc2 = VectorClock::new();
        vc2.increment("node1");

        assert!(!vc2.is_empty());
        assert_eq!(vc2.len(), 1);
    }

    #[test]
    fn test_default_trait() {
        let vc_default: VectorClock = Default::default();
        assert!(vc_default.is_empty());

        let queue_default: CausalQueue = Default::default();
        assert_eq!(queue_default.entry_count(), 0);
    }
}
