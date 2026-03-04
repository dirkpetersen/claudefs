//! pNFS Data Layout Protocol Types.
//!
//! pNFS (parallel NFS, RFC 5661) allows NFS clients to directly access storage
//! servers for data I/O, bypassing the MDS (metadata server). The layout tells
//! the client which nodes hold which EC stripes. A7 (gateway agent) uses this
//! to serve pNFS LAYOUTGET/LAYOUTRETURN.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// pNFS layout type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayoutType {
    /// File layout (RFC 5661 Appendix B) — stripe-based.
    File,
    /// Block layout (RFC 5663) — block device.
    Block,
    /// Object layout (RFC 5664) — object storage.
    Object,
    /// ClaudeFS-specific layout with EC metadata.
    CfsErasure,
}

/// Numeric tag for layout type (for wire format).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutTypeTag(pub u32);

impl LayoutTypeTag {
    /// File layout type tag.
    pub const FILE: Self = Self(1);
    /// Block layout type tag.
    pub const BLOCK: Self = Self(2);
    /// Object layout type tag.
    pub const OBJECT: Self = Self(3);
    /// ClaudeFS-specific erasure-coded layout.
    pub const CFS_ERASURE: Self = Self(0x1CF5);
}

/// Identifies a storage device for pNFS.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId {
    /// Filesystem identifier.
    pub fsid: u64,
    /// Index within the filesystem's device list.
    pub device_index: u32,
}

/// Address of a storage device (node + port).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceAddr {
    /// Hostname or IP address.
    pub host: String,
    /// Port number.
    pub port: u16,
    /// Device identifier.
    pub device_id: DeviceId,
}

/// How the stripe units map to storage devices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StripePattern {
    /// Bytes per stripe unit, e.g. 1MB.
    pub stripe_unit_size: u64,
    /// k in k+m erasure coding (data stripe count).
    pub data_stripe_count: u32,
    /// m in k+m erasure coding (parity stripe count).
    pub parity_stripe_count: u32,
    /// Devices in stripe order: [data0, data1, ..., dataN, par0, par1, ...].
    pub devices: Vec<DeviceAddr>,
}

impl StripePattern {
    /// Returns total stripe count (data + parity).
    pub fn total_stripe_count(&self) -> u32 {
        self.data_stripe_count + self.parity_stripe_count
    }

    /// Returns true if the stripe pattern is valid.
    ///
    /// A pattern is valid if the number of devices matches the total stripe count.
    pub fn is_valid(&self) -> bool {
        self.devices.len() == self.total_stripe_count() as usize
    }

    /// Returns the data devices slice.
    pub fn data_devices(&self) -> &[DeviceAddr] {
        let count = self.data_stripe_count as usize;
        &self.devices[..count.min(self.devices.len())]
    }

    /// Returns the parity devices slice.
    pub fn parity_devices(&self) -> &[DeviceAddr] {
        let data_count = self.data_stripe_count as usize;
        if self.devices.len() <= data_count {
            &self.devices[0..0]
        } else {
            &self.devices[data_count..]
        }
    }

    /// Returns the device index for a given byte offset.
    ///
    /// The offset is mapped to a stripe unit, then to the device index.
    pub fn device_for_offset(&self, byte_offset: u64) -> usize {
        if self.stripe_unit_size == 0 || self.data_stripe_count == 0 {
            return 0;
        }
        let stripe_unit = byte_offset / self.stripe_unit_size;
        (stripe_unit % self.data_stripe_count as u64) as usize
    }
}

/// I/O mode for a layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IoMode {
    /// Read-only layout.
    Read,
    /// Read-write layout.
    ReadWrite,
}

/// A pNFS layout segment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutSegment {
    /// Byte offset within the file.
    pub offset: u64,
    /// Length of this segment (u64::MAX = to end of file).
    pub length: u64,
    /// I/O mode for this segment.
    pub iomode: IoMode,
    /// Stripe pattern for this segment.
    pub stripe: StripePattern,
}

/// Opaque state identifier for a layout grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutStateId {
    /// Sequence ID for this state.
    pub seqid: u32,
    /// Opaque 12 bytes per RFC 5661 §12.5.2.
    pub other: [u8; 12],
}

