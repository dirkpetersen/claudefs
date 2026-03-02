//! Flow control module for request throttling and backpressure.
//!
//! Provides sliding window and token bucket based flow control for
//! managing inflight requests and bytes to prevent overwhelming the system.
//!
//! # Architecture
//!
//! The flow control module provides two main components:
//!
//! 1. **[`FlowController`]**: Token bucket-based control for inflight requests/bytes
//! 2. **[`WindowController`]**: Sliding window for sequence-based flow control
//!
//! # Backpressure States
//!
//! - **Open**: Below high watermark, normal operation
//! - **Throttled**: Above high watermark, slowing down
//! - **Blocked**: At capacity, cannot accept more
//!
//! # Example
//!
//! ```
//! use claudefs_transport::flowcontrol::{FlowController, FlowControlConfig, FlowControlState};
//!
//! let config = FlowControlConfig::default();
/// let controller = FlowController::new(config);
/// 
/// // Try to acquire capacity for a request
/// if let Some(permit) = controller.try_acquire(1024) {
///     println!("Acquired permit for 1024 bytes");
///     println!("Inflight: {} requests, {} bytes", 
///         controller.inflight_requests(), 
///         controller.inflight_bytes());
///     
///     // Permit is automatically released when dropped
/// }
/// ```
//!
//! # See Also
//! - [`FlowControlConfig`] - Configuration options
//! - [`FlowPermit`] - RAII guard for acquired capacity
/// - [`FlowControlState`] - Backpressure states

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

/// Configuration for flow control limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowControlConfig {
    /// Maximum number of inflight requests.
    pub max_inflight_requests: u32,
    /// Maximum inflight bytes (default 64MB).
    pub max_inflight_bytes: u64,
    /// Sliding window size for flow control.
    pub window_size: u32,
    /// High watermark percentage (0-100) - start backpressure.
    pub high_watermark_pct: u8,
    /// Low watermark percentage (0-100) - release backpressure.
    pub low_watermark_pct: u8,
}

impl Default for FlowControlConfig {
    fn default() -> Self {
        Self {
            max_inflight_requests: 1024,
            max_inflight_bytes: 64 * 1024 * 1024, // 64 MB
            window_size: 256,
            high_watermark_pct: 80,
            low_watermark_pct: 50,
        }
    }
}

/// State of the flow controller indicating backpressure level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlowControlState {
    /// System is accepting new requests normally.
    Open,
    /// System is slowing down (between high watermark and max).
    Throttled,
    /// System is not accepting new requests (at maximum capacity).
    Blocked,
}

/// Inner state for FlowController that can be shared with FlowPermit
struct FlowControllerInner {
    inflight_requests: AtomicU32,
    inflight_bytes: AtomicU64,
}

/// Flow controller for managing inflight requests and bytes.
///
/// Uses atomic counters for thread-safe operation without locking
/// on the hot path.
pub struct FlowController {
    config: FlowControlConfig,
    inner: Arc<FlowControllerInner>,
}

impl FlowController {
    /// Creates a new flow controller with the given configuration.
    pub fn new(config: FlowControlConfig) -> Self {
        Self {
            config: config.clone(),
            inner: Arc::new(FlowControllerInner {
                inflight_requests: AtomicU32::new(0),
                inflight_bytes: AtomicU64::new(0),
            }),
        }
    }

