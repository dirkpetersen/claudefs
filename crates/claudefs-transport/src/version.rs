//! Protocol version negotiation for ClaudeFS transport layer.
//!
//! This module provides types and utilities for negotiating protocol versions
//! between nodes during connection setup, enabling rolling upgrades.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Feature flags for protocol capabilities.
pub mod features {
    /// Support for payload compression.
    pub const COMPRESSION: &str = "compression";
    /// Support for payload encryption.
    pub const ENCRYPTION: &str = "encryption";
    /// Support for connection multiplexing.
    pub const MULTIPLEXING: &str = "multiplexing";
    /// Support for zero-copy data transfer.
    pub const ZERO_COPY: &str = "zero_copy";
    /// Support for batch RPC operations.
    pub const BATCH_RPC: &str = "batch_rpc";
}

/// Semantic version representation for the protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ProtocolVersion {
    /// Major version - incompatible changes.
    pub major: u16,
    /// Minor version - backward compatible features.
    pub minor: u16,
    /// Patch version - backward compatible bug fixes.
    pub patch: u16,
}

impl ProtocolVersion {
    /// Create a new protocol version.
    pub fn new(major: u16, minor: u16, patch: u16) -> Self {
        ProtocolVersion {
            major,
            minor,
            patch,
        }
    }

    /// Returns the current protocol version (1.0.0).
    pub fn current() -> Self {
        ProtocolVersion::new(1, 0, 0)
    }

    /// Check if this version is compatible with another.
    /// Versions are compatible if they have the same major version.
    pub fn is_compatible_with(&self, other: &Self) -> bool {
        self.major == other.major
    }

    /// Encode version as 6-byte big-endian array.
    pub fn encode(&self) -> [u8; 6] {
        let mut bytes = [0u8; 6];
        bytes[0..2].copy_from_slice(&self.major.to_be_bytes());
        bytes[2..4].copy_from_slice(&self.minor.to_be_bytes());
        bytes[4..6].copy_from_slice(&self.patch.to_be_bytes());
        bytes
    }

    /// Decode version from 6-byte big-endian array.
    pub fn decode(bytes: &[u8; 6]) -> Self {
        let major = u16::from_be_bytes([bytes[0], bytes[1]]);
        let minor = u16::from_be_bytes([bytes[2], bytes[3]]);
        let patch = u16::from_be_bytes([bytes[4], bytes[5]]);
        ProtocolVersion {
            major,
            minor,
            patch,
        }
    }
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl FromStr for ProtocolVersion {
    type Err = VersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return Err(VersionError::ParseError(s.to_string()));
        }

        let major = parts[0]
            .parse()
            .map_err(|_| VersionError::ParseError(s.to_string()))?;
        let minor = parts[1]
            .parse()
            .map_err(|_| VersionError::ParseError(s.to_string()))?;
        let patch = parts[2]
            .parse()
            .map_err(|_| VersionError::ParseError(s.to_string()))?;

        Ok(ProtocolVersion {
            major,
            minor,
            patch,
        })
    }
}

/// A range of compatible protocol versions.
#[derive(Debug, Clone)]
pub struct VersionRange {
    /// Minimum version in the range (inclusive).
    pub min: ProtocolVersion,
    /// Maximum version in the range (inclusive).
    pub max: ProtocolVersion,
}

impl VersionRange {
    /// Create a new version range.
    pub fn new(min: ProtocolVersion, max: ProtocolVersion) -> Self {
        VersionRange { min, max }
    }

    /// Check if a version is within this range.
    pub fn contains(&self, version: &ProtocolVersion) -> bool {
        version.major == self.min.major
            && version.major == self.max.major
            && version.minor >= self.min.minor
            && version.minor <= self.max.minor
    }

    /// Find the intersection with another range.
    pub fn intersect(&self, other: &VersionRange) -> Option<VersionRange> {
        if self.min.major != other.min.major || self.max.major != other.max.major {
            return None;
        }

        let min = if self.min.minor > other.min.minor {
            self.min
        } else {
            other.min
        };

        let max = if self.max.minor < other.max.minor {
            self.max
        } else {
            other.max
        };

        if min.minor <= max.minor {
            Some(VersionRange { min, max })
        } else {
            None
        }
    }

    /// Get the highest version in the range.
    pub fn highest(&self) -> ProtocolVersion {
        self.max
    }
}

impl fmt::Display for VersionRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}, {}]", self.min, self.max)
    }
}

