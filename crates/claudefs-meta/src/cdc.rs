//! Change Data Capture (CDC) event streaming.
//!
//! Provides a persistent, cursor-based event stream for external consumers
//! (webhooks, analytics, etc.). Each metadata operation is published as a
//! CDC event with a monotonic sequence number. Consumers track their own
//! cursor position independently.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use crate::types::*;

/// A CDC event representing a metadata operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CdcEvent {
    /// Monotonically increasing sequence number.
    pub sequence: u64,
    /// Timestamp when the event occurred.
    pub timestamp: Timestamp,
    /// The metadata operation that occurred.
    pub op: MetaOp,
    /// Site identifier for cross-site tracking.
    pub site_id: u64,
}

impl CdcEvent {
    /// Creates a new CDC event.
    pub fn new(sequence: u64, op: MetaOp, site_id: u64) -> Self {
        Self {
            sequence,
            timestamp: Timestamp::now(),
            op,
            site_id,
        }
    }
}

/// Cursor position for a CDC consumer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CdcCursor {
    /// Unique identifier for the consumer.
    pub consumer_id: String,
    /// Last processed sequence number.
    pub last_sequence: u64,
}

impl CdcCursor {
    /// Creates a new cursor.
    pub fn new(consumer_id: String) -> Self {
        Self {
            consumer_id,
            last_sequence: 0,
        }
    }

    /// Creates a cursor starting at a specific sequence.
    pub fn with_sequence(consumer_id: String, sequence: u64) -> Self {
        Self {
            consumer_id,
            last_sequence: sequence,
        }
    }
}

/// CDC event stream with consumer management.
pub struct CdcStream {
    /// Published events in sequence order.
    events: RwLock<Vec<CdcEvent>>,
    /// Consumer cursors indexed by consumer ID.
    cursors: RwLock<HashMap<String, CdcCursor>>,
    /// Next sequence number to assign.
    next_sequence: AtomicU64,
    /// Maximum number of events to retain.
    max_events: usize,
}

impl CdcStream {
    /// Creates a new CDC stream with the specified capacity.
    ///
    /// # Arguments
    /// * `max_events` - Maximum number of events to retain in the buffer
    pub fn new(max_events: usize) -> Self {
        Self {
            events: RwLock::new(Vec::with_capacity(max_events)),
            cursors: RwLock::new(HashMap::new()),
            next_sequence: AtomicU64::new(1),
            max_events,
        }
    }

    /// Publishes a new event to the stream.
    ///
    /// # Arguments
    /// * `op` - The metadata operation
    /// * `site_id` - Site identifier
    ///
    /// # Returns
    /// The sequence number assigned to the event
    pub fn publish(&self, op: MetaOp, site_id: u64) -> u64 {
        let sequence = self.next_sequence.fetch_add(1, Ordering::Relaxed);
        let event = CdcEvent::new(sequence, op, site_id);

        let mut events = self.events.write().expect("lock poisoned");
        events.push(event);

        // Trim old events if over capacity
        if events.len() > self.max_events {
            let remove_count = events.len() - self.max_events;
            events.drain(0..remove_count);
        }

        tracing::debug!("Published CDC event: seq={}", sequence);
        sequence
    }

    /// Registers a new consumer.
    ///
    /// # Arguments
    /// * `consumer_id` - Unique identifier for the consumer
    ///
    /// # Returns
    /// The initial cursor (starting at sequence 0)
    pub fn register_consumer(&self, consumer_id: String) -> CdcCursor {
        let cursor = CdcCursor::new(consumer_id.clone());
        let mut cursors = self.cursors.write().expect("lock poisoned");
        cursors.insert(consumer_id.clone(), cursor.clone());
        tracing::info!("Registered CDC consumer: {}", consumer_id);
        cursor
    }

    /// Unregisters a consumer.
    ///
    /// # Arguments
    /// * `consumer_id` - The consumer to remove
    ///
    /// # Returns
    /// true if consumer was removed
    pub fn unregister_consumer(&self, consumer_id: &str) -> bool {
        let mut cursors = self.cursors.write().expect("lock poisoned");
        let removed = cursors.remove(consumer_id).is_some();
        if removed {
            tracing::info!("Unregistered CDC consumer: {}", consumer_id);
        }
        removed
    }

