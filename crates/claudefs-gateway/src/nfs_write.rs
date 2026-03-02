//! NFS3 write commit tracking

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Write stability level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u32)]
pub enum WriteStability {
    /// Written to volatile cache (default, fastest)
    Unstable = 0,
    /// Written to data sync
    DataSync = 1,
    /// Written to file sync (synchronous)
    FileSync = 2,
}

/// A pending write operation
#[derive(Debug, Clone)]
pub struct PendingWrite {
    /// File handle key identifying the file
    pub fh_key: Vec<u8>,
    /// Byte offset where write starts
    pub offset: u64,
    /// Number of bytes written
    pub count: u32,
    /// Stability level of the write
    pub stability: WriteStability,
    /// Write verifier for this operation
    pub verf: u64,
}

/// Write commit tracker — tracks unstable writes that need committing
pub struct WriteTracker {
    pending: Mutex<HashMap<Vec<u8>, Vec<PendingWrite>>>,
    write_verf: u64,
    total_pending: AtomicU64,
}

impl WriteTracker {
    /// Creates a new WriteTracker with the given verifier.
    pub fn new(verf: u64) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            write_verf: verf,
            total_pending: AtomicU64::new(0),
        }
    }

    /// Records a pending write operation for tracking.
    pub fn record_write(
        &self,
        fh_key: Vec<u8>,
        offset: u64,
        count: u32,
        stability: WriteStability,
    ) {
        let write = PendingWrite {
            fh_key: fh_key.clone(),
            offset,
            count,
            stability,
            verf: self.write_verf,
        };

        let mut pending = self.pending.lock().unwrap();
        let entry = pending.entry(fh_key).or_default();
        entry.push(write);
        self.total_pending.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns all pending writes for a given file handle.
    pub fn pending_writes(&self, fh_key: &[u8]) -> Vec<PendingWrite> {
        let pending = self.pending.lock().unwrap();
        pending.get(fh_key).cloned().unwrap_or_default()
    }

    /// Commits all pending writes for a file handle and returns the verifier.
    pub fn commit(&self, fh_key: &[u8]) -> u64 {
        let mut pending = self.pending.lock().unwrap();
        let count = pending.remove(fh_key).map(|v| v.len()).unwrap_or(0);
        self.total_pending
            .fetch_sub(count as u64, Ordering::Relaxed);
        self.write_verf
    }

    /// Commits all pending writes across all files and returns the verifier.
    pub fn commit_all(&self) -> u64 {
        let mut pending = self.pending.lock().unwrap();
        let _count: usize = pending.values().map(|v| v.len()).sum();
        pending.clear();
        self.total_pending.store(0, Ordering::Relaxed);
        self.write_verf
    }

    /// Returns true if there are any pending writes for the given file handle.
    pub fn has_pending_writes(&self, fh_key: &[u8]) -> bool {
        let pending = self.pending.lock().unwrap();
        pending.get(fh_key).map(|v| !v.is_empty()).unwrap_or(false)
    }

    /// Returns the number of pending writes for a given file handle.
    pub fn pending_count(&self, fh_key: &[u8]) -> usize {
        let pending = self.pending.lock().unwrap();
        pending.get(fh_key).map(|v| v.len()).unwrap_or(0)
    }

    /// Returns the total number of pending writes across all files.
    pub fn total_pending(&self) -> u64 {
        self.total_pending.load(Ordering::Relaxed)
    }

    /// Returns the write verifier used by this tracker.
    pub fn write_verf(&self) -> u64 {
        self.write_verf
    }

    /// Removes all pending writes for a given file handle.
    pub fn remove_file(&self, fh_key: &[u8]) {
        let mut pending = self.pending.lock().unwrap();
        let count = pending.remove(fh_key).map(|v| v.len()).unwrap_or(0);
        self.total_pending
            .fetch_sub(count as u64, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fh_key(v: u64) -> Vec<u8> {
        v.to_le_bytes().to_vec()
    }

    #[test]
    fn test_write_tracker_new() {
        let tracker = WriteTracker::new(12345);
        assert_eq!(tracker.write_verf(), 12345);
        assert_eq!(tracker.total_pending(), 0);
    }

    #[test]
    fn test_record_write() {
        let tracker = WriteTracker::new(100);
        tracker.record_write(fh_key(1), 0, 4096, WriteStability::Unstable);

        let writes = tracker.pending_writes(&fh_key(1));
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].offset, 0);
        assert_eq!(writes[0].count, 4096);
        assert_eq!(writes[0].stability, WriteStability::Unstable);
    }

    #[test]
    fn test_record_write_multiple() {
        let tracker = WriteTracker::new(100);
        tracker.record_write(fh_key(1), 0, 4096, WriteStability::Unstable);
        tracker.record_write(fh_key(1), 4096, 4096, WriteStability::DataSync);

        let writes = tracker.pending_writes(&fh_key(1));
        assert_eq!(writes.len(), 2);
        assert_eq!(tracker.total_pending(), 2);
    }

    #[test]
    fn test_pending_writes_empty() {
        let tracker = WriteTracker::new(100);
        let writes = tracker.pending_writes(&fh_key(999));
        assert!(writes.is_empty());
    }

    #[test]
    fn test_commit() {
        let tracker = WriteTracker::new(100);
        tracker.record_write(fh_key(1), 0, 4096, WriteStability::Unstable);

        let verf = tracker.commit(&fh_key(1));
        assert_eq!(verf, 100);

        let writes = tracker.pending_writes(&fh_key(1));
        assert!(writes.is_empty());
        assert_eq!(tracker.total_pending(), 0);
    }

    #[test]
    fn test_commit_nonexistent() {
        let tracker = WriteTracker::new(100);
        let verf = tracker.commit(&fh_key(999));
        assert_eq!(verf, 100);
    }

    #[test]
    fn test_commit_all() {
        let tracker = WriteTracker::new(100);
        tracker.record_write(fh_key(1), 0, 4096, WriteStability::Unstable);
        tracker.record_write(fh_key(2), 0, 8192, WriteStability::DataSync);

        let verf = tracker.commit_all();
        assert_eq!(verf, 100);

        assert_eq!(tracker.total_pending(), 0);
        assert!(tracker.pending_writes(&fh_key(1)).is_empty());
        assert!(tracker.pending_writes(&fh_key(2)).is_empty());
    }

    #[test]
    fn test_has_pending_writes() {
        let tracker = WriteTracker::new(100);
        tracker.record_write(fh_key(1), 0, 4096, WriteStability::Unstable);

        assert!(tracker.has_pending_writes(&fh_key(1)));
        assert!(!tracker.has_pending_writes(&fh_key(999)));
    }

    #[test]
    fn test_pending_count() {
        let tracker = WriteTracker::new(100);
        tracker.record_write(fh_key(1), 0, 4096, WriteStability::Unstable);
        tracker.record_write(fh_key(1), 4096, 4096, WriteStability::Unstable);

        assert_eq!(tracker.pending_count(&fh_key(1)), 2);
        assert_eq!(tracker.pending_count(&fh_key(999)), 0);
    }

    #[test]
    fn test_total_pending() {
        let tracker = WriteTracker::new(100);
        tracker.record_write(fh_key(1), 0, 4096, WriteStability::Unstable);
        tracker.record_write(fh_key(2), 0, 8192, WriteStability::DataSync);

        assert_eq!(tracker.total_pending(), 2);
    }

    #[test]
    fn test_write_verf() {
        let tracker = WriteTracker::new(99999);
        assert_eq!(tracker.write_verf(), 99999);
    }

    #[test]
    fn test_remove_file() {
        let tracker = WriteTracker::new(100);
        tracker.record_write(fh_key(1), 0, 4096, WriteStability::Unstable);
        tracker.record_write(fh_key(2), 0, 8192, WriteStability::DataSync);

        tracker.remove_file(&fh_key(1));

        assert!(tracker.pending_writes(&fh_key(1)).is_empty());
        assert!(!tracker.pending_writes(&fh_key(2)).is_empty());
        assert_eq!(tracker.total_pending(), 1);
    }

    #[test]
    fn test_write_stability_ordering() {
        assert!(WriteStability::Unstable < WriteStability::DataSync);
        assert!(WriteStability::DataSync < WriteStability::FileSync);
    }

    #[test]
    fn test_pending_write_fields() {
        let tracker = WriteTracker::new(100);
        tracker.record_write(fh_key(42), 1000, 2048, WriteStability::FileSync);

        let writes = tracker.pending_writes(&fh_key(42));
        let write = &writes[0];

        assert_eq!(write.fh_key, fh_key(42));
        assert_eq!(write.offset, 1000);
        assert_eq!(write.count, 2048);
        assert_eq!(write.stability, WriteStability::FileSync);
        assert_eq!(write.verf, 100);
    }
}
