//! MOUNT protocol v3 (RFC 1813 Appendix I)

use crate::protocol::FileHandle3;
use crate::xdr::XdrEncoder;
use std::sync::{Arc, Mutex};

/// MOUNT status: OK (success)
pub const MNT_OK: u32 = 0;
/// MOUNT status: not owner
pub const MNT_ERR_PERM: u32 = 1;
/// MOUNT status: no such file or directory
pub const MNT_ERR_NOENT: u32 = 2;
/// MOUNT status: I/O error
pub const MNT_ERR_IO: u32 = 5;
/// MOUNT status: permission denied
pub const MNT_ERR_ACCESS: u32 = 13;
/// MOUNT status: not a directory
pub const MNT_ERR_NOTDIR: u32 = 20;
/// MOUNT status: invalid argument
pub const MNT_ERR_INVAL: u32 = 22;
/// MOUNT status: filename too long
pub const MNT_ERR_NAMETOOLONG: u32 = 63;
/// MOUNT status: operation not supported
pub const MNT_ERR_NOTSUPP: u32 = 10004;
/// MOUNT status: server fault
pub const MNT_ERR_SERVERFAULT: u32 = 10006;

/// MOUNTv3 procedure: NULL
pub const MNT3_NULL: u32 = 0;
/// MOUNTv3 procedure: MNT (mount)
pub const MNT3_MNT: u32 = 1;
/// MOUNTv3 procedure: DUMP
pub const MNT3_DUMP: u32 = 2;
/// MOUNTv3 procedure: UMNT (unmount)
pub const MNT3_UMNT: u32 = 3;
/// MOUNTv3 procedure: UMNTALL (unmount all)
pub const MNT3_UMNTALL: u32 = 4;
/// MOUNTv3 procedure: EXPORT (list exports)
pub const MNT3_EXPORT: u32 = 5;

/// Active mount record
#[derive(Debug, Clone)]
pub struct MountEntry {
    /// Client hostname/IP
    pub hostname: String,
    /// Exported path
    pub dirpath: String,
}

/// Export entry for MOUNT protocol
#[derive(Debug, Clone)]
pub struct ExportEntry {
    /// Directory path
    pub dirpath: String,
    /// Allowed client groups
    pub groups: Vec<String>,
}

/// MOUNT protocol MNT procedure result
#[derive(Debug, Clone)]
pub struct MntResult {
    /// Status code (0 = success)
    pub status: u32,
    /// File handle if successful
    pub filehandle: Option<FileHandle3>,
    /// Supported auth flavors
    pub auth_flavors: Vec<u32>,
}

/// MOUNT protocol v3 handler
pub struct MountHandler {
    exports: Vec<ExportEntry>,
    mounts: Arc<Mutex<Vec<MountEntry>>>,
    root_fh: FileHandle3,
}

impl MountHandler {
    pub fn new(exports: Vec<ExportEntry>, root_fh: FileHandle3) -> Self {
        Self {
            exports,
            mounts: Arc::new(Mutex::new(Vec::new())),
            root_fh,
        }
    }

    pub fn null(&self) {}

    pub fn mnt(&self, path: &str, client_host: &str) -> MntResult {
        let export = match self.is_exported(path) {
            Some(e) => e,
            None => {
                return MntResult {
                    status: MNT_ERR_NOENT,
                    filehandle: None,
                    auth_flavors: vec![],
                };
            }
        };

        if !self.is_allowed(export, client_host) {
            return MntResult {
                status: MNT_ERR_ACCESS,
                filehandle: None,
                auth_flavors: vec![],
            };
        }

        let entry = MountEntry {
            hostname: client_host.to_string(),
            dirpath: path.to_string(),
        };

        if let Ok(mut mounts) = self.mounts.lock() {
            mounts.push(entry);
        }

        MntResult {
            status: MNT_OK,
            filehandle: Some(self.root_fh.clone()),
            auth_flavors: vec![0, 1],
        }
    }

    pub fn dump(&self) -> Vec<MountEntry> {
        self.mounts.lock().map(|m| m.clone()).unwrap_or_default()
    }

    pub fn umnt(&self, path: &str) {
        if let Ok(mut mounts) = self.mounts.lock() {
            mounts.retain(|m| m.dirpath != path);
        }
    }

    pub fn umntall(&self) {
        if let Ok(mut mounts) = self.mounts.lock() {
            mounts.clear();
        }
    }

    pub fn export(&self) -> Vec<ExportEntry> {
        self.exports.clone()
    }

    pub fn is_exported(&self, path: &str) -> Option<&ExportEntry> {
        self.exports.iter().find(|e| e.dirpath == path)
    }

    pub fn is_allowed(&self, export: &ExportEntry, client_host: &str) -> bool {
        if export.groups.is_empty() {
            return true;
        }
        if client_host.is_empty() || client_host == "127.0.0.1" {
            return true;
        }
        export.groups.iter().any(|g| g == client_host || g == "*")
    }

