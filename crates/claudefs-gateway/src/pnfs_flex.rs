//! pNFS Flexible File layout (RFC 8435)

use crate::error::{GatewayError, Result};
use crate::pnfs::IoMode;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// pNFS Flexible File data server — serves file data directly
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlexFileDataServer {
    /// Server address in "host:port" format
    pub address: String,
    /// Unique device ID (16 bytes)
    pub device_id: [u8; 16],
    /// Protocol (TCP or RDMA)
    pub protocol: FlexFileProtocol,
    /// Connection retry count
    pub max_connect_attempts: u32,
}

/// Protocol used to access the data server
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlexFileProtocol {
    /// TCP transport
    Tcp,
    /// RDMA transport (InfiniBand/RoCE)
    Rdma,
}

/// A mirror group — set of data servers that hold identical copies of a segment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlexFileMirror {
    /// Data servers in this mirror group (at least one)
    pub data_servers: Vec<FlexFileDataServer>,
    /// Stripe unit size in bytes (must be power of 2)
    pub stripe_unit: u64,
    /// NFSv4 stateid for this mirror
    pub stateid: [u8; 16],
}

impl FlexFileMirror {
    /// Create a new mirror with the given data servers and stripe unit
    pub fn new(servers: Vec<FlexFileDataServer>, stripe_unit: u64) -> Self {
        Self {
            data_servers: servers,
            stripe_unit,
            stateid: [0; 16],
        }
    }

    /// Check if a stripe unit is valid (power of 2 and >= 4096)
    pub fn is_valid_stripe_unit(stripe_unit: u64) -> bool {
        stripe_unit >= 4096 && stripe_unit.is_power_of_two()
    }

    /// Number of data servers in this mirror
    pub fn server_count(&self) -> usize {
        self.data_servers.len()
    }
}

/// A pNFS Flexible File layout segment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlexFileSegment {
    /// Byte offset in the file
    pub offset: u64,
    /// Length of segment (u64::MAX = to end of file)
    pub length: u64,
    /// I/O mode
    pub iomode: IoMode,
    /// Mirror groups for this segment (for replication/RAID)
    pub mirrors: Vec<FlexFileMirror>,
}

impl FlexFileSegment {
    /// Create a new segment with the given offset, length, I/O mode, and mirrors
    pub fn new(offset: u64, length: u64, iomode: IoMode, mirrors: Vec<FlexFileMirror>) -> Self {
        Self {
            offset,
            length,
            iomode,
            mirrors,
        }
    }

    /// Whether this segment covers a given byte offset
    pub fn contains_offset(&self, offset: u64) -> bool {
        if self.length == u64::MAX {
            offset >= self.offset
        } else {
            offset >= self.offset && offset < self.offset + self.length
        }
    }

    /// Data server count across all mirrors
    pub fn total_server_count(&self) -> usize {
        self.mirrors.iter().map(|m| m.server_count()).sum()
    }
}

/// pNFS Flexible File layout for a file
#[derive(Debug, Clone)]
pub struct FlexFileLayout {
    /// File inode
    pub inode: u64,
    /// Layout segments
    pub segments: Vec<FlexFileSegment>,
    /// Layout stateid
    pub stateid: [u8; 16],
    /// Suggested I/O error reporting
    pub report_errors: bool,
}

impl FlexFileLayout {
    /// Create a new layout for the given inode
    pub fn new(inode: u64) -> Self {
        Self {
            inode,
            segments: Vec::new(),
            stateid: [0; 16],
            report_errors: true,
        }
    }

    /// Add a segment
    pub fn add_segment(&mut self, segment: FlexFileSegment) {
        self.segments.push(segment);
    }

    /// Find segments overlapping a byte range
    pub fn segments_for_range(&self, offset: u64, length: u64) -> Vec<&FlexFileSegment> {
        let end = offset.saturating_add(length);
        self.segments
            .iter()
            .filter(|s| {
                let seg_end = if s.length == u64::MAX {
                    u64::MAX
                } else {
                    s.offset + s.length
                };
                offset < seg_end && (s.length == u64::MAX || end > s.offset)
            })
            .collect()
    }

    /// Total coverage in bytes
    pub fn total_bytes(&self) -> u64 {
        if self.segments.iter().any(|s| s.length == u64::MAX) {
            u64::MAX
        } else {
            self.segments.iter().map(|s| s.length).sum()
        }
    }

    /// Number of segments
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }
}

/// Factory for creating Flexible File layouts
pub struct FlexFileLayoutServer {
    data_servers: Vec<FlexFileDataServer>,
    stripe_unit: u64,
    mirror_count: u32,
    next_stateid: Mutex<u64>,
}

