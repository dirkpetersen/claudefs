//! Protocol definitions for ClaudeFS RPC communication.
//!
//! This module defines the binary wire format for all inter-node communication.

use crate::error::{Result, TransportError};
use serde::{Deserialize, Serialize};

/// Magic number identifying ClaudeFS protocol frames.
pub const MAGIC: u32 = 0xCF5F0001;

/// Current protocol version.
pub const PROTOCOL_VERSION: u8 = 1;

/// Fixed header size in bytes.
pub const FRAME_HEADER_SIZE: usize = 24;

/// Maximum allowed payload size (64 MB).
pub const MAX_PAYLOAD_SIZE: u32 = 64 * 1024 * 1024;

/// CRC32 lookup table (IEEE polynomial).
const CRC32_TABLE: [u32; 256] = [
    0x00000000, 0x77073096, 0xEE0E612C, 0x990951BA, 0x076DC419, 0x706AF48F, 0xE963A535, 0x9E6495A3,
    0x0EDB8832, 0x79DCB8A4, 0xE0D5E91E, 0x97D2D988, 0x09B64C2B, 0x7EB17CBD, 0xE7B82D07, 0x90BF1D91,
    0x1DB71064, 0x6AB020F2, 0xF3B97148, 0x84BE41DE, 0x1ADAD47D, 0x6DDDE4EB, 0xF4D4B551, 0x83D385C7,
    0x136C9856, 0x646BA8C0, 0xFD62F97A, 0x8A65C9EC, 0x14015C4F, 0x63066CD9, 0xFA0F3D63, 0x8D080DF5,
    0x3B6E20C8, 0x4C69105E, 0xD56041E4, 0xA2677172, 0x3C03E4D1, 0x4B04D447, 0xD20D85FD, 0xA50AB56B,
    0x35B5A8FA, 0x42B2986C, 0xDBBBC9D6, 0xACBCF940, 0x32D86CE3, 0x45DF5C75, 0xDCD60DCF, 0xABD13D59,
    0x26D930AC, 0x51DE003A, 0xC8D75180, 0xBFD06116, 0x21B4F4B5, 0x56B3C423, 0xCFBA9599, 0xB8BDA50F,
    0x2802B89E, 0x5F058808, 0xC60CD9B2, 0xB10BE924, 0x2F6F7C87, 0x58684C11, 0xC1611DAB, 0xB6662D3D,
    0x76DC4190, 0x01DB7106, 0x98D220BC, 0xEFD5102A, 0x71B18589, 0x06B6B51F, 0x9FBFE4A5, 0xE8B8D433,
    0x7807C9A2, 0x0F00F934, 0x9609A88E, 0xE10E9818, 0x7F6A0DBB, 0x086D3D2D, 0x91646C97, 0xE6635C01,
    0x6B6B51F4, 0x1C6C6162, 0x856530D8, 0xF262004E, 0x6C0695ED, 0x1B01A57B, 0x8208F4C1, 0xF50FC457,
    0x65B0D9C6, 0x12B7E950, 0x8BBEB8EA, 0xFCB9887C, 0x62DD1DDF, 0x15DA2D49, 0x8CD37CF3, 0xFBD44C65,
    0x4DB26158, 0x3AB551CE, 0xA3BC0074, 0xD4BB30E2, 0x4ADFA541, 0x3DD895D7, 0xA4D1C46D, 0xD3D6F4FB,
    0x4369E96A, 0x346ED9FC, 0xAD678846, 0xDA60B8D0, 0x44042D73, 0x33031DE5, 0xAA0A4C5F, 0xDD0D7CC9,
    0x5005713C, 0x270241AA, 0xBE0B1010, 0xC90C2086, 0x5768B525, 0x206F85B3, 0xB966D409, 0xCE61E49F,
    0x5EDEF90E, 0x29D9C998, 0xB0D09822, 0xC7D7A8B4, 0x59B33D17, 0x2EB40D81, 0xB7BD5C3B, 0xC0BA6CAD,
    0xEDB88320, 0x9ABFB3B6, 0x03B6E20C, 0x74B1D29A, 0xEAD54739, 0x9DD277AF, 0x04DB2615, 0x73DC1683,
    0xE3630B12, 0x94643B84, 0x0D6D6A3E, 0x7A6A5AA8, 0xE40ECF0B, 0x9309FF9D, 0x0A00AE27, 0x7D079EB1,
    0xF00F9344, 0x8708A3D2, 0x1E01F268, 0x6906C2FE, 0xF762575D, 0x806567CB, 0x196C3671, 0x6E6B06E7,
    0xFED41B76, 0x89D32BE0, 0x10DA7A5A, 0x67DD4ACC, 0xF9B9DF6F, 0x8EBEEFF9, 0x17B7BE43, 0x60B08ED5,
    0xD6D6A3E8, 0xA1D1937E, 0x38D8C2C4, 0x4FDFF252, 0xD1BB67F1, 0xA6BC5767, 0x3FB506DD, 0x48B2364B,
    0xD80D2BDA, 0xAF0A1B4C, 0x36034AF6, 0x41047A60, 0xDF60EFC3, 0xA867DF55, 0x316E8EEF, 0x4669BE79,
    0xCB61B38C, 0xBC66831A, 0x256FD2A0, 0x5268E236, 0xCC0C7795, 0xBB0B4703, 0x220216B9, 0x5505262F,
    0xC5BA3BBE, 0xB2BD0B28, 0x2BB45A92, 0x5CB36A04, 0xC2D7FFA7, 0xB5D0CF31, 0x2CD99E8B, 0x5BDEAE1D,
    0x9B64C2B0, 0xEC63F226, 0x756AA39C, 0x026D930A, 0x9C0906A9, 0xEB0E363F, 0x72076785, 0x05005713,
    0x95BF4A82, 0xE2B87A14, 0x7BB12BAE, 0x0CB61B38, 0x92D28E9B, 0xE5D5BE0D, 0x7CDCEFB7, 0x0BDBDF21,
    0x86D3D2D4, 0xF1D4E242, 0x68DDB3F8, 0x1FDA836E, 0x81BE16CD, 0xF6B9265B, 0x6FB077E1, 0x18B74777,
    0x88085AE6, 0xFF0F6A70, 0x66063BCA, 0x11010B5C, 0x8F659EFF, 0xF862AE69, 0x616BFFD3, 0x166CCF45,
    0xA00AE278, 0xD70DD2EE, 0x4E048354, 0x3903B3C2, 0xA7672661, 0xD06016F7, 0x4969474D, 0x3E6E77DB,
    0xAED16A4A, 0xD9D65ADC, 0x40DF0B66, 0x37D83BF0, 0xA9BCAE53, 0xDEBB9EC5, 0x47B2CF7F, 0x30B5FFE9,
    0xBDBDF21C, 0xCABAC28A, 0x53B39330, 0x24B4A3A6, 0xBAD03605, 0xCDD70693, 0x54DE5729, 0x23D967BF,
    0xB3667A2E, 0xC4614AB8, 0x5D681B02, 0x2A6F2B94, 0xB40BBE37, 0xC30C8EA1, 0x5A05DF1B, 0x2D02EF8D,
];

