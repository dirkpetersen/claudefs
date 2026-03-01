//! Deadline/timeout propagation for distributed RPC operations.
//!
//! This module provides utilities for propagating deadlines through a call chain,
//! allowing downstream services to detect expired deadlines early and avoid wasted work.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::TransportError;

/// Absolute deadline as milliseconds since UNIX epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Deadline {
    expiry_ms: u64,
}

impl Deadline {
    /// Creates a new deadline from now + timeout.
    pub fn new(timeout: Duration) -> Self {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let expiry_ms = now_ms.saturating_add(timeout.as_millis() as u64);
        Self::from_epoch_ms(expiry_ms)
    }

    /// Creates a deadline from milliseconds since UNIX epoch.
    pub fn from_epoch_ms(ms: u64) -> Self {
        Self { expiry_ms: ms }
    }

    /// Returns the remaining time until the deadline expires.
    ///
    /// Returns `None` if the deadline has already expired.
    pub fn remaining(&self) -> Option<Duration> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        if self.expiry_ms > now_ms {
            Some(Duration::from_millis(self.expiry_ms.saturating_sub(now_ms)))
        } else {
            None
        }
    }

    /// Returns `true` if the deadline has expired.
    pub fn is_expired(&self) -> bool {
        self.remaining().is_none()
    }

    /// Returns the expiry time as milliseconds since UNIX epoch.
    pub fn expiry_ms(&self) -> u64 {
        self.expiry_ms
    }
}

/// Context that holds an optional deadline for RPC operations.
///
/// Used to propagate deadline information through the call chain.
#[derive(Debug, Clone, Default)]
pub struct DeadlineContext {
    deadline: Option<Deadline>,
}

impl DeadlineContext {
    /// Creates a new context with no deadline.
    pub fn new() -> Self {
        Self { deadline: None }
    }

    /// Creates a new context with the given deadline.
    pub fn with_deadline(deadline: Deadline) -> Self {
        Self {
            deadline: Some(deadline),
        }
    }

    /// Creates a new context with a deadline set to now + timeout.
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            deadline: Some(Deadline::new(timeout)),
        }
    }

    /// Returns the deadline, if one is set.
    pub fn deadline(&self) -> Option<&Deadline> {
        self.deadline.as_ref()
    }

    /// Returns `true` if a deadline is set and has expired.
    ///
    /// Returns `false` if no deadline is set.
    pub fn is_expired(&self) -> bool {
        self.deadline
            .as_ref()
            .map(|d| d.is_expired())
            .unwrap_or(false)
    }

    /// Returns the remaining time until the deadline expires.
    ///
    /// Returns `None` if no deadline is set or if the deadline has expired.
    pub fn remaining(&self) -> Option<Duration> {
        self.deadline.as_ref().and_then(|d| d.remaining())
    }

    /// Checks if the deadline has expired.
    ///
    /// Returns `Ok(())` if no deadline is set or if the deadline is still valid.
    /// Returns `Err(RequestTimeout)` if the deadline has expired.
    pub fn check(&self) -> Result<(), TransportError> {
        if let Some(deadline) = &self.deadline {
            if deadline.is_expired() {
                let timeout_ms = deadline.expiry_ms();
                let now_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                let elapsed = timeout_ms.saturating_sub(now_ms);
                return Err(TransportError::RequestTimeout {
                    request_id: 0,
                    timeout_ms: elapsed,
                });
            }
        }
        Ok(())
    }
}

/// Encodes a deadline context as 8-byte big-endian u64.
///
/// Returns 0 if no deadline is set, otherwise returns the expiry time in ms.
pub fn encode_deadline(ctx: &DeadlineContext) -> [u8; 8] {
    let expiry = ctx.deadline.map(|d| d.expiry_ms()).unwrap_or(0);
    expiry.to_be_bytes()
}

/// Decodes a deadline context from 8-byte big-endian u64.
///
/// Returns `DeadlineContext::new()` (no deadline) if the value is 0,
/// otherwise returns a context with the deadline set to the expiry time.
pub fn decode_deadline(bytes: &[u8; 8]) -> DeadlineContext {
    let expiry = u64::from_be_bytes(*bytes);
    if expiry == 0 {
        DeadlineContext::new()
    } else {
        DeadlineContext::with_deadline(Deadline::from_epoch_ms(expiry))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::sleep;

    #[test]
    fn test_deadline_new() {
        let deadline = Deadline::new(Duration::from_secs(5));
        assert!(!deadline.is_expired());
        assert!(deadline.remaining().is_some());
        assert!(deadline.remaining().unwrap() > Duration::from_secs(4));
    }

    #[test]
    fn test_deadline_expired() {
        let deadline = Deadline::new(Duration::from_millis(0));
        assert!(deadline.is_expired());
        assert!(deadline.remaining().is_none());
    }

    #[tokio::test]
    async fn test_deadline_remaining() {
        let deadline = Deadline::new(Duration::from_millis(100));
        let remaining1 = deadline.remaining().unwrap();
        sleep(Duration::from_millis(10)).await;
        let remaining2 = deadline.remaining().unwrap();
        assert!(remaining2 < remaining1);
    }

    #[test]
    fn test_deadline_context_no_deadline() {
        let ctx = DeadlineContext::new();
        assert!(ctx.deadline().is_none());
        assert!(!ctx.is_expired());
    }

    #[test]
    fn test_deadline_context_with_timeout() {
        let ctx = DeadlineContext::with_timeout(Duration::from_secs(1));
        assert!(ctx.deadline().is_some());
        assert!(!ctx.is_expired());
    }

    #[test]
    fn test_deadline_context_check_ok() {
        let ctx = DeadlineContext::with_timeout(Duration::from_secs(5));
        assert!(ctx.check().is_ok());
    }

    #[test]
    fn test_deadline_context_check_expired() {
        let ctx = DeadlineContext::with_timeout(Duration::from_millis(0));
        assert!(ctx.check().is_err());
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let ctx = DeadlineContext::with_timeout(Duration::from_secs(10));
        let encoded = encode_deadline(&ctx);
        let decoded = decode_deadline(&encoded);
        assert_eq!(ctx.deadline().map(|d| d.expiry_ms()), decoded.deadline().map(|d| d.expiry_ms()));
    }

    #[test]
    fn test_encode_decode_no_deadline() {
        let ctx = DeadlineContext::new();
        let encoded = encode_deadline(&ctx);
        let decoded = decode_deadline(&encoded);
        assert!(decoded.deadline().is_none());
    }
}