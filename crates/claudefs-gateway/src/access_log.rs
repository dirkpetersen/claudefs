#![allow(missing_docs)]

//! NFS/S3 access logging with structured events

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Gateway protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GatewayProtocol {
    Nfs3,
    Nfs4,
    S3,
    Smb3,
}

/// Access log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessLogEntry {
    /// Unix timestamp in seconds
    pub timestamp: u64,
    /// Client IP address
    pub client_ip: String,
    /// Protocol
    pub protocol: GatewayProtocol,
    /// Operation name (e.g., "GETATTR", "READ", "GET_OBJECT")
    pub operation: String,
    /// Resource path or key
    pub resource: String,
    /// Result status (0 = success, non-zero = NFS status or HTTP status)
    pub status: u32,
    /// Bytes transferred (read or written)
    pub bytes: u64,
    /// Duration in microseconds
    pub duration_us: u64,
    /// User ID (0 = anonymous)
    pub uid: u32,
}

impl AccessLogEntry {
    pub fn new(
        client_ip: &str,
        protocol: GatewayProtocol,
        operation: &str,
        resource: &str,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            timestamp,
            client_ip: client_ip.to_string(),
            protocol,
            operation: operation.to_string(),
            resource: resource.to_string(),
            status: 0,
            bytes: 0,
            duration_us: 0,
            uid: 0,
        }
    }

    pub fn with_status(mut self, status: u32) -> Self {
        self.status = status;
        self
    }

    pub fn with_bytes(mut self, bytes: u64) -> Self {
        self.bytes = bytes;
        self
    }

    pub fn with_duration_us(mut self, duration_us: u64) -> Self {
        self.duration_us = duration_us;
        self
    }

    pub fn with_uid(mut self, uid: u32) -> Self {
        self.uid = uid;
        self
    }

    /// Check if this is an error (status != 0)
    pub fn is_error(&self) -> bool {
        self.status != 0
    }

    /// Format as a single log line (CSV-like: timestamp,client_ip,protocol,operation,resource,status,bytes,duration_us,uid)
    pub fn format_csv(&self) -> String {
        format!(
            "{},{},{:?},{},{},{},{},{},{}",
            self.timestamp,
            self.client_ip,
            self.protocol,
            self.operation,
            self.resource,
            self.status,
            self.bytes,
            self.duration_us,
            self.uid
        )
    }

    /// Format as structured string for tracing
    pub fn format_structured(&self) -> String {
        format!(
            "timestamp={} client_ip={} protocol={:?} operation={} resource={} status={} bytes={} duration_us={} uid={}",
            self.timestamp,
            self.client_ip,
            self.protocol,
            self.operation,
            self.resource,
            self.status,
            self.bytes,
            self.duration_us,
            self.uid
        )
    }
}

/// Access log statistics for a time window
#[derive(Debug, Clone, Default)]
pub struct AccessLogStats {
    pub total_requests: u64,
    pub error_count: u64,
    pub total_bytes: u64,
    pub total_duration_us: u64,
}

impl AccessLogStats {
    pub fn add_entry(&mut self, entry: &AccessLogEntry) {
        self.total_requests += 1;
        if entry.is_error() {
            self.error_count += 1;
        }
        self.total_bytes += entry.bytes;
        self.total_duration_us += entry.duration_us;
    }

    pub fn avg_duration_us(&self) -> u64 {
        if self.total_requests == 0 {
            0
        } else {
            self.total_duration_us / self.total_requests
        }
    }

    pub fn error_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            self.error_count as f64 / self.total_requests as f64
        }
    }

    pub fn requests_per_sec(&self, window_secs: u64) -> f64 {
        if window_secs == 0 {
            0.0
        } else {
            self.total_requests as f64 / window_secs as f64
        }
    }
}

/// In-memory access log with fixed capacity (ring buffer)
pub struct AccessLog {
    capacity: usize,
    entries: Mutex<VecDeque<AccessLogEntry>>,
    stats: Mutex<AccessLogStats>,
}

