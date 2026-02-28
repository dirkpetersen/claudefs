//! Async metadata RPC protocol types for transport integration.
//!
//! Defines the request/response types that A4 (Transport) and A5 (FUSE)
//! use to invoke metadata operations remotely. Maps to the transport
//! layer's metadata opcodes (0x0100-0x0112).

use serde::{Deserialize, Serialize};
use std::io::Read;

use crate::types::*;

/// Metadata operation request types for RPC transport.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MetadataRequest {
    /// Lookup a path component in a directory.
    Lookup {
        /// Parent directory inode.
        parent: InodeId,
        /// Name to lookup.
        name: String,
    },
    /// Get file attributes.
    GetAttr {
        /// Target inode.
        ino: InodeId,
    },
    /// Set file attributes.
    SetAttr {
        /// Target inode.
        ino: InodeId,
        /// New attributes.
        attr: InodeAttr,
    },
    /// Create a new regular file.
    CreateFile {
        /// Parent directory inode.
        parent: InodeId,
        /// File name.
        name: String,
        /// File attributes.
        attr: InodeAttr,
    },
    /// Create a new directory.
    Mkdir {
        /// Parent directory inode.
        parent: InodeId,
        /// Directory name.
        name: String,
        /// Directory attributes.
        attr: InodeAttr,
    },
    /// Remove a file.
    Unlink {
        /// Parent directory inode.
        parent: InodeId,
        /// File name.
        name: String,
    },
    /// Remove a directory.
    Rmdir {
        /// Parent directory inode.
        parent: InodeId,
        /// Directory name.
        name: String,
    },
    /// Rename a file or directory.
    Rename {
        /// Source parent directory.
        src_parent: InodeId,
        /// Source name.
        src_name: String,
        /// Destination parent directory.
        dst_parent: InodeId,
        /// Destination name.
        dst_name: String,
    },
    /// Create a symbolic link.
    Symlink {
        /// Parent directory inode.
        parent: InodeId,
        /// Link name.
        name: String,
        /// Target path.
        target: String,
    },
    /// Read symbolic link target.
    Readlink {
        /// Symlink inode.
        ino: InodeId,
    },
    /// Create a hard link.
    Link {
        /// Parent directory for new link.
        parent: InodeId,
        /// Link name.
        name: String,
        /// Target inode.
        target_ino: InodeId,
    },
    /// Read directory entries.
    Readdir {
        /// Directory inode.
        dir: InodeId,
        /// Starting offset.
        offset: u64,
        /// Maximum entries to return.
        count: u32,
    },
    /// Open a file.
    Open {
        /// File inode.
        ino: InodeId,
        /// Open flags.
        flags: u32,
    },
    /// Close a file.
    Close {
        /// File handle ID.
        handle: u64,
    },
    /// Get extended attribute.
    GetXattr {
        /// Target inode.
        ino: InodeId,
        /// Attribute name.
        name: String,
    },
    /// Set extended attribute.
    SetXattr {
        /// Target inode.
        ino: InodeId,
        /// Attribute name.
        name: String,
        /// Attribute value.
        value: Vec<u8>,
    },
    /// List extended attributes.
    ListXattrs {
        /// Target inode.
        ino: InodeId,
    },
    /// Remove extended attribute.
    RemoveXattr {
        /// Target inode.
        ino: InodeId,
        /// Attribute name.
        name: String,
    },
}

impl MetadataRequest {
    /// Returns the opcode for this request type.
    pub fn opcode(&self) -> u16 {
        request_to_opcode(self)
    }
}

/// Maps request variants to unique opcode numbers (0x0100-0x0112).
pub fn request_to_opcode(request: &MetadataRequest) -> u16 {
    match request {
        MetadataRequest::Lookup { .. } => 0x0100,
        MetadataRequest::GetAttr { .. } => 0x0101,
        MetadataRequest::SetAttr { .. } => 0x0102,
        MetadataRequest::CreateFile { .. } => 0x0103,
        MetadataRequest::Mkdir { .. } => 0x0104,
        MetadataRequest::Unlink { .. } => 0x0105,
        MetadataRequest::Rmdir { .. } => 0x0106,
        MetadataRequest::Rename { .. } => 0x0107,
        MetadataRequest::Symlink { .. } => 0x0108,
        MetadataRequest::Readlink { .. } => 0x0109,
        MetadataRequest::Link { .. } => 0x010A,
        MetadataRequest::Readdir { .. } => 0x010B,
        MetadataRequest::Open { .. } => 0x010C,
        MetadataRequest::Close { .. } => 0x010D,
        MetadataRequest::GetXattr { .. } => 0x010E,
        MetadataRequest::SetXattr { .. } => 0x010F,
        MetadataRequest::ListXattrs { .. } => 0x0110,
        MetadataRequest::RemoveXattr { .. } => 0x0111,
    }
}