impl LayoutStateId {
    /// Creates a new layout state ID.
    pub fn new(seqid: u32, other: [u8; 12]) -> Self {
        Self { seqid, other }
    }

    /// Bumps the sequence ID.
    pub fn bump_seqid(&mut self) {
        self.seqid = self.seqid.wrapping_add(1);
    }
}

/// A complete pNFS layout response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataLayout {
    /// Layout type tag.
    pub layout_type: LayoutTypeTag,
    /// Layout segments.
    pub segments: Vec<LayoutSegment>,
    /// Whether to return layout on file close.
    pub return_on_close: bool,
    /// State ID for this layout grant.
    pub stateid: LayoutStateId,
}

/// Errors in layout operations.
#[derive(Debug, Error)]
pub enum LayoutError {
    /// Layout type is not supported.
    #[error("Layout type {0:?} not supported")]
    UnsupportedLayoutType(String),
    /// Invalid stripe pattern.
    #[error("Invalid stripe pattern: {reason}")]
    InvalidStripePattern {
        /// Reason for the error.
        reason: String,
    },
    /// No layout for the requested offset.
    #[error("No layout for offset {offset}")]
    NoLayoutForOffset {
        /// The requested offset.
        offset: u64,
    },
    /// Layout has expired.
    #[error("Layout expired")]
    LayoutExpired,
    /// Conflicting layout.
    #[error("Conflicting layout: {reason}")]
    ConflictingLayout {
        /// Reason for the conflict.
        reason: String,
    },
}

/// Manages layouts for multiple files (MDS-side layout cache).
#[derive(Debug)]
pub struct LayoutCache {
    /// Map of inode to layout segments.
    layouts: HashMap<u64, Vec<LayoutSegment>>,
    /// Map of inode to current state ID.
    stateids: HashMap<u64, LayoutStateId>,
    /// Next sequence ID to assign.
    next_seqid: u32,
}

impl LayoutCache {
    /// Creates a new empty layout cache.
    pub fn new() -> Self {
        Self {
            layouts: HashMap::new(),
            stateids: HashMap::new(),
            next_seqid: 1,
        }
    }

    /// Grants a layout for an inode.
    ///
    /// Returns the state ID for the granted layout.
    pub fn grant_layout(&mut self, inode: u64, segment: LayoutSegment) -> LayoutStateId {
        let seqid = self.next_seqid;
        self.next_seqid = self.next_seqid.wrapping_add(1);

        let other = [0u8; 12];
        let stateid = LayoutStateId::new(seqid, other);

        self.layouts.insert(inode, vec![segment]);
        self.stateids.insert(inode, stateid.clone());

        stateid
    }

    /// Returns the layout segments for an inode, if any.
    pub fn get_layout(&self, inode: u64) -> Option<&[LayoutSegment]> {
        self.layouts.get(&inode).map(|v| v.as_slice())
    }

    /// Returns a layout for an inode.
    ///
    /// Returns true if the layout was removed.
    /// Validates that the state ID matches.
    pub fn return_layout(&mut self, inode: u64, stateid: &LayoutStateId) -> bool {
        if let Some(current_stateid) = self.stateids.get(&inode) {
            if current_stateid == stateid {
                self.layouts.remove(&inode);
                self.stateids.remove(&inode);
                return true;
            }
        }
        false
    }

    /// Returns true if a layout is currently granted for the inode.
    pub fn is_granted(&self, inode: u64) -> bool {
        self.layouts.contains_key(&inode)
    }

    /// Returns the number of currently granted layouts.
    pub fn layout_count(&self) -> usize {
        self.layouts.len()
    }

    /// Recalls all layouts and returns the inodes whose layouts were removed.
    pub fn recall_all(&mut self) -> Vec<u64> {
        let inodes: Vec<u64> = self.layouts.keys().copied().collect();
        self.layouts.clear();
        self.stateids.clear();
        inodes
    }

    /// Returns the state ID for an inode, if any.
    pub fn get_stateid(&self, inode: u64) -> Option<&LayoutStateId> {
        self.stateids.get(&inode)
    }
}