    /// Consumes events from a consumer's cursor position.
    ///
    /// # Arguments
    /// * `consumer_id` - The consumer ID
    /// * `max_count` - Maximum number of events to return
    ///
    /// # Returns
    /// Vector of events starting after the cursor, empty if at end
    pub fn consume(&self, consumer_id: &str, max_count: usize) -> Vec<CdcEvent> {
        let mut cursors = self.cursors.write().expect("lock poisoned");
        let cursor = match cursors.get_mut(consumer_id) {
            Some(c) => c,
            None => return Vec::new(),
        };

        let events = self.events.read().expect("lock poisoned");
        let start_idx = match events.binary_search_by(|e| e.sequence.cmp(&cursor.last_sequence)) {
            Ok(idx) => idx + 1,
            Err(idx) => idx,
        };

        let end_idx = (start_idx + max_count).min(events.len());
        let result: Vec<CdcEvent> = events[start_idx..end_idx].to_vec();

        // Update cursor to last consumed
        if let Some(last) = result.last() {
            cursor.last_sequence = last.sequence;
        }

        result
    }

    /// Peeks at events without advancing the cursor.
    ///
    /// # Arguments
    /// * `consumer_id` - The consumer ID
    /// * `max_count` - Maximum number of events to return
    ///
    /// # Returns
    /// Vector of events after the cursor, empty if at end
    pub fn peek(&self, consumer_id: &str, max_count: usize) -> Vec<CdcEvent> {
        let cursors = self.cursors.read().expect("lock poisoned");
        let cursor = match cursors.get(consumer_id) {
            Some(c) => c,
            None => return Vec::new(),
        };

        let events = self.events.read().expect("lock poisoned");
        let start_idx = match events.binary_search_by(|e| e.sequence.cmp(&cursor.last_sequence)) {
            Ok(idx) => idx + 1,
            Err(idx) => idx,
        };

        let end_idx = (start_idx + max_count).min(events.len());
        events[start_idx..end_idx].to_vec()
    }

    /// Seeks a consumer's cursor to a specific sequence.
    ///
    /// # Arguments
    /// * `consumer_id` - The consumer ID
    /// * `sequence` - The sequence number to seek to
    ///
    /// # Returns
    /// true if seek was successful
    pub fn seek(&self, consumer_id: &str, sequence: u64) -> bool {
        let mut cursors = self.cursors.write().expect("lock poisoned");
        if let Some(cursor) = cursors.get_mut(consumer_id) {
            cursor.last_sequence = sequence;
            tracing::debug!("Consumer {} seeked to sequence {}", consumer_id, sequence);
            true
        } else {
            false
        }
    }

    /// Returns the lag (number of events behind head) for a consumer.
    ///
    /// # Arguments
    /// * `consumer_id` - The consumer ID
    ///
    /// # Returns
    /// Number of events behind the latest event, None if consumer not found
    pub fn lag(&self, consumer_id: &str) -> Option<u64> {
        let cursors = self.cursors.read().expect("lock poisoned");
        let cursor = cursors.get(consumer_id)?;

        let head = self.next_sequence.load(Ordering::Relaxed) - 1;
        Some(head.saturating_sub(cursor.last_sequence))
    }

    /// Returns the number of registered consumers.
    pub fn consumer_count(&self) -> usize {
        self.cursors.read().expect("lock poisoned").len()
    }

    /// Returns the total number of events in the stream.
    pub fn total_events(&self) -> usize {
        self.events.read().expect("lock poisoned").len()
    }