/// Errors that can occur during version negotiation.
#[derive(Debug, Clone, thiserror::Error)]
pub enum VersionError {
    /// Local and remote versions are incompatible.
    #[error("incompatible versions: local {local}, remote {remote}")]
    Incompatible {
        /// Local supported version range.
        local: VersionRange,
        /// Remote supported version range.
        remote: VersionRange,
    },
    /// Version is too old to be supported.
    #[error("version too old: {version}, minimum required: {minimum}")]
    TooOld {
        /// The version that is too old.
        version: ProtocolVersion,
        /// The minimum required version.
        minimum: ProtocolVersion,
    },
    /// Version is too new to be supported.
    #[error("version too new: {version}, maximum supported: {maximum}")]
    TooNew {
        /// The version that is too new.
        version: ProtocolVersion,
        /// The maximum supported version.
        maximum: ProtocolVersion,
    },
    /// Failed to parse a version string.
    #[error("invalid version string: {0}")]
    ParseError(String),
}

/// Handles version negotiation between local and remote nodes.
pub struct VersionNegotiator {
    supported: VersionRange,
}

impl VersionNegotiator {
    /// Create a new negotiator with the given supported version range.
    pub fn new(supported: VersionRange) -> Self {
        VersionNegotiator { supported }
    }

    /// Negotiate a mutually supported version with the remote range.
    pub fn negotiate(&self, remote_range: &VersionRange) -> Result<ProtocolVersion, VersionError> {
        match self.supported.intersect(remote_range) {
            Some(intersection) => Ok(intersection.highest()),
            None => Err(VersionError::Incompatible {
                local: self.supported.clone(),
                remote: remote_range.clone(),
            }),
        }
    }

    /// Check if a specific version is supported.
    pub fn is_supported(&self, version: &ProtocolVersion) -> bool {
        self.supported.contains(version)
    }
}

/// Initial handshake message for version negotiation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionHandshake {
    /// Supported version range (min, max).
    pub supported_range: (ProtocolVersion, ProtocolVersion),
    /// Preferred version (highest in supported range).
    pub preferred: ProtocolVersion,
    /// Supported feature flags.
    pub features: Vec<String>,
}

impl VersionHandshake {
    /// Create a new version handshake.
    pub fn new(supported: VersionRange, features: Vec<String>) -> Self {
        VersionHandshake {
            supported_range: (supported.min, supported.max),
            preferred: supported.max,
            features,
        }
    }

    /// Encode handshake using bincode.
    pub fn encode(&self) -> Vec<u8> {
        bincode::serialize(self).expect("Failed to serialize VersionHandshake")
    }

    /// Decode handshake from bincode bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, VersionError> {
        bincode::deserialize(bytes).map_err(|e| VersionError::ParseError(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_version_new() {
        let v = ProtocolVersion::new(1, 2, 3);
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);
    }

    #[test]
    fn test_protocol_version_current() {
        let v = ProtocolVersion::current();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 0);
        assert_eq!(v.patch, 0);
    }

    #[test]
    fn test_protocol_version_display() {
        let v = ProtocolVersion::new(1, 0, 0);
        assert_eq!(format!("{}", v), "1.0.0");
    }