/// Compute CRC32 checksum using IEEE polynomial.
#[inline]
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for &byte in data {
        let index = ((crc ^ byte as u32) & 0xFF) as usize;
        crc = (crc >> 8) ^ CRC32_TABLE[index];
    }
    crc ^ 0xFFFFFFFF
}

/// Frame flags indicating special frame properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameFlags(u8);

impl FrameFlags {
    /// Frame payload is compressed.
    pub const COMPRESSED: FrameFlags = FrameFlags(0x01);
    /// Frame payload is encrypted.
    pub const ENCRYPTED: FrameFlags = FrameFlags(0x02);
    /// No response expected (one-way message).
    pub const ONE_WAY: FrameFlags = FrameFlags(0x04);
    /// This frame is a response to a previous request.
    pub const RESPONSE: FrameFlags = FrameFlags(0x08);

    /// Create new flags from raw value.
    pub const fn new(raw: u8) -> Self {
        FrameFlags(raw)
    }

    /// Get the raw u8 value.
    pub const fn bits(&self) -> u8 {
        self.0
    }

    /// Check if a flag is set.
    pub const fn contains(&self, flag: FrameFlags) -> bool {
        (self.0 & flag.0) != 0
    }

    /// Set a flag.
    pub const fn with(mut self, flag: FrameFlags) -> Self {
        self.0 |= flag.0;
        self
    }

