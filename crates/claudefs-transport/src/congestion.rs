//! TCP-style congestion control module for the transport layer.
//!
//! Implements multiple congestion control algorithms: AIMD, Cubic, and BBR.
//! Provides window-based flow control with RTT tracking and loss detection.

use serde::{Deserialize, Serialize};
use tracing::{debug, trace};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CongestionAlgorithm {
    #[default]
    Aimd,
    Cubic,
    Bbr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CongestionState {
    #[default]
    SlowStart,
    CongestionAvoidance,
    Recovery,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CongestionConfig {
    pub algorithm: CongestionAlgorithm,
    pub initial_window: u64,
    pub min_window: u64,
    pub max_window: u64,
    pub aimd_increase: u64,
    pub aimd_decrease_factor: f64,
    pub cubic_beta: f64,
    pub cubic_c: f64,
    pub slow_start_threshold: u64,
    pub rtt_smoothing_alpha: f64,
}

impl Default for CongestionConfig {
    fn default() -> Self {
        Self {
            algorithm: CongestionAlgorithm::default(),
            initial_window: 65536,
            min_window: 4096,
            max_window: 16 * 1024 * 1024,
            aimd_increase: 4096,
            aimd_decrease_factor: 0.5,
            cubic_beta: 0.7,
            cubic_c: 0.4,
            slow_start_threshold: 1048576,
            rtt_smoothing_alpha: 0.125,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CongestionStats {
    pub window_size: u64,
    pub ssthresh: u64,
    pub bytes_in_flight: u64,
    pub smoothed_rtt_us: u64,
    pub min_rtt_us: u64,
    pub total_sent: u64,
    pub total_acked: u64,
    pub total_lost: u64,
    pub loss_events: u64,
    pub state: String,
}

pub struct CongestionWindow {
    config: CongestionConfig,
    state: CongestionState,
    window_size: u64,
    ssthresh: u64,
    bytes_in_flight: u64,
    smoothed_rtt_us: u64,
    min_rtt_us: u64,
    rtt_variance_us: u64,
    total_sent: u64,
    total_acked: u64,
    total_lost: u64,
    loss_events: u64,
    cubic_t_last_loss_ms: u64,
    cubic_w_max: u64,
    bbr_bandwidth_bps: u64,
    bbr_pacing_rate_bps: u64,
}

impl CongestionWindow {
    pub fn new(config: CongestionConfig) -> Self {
        Self {
            config,
            state: CongestionState::SlowStart,
            window_size: 0,
            ssthresh: 0,
            bytes_in_flight: 0,
            smoothed_rtt_us: 0,
            min_rtt_us: u64::MAX,
            rtt_variance_us: 0,
            total_sent: 0,
            total_acked: 0,
            total_lost: 0,
            loss_events: 0,
            cubic_t_last_loss_ms: 0,
            cubic_w_max: 0,
            bbr_bandwidth_bps: 0,
            bbr_pacing_rate_bps: 0,
        }
    }

    pub fn available_window(&self) -> u64 {
        self.window_size.saturating_sub(self.bytes_in_flight)
    }

    pub fn can_send(&self, bytes: u64) -> bool {
        self.bytes_in_flight + bytes <= self.window_size
    }

    pub fn on_send(&mut self, bytes: u64) {
        trace!(
            "Sending {} bytes, window={}, in_flight={}",
            bytes,
            self.window_size,
            self.bytes_in_flight
        );
        self.bytes_in_flight += bytes;
        self.total_sent += bytes;
        if self.window_size == 0 {
            self.window_size = self.config.initial_window;
        }
    }

    pub fn on_ack(&mut self, bytes: u64, rtt_us: u64) {
        trace!(
            "ACK: {} bytes, rtt={}us, state={:?}",
            bytes,
            rtt_us,
            self.state
        );

        self.bytes_in_flight = self.bytes_in_flight.saturating_sub(bytes);
        self.total_acked += bytes;

        let alpha = self.config.rtt_smoothing_alpha;
        if self.smoothed_rtt_us == 0 {
            self.smoothed_rtt_us = rtt_us;
        } else {
            self.smoothed_rtt_us =
                ((1.0 - alpha) * self.smoothed_rtt_us as f64 + alpha * rtt_us as f64) as u64;
        }

        if rtt_us < self.min_rtt_us {
            self.min_rtt_us = rtt_us;
        }

        let rtt_diff = rtt_us.abs_diff(self.smoothed_rtt_us);
        if self.rtt_variance_us == 0 {
            self.rtt_variance_us = rtt_diff;
        } else {
            self.rtt_variance_us =
                ((1.0 - alpha) * self.rtt_variance_us as f64 + alpha * rtt_diff as f64) as u64;
        }

        match self.state {
            CongestionState::SlowStart => {
                if self.ssthresh > 0 && self.window_size >= self.ssthresh {
                    self.state = CongestionState::CongestionAvoidance;
                    debug!(
                        "Exiting slow start, entering congestion avoidance at window {}",
                        self.window_size
                    );
                } else {
                    let new_window = (self.window_size + bytes).min(self.config.max_window);
                    if new_window > self.window_size {
                        trace!("Slow start: window {} -> {}", self.window_size, new_window);
                    }
                    self.window_size = new_window;
                }
            }
            CongestionState::CongestionAvoidance => match self.config.algorithm {
                CongestionAlgorithm::Aimd => {
                    let window_fraction = (bytes as f64 / self.window_size as f64).min(1.0);
                    let increase = (self.config.aimd_increase as f64 * window_fraction) as u64;
                    self.window_size = (self.window_size + increase).min(self.config.max_window);
                    trace!("AIMD CA: window += {} -> {}", increase, self.window_size);
                }
                CongestionAlgorithm::Cubic => {
                    let elapsed_ms = 1u64;
                    let k = Self::cubic_root(
                        (self.cubic_w_max as f64 * self.config.cubic_beta / self.config.cubic_c)
                            as u64,
                    );
                    let target = (self.config.cubic_c * ((elapsed_ms + k).pow(3) as f64)) as u64;
                    let w_ca = self.cubic_w_max as f64 * self.config.cubic_beta;
                    if (target as f64) < w_ca {
                        self.window_size =
                            (w_ca as u64 + self.config.aimd_increase).min(self.config.max_window);
                    } else {
                        self.window_size =
                            (target + self.config.aimd_increase).min(self.config.max_window);
                    }
                    trace!("Cubic CA: window -> {}", self.window_size);
                }
                CongestionAlgorithm::Bbr => {
                    if self.min_rtt_us > 0 && self.min_rtt_us != u64::MAX {
                        let bw_rtt = self.bbr_bandwidth_bps.saturating_mul(self.min_rtt_us);
                        self.window_size = (bw_rtt / 1_000_000)
                            .min(self.config.max_window)
                            .max(self.config.min_window);
                        trace!("BBR CA: window -> {}", self.window_size);
                    }
                }
            },
            CongestionState::Recovery => {
                self.state = CongestionState::CongestionAvoidance;
                debug!("Exiting recovery, entering congestion avoidance");
            }
        }

        if matches!(self.config.algorithm, CongestionAlgorithm::Bbr)
            && self.bbr_bandwidth_bps > 0
            && self.min_rtt_us > 0
            && self.min_rtt_us != u64::MAX
        {
            let delivery_time_us =
                (bytes as f64 / self.bbr_bandwidth_bps as f64 * 1_000_000.0) as u64;
            if delivery_time_us > 0 {
                let estimated_bw = (bytes as f64 / delivery_time_us as f64 * 1_000_000.0) as u64;
                self.bbr_bandwidth_bps = (self.bbr_bandwidth_bps * 7 / 8 + estimated_bw / 8).max(1);
                self.bbr_pacing_rate_bps = self.bbr_bandwidth_bps;
            }
        }
    }

    fn cubic_root(n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        let mut x = (n as f64).cbrt() as u64;
        if x == 0 {
            x = 1;
        }
        x
    }

    pub fn on_loss(&mut self, bytes: u64) {
        trace!("Loss: {} bytes, state={:?}", bytes, self.state);

        self.bytes_in_flight = self.bytes_in_flight.saturating_sub(bytes);
        self.total_lost += bytes;
        self.loss_events += 1;

        match self.config.algorithm {
            CongestionAlgorithm::Aimd => {
                self.ssthresh = ((self.window_size as f64 * self.config.aimd_decrease_factor)
                    as u64)
                    .max(self.config.min_window);
                self.window_size = self.ssthresh;
                debug!(
                    "AIMD loss: ssthresh={}, window={}",
                    self.ssthresh, self.window_size
                );
            }
            CongestionAlgorithm::Cubic => {
                self.cubic_w_max = self.window_size;
                self.cubic_t_last_loss_ms = 1;
                self.ssthresh = ((self.window_size as f64 * self.config.cubic_beta) as u64)
                    .max(self.config.min_window);
                self.window_size = self.ssthresh;
                debug!(
                    "Cubic loss: w_max={}, ssthresh={}, window={}",
                    self.cubic_w_max, self.ssthresh, self.window_size
                );
            }
            CongestionAlgorithm::Bbr => {
                self.ssthresh =
                    ((self.window_size as f64 * 0.7) as u64).max(self.config.min_window);
                self.window_size = self.ssthresh;
                self.bbr_pacing_rate_bps = (self.bbr_pacing_rate_bps as f64 * 0.7) as u64;
                debug!(
                    "BBR loss: ssthresh={}, window={}, pacing={}",
                    self.ssthresh, self.window_size, self.bbr_pacing_rate_bps
                );
            }
        }

        if self.state != CongestionState::Recovery {
            self.state = CongestionState::Recovery;
        }
    }

    pub fn state(&self) -> &CongestionState {
        &self.state
    }

    pub fn window_size(&self) -> u64 {
        self.window_size
    }

    pub fn smoothed_rtt_us(&self) -> u64 {
        self.smoothed_rtt_us
    }

    pub fn stats(&self) -> CongestionStats {
        let state_str = match self.state {
            CongestionState::SlowStart => "SlowStart",
            CongestionState::CongestionAvoidance => "CongestionAvoidance",
            CongestionState::Recovery => "Recovery",
        };
        CongestionStats {
            window_size: self.window_size,
            ssthresh: self.ssthresh,
            bytes_in_flight: self.bytes_in_flight,
            smoothed_rtt_us: self.smoothed_rtt_us,
            min_rtt_us: if self.min_rtt_us == u64::MAX {
                0
            } else {
                self.min_rtt_us
            },
            total_sent: self.total_sent,
            total_acked: self.total_acked,
            total_lost: self.total_lost,
            loss_events: self.loss_events,
            state: state_str.to_string(),
        }
    }

    pub fn set_ssthresh(&mut self, ssthresh: u64) {
        self.ssthresh = ssthresh;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = CongestionConfig::default();
        assert_eq!(config.algorithm, CongestionAlgorithm::Aimd);
        assert_eq!(config.initial_window, 65536);
        assert_eq!(config.min_window, 4096);
        assert_eq!(config.max_window, 16 * 1024 * 1024);
        assert_eq!(config.aimd_increase, 4096);
        assert!((config.aimd_decrease_factor - 0.5).abs() < 1e-6);
        assert!((config.cubic_beta - 0.7).abs() < 1e-6);
        assert!((config.cubic_c - 0.4).abs() < 1e-6);
        assert_eq!(config.slow_start_threshold, 1048576);
        assert!((config.rtt_smoothing_alpha - 0.125).abs() < 1e-6);
    }

    #[test]
    fn test_initial_state() {
        let config = CongestionConfig::default();
        let cw = CongestionWindow::new(config);
        assert_eq!(*cw.state(), CongestionState::SlowStart);
        assert_eq!(cw.window_size(), 0);
        assert_eq!(cw.smoothed_rtt_us(), 0);
    }

    #[test]
    fn test_slow_start_growth() {
        let config = CongestionConfig::default();
        let mut cw = CongestionWindow::new(config);

        cw.on_send(1000);
        cw.on_ack(1000, 1000);

        assert_eq!(cw.window_size(), 65536 + 1000);
    }

    #[test]
    fn test_slow_start_to_congestion_avoidance() {
        let mut config = CongestionConfig::default();
        config.slow_start_threshold = 10000;
        let mut cw = CongestionWindow::new(config);

        cw.set_ssthresh(10000);
        cw.on_send(1000);
        cw.on_ack(1000, 1000);
        cw.on_send(1000);
        cw.on_ack(1000, 1000);
        cw.on_send(1000);
        cw.on_ack(1000, 1000);

        assert_eq!(*cw.state(), CongestionState::CongestionAvoidance);
    }

    #[test]
    fn test_aimd_congestion_avoidance() {
        let mut config = CongestionConfig::default();
        config.initial_window = 10000;
        let mut cw = CongestionWindow::new(config);

        cw.set_ssthresh(5000);
        cw.on_send(5000);
        cw.on_ack(5000, 1000);

        assert_eq!(*cw.state(), CongestionState::CongestionAvoidance);
        let window_after = cw.window_size();
        assert!(window_after >= 10000);
    }

    #[test]
    fn test_aimd_loss_handling() {
        let mut config = CongestionConfig::default();
        config.aimd_decrease_factor = 0.5;
        let mut cw = CongestionWindow::new(config);

        cw.on_send(10000);
        cw.on_ack(10000, 1000);
        cw.on_loss(5000);

        let stats = cw.stats();
        assert_eq!(stats.loss_events, 1);
        assert!(cw.window_size() < 65536);
    }

    #[test]
    fn test_cubic_loss_handling() {
        let mut config = CongestionConfig::default();
        config.algorithm = CongestionAlgorithm::Cubic;
        let mut cw = CongestionWindow::new(config);

        cw.on_send(20000);
        cw.on_ack(20000, 1000);
        cw.on_loss(5000);

        assert_eq!(*cw.state(), CongestionState::Recovery);
    }

    #[test]
    fn test_bbr_bandwidth_estimation() {
        let mut config = CongestionConfig::default();
        config.algorithm = CongestionAlgorithm::Bbr;
        let mut cw = CongestionWindow::new(config);

        cw.bbr_bandwidth_bps = 1000000;
        cw.on_send(1000);
        cw.on_ack(1000, 1000);

        assert!(cw.bbr_bandwidth_bps > 0);
    }

    #[test]
    fn test_rtt_smoothing() {
        let config = CongestionConfig::default();
        let mut cw = CongestionWindow::new(config);

        cw.on_send(1000);
        cw.on_ack(1000, 1000);

        assert_eq!(cw.smoothed_rtt_us(), 1000);

        cw.on_send(1000);
        cw.on_ack(1000, 2000);

        let expected = (0.875 * 1000.0 + 0.125 * 2000.0) as u64;
        assert!((cw.smoothed_rtt_us() as f64 - expected as f64).abs() < 1.0);
    }

    #[test]
    fn test_available_window() {
        let config = CongestionConfig::default();
        let mut cw = CongestionWindow::new(config);

        cw.on_send(5000);

        assert_eq!(cw.available_window(), cw.window_size() - 5000);
    }

    #[test]
    fn test_recovery_to_avoidance() {
        let config = CongestionConfig::default();
        let mut cw = CongestionWindow::new(config);

        cw.on_loss(1000);
        assert_eq!(*cw.state(), CongestionState::Recovery);

        cw.on_ack(100, 1000);
        assert_eq!(*cw.state(), CongestionState::CongestionAvoidance);
    }

    #[test]
    fn test_window_bounds() {
        let mut config = CongestionConfig::default();
        config.max_window = 100000;
        config.min_window = 1000;
        let max_win = config.max_window;
        let min_win = config.min_window;
        let mut cw = CongestionWindow::new(config);

        cw.on_loss(100000);
        assert!(cw.window_size() >= min_win);

        cw.window_size = max_win;
        cw.on_send(max_win);
        assert!(!cw.can_send(1));
    }

    #[test]
    fn test_stats_snapshot() {
        let config = CongestionConfig::default();
        let mut cw = CongestionWindow::new(config);

        cw.on_send(1000);
        cw.on_ack(500, 1000);

        let stats = cw.stats();
        assert_eq!(stats.total_sent, 1000);
        assert_eq!(stats.total_acked, 500);
        assert_eq!(stats.bytes_in_flight, 500);
    }

    #[test]
    fn test_loss_events_count() {
        let config = CongestionConfig::default();
        let mut cw = CongestionWindow::new(config);

        cw.on_loss(1000);
        cw.on_loss(2000);

        assert_eq!(cw.stats().loss_events, 2);
    }

    #[test]
    fn test_bytes_tracking() {
        let config = CongestionConfig::default();
        let mut cw = CongestionWindow::new(config);

        cw.on_send(1000);
        cw.on_ack(800, 1000);
        cw.on_loss(150);

        let stats = cw.stats();
        assert_eq!(stats.total_sent, 1000);
        assert_eq!(stats.total_acked, 800);
        assert_eq!(stats.total_lost, 150);
    }
}
