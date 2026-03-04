//! Transport-level fault injection for chaos testing.
//!
//! This module allows injecting failures into the transport layer:
//! packet drops, artificial delays, corrupted payloads, and connection resets.
//! Configurable by fault type and injection probability.

use std::sync::atomic::{AtomicU64, Ordering};

/// Type of fault to inject.
#[derive(Debug, Clone, PartialEq)]
pub enum FaultKind {
    /// Drop the operation entirely (simulate packet loss).
    Drop,
    /// Corrupt the payload (flip a byte).
    Corrupt,
    /// Inject artificial latency (delay in milliseconds).
    Delay(u64),
    /// Simulate connection reset.
    Reset,
}

/// Fault specification: kind + probability.
#[derive(Debug, Clone)]
pub struct FaultSpec {
    /// Type of fault to inject.
    pub kind: FaultKind,
    /// Probability 0.0–1.0 of injecting this fault.
    pub probability: f64,
}

impl FaultSpec {
    /// Creates a new fault specification.
    pub fn new(kind: FaultKind, probability: f64) -> Self {
        Self { kind, probability }
    }

    /// Returns true if this fault fires given a [0,1) random value.
    pub fn fires(&self, random: f64) -> bool {
        random < self.probability
    }
}

/// Configuration for the fault injector.
#[derive(Debug, Clone, Default)]
pub struct FaultConfig {
    /// Whether fault injection is enabled (default false — safe for production).
    pub enabled: bool,
    /// Faults to potentially inject on send operations.
    pub send_faults: Vec<FaultSpec>,
    /// Faults to potentially inject on receive operations.
    pub recv_faults: Vec<FaultSpec>,
    /// Faults to potentially inject on connect operations.
    pub connect_faults: Vec<FaultSpec>,
    /// Fixed seed for deterministic testing (None = use counter-based pseudo-random).
    pub seed: Option<u64>,
}

/// Stats tracking for fault injection.
pub struct FaultInjectorStats {
    sends_attempted: AtomicU64,
    sends_dropped: AtomicU64,
    sends_corrupted: AtomicU64,
    sends_delayed: AtomicU64,
    recvs_attempted: AtomicU64,
    recvs_dropped: AtomicU64,
    connects_attempted: AtomicU64,
    connects_reset: AtomicU64,
}

impl Default for FaultInjectorStats {
    fn default() -> Self {
        Self::new()
    }
}

impl FaultInjectorStats {
    /// Creates a new stats tracker with all counters zero.
    pub fn new() -> Self {
        Self {
            sends_attempted: AtomicU64::new(0),
            sends_dropped: AtomicU64::new(0),
            sends_corrupted: AtomicU64::new(0),
            sends_delayed: AtomicU64::new(0),
            recvs_attempted: AtomicU64::new(0),
            recvs_dropped: AtomicU64::new(0),
            connects_attempted: AtomicU64::new(0),
            connects_reset: AtomicU64::new(0),
        }
    }

    fn inc_sends_attempted(&self) {
        self.sends_attempted.fetch_add(1, Ordering::Relaxed);
    }

    fn inc_sends_dropped(&self) {
        self.sends_dropped.fetch_add(1, Ordering::Relaxed);
    }

    fn inc_sends_corrupted(&self) {
        self.sends_corrupted.fetch_add(1, Ordering::Relaxed);
    }

    fn inc_sends_delayed(&self) {
        self.sends_delayed.fetch_add(1, Ordering::Relaxed);
    }

    fn inc_recvs_attempted(&self) {
        self.recvs_attempted.fetch_add(1, Ordering::Relaxed);
    }

    fn inc_recvs_dropped(&self) {
        self.recvs_dropped.fetch_add(1, Ordering::Relaxed);
    }

    fn inc_connects_attempted(&self) {
        self.connects_attempted.fetch_add(1, Ordering::Relaxed);
    }

