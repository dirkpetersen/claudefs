//! FUSE request interrupt handling.
//!
//! Tracks pending FUSE requests and handles `FUSE_INTERRUPT` operations.
//! The kernel may send an interrupt for a previously issued request,
//! typically when the calling process receives a signal.

use crate::error::{FuseError, Result};
use std::collections::HashMap;

/// Unique identifier for a FUSE request.
///
/// Wraps the kernel-provided `unique` value for each request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestId(pub u64);

/// Lifecycle state of a tracked request.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RequestState {
    /// Request is queued but not yet being processed.
    Pending,
    /// Request is currently being processed by a worker.
    Processing,
    /// Request was interrupted by the kernel before completion.
    Interrupted,
    /// Request finished successfully or with an error.
    Completed,
}

/// Record of a tracked FUSE request.
#[derive(Debug, Clone)]
pub struct RequestRecord {
    /// The unique request identifier.
    pub id: RequestId,
    /// The FUSE opcode (e.g., `FUSE_READ`, `FUSE_WRITE`).
    pub opcode: u32,
    /// Process ID of the requesting process.
    pub pid: u32,
    /// Current lifecycle state of the request.
    pub state: RequestState,
    /// Timestamp (ms) when the request was enqueued.
    pub enqueued_at_ms: u64,
    /// Timestamp (ms) when processing started, if any.
    pub started_at_ms: Option<u64>,
}

impl RequestRecord {
    /// Creates a new request record in the `Pending` state.
    ///
    /// # Arguments
    ///
    /// * `id` - Unique request identifier.
    /// * `opcode` - FUSE operation code.
    /// * `pid` - Process ID of the caller.
    /// * `now_ms` - Current timestamp in milliseconds.
    pub fn new(id: RequestId, opcode: u32, pid: u32, now_ms: u64) -> Self {
        RequestRecord {
            id,
            opcode,
            pid,
            state: RequestState::Pending,
            enqueued_at_ms: now_ms,
            started_at_ms: None,
        }
    }

    /// Returns how long this request has been waiting (ms).
    ///
    /// Uses saturating subtraction to handle clock skew.
    pub fn wait_ms(&self, now_ms: u64) -> u64 {
        now_ms.saturating_sub(self.enqueued_at_ms)
    }

    /// Returns `true` if the request is no longer active.
    ///
    /// Both interrupted and completed requests are considered done.
    pub fn is_done(&self) -> bool {
        matches!(
            self.state,
            RequestState::Interrupted | RequestState::Completed
        )
    }
}

/// Tracks pending FUSE requests and handles interrupts.
///
/// Used to detect and handle `FUSE_INTERRUPT` requests from the kernel.
/// Workers should check `is_interrupted` during long-running operations.
pub struct InterruptTracker {
    pending: HashMap<RequestId, RequestRecord>,
    max_pending: usize,
    total_interrupted: u64,
    total_completed: u64,
}

impl InterruptTracker {
    /// Creates a new tracker with a maximum pending request count.
    ///
    /// # Arguments
    ///
    /// * `max_pending` - Maximum number of concurrent requests to track.
    pub fn new(max_pending: usize) -> Self {
        InterruptTracker {
            pending: HashMap::new(),
            max_pending,
            total_interrupted: 0,
            total_completed: 0,
        }
    }

    /// Registers a new request for tracking.
    ///
    /// Returns an error if the tracker is at capacity.
    ///
    /// # Arguments
    ///
    /// * `id` - Unique request identifier.
    /// * `opcode` - FUSE operation code.
    /// * `pid` - Process ID of the caller.
    /// * `now_ms` - Current timestamp in milliseconds.
    pub fn register(&mut self, id: RequestId, opcode: u32, pid: u32, now_ms: u64) -> Result<()> {
        if self.pending.len() >= self.max_pending {
            return Err(FuseError::InvalidArgument {
                msg: "interrupt tracker at capacity".to_string(),
            });
        }

        let record = RequestRecord::new(id, opcode, pid, now_ms);
        self.pending.insert(id, record);
        Ok(())
    }

    /// Marks a request as being processed by a worker.
    ///
    /// Returns `true` if the request was found and updated.
    pub fn start(&mut self, id: RequestId, now_ms: u64) -> bool {
        if let Some(record) = self.pending.get_mut(&id) {
            record.state = RequestState::Processing;
            record.started_at_ms = Some(now_ms);
            true
        } else {
            false
        }
    }