    /// Attempts to acquire capacity for a request without blocking.
    ///
    /// Returns `Some(FlowPermit)` if capacity is available, `None` otherwise.
    /// The permit must be held for the duration of the request.
    pub fn try_acquire(&self, bytes: u64) -> Option<FlowPermit> {
        let requests = self.inner.inflight_requests.load(Ordering::Acquire);
        let bytes_val = self.inner.inflight_bytes.load(Ordering::Acquire);

        // Check request limit
        if requests >= self.config.max_inflight_requests {
            return None;
        }

        // Check byte limit
        if bytes_val.saturating_add(bytes) > self.config.max_inflight_bytes {
            return None;
        }

        // Try to acquire
        match self.inner.inflight_requests.compare_exchange(
            requests,
            requests + 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                self.inner.inflight_bytes.fetch_add(bytes, Ordering::AcqRel);
                Some(FlowPermit {
                    inner: Arc::clone(&self.inner),
                    bytes,
                })
            }
            Err(_) => None,
        }
    }

    /// Returns the current flow control state.
    pub fn state(&self) -> FlowControlState {
        let requests = self.inner.inflight_requests.load(Ordering::Acquire);
        let bytes = self.inner.inflight_bytes.load(Ordering::Acquire);

        let request_pct =
            (requests as f64 / self.config.max_inflight_requests as f64 * 100.0) as u8;
        let byte_pct = (bytes as f64 / self.config.max_inflight_bytes as f64 * 100.0) as u8;

        let max_pct = request_pct.max(byte_pct);

        if max_pct >= 100 {
            FlowControlState::Blocked
        } else if max_pct >= self.config.high_watermark_pct {
            FlowControlState::Throttled
        } else {
            FlowControlState::Open
        }
    }

    /// Returns the current number of inflight requests.
    pub fn inflight_requests(&self) -> u32 {
        self.inner.inflight_requests.load(Ordering::Acquire)
    }

    /// Returns the current inflight bytes.
    pub fn inflight_bytes(&self) -> u64 {
        self.inner.inflight_bytes.load(Ordering::Acquire)
    }

    /// Releases capacity back to the flow controller.
    pub fn release(&self, bytes: u64) {
        self.inner.inflight_requests.fetch_sub(1, Ordering::AcqRel);
        self.inner.inflight_bytes.fetch_sub(bytes, Ordering::AcqRel);
    }

    /// Returns a reference to the configuration.
    pub fn config(&self) -> &FlowControlConfig {
        &self.config
    }
}

impl Default for FlowController {
    fn default() -> Self {
        Self::new(FlowControlConfig::default())
    }
}

/// RAII guard for flow control permits.
///
/// Automatically releases capacity when dropped.
pub struct FlowPermit {
    inner: Arc<FlowControllerInner>,
    bytes: u64,
}

