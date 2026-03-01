//! Async metadata RPC protocol types for transport integration.
//!
//! Defines the request/response types that A4 (Transport) and A5 (FUSE)
//! use to invoke metadata operations remotely. Maps to the transport
//! layer's metadata opcodes (0x0100-0x0112).

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::node::MetadataNode;
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
/// Delegates all requests to a MetadataNode for actual processing.
pub struct RpcDispatcher {
    node: Arc<MetadataNode>,
}

impl RpcDispatcher {
    /// Creates a new RPC dispatcher backed by the given MetadataNode.
    pub fn new(node: Arc<MetadataNode>) -> Self {
        Self { node }
    }

    /// Creates a stub RPC dispatcher with an in-memory MetadataNode.
    /// Useful for testing.
    pub fn new_stub() -> Self {
        use crate::node::MetadataNodeConfig;
        let config = MetadataNodeConfig::default();
        let node = Arc::new(MetadataNode::new(config).expect("stub node creation"));
        Self { node }
    }

    /// Dispatches a metadata request to the MetadataNode.
    pub fn dispatch(&self, request: MetadataRequest) -> MetadataResponse {
        match request {
            MetadataRequest::Lookup { parent, name } => match self.node.lookup(parent, &name) {
                Ok(entry) => match self.node.getattr(entry.ino) {
                    Ok(attr) => MetadataResponse::LookupResult {
                        ino: entry.ino,
                        attr,
                    },
                    Err(e) => MetadataResponse::Error {
                        message: e.to_string(),
                    },
                },
                Err(e) => MetadataResponse::Error {
                    message: e.to_string(),
                },
            },
            MetadataRequest::GetAttr { ino } => match self.node.getattr(ino) {
                Ok(attr) => MetadataResponse::AttrResult { attr },
                Err(e) => MetadataResponse::Error {
                    message: e.to_string(),
                },
            },
            MetadataRequest::SetAttr { ino, attr } => match self.node.setattr(ino, attr) {
                Ok(()) => MetadataResponse::Ok,
                Err(e) => MetadataResponse::Error {
                    message: e.to_string(),
                },
            },
            MetadataRequest::CreateFile { parent, name, attr } => {
                match self
                    .node
                    .create_file(parent, &name, attr.uid, attr.gid, attr.mode)
                {
                    Ok(created) => MetadataResponse::EntryCreated {
                        ino: created.ino,
                        attr: created,
                    },
                    Err(e) => MetadataResponse::Error {
                        message: e.to_string(),
                    },
                }
            }
            MetadataRequest::Mkdir { parent, name, attr } => {
                match self
                    .node
                    .mkdir(parent, &name, attr.uid, attr.gid, attr.mode)
                {
                    Ok(created) => MetadataResponse::DirCreated {
                        ino: created.ino,
                        attr: created,
                    },
                    Err(e) => MetadataResponse::Error {
                        message: e.to_string(),
                    },
                }
            }
            MetadataRequest::Unlink { parent, name } => match self.node.unlink(parent, &name) {
                Ok(()) => MetadataResponse::Ok,
                Err(e) => MetadataResponse::Error {
                    message: e.to_string(),
                },
            },
            MetadataRequest::Rmdir { parent, name } => match self.node.rmdir(parent, &name) {
                Ok(()) => MetadataResponse::Ok,
                Err(e) => MetadataResponse::Error {
                    message: e.to_string(),
                },
            },
            MetadataRequest::Rename {
                src_parent,
                src_name,
                dst_parent,
                dst_name,
            } => {
                match self
                    .node
                    .rename(src_parent, &src_name, dst_parent, &dst_name)
                {
                    Ok(()) => MetadataResponse::Ok,
                    Err(e) => MetadataResponse::Error {
                        message: e.to_string(),
                    },
                }
            }
            MetadataRequest::Symlink {
                parent,
                name,
                target,
            } => match self.node.symlink(parent, &name, &target, 0, 0) {
                Ok(created) => MetadataResponse::EntryCreated {
                    ino: created.ino,
                    attr: created,
                },
                Err(e) => MetadataResponse::Error {
                    message: e.to_string(),
                },
            },
            MetadataRequest::Readlink { ino } => match self.node.readlink(ino) {
                Ok(target) => MetadataResponse::SymlinkTarget { target },
                Err(e) => MetadataResponse::Error {
                    message: e.to_string(),
                },
            },
            MetadataRequest::Link {
                parent,
                name,
                target_ino,
            } => match self.node.link(parent, &name, target_ino) {
                Ok(attr) => MetadataResponse::EntryCreated {
                    ino: attr.ino,
                    attr,
                },
                Err(e) => MetadataResponse::Error {
                    message: e.to_string(),
                },
            },
            MetadataRequest::Readdir { dir, .. } => match self.node.readdir(dir) {
                Ok(entries) => MetadataResponse::DirEntries {
                    entries,
                    has_more: false,
                },
                Err(e) => MetadataResponse::Error {
                    message: e.to_string(),
                },
            },
            MetadataRequest::Open { ino, flags } => match self.node.open(ino, 0, flags) {
                Ok(handle) => MetadataResponse::FileOpened { handle },
                Err(e) => MetadataResponse::Error {
                    message: e.to_string(),
                },
            },
            MetadataRequest::Close { handle } => match self.node.close(handle) {
                Ok(()) => MetadataResponse::Ok,
                Err(e) => MetadataResponse::Error {
                    message: e.to_string(),
                },
            },
            MetadataRequest::GetXattr { ino, name } => match self.node.get_xattr(ino, &name) {
                Ok(value) => MetadataResponse::XattrValue { value },
                Err(e) => MetadataResponse::Error {
                    message: e.to_string(),
                },
            },
            MetadataRequest::SetXattr { ino, name, value } => {
                match self.node.set_xattr(ino, &name, &value) {
                    Ok(()) => MetadataResponse::Ok,
                    Err(e) => MetadataResponse::Error {
                        message: e.to_string(),
                    },
                }
            }
            MetadataRequest::ListXattrs { ino } => match self.node.list_xattrs(ino) {
                Ok(names) => MetadataResponse::XattrList { names },
                Err(e) => MetadataResponse::Error {
                    message: e.to_string(),
                },
            },
            MetadataRequest::RemoveXattr { ino, name } => {
                match self.node.remove_xattr(ino, &name) {
                    Ok(()) => MetadataResponse::Ok,
                    Err(e) => MetadataResponse::Error {
                        message: e.to_string(),
                    },
                }
            }
        }
    }