    /// Marks a request as interrupted by the kernel.
    ///
    /// Returns `true` if the request was found and marked.
    /// Increments the interrupted counter.
    pub fn interrupt(&mut self, id: RequestId) -> bool {
        if let Some(record) = self.pending.get_mut(&id) {
            record.state = RequestState::Interrupted;
            self.total_interrupted += 1;
            true
        } else {
            false
        }
    }

    /// Removes a completed request from tracking.
    ///
    /// Returns `true` if the request was found and removed.
    /// Increments the completed counter.
    pub fn complete(&mut self, id: RequestId) -> bool {
        if self.pending.remove(&id).is_some() {
            self.total_completed += 1;
            true
        } else {
            false
        }
    }

    /// Checks if a request has been interrupted.
    ///
    /// Returns `false` for unknown request IDs.
    pub fn is_interrupted(&self, id: RequestId) -> bool {
        self.pending
            .get(&id)
            .map(|r| r.state == RequestState::Interrupted)
            .unwrap_or(false)
    }

    /// Returns the current number of pending requests.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Returns the total number of interrupted requests.
    pub fn total_interrupted(&self) -> u64 {
        self.total_interrupted
    }

    /// Returns the total number of completed requests.
    pub fn total_completed(&self) -> u64 {
        self.total_completed
    }

    /// Returns IDs of all currently interrupted requests.
    pub fn interrupted_ids(&self) -> Vec<RequestId> {
        self.pending
            .values()
            .filter(|r| r.state == RequestState::Interrupted)
            .map(|r| r.id)
            .collect()
    }