    pub fn encode_mnt_result(result: &MntResult, enc: &mut XdrEncoder) {
        enc.encode_u32(result.status);

        match &result.filehandle {
            Some(fh) => {
                enc.encode_u32(1);
                fh.encode_xdr(enc);
            }
            None => {
                enc.encode_u32(0);
            }
        }

        enc.encode_u32(result.auth_flavors.len() as u32);
        for flavor in &result.auth_flavors {
            enc.encode_u32(*flavor);
        }
    }

    pub fn mount_count(&self) -> usize {
        self.mounts.lock().map(|m| m.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::FileHandle3;

    fn test_export() -> ExportEntry {
        ExportEntry {
            dirpath: "/export".to_string(),
            groups: vec![],
        }
    }

    fn test_handler() -> MountHandler {
        let root_fh = FileHandle3::from_inode(1);
        MountHandler::new(vec![test_export()], root_fh)
    }

    #[test]
    fn test_create_handler() {
        let handler = test_handler();
        assert_eq!(handler.mount_count(), 0);
    }

    #[test]
    fn test_mnt_valid_path() {
        let handler = test_handler();
        let result = handler.mnt("/export", "client1");
        assert_eq!(result.status, MNT_OK);
        assert!(result.filehandle.is_some());
    }

    #[test]
    fn test_mnt_invalid_path() {
        let handler = test_handler();
        let result = handler.mnt("/nonexistent", "client1");
        assert_eq!(result.status, MNT_ERR_NOENT);
        assert!(result.filehandle.is_none());
    }

    #[test]
    fn test_mnt_wrong_client() {
        let root_fh = FileHandle3::from_inode(1);
        let export = ExportEntry {
            dirpath: "/secure".to_string(),
            groups: vec!["allowedhost".to_string()],
        };
        let handler = MountHandler::new(vec![export], root_fh);

        let result = handler.mnt("/secure", "otherhost");
        assert_eq!(result.status, MNT_ERR_ACCESS);
    }

    #[test]
    fn test_mnt_allowed_client() {
        let root_fh = FileHandle3::from_inode(1);
        let export = ExportEntry {
            dirpath: "/secure".to_string(),
            groups: vec!["allowedhost".to_string()],
        };
        let handler = MountHandler::new(vec![export], root_fh);

        let result = handler.mnt("/secure", "allowedhost");
        assert_eq!(result.status, MNT_OK);
    }

    #[test]
    fn test_dump() {
        let handler = test_handler();
        handler.mnt("/export", "host1");
        handler.mnt("/export", "host2");
        let mounts = handler.dump();
        assert_eq!(mounts.len(), 2);
    }

    #[test]
    fn test_umnt() {
        let handler = test_handler();
        handler.mnt("/export", "host1");
        handler.mnt("/export", "host2");
        handler.umnt("/export");
        let mounts = handler.dump();
        assert!(mounts.is_empty());
    }

    #[test]
    fn test_umntall() {
        let handler = test_handler();
        handler.mnt("/export", "host1");
        handler.mnt("/export", "host2");
        handler.umntall();
        let mounts = handler.dump();
        assert!(mounts.is_empty());
    }

    #[test]
    fn test_export_list() {
        let handler = test_handler();
        let exports = handler.export();
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].dirpath, "/export");
    }

    #[test]
    fn test_is_exported() {
        let handler = test_handler();
        assert!(handler.is_exported("/export").is_some());
        assert!(handler.is_exported("/nonexport").is_none());
    }

    #[test]
    fn test_is_allowed() {
        let export = ExportEntry {
            dirpath: "/test".to_string(),
            groups: vec!["host1".to_string(), "host2".to_string()],
        };
        let handler = test_handler();
        assert!(handler.is_allowed(&export, "host1"));
        assert!(handler.is_allowed(&export, "host2"));
        assert!(!handler.is_allowed(&export, "host3"));
    }

    #[test]
    fn test_is_allowed_empty_groups() {
        let export = ExportEntry {
            dirpath: "/test".to_string(),
            groups: vec![],
        };
        let handler = test_handler();
        assert!(handler.is_allowed(&export, "anyhost"));
        assert!(handler.is_allowed(&export, ""));
        assert!(handler.is_allowed(&export, "127.0.0.1"));
    }

    #[test]
    fn test_mnt_auth_flavors() {
        let handler = test_handler();
        let result = handler.mnt("/export", "client1");
        assert_eq!(result.auth_flavors, vec![0, 1]);
    }

    #[test]
    fn test_mnt_registers_mount() {
        let handler = test_handler();
        assert_eq!(handler.mount_count(), 0);
        handler.mnt("/export", "host1");
        assert_eq!(handler.mount_count(), 1);
    }

    #[test]
    fn test_null() {
        let handler = test_handler();
        handler.null();
    }
}
