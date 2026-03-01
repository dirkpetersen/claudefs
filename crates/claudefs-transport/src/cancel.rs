//! Request cancellation propagation for cooperative cancellation across RPC boundaries.
//!
//! This module provides cooperative request cancellation that propagates across the RPC boundary.
//! When a client disconnects or times out, the server can cancel in-flight work to save resources.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use thiserror::Error;

/// Reasons for request cancellation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CancelReason {
    /// Client went away (disconnected, closed connection, etc.)
    ClientDisconnected,
    /// Deadline expired (request took too long)
    DeadlineExceeded,
    /// Server is shutting down
    ServerShutdown,
    /// Explicit user cancellation
    #[default]
    UserRequested,
    /// Newer request supersedes this one
    Superseded,
}

impl std::fmt::Display for CancelReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CancelReason::ClientDisconnected => write!(f, "ClientDisconnected"),
            CancelReason::DeadlineExceeded => write!(f, "DeadlineExceeded"),
            CancelReason::ServerShutdown => write!(f, "ServerShutdown"),
            CancelReason::UserRequested => write!(f, "UserRequested"),
            CancelReason::Superseded => write!(f, "Superseded"),
        }
    }
}

/// Error returned when an operation is cancelled.
#[derive(Error, Debug)]
#[error("Operation cancelled: {0}")]
pub struct CancelledError(pub CancelReason);

/// A lightweight, cloneable cancellation token.
/// Multiple recipients can listen for cancellation.
#[derive(Clone)]
pub struct CancelToken {
    cancelled: Arc<AtomicBool>,
    reason: Arc<Mutex<Option<CancelReason>>>,
    #[allow(dead_code)]
    children: Arc<Mutex<Vec<Arc<AtomicBool>>>>,
}

impl CancelToken {
    /// Check if this token has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Get the cancellation reason if cancelled.
    pub fn cancelled_reason(&self) -> Option<CancelReason> {
        *self.reason.lock().unwrap()
    }
}

/// The handle that triggers cancellation.
#[derive(Clone)]
pub struct CancelHandle {
    cancelled: Arc<AtomicBool>,
    reason: Arc<Mutex<Option<CancelReason>>>,
    children: Arc<Mutex<Vec<Arc<AtomicBool>>>>,
}

impl CancelHandle {
    /// Trigger cancellation with the given reason.
    pub fn cancel(&self, reason: CancelReason) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Ok(mut r) = self.reason.lock() {
            if r.is_none() {
                *r = Some(reason);
            }
        }
        if let Ok(children) = self.children.lock() {
            for child_cancelled in children.iter() {
                child_cancelled.store(true, Ordering::SeqCst);
            }
        }
    }

    /// Check if cancellation has been triggered.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// Create a new cancellation token/handle pair.
pub fn new_cancel_pair() -> (CancelToken, CancelHandle) {
    let cancelled = Arc::new(AtomicBool::new(false));
    let reason = Arc::new(Mutex::new(None));
    let children = Arc::new(Mutex::new(Vec::new()));

    let token = CancelToken {
        cancelled: cancelled.clone(),
        reason: reason.clone(),
        children: children.clone(),
    };

    let handle = CancelHandle {
        cancelled,
        reason,
        children,
    };

    (token, handle)
}

impl CancelToken {
    /// Create a child token that is cancelled when the parent is cancelled,
    /// but can also be cancelled independently.
    pub fn child(&self) -> (CancelToken, CancelHandle) {
        let cancelled = Arc::new(AtomicBool::new(self.is_cancelled()));
        let reason = Arc::new(Mutex::new(self.cancelled_reason()));
        let children = Arc::new(Mutex::new(Vec::new()));

        if let Ok(mut c) = self.children.lock() {
            c.push(cancelled.clone());
        }

        let token = CancelToken {
            cancelled: cancelled.clone(),
            reason: reason.clone(),
            children: children.clone(),
        };

        let handle = CancelHandle {
            cancelled,
            reason,
            children,
        };

        (token, handle)
    }
}