impl FlexFileLayoutServer {
    /// Create a new layout server with the given data servers, stripe unit, and mirror count
    pub fn new(
        data_servers: Vec<FlexFileDataServer>,
        stripe_unit: u64,
        mirror_count: u32,
    ) -> Result<Self> {
        if !FlexFileMirror::is_valid_stripe_unit(stripe_unit) {
            return Err(GatewayError::ProtocolError {
                reason: "stripe_unit must be power of 2 and >= 4096".to_string(),
            });
        }
        if mirror_count == 0 {
            return Err(GatewayError::ProtocolError {
                reason: "mirror_count must be > 0".to_string(),
            });
        }
        if data_servers.is_empty() {
            return Err(GatewayError::ProtocolError {
                reason: "at least one data server required".to_string(),
            });
        }

        Ok(Self {
            data_servers,
            stripe_unit,
            mirror_count,
            next_stateid: Mutex::new(1),
        })
    }

    /// Generate a Flexible File layout for a file
    pub fn get_layout(&self, inode: u64, iomode: IoMode) -> FlexFileLayout {
        let mut layout = FlexFileLayout::new(inode);

        if self.data_servers.is_empty() {
            return layout;
        }

        let mut stateid_counter = self.next_stateid.lock().unwrap();
        let base_stateid = *stateid_counter;
        *stateid_counter += 1;

        let mut stateid = [0u8; 16];
        stateid[0..8].copy_from_slice(&inode.to_le_bytes());
        stateid[8..16].copy_from_slice(&base_stateid.to_le_bytes());
        layout.stateid = stateid;

        let server_count = self.data_servers.len();
        let servers_per_mirror = server_count / self.mirror_count as usize;
        let remainder = server_count % self.mirror_count as usize;

        for mi in 0..self.mirror_count as usize {
            let start = mi * servers_per_mirror + remainder.min(mi);
            let count = servers_per_mirror + (if mi < remainder { 1 } else { 0 });
            let end = (start + count).min(server_count);

            if start >= end {
                continue;
            }

            let servers: Vec<_> = self.data_servers[start..end].to_vec();
            let mirror = FlexFileMirror::new(servers, self.stripe_unit);

            let segment = FlexFileSegment::new(0, u64::MAX, iomode, vec![mirror]);
            layout.segments.push(segment);
        }

        layout
    }

    /// Add a data server dynamically
    pub fn add_server(&mut self, server: FlexFileDataServer) {
        self.data_servers.push(server);
    }

    /// Remove a data server by address
    pub fn remove_server(&mut self, address: &str) -> bool {
        if let Some(pos) = self.data_servers.iter().position(|s| s.address == address) {
            self.data_servers.remove(pos);
            true
        } else {
            false
        }
    }