    /// Clear a flag.
    pub const fn without(mut self, flag: FrameFlags) -> Self {
        self.0 &= !flag.0;
        self
    }
}

impl From<u8> for FrameFlags {
    fn from(raw: u8) -> Self {
        FrameFlags(raw)
    }
}

impl From<FrameFlags> for u8 {
    fn from(flags: FrameFlags) -> Self {
        flags.0
    }
}

impl std::ops::BitOr for FrameFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        FrameFlags(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for FrameFlags {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        FrameFlags(self.0 & rhs.0)
    }
}

/// Protocol opcodes for different operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u16)]
pub enum Opcode {
    // Metadata operations (0x01xx)
    /// Lookup a file by path.
    Lookup = 0x0101,
    /// Create a new file.
    Create = 0x0102,
    /// Create a directory.
    Mkdir = 0x0103,
    /// Remove a file.
    Unlink = 0x0104,
    /// Remove a directory.
    Rmdir = 0x0105,
    /// Rename a file or directory.
    Rename = 0x0106,
    /// Get file attributes.
    Getattr = 0x0107,
    /// Set file attributes.
    Setattr = 0x0108,
    /// Read directory entries.
    Readdir = 0x0109,
    /// Create symbolic link.
    Symlink = 0x010A,
    /// Read symbolic link target.
    Readlink = 0x010B,
    /// Create hard link.
    Link = 0x010C,
    /// Get filesystem statistics.
    Statfs = 0x010D,

    // Data operations (0x02xx)
    /// Read file data.
    Read = 0x0201,
    /// Write file data.
    Write = 0x0202,
    /// Synchronize file data.
    Fsync = 0x0203,
    /// Allocate file space.
    Fallocate = 0x0204,
    /// Open a file.
    Open = 0x0205,
    /// Close a file.
    Close = 0x0206,

    // Cluster operations (0x03xx)
    /// Heartbeat message.
    Heartbeat = 0x0301,
    /// Join cluster request.
    JoinCluster = 0x0302,
    /// Leave cluster notification.
    LeaveCluster = 0x0303,
    /// Shard information query.
    ShardInfo = 0x0304,
    /// Node status query.
    NodeStatus = 0x0305,

    // Replication operations (0x04xx)
    /// Journal synchronization.
    JournalSync = 0x0401,
    /// Journal acknowledgment.
    JournalAck = 0x0402,
    /// Snapshot transfer.
    SnapshotTransfer = 0x0403,
}

impl Opcode {
    /// Convert opcode to raw u16 value.
    pub fn into_u16(self) -> u16 {
        self as u16
    }

