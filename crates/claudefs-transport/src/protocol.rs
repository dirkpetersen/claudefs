//! Protocol definitions and serialization for transport layer
//!
//! This module provides the binary RPC protocol for ClaudeFS transport.
//! The protocol uses frame-based messaging with a fixed 24-byte header.
//!
//! NOTE: This is a skeleton module with type definitions for reference.
//! Full implementation will be completed by A4 (Transport) agent.

/// Frame header size in bytes (magic:4 + version:1 + flags:1 + opcode:2 + request_id:8 + payload_length:4 + checksum:4)
pub const FRAME_HEADER_SIZE: usize = 24;

/// Protocol magic number for frame validation
pub const MAGIC: u32 = 0xCF5F0001;

/// Protocol version
pub const PROTOCOL_VERSION: u8 = 1;

/// Supported operations in the RPC protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Opcode {
    /// Lookup operation
    Lookup = 0x0101,
    /// Lookup response
    LookupResponse = 0x0102,
    /// Read operation
    Read = 0x0117,
    /// Read response
    ReadResponse = 0x0118,
    /// Write operation
    Write = 0x0119,
    /// Write response
    WriteResponse = 0x011A,
}

/// Frame flags for protocol control
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameFlags {
    /// Payload is compressed
    pub compressed: bool,
    /// Payload is encrypted
    pub encrypted: bool,
    /// One-way message (no response expected)
    pub one_way: bool,
}

impl FrameFlags {
    /// Create empty flags
    pub fn empty() -> Self {
        Self {
            compressed: false,
            encrypted: false,
            one_way: false,
        }
    }

    /// Convert to raw byte representation
    pub fn as_u8(&self) -> u8 {
        let mut b = 0u8;
        if self.compressed {
            b |= 0x01;
        }
        if self.encrypted {
            b |= 0x02;
        }
        if self.one_way {
            b |= 0x04;
        }
        b
    }

    /// Create from raw byte representation
    pub fn from_u8(b: u8) -> Self {
        Self {
            compressed: (b & 0x01) != 0,
            encrypted: (b & 0x02) != 0,
            one_way: (b & 0x04) != 0,
        }
    }
}

/// Frame header containing metadata about the payload
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    /// Protocol magic number
    pub magic: u32,
    /// Protocol version
    pub version: u8,
    /// Control flags
    pub flags: FrameFlags,
    /// Operation code
    pub opcode: Opcode,
    /// Request ID for multiplexing
    pub request_id: u64,
    /// Payload length in bytes
    pub payload_length: u32,
    /// CRC32 checksum of payload
    pub checksum: u32,
}

impl FrameHeader {
    /// Create a new frame header
    pub fn new(opcode: Opcode, request_id: u64, payload_length: u32, flags: FrameFlags) -> Self {
        Self {
            magic: MAGIC,
            version: PROTOCOL_VERSION,
            flags,
            opcode,
            request_id,
            payload_length,
            checksum: 0,
        }
    }
}

/// A single frame in the RPC protocol
#[derive(Debug, Clone)]
pub struct Frame {
    /// Frame header
    pub header: FrameHeader,
    /// Payload data
    pub payload: Vec<u8>,
}

impl Frame {
    /// Create a new frame
    pub fn new(opcode: Opcode, request_id: u64, payload: Vec<u8>, flags: FrameFlags) -> Self {
        let payload_length = payload.len() as u32;
        let header = FrameHeader::new(opcode, request_id, payload_length, flags);
        Self { header, payload }
    }
}