impl AccessLog {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: Mutex::new(VecDeque::with_capacity(capacity)),
            stats: Mutex::new(AccessLogStats::default()),
        }
    }

    /// Append a log entry (evicts oldest if at capacity)
    pub fn append(&self, entry: AccessLogEntry) {
        let mut entries = self.entries.lock().unwrap();
        let mut stats = self.stats.lock().unwrap();

        if entries.len() >= self.capacity {
            if let Some(evicted) = entries.pop_front() {
                stats.total_requests = stats.total_requests.saturating_sub(1);
                stats.error_count =
                    stats
                        .error_count
                        .saturating_sub(if evicted.is_error() { 1 } else { 0 });
                stats.total_bytes = stats.total_bytes.saturating_sub(evicted.bytes);
                stats.total_duration_us =
                    stats.total_duration_us.saturating_sub(evicted.duration_us);
            }
        }

        stats.add_entry(&entry);
        entries.push_back(entry);
    }

    /// Get recent entries (up to n, newest first)
    pub fn recent(&self, n: usize) -> Vec<AccessLogEntry> {
        let entries = self.entries.lock().unwrap();
        entries.iter().rev().take(n).cloned().collect()
    }

    /// Get current statistics
    pub fn stats(&self) -> AccessLogStats {
        self.stats.lock().unwrap().clone()
    }

    /// Reset statistics
    pub fn reset_stats(&self) {
        let mut stats = self.stats.lock().unwrap();
        *stats = AccessLogStats::default();
    }

    /// Filter entries by protocol
    pub fn filter_protocol(&self, protocol: GatewayProtocol) -> Vec<AccessLogEntry> {
        let entries = self.entries.lock().unwrap();
        entries
            .iter()
            .filter(|e| e.protocol == protocol)
            .cloned()
            .collect()
    }

    /// Filter entries by client IP
    pub fn filter_client(&self, ip: &str) -> Vec<AccessLogEntry> {
        let entries = self.entries.lock().unwrap();
        entries
            .iter()
            .filter(|e| e.client_ip == ip)
            .cloned()
            .collect()
    }

    /// Count entries
    pub fn len(&self) -> usize {
        self.entries.lock().unwrap().len()
    }

    /// Is empty
    pub fn is_empty(&self) -> bool {
        self.entries.lock().unwrap().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_access_log_entry_new() {
        let entry = AccessLogEntry::new(
            "192.168.1.1",
            GatewayProtocol::Nfs3,
            "GETATTR",
            "/test/file",
        );
        assert_eq!(entry.client_ip, "192.168.1.1");
        assert_eq!(entry.protocol, GatewayProtocol::Nfs3);
        assert_eq!(entry.operation, "GETATTR");
        assert_eq!(entry.resource, "/test/file");
        assert_eq!(entry.status, 0);
        assert_eq!(entry.bytes, 0);
        assert_eq!(entry.duration_us, 0);
        assert_eq!(entry.uid, 0);
    }

    #[test]
    fn test_access_log_entry_with_status() {
        let entry = AccessLogEntry::new("192.168.1.1", GatewayProtocol::S3, "GET", "bucket/key")
            .with_status(404);
        assert_eq!(entry.status, 404);
    }

    #[test]
    fn test_access_log_entry_with_bytes() {
        let entry = AccessLogEntry::new("192.168.1.1", GatewayProtocol::Nfs3, "READ", "/file")
            .with_bytes(4096);
        assert_eq!(entry.bytes, 4096);
    }

    #[test]
    fn test_access_log_entry_with_duration() {
        let entry = AccessLogEntry::new("192.168.1.1", GatewayProtocol::Nfs3, "READ", "/file")
            .with_duration_us(1500);
        assert_eq!(entry.duration_us, 1500);
    }

    #[test]
    fn test_access_log_entry_with_uid() {
        let entry = AccessLogEntry::new("192.168.1.1", GatewayProtocol::Nfs3, "WRITE", "/file")
            .with_uid(1000);
        assert_eq!(entry.uid, 1000);
    }

    #[test]
    fn test_access_log_entry_is_error() {
        let success_entry =
            AccessLogEntry::new("192.168.1.1", GatewayProtocol::Nfs3, "GETATTR", "/")
                .with_status(0);
        assert!(!success_entry.is_error());

        let error_entry = AccessLogEntry::new("192.168.1.1", GatewayProtocol::Nfs3, "GETATTR", "/")
            .with_status(13);
        assert!(error_entry.is_error());
    }

    #[test]
    fn test_access_log_entry_format_csv() {
        let entry = AccessLogEntry::new("10.0.0.1", GatewayProtocol::Nfs3, "READ", "/data/file")
            .with_status(0)
            .with_bytes(8192)
            .with_duration_us(500)
            .with_uid(100);

        let csv = entry.format_csv();
        assert!(csv.contains("10.0.0.1"));
        assert!(csv.contains("Nfs3"));
        assert!(csv.contains("READ"));
        assert!(csv.contains("/data/file"));
    }

    #[test]
    fn test_access_log_entry_format_structured() {
        let entry =
            AccessLogEntry::new("10.0.0.1", GatewayProtocol::S3, "GET_OBJECT", "bucket/key")
                .with_status(200)
                .with_bytes(1024);

        let structured = entry.format_structured();
        assert!(structured.contains("10.0.0.1"));
        assert!(structured.contains("S3"));
    }

    #[test]
    fn test_access_log_stats_add_entry() {
        let mut stats = AccessLogStats::default();
        let entry = AccessLogEntry::new("192.168.1.1", GatewayProtocol::Nfs3, "READ", "/")
            .with_bytes(4096)
            .with_duration_us(1000);

        stats.add_entry(&entry);
        assert_eq!(stats.total_requests, 1);
        assert_eq!(stats.error_count, 0);
        assert_eq!(stats.total_bytes, 4096);
        assert_eq!(stats.total_duration_us, 1000);
    }

    #[test]
    fn test_access_log_stats_error_count() {
        let mut stats = AccessLogStats::default();
        let success = AccessLogEntry::new("ip", GatewayProtocol::Nfs3, "X", "/").with_status(0);
        let error = AccessLogEntry::new("ip", GatewayProtocol::Nfs3, "X", "/").with_status(5);

        stats.add_entry(&success);
        stats.add_entry(&error);
        stats.add_entry(&error);

        assert_eq!(stats.total_requests, 3);
        assert_eq!(stats.error_count, 2);
    }

    #[test]
    fn test_access_log_stats_avg_duration() {
        let mut stats = AccessLogStats::default();
        stats.add_entry(
            &AccessLogEntry::new("ip", GatewayProtocol::Nfs3, "X", "/").with_duration_us(100),
        );
        stats.add_entry(
            &AccessLogEntry::new("ip", GatewayProtocol::Nfs3, "X", "/").with_duration_us(200),
        );
        stats.add_entry(
            &AccessLogEntry::new("ip", GatewayProtocol::Nfs3, "X", "/").with_duration_us(300),
        );

        assert_eq!(stats.avg_duration_us(), 200);
    }

    #[test]
    fn test_access_log_stats_avg_duration_empty() {
        let stats = AccessLogStats::default();
        assert_eq!(stats.avg_duration_us(), 0);
    }

    #[test]
    fn test_access_log_stats_error_rate() {
        let mut stats = AccessLogStats::default();
        stats.add_entry(&AccessLogEntry::new("ip", GatewayProtocol::Nfs3, "X", "/").with_status(0));
        stats.add_entry(&AccessLogEntry::new("ip", GatewayProtocol::Nfs3, "X", "/").with_status(0));
        stats.add_entry(&AccessLogEntry::new("ip", GatewayProtocol::Nfs3, "X", "/").with_status(5));
        stats.add_entry(&AccessLogEntry::new("ip", GatewayProtocol::Nfs3, "X", "/").with_status(0));

        assert_eq!(stats.error_rate(), 0.25);
    }

    #[test]
    fn test_access_log_stats_requests_per_sec() {
        let mut stats = AccessLogStats::default();
        stats.total_requests = 100;
        assert_eq!(stats.requests_per_sec(10), 10.0);
    }

    #[test]
    fn test_access_log_stats_requests_per_sec_zero_window() {
        let stats = AccessLogStats::default();
        assert_eq!(stats.requests_per_sec(0), 0.0);
    }

    #[test]
    fn test_access_log_append() {
        let log = AccessLog::new(10);
        let entry = AccessLogEntry::new("192.168.1.1", GatewayProtocol::Nfs3, "GETATTR", "/");
        log.append(entry);

        assert_eq!(log.len(), 1);
    }

    #[test]
    fn test_access_log_recent() {
        let log = AccessLog::new(10);
        for i in 0..5 {
            log.append(AccessLogEntry::new(
                "192.168.1.1",
                GatewayProtocol::Nfs3,
                "OP",
                &format!("/{}", i),
            ));
        }

        let recent = log.recent(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].resource, "/4");
        assert_eq!(recent[1].resource, "/3");
    }

    #[test]
    fn test_access_log_stats() {
        let log = AccessLog::new(10);
        log.append(
            AccessLogEntry::new("192.168.1.1", GatewayProtocol::Nfs3, "READ", "/")
                .with_bytes(1000)
                .with_duration_us(100),
        );
        log.append(
            AccessLogEntry::new("192.168.1.2", GatewayProtocol::Nfs3, "WRITE", "/")
                .with_bytes(2000)
                .with_duration_us(200),
        );

        let stats = log.stats();
        assert_eq!(stats.total_requests, 2);
        assert_eq!(stats.total_bytes, 3000);
    }

    #[test]
    fn test_access_log_reset_stats() {
        let log = AccessLog::new(10);
        log.append(AccessLogEntry::new(
            "192.168.1.1",
            GatewayProtocol::Nfs3,
            "READ",
            "/",
        ));

        log.reset_stats();
        let stats = log.stats();
        assert_eq!(stats.total_requests, 0);
    }

    #[test]
    fn test_access_log_filter_protocol() {
        let log = AccessLog::new(10);
        log.append(AccessLogEntry::new(
            "ip1",
            GatewayProtocol::Nfs3,
            "GETATTR",
            "/",
        ));
        log.append(AccessLogEntry::new(
            "ip2",
            GatewayProtocol::S3,
            "GET",
            "bucket/key",
        ));
        log.append(AccessLogEntry::new(
            "ip3",
            GatewayProtocol::Nfs3,
            "READ",
            "/file",
        ));

        let nfs_entries = log.filter_protocol(GatewayProtocol::Nfs3);
        assert_eq!(nfs_entries.len(), 2);

        let s3_entries = log.filter_protocol(GatewayProtocol::S3);
        assert_eq!(s3_entries.len(), 1);
    }

    #[test]
    fn test_access_log_filter_client() {
        let log = AccessLog::new(10);
        log.append(AccessLogEntry::new(
            "192.168.1.1",
            GatewayProtocol::Nfs3,
            "GETATTR",
            "/",
        ));
        log.append(AccessLogEntry::new(
            "192.168.1.2",
            GatewayProtocol::Nfs3,
            "READ",
            "/",
        ));
        log.append(AccessLogEntry::new(
            "192.168.1.1",
            GatewayProtocol::Nfs3,
            "WRITE",
            "/file",
        ));

        let entries = log.filter_client("192.168.1.1");
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_access_log_capacity_eviction() {
        let log = AccessLog::new(3);
        log.append(AccessLogEntry::new("ip", GatewayProtocol::Nfs3, "A", "/1"));
        log.append(AccessLogEntry::new("ip", GatewayProtocol::Nfs3, "B", "/2"));
        log.append(AccessLogEntry::new("ip", GatewayProtocol::Nfs3, "C", "/3"));
        log.append(AccessLogEntry::new("ip", GatewayProtocol::Nfs3, "D", "/4"));

        assert_eq!(log.len(), 3);
        let recent = log.recent(3);
        assert!(!recent.iter().any(|e| e.operation == "A"));
    }

    #[test]
    fn test_access_log_len() {
        let log = AccessLog::new(10);
        assert_eq!(log.len(), 0);

        log.append(AccessLogEntry::new("ip", GatewayProtocol::Nfs3, "X", "/"));
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn test_access_log_is_empty() {
        let log = AccessLog::new(10);
        assert!(log.is_empty());

        log.append(AccessLogEntry::new("ip", GatewayProtocol::Nfs3, "X", "/"));
        assert!(!log.is_empty());
    }
}