    /// Returns the opcode for a request type.
    pub fn request_to_opcode(&self, request: &MetadataRequest) -> u16 {
        request_to_opcode(request)
    }

    /// Returns whether the request is read-only.
    pub fn is_read_only(&self, request: &MetadataRequest) -> bool {
        is_read_only(request)
    }
}

impl Default for RpcDispatcher {
    fn default() -> Self {
        Self::new_stub()
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
        let serialized = bincode::serialize(&request).unwrap();
        let deserialized: MetadataRequest = bincode::deserialize(&serialized).unwrap();
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
        let serialized = bincode::serialize(&response).unwrap();
        let deserialized: MetadataResponse = bincode::deserialize(&serialized).unwrap();
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
    fn test_dispatcher_lookup() {
        let dispatcher = RpcDispatcher::new_stub();
        let create_req = MetadataRequest::CreateFile {
            parent: InodeId::ROOT_INODE,
            name: "test.txt".to_string(),
            attr: InodeAttr::new_file(InodeId::new(0), 1000, 1000, 0o644, 1),
        };
        let create_resp = dispatcher.dispatch(create_req);
        let created_ino = match create_resp {
            MetadataResponse::EntryCreated { ino, .. } => ino,
            other => panic!("expected EntryCreated, got {:?}", other),
        };

        let lookup_req = MetadataRequest::Lookup {
            parent: InodeId::ROOT_INODE,
            name: "test.txt".to_string(),
        };
        let resp = dispatcher.dispatch(lookup_req);
        match resp {
            MetadataResponse::LookupResult { ino, attr } => {
                assert_eq!(ino, created_ino);
                assert_eq!(attr.uid, 1000);
            }
            other => panic!("expected LookupResult, got {:?}", other),
        }
    }

    #[test]
    fn test_dispatcher_create_file() {
        let dispatcher = RpcDispatcher::new_stub();
        let req = MetadataRequest::CreateFile {
            parent: InodeId::ROOT_INODE,
            name: "hello.txt".to_string(),
            attr: InodeAttr::new_file(InodeId::new(0), 500, 500, 0o644, 1),
        };
        let resp = dispatcher.dispatch(req);
        match resp {
            MetadataResponse::EntryCreated { attr, .. } => {
                assert_eq!(attr.uid, 500);
                assert_eq!(attr.mode, 0o644);
            }
            other => panic!("expected EntryCreated, got {:?}", other),
        }
    }

    #[test]
    fn test_dispatcher_mkdir() {
        let dispatcher = RpcDispatcher::new_stub();
        let req = MetadataRequest::Mkdir {
            parent: InodeId::ROOT_INODE,
            name: "subdir".to_string(),
            attr: InodeAttr::new_directory(InodeId::new(0), 1000, 1000, 0o755, 1),
        };
        let resp = dispatcher.dispatch(req);
        match resp {
            MetadataResponse::DirCreated { attr, .. } => {
                assert_eq!(attr.file_type, FileType::Directory);
            }
            other => panic!("expected DirCreated, got {:?}", other),
        }
    }

    #[test]
    fn test_dispatcher_readdir() {
        let dispatcher = RpcDispatcher::new_stub();
        dispatcher.dispatch(MetadataRequest::CreateFile {
            parent: InodeId::ROOT_INODE,
            name: "a.txt".to_string(),
            attr: InodeAttr::new_file(InodeId::new(0), 0, 0, 0o644, 1),
        });
        dispatcher.dispatch(MetadataRequest::CreateFile {
            parent: InodeId::ROOT_INODE,
            name: "b.txt".to_string(),
            attr: InodeAttr::new_file(InodeId::new(0), 0, 0, 0o644, 1),
        });

        let req = MetadataRequest::Readdir {
            dir: InodeId::ROOT_INODE,
            offset: 0,
            count: 100,
        };
        let resp = dispatcher.dispatch(req);
        match resp {
            MetadataResponse::DirEntries { entries, .. } => {
                assert!(entries.len() >= 2);
            }
            other => panic!("expected DirEntries, got {:?}", other),
        }
    }

    #[test]
    fn test_dispatcher_xattr() {
        let dispatcher = RpcDispatcher::new_stub();
        let create_resp = dispatcher.dispatch(MetadataRequest::CreateFile {
            parent: InodeId::ROOT_INODE,
            name: "test.txt".to_string(),
            attr: InodeAttr::new_file(InodeId::new(0), 0, 0, 0o644, 1),
        });
        let ino = match create_resp {
            MetadataResponse::EntryCreated { ino, .. } => ino,
            other => panic!("expected EntryCreated, got {:?}", other),
        };

        let set_resp = dispatcher.dispatch(MetadataRequest::SetXattr {
            ino,
            name: "user.key".to_string(),
            value: b"value".to_vec(),
        });
        assert!(matches!(set_resp, MetadataResponse::Ok));

        let get_resp = dispatcher.dispatch(MetadataRequest::GetXattr {
            ino,
            name: "user.key".to_string(),
        });
        match get_resp {
            MetadataResponse::XattrValue { value } => {
                assert_eq!(value, b"value");
            }
            other => panic!("expected XattrValue, got {:?}", other),
        }

        let list_resp = dispatcher.dispatch(MetadataRequest::ListXattrs { ino });
        match list_resp {
            MetadataResponse::XattrList { names } => {
                assert!(names.contains(&"user.key".to_string()));
            }
            other => panic!("expected XattrList, got {:?}", other),
        }
    }

    #[test]
    fn test_dispatcher_unlink() {
        let dispatcher = RpcDispatcher::new_stub();
        dispatcher.dispatch(MetadataRequest::CreateFile {
            parent: InodeId::ROOT_INODE,
            name: "delete_me.txt".to_string(),
            attr: InodeAttr::new_file(InodeId::new(0), 0, 0, 0o644, 1),
        });

        let resp = dispatcher.dispatch(MetadataRequest::Unlink {
            parent: InodeId::ROOT_INODE,
            name: "delete_me.txt".to_string(),
        });
        assert!(matches!(resp, MetadataResponse::Ok));

        let lookup_resp = dispatcher.dispatch(MetadataRequest::Lookup {
            parent: InodeId::ROOT_INODE,
            name: "delete_me.txt".to_string(),
        });
        assert!(matches!(lookup_resp, MetadataResponse::Error { .. }));
    }

    #[test]
    fn test_dispatcher_symlink_readlink() {
        let dispatcher = RpcDispatcher::new_stub();
        let create_resp = dispatcher.dispatch(MetadataRequest::Symlink {
            parent: InodeId::ROOT_INODE,
            name: "mylink".to_string(),
            target: "/etc/hosts".to_string(),
        });
        let ino = match create_resp {
            MetadataResponse::EntryCreated { ino, .. } => ino,
            other => panic!("expected EntryCreated, got {:?}", other),
        };

        let readlink_resp = dispatcher.dispatch(MetadataRequest::Readlink { ino });
        match readlink_resp {
            MetadataResponse::SymlinkTarget { target } => {
                assert_eq!(target, "/etc/hosts");
            }
            other => panic!("expected SymlinkTarget, got {:?}", other),
        }
    }

    #[test]
    fn test_dispatcher_request_to_opcode() {
        let dispatcher = RpcDispatcher::new_stub();
        let request = MetadataRequest::Mkdir {
            parent: InodeId::new(1),
            name: "test".to_string(),
            attr: InodeAttr::new_directory(InodeId::new(2), 0, 0, 0o755, 1),
        };
        assert_eq!(dispatcher.request_to_opcode(&request), 0x0104);
    }

    #[test]
    fn test_dispatcher_is_read_only() {
        let dispatcher = RpcDispatcher::new_stub();
        let request = MetadataRequest::Readdir {
            dir: InodeId::new(1),
            offset: 0,
            count: 100,
        };
        assert!(dispatcher.is_read_only(&request));
    }
}
