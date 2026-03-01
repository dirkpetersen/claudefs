//! SMB3 gateway stub.

use crate::error::{GatewayError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SmbSessionId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SmbTreeId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SmbFileId(pub u64);

#[derive(Debug, Clone)]
pub struct SmbAuthInfo {
    pub session_id: SmbSessionId,
    pub uid: u32,
    pub gid: u32,
    pub supplementary_gids: Vec<u32>,
    pub username: String,
    pub domain: String,
}

#[derive(Debug, Clone, Copy)]
pub struct OpenFlags {
    pub read: bool,
    pub write: bool,
    pub create: bool,
    pub truncate: bool,
    pub exclusive: bool,
}

impl OpenFlags {
    pub fn new(read: bool, write: bool, create: bool, truncate: bool, exclusive: bool) -> Self {
        Self {
            read,
            write,
            create,
            truncate,
            exclusive,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SmbFileStat {
    pub size: u64,
    pub uid: u32,
    pub gid: u32,
    pub mode: u32,
    pub inode: u64,
    pub atime_ns: u64,
    pub mtime_ns: u64,
    pub ctime_ns: u64,
}

#[derive(Debug, Clone)]
pub struct SmbDirEntry {
    pub name: String,
    pub stat: SmbFileStat,
}

pub trait SmbVfsOps: Send + Sync {
    fn smb_open(&self, auth: &SmbAuthInfo, path: &str, flags: OpenFlags) -> Result<SmbFileId>;
    fn smb_close(&self, file_id: SmbFileId) -> Result<()>;
    fn smb_read(&self, file_id: SmbFileId, offset: u64, len: u32) -> Result<Vec<u8>>;
    fn smb_write(&self, file_id: SmbFileId, offset: u64, data: &[u8]) -> Result<u32>;
    fn smb_stat(&self, auth: &SmbAuthInfo, path: &str) -> Result<SmbFileStat>;
    fn smb_mkdir(&self, auth: &SmbAuthInfo, path: &str) -> Result<()>;
    fn smb_unlink(&self, auth: &SmbAuthInfo, path: &str) -> Result<()>;
    fn smb_rename(&self, auth: &SmbAuthInfo, from: &str, to: &str) -> Result<()>;
    fn smb_readdir(&self, auth: &SmbAuthInfo, path: &str) -> Result<Vec<SmbDirEntry>>;
}

pub struct SmbVfsStub;

impl SmbVfsOps for SmbVfsStub {
    fn smb_open(&self, _auth: &SmbAuthInfo, _path: &str, _flags: OpenFlags) -> Result<SmbFileId> {
        Err(GatewayError::NotImplemented {
            feature: "smb3".to_string(),
        })
    }

    fn smb_close(&self, _file_id: SmbFileId) -> Result<()> {
        Err(GatewayError::NotImplemented {
            feature: "smb3".to_string(),
        })
    }

    fn smb_read(&self, _file_id: SmbFileId, _offset: u64, _len: u32) -> Result<Vec<u8>> {
        Err(GatewayError::NotImplemented {
            feature: "smb3".to_string(),
        })
    }

    fn smb_write(&self, _file_id: SmbFileId, _offset: u64, _data: &[u8]) -> Result<u32> {
        Err(GatewayError::NotImplemented {
            feature: "smb3".to_string(),
        })
    }

    fn smb_stat(&self, _auth: &SmbAuthInfo, _path: &str) -> Result<SmbFileStat> {
        Err(GatewayError::NotImplemented {
            feature: "smb3".to_string(),
        })
    }

    fn smb_mkdir(&self, _auth: &SmbAuthInfo, _path: &str) -> Result<()> {
        Err(GatewayError::NotImplemented {
            feature: "smb3".to_string(),
        })
    }

    fn smb_unlink(&self, _auth: &SmbAuthInfo, _path: &str) -> Result<()> {
        Err(GatewayError::NotImplemented {
            feature: "smb3".to_string(),
        })
    }

    fn smb_rename(&self, _auth: &SmbAuthInfo, _from: &str, _to: &str) -> Result<()> {
        Err(GatewayError::NotImplemented {
            feature: "smb3".to_string(),
        })
    }

    fn smb_readdir(&self, _auth: &SmbAuthInfo, _path: &str) -> Result<Vec<SmbDirEntry>> {
        Err(GatewayError::NotImplemented {
            feature: "smb3".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smb_session_id() {
        let id = SmbSessionId(12345);
        assert_eq!(id.0, 12345);
    }

    #[test]
    fn test_smb_session_id_equality() {
        let id1 = SmbSessionId(100);
        let id2 = SmbSessionId(100);
        let id3 = SmbSessionId(200);
        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_smb_tree_id() {
        let id = SmbTreeId(999);
        assert_eq!(id.0, 999);
    }

    #[test]
    fn test_smb_tree_id_equality() {
        let id1 = SmbTreeId(50);
        let id2 = SmbTreeId(50);
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_smb_file_id() {
        let id = SmbFileId(555);
        assert_eq!(id.0, 555);
    }

    #[test]
    fn test_smb_file_id_equality() {
        let id1 = SmbFileId(777);
        let id2 = SmbFileId(777);
        let id3 = SmbFileId(888);
        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_smb_auth_info() {
        let auth = SmbAuthInfo {
            session_id: SmbSessionId(1),
            uid: 1000,
            gid: 1000,
            supplementary_gids: vec![1001, 1002],
            username: "testuser".to_string(),
            domain: "TESTDOMAIN".to_string(),
        };
        assert_eq!(auth.uid, 1000);
        assert_eq!(auth.username, "testuser");
    }

    #[test]
    fn test_open_flags() {
        let flags = OpenFlags::new(true, false, true, false, false);
        assert!(flags.read);
        assert!(!flags.write);
        assert!(flags.create);
        assert!(!flags.truncate);
        assert!(!flags.exclusive);
    }

    #[test]
    fn test_open_flags_default() {
        let flags = OpenFlags {
            read: false,
            write: true,
            create: false,
            truncate: true,
            exclusive: false,
        };
        assert!(!flags.read);
        assert!(flags.write);
        assert!(flags.truncate);
    }

    #[test]
    fn test_smb_vfs_stub_open() {
        let stub = SmbVfsStub;
        let auth = SmbAuthInfo {
            session_id: SmbSessionId(1),
            uid: 0,
            gid: 0,
            supplementary_gids: vec![],
            username: "test".to_string(),
            domain: "TEST".to_string(),
        };
        let result = stub.smb_open(
            &auth,
            "/test",
            OpenFlags::new(true, false, false, false, false),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_smb_vfs_stub_read() {
        let stub = SmbVfsStub;
        let result = stub.smb_read(SmbFileId(1), 0, 100);
        assert!(result.is_err());
    }

    #[test]
    fn test_smb_vfs_stub_write() {
        let stub = SmbVfsStub;
        let result = stub.smb_write(SmbFileId(1), 0, b"data");
        assert!(result.is_err());
    }

    #[test]
    fn test_smb_vfs_stub_stat() {
        let stub = SmbVfsStub;
        let auth = SmbAuthInfo {
            session_id: SmbSessionId(1),
            uid: 0,
            gid: 0,
            supplementary_gids: vec![],
            username: "test".to_string(),
            domain: "TEST".to_string(),
        };
        let result = stub.smb_stat(&auth, "/test");
        assert!(result.is_err());
    }

    #[test]
    fn test_smb_vfs_stub_not_implemented_error() {
        let stub = SmbVfsStub;
        let auth = SmbAuthInfo {
            session_id: SmbSessionId(1),
            uid: 0,
            gid: 0,
            supplementary_gids: vec![],
            username: "test".to_string(),
            domain: "TEST".to_string(),
        };
        let result = stub.smb_mkdir(&auth, "/test");
        let err = result.unwrap_err();
        match err {
            GatewayError::NotImplemented { feature } => assert_eq!(feature, "smb3"),
            _ => panic!("expected NotImplemented"),
        }
    }

    #[test]
    fn test_smb_vfs_stub_rename() {
        let stub = SmbVfsStub;
        let auth = SmbAuthInfo {
            session_id: SmbSessionId(1),
            uid: 0,
            gid: 0,
            supplementary_gids: vec![],
            username: "test".to_string(),
            domain: "TEST".to_string(),
        };
        let result = stub.smb_rename(&auth, "/old", "/new");
        assert!(result.is_err());
    }

    #[test]
    fn test_smb_vfs_stub_readdir() {
        let stub = SmbVfsStub;
        let auth = SmbAuthInfo {
            session_id: SmbSessionId(1),
            uid: 0,
            gid: 0,
            supplementary_gids: vec![],
            username: "test".to_string(),
            domain: "TEST".to_string(),
        };
        let result = stub.smb_readdir(&auth, "/");
        assert!(result.is_err());
    }
}