impl FlowPermit {
    /// Returns the number of bytes this permit represents.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}

impl Drop for FlowPermit {
    fn drop(&mut self) {
        self.inner.inflight_requests.fetch_sub(1, Ordering::AcqRel);
        self.inner
            .inflight_bytes
            .fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

/// Sliding window flow controller for sequence-based flow control.
///
/// Tracks sent and acknowledged sequences to manage the window.
pub struct WindowController {
    window_size: u32,
    window_start: AtomicU64,
    window_end: AtomicU64,
    sent: AtomicU64,
}

impl WindowController {
    /// Creates a new window controller with the given window size.
    pub fn new(window_size: u32) -> Self {
        Self {
            window_size,
            window_start: AtomicU64::new(0),
            window_end: AtomicU64::new(window_size as u64),
            sent: AtomicU64::new(0),
        }
    }

    /// Advances the window by the given sequence number.
    ///
    /// Returns `true` if the sequence is within the window, `false` otherwise.
    pub fn advance(&self, sequence: u64) -> bool {
        let start = self.window_start.load(Ordering::Acquire);
        let end = start.saturating_add(self.window_size as u64);

        // Check if sequence is within the window [start, end)
        if sequence < start || sequence >= end {
            return false;
        }

        // Check if we have room in the window
        let current_sent = self.sent.load(Ordering::Acquire);
        if current_sent.saturating_sub(start) >= self.window_size as u64 {
            return false; // Window is full
        }

        // Try to increment sent count
        let new_sent = current_sent + 1;
        self.sent
            .compare_exchange(current_sent, new_sent, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Returns whether the window can accept new sends.
    pub fn can_send(&self) -> bool {
        let start = self.window_start.load(Ordering::Acquire);
        let sent = self.sent.load(Ordering::Acquire);
        sent.saturating_sub(start) < self.window_size as u64
    }

    /// Acknowledges a sequence number, sliding the window forward.
    pub fn ack(&self, sequence: u64) {
        let mut start = self.window_start.load(Ordering::Acquire);

        // Advance window start to the lowest unacknowledged sequence
        while start <= sequence {
            match self.window_start.compare_exchange(
                start,
                start + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    // Successfully advanced to start + 1
                    // Continue loop to advance further if needed
                    start += 1;
                }
                Err(new_start) => start = new_start,
            }
        }
    }

    /// Returns the current window start sequence.
    pub fn window_start(&self) -> u64 {
        self.window_start.load(Ordering::Acquire)
    }

    /// Returns the current window end sequence.
    pub fn window_end(&self) -> u64 {
        self.window_end.load(Ordering::Acquire)
    }

    /// Returns the window size.
    pub fn window_size(&self) -> u32 {
        self.window_size
    }

    /// Returns the number of slots currently in use.
    pub fn in_flight(&self) -> u32 {
        let start = self.window_start.load(Ordering::Acquire);
        let sent = self.sent.load(Ordering::Acquire);
        sent.saturating_sub(start) as u32
    }
}

impl Default for WindowController {
    fn default() -> Self {
        Self::new(256)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn flow_control_basic() {
        let config = FlowControlConfig::default();
        let controller = FlowController::new(config);

        // Should be able to acquire
        let permit = controller.try_acquire(1024);
        assert!(permit.is_some());

        let permit = permit.unwrap();
        assert_eq!(permit.bytes(), 1024);

        // Check inflight counts
        assert_eq!(controller.inflight_requests(), 1);
        assert_eq!(controller.inflight_bytes(), 1024);

        // Release on drop
        drop(permit);

        assert_eq!(controller.inflight_requests(), 0);
        assert_eq!(controller.inflight_bytes(), 0);
    }

    #[test]
    fn flow_control_limits() {
        let mut config = FlowControlConfig::default();
        config.max_inflight_requests = 2;
        config.max_inflight_bytes = 2048;
        let controller = FlowController::new(config);

        // Acquire up to the limit
        let p1 = controller.try_acquire(1024);
        assert!(p1.is_some());

        let p2 = controller.try_acquire(1024);
        assert!(p2.is_some());

        // Should be at limit
        assert_eq!(controller.inflight_requests(), 2);
        assert_eq!(controller.inflight_bytes(), 2048);

        // Should fail to acquire more
        let p3 = controller.try_acquire(1);
        assert!(p3.is_none());

        // Even with smaller size should fail (bytes limit)
        let p4 = controller.try_acquire(1);
        assert!(p4.is_none());

        // Release one and try again
        drop(p1);
        assert_eq!(controller.inflight_requests(), 1);
        assert_eq!(controller.inflight_bytes(), 1024);

        let p5 = controller.try_acquire(1024);
        assert!(p5.is_some());

        drop(p2);
        drop(p5);
    }

    #[test]
    fn backpressure_states() {
        let mut config = FlowControlConfig::default();
        config.max_inflight_requests = 10;
        config.max_inflight_bytes = 1000;
        config.high_watermark_pct = 80;
        config.low_watermark_pct = 50;
        let controller = FlowController::new(config);

        // Initially Open
        assert_eq!(controller.state(), FlowControlState::Open);

        let mut permits = Vec::new();

        // Fill to 50% - should still be Open
        for _ in 0..5 {
            permits.push(controller.try_acquire(100).unwrap());
        }
        assert_eq!(controller.state(), FlowControlState::Open);

        // Fill to 80% - should be Throttled
        for _ in 0..3 {
            permits.push(controller.try_acquire(100).unwrap());
        }
        assert_eq!(controller.state(), FlowControlState::Throttled);

        // Fill to 100% - should be Blocked
        for _ in 0..2 {
            permits.push(controller.try_acquire(100).unwrap());
        }
        assert_eq!(controller.state(), FlowControlState::Blocked);
    }

    #[test]
    fn permit_drop() {
        let config = FlowControlConfig::default();
        let controller = FlowController::new(config);

        {
            let _permit = controller.try_acquire(5000).unwrap();
            assert_eq!(controller.inflight_requests(), 1);
            assert_eq!(controller.inflight_bytes(), 5000);
        } // permit dropped here

        assert_eq!(controller.inflight_requests(), 0);
        assert_eq!(controller.inflight_bytes(), 0);
    }

    #[test]
    fn window_basic() {
        let controller = WindowController::new(10);

        assert!(controller.can_send());
        assert_eq!(controller.window_start(), 0);
        assert_eq!(controller.window_end(), 10);
        assert_eq!(controller.in_flight(), 0);
    }

    #[test]
    fn window_advance() {
        let controller = WindowController::new(5);

        // Advance within window
        assert!(controller.advance(0));
        assert!(controller.advance(1));
        assert!(controller.advance(2));

        assert_eq!(controller.in_flight(), 3);

        // Try to advance beyond window end - should fail
        assert!(!controller.advance(10));
    }

    #[test]
    fn window_ack() {
        let controller = WindowController::new(5);

        // Send some sequences
        controller.advance(0);
        controller.advance(1);
        controller.advance(2);

        assert_eq!(controller.in_flight(), 3);

        // Acknowledge sequence 0
        controller.ack(0);
        assert_eq!(controller.window_start(), 1);
        assert_eq!(controller.in_flight(), 2);

        // Acknowledge remaining
        controller.ack(1);
        controller.ack(2);
        assert_eq!(controller.window_start(), 3);
        assert_eq!(controller.in_flight(), 0);
    }

    #[test]
    fn concurrent_flow() {
        let config = FlowControlConfig {
            max_inflight_requests: 100,
            max_inflight_bytes: 100_000,
            ..Default::default()
        };
        let controller = Arc::new(FlowController::new(config));

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let controller = controller.clone();
                thread::spawn(move || {
                    for _ in 0..100 {
                        loop {
                            if let Some(permit) = controller.try_acquire(100) {
                                // Simulate some work
                                thread::yield_now();
                                drop(permit);
                                break;
                            }
                            thread::yield_now();
                        }
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        // Should be back to zero after all permits released
        assert_eq!(controller.inflight_requests(), 0);
        assert_eq!(controller.inflight_bytes(), 0);
    }

    #[test]
    fn config_default() {
        let config = FlowControlConfig::default();

        assert_eq!(config.max_inflight_requests, 1024);
        assert_eq!(config.max_inflight_bytes, 64 * 1024 * 1024);
        assert_eq!(config.window_size, 256);
        assert_eq!(config.high_watermark_pct, 80);
        assert_eq!(config.low_watermark_pct, 50);
    }

    #[test]
    fn high_low_watermarks() {
        let mut config = FlowControlConfig::default();
        config.max_inflight_requests = 100;
        config.max_inflight_bytes = 1000;
        config.high_watermark_pct = 60;
        config.low_watermark_pct = 30;
        let controller = FlowController::new(config);

        let mut permits = Vec::new();

        // At 0-59%: Open
        for _ in 0..30 {
            permits.push(controller.try_acquire(1).unwrap());
        }
        assert_eq!(controller.state(), FlowControlState::Open);

        for _ in 0..29 {
            permits.push(controller.try_acquire(1).unwrap());
        }
        assert_eq!(controller.state(), FlowControlState::Open);

        // At 60%+: Throttled
        for _ in 0..1 {
            permits.push(controller.try_acquire(1).unwrap());
        }
        assert_eq!(controller.state(), FlowControlState::Throttled);

        // Release to 59% - back to Open (below high watermark)
        for _ in 0..1 {
            let p = permits.remove(0);
            drop(p);
        }
        assert_eq!(controller.state(), FlowControlState::Open);

        // Acquire back to 60%+ - Throttled again
        for _ in 0..1 {
            permits.push(controller.try_acquire(1).unwrap());
        }
        assert_eq!(controller.state(), FlowControlState::Throttled);

        // Release all - Open
        permits.clear();
        assert_eq!(controller.state(), FlowControlState::Open);
    }

    #[test]
    fn flow_control_state_serialization() {
        let open = FlowControlState::Open;
        let throttled = FlowControlState::Throttled;
        let blocked = FlowControlState::Blocked;

        // Test serialization
        let open_json = serde_json::to_string(&open).unwrap();
        let throttled_json = serde_json::to_string(&throttled).unwrap();
        let blocked_json = serde_json::to_string(&blocked).unwrap();

        assert_eq!(open_json, "\"Open\"");
        assert_eq!(throttled_json, "\"Throttled\"");
        assert_eq!(blocked_json, "\"Blocked\"");

        // Test deserialization
        let open_back: FlowControlState = serde_json::from_str(&open_json).unwrap();
        let throttled_back: FlowControlState = serde_json::from_str(&throttled_json).unwrap();
        let blocked_back: FlowControlState = serde_json::from_str(&blocked_json).unwrap();

        assert_eq!(open, open_back);
        assert_eq!(throttled, throttled_back);
        assert_eq!(blocked, blocked_back);
    }

    #[test]
    fn config_serialization() {
        let config = FlowControlConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let config_back: FlowControlConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(
            config.max_inflight_requests,
            config_back.max_inflight_requests
        );
        assert_eq!(config.max_inflight_bytes, config_back.max_inflight_bytes);
        assert_eq!(config.window_size, config_back.window_size);
        assert_eq!(config.high_watermark_pct, config_back.high_watermark_pct);
        assert_eq!(config.low_watermark_pct, config_back.low_watermark_pct);
    }

    #[test]
    fn window_controller_race() {
        let controller = Arc::new(WindowController::new(100));

        let controller_clone = controller.clone();
        let sender = thread::spawn(move || {
            for i in 0..1000 {
                while !controller_clone.can_send() {
                    thread::yield_now();
                }
                controller_clone.advance(i);
            }
        });

        let controller_clone = controller.clone();
        let acker = thread::spawn(move || {
            let mut seq = 0;
            for _ in 0..1000 {
                controller_clone.ack(seq);
                seq += 1;
            }
        });

        sender.join().unwrap();
        acker.join().unwrap();

        assert_eq!(controller.window_start(), 1000);
    }

    #[test]
    fn flow_controller_multiple_release() {
        let config = FlowControlConfig::default();
        let controller = FlowController::new(config);

        // Acquire and manually release multiple times
        let _permit1 = controller.try_acquire(100).unwrap();
        let permit2 = controller.try_acquire(200).unwrap();
        let permit3 = controller.try_acquire(300).unwrap();

        assert_eq!(controller.inflight_requests(), 3);
        assert_eq!(controller.inflight_bytes(), 600);

        // Manual release
        controller.release(100);
        assert_eq!(controller.inflight_requests(), 2);
        assert_eq!(controller.inflight_bytes(), 500);

        // Drop remaining permits
        drop(permit2);
        assert_eq!(controller.inflight_requests(), 1);
        assert_eq!(controller.inflight_bytes(), 300);

        drop(permit3);
        assert_eq!(controller.inflight_requests(), 0);
        assert_eq!(controller.inflight_bytes(), 0);
    }

    #[test]
    fn zero_byte_permit() {
        let config = FlowControlConfig::default();
        let controller = FlowController::new(config);

        // Acquire with 0 bytes
        let permit = controller.try_acquire(0);
        assert!(permit.is_some());

        assert_eq!(controller.inflight_requests(), 1);
        assert_eq!(controller.inflight_bytes(), 0);

        drop(permit);
        assert_eq!(controller.inflight_requests(), 0);
        assert_eq!(controller.inflight_bytes(), 0);
    }

    #[test]
    fn window_sliding() {
        let controller = WindowController::new(3);

        // Fill window
        assert!(controller.advance(0));
        assert!(controller.advance(1));
        assert!(controller.advance(2));

        assert!(!controller.can_send()); // Window full

        // Acknowledge first two
        controller.ack(0);
        controller.ack(1);

        assert!(controller.can_send()); // Should have room now

        // Can send more
        assert!(controller.advance(3));
        assert!(controller.advance(4));

        assert!(!controller.can_send()); // Window full again
    }
}