/// Returns true if the request is read-only (does not modify metadata).
pub fn is_read_only(request: &MetadataRequest) -> bool {
    match request {
        MetadataRequest::Lookup { .. }
        | MetadataRequest::GetAttr { .. }
        | MetadataRequest::Readlink { .. }
        | MetadataRequest::Readdir { .. }
        | MetadataRequest::ListXattrs { .. } => true,
        MetadataRequest::SetAttr { .. }
        | MetadataRequest::CreateFile { .. }
        | MetadataRequest::Mkdir { .. }
        | MetadataRequest::Unlink { .. }
        | MetadataRequest::Rmdir { .. }
        | MetadataRequest::Rename { .. }
        | MetadataRequest::Symlink { .. }
        | MetadataRequest::Link { .. }
        | MetadataRequest::Open { .. }
        | MetadataRequest::Close { .. }
        | MetadataRequest::GetXattr { .. }
        | MetadataRequest::SetXattr { .. }
        | MetadataRequest::RemoveXattr { .. } => false,
    }
}

/// Metadata operation response types for RPC transport.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MetadataResponse {
    /// Lookup successful - returns the found inode.
    LookupResult {
        /// Found inode ID.
        ino: InodeId,
        /// Inode attributes.
        attr: InodeAttr,
    },
    /// GetAttr successful - returns attributes.
    AttrResult {
        /// Inode attributes.
        attr: InodeAttr,
    },
    /// File creation successful.
    EntryCreated {
        /// Created inode ID.
        ino: InodeId,
        /// Inode attributes.
        attr: InodeAttr,
    },
    /// Directory creation successful.
    DirCreated {
        /// Created inode ID.
        ino: InodeId,
        /// Inode attributes.
        attr: InodeAttr,
    },
    /// Generic success response.
    Ok,
    /// Directory entries response.
    DirEntries {
        /// List of directory entries.
        entries: Vec<DirEntry>,
        /// Whether there are more entries.
        has_more: bool,
    },
    /// Symlink target response.
    SymlinkTarget {
        /// Target path.
        target: String,
    },
    /// File opened successfully.
    FileOpened {
        /// File handle ID.
        handle: u64,
    },
    /// Extended attribute value response.
    XattrValue {
        /// Attribute value.
        value: Vec<u8>,
    },
    /// Extended attribute list response.
    XattrList {
        /// List of attribute names.
        names: Vec<String>,
    },
    /// Error response.
    Error {
        /// Error message.
        message: String,
    },
}

/// RPC dispatcher for metadata requests.
///
/// Currently returns error stubs for all requests; full implementation
/// would delegate to the metadata service.
pub struct RpcDispatcher;

impl RpcDispatcher {
    /// Creates a new RPC dispatcher.
    pub fn new() -> Self {
        Self
    }

    /// Dispatches a metadata request.
    ///
    /// Currently returns an error stub indicating the method is not implemented.
    pub fn dispatch(&self, _request: MetadataRequest) -> MetadataResponse {
        tracing::debug!("RPC dispatch called - returning stub error");
        MetadataResponse::Error {
            message: "RPC dispatch not implemented".to_string(),
        }
    }

    /// Returns the opcode for a request type.
    pub fn request_to_opcode(request: &MetadataRequest) -> u16 {
        request_to_opcode(request)
    }

    /// Returns whether the request is read-only.
    pub fn is_read_only(request: &MetadataRequest) -> bool {
        is_read_only(request)
    }
}