impl Default for LayoutCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_device_addr(host: &str, port: u16, fsid: u64, idx: u32) -> DeviceAddr {
        DeviceAddr {
            host: host.to_string(),
            port,
            device_id: DeviceId {
                fsid,
                device_index: idx,
            },
        }
    }

    fn make_stripe_pattern(data: u32, parity: u32) -> StripePattern {
        let mut devices = Vec::new();
        for i in 0..(data + parity) {
            devices.push(make_device_addr("10.0.0.1", 9400 + i as u16, 1, i));
        }
        StripePattern {
            stripe_unit_size: 1024 * 1024,
            data_stripe_count: data,
            parity_stripe_count: parity,
            devices,
        }
    }

    #[test]
    fn test_layout_type_tag_constants() {
        assert_eq!(LayoutTypeTag::FILE.0, 1);
        assert_eq!(LayoutTypeTag::BLOCK.0, 2);
        assert_eq!(LayoutTypeTag::OBJECT.0, 3);
        assert_eq!(LayoutTypeTag::CFS_ERASURE.0, 0x1CF5);
    }

    #[test]
    fn test_device_id_creation() {
        let id = DeviceId {
            fsid: 42,
            device_index: 5,
        };
        assert_eq!(id.fsid, 42);
        assert_eq!(id.device_index, 5);
    }

    #[test]
    fn test_device_addr_creation() {
        let addr = make_device_addr("10.0.0.1", 9400, 1, 0);
        assert_eq!(addr.host, "10.0.0.1");
        assert_eq!(addr.port, 9400);
        assert_eq!(addr.device_id.fsid, 1);
        assert_eq!(addr.device_id.device_index, 0);
    }

    #[test]
    fn test_stripe_pattern_total_count() {
        let pattern = make_stripe_pattern(4, 2);
        assert_eq!(pattern.total_stripe_count(), 6);
    }

    #[test]
    fn test_stripe_pattern_is_valid() {
        let pattern = make_stripe_pattern(4, 2);
        assert!(pattern.is_valid());

        let invalid_pattern = StripePattern {
            stripe_unit_size: 1024 * 1024,
            data_stripe_count: 4,
            parity_stripe_count: 2,
            devices: vec![make_device_addr("10.0.0.1", 9400, 1, 0); 3],
        };
        assert!(!invalid_pattern.is_valid());
    }

    #[test]
    fn test_stripe_pattern_data_devices() {
        let pattern = make_stripe_pattern(4, 2);
        let data = pattern.data_devices();
        assert_eq!(data.len(), 4);
    }

    #[test]
    fn test_stripe_pattern_parity_devices() {
        let pattern = make_stripe_pattern(4, 2);
        let parity = pattern.parity_devices();
        assert_eq!(parity.len(), 2);
    }

    #[test]
    fn test_stripe_pattern_device_for_offset() {
        let mut pattern = make_stripe_pattern(4, 2);
        pattern.stripe_unit_size = 1024 * 1024;

        assert_eq!(pattern.device_for_offset(0), 0);
        assert_eq!(pattern.device_for_offset(1024 * 1024), 1);
        assert_eq!(pattern.device_for_offset(2 * 1024 * 1024), 2);
        assert_eq!(pattern.device_for_offset(3 * 1024 * 1024), 3);
        assert_eq!(pattern.device_for_offset(4 * 1024 * 1024), 0);
        assert_eq!(pattern.device_for_offset(5 * 1024 * 1024), 1);
    }

    #[test]
    fn test_stripe_pattern_device_for_offset_zero_unit() {
        let pattern = StripePattern {
            stripe_unit_size: 0,
            data_stripe_count: 4,
            parity_stripe_count: 2,
            devices: vec![],
        };
        assert_eq!(pattern.device_for_offset(1000), 0);
    }

    #[test]
    fn test_io_mode() {
        assert_ne!(IoMode::Read, IoMode::ReadWrite);
    }

    #[test]
    fn test_layout_state_id_new() {
        let other = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        let stateid = LayoutStateId::new(42, other);
        assert_eq!(stateid.seqid, 42);
        assert_eq!(stateid.other, other);
    }

    #[test]
    fn test_layout_state_id_bump_seqid() {
        let other = [0u8; 12];
        let mut stateid = LayoutStateId::new(1, other);
        stateid.bump_seqid();
        assert_eq!(stateid.seqid, 2);
        stateid.bump_seqid();
        assert_eq!(stateid.seqid, 3);
    }

    #[test]
    fn test_layout_state_id_wrapping() {
        let other = [0u8; 12];
        let mut stateid = LayoutStateId::new(u32::MAX, other);
        stateid.bump_seqid();
        assert_eq!(stateid.seqid, 0);
    }

    #[test]
    fn test_layout_cache_new() {
        let cache = LayoutCache::new();
        assert_eq!(cache.layout_count(), 0);
    }

    #[test]
    fn test_layout_cache_grant() {
        let mut cache = LayoutCache::new();
        let pattern = make_stripe_pattern(4, 2);
        let segment = LayoutSegment {
            offset: 0,
            length: 1024 * 1024,
            iomode: IoMode::ReadWrite,
            stripe: pattern,
        };

        let stateid = cache.grant_layout(1, segment);
        assert_eq!(cache.layout_count(), 1);
        assert!(cache.is_granted(1));
        assert!(cache.get_layout(1).is_some());
        assert_eq!(cache.get_stateid(1), Some(&stateid));
    }

    #[test]
    fn test_layout_cache_get_layout() {
        let mut cache = LayoutCache::new();
        let pattern = make_stripe_pattern(4, 2);
        let segment = LayoutSegment {
            offset: 0,
            length: 1024 * 1024,
            iomode: IoMode::Read,
            stripe: pattern,
        };

        cache.grant_layout(1, segment.clone());

        let segments = cache.get_layout(1).unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].offset, 0);
        assert_eq!(segments[0].iomode, IoMode::Read);
    }

    #[test]
    fn test_layout_cache_return_layout() {
        let mut cache = LayoutCache::new();
        let pattern = make_stripe_pattern(4, 2);
        let segment = LayoutSegment {
            offset: 0,
            length: 1024 * 1024,
            iomode: IoMode::ReadWrite,
            stripe: pattern,
        };

        let stateid = cache.grant_layout(1, segment);
        assert!(cache.is_granted(1));

        let removed = cache.return_layout(1, &stateid);
        assert!(removed);
        assert!(!cache.is_granted(1));
        assert_eq!(cache.layout_count(), 0);
    }

    #[test]
    fn test_layout_cache_return_wrong_stateid() {
        let mut cache = LayoutCache::new();
        let pattern = make_stripe_pattern(4, 2);
        let segment = LayoutSegment {
            offset: 0,
            length: 1024 * 1024,
            iomode: IoMode::ReadWrite,
            stripe: pattern,
        };

        cache.grant_layout(1, segment);

        let wrong_stateid = LayoutStateId::new(999, [0u8; 12]);
        let removed = cache.return_layout(1, &wrong_stateid);
        assert!(!removed);
        assert!(cache.is_granted(1));
    }

    #[test]
    fn test_layout_cache_return_nonexistent() {
        let mut cache = LayoutCache::new();
        let stateid = LayoutStateId::new(1, [0u8; 12]);
        let removed = cache.return_layout(999, &stateid);
        assert!(!removed);
    }

    #[test]
    fn test_layout_cache_is_granted() {
        let mut cache = LayoutCache::new();

        assert!(!cache.is_granted(1));

        let pattern = make_stripe_pattern(4, 2);
        let segment = LayoutSegment {
            offset: 0,
            length: 1024 * 1024,
            iomode: IoMode::ReadWrite,
            stripe: pattern,
        };
        cache.grant_layout(1, segment);

        assert!(cache.is_granted(1));
        assert!(!cache.is_granted(2));
    }

    #[test]
    fn test_layout_cache_recall_all() {
        let mut cache = LayoutCache::new();
        let pattern = make_stripe_pattern(4, 2);
        let segment = LayoutSegment {
            offset: 0,
            length: 1024 * 1024,
            iomode: IoMode::ReadWrite,
            stripe: pattern,
        };

        cache.grant_layout(1, segment.clone());
        cache.grant_layout(2, segment.clone());
        cache.grant_layout(3, segment);

        assert_eq!(cache.layout_count(), 3);

        let recalled = cache.recall_all();
        assert_eq!(recalled.len(), 3);
        assert!(recalled.contains(&1));
        assert!(recalled.contains(&2));
        assert!(recalled.contains(&3));
        assert_eq!(cache.layout_count(), 0);
    }

    #[test]
    fn test_layout_error_display() {
        let err = LayoutError::UnsupportedLayoutType("Unknown".to_string());
        assert_eq!(format!("{}", err), "Layout type \"Unknown\" not supported");

        let err = LayoutError::InvalidStripePattern {
            reason: "device count mismatch".to_string(),
        };
        assert_eq!(
            format!("{}", err),
            "Invalid stripe pattern: device count mismatch"
        );

        let err = LayoutError::NoLayoutForOffset { offset: 1024 };
        assert_eq!(format!("{}", err), "No layout for offset 1024");

        let err = LayoutError::LayoutExpired;
        assert_eq!(format!("{}", err), "Layout expired");

        let err = LayoutError::ConflictingLayout {
            reason: "write conflict".to_string(),
        };
        assert_eq!(format!("{}", err), "Conflicting layout: write conflict");
    }

    #[test]
    fn test_device_id_serde() {
        let id = DeviceId {
            fsid: 42,
            device_index: 5,
        };
        let json = serde_json::to_string(&id).unwrap();
        let decoded: DeviceId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, decoded);
    }

    #[test]
    fn test_device_addr_serde() {
        let addr = make_device_addr("10.0.0.1", 9400, 1, 0);
        let json = serde_json::to_string(&addr).unwrap();
        let decoded: DeviceAddr = serde_json::from_str(&json).unwrap();
        assert_eq!(addr, decoded);
    }

    #[test]
    fn test_stripe_pattern_serde() {
        let pattern = make_stripe_pattern(4, 2);
        let json = serde_json::to_string(&pattern).unwrap();
        let decoded: StripePattern = serde_json::from_str(&json).unwrap();
        assert_eq!(pattern.data_stripe_count, decoded.data_stripe_count);
        assert_eq!(pattern.parity_stripe_count, decoded.parity_stripe_count);
        assert_eq!(pattern.devices.len(), decoded.devices.len());
    }

    #[test]
    fn test_io_mode_serde() {
        let mode = IoMode::ReadWrite;
        let json = serde_json::to_string(&mode).unwrap();
        let decoded: IoMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, decoded);
    }

    #[test]
    fn test_layout_state_id_serde() {
        let stateid = LayoutStateId::new(42, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        let json = serde_json::to_string(&stateid).unwrap();
        let decoded: LayoutStateId = serde_json::from_str(&json).unwrap();
        assert_eq!(stateid, decoded);
    }

    #[test]
    fn test_data_layout_serde() {
        let pattern = make_stripe_pattern(4, 2);
        let segment = LayoutSegment {
            offset: 0,
            length: 1024 * 1024,
            iomode: IoMode::ReadWrite,
            stripe: pattern,
        };
        let layout = DataLayout {
            layout_type: LayoutTypeTag::CFS_ERASURE,
            segments: vec![segment],
            return_on_close: true,
            stateid: LayoutStateId::new(1, [0u8; 12]),
        };
        let json = serde_json::to_string(&layout).unwrap();
        let decoded: DataLayout = serde_json::from_str(&json).unwrap();
        assert_eq!(layout.layout_type, decoded.layout_type);
        assert_eq!(layout.return_on_close, decoded.return_on_close);
    }

    #[test]
    fn test_layout_type_serde() {
        let layout_type = LayoutType::CfsErasure;
        let json = serde_json::to_string(&layout_type).unwrap();
        let decoded: LayoutType = serde_json::from_str(&json).unwrap();
        assert_eq!(layout_type, decoded);
    }

    #[test]
    fn test_layout_type_tag_serde() {
        let tag = LayoutTypeTag::CFS_ERASURE;
        let json = serde_json::to_string(&tag).unwrap();
        let decoded: LayoutTypeTag = serde_json::from_str(&json).unwrap();
        assert_eq!(tag, decoded);
    }
}
