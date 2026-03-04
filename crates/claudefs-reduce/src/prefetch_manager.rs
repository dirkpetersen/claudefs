//! Higher-level prefetch manager coordinating requests from multiple files.

use std::collections::VecDeque;
use thiserror::Error;

/// A prefetch request for one or more chunks.
#[derive(Debug, Clone)]
pub struct PrefetchRequest {
    /// Inode being prefetched.
    pub inode_id: u64,
    /// Chunk hashes to prefetch.
    pub chunk_hashes: Vec<[u8; 32]>,
    /// Priority (0-255, higher is more urgent).
    pub priority: u8,
    /// Creation timestamp in milliseconds.
    pub created_at_ms: u64,
}

/// Status of a prefetch request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefetchStatus {
    /// Waiting to be processed.
    Pending,
    /// Currently being fetched.
    InFlight,
    /// Successfully completed.
    Completed,
    /// Failed to complete.
    Failed,
}

/// An entry in the prefetch manager.
#[derive(Debug, Clone)]
pub struct PrefetchEntry {
    /// The original request.
    pub request: PrefetchRequest,
    /// Current status.
    pub status: PrefetchStatus,
    /// Number of chunks completed.
    pub completed_count: usize,
}

/// Configuration for the prefetch manager.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PrefetchManagerConfig {
    /// Maximum pending requests.
    pub max_pending: usize,
    /// Maximum in-flight requests.
    pub max_inflight: usize,
    /// Maximum chunks per request.
    pub max_chunks_per_request: usize,
}

impl Default for PrefetchManagerConfig {
    fn default() -> Self {
        Self {
            max_pending: 100,
            max_inflight: 10,
            max_chunks_per_request: 16,
        }
    }
}

/// Errors from the prefetch manager.
#[derive(Debug, Error)]
pub enum PrefetchError {
    /// Queue is full.
    #[error("prefetch queue is full")]
    QueueFull,
    /// Too many chunks in request.
    #[error("too many chunks in request")]
    TooManyChunks,
}

/// The prefetch manager.
#[derive(Debug)]
pub struct PrefetchManager {
    config: PrefetchManagerConfig,
    pending: VecDeque<(u64, PrefetchEntry)>,
    inflight: VecDeque<(u64, PrefetchEntry)>,
    completed: VecDeque<(u64, PrefetchEntry)>,
    next_id: u64,
}

impl PrefetchManager {
    /// Create a new prefetch manager.
    pub fn new(config: PrefetchManagerConfig) -> Self {
        Self {
            config,
            pending: VecDeque::new(),
            inflight: VecDeque::new(),
            completed: VecDeque::new(),
            next_id: 1,
        }
    }

    /// Submit a new prefetch request.
    pub fn submit(&mut self, request: PrefetchRequest) -> Result<u64, PrefetchError> {
        if self.pending.len() >= self.config.max_pending {
            return Err(PrefetchError::QueueFull);
        }

        if request.chunk_hashes.len() > self.config.max_chunks_per_request {
            return Err(PrefetchError::TooManyChunks);
        }

        let id = self.next_id;
        self.next_id += 1;

        let entry = PrefetchEntry {
            request,
            status: PrefetchStatus::Pending,
            completed_count: 0,
        };

        self.pending.push_back((id, entry));
        Ok(id)
    }

    /// Dequeue the highest-priority pending request.
    pub fn next_request(&mut self) -> Option<PrefetchRequest> {
        if self.inflight.len() >= self.config.max_inflight {
            return None;
        }

        let best_idx = self.find_highest_priority();
        if let Some(idx) = best_idx {
            let (id, mut entry) = self.pending.remove(idx).unwrap();
            entry.status = PrefetchStatus::InFlight;
            let request = entry.request.clone();
            self.inflight.push_back((id, entry));
            return Some(request);
        }
        None
    }

    /// Mark a request as completed or failed.
    pub fn complete(&mut self, request_id: u64, success: bool) {
        if let Some(idx) = self.inflight.iter().position(|(id, _)| *id == request_id) {
            let (id, mut entry) = self.inflight.remove(idx).unwrap();
            entry.status = if success {
                PrefetchStatus::Completed
            } else {
                PrefetchStatus::Failed
            };
            entry.completed_count = entry.request.chunk_hashes.len();
            self.completed.push_back((id, entry));
        }
    }

    /// Return count of pending requests.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Return count of in-flight requests.
    pub fn inflight_count(&self) -> usize {
        self.inflight.len()
    }

    /// Drain all completed entries.
    pub fn drain_completed(&mut self) -> Vec<PrefetchEntry> {
        self.completed.drain(..).map(|(_, entry)| entry).collect()
    }