    /// Create opcode from raw u16 value.
    pub fn from_u16(value: u16) -> Option<Self> {
        // Use transmute-like approach via unsafe or match
        // Since we have repr(u16), we can use std::mem::transmute
        // But let's use a safe match for clarity
        match value {
            0x0101 => Some(Opcode::Lookup),
            0x0102 => Some(Opcode::Create),
            0x0103 => Some(Opcode::Mkdir),
            0x0104 => Some(Opcode::Unlink),
            0x0105 => Some(Opcode::Rmdir),
            0x0106 => Some(Opcode::Rename),
            0x0107 => Some(Opcode::Getattr),
            0x0108 => Some(Opcode::Setattr),
            0x0109 => Some(Opcode::Readdir),
            0x010A => Some(Opcode::Symlink),
            0x010B => Some(Opcode::Readlink),
            0x010C => Some(Opcode::Link),
            0x010D => Some(Opcode::Statfs),
            0x0201 => Some(Opcode::Read),
            0x0202 => Some(Opcode::Write),
            0x0203 => Some(Opcode::Fsync),
            0x0204 => Some(Opcode::Fallocate),
            0x0205 => Some(Opcode::Open),
            0x0206 => Some(Opcode::Close),
            0x0301 => Some(Opcode::Heartbeat),
            0x0302 => Some(Opcode::JoinCluster),
            0x0303 => Some(Opcode::LeaveCluster),
            0x0304 => Some(Opcode::ShardInfo),
            0x0305 => Some(Opcode::NodeStatus),
            0x0401 => Some(Opcode::JournalSync),
            0x0402 => Some(Opcode::JournalAck),
            0x0403 => Some(Opcode::SnapshotTransfer),
            _ => None,
        }
    }
}

impl From<u16> for Opcode {
    fn from(value: u16) -> Self {
        Opcode::from_u16(value).unwrap_or_else(|| panic!("Unknown opcode: 0x{:04X}", value))
    }
}

impl From<Opcode> for u16 {
    fn from(opcode: Opcode) -> Self {
        opcode.into_u16()
    }
}

/// Fixed 24-byte frame header structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    /// Magic number for protocol identification.
    pub magic: u32,
    /// Protocol version.
    pub version: u8,
    /// Frame flags.
    pub flags: FrameFlags,
    /// Operation code.
    pub opcode: u16,
    /// Unique request identifier.
    pub request_id: u64,
    /// Length of payload in bytes.
    pub payload_length: u32,
    /// CRC32 checksum of payload.
    pub checksum: u32,
}

impl FrameHeader {
    /// Create a new frame header.
    pub const fn new(
        flags: FrameFlags,
        opcode: u16,
        request_id: u64,
        payload_length: u32,
        checksum: u32,
    ) -> Self {
        FrameHeader {
            magic: MAGIC,
            version: PROTOCOL_VERSION,
            flags,
            opcode,
            request_id,
            payload_length,
            checksum,
        }
    }

    /// Encode header to 24-byte array in big-endian format.
    pub fn encode(&self) -> [u8; FRAME_HEADER_SIZE] {
        let mut bytes = [0u8; FRAME_HEADER_SIZE];

        // magic: u32 (big-endian)
        bytes[0] = ((self.magic >> 24) & 0xFF) as u8;
        bytes[1] = ((self.magic >> 16) & 0xFF) as u8;
        bytes[2] = ((self.magic >> 8) & 0xFF) as u8;
        bytes[3] = (self.magic & 0xFF) as u8;

        // version: u8
        bytes[4] = self.version;

        // flags: u8
        bytes[5] = self.flags.bits();

        // opcode: u16 (big-endian)
        bytes[6] = ((self.opcode >> 8) & 0xFF) as u8;
        bytes[7] = (self.opcode & 0xFF) as u8;

        // request_id: u64 (big-endian)
        bytes[8] = ((self.request_id >> 56) & 0xFF) as u8;
        bytes[9] = ((self.request_id >> 48) & 0xFF) as u8;
        bytes[10] = ((self.request_id >> 40) & 0xFF) as u8;
        bytes[11] = ((self.request_id >> 32) & 0xFF) as u8;
        bytes[12] = ((self.request_id >> 24) & 0xFF) as u8;
        bytes[13] = ((self.request_id >> 16) & 0xFF) as u8;
        bytes[14] = ((self.request_id >> 8) & 0xFF) as u8;
        bytes[15] = (self.request_id & 0xFF) as u8;

        // payload_length: u32 (big-endian)
        bytes[16] = ((self.payload_length >> 24) & 0xFF) as u8;
        bytes[17] = ((self.payload_length >> 16) & 0xFF) as u8;
        bytes[18] = ((self.payload_length >> 8) & 0xFF) as u8;
        bytes[19] = (self.payload_length & 0xFF) as u8;

        // checksum: u32 (big-endian)
        bytes[20] = ((self.checksum >> 24) & 0xFF) as u8;
        bytes[21] = ((self.checksum >> 16) & 0xFF) as u8;
        bytes[22] = ((self.checksum >> 8) & 0xFF) as u8;
        bytes[23] = (self.checksum & 0xFF) as u8;

        bytes
    }