    fn inc_connects_reset(&self) {
        self.connects_reset.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns a snapshot of current stats.
    pub fn snapshot(&self) -> FaultInjectorStatsSnapshot {
        let sends_attempted = self.sends_attempted.load(Ordering::Relaxed);
        let sends_dropped = self.sends_dropped.load(Ordering::Relaxed);
        FaultInjectorStatsSnapshot {
            sends_attempted,
            sends_dropped,
            sends_corrupted: self.sends_corrupted.load(Ordering::Relaxed),
            sends_delayed: self.sends_delayed.load(Ordering::Relaxed),
            recvs_attempted: self.recvs_attempted.load(Ordering::Relaxed),
            recvs_dropped: self.recvs_dropped.load(Ordering::Relaxed),
            connects_attempted: self.connects_attempted.load(Ordering::Relaxed),
            connects_reset: self.connects_reset.load(Ordering::Relaxed),
            send_drop_rate: if sends_attempted > 0 {
                sends_dropped as f64 / sends_attempted as f64
            } else {
                0.0
            },
        }
    }
}

/// Snapshot of fault injection stats.
#[derive(Debug, Clone)]
pub struct FaultInjectorStatsSnapshot {
    /// Total send operations attempted.
    pub sends_attempted: u64,
    /// Total send operations dropped.
    pub sends_dropped: u64,
    /// Total send operations corrupted.
    pub sends_corrupted: u64,
    /// Total send operations delayed.
    pub sends_delayed: u64,
    /// Total receive operations attempted.
    pub recvs_attempted: u64,
    /// Total receive operations dropped.
    pub recvs_dropped: u64,
    /// Total connect operations attempted.
    pub connects_attempted: u64,
    /// Total connect operations reset.
    pub connects_reset: u64,
    /// Drop rate = sends_dropped / sends_attempted (0.0 if no sends).
    pub send_drop_rate: f64,
}

/// Action for send operations.
#[derive(Debug, Clone, PartialEq)]
pub enum SendAction {
    /// Allow the send with original payload.
    Allow,
    /// Drop the send (simulate packet loss).
    Drop,
    /// Corrupt the payload — return corrupted bytes.
    Corrupt(Vec<u8>),
    /// Delay by N milliseconds (caller decides how to implement the delay).
    Delay(u64),
}

/// Action for receive operations.
#[derive(Debug, Clone, PartialEq)]
pub enum RecvAction {
    /// Allow the receive.
    Allow,
    /// Drop/simulate no data received.
    Drop,
}

/// Action for connect operations.
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectAction {
    /// Allow the connection.
    Allow,
    /// Simulate connection reset.
    Reset,
}

/// The fault injector.
pub struct FaultInjector {
    config: FaultConfig,
    stats: FaultInjectorStats,
    /// Monotonic counter used as pseudo-random source when seed is set.
    counter: AtomicU64,
}

impl FaultInjector {
    /// Creates a new fault injector with the given configuration.
    pub fn new(config: FaultConfig) -> Self {
        Self {
            config,
            stats: FaultInjectorStats::new(),
            counter: AtomicU64::new(0),
        }
    }

    /// Returns a [0,1) pseudo-random value (LCG based on counter + seed).
    fn next_random(&self) -> f64 {
        let counter_val = self.counter.fetch_add(1, Ordering::Relaxed);
        let seed = self.config.seed.unwrap_or(0xdeadbeef);
        let lcg_value = (seed ^ counter_val)
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        lcg_value as f64 / u64::MAX as f64
    }

    /// Decide what to do on a send operation. Returns the action.
    pub fn on_send(&self, payload: &[u8]) -> SendAction {
        self.stats.inc_sends_attempted();

        if !self.config.enabled {
            return SendAction::Allow;
        }

        let random = self.next_random();

        for fault in &self.config.send_faults {
            if fault.fires(random) {
                match &fault.kind {
                    FaultKind::Drop => {
                        self.stats.inc_sends_dropped();
                        return SendAction::Drop;
                    }
                    FaultKind::Corrupt => {
                        self.stats.inc_sends_corrupted();
                        return SendAction::Corrupt(corrupt_payload(payload));
                    }
                    FaultKind::Delay(ms) => {
                        self.stats.inc_sends_delayed();
                        return SendAction::Delay(*ms);
                    }
                    FaultKind::Reset => {
                        continue;
                    }
                }
            }
        }

        SendAction::Allow
    }

    /// Decide what to do on a receive operation. Returns the action.
    pub fn on_recv(&self) -> RecvAction {
        self.stats.inc_recvs_attempted();

        if !self.config.enabled {
            return RecvAction::Allow;
        }

        let random = self.next_random();

        for fault in &self.config.recv_faults {
            if fault.fires(random) {
                match &fault.kind {
                    FaultKind::Drop => {
                        self.stats.inc_recvs_dropped();
                        return RecvAction::Drop;
                    }
                    _ => continue,
                }
            }
        }

        RecvAction::Allow
    }

