//! Wire-level diagnostics for ClaudeFS transport connections.
//!
//! Provides ping/pong RTT measurement, rolling latency statistics, and path tracing
//! for health checks and cluster observability dashboards.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Configuration for wire diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireDiagConfig {
    /// Window size for rolling RTT statistics (default: 128).
    pub window_size: usize,
    /// Timeout after which a ping is considered lost (milliseconds, default: 5000).
    pub ping_timeout_ms: u64,
    /// Maximum number of concurrent in-flight pings (default: 8).
    pub max_inflight: usize,
}

impl Default for WireDiagConfig {
    fn default() -> Self {
        Self {
            window_size: 128,
            ping_timeout_ms: 5000,
            max_inflight: 8,
        }
    }
}

/// A single RTT sample.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RttSample {
    /// Sequence number of the ping.
    pub seq: u64,
    /// Round-trip time in microseconds.
    pub rtt_us: u64,
    /// Timestamp of the ping send (ms since epoch).
    pub sent_at_ms: u64,
}

/// Rolling RTT statistics over the last `window_size` samples.
#[derive(Debug, Clone)]
pub struct RttSeries {
    samples: VecDeque<RttSample>,
    window_size: usize,
}

impl RttSeries {
    /// Create a new RttSeries with the given window size.
    pub fn new(window_size: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(window_size),
            window_size,
        }
    }

    /// Push a new sample (drops oldest if window is full).
    pub fn push(&mut self, sample: RttSample) {
        if self.samples.len() >= self.window_size {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    /// Minimum RTT in the window (None if empty).
    pub fn min_us(&self) -> Option<u64> {
        self.samples.iter().map(|s| s.rtt_us).min()
    }

    /// Maximum RTT in the window (None if empty).
    pub fn max_us(&self) -> Option<u64> {
        self.samples.iter().map(|s| s.rtt_us).max()
    }

    /// Mean RTT in the window (None if empty).
    pub fn mean_us(&self) -> Option<u64> {
        if self.samples.is_empty() {
            return None;
        }
        let sum: u64 = self.samples.iter().map(|s| s.rtt_us).sum();
        Some(sum / self.samples.len() as u64)
    }

    /// p99 RTT in the window (None if < 100 samples; sort-based ok).
    pub fn p99_us(&self) -> Option<u64> {
        if self.samples.len() < 100 {
            return None;
        }
        let mut values: Vec<u64> = self.samples.iter().map(|s| s.rtt_us).collect();
        values.sort();
        let idx = (values.len() as f64 * 0.99) as usize;
        let idx = idx.min(values.len() - 1);
        Some(values[idx])
    }

    /// Number of samples in the window.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Returns true if the window is empty.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

/// A hop entry in a trace path (one intermediate node in a multi-hop RPC path).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceHop {
    /// Node identifier (opaque bytes, typically 16-byte UUID).
    pub node_id: [u8; 16],
    /// Cumulative RTT to this hop in microseconds.
    pub cumulative_rtt_us: u64,
    /// Incremental latency added by this hop in microseconds.
    pub hop_latency_us: u64,
}

/// Result of a trace-path operation (like traceroute but for ClaudeFS RPC).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracePath {
    pub hops: Vec<TraceHop>,
    /// Total RTT for the full path in microseconds.
    pub total_rtt_us: u64,
    /// Whether the path completed successfully (destination responded).
    pub complete: bool,
}

/// An in-flight ping record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InFlightPing {
    pub seq: u64,
    pub sent_at_ms: u64,
    pub timeout_ms: u64,
}

/// Atomic stats for wire diagnostics.
pub struct WireDiagStats {
    pub pings_sent: AtomicU64,
    pub pongs_received: AtomicU64,
    pub pings_timed_out: AtomicU64,
    pub pings_rejected: AtomicU64,
}

impl WireDiagStats {
    fn new() -> Self {
        Self {
            pings_sent: AtomicU64::new(0),
            pongs_received: AtomicU64::new(0),
            pings_timed_out: AtomicU64::new(0),
            pings_rejected: AtomicU64::new(0),
        }
    }

    pub fn snapshot(&self) -> WireDiagStatsSnapshot {
        WireDiagStatsSnapshot {
            pings_sent: self.pings_sent.load(Ordering::Relaxed),
            pongs_received: self.pongs_received.load(Ordering::Relaxed),
            pings_timed_out: self.pings_timed_out.load(Ordering::Relaxed),
            pings_rejected: self.pings_rejected.load(Ordering::Relaxed),
        }
    }
}

/// Snapshot of wire diagnostics stats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireDiagStatsSnapshot {
    pub pings_sent: u64,
    pub pongs_received: u64,
    pub pings_timed_out: u64,
    pub pings_rejected: u64,
}

/// Immutable snapshot of RTT series statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RttSeriesSnapshot {
    pub sample_count: usize,
    pub min_us: Option<u64>,
    pub max_us: Option<u64>,
    pub mean_us: Option<u64>,
    pub p99_us: Option<u64>,
}