    /// Decode header from byte slice.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < FRAME_HEADER_SIZE {
            return Err(TransportError::InvalidFrame {
                reason: format!(
                    "Header too short: {} bytes, expected {}",
                    bytes.len(),
                    FRAME_HEADER_SIZE
                ),
            });
        }

        let magic = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if magic != MAGIC {
            return Err(TransportError::InvalidMagic {
                expected: MAGIC,
                got: magic,
            });
        }

        let version = bytes[4];
        if version != PROTOCOL_VERSION {
            return Err(TransportError::VersionMismatch {
                expected: PROTOCOL_VERSION,
                got: version,
            });
        }

        let flags = FrameFlags::new(bytes[5]);
        let opcode = u16::from_be_bytes([bytes[6], bytes[7]]);
        let request_id = u64::from_be_bytes([
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ]);
        let payload_length = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let checksum = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);

        Ok(FrameHeader {
            magic,
            version,
            flags,
            opcode,
            request_id,
            payload_length,
            checksum,
        })
    }
}

/// A complete protocol frame with header and payload.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// Frame header.
    pub header: FrameHeader,
    /// Frame payload (serialized with bincode).
    pub payload: Vec<u8>,
}

impl Frame {
    /// Create a new frame with the given opcode, request ID, and payload.
    pub fn new(opcode: Opcode, request_id: u64, payload: Vec<u8>) -> Self {
        let payload_length = payload.len() as u32;
        let checksum = crc32(&payload);
        let header = FrameHeader::new(
            FrameFlags::default(),
            opcode.into_u16(),
            request_id,
            payload_length,
            checksum,
        );
        Frame { header, payload }
    }

    /// Validate the frame by checking payload length and checksum.
    pub fn validate(&self) -> Result<()> {
        // Check payload size
        if self.header.payload_length > MAX_PAYLOAD_SIZE {
            return Err(TransportError::PayloadTooLarge {
                size: self.header.payload_length,
                max_size: MAX_PAYLOAD_SIZE,
            });
        }

        // Verify payload length matches header
        if self.payload.len() as u32 != self.header.payload_length {
            return Err(TransportError::InvalidFrame {
                reason: format!(
                    "Payload length mismatch: header says {}, got {}",
                    self.header.payload_length,
                    self.payload.len()
                ),
            });
        }

        // Verify checksum
        let computed = crc32(&self.payload);
        if computed != self.header.checksum {
            return Err(TransportError::ChecksumMismatch {
                expected: self.header.checksum,
                computed,
            });
        }

        Ok(())
    }

    /// Check if this frame is a response.
    pub fn is_response(&self) -> bool {
        self.header.flags.contains(FrameFlags::RESPONSE)
    }

    /// Create a response frame with the given payload.
    pub fn make_response(&self, payload: Vec<u8>) -> Self {
        let payload_length = payload.len() as u32;
        let checksum = crc32(&payload);
        let header = FrameHeader::new(
            FrameFlags::RESPONSE,
            self.header.opcode,
            self.header.request_id,
            payload_length,
            checksum,
        );
        Frame { header, payload }
    }

    /// Get the opcode as an enum.
    pub fn opcode(&self) -> Opcode {
        Opcode::from(self.header.opcode)
    }