    fn find_highest_priority(&self) -> Option<usize> {
        let mut best_idx: Option<usize> = None;
        let mut best_priority: u8 = 0;

        for (idx, (_, entry)) in self.pending.iter().enumerate() {
            if entry.request.priority > best_priority || best_idx.is_none() {
                best_priority = entry.request.priority;
                best_idx = Some(idx);
            }
        }

        best_idx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request(priority: u8, chunks: usize) -> PrefetchRequest {
        PrefetchRequest {
            inode_id: 1,
            chunk_hashes: vec![[0u8; 32]; chunks],
            priority,
            created_at_ms: 0,
        }
    }

    #[test]
    fn manager_config_default() {
        let config = PrefetchManagerConfig::default();
        assert_eq!(config.max_pending, 100);
        assert_eq!(config.max_inflight, 10);
        assert_eq!(config.max_chunks_per_request, 16);
    }

    #[test]
    fn submit_returns_id() {
        let mut manager = PrefetchManager::new(PrefetchManagerConfig::default());
        let req = make_request(1, 1);
        let id = manager.submit(req).unwrap();
        assert!(id > 0);
    }

    #[test]
    fn pending_count_after_submit() {
        let mut manager = PrefetchManager::new(PrefetchManagerConfig::default());
        let req = make_request(1, 1);
        manager.submit(req).unwrap();
        assert_eq!(manager.pending_count(), 1);
    }

    #[test]
    fn next_request_dequeues_highest_priority() {
        let mut manager = PrefetchManager::new(PrefetchManagerConfig::default());
        manager.submit(make_request(1, 1)).unwrap();
        manager.submit(make_request(5, 1)).unwrap();
        manager.submit(make_request(3, 1)).unwrap();

        let req = manager.next_request().unwrap();
        assert_eq!(req.priority, 5);
    }

    #[test]
    fn next_request_marks_inflight() {
        let mut manager = PrefetchManager::new(PrefetchManagerConfig::default());
        manager.submit(make_request(1, 1)).unwrap();
        manager.next_request().unwrap();
        assert_eq!(manager.inflight_count(), 1);
    }

    #[test]
    fn inflight_count_after_next() {
        let mut manager = PrefetchManager::new(PrefetchManagerConfig::default());
        manager.submit(make_request(1, 1)).unwrap();
        manager.submit(make_request(2, 1)).unwrap();
        manager.next_request().unwrap();
        assert_eq!(manager.inflight_count(), 1);
        manager.next_request().unwrap();
        assert_eq!(manager.inflight_count(), 2);
    }

    #[test]
    fn complete_success() {
        let mut manager = PrefetchManager::new(PrefetchManagerConfig::default());
        let id = manager.submit(make_request(1, 1)).unwrap();
        manager.next_request().unwrap();
        manager.complete(id, true);
        assert_eq!(manager.inflight_count(), 0);
        let drained = manager.drain_completed();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].status, PrefetchStatus::Completed);
    }

    #[test]
    fn complete_failure() {
        let mut manager = PrefetchManager::new(PrefetchManagerConfig::default());
        let id = manager.submit(make_request(1, 1)).unwrap();
        manager.next_request().unwrap();
        manager.complete(id, false);
        let drained = manager.drain_completed();
        assert_eq!(drained[0].status, PrefetchStatus::Failed);
    }

    #[test]
    fn drain_completed_returns_completed() {
        let mut manager = PrefetchManager::new(PrefetchManagerConfig::default());
        let id1 = manager.submit(make_request(1, 1)).unwrap();
        let id2 = manager.submit(make_request(2, 1)).unwrap();
        manager.next_request().unwrap();
        manager.next_request().unwrap();
        manager.complete(id1, true);
        manager.complete(id2, true);
        let drained = manager.drain_completed();
        assert_eq!(drained.len(), 2);
    }

    #[test]
    fn drain_completed_clears() {
        let mut manager = PrefetchManager::new(PrefetchManagerConfig::default());
        let id = manager.submit(make_request(1, 1)).unwrap();
        manager.next_request().unwrap();
        manager.complete(id, true);
        manager.drain_completed();
        let drained = manager.drain_completed();
        assert!(drained.is_empty());
    }

    #[test]
    fn submit_full_queue_returns_error() {
        let config = PrefetchManagerConfig {
            max_pending: 2,
            ..Default::default()
        };
        let mut manager = PrefetchManager::new(config);
        manager.submit(make_request(1, 1)).unwrap();
        manager.submit(make_request(2, 1)).unwrap();
        let result = manager.submit(make_request(3, 1));
        assert!(matches!(result, Err(PrefetchError::QueueFull)));
    }

    #[test]
    fn submit_too_many_chunks_returns_error() {
        let config = PrefetchManagerConfig {
            max_chunks_per_request: 2,
            ..Default::default()
        };
        let mut manager = PrefetchManager::new(config);
        let req = make_request(1, 5);
        let result = manager.submit(req);
        assert!(matches!(result, Err(PrefetchError::TooManyChunks)));
    }

    #[test]
    fn next_request_empty_returns_none() {
        let mut manager = PrefetchManager::new(PrefetchManagerConfig::default());
        assert!(manager.next_request().is_none());
    }

    #[test]
    fn priority_ordering() {
        let mut manager = PrefetchManager::new(PrefetchManagerConfig::default());
        manager.submit(make_request(10, 1)).unwrap();
        manager.submit(make_request(50, 1)).unwrap();
        manager.submit(make_request(30, 1)).unwrap();
        manager.submit(make_request(20, 1)).unwrap();

        let r1 = manager.next_request().unwrap();
        assert_eq!(r1.priority, 50);

        let r2 = manager.next_request().unwrap();
        assert_eq!(r2.priority, 30);

        let r3 = manager.next_request().unwrap();
        assert_eq!(r3.priority, 20);

        let r4 = manager.next_request().unwrap();
        assert_eq!(r4.priority, 10);
    }
}
