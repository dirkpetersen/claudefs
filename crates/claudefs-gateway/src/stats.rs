//! Gateway statistics aggregation

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Per-protocol statistics counters
#[derive(Debug, Default)]
pub struct ProtocolStats {
    /// Total request count
    pub requests: AtomicU64,
    /// Total error count
    pub errors: AtomicU64,
    /// Total bytes received
    pub bytes_in: AtomicU64,
    /// Total bytes sent
    pub bytes_out: AtomicU64,
    /// Sum of all request latencies in microseconds
    pub latency_us_total: AtomicU64,
}

impl ProtocolStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_request(&self, bytes_in: u64, bytes_out: u64, latency_us: u64) {
        self.requests.fetch_add(1, Ordering::Relaxed);
        self.bytes_in.fetch_add(bytes_in, Ordering::Relaxed);
        self.bytes_out.fetch_add(bytes_out, Ordering::Relaxed);
        self.latency_us_total
            .fetch_add(latency_us, Ordering::Relaxed);
    }

    pub fn record_error(&self, latency_us: u64) {
        self.errors.fetch_add(1, Ordering::Relaxed);
        self.latency_us_total
            .fetch_add(latency_us, Ordering::Relaxed);
    }

    pub fn requests(&self) -> u64 {
        self.requests.load(Ordering::Relaxed)
    }

    pub fn errors(&self) -> u64 {
        self.errors.load(Ordering::Relaxed)
    }

    pub fn bytes_in(&self) -> u64 {
        self.bytes_in.load(Ordering::Relaxed)
    }

    pub fn bytes_out(&self) -> u64 {
        self.bytes_out.load(Ordering::Relaxed)
    }

    pub fn avg_latency_us(&self) -> u64 {
        let reqs = self.requests.load(Ordering::Relaxed);
        if reqs == 0 {
            return 0;
        }
        let total = self.latency_us_total.load(Ordering::Relaxed);
        total / reqs
    }

    pub fn error_rate(&self) -> f64 {
        let reqs = self.requests.load(Ordering::Relaxed);
        if reqs == 0 {
            return 0.0;
        }
        let errs = self.errors.load(Ordering::Relaxed);
        errs as f64 / reqs as f64
    }
}

/// Gateway-wide aggregated statistics
pub struct GatewayStats {
    /// NFSv3 protocol stats
    pub nfs3: ProtocolStats,
    /// S3 protocol stats
    pub s3: ProtocolStats,
    /// SMB3 protocol stats
    pub smb3: ProtocolStats,
    /// Server start time
    pub uptime_start: Instant,
}

impl GatewayStats {
    pub fn new() -> Self {
        Self {
            nfs3: ProtocolStats::new(),
            s3: ProtocolStats::new(),
            smb3: ProtocolStats::new(),
            uptime_start: Instant::now(),
        }
    }

    pub fn uptime_secs(&self) -> u64 {
        self.uptime_start.elapsed().as_secs()
    }

    pub fn total_requests(&self) -> u64 {
        self.nfs3.requests() + self.s3.requests() + self.smb3.requests()
    }

    pub fn total_errors(&self) -> u64 {
        self.nfs3.errors() + self.s3.errors() + self.smb3.errors()
    }

    pub fn total_bytes_in(&self) -> u64 {
        self.nfs3.bytes_in() + self.s3.bytes_in() + self.smb3.bytes_in()
    }

    pub fn total_bytes_out(&self) -> u64 {
        self.nfs3.bytes_out() + self.s3.bytes_out() + self.smb3.bytes_out()
    }

    pub fn overall_error_rate(&self) -> f64 {
        let total = self.total_requests();
        if total == 0 {
            return 0.0;
        }
        self.total_errors() as f64 / total as f64
    }

    pub fn to_prometheus(&self) -> String {
        let mut output = String::new();

        output.push_str("# HELP cfs_gateway_requests_total Total requests by protocol\n");
        output.push_str("# TYPE cfs_gateway_requests_total counter\n");
        output.push_str(&format!(
            "cfs_gateway_requests_total{{protocol=\"nfs3\"}} {}\n",
            self.nfs3.requests()
        ));
        output.push_str(&format!(
            "cfs_gateway_requests_total{{protocol=\"s3\"}} {}\n",
            self.s3.requests()
        ));
        output.push_str(&format!(
            "cfs_gateway_requests_total{{protocol=\"smb3\"}} {}\n",
            self.smb3.requests()
        ));

        output.push_str("# HELP cfs_gateway_errors_total Total errors by protocol\n");
        output.push_str("# TYPE cfs_gateway_errors_total counter\n");
        output.push_str(&format!(
            "cfs_gateway_errors_total{{protocol=\"nfs3\"}} {}\n",
            self.nfs3.errors()
        ));
        output.push_str(&format!(
            "cfs_gateway_errors_total{{protocol=\"s3\"}} {}\n",
            self.s3.errors()
        ));
        output.push_str(&format!(
            "cfs_gateway_errors_total{{protocol=\"smb3\"}} {}\n",
            self.smb3.errors()
        ));

        output.push_str("# HELP cfs_gateway_bytes_in_total Total bytes in by protocol\n");
        output.push_str("# TYPE cfs_gateway_bytes_in_total counter\n");
        output.push_str(&format!(
            "cfs_gateway_bytes_in_total{{protocol=\"nfs3\"}} {}\n",
            self.nfs3.bytes_in()
        ));
        output.push_str(&format!(
            "cfs_gateway_bytes_in_total{{protocol=\"s3\"}} {}\n",
            self.s3.bytes_in()
        ));
        output.push_str(&format!(
            "cfs_gateway_bytes_in_total{{protocol=\"smb3\"}} {}\n",
            self.smb3.bytes_in()
        ));

        output.push_str("# HELP cfs_gateway_bytes_out_total Total bytes out by protocol\n");
        output.push_str("# TYPE cfs_gateway_bytes_out_total counter\n");
        output.push_str(&format!(
            "cfs_gateway_bytes_out_total{{protocol=\"nfs3\"}} {}\n",
            self.nfs3.bytes_out()
        ));
        output.push_str(&format!(
            "cfs_gateway_bytes_out_total{{protocol=\"s3\"}} {}\n",
            self.s3.bytes_out()
        ));
        output.push_str(&format!(
            "cfs_gateway_bytes_out_total{{protocol=\"smb3\"}} {}\n",
            self.smb3.bytes_out()
        ));

        output
    }
}