    /// Get the request ID.
    pub fn request_id(&self) -> u64 {
        self.header.request_id
    }

    /// Get the flags.
    pub fn flags(&self) -> FrameFlags {
        self.header.flags
    }

    /// Get the payload length.
    pub fn payload_length(&self) -> u32 {
        self.header.payload_length
    }

    /// Get the checksum.
    pub fn checksum(&self) -> u32 {
        self.header.checksum
    }

    /// Encode the entire frame to bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(FRAME_HEADER_SIZE + self.payload.len());
        bytes.extend_from_slice(&self.header.encode());
        bytes.extend_from_slice(&self.payload);
        bytes
    }

    /// Decode a frame from bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let header = FrameHeader::decode(bytes)?;
        let payload_start = FRAME_HEADER_SIZE;
        let payload_end = payload_start + header.payload_length as usize;

        if bytes.len() < payload_end {
            return Err(TransportError::InvalidFrame {
                reason: format!(
                    "Frame truncated: expected {} bytes, got {}",
                    payload_end,
                    bytes.len()
                ),
            });
        }

        let payload = bytes[payload_start..payload_end].to_vec();
        let frame = Frame { header, payload };
        frame.validate()?;
        Ok(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_header_encode_decode() {
        let header = FrameHeader::new(FrameFlags::RESPONSE, 0x0101, 12345, 100, 0xDEADBEEF);

        let encoded = header.encode();
        assert_eq!(encoded.len(), FRAME_HEADER_SIZE);

        let decoded = FrameHeader::decode(&encoded).unwrap();
        assert_eq!(decoded.magic, MAGIC);
        assert_eq!(decoded.version, PROTOCOL_VERSION);
        assert_eq!(decoded.flags.bits(), FrameFlags::RESPONSE.bits());
        assert_eq!(decoded.opcode, 0x0101);
        assert_eq!(decoded.request_id, 12345);
        assert_eq!(decoded.payload_length, 100);
        assert_eq!(decoded.checksum, 0xDEADBEEF);
    }

    #[test]
    fn test_frame_header_encode_decode_roundtrip() {
        let original = FrameHeader::new(
            FrameFlags::COMPRESSED | FrameFlags::ENCRYPTED,
            0x0202,
            0x123456789ABCDEF0,
            1024,
            0xABCD1234,
        );

        let encoded = original.encode();
        let decoded = FrameHeader::decode(&encoded).unwrap();

        assert_eq!(original.magic, decoded.magic);
        assert_eq!(original.version, decoded.version);
        assert_eq!(original.flags.bits(), decoded.flags.bits());
        assert_eq!(original.opcode, decoded.opcode);
        assert_eq!(original.request_id, decoded.request_id);
        assert_eq!(original.payload_length, decoded.payload_length);
        assert_eq!(original.checksum, decoded.checksum);
    }

    #[test]
    fn test_frame_new_and_validate() {
        let payload = b"Hello, ClaudeFS!".to_vec();
        let frame = Frame::new(Opcode::Read, 42, payload.clone());

        assert_eq!(frame.header.opcode, Opcode::Read as u16);
        assert_eq!(frame.header.request_id, 42);
        assert_eq!(frame.header.payload_length as usize, payload.len());
        assert!(!frame.is_response());

        frame.validate().expect("Frame should be valid");
    }

    #[test]
    fn test_frame_is_response() {
        let payload = b"test".to_vec();
        let request_frame = Frame::new(Opcode::Lookup, 1, payload.clone());
        assert!(!request_frame.is_response());

        let response_frame = request_frame.make_response(b"response".to_vec());
        assert!(response_frame.is_response());
        assert_eq!(response_frame.header.opcode, request_frame.header.opcode);
        assert_eq!(
            response_frame.header.request_id,
            request_frame.header.request_id
        );
    }

    #[test]
    fn test_frame_encode_decode_roundtrip() {
        let original_payload = b"This is a test payload for ClaudeFS transport layer!".to_vec();
        let original = Frame::new(Opcode::Write, 999, original_payload.clone());

        let encoded = original.encode();
        let decoded = Frame::decode(&encoded).unwrap();

        assert_eq!(decoded.header.opcode, original.header.opcode);
        assert_eq!(decoded.header.request_id, original.header.request_id);
        assert_eq!(decoded.payload, original.payload);
    }

    #[test]
    fn test_frame_with_flags() {
        let payload = b"test".to_vec();
        let frame = Frame::new(Opcode::Heartbeat, 1, payload);

        // Add response flag manually for testing
        let mut header = frame.header;
        header.flags = FrameFlags::RESPONSE;

        let modified_frame = Frame {
            header,
            payload: frame.payload,
        };
        assert!(modified_frame.is_response());
    }

    #[test]
    fn test_crc32() {
        // Test known CRC32 values
        assert_eq!(crc32(b""), 0x00000000);
        assert_eq!(crc32(b"a"), 0xE8B7BE43);
        assert_eq!(crc32(b"abc"), 0x352441C2);
        assert_eq!(crc32(b"hello"), 0x3610A686);
    }

    #[test]
    fn test_opcode_conversion() {
        assert_eq!(Opcode::Lookup.into_u16(), 0x0101);
        assert_eq!(Opcode::Read.into_u16(), 0x0201);
        assert_eq!(Opcode::Heartbeat.into_u16(), 0x0301);
        assert_eq!(Opcode::JournalSync.into_u16(), 0x0401);

        assert_eq!(Opcode::from(0x0101), Opcode::Lookup);
        assert_eq!(Opcode::from(0x0202), Opcode::Write);
        assert_eq!(Opcode::from(0x0301), Opcode::Heartbeat);
    }

    #[test]
    fn test_frame_flags() {
        let empty = FrameFlags::default();
        assert!(!empty.contains(FrameFlags::COMPRESSED));

        let compressed = FrameFlags::COMPRESSED;
        assert!(compressed.contains(FrameFlags::COMPRESSED));
        assert!(!compressed.contains(FrameFlags::ENCRYPTED));

        let combined = FrameFlags::COMPRESSED | FrameFlags::ENCRYPTED;
        assert!(combined.contains(FrameFlags::COMPRESSED));
        assert!(combined.contains(FrameFlags::ENCRYPTED));

        let with_flag = empty.with(FrameFlags::ONE_WAY);
        assert!(with_flag.contains(FrameFlags::ONE_WAY));

        let without_flag = with_flag.without(FrameFlags::ONE_WAY);
        assert!(!without_flag.contains(FrameFlags::ONE_WAY));
    }

    #[test]
    fn test_invalid_magic() {
        let mut header = FrameHeader::new(FrameFlags::default(), 0x0101, 1, 0, 0);
        header.magic = 0x12345678; // Invalid magic

        let encoded = header.encode();
        let result = FrameHeader::decode(&encoded);

        assert!(matches!(result, Err(TransportError::InvalidMagic { .. })));
    }

    #[test]
    fn test_invalid_version() {
        let mut header = FrameHeader::new(FrameFlags::default(), 0x0101, 1, 0, 0);
        header.version = 99; // Invalid version

        let encoded = header.encode();
        let result = FrameHeader::decode(&encoded);

        assert!(matches!(
            result,
            Err(TransportError::VersionMismatch { .. })
        ));
    }

    #[test]
    fn test_checksum_mismatch() {
        let payload = b"test data".to_vec();
        let mut frame = Frame::new(Opcode::Read, 1, payload);
        frame.header.checksum = 0x12345678; // Wrong checksum

        let result = frame.validate();
        assert!(matches!(
            result,
            Err(TransportError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn test_payload_too_large() {
        let payload = vec![0u8; (MAX_PAYLOAD_SIZE + 1) as usize];
        let frame = Frame::new(Opcode::Read, 1, payload);

        let result = frame.validate();
        assert!(matches!(
            result,
            Err(TransportError::PayloadTooLarge { .. })
        ));
    }

    #[test]
    fn test_constants() {
        assert_eq!(FRAME_HEADER_SIZE, 24);
        assert_eq!(MAGIC, 0xCF5F0001);
        assert_eq!(PROTOCOL_VERSION, 1);
        assert_eq!(MAX_PAYLOAD_SIZE, 64 * 1024 * 1024);
    }
}

#[cfg(test)]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Any valid frame can be encoded and decoded back identically.
        #[test]
        fn frame_roundtrip(
            opcode_val in prop::sample::select(vec![
                0x0101u16, 0x0102, 0x0103, 0x0104, 0x0105, 0x0106, 0x0107, 0x0108,
                0x0109, 0x010A, 0x010B, 0x010C, 0x010D,
                0x0201, 0x0202, 0x0203, 0x0204, 0x0205, 0x0206,
                0x0301, 0x0302, 0x0303, 0x0304, 0x0305,
                0x0401, 0x0402, 0x0403,
            ]),
            request_id in any::<u64>(),
            payload in proptest::collection::vec(any::<u8>(), 0..1024),
        ) {
            let opcode = Opcode::from_u16(opcode_val).unwrap();
            let frame = Frame::new(opcode, request_id, payload.clone());
            let encoded = frame.encode();
            let decoded = Frame::decode(&encoded).unwrap();
            prop_assert_eq!(decoded.header.opcode, frame.header.opcode);
            prop_assert_eq!(decoded.header.request_id, frame.header.request_id);
            prop_assert_eq!(&decoded.payload, &frame.payload);
        }

        /// Frame header roundtrip for arbitrary flag combinations.
        #[test]
        fn frame_header_roundtrip(
            flags_raw in any::<u8>(),
            opcode in 0x0101u16..=0x0403,
            request_id in any::<u64>(),
            payload_length in 0u32..MAX_PAYLOAD_SIZE,
            checksum in any::<u32>(),
        ) {
            let header = FrameHeader::new(
                FrameFlags::new(flags_raw),
                opcode,
                request_id,
                payload_length,
                checksum,
            );
            let encoded = header.encode();
            let decoded = FrameHeader::decode(&encoded).unwrap();
            prop_assert_eq!(decoded.flags.bits(), flags_raw);
            prop_assert_eq!(decoded.opcode, opcode);
            prop_assert_eq!(decoded.request_id, request_id);
            prop_assert_eq!(decoded.payload_length, payload_length);
            prop_assert_eq!(decoded.checksum, checksum);
        }

        /// CRC32 is deterministic: same input always gives same output.
        #[test]
        fn crc32_deterministic(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
            let c1 = crc32(&data);
            let c2 = crc32(&data);
            prop_assert_eq!(c1, c2);
        }

        /// CRC32 changes when data changes (collision resistance).
        #[test]
        fn crc32_changes_on_mutation(
            data in proptest::collection::vec(any::<u8>(), 1..256),
            bit_index in 0usize..8,
        ) {
            let mut mutated = data.clone();
            mutated[0] ^= 1 << bit_index;
            if data != mutated {
                let c1 = crc32(&data);
                let c2 = crc32(&mutated);
                prop_assert_ne!(c1, c2);
            }
        }

        /// FrameFlags set/clear operations are idempotent.
        #[test]
        fn frame_flags_set_clear_idempotent(raw in any::<u8>()) {
            let flags = FrameFlags::new(raw);
            let with_compressed = flags.with(FrameFlags::COMPRESSED);
            let double_set = with_compressed.with(FrameFlags::COMPRESSED);
            prop_assert_eq!(with_compressed.bits(), double_set.bits());

            let without_compressed = flags.without(FrameFlags::COMPRESSED);
            let double_clear = without_compressed.without(FrameFlags::COMPRESSED);
            prop_assert_eq!(without_compressed.bits(), double_clear.bits());
        }
    }
}