    #[test]
    fn test_protocol_version_from_str() {
        let v: ProtocolVersion = "1.0.0".parse().unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 0);
        assert_eq!(v.patch, 0);
    }

    #[test]
    fn test_protocol_version_from_str_invalid() {
        let result: Result<ProtocolVersion, _> = "1.0".parse();
        assert!(result.is_err());

        let result: Result<ProtocolVersion, _> = "invalid".parse();
        assert!(result.is_err());
    }

    #[test]
    fn test_protocol_version_encode_decode() {
        let original = ProtocolVersion::new(1, 2, 3);
        let encoded = original.encode();
        let decoded = ProtocolVersion::decode(&encoded);
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_protocol_version_compatibility() {
        let v1 = ProtocolVersion::new(1, 0, 0);
        let v2 = ProtocolVersion::new(1, 2, 0);
        let v3 = ProtocolVersion::new(1, 5, 0);

        assert!(v1.is_compatible_with(&v2));
        assert!(v2.is_compatible_with(&v3));
        assert!(v1.is_compatible_with(&v3));
    }

    #[test]
    fn test_protocol_version_incompatible() {
        let v1 = ProtocolVersion::new(1, 0, 0);
        let v2 = ProtocolVersion::new(2, 0, 0);

        assert!(!v1.is_compatible_with(&v2));
        assert!(!v2.is_compatible_with(&v1));
    }

    #[test]
    fn test_version_range_contains() {
        let range = VersionRange::new(ProtocolVersion::new(1, 0, 0), ProtocolVersion::new(1, 5, 0));

        assert!(range.contains(&ProtocolVersion::new(1, 0, 0)));
        assert!(range.contains(&ProtocolVersion::new(1, 2, 3)));
        assert!(range.contains(&ProtocolVersion::new(1, 5, 0)));
        assert!(!range.contains(&ProtocolVersion::new(1, 6, 0)));
        assert!(!range.contains(&ProtocolVersion::new(2, 0, 0)));
    }

    #[test]
    fn test_version_range_intersect() {
        let range1 =
            VersionRange::new(ProtocolVersion::new(1, 0, 0), ProtocolVersion::new(1, 5, 0));
        let range2 =
            VersionRange::new(ProtocolVersion::new(1, 3, 0), ProtocolVersion::new(1, 8, 0));

        let intersection = range1.intersect(&range2).unwrap();
        assert_eq!(intersection.min, ProtocolVersion::new(1, 3, 0));
        assert_eq!(intersection.max, ProtocolVersion::new(1, 5, 0));
    }

    #[test]
    fn test_version_range_no_intersect() {
        let range1 =
            VersionRange::new(ProtocolVersion::new(1, 0, 0), ProtocolVersion::new(1, 2, 0));
        let range2 =
            VersionRange::new(ProtocolVersion::new(1, 5, 0), ProtocolVersion::new(1, 8, 0));

        let intersection = range1.intersect(&range2);
        assert!(intersection.is_none());

        let range3 =
            VersionRange::new(ProtocolVersion::new(1, 0, 0), ProtocolVersion::new(1, 2, 0));
        let range4 =
            VersionRange::new(ProtocolVersion::new(2, 0, 0), ProtocolVersion::new(2, 5, 0));

        let intersection = range3.intersect(&range4);
        assert!(intersection.is_none());
    }

    #[test]
    fn test_negotiator_success() {
        let local_range =
            VersionRange::new(ProtocolVersion::new(1, 0, 0), ProtocolVersion::new(1, 5, 0));
        let negotiator = VersionNegotiator::new(local_range);

        let remote_range =
            VersionRange::new(ProtocolVersion::new(1, 2, 0), ProtocolVersion::new(1, 8, 0));
        let result = negotiator.negotiate(&remote_range).unwrap();

        assert_eq!(result, ProtocolVersion::new(1, 5, 0));
    }

    #[test]
    fn test_negotiator_incompatible() {
        let local_range =
            VersionRange::new(ProtocolVersion::new(1, 0, 0), ProtocolVersion::new(1, 2, 0));
        let negotiator = VersionNegotiator::new(local_range);

        let remote_range =
            VersionRange::new(ProtocolVersion::new(2, 0, 0), ProtocolVersion::new(2, 5, 0));
        let result = negotiator.negotiate(&remote_range);

        assert!(matches!(result, Err(VersionError::Incompatible { .. })));
    }

    #[test]
    fn test_version_handshake_encode_decode() {
        let range = VersionRange::new(ProtocolVersion::new(1, 0, 0), ProtocolVersion::new(1, 5, 0));
        let features = vec![
            features::COMPRESSION.to_string(),
            features::ENCRYPTION.to_string(),
        ];
        let handshake = VersionHandshake::new(range, features);

        let encoded = handshake.encode();
        let decoded = VersionHandshake::decode(&encoded).unwrap();

        assert_eq!(decoded.supported_range.0.major, 1);
        assert_eq!(decoded.supported_range.1.major, 1);
        assert_eq!(decoded.preferred.major, 1);
        assert_eq!(decoded.features.len(), 2);
    }

    #[test]
    fn test_version_ordering() {
        let v1 = ProtocolVersion::new(1, 0, 0);
        let v2 = ProtocolVersion::new(1, 1, 0);
        let v3 = ProtocolVersion::new(1, 1, 1);
        let v4 = ProtocolVersion::new(2, 0, 0);

        assert!(v1 < v2);
        assert!(v2 < v3);
        assert!(v3 < v4);
        assert!(v1 <= v1);
        assert!(v1 >= v1);
    }

    #[test]
    fn test_feature_constants() {
        assert_eq!(features::COMPRESSION, "compression");
        assert_eq!(features::ENCRYPTION, "encryption");
        assert_eq!(features::MULTIPLEXING, "multiplexing");
        assert_eq!(features::ZERO_COPY, "zero_copy");
        assert_eq!(features::BATCH_RPC, "batch_rpc");
    }
}