impl Default for GatewayStats {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_stats_record_request() {
        let stats = ProtocolStats::new();
        stats.record_request(100, 200, 50);
        assert_eq!(stats.requests(), 1);
        assert_eq!(stats.bytes_in(), 100);
        assert_eq!(stats.bytes_out(), 200);
    }

    #[test]
    fn test_protocol_stats_record_error() {
        let stats = ProtocolStats::new();
        stats.record_request(100, 200, 50);
        stats.record_error(30);
        assert_eq!(stats.requests(), 1);
        assert_eq!(stats.errors(), 1);
    }

    #[test]
    fn test_protocol_stats_avg_latency() {
        let stats = ProtocolStats::new();
        stats.record_request(100, 200, 100);
        stats.record_request(100, 200, 200);
        assert_eq!(stats.avg_latency_us(), 150);
    }

    #[test]
    fn test_protocol_stats_avg_latency_zero_requests() {
        let stats = ProtocolStats::new();
        assert_eq!(stats.avg_latency_us(), 0);
    }

    #[test]
    fn test_protocol_stats_error_rate() {
        let stats = ProtocolStats::new();
        stats.record_request(100, 200, 50);
        stats.record_request(100, 200, 50);
        stats.record_error(30);
        assert_eq!(stats.error_rate(), 0.5);
    }

    #[test]
    fn test_protocol_stats_error_rate_zero_requests() {
        let stats = ProtocolStats::new();
        assert_eq!(stats.error_rate(), 0.0);
    }

    #[test]
    fn test_gateway_stats_total_requests() {
        let stats = GatewayStats::new();
        stats.nfs3.record_request(100, 200, 50);
        stats.s3.record_request(100, 200, 50);
        stats.smb3.record_request(100, 200, 50);
        assert_eq!(stats.total_requests(), 3);
    }

    #[test]
    fn test_gateway_stats_total_errors() {
        let stats = GatewayStats::new();
        stats.nfs3.record_request(100, 200, 50);
        stats.s3.record_error(30);
        stats.smb3.record_error(30);
        assert_eq!(stats.total_errors(), 2);
    }

    #[test]
    fn test_gateway_stats_total_bytes_in() {
        let stats = GatewayStats::new();
        stats.nfs3.record_request(100, 0, 0);
        stats.s3.record_request(200, 0, 0);
        stats.smb3.record_request(300, 0, 0);
        assert_eq!(stats.total_bytes_in(), 600);
    }

    #[test]
    fn test_gateway_stats_total_bytes_out() {
        let stats = GatewayStats::new();
        stats.nfs3.record_request(0, 100, 0);
        stats.s3.record_request(0, 200, 0);
        stats.smb3.record_request(0, 300, 0);
        assert_eq!(stats.total_bytes_out(), 600);
    }

    #[test]
    fn test_gateway_stats_overall_error_rate() {
        let stats = GatewayStats::new();
        stats.nfs3.record_request(100, 0, 0);
        stats.nfs3.record_request(100, 0, 0);
        stats.s3.record_request(100, 0, 0);
        stats.smb3.record_request(100, 0, 0);
        stats.smb3.record_error(50);
        assert!((stats.overall_error_rate() - 0.25).abs() < 0.01);
    }

    #[test]
    fn test_gateway_stats_to_prometheus_format() {
        let stats = GatewayStats::new();
        stats.nfs3.record_request(100, 200, 50);

        let output = stats.to_prometheus();
        assert!(output.contains("cfs_gateway_requests_total{protocol=\"nfs3\"} 1"));
        assert!(output.contains("cfs_gateway_errors_total{protocol=\"nfs3\"} 0"));
        assert!(output.contains("cfs_gateway_bytes_in_total{protocol=\"nfs3\"} 100"));
        assert!(output.contains("cfs_gateway_bytes_out_total{protocol=\"nfs3\"} 200"));
    }

    #[test]
    fn test_gateway_stats_uptime() {
        let stats = GatewayStats::new();
        std::thread::yield_now();
        let uptime = stats.uptime_secs();
        assert!(uptime >= 0);
    }

    #[test]
    fn test_protocol_stats_multiple_requests() {
        let stats = ProtocolStats::new();
        for _ in 0..100 {
            stats.record_request(1024, 2048, 100);
        }
        assert_eq!(stats.requests(), 100);
        assert_eq!(stats.bytes_in(), 1024 * 100);
        assert_eq!(stats.bytes_out(), 2048 * 100);
    }

    #[test]
    fn test_gateway_stats_all_protocols() {
        let stats = GatewayStats::new();
        stats.nfs3.record_request(100, 100, 50);
        stats.s3.record_request(200, 200, 50);
        stats.smb3.record_request(300, 300, 50);

        let output = stats.to_prometheus();
        assert!(output.contains("protocol=\"nfs3\"} 1"));
        assert!(output.contains("protocol=\"s3\"} 1"));
        assert!(output.contains("protocol=\"smb3\"} 1"));
    }
}