    /// Returns the oldest sequence number in the stream (or 0 if empty).
    pub fn oldest_sequence(&self) -> u64 {
        let events = self.events.read().expect("lock poisoned");
        events.first().map(|e| e.sequence).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_op(ino: InodeId) -> MetaOp {
        MetaOp::CreateInode {
            attr: InodeAttr::new_file(ino, 0, 0, 0o644, 1),
        }
    }

    #[test]
    fn test_publish() {
        let stream = CdcStream::new(100);
        let seq = stream.publish(create_test_op(InodeId::new(1)), 1);
        assert_eq!(seq, 1);
        assert_eq!(stream.total_events(), 1);
    }

    #[test]
    fn test_publish_multiple() {
        let stream = CdcStream::new(100);
        stream.publish(create_test_op(InodeId::new(1)), 1);
        stream.publish(create_test_op(InodeId::new(2)), 1);
        stream.publish(create_test_op(InodeId::new(3)), 1);

        assert_eq!(stream.total_events(), 3);
        assert_eq!(stream.oldest_sequence(), 1);
    }

    #[test]
    fn test_register_consumer() {
        let stream = CdcStream::new(100);
        let cursor = stream.register_consumer("consumer-1".to_string());

        assert_eq!(cursor.consumer_id, "consumer-1");
        assert_eq!(cursor.last_sequence, 0);
        assert_eq!(stream.consumer_count(), 1);
    }

    #[test]
    fn test_unregister_consumer() {
        let stream = CdcStream::new(100);
        stream.register_consumer("consumer-1".to_string());

        assert!(stream.unregister_consumer("consumer-1"));
        assert_eq!(stream.consumer_count(), 0);

        assert!(!stream.unregister_consumer("nonexistent"));
    }

    #[test]
    fn test_consume() {
        let stream = CdcStream::new(100);
        stream.register_consumer("consumer-1".to_string());

        stream.publish(create_test_op(InodeId::new(1)), 1);
        stream.publish(create_test_op(InodeId::new(2)), 1);

        let events = stream.consume("consumer-1", 10);
        assert_eq!(events.len(), 2);

        let events = stream.consume("consumer-1", 10);
        assert_eq!(events.len(), 0); // Cursor advanced
    }

    #[test]
    fn test_consume_max_count() {
        let stream = CdcStream::new(100);
        stream.register_consumer("consumer-1".to_string());

        for i in 1..=10 {
            stream.publish(create_test_op(InodeId::new(i)), 1);
        }

        let events = stream.consume("consumer-1", 3);
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn test_peek() {
        let stream = CdcStream::new(100);
        stream.register_consumer("consumer-1".to_string());

        stream.publish(create_test_op(InodeId::new(1)), 1);
        stream.publish(create_test_op(InodeId::new(2)), 1);

        let events = stream.peek("consumer-1", 10);
        assert_eq!(events.len(), 2);

        // Peek doesn't advance cursor
        let events = stream.peek("consumer-1", 10);
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_seek() {
        let stream = CdcStream::new(100);
        stream.register_consumer("consumer-1".to_string());

        stream.publish(create_test_op(InodeId::new(1)), 1);
        stream.publish(create_test_op(InodeId::new(2)), 1);
        stream.publish(create_test_op(InodeId::new(3)), 1);

        assert!(stream.seek("consumer-1", 2));

        let events = stream.consume("consumer-1", 10);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].sequence, 3);
    }

    #[test]
    fn test_lag() {
        let stream = CdcStream::new(100);
        stream.register_consumer("consumer-1".to_string());

        for i in 1..=5 {
            stream.publish(create_test_op(InodeId::new(i)), 1);
        }

        let lag = stream.lag("consumer-1");
        assert_eq!(lag, Some(5));

        stream.consume("consumer-1", 3);
        let lag = stream.lag("consumer-1");
        assert_eq!(lag, Some(2));
    }

    #[test]
    fn test_lag_nonexistent_consumer() {
        let stream = CdcStream::new(100);
        let lag = stream.lag("nonexistent");
        assert!(lag.is_none());
    }

    #[test]
    fn test_consumer_count() {
        let stream = CdcStream::new(100);
        assert_eq!(stream.consumer_count(), 0);

        stream.register_consumer("c1".to_string());
        stream.register_consumer("c2".to_string());

        assert_eq!(stream.consumer_count(), 2);
    }

    #[test]
    fn test_total_events() {
        let stream = CdcStream::new(100);
        assert_eq!(stream.total_events(), 0);

        stream.publish(create_test_op(InodeId::new(1)), 1);
        assert_eq!(stream.total_events(), 1);
    }

    #[test]
    fn test_oldest_sequence() {
        let stream = CdcStream::new(100);
        assert_eq!(stream.oldest_sequence(), 0);

        stream.publish(create_test_op(InodeId::new(1)), 1);
        stream.publish(create_test_op(InodeId::new(2)), 1);

        assert_eq!(stream.oldest_sequence(), 1);
    }

    #[test]
    fn test_max_events_eviction() {
        let stream = CdcStream::new(5);

        for i in 1..=10 {
            stream.publish(create_test_op(InodeId::new(i)), 1);
        }

        assert_eq!(stream.total_events(), 5);
        assert_eq!(stream.oldest_sequence(), 6);
    }

    #[test]
    fn test_cdc_cursor_new() {
        let cursor = CdcCursor::new("test".to_string());
        assert_eq!(cursor.consumer_id, "test");
        assert_eq!(cursor.last_sequence, 0);
    }

    #[test]
    fn test_cdc_cursor_with_sequence() {
        let cursor = CdcCursor::with_sequence("test".to_string(), 100);
        assert_eq!(cursor.consumer_id, "test");
        assert_eq!(cursor.last_sequence, 100);
    }

    #[test]
    fn test_multiple_consumers_independent() {
        let stream = CdcStream::new(100);

        stream.register_consumer("c1".to_string());
        stream.register_consumer("c2".to_string());

        stream.publish(create_test_op(InodeId::new(1)), 1);

        let events1 = stream.consume("c1", 10);
        assert_eq!(events1.len(), 1);

        let events2 = stream.consume("c2", 10);
        assert_eq!(events2.len(), 1);
    }
}