    /// Removes and returns requests that have exceeded a timeout.
    ///
    /// # Arguments
    ///
    /// * `now_ms` - Current timestamp in milliseconds.
    /// * `timeout_ms` - Maximum wait time before a request is considered stale.
    ///
    /// # Returns
    ///
    /// Vector of timed-out request IDs (removed from tracking).
    pub fn drain_timed_out(&mut self, now_ms: u64, timeout_ms: u64) -> Vec<RequestId> {
        let timed_out: Vec<RequestId> = self
            .pending
            .iter()
            .filter(|(_, r)| r.wait_ms(now_ms) > timeout_ms)
            .map(|(id, _)| *id)
            .collect();

        for id in &timed_out {
            self.pending.remove(id);
            self.total_completed += 1;
        }

        timed_out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_tracker() -> InterruptTracker {
        InterruptTracker::new(100)
    }

    #[test]
    fn test_register_request() {
        let mut tracker = create_test_tracker();
        let result = tracker.register(RequestId(1), 15, 1000, 0);
        assert!(result.is_ok());
        assert_eq!(tracker.pending_count(), 1);
    }

    #[test]
    fn test_register_start_complete_lifecycle() {
        let mut tracker = create_test_tracker();

        tracker.register(RequestId(1), 15, 1000, 0).unwrap();

        assert!(tracker.start(RequestId(1), 100));

        let record = tracker.pending.get(&RequestId(1)).unwrap();
        assert_eq!(record.state, RequestState::Processing);
        assert_eq!(record.started_at_ms, Some(100));

        assert!(tracker.complete(RequestId(1)));
        assert_eq!(tracker.pending_count(), 0);
        assert_eq!(tracker.total_completed(), 1);
    }

    #[test]
    fn test_interrupt_marks_as_interrupted() {
        let mut tracker = create_test_tracker();

        tracker.register(RequestId(1), 15, 1000, 0).unwrap();
        assert!(tracker.interrupt(RequestId(1)));

        let record = tracker.pending.get(&RequestId(1)).unwrap();
        assert_eq!(record.state, RequestState::Interrupted);
    }

    #[test]
    fn test_is_interrupted_returns_true() {
        let mut tracker = create_test_tracker();

        tracker.register(RequestId(1), 15, 1000, 0).unwrap();
        tracker.interrupt(RequestId(1));

        assert!(tracker.is_interrupted(RequestId(1)));
        assert!(!tracker.is_interrupted(RequestId(2)));
    }

    #[test]
    fn test_complete_increments_counter() {
        let mut tracker = create_test_tracker();

        tracker.register(RequestId(1), 15, 1000, 0).unwrap();
        tracker.complete(RequestId(1));

        assert_eq!(tracker.total_completed(), 1);
    }

    #[test]
    fn test_drain_timed_out_removes_stale() {
        let mut tracker = create_test_tracker();

        tracker.register(RequestId(1), 15, 1000, 0).unwrap();

        let timed_out = tracker.drain_timed_out(10000, 5000);
        assert_eq!(timed_out.len(), 1);
        assert_eq!(timed_out[0], RequestId(1));

        assert_eq!(tracker.pending_count(), 0);
    }

    #[test]
    fn test_drain_timed_out_keeps_recent() {
        let mut tracker = create_test_tracker();

        tracker.register(RequestId(1), 15, 1000, 0).unwrap();

        let timed_out = tracker.drain_timed_out(2000, 5000);
        assert!(timed_out.is_empty());

        assert_eq!(tracker.pending_count(), 1);
    }

    #[test]
    fn test_pending_count_decrements_on_complete() {
        let mut tracker = create_test_tracker();

        tracker.register(RequestId(1), 15, 1000, 0).unwrap();
        tracker.register(RequestId(2), 16, 1000, 0).unwrap();

        assert_eq!(tracker.pending_count(), 2);

        tracker.complete(RequestId(1));
        assert_eq!(tracker.pending_count(), 1);
    }

    #[test]
    fn test_capacity_limit_returns_err() {
        let mut tracker = InterruptTracker::new(2);

        tracker.register(RequestId(1), 15, 1000, 0).unwrap();
        tracker.register(RequestId(2), 15, 1000, 0).unwrap();

        let result = tracker.register(RequestId(3), 15, 1000, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_interrupted_ids_returns_correct_set() {
        let mut tracker = create_test_tracker();

        tracker.register(RequestId(1), 15, 1000, 0).unwrap();
        tracker.register(RequestId(2), 16, 1000, 0).unwrap();

        tracker.interrupt(RequestId(1));

        let ids = tracker.interrupted_ids();
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], RequestId(1));
    }

    #[test]
    fn test_multiple_requests_coexist() {
        let mut tracker = create_test_tracker();

        tracker.register(RequestId(1), 15, 1000, 0).unwrap();
        tracker.register(RequestId(2), 16, 1001, 0).unwrap();
        tracker.register(RequestId(3), 17, 1002, 0).unwrap();

        assert_eq!(tracker.pending_count(), 3);

        let record1 = tracker.pending.get(&RequestId(1)).unwrap();
        let record2 = tracker.pending.get(&RequestId(2)).unwrap();
        let record3 = tracker.pending.get(&RequestId(3)).unwrap();

        assert_eq!(record1.opcode, 15);
        assert_eq!(record2.pid, 1001);
        assert_eq!(record3.state, RequestState::Pending);
    }

    #[test]
    fn test_start_returns_false_if_not_found() {
        let mut tracker = create_test_tracker();

        assert!(!tracker.start(RequestId(999), 0));
    }

    #[test]
    fn test_interrupt_returns_false_if_not_found() {
        let mut tracker = create_test_tracker();

        assert!(!tracker.interrupt(RequestId(999)));
    }

    #[test]
    fn test_complete_returns_false_if_not_found() {
        let mut tracker = create_test_tracker();

        assert!(!tracker.complete(RequestId(999)));
    }

    #[test]
    fn test_request_wait_ms() {
        let record = RequestRecord::new(RequestId(1), 15, 1000, 1000);

        assert_eq!(record.wait_ms(1500), 500);
        assert_eq!(record.wait_ms(500), 0);
    }

    #[test]
    fn test_request_is_done() {
        let mut record = RequestRecord::new(RequestId(1), 15, 1000, 0);

        assert!(!record.is_done());

        record.state = RequestState::Interrupted;
        assert!(record.is_done());

        record.state = RequestState::Completed;
        assert!(record.is_done());

        record.state = RequestState::Processing;
        assert!(!record.is_done());
    }

    #[test]
    fn test_request_id_equality() {
        let id1 = RequestId(1);
        let id2 = RequestId(1);
        let id3 = RequestId(2);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_total_interrupted_increments() {
        let mut tracker = create_test_tracker();

        tracker.register(RequestId(1), 15, 1000, 0).unwrap();
        tracker.interrupt(RequestId(1));

        assert_eq!(tracker.total_interrupted(), 1);
    }
}
