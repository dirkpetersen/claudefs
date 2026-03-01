use crate::error::{FuseError, Result};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RequestState {
    Pending,
    Processing,
    Interrupted,
    Completed,
}

#[derive(Debug, Clone)]
pub struct RequestRecord {
    pub id: RequestId,
    pub opcode: u32,
    pub pid: u32,
    pub state: RequestState,
    pub enqueued_at_ms: u64,
    pub started_at_ms: Option<u64>,
}

impl RequestRecord {
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

    pub fn wait_ms(&self, now_ms: u64) -> u64 {
        now_ms.saturating_sub(self.enqueued_at_ms)
    }

    pub fn is_done(&self) -> bool {
        matches!(
            self.state,
            RequestState::Interrupted | RequestState::Completed
        )
    }
}

pub struct InterruptTracker {
    pending: HashMap<RequestId, RequestRecord>,
    max_pending: usize,
    total_interrupted: u64,
    total_completed: u64,
}

impl InterruptTracker {
    pub fn new(max_pending: usize) -> Self {
        InterruptTracker {
            pending: HashMap::new(),
            max_pending,
            total_interrupted: 0,
            total_completed: 0,
        }
    }

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

    pub fn start(&mut self, id: RequestId, now_ms: u64) -> bool {
        if let Some(record) = self.pending.get_mut(&id) {
            record.state = RequestState::Processing;
            record.started_at_ms = Some(now_ms);
            true
        } else {
            false
        }
    }

    pub fn interrupt(&mut self, id: RequestId) -> bool {
        if let Some(record) = self.pending.get_mut(&id) {
            record.state = RequestState::Interrupted;
            self.total_interrupted += 1;
            true
        } else {
            false
        }
    }

    pub fn complete(&mut self, id: RequestId) -> bool {
        if self.pending.remove(&id).is_some() {
            self.total_completed += 1;
            true
        } else {
            false
        }
    }

    pub fn is_interrupted(&self, id: RequestId) -> bool {
        self.pending
            .get(&id)
            .map(|r| r.state == RequestState::Interrupted)
            .unwrap_or(false)
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn total_interrupted(&self) -> u64 {
        self.total_interrupted
    }

    pub fn total_completed(&self) -> u64 {
        self.total_completed
    }

    pub fn interrupted_ids(&self) -> Vec<RequestId> {
        self.pending
            .values()
            .filter(|r| r.state == RequestState::Interrupted)
            .map(|r| r.id)
            .collect()
    }

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