    /// Decide what to do on a connect operation. Returns the action.
    pub fn on_connect(&self) -> ConnectAction {
        self.stats.inc_connects_attempted();

        if !self.config.enabled {
            return ConnectAction::Allow;
        }

        let random = self.next_random();

        for fault in &self.config.connect_faults {
            if fault.fires(random) {
                match &fault.kind {
                    FaultKind::Reset => {
                        self.stats.inc_connects_reset();
                        return ConnectAction::Reset;
                    }
                    _ => continue,
                }
            }
        }

        ConnectAction::Allow
    }

    /// Returns a snapshot of injector stats.
    pub fn stats(&self) -> FaultInjectorStatsSnapshot {
        self.stats.snapshot()
    }

    /// Returns true if fault injection is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}

impl Default for FaultInjector {
    fn default() -> Self {
        Self::new(FaultConfig::default())
    }
}

/// Corrupts a byte slice: flips the first byte's MSB, or if empty returns empty.
pub fn corrupt_payload(payload: &[u8]) -> Vec<u8> {
    if payload.is_empty() {
        return Vec::new();
    }
    let mut corrupted = payload.to_vec();
    corrupted[0] ^= 0x80;
    corrupted
}

#[cfg(test)]
mod tests {
    use crate::fault_inject::{
        corrupt_payload, ConnectAction, FaultConfig, FaultInjector, FaultInjectorStatsSnapshot,
        FaultKind, FaultSpec, RecvAction, SendAction,
    };

    #[test]
    fn test_default_config_disabled() {
        let config = FaultConfig::default();
        assert!(!config.enabled);
        assert!(config.send_faults.is_empty());
        assert!(config.recv_faults.is_empty());
        assert!(config.connect_faults.is_empty());
        assert!(config.seed.is_none());
    }

    #[test]
    fn test_fault_spec_fires_never() {
        let spec = FaultSpec::new(FaultKind::Drop, 0.0);
        assert!(!spec.fires(0.0));
        assert!(!spec.fires(0.5));
        assert!(!spec.fires(0.999));
    }

    #[test]
    fn test_fault_spec_fires_always() {
        let spec = FaultSpec::new(FaultKind::Drop, 1.0);
        assert!(spec.fires(0.0));
        assert!(spec.fires(0.5));
        assert!(spec.fires(0.999));
    }

    #[test]
    fn test_fault_spec_fires_partial() {
        let spec = FaultSpec::new(FaultKind::Drop, 0.5);
        assert!(spec.fires(0.0));
        assert!(spec.fires(0.4));
        assert!(!spec.fires(0.5));
        assert!(!spec.fires(0.9));
    }

    #[test]
    fn test_on_send_no_faults() {
        let config = FaultConfig {
            enabled: true,
            ..Default::default()
        };
        let injector = FaultInjector::new(config);
        let action = injector.on_send(b"hello");
        assert_eq!(action, SendAction::Allow);
    }

    #[test]
    fn test_on_send_drop_probability_1() {
        let config = FaultConfig {
            enabled: true,
            send_faults: vec![FaultSpec::new(FaultKind::Drop, 1.0)],
            ..Default::default()
        };
        let injector = FaultInjector::new(config);
        let action = injector.on_send(b"hello");
        assert_eq!(action, SendAction::Drop);

        let stats = injector.stats();
        assert_eq!(stats.sends_dropped, 1);
    }

    #[test]
    fn test_on_send_corrupt_probability_1() {
        let config = FaultConfig {
            enabled: true,
            send_faults: vec![FaultSpec::new(FaultKind::Corrupt, 1.0)],
            ..Default::default()
        };
        let injector = FaultInjector::new(config);
        let action = injector.on_send(b"hello");
        match action {
            SendAction::Corrupt(data) => {
                assert_ne!(data, b"hello");
                assert_eq!(data.len(), 5);
            }
            _ => panic!("expected Corrupt"),
        }

        let stats = injector.stats();
        assert_eq!(stats.sends_corrupted, 1);
    }

    #[test]
    fn test_on_send_delay_probability_1() {
        let config = FaultConfig {
            enabled: true,
            send_faults: vec![FaultSpec::new(FaultKind::Delay(100), 1.0)],
            ..Default::default()
        };
        let injector = FaultInjector::new(config);
        let action = injector.on_send(b"hello");
        assert_eq!(action, SendAction::Delay(100));

        let stats = injector.stats();
        assert_eq!(stats.sends_delayed, 1);
    }