    /// Server count
    pub fn server_count(&self) -> usize {
        self.data_servers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_server(address: &str) -> FlexFileDataServer {
        FlexFileDataServer {
            address: address.to_string(),
            device_id: [0xAB; 16],
            protocol: FlexFileProtocol::Tcp,
            max_connect_attempts: 3,
        }
    }

    #[test]
    fn test_flex_file_mirror_new() {
        let servers = vec![make_server("192.168.1.1:2001")];
        let mirror = FlexFileMirror::new(servers, 65536);
        assert_eq!(mirror.server_count(), 1);
        assert_eq!(mirror.stripe_unit, 65536);
    }

    #[test]
    fn test_flex_file_mirror_is_valid_stripe_unit() {
        assert!(FlexFileMirror::is_valid_stripe_unit(4096));
        assert!(FlexFileMirror::is_valid_stripe_unit(65536));
        assert!(FlexFileMirror::is_valid_stripe_unit(1048576));
        assert!(!FlexFileMirror::is_valid_stripe_unit(4095));
        assert!(!FlexFileMirror::is_valid_stripe_unit(10000));
        assert!(!FlexFileMirror::is_valid_stripe_unit(2048));
    }

    #[test]
    fn test_flex_file_segment_new() {
        let servers = vec![make_server("192.168.1.1:2001")];
        let mirrors = vec![FlexFileMirror::new(servers, 65536)];
        let segment = FlexFileSegment::new(0, 1_000_000, IoMode::Read, mirrors);

        assert_eq!(segment.offset, 0);
        assert_eq!(segment.length, 1_000_000);
        assert_eq!(segment.iomode, IoMode::Read);
        assert_eq!(segment.mirrors.len(), 1);
    }

    #[test]
    fn test_flex_file_segment_contains_offset() {
        let servers = vec![make_server("192.168.1.1:2001")];
        let mirrors = vec![FlexFileMirror::new(servers, 65536)];
        let segment = FlexFileSegment::new(1000, 5000, IoMode::Read, mirrors);

        assert!(segment.contains_offset(1000));
        assert!(segment.contains_offset(2500));
        assert!(segment.contains_offset(5999));
        assert!(!segment.contains_offset(999));
        assert!(!segment.contains_offset(6000));
    }

    #[test]
    fn test_flex_file_segment_contains_offset_unlimited() {
        let servers = vec![make_server("192.168.1.1:2001")];
        let mirrors = vec![FlexFileMirror::new(servers, 65536)];
        let segment = FlexFileSegment::new(1000, u64::MAX, IoMode::Read, mirrors);

        assert!(segment.contains_offset(1000));
        assert!(segment.contains_offset(u64::MAX / 2));
        assert!(!segment.contains_offset(999));
    }

    #[test]
    fn test_flex_file_segment_total_server_count() {
        let servers = vec![
            make_server("192.168.1.1:2001"),
            make_server("192.168.1.2:2001"),
        ];
        let mirrors = vec![
            FlexFileMirror::new(servers, 65536),
            FlexFileMirror::new(vec![make_server("192.168.1.3:2001")], 65536),
        ];
        let segment = FlexFileSegment::new(0, 1_000_000, IoMode::Read, mirrors);

        assert_eq!(segment.total_server_count(), 3);
    }

    #[test]
    fn test_flex_file_layout_new() {
        let layout = FlexFileLayout::new(12345);
        assert_eq!(layout.inode, 12345);
        assert!(layout.segments.is_empty());
    }

    #[test]
    fn test_flex_file_layout_add_segment() {
        let mut layout = FlexFileLayout::new(123);
        let servers = vec![make_server("192.168.1.1:2001")];
        let mirrors = vec![FlexFileMirror::new(servers, 65536)];
        let segment = FlexFileSegment::new(0, 1_000_000, IoMode::Read, mirrors);

        layout.add_segment(segment);
        assert_eq!(layout.segment_count(), 1);
    }

    #[test]
    fn test_flex_file_layout_segments_for_range() {
        let mut layout = FlexFileLayout::new(123);
        let servers = vec![make_server("192.168.1.1:2001")];
        let mirrors = vec![FlexFileMirror::new(servers.clone(), 65536)];

        layout.add_segment(FlexFileSegment::new(0, 1000, IoMode::Read, mirrors.clone()));

        let mirrors2 = vec![FlexFileMirror::new(servers, 65536)];
        layout.add_segment(FlexFileSegment::new(2000, 1000, IoMode::Read, mirrors2));

        let segs = layout.segments_for_range(500, 100);
        assert_eq!(segs.len(), 1);

        let segs = layout.segments_for_range(0, 5000);
        assert_eq!(segs.len(), 2);
    }

    #[test]
    fn test_flex_file_layout_total_bytes() {
        let mut layout = FlexFileLayout::new(123);
        let servers = vec![make_server("192.168.1.1:2001")];
        let mirrors = vec![FlexFileMirror::new(servers, 65536)];

        layout.add_segment(FlexFileSegment::new(0, 1000, IoMode::Read, mirrors.clone()));
        assert_eq!(layout.total_bytes(), 1000);

        layout.add_segment(FlexFileSegment::new(0, u64::MAX, IoMode::Read, mirrors));
        assert_eq!(layout.total_bytes(), u64::MAX);
    }

    #[test]
    fn test_flex_file_layout_server_new() {
        let servers = vec![make_server("192.168.1.1:2001")];
        let server = FlexFileLayoutServer::new(servers, 65536, 1).unwrap();
        assert_eq!(server.server_count(), 1);
    }

    #[test]
    fn test_flex_file_layout_server_invalid_stripe_unit() {
        let servers = vec![make_server("192.168.1.1:2001")];
        let result = FlexFileLayoutServer::new(servers, 1000, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_flex_file_layout_server_invalid_mirror_count() {
        let servers = vec![make_server("192.168.1.1:2001")];
        let result = FlexFileLayoutServer::new(servers, 65536, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_flex_file_layout_server_no_servers() {
        let result = FlexFileLayoutServer::new(vec![], 65536, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_flex_file_layout_server_get_layout() {
        let servers = vec![
            make_server("192.168.1.1:2001"),
            make_server("192.168.1.2:2001"),
        ];
        let server = FlexFileLayoutServer::new(servers, 65536, 1).unwrap();
        let layout = server.get_layout(123, IoMode::Read);

        assert_eq!(layout.inode, 123);
        assert!(!layout.segments.is_empty());
    }

    #[test]
    fn test_flex_file_layout_server_add_server() {
        let servers = vec![make_server("192.168.1.1:2001")];
        let mut server = FlexFileLayoutServer::new(servers, 65536, 1).unwrap();

        server.add_server(make_server("192.168.1.2:2001"));
        assert_eq!(server.server_count(), 2);
    }

    #[test]
    fn test_flex_file_layout_server_remove_server() {
        let servers = vec![
            make_server("192.168.1.1:2001"),
            make_server("192.168.1.2:2001"),
        ];
        let mut server = FlexFileLayoutServer::new(servers, 65536, 1).unwrap();

        assert!(server.remove_server("192.168.1.1:2001"));
        assert_eq!(server.server_count(), 1);

        assert!(!server.remove_server("192.168.1.99:2001"));
    }
}