/// Statistics about cancellation operations.
#[derive(Debug, Clone, Default)]
pub struct CancelStats {
    /// Total requests registered.
    pub total_registered: u64,
    /// Total requests cancelled.
    pub total_cancelled: u64,
    /// Total requests completed normally (removed).
    pub total_completed: u64,
    /// Currently active count.
    pub active_count: usize,
}

/// Tracks active cancellation tokens for a server.
pub struct CancelRegistry {
    handles: Mutex<HashMap<u64, CancelHandle>>,
    total_registered: Mutex<u64>,
    total_cancelled: Mutex<u64>,
    total_completed: Mutex<u64>,
}

impl Default for CancelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CancelRegistry {
    /// Create a new registry.
    pub fn new() -> Self {
        CancelRegistry {
            handles: Mutex::new(HashMap::new()),
            total_registered: Mutex::new(0),
            total_cancelled: Mutex::new(0),
            total_completed: Mutex::new(0),
        }
    }

    /// Register a new request, returning token/handle.
    pub fn register(&self, request_id: u64) -> (CancelToken, CancelHandle) {
        let (token, handle) = new_cancel_pair();
        if let Ok(mut handles) = self.handles.lock() {
            handles.insert(request_id, handle.clone());
        }
        if let Ok(mut count) = self.total_registered.lock() {
            *count += 1;
        }
        (token, handle)
    }

    /// Cancel a specific request.
    pub fn cancel(&self, request_id: u64, reason: CancelReason) {
        if let Ok(handles) = self.handles.lock() {
            if let Some(handle) = handles.get(&request_id) {
                handle.cancel(reason);
                drop(handles);
                if let Ok(mut count) = self.total_cancelled.lock() {
                    *count += 1;
                }
            }
        }
    }

    /// Cancel all active requests.
    pub fn cancel_all(&self, reason: CancelReason) {
        if let Ok(handles) = self.handles.lock() {
            for handle in handles.values() {
                handle.cancel(reason);
            }
            let count = handles.len();
            drop(handles);
            if let Ok(mut total) = self.total_cancelled.lock() {
                *total += count as u64;
            }
        }
    }

    /// Remove a completed request.
    pub fn remove(&self, request_id: u64) {
        if let Ok(mut handles) = self.handles.lock() {
            if handles.remove(&request_id).is_some() {
                drop(handles);
                if let Ok(mut count) = self.total_completed.lock() {
                    *count += 1;
                }
            }
        }
    }

    /// Number of active cancellation tokens.
    pub fn active_count(&self) -> usize {
        self.handles.lock().unwrap().len()
    }