    #[test]
    fn test_on_recv_no_faults() {
        let config = FaultConfig {
            enabled: true,
            ..Default::default()
        };
        let injector = FaultInjector::new(config);
        let action = injector.on_recv();
        assert_eq!(action, RecvAction::Allow);
    }

    #[test]
    fn test_on_recv_drop_probability_1() {
        let config = FaultConfig {
            enabled: true,
            recv_faults: vec![FaultSpec::new(FaultKind::Drop, 1.0)],
            ..Default::default()
        };
        let injector = FaultInjector::new(config);
        let action = injector.on_recv();
        assert_eq!(action, RecvAction::Drop);

        let stats = injector.stats();
        assert_eq!(stats.recvs_dropped, 1);
    }

    #[test]
    fn test_on_connect_no_faults() {
        let config = FaultConfig {
            enabled: true,
            ..Default::default()
        };
        let injector = FaultInjector::new(config);
        let action = injector.on_connect();
        assert_eq!(action, ConnectAction::Allow);
    }

    #[test]
    fn test_on_connect_reset_probability_1() {
        let config = FaultConfig {
            enabled: true,
            connect_faults: vec![FaultSpec::new(FaultKind::Reset, 1.0)],
            ..Default::default()
        };
        let injector = FaultInjector::new(config);
        let action = injector.on_connect();
        assert_eq!(action, ConnectAction::Reset);

        let stats = injector.stats();
        assert_eq!(stats.connects_reset, 1);
    }

    #[test]
    fn test_disabled_injector_always_allows() {
        let config = FaultConfig {
            enabled: false,
            send_faults: vec![FaultSpec::new(FaultKind::Drop, 1.0)],
            recv_faults: vec![FaultSpec::new(FaultKind::Drop, 1.0)],
            connect_faults: vec![FaultSpec::new(FaultKind::Reset, 1.0)],
            ..Default::default()
        };
        let injector = FaultInjector::new(config);

        assert_eq!(injector.on_send(b"x"), SendAction::Allow);
        assert_eq!(injector.on_recv(), RecvAction::Allow);
        assert_eq!(injector.on_connect(), ConnectAction::Allow);
    }

    #[test]
    fn test_stats_increment() {
        let config = FaultConfig {
            enabled: true,
            send_faults: vec![FaultSpec::new(FaultKind::Drop, 1.0)],
            recv_faults: vec![FaultSpec::new(FaultKind::Drop, 1.0)],
            connect_faults: vec![FaultSpec::new(FaultKind::Reset, 1.0)],
            ..Default::default()
        };
        let injector = FaultInjector::new(config);

        injector.on_send(b"x");
        injector.on_send(b"x");
        injector.on_recv();
        injector.on_connect();

        let stats = injector.stats();
        assert_eq!(stats.sends_attempted, 2);
        assert_eq!(stats.sends_dropped, 2);
        assert_eq!(stats.recvs_attempted, 1);
        assert_eq!(stats.recvs_dropped, 1);
        assert_eq!(stats.connects_attempted, 1);
        assert_eq!(stats.connects_reset, 1);
    }