impl Default for RpcDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization_roundtrip() {
        let request = MetadataRequest::Lookup {
            parent: InodeId::new(100),
            name: "test.txt".to_string(),
        };
        let serialized = serde_json::to_string(&request).unwrap();
        let deserialized: MetadataRequest = serde_json::from_str(&serialized).unwrap();
        match deserialized {
            MetadataRequest::Lookup { parent, name } => {
                assert_eq!(parent, InodeId::new(100));
                assert_eq!(name, "test.txt");
            }
            _ => panic!("unexpected variant"),
        }
    }

    #[test]
    fn test_response_serialization_roundtrip() {
        let response = MetadataResponse::AttrResult {
            attr: InodeAttr::new_file(InodeId::new(200), 1000, 1000, 0o644, 1),
        };
        let serialized = serde_json::to_string(&response).unwrap();
        let deserialized: MetadataResponse = serde_json::from_str(&serialized).unwrap();
        assert!(matches!(deserialized, MetadataResponse::AttrResult { .. }));
    }

    #[test]
    fn test_opcode_mapping() {
        let lookup = MetadataRequest::Lookup {
            parent: InodeId::new(1),
            name: "test".to_string(),
        };
        assert_eq!(request_to_opcode(&lookup), 0x0100);

        let getattr = MetadataRequest::GetAttr {
            ino: InodeId::new(1),
        };
        assert_eq!(request_to_opcode(&getattr), 0x0101);

        let mkdir = MetadataRequest::Mkdir {
            parent: InodeId::new(1),
            name: "dir".to_string(),
            attr: InodeAttr::new_directory(InodeId::new(2), 0, 0, 0o755, 1),
        };
        assert_eq!(request_to_opcode(&mkdir), 0x0104);

        let unlink = MetadataRequest::Unlink {
            parent: InodeId::new(1),
            name: "file".to_string(),
        };
        assert_eq!(request_to_opcode(&unlink), 0x0105);

        let readdir = MetadataRequest::Readdir {
            dir: InodeId::new(1),
            offset: 0,
            count: 100,
        };
        assert_eq!(request_to_opcode(&readdir), 0x010B);

        let removexattr = MetadataRequest::RemoveXattr {
            ino: InodeId::new(1),
            name: "user.attr".to_string(),
        };
        assert_eq!(request_to_opcode(&removexattr), 0x0111);
    }

    #[test]
    fn test_is_read_only_lookup() {
        let request = MetadataRequest::Lookup {
            parent: InodeId::new(1),
            name: "test".to_string(),
        };
        assert!(is_read_only(&request));
    }

    #[test]
    fn test_is_read_only_getattr() {
        let request = MetadataRequest::GetAttr {
            ino: InodeId::new(1),
        };
        assert!(is_read_only(&request));
    }

    #[test]
    fn test_is_not_read_only_create() {
        let request = MetadataRequest::CreateFile {
            parent: InodeId::new(1),
            name: "test".to_string(),
            attr: InodeAttr::new_file(InodeId::new(2), 0, 0, 0o644, 1),
        };
        assert!(!is_read_only(&request));
    }

    #[test]
    fn test_is_not_read_only_setattr() {
        let request = MetadataRequest::SetAttr {
            ino: InodeId::new(1),
            attr: InodeAttr::new_file(InodeId::new(1), 0, 0, 0o644, 1),
        };
        assert!(!is_read_only(&request));
    }

    #[test]
    fn test_dispatcher_returns_error_stub() {
        let dispatcher = RpcDispatcher::new();
        let request = MetadataRequest::GetAttr {
            ino: InodeId::new(1),
        };
        let response = dispatcher.dispatch(request);
        assert!(matches!(response, MetadataResponse::Error { .. }));
    }

    #[test]
    fn test_dispatcher_request_to_opcode() {
        let dispatcher = RpcDispatcher::new();
        let request = MetadataRequest::Mkdir {
            parent: InodeId::new(1),
            name: "test".to_string(),
            attr: InodeAttr::new_directory(InodeId::new(2), 0, 0, 0o755, 1),
        };
        assert_eq!(dispatcher.request_to_opcode(&request), 0x0104);
    }

    #[test]
    fn test_dispatcher_is_read_only() {
        let dispatcher = RpcDispatcher::new();
        let request = MetadataRequest::Readdir {
            dir: InodeId::new(1),
            offset: 0,
            count: 100,
        };
        assert!(dispatcher.is_read_only(&request));
    }
}