    /// Get statistics snapshot.
    pub fn stats(&self) -> CancelStats {
        CancelStats {
            total_registered: *self.total_registered.lock().unwrap(),
            total_cancelled: *self.total_cancelled.lock().unwrap(),
            total_completed: *self.total_completed.lock().unwrap(),
            active_count: self.active_count(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cancel_token_new() {
        let (token, _handle) = new_cancel_pair();
        assert!(!token.is_cancelled());
    }

    #[test]
    fn test_cancel_handle_cancels_token() {
        let (token, handle) = new_cancel_pair();
        handle.cancel(CancelReason::UserRequested);
        assert!(token.is_cancelled());
    }

    #[test]
    fn test_cancel_reason() {
        let (token, handle) = new_cancel_pair();
        handle.cancel(CancelReason::ClientDisconnected);
        assert_eq!(
            token.cancelled_reason(),
            Some(CancelReason::ClientDisconnected)
        );
    }

    #[test]
    fn test_cancel_reason_none_initially() {
        let (token, _handle) = new_cancel_pair();
        assert_eq!(token.cancelled_reason(), None);
    }

    #[test]
    fn test_cancel_token_clone() {
        let (token, handle) = new_cancel_pair();
        let token_clone = token.clone();
        handle.cancel(CancelReason::ServerShutdown);
        assert!(token.is_cancelled());
        assert!(token_clone.is_cancelled());
    }

    #[test]
    fn test_child_token_cancelled_by_parent() {
        let (token, handle) = new_cancel_pair();
        let (child_token, _child_handle) = token.child();
        handle.cancel(CancelReason::DeadlineExceeded);
        assert!(token.is_cancelled());
        assert!(child_token.is_cancelled());
    }

    #[test]
    fn test_child_token_cancelled_independently() {
        let (token, _handle) = new_cancel_pair();
        let (child_token, child_handle) = token.child();
        assert!(!token.is_cancelled());
        assert!(!child_token.is_cancelled());
        child_handle.cancel(CancelReason::UserRequested);
        assert!(!token.is_cancelled());
        assert!(child_token.is_cancelled());
    }

    #[test]
    fn test_parent_not_cancelled_by_child() {
        let (token, _handle) = new_cancel_pair();
        let (_child_token, child_handle) = token.child();
        child_handle.cancel(CancelReason::Superseded);
        assert!(!token.is_cancelled());
    }

    #[test]
    fn test_multiple_children() {
        let (token, handle) = new_cancel_pair();
        let (child1, _) = token.child();
        let (child2, _) = token.child();
        let (child3, _) = token.child();
        handle.cancel(CancelReason::ServerShutdown);
        assert!(token.is_cancelled());
        assert!(child1.is_cancelled());
        assert!(child2.is_cancelled());
        assert!(child3.is_cancelled());
    }

    #[test]
    fn test_registry_new() {
        let registry = CancelRegistry::new();
        assert_eq!(registry.active_count(), 0);
    }

    #[test]
    fn test_registry_register() {
        let registry = CancelRegistry::new();
        let (token, _handle) = registry.register(1);
        assert_eq!(registry.active_count(), 1);
        assert!(!token.is_cancelled());
    }

    #[test]
    fn test_registry_cancel() {
        let registry = CancelRegistry::new();
        registry.register(1);
        registry.cancel(1, CancelReason::ClientDisconnected);
        let stats = registry.stats();
        assert_eq!(stats.total_cancelled, 1);
    }

    #[test]
    fn test_registry_cancel_all() {
        let registry = CancelRegistry::new();
        registry.register(1);
        registry.register(2);
        registry.register(3);
        registry.cancel_all(CancelReason::ServerShutdown);
        let stats = registry.stats();
        assert_eq!(stats.total_cancelled, 3);
    }

    #[test]
    fn test_registry_remove() {
        let registry = CancelRegistry::new();
        registry.register(1);
        registry.register(2);
        assert_eq!(registry.active_count(), 2);
        registry.remove(1);
        assert_eq!(registry.active_count(), 1);
        let stats = registry.stats();
        assert_eq!(stats.total_completed, 1);
    }

    #[test]
    fn test_registry_stats() {
        let registry = CancelRegistry::new();
        registry.register(1);
        registry.register(2);
        registry.cancel(1, CancelReason::DeadlineExceeded);
        registry.remove(2);
        let stats = registry.stats();
        assert_eq!(stats.total_registered, 2);
        assert_eq!(stats.total_cancelled, 1);
        assert_eq!(stats.total_completed, 1);
        assert_eq!(stats.active_count, 1);
    }

    #[test]
    fn test_cancel_handle_is_cancelled() {
        let (_token, handle) = new_cancel_pair();
        assert!(!handle.is_cancelled());
        handle.cancel(CancelReason::UserRequested);
        assert!(handle.is_cancelled());
    }

    #[test]
    fn test_double_cancel() {
        let (token, handle) = new_cancel_pair();
        handle.cancel(CancelReason::UserRequested);
        handle.cancel(CancelReason::ServerShutdown);
        assert!(token.is_cancelled());
        assert_eq!(token.cancelled_reason(), Some(CancelReason::UserRequested));
    }

    #[test]
    fn test_cancel_with_each_reason() {
        let reasons = [
            CancelReason::ClientDisconnected,
            CancelReason::DeadlineExceeded,
            CancelReason::ServerShutdown,
            CancelReason::UserRequested,
            CancelReason::Superseded,
        ];

        for reason in reasons {
            let (token, handle) = new_cancel_pair();
            handle.cancel(reason);
            assert!(token.is_cancelled());
            assert_eq!(token.cancelled_reason(), Some(reason));
        }
    }
}