    #[test]
    fn test_send_drop_rate_calculation() {
        let config = FaultConfig {
            enabled: true,
            send_faults: vec![FaultSpec::new(FaultKind::Drop, 1.0)],
            ..Default::default()
        };
        let injector = FaultInjector::new(config);

        injector.on_send(b"x");
        injector.on_send(b"x");

        let stats = injector.stats();
        assert!((stats.send_drop_rate - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_send_drop_rate_zero_sends() {
        let injector = FaultInjector::default();
        let stats = injector.stats();
        assert_eq!(stats.send_drop_rate, 0.0);
    }

    #[test]
    fn test_corrupt_payload_flips_msb() {
        let original = b"\x00hello";
        let corrupted = corrupt_payload(original);
        assert_eq!(corrupted[0], 0x80);
        assert_eq!(&corrupted[1..], b"hello");
    }

    #[test]
    fn test_corrupt_payload_empty() {
        let corrupted = corrupt_payload(b"");
        assert!(corrupted.is_empty());
    }

    #[test]
    fn test_corrupt_payload_single_byte() {
        let corrupted = corrupt_payload(b"\xff");
        assert_eq!(corrupted, vec![0x7f]);
    }

    #[test]
    fn test_seeded_deterministic() {
        let config1 = FaultConfig {
            enabled: true,
            send_faults: vec![
                FaultSpec::new(FaultKind::Drop, 0.5),
                FaultSpec::new(FaultKind::Corrupt, 0.5),
            ],
            seed: Some(12345),
            ..Default::default()
        };
        let config2 = FaultConfig {
            enabled: true,
            send_faults: vec![
                FaultSpec::new(FaultKind::Drop, 0.5),
                FaultSpec::new(FaultKind::Corrupt, 0.5),
            ],
            seed: Some(12345),
            ..Default::default()
        };

        let injector1 = FaultInjector::new(config1);
        let injector2 = FaultInjector::new(config2);

        let actions1: Vec<_> = (0..10).map(|_| injector1.on_send(b"x")).collect();
        let actions2: Vec<_> = (0..10).map(|_| injector2.on_send(b"x")).collect();

        assert_eq!(actions1, actions2);
    }

    #[test]
    fn test_multiple_fault_specs_first_wins() {
        let config = FaultConfig {
            enabled: true,
            send_faults: vec![
                FaultSpec::new(FaultKind::Drop, 1.0),
                FaultSpec::new(FaultKind::Corrupt, 1.0),
            ],
            ..Default::default()
        };
        let injector = FaultInjector::new(config);

        let action = injector.on_send(b"hello");
        assert_eq!(action, SendAction::Drop);
        assert!(matches!(action, SendAction::Drop));
    }

    #[test]
    fn test_is_enabled() {
        let config_enabled = FaultConfig {
            enabled: true,
            ..Default::default()
        };
        let config_disabled = FaultConfig::default();

        let injector_enabled = FaultInjector::new(config_enabled);
        let injector_disabled = FaultInjector::new(config_disabled);

        assert!(injector_enabled.is_enabled());
        assert!(!injector_disabled.is_enabled());
    }

    #[test]
    fn test_fault_kind_equality() {
        assert_eq!(FaultKind::Drop, FaultKind::Drop);
        assert_eq!(FaultKind::Delay(100), FaultKind::Delay(100));
        assert_ne!(FaultKind::Drop, FaultKind::Corrupt);
        assert_ne!(FaultKind::Delay(100), FaultKind::Delay(200));
    }

    #[test]
    fn test_send_action_equality() {
        assert_eq!(SendAction::Allow, SendAction::Allow);
        assert_eq!(SendAction::Drop, SendAction::Drop);
        assert_eq!(SendAction::Delay(100), SendAction::Delay(100));
        assert_ne!(SendAction::Allow, SendAction::Drop);
    }

    #[test]
    fn test_recv_action_equality() {
        assert_eq!(RecvAction::Allow, RecvAction::Allow);
        assert_eq!(RecvAction::Drop, RecvAction::Drop);
        assert_ne!(RecvAction::Allow, RecvAction::Drop);
    }

    #[test]
    fn test_connect_action_equality() {
        assert_eq!(ConnectAction::Allow, ConnectAction::Allow);
        assert_eq!(ConnectAction::Reset, ConnectAction::Reset);
        assert_ne!(ConnectAction::Allow, ConnectAction::Reset);
    }

    #[test]
    fn test_stats_snapshot_values() {
        let config = FaultConfig {
            enabled: true,
            send_faults: vec![FaultSpec::new(FaultKind::Drop, 0.5)],
            seed: Some(42),
            ..Default::default()
        };
        let injector = FaultInjector::new(config);

        for _ in 0..100 {
            injector.on_send(b"x");
        }

        let stats = injector.stats();
        assert_eq!(stats.sends_attempted, 100);
        assert!(stats.sends_dropped > 0);
        assert!(stats.sends_dropped < 100);
    }

    #[test]
    fn test_connect_ignores_non_reset_faults() {
        let config = FaultConfig {
            enabled: true,
            connect_faults: vec![
                FaultSpec::new(FaultKind::Drop, 1.0),
                FaultSpec::new(FaultKind::Delay(100), 1.0),
            ],
            ..Default::default()
        };
        let injector = FaultInjector::new(config);

        let action = injector.on_connect();
        assert_eq!(action, ConnectAction::Allow);
    }

    #[test]
    fn test_recv_ignores_non_drop_faults() {
        let config = FaultConfig {
            enabled: true,
            recv_faults: vec![
                FaultSpec::new(FaultKind::Corrupt, 1.0),
                FaultSpec::new(FaultKind::Delay(100), 1.0),
            ],
            ..Default::default()
        };
        let injector = FaultInjector::new(config);

        let action = injector.on_recv();
        assert_eq!(action, RecvAction::Allow);
    }
}