/// Wire diagnostics manager.
pub struct WireDiag {
    config: WireDiagConfig,
    next_seq: AtomicU64,
    inflight: Mutex<HashMap<u64, InFlightPing>>,
    rtt_series: Mutex<RttSeries>,
    stats: Arc<WireDiagStats>,
}

use std::collections::HashMap;

impl WireDiag {
    pub fn new(config: WireDiagConfig) -> Self {
        let rtt_series = Mutex::new(RttSeries::new(config.window_size));
        Self {
            config,
            next_seq: AtomicU64::new(0),
            inflight: Mutex::new(HashMap::new()),
            rtt_series,
            stats: Arc::new(WireDiagStats::new()),
        }
    }

    /// Create a new ping request — returns the ping sequence number to include in the wire message.
    /// Returns None if max_inflight is already reached.
    pub fn send_ping(&self, now_ms: u64) -> Option<u64> {
        let mut inflight = self.inflight.lock().ok()?;
        if inflight.len() >= self.config.max_inflight {
            self.stats.pings_rejected.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        inflight.insert(
            seq,
            InFlightPing {
                seq,
                sent_at_ms: now_ms,
                timeout_ms: self.config.ping_timeout_ms,
            },
        );
        self.stats.pings_sent.fetch_add(1, Ordering::Relaxed);
        Some(seq)
    }

    /// Record a received pong response. Updates RTT series and stats.
    /// Returns the RTT in microseconds, or None if the seq was not in-flight.
    pub fn receive_pong(&self, seq: u64, now_ms: u64) -> Option<u64> {
        let mut inflight = self.inflight.lock().ok()?;
        let ping = inflight.remove(&seq)?;
        let rtt_us = now_ms.saturating_sub(ping.sent_at_ms) * 1000;
        {
            let mut series = self.rtt_series.lock().ok()?;
            series.push(RttSample {
                seq,
                rtt_us,
                sent_at_ms: ping.sent_at_ms,
            });
        }
        self.stats.pongs_received.fetch_add(1, Ordering::Relaxed);
        Some(rtt_us)
    }

    /// Expire timed-out in-flight pings. Returns count of timed-out pings.
    pub fn expire_timeouts(&self, now_ms: u64) -> u64 {
        let mut inflight = match self.inflight.lock() {
            Ok(g) => g,
            Err(_) => return 0,
        };
        let mut expired = 0u64;
        inflight.retain(|_, ping| {
            let elapsed = now_ms.saturating_sub(ping.sent_at_ms);
            if elapsed > ping.timeout_ms {
                expired += 1;
                false
            } else {
                true
            }
        });
        self.stats
            .pings_timed_out
            .fetch_add(expired, Ordering::Relaxed);
        expired
    }

    /// Current count of in-flight pings.
    pub fn inflight_count(&self) -> usize {
        self.inflight.lock().map(|g| g.len()).unwrap_or(0)
    }

    /// Take a snapshot of the current RTT series statistics.
    pub fn rtt_snapshot(&self) -> RttSeriesSnapshot {
        self.rtt_series
            .lock()
            .map(|series| RttSeriesSnapshot {
                sample_count: series.len(),
                min_us: series.min_us(),
                max_us: series.max_us(),
                mean_us: series.mean_us(),
                p99_us: series.p99_us(),
            })
            .unwrap_or(RttSeriesSnapshot {
                sample_count: 0,
                min_us: None,
                max_us: None,
                mean_us: None,
                p99_us: None,
            })
    }

    /// Get the stats arc.
    pub fn stats(&self) -> Arc<WireDiagStats> {
        Arc::clone(&self.stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_default_config() {
        let config = WireDiagConfig::default();
        assert_eq!(config.window_size, 128);
        assert_eq!(config.ping_timeout_ms, 5000);
        assert_eq!(config.max_inflight, 8);
    }

    #[test]
    fn test_send_ping_basic() {
        let diag = WireDiag::new(WireDiagConfig::default());
        let seq = diag.send_ping(1000);
        assert!(seq.is_some());
        assert_eq!(seq.unwrap(), 0);
        assert_eq!(diag.inflight_count(), 1);
    }

    #[test]
    fn test_send_ping_max_inflight() {
        let config = WireDiagConfig {
            max_inflight: 3,
            ..Default::default()
        };
        let diag = WireDiag::new(config);
        assert!(diag.send_ping(0).is_some());
        assert!(diag.send_ping(0).is_some());
        assert!(diag.send_ping(0).is_some());
        assert!(diag.send_ping(0).is_none());
    }

    #[test]
    fn test_receive_pong_records_rtt() {
        let diag = WireDiag::new(WireDiagConfig::default());
        let seq = diag.send_ping(1000).unwrap();
        let rtt = diag.receive_pong(seq, 1005);
        assert!(rtt.is_some());
        assert!(rtt.unwrap() > 0);
        assert_eq!(diag.inflight_count(), 0);
    }

    #[test]
    fn test_receive_pong_unknown_seq() {
        let diag = WireDiag::new(WireDiagConfig::default());
        let result = diag.receive_pong(999, 1000);
        assert!(result.is_none());
    }

    #[test]
    fn test_expire_timeouts_removes_stale() {
        let config = WireDiagConfig {
            ping_timeout_ms: 100,
            ..Default::default()
        };
        let diag = WireDiag::new(config);
        diag.send_ping(0);
        let expired = diag.expire_timeouts(200);
        assert_eq!(expired, 1);
        assert_eq!(diag.inflight_count(), 0);
    }

    #[test]
    fn test_expire_timeouts_keeps_fresh() {
        let config = WireDiagConfig {
            ping_timeout_ms: 500,
            ..Default::default()
        };
        let diag = WireDiag::new(config);
        diag.send_ping(1000);
        let expired = diag.expire_timeouts(1100);
        assert_eq!(expired, 0);
        assert_eq!(diag.inflight_count(), 1);
    }

    #[test]
    fn test_rtt_series_push_and_stats() {
        let mut series = RttSeries::new(10);
        for i in 0..10u64 {
            series.push(RttSample {
                seq: i,
                rtt_us: 100 + i * 10,
                sent_at_ms: i,
            });
        }
        assert_eq!(series.len(), 10);
        assert_eq!(series.min_us(), Some(100));
        assert_eq!(series.max_us(), Some(190));
        assert!(series.mean_us().is_some());
        let mean = series.mean_us().unwrap();
        assert!(mean >= 100 && mean <= 200);
    }

    #[test]
    fn test_rtt_series_window_eviction() {
        let mut series = RttSeries::new(5);
        for i in 0..6u64 {
            series.push(RttSample {
                seq: i,
                rtt_us: i,
                sent_at_ms: i,
            });
        }
        assert_eq!(series.len(), 5);
        assert_eq!(series.min_us(), Some(1));
        assert_eq!(series.max_us(), Some(5));
    }

    #[test]
    fn test_rtt_series_p99_needs_100() {
        let mut series = RttSeries::new(200);
        for i in 0..99u64 {
            series.push(RttSample {
                seq: i,
                rtt_us: 100 + i,
                sent_at_ms: i,
            });
        }
        assert!(series.p99_us().is_none());
        series.push(RttSample {
            seq: 99,
            rtt_us: 200,
            sent_at_ms: 99,
        });
        assert!(series.p99_us().is_some());
    }

    #[test]
    fn test_rtt_series_empty() {
        let series = RttSeries::new(10);
        assert!(series.is_empty());
        assert!(series.min_us().is_none());
        assert!(series.max_us().is_none());
        assert!(series.mean_us().is_none());
        assert!(series.p99_us().is_none());
    }

    #[test]
    fn test_stats_snapshot() {
        let diag = WireDiag::new(WireDiagConfig {
            ping_timeout_ms: 10,
            ..Default::default()
        });
        let _ = diag.send_ping(0);
        let _ = diag.send_ping(0);
        let _ = diag.send_ping(0);
        let _ = diag.receive_pong(0, 5);
        let _ = diag.receive_pong(1, 5);
        diag.expire_timeouts(100);
        let snap = diag.stats().snapshot();
        assert_eq!(snap.pings_sent, 3);
        assert_eq!(snap.pongs_received, 2);
        assert_eq!(snap.pings_timed_out, 1);
    }

    #[test]
    fn test_trace_path_complete() {
        let path = TracePath {
            hops: vec![
                TraceHop {
                    node_id: [1; 16],
                    cumulative_rtt_us: 100,
                    hop_latency_us: 100,
                },
                TraceHop {
                    node_id: [2; 16],
                    cumulative_rtt_us: 200,
                    hop_latency_us: 100,
                },
                TraceHop {
                    node_id: [3; 16],
                    cumulative_rtt_us: 350,
                    hop_latency_us: 150,
                },
            ],
            total_rtt_us: 350,
            complete: true,
        };
        assert_eq!(path.hops.len(), 3);
        assert!(path.complete);
    }

    #[test]
    fn test_trace_path_incomplete() {
        let path = TracePath {
            hops: vec![
                TraceHop {
                    node_id: [1; 16],
                    cumulative_rtt_us: 100,
                    hop_latency_us: 100,
                },
                TraceHop {
                    node_id: [2; 16],
                    cumulative_rtt_us: 200,
                    hop_latency_us: 100,
                },
            ],
            total_rtt_us: 200,
            complete: false,
        };
        assert_eq!(path.hops.len(), 2);
        assert!(!path.complete);
    }

    #[test]
    fn test_rtt_sample_ordering() {
        let mut series = RttSeries::new(10);
        series.push(RttSample {
            seq: 0,
            rtt_us: 500,
            sent_at_ms: 0,
        });
        series.push(RttSample {
            seq: 1,
            rtt_us: 100,
            sent_at_ms: 1,
        });
        series.push(RttSample {
            seq: 2,
            rtt_us: 300,
            sent_at_ms: 2,
        });
        assert_eq!(series.min_us(), Some(100));
        assert_eq!(series.max_us(), Some(500));
    }
}
