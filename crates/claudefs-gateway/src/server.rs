//! NFS server combining RPC dispatcher + NFS handlers

use crate::auth::AuthCred;
use crate::mount::{ExportEntry, MountHandler};
use crate::nfs::{Nfs3Handler, VfsBackend};
use crate::protocol::FileHandle3;
#[allow(unused_imports)]
use crate::rpc::{
    OpaqueAuth, RpcCall, RpcReply, ACCEPT_PROC_UNAVAIL, ACCEPT_SUCCESS, AUTH_NONE, MNT3_DUMP,
    MNT3_EXPORT, MNT3_MNT, MNT3_NULL, MNT3_UMNT, MNT3_UMNTALL, MOUNT_PROGRAM, MOUNT_VERSION,
    NFS3_COMMIT, NFS3_GETATTR, NFS3_LOOKUP, NFS3_MKDIR, NFS3_NULL, NFS3_READ, NFS3_READDIR,
    NFS3_RENAME, NFS3_SETATTR, NFS3_WRITE, NFS_PROGRAM, NFS_VERSION,
};
use crate::xdr::{XdrDecoder, XdrEncoder};
use std::sync::Arc;

pub struct NfsServerConfig {
    pub tcp_port: u16,
    pub mount_port: u16,
    pub max_read_size: u32,
    pub max_write_size: u32,
    pub fsid: u64,
    pub exports: Vec<ExportEntry>,
}

impl NfsServerConfig {
    pub fn default_with_export(path: &str) -> Self {
        Self {
            tcp_port: 2049,
            mount_port: 20048,
            max_read_size: 1024 * 1024,
            max_write_size: 1024 * 1024,
            fsid: 0x636673,
            exports: vec![ExportEntry {
                dirpath: path.to_string(),
                groups: vec![],
            }],
        }
    }
}

pub struct RpcDispatcher<B: VfsBackend> {
    nfs_handler: Nfs3Handler<B>,
    mount_handler: MountHandler,
    #[allow(dead_code)]
    config: NfsServerConfig,
}

impl<B: VfsBackend> RpcDispatcher<B> {
    pub fn new(backend: Arc<B>, mount_handler: MountHandler, config: NfsServerConfig) -> Self {
        Self {
            nfs_handler: Nfs3Handler::new(backend, config.fsid),
            mount_handler,
            config,
        }
    }

    pub fn dispatch(&self, call: &RpcCall) -> Vec<u8> {
        let auth = AuthCred::from_opaque_auth(&call.cred);

        match (call.prog, call.vers) {
            (NFS_PROGRAM, NFS_VERSION) => self.dispatch_nfs(call, &auth),
            (MOUNT_PROGRAM, MOUNT_VERSION) => self.dispatch_mount(call),
            _ => RpcReply::encode_prog_mismatch(call.xid, 3, 3),
        }
    }

    fn dispatch_nfs(&self, call: &RpcCall, auth: &AuthCred) -> Vec<u8> {
        let _uid = auth.uid();
        let _gid = auth.gid();

        match call.proc {
            NFS3_NULL => RpcReply::encode_success(call.xid, &[]),
            NFS3_GETATTR => {
                let mut dec =
                    XdrDecoder::new(prost::bytes::Bytes::copy_from_slice(&call.args_bytes));
                if let Ok(fh) = FileHandle3::decode_xdr(&mut dec) {
                    match self.nfs_handler.handle_getattr(&fh) {
                        crate::nfs::Nfs3GetAttrResult::Ok(attr) => {
                            let mut enc = XdrEncoder::new();
                            enc.encode_u32(0);
                            attr.encode_xdr(&mut enc);
                            return RpcReply::encode_success(call.xid, &enc.finish());
                        }
                        crate::nfs::Nfs3GetAttrResult::Err(status) => {
                            let mut enc = XdrEncoder::new();
                            enc.encode_u32(status);
                            return RpcReply::encode_success(call.xid, &enc.finish());
                        }
                    }
                }
                RpcReply::encode_garbage_args(call.xid)
            }
            NFS3_LOOKUP => {
                let mut dec =
                    XdrDecoder::new(prost::bytes::Bytes::copy_from_slice(&call.args_bytes));
                if let (Ok(dir_fh), Ok(name)) =
                    (FileHandle3::decode_xdr(&mut dec), dec.decode_string())
                {
                    match self.nfs_handler.handle_lookup(&dir_fh, &name) {
                        crate::nfs::Nfs3LookupResult::Ok(result) => {
                            let mut enc = XdrEncoder::new();
                            enc.encode_u32(0);
                            enc.encode_bool(true);
                            result.object.encode_xdr(&mut enc);
                            if let Some(attr) = &result.obj_attributes {
                                enc.encode_bool(true);
                                attr.encode_xdr(&mut enc);
                            } else {
                                enc.encode_bool(false);
                            }
                            if let Some(attr) = &result.dir_attributes {
                                enc.encode_bool(true);
                                attr.encode_xdr(&mut enc);
                            } else {
                                enc.encode_bool(false);
                            }
                            return RpcReply::encode_success(call.xid, &enc.finish());
                        }
                        crate::nfs::Nfs3LookupResult::Err(status) => {
                            let mut enc = XdrEncoder::new();
                            enc.encode_u32(status);
                            return RpcReply::encode_success(call.xid, &enc.finish());
                        }
                    }
                }
                RpcReply::encode_garbage_args(call.xid)
            }
            NFS3_MKDIR => {
                let mut dec =
                    XdrDecoder::new(prost::bytes::Bytes::copy_from_slice(&call.args_bytes));
                if let (Ok(dir_fh), Ok(name), Ok(mode)) = (
                    FileHandle3::decode_xdr(&mut dec),
                    dec.decode_string(),
                    dec.decode_u32(),
                ) {
                    match self.nfs_handler.handle_mkdir(&dir_fh, &name, mode) {
                        crate::nfs::Nfs3MkdirResult::Ok(fh, attr) => {
                            let mut enc = XdrEncoder::new();
                            enc.encode_u32(0);
                            enc.encode_bool(true);
                            fh.encode_xdr(&mut enc);
                            enc.encode_bool(true);
                            attr.encode_xdr(&mut enc);
                            enc.encode_bool(false);
                            return RpcReply::encode_success(call.xid, &enc.finish());
                        }
                        crate::nfs::Nfs3MkdirResult::Err(status) => {
                            let mut enc = XdrEncoder::new();
                            enc.encode_u32(status);
                            return RpcReply::encode_success(call.xid, &enc.finish());
                        }
                    }
                }
                RpcReply::encode_garbage_args(call.xid)
            }
            NFS3_WRITE => {
                let mut dec =
                    XdrDecoder::new(prost::bytes::Bytes::copy_from_slice(&call.args_bytes));
                if let (Ok(fh), Ok(offset), Ok(count), Ok(_stable)) = (
                    FileHandle3::decode_xdr(&mut dec),
                    dec.decode_u64(),
                    dec.decode_u32(),
                    dec.decode_u32(),
                ) {
                    let data_len = dec.remaining();
                    let remaining = dec.remaining_bytes();
                    let data = &remaining[..data_len.min(count as usize)];
                    match self.nfs_handler.handle_write(&fh, offset, 0, data) {
                        crate::nfs::Nfs3WriteResult::Ok(count, stable) => {
                            let mut enc = XdrEncoder::new();
                            enc.encode_u32(0);
                            enc.encode_u32(count);
                            enc.encode_u32(stable);
                            let mut verf = XdrEncoder::new();
                            verf.encode_u32(0);
                            verf.encode_u32(0);
                            enc.encode_opaque_variable(&verf.finish());
                            return RpcReply::encode_success(call.xid, &enc.finish());
                        }
                        crate::nfs::Nfs3WriteResult::Err(status) => {
                            let mut enc = XdrEncoder::new();
                            enc.encode_u32(status);
                            return RpcReply::encode_success(call.xid, &enc.finish());
                        }
                    }
                }
                RpcReply::encode_garbage_args(call.xid)
            }
            NFS3_READ => {
                let mut dec =
                    XdrDecoder::new(prost::bytes::Bytes::copy_from_slice(&call.args_bytes));
                if let (Ok(fh), Ok(offset), Ok(count)) = (
                    FileHandle3::decode_xdr(&mut dec),
                    dec.decode_u64(),
                    dec.decode_u32(),
                ) {
                    match self.nfs_handler.handle_read(&fh, offset, count) {
                        crate::nfs::Nfs3ReadResult::Ok(data, eof) => {
                            let mut enc = XdrEncoder::new();
                            enc.encode_u32(0);
                            enc.encode_u32(data.len() as u32);
                            enc.encode_opaque_variable(&data);
                            enc.encode_bool(eof);
                            return RpcReply::encode_success(call.xid, &enc.finish());
                        }
                        crate::nfs::Nfs3ReadResult::Err(status) => {
                            let mut enc = XdrEncoder::new();
                            enc.encode_u32(status);
                            return RpcReply::encode_success(call.xid, &enc.finish());
                        }
                    }
                }
                RpcReply::encode_garbage_args(call.xid)
            }
            NFS3_READDIR => {
                let mut dec =
                    XdrDecoder::new(prost::bytes::Bytes::copy_from_slice(&call.args_bytes));
                if let (Ok(dir_fh), Ok(cookie), Ok(count)) = (
                    FileHandle3::decode_xdr(&mut dec),
                    dec.decode_u64(),
                    dec.decode_u32(),
                ) {
                    match self.nfs_handler.handle_readdir(&dir_fh, cookie, 0, count) {
                        crate::nfs::Nfs3ReadDirResult::Ok(result) => {
                            let mut enc = XdrEncoder::new();
                            enc.encode_u32(0);
                            enc.encode_bool(false);
                            enc.encode_u64(result.cookieverf);
                            enc.encode_u32(result.entries.len() as u32);
                            for entry in &result.entries {
                                enc.encode_u64(entry.fileid);
                                enc.encode_string(&entry.name);
                                enc.encode_u64(entry.cookie);
                                enc.encode_bool(true);
                            }
                            enc.encode_bool(false);
                            enc.encode_bool(result.eof);
                            return RpcReply::encode_success(call.xid, &enc.finish());
                        }
                        crate::nfs::Nfs3ReadDirResult::Err(status) => {
                            let mut enc = XdrEncoder::new();
                            enc.encode_u32(status);
                            return RpcReply::encode_success(call.xid, &enc.finish());
                        }
                    }
                }
                RpcReply::encode_garbage_args(call.xid)
            }
            NFS3_RENAME => RpcReply::encode_proc_unavail(call.xid),
            NFS3_SETATTR => RpcReply::encode_proc_unavail(call.xid),
            NFS3_COMMIT => {
                let mut enc = XdrEncoder::new();
                enc.encode_u32(0);
                let mut verf = XdrEncoder::new();
                verf.encode_u32(0);
                verf.encode_u32(0);
                enc.encode_opaque_variable(&verf.finish());
                RpcReply::encode_success(call.xid, &enc.finish())
            }
            _ => RpcReply::encode_proc_unavail(call.xid),
        }
    }

    fn dispatch_mount(&self, call: &RpcCall) -> Vec<u8> {
        match call.proc {
            MNT3_NULL => RpcReply::encode_success(call.xid, &[]),
            MNT3_MNT => {
                let mut dec =
                    XdrDecoder::new(prost::bytes::Bytes::copy_from_slice(&call.args_bytes));
                if let Ok(path) = dec.decode_string() {
                    let client_host = "";
                    let result = self.mount_handler.mnt(&path, client_host);
                    let mut enc = XdrEncoder::new();
                    crate::mount::MountHandler::encode_mnt_result(&result, &mut enc);
                    return RpcReply::encode_success(call.xid, &enc.finish());
                }
                RpcReply::encode_garbage_args(call.xid)
            }
            MNT3_DUMP => {
                let mounts = self.mount_handler.dump();
                let mut enc = XdrEncoder::new();
                enc.encode_u32(mounts.len() as u32);
                for m in mounts {
                    enc.encode_string(&m.hostname);
                    enc.encode_string(&m.dirpath);
                }
                RpcReply::encode_success(call.xid, &enc.finish())
            }
            MNT3_UMNT => {
                let mut dec =
                    XdrDecoder::new(prost::bytes::Bytes::copy_from_slice(&call.args_bytes));
                if let Ok(path) = dec.decode_string() {
                    self.mount_handler.umnt(&path);
                }
                RpcReply::encode_success(call.xid, &[])
            }
            MNT3_UMNTALL => {
                self.mount_handler.umntall();
                RpcReply::encode_success(call.xid, &[])
            }
            MNT3_EXPORT => {
                let exports = self.mount_handler.export();
                let mut enc = XdrEncoder::new();
                enc.encode_u32(exports.len() as u32);
                for e in exports {
                    enc.encode_string(&e.dirpath);
                    enc.encode_u32(e.groups.len() as u32);
                    for g in &e.groups {
                        enc.encode_string(g);
                    }
                }
                RpcReply::encode_success(call.xid, &enc.finish())
            }
            _ => RpcReply::encode_proc_unavail(call.xid),
        }
    }

    pub fn process_tcp(&self, data: &[u8]) -> Vec<u8> {
        if data.len() < 4 {
            return vec![];
        }

        let header: [u8; 4] = data[..4].try_into().unwrap();
        let (_is_last, fragment_len) = crate::rpc::TcpRecordMark::decode(header);

        if data.len() < 4 + fragment_len as usize {
            return vec![];
        }

        let rpc_data = &data[4..4 + fragment_len as usize];

        match RpcCall::decode(rpc_data) {
            Ok(call) => {
                let reply = self.dispatch(&call);
                let marked = crate::rpc::TcpRecordMark::encode(&reply);
                marked
            }
            Err(_) => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mount::MountHandler;
    use crate::nfs::MockVfsBackend;
    use crate::protocol::FileHandle3;
    use crate::rpc::TcpRecordMark;

    fn test_dispatcher() -> RpcDispatcher<MockVfsBackend> {
        let backend = Arc::new(MockVfsBackend::new(1));
        let root_fh = FileHandle3::from_inode(1);
        let mount_handler = MountHandler::new(
            vec![ExportEntry {
                dirpath: "/".to_string(),
                groups: vec![],
            }],
            root_fh,
        );
        let config = NfsServerConfig::default_with_export("/");
        RpcDispatcher::new(backend, mount_handler, config)
    }

    #[test]
    fn test_create_dispatcher() {
        let disp = test_dispatcher();
        assert_eq!(disp.mount_handler.mount_count(), 0);
    }

    #[test]
    fn test_dispatch_nfs_null() {
        let disp = test_dispatcher();

        let mut enc = XdrEncoder::new();
        enc.encode_u32(1);
        enc.encode_u32(0);
        enc.encode_u32(2);
        enc.encode_u32(NFS_PROGRAM);
        enc.encode_u32(NFS_VERSION);
        enc.encode_u32(NFS3_NULL);
        OpaqueAuth::none().encode_xdr(&mut enc);
        OpaqueAuth::none().encode_xdr(&mut enc);
        let call_data = enc.finish().to_vec();

        let call = RpcCall::decode(&call_data).unwrap();
        let reply = disp.dispatch(&call);

        let mut dec = XdrDecoder::new(prost::bytes::Bytes::from(reply));
        assert_eq!(dec.decode_u32().unwrap(), 1);
        assert_eq!(dec.decode_u32().unwrap(), 1);
    }

    #[test]
    fn test_dispatch_mount_null() {
        let disp = test_dispatcher();

        let mut enc = XdrEncoder::new();
        enc.encode_u32(2);
        enc.encode_u32(0);
        enc.encode_u32(2);
        enc.encode_u32(MOUNT_PROGRAM);
        enc.encode_u32(MOUNT_VERSION);
        enc.encode_u32(MNT3_NULL);
        OpaqueAuth::none().encode_xdr(&mut enc);
        OpaqueAuth::none().encode_xdr(&mut enc);
        let call_data = enc.finish().to_vec();

        let call = RpcCall::decode(&call_data).unwrap();
        let reply = disp.dispatch(&call);

        let mut dec = XdrDecoder::new(prost::bytes::Bytes::from(reply));
        assert_eq!(dec.decode_u32().unwrap(), 2);
        assert_eq!(dec.decode_u32().unwrap(), 1);
    }

    #[test]
    fn test_dispatch_nfs_getattr() {
        let disp = test_dispatcher();

        let mut enc = XdrEncoder::new();
        enc.encode_u32(3);
        enc.encode_u32(0);
        enc.encode_u32(2);
        enc.encode_u32(NFS_PROGRAM);
        enc.encode_u32(NFS_VERSION);
        enc.encode_u32(NFS3_GETATTR);

        let mut auth = XdrEncoder::new();
        auth.encode_u32(AUTH_NONE);
        auth.encode_opaque_variable(b"");

        let auth_bytes = auth.finish();
        enc.encode_u32(AUTH_NONE);
        enc.encode_opaque_variable(&auth_bytes);
        enc.encode_u32(AUTH_NONE);
        enc.encode_opaque_variable(&auth_bytes);

        let root_fh = FileHandle3::from_inode(1);
        root_fh.encode_xdr(&mut enc);

        let call_data = enc.finish().to_vec();

        let call = RpcCall::decode(&call_data).unwrap();
        let reply = disp.dispatch(&call);

        let mut dec = XdrDecoder::new(prost::bytes::Bytes::from(reply));
        assert_eq!(dec.decode_u32().unwrap(), 3);
        assert_eq!(dec.decode_u32().unwrap(), 1);
        assert_eq!(dec.decode_u32().unwrap(), ACCEPT_SUCCESS);
    }

    #[test]
    fn test_dispatch_wrong_program() {
        let disp = test_dispatcher();

        let mut enc = XdrEncoder::new();
        enc.encode_u32(4);
        enc.encode_u32(0);
        enc.encode_u32(2);
        enc.encode_u32(999999);
        enc.encode_u32(1);
        enc.encode_u32(0);
        OpaqueAuth::none().encode_xdr(&mut enc);
        OpaqueAuth::none().encode_xdr(&mut enc);
        let call_data = enc.finish().to_vec();

        let call = RpcCall::decode(&call_data).unwrap();
        let reply = disp.dispatch(&call);

        let mut dec = XdrDecoder::new(prost::bytes::Bytes::from(reply));
        assert_eq!(dec.decode_u32().unwrap(), 4);
        assert_eq!(dec.decode_u32().unwrap(), 1);
    }

    #[test]
    fn test_process_tcp_with_record_mark() {
        let disp = test_dispatcher();

        let mut enc = XdrEncoder::new();
        enc.encode_u32(5);
        enc.encode_u32(0);
        enc.encode_u32(2);
        enc.encode_u32(NFS_PROGRAM);
        enc.encode_u32(NFS_VERSION);
        enc.encode_u32(NFS3_NULL);
        OpaqueAuth::none().encode_xdr(&mut enc);
        OpaqueAuth::none().encode_xdr(&mut enc);
        let call_data = enc.finish().to_vec();

        let marked = TcpRecordMark::encode(&call_data);
        let reply = disp.process_tcp(&marked);

        assert!(reply.len() > 4);
    }

    #[test]
    fn test_dispatch_nfs_lookup() {
        let disp = test_dispatcher();

        let mut enc = XdrEncoder::new();
        enc.encode_u32(6);
        enc.encode_u32(0);
        enc.encode_u32(2);
        enc.encode_u32(NFS_PROGRAM);
        enc.encode_u32(NFS_VERSION);
        enc.encode_u32(NFS3_LOOKUP);

        let mut auth = XdrEncoder::new();
        auth.encode_u32(AUTH_NONE);
        auth.encode_opaque_variable(b"");
        let auth_bytes = auth.finish();
        enc.encode_u32(AUTH_NONE);
        enc.encode_opaque_variable(&auth_bytes);
        enc.encode_u32(AUTH_NONE);
        enc.encode_opaque_variable(&auth_bytes);

        let root_fh = FileHandle3::from_inode(1);
        root_fh.encode_xdr(&mut enc);
        enc.encode_string(".");

        let call_data = enc.finish().to_vec();
        let call = RpcCall::decode(&call_data).unwrap();
        let reply = disp.dispatch(&call);

        let mut dec = XdrDecoder::new(prost::bytes::Bytes::from(reply));
        assert_eq!(dec.decode_u32().unwrap(), 6);
        assert_eq!(dec.decode_u32().unwrap(), 1);
        assert_eq!(dec.decode_u32().unwrap(), ACCEPT_SUCCESS);
    }

    #[test]
    fn test_dispatch_mount_mnt() {
        let disp = test_dispatcher();

        let mut enc = XdrEncoder::new();
        enc.encode_u32(7);
        enc.encode_u32(0);
        enc.encode_u32(2);
        enc.encode_u32(MOUNT_PROGRAM);
        enc.encode_u32(MOUNT_VERSION);
        enc.encode_u32(MNT3_MNT);
        OpaqueAuth::none().encode_xdr(&mut enc);
        OpaqueAuth::none().encode_xdr(&mut enc);
        enc.encode_string("/");
        let call_data = enc.finish().to_vec();

        let call = RpcCall::decode(&call_data).unwrap();
        let reply = disp.dispatch(&call);

        let mut dec = XdrDecoder::new(prost::bytes::Bytes::from(reply));
        assert_eq!(dec.decode_u32().unwrap(), 7);
        assert_eq!(dec.decode_u32().unwrap(), 1);
    }

    #[test]
    fn test_dispatch_nfs_proc_unavail() {
        let disp = test_dispatcher();

        let mut enc = XdrEncoder::new();
        enc.encode_u32(8);
        enc.encode_u32(0);
        enc.encode_u32(2);
        enc.encode_u32(NFS_PROGRAM);
        enc.encode_u32(NFS_VERSION);
        enc.encode_u32(999);
        OpaqueAuth::none().encode_xdr(&mut enc);
        OpaqueAuth::none().encode_xdr(&mut enc);
        let call_data = enc.finish().to_vec();

        let call = RpcCall::decode(&call_data).unwrap();
        let reply = disp.dispatch(&call);

        let mut dec = XdrDecoder::new(prost::bytes::Bytes::from(reply));
        assert_eq!(dec.decode_u32().unwrap(), 8);
        assert_eq!(dec.decode_u32().unwrap(), 1);
        assert_eq!(dec.decode_u32().unwrap(), ACCEPT_PROC_UNAVAIL);
    }

    #[test]
    fn test_dispatch_nfs_commit() {
        let disp = test_dispatcher();

        let mut enc = XdrEncoder::new();
        enc.encode_u32(9);
        enc.encode_u32(0);
        enc.encode_u32(2);
        enc.encode_u32(NFS_PROGRAM);
        enc.encode_u32(NFS_VERSION);
        enc.encode_u32(NFS3_COMMIT);

        let mut auth = XdrEncoder::new();
        auth.encode_u32(AUTH_NONE);
        auth.encode_opaque_variable(b"");
        let auth_bytes = auth.finish();
        enc.encode_u32(AUTH_NONE);
        enc.encode_opaque_variable(&auth_bytes);
        enc.encode_u32(AUTH_NONE);
        enc.encode_opaque_variable(&auth_bytes);

        let root_fh = FileHandle3::from_inode(1);
        root_fh.encode_xdr(&mut enc);
        enc.encode_u64(0);
        enc.encode_u32(0);

        let call_data = enc.finish().to_vec();
        let call = RpcCall::decode(&call_data).unwrap();
        let reply = disp.dispatch(&call);

        let mut dec = XdrDecoder::new(prost::bytes::Bytes::from(reply));
        assert_eq!(dec.decode_u32().unwrap(), 9);
        assert_eq!(dec.decode_u32().unwrap(), 1);
        assert_eq!(dec.decode_u32().unwrap(), ACCEPT_SUCCESS);
    }

    #[test]
    fn test_dispatch_mount_export() {
        let disp = test_dispatcher();

        let mut enc = XdrEncoder::new();
        enc.encode_u32(10);
        enc.encode_u32(0);
        enc.encode_u32(2);
        enc.encode_u32(MOUNT_PROGRAM);
        enc.encode_u32(MOUNT_VERSION);
        enc.encode_u32(MNT3_EXPORT);
        OpaqueAuth::none().encode_xdr(&mut enc);
        OpaqueAuth::none().encode_xdr(&mut enc);
        let call_data = enc.finish().to_vec();

        let call = RpcCall::decode(&call_data).unwrap();
        let reply = disp.dispatch(&call);

        let mut dec = XdrDecoder::new(prost::bytes::Bytes::from(reply));
        assert_eq!(dec.decode_u32().unwrap(), 10);
        assert_eq!(dec.decode_u32().unwrap(), 1);
    }

    #[test]
    fn test_dispatch_nfs_readdir() {
        let disp = test_dispatcher();

        let mut enc = XdrEncoder::new();
        enc.encode_u32(11);
        enc.encode_u32(0);
        enc.encode_u32(2);
        enc.encode_u32(NFS_PROGRAM);
        enc.encode_u32(NFS_VERSION);
        enc.encode_u32(NFS3_READDIR);

        let mut auth = XdrEncoder::new();
        auth.encode_u32(AUTH_NONE);
        auth.encode_opaque_variable(b"");
        let auth_bytes = auth.finish();
        enc.encode_u32(AUTH_NONE);
        enc.encode_opaque_variable(&auth_bytes);
        enc.encode_u32(AUTH_NONE);
        enc.encode_opaque_variable(&auth_bytes);

        let root_fh = FileHandle3::from_inode(1);
        root_fh.encode_xdr(&mut enc);
        enc.encode_u64(0);
        enc.encode_u64(0);
        enc.encode_u32(4096);

        let call_data = enc.finish().to_vec();
        let call = RpcCall::decode(&call_data).unwrap();
        let reply = disp.dispatch(&call);

        let mut dec = XdrDecoder::new(prost::bytes::Bytes::from(reply));
        assert_eq!(dec.decode_u32().unwrap(), 11);
        assert_eq!(dec.decode_u32().unwrap(), 1);
    }

    #[test]
    fn test_dispatch_nfs_write() {
        let disp = test_dispatcher();

        let mut enc = XdrEncoder::new();
        enc.encode_u32(12);
        enc.encode_u32(0);
        enc.encode_u32(2);
        enc.encode_u32(NFS_PROGRAM);
        enc.encode_u32(NFS_VERSION);
        enc.encode_u32(NFS3_WRITE);

        let mut auth = XdrEncoder::new();
        auth.encode_u32(AUTH_NONE);
        auth.encode_opaque_variable(b"");
        let auth_bytes = auth.finish();
        enc.encode_u32(AUTH_NONE);
        enc.encode_opaque_variable(&auth_bytes);
        enc.encode_u32(AUTH_NONE);
        enc.encode_opaque_variable(&auth_bytes);

        let root_fh = FileHandle3::from_inode(1);
        root_fh.encode_xdr(&mut enc);
        enc.encode_u64(0);
        enc.encode_u32(5);
        enc.encode_u32(0);
        enc.encode_opaque_variable(b"hello");

        let call_data = enc.finish().to_vec();
        let call = RpcCall::decode(&call_data).unwrap();
        let reply = disp.dispatch(&call);

        assert!(reply.len() > 4);
    }

    #[test]
    fn test_dispatch_mount_umnt() {
        let disp = test_dispatcher();

        disp.mount_handler.mnt("/", "testhost");
        assert_eq!(disp.mount_handler.mount_count(), 1);

        let mut enc = XdrEncoder::new();
        enc.encode_u32(13);
        enc.encode_u32(0);
        enc.encode_u32(2);
        enc.encode_u32(MOUNT_PROGRAM);
        enc.encode_u32(MOUNT_VERSION);
        enc.encode_u32(MNT3_UMNT);
        OpaqueAuth::none().encode_xdr(&mut enc);
        OpaqueAuth::none().encode_xdr(&mut enc);
        enc.encode_string("/");
        let call_data = enc.finish().to_vec();

        let call = RpcCall::decode(&call_data).unwrap();
        let reply = disp.dispatch(&call);

        let mut dec = XdrDecoder::new(prost::bytes::Bytes::from(reply));
        assert_eq!(dec.decode_u32().unwrap(), 13);
        assert_eq!(dec.decode_u32().unwrap(), 1);

        assert_eq!(disp.mount_handler.mount_count(), 0);
    }

    #[test]
    fn test_dispatch_nfs_mkdir() {
        let disp = test_dispatcher();

        let mut enc = XdrEncoder::new();
        enc.encode_u32(14);
        enc.encode_u32(0);
        enc.encode_u32(2);
        enc.encode_u32(NFS_PROGRAM);
        enc.encode_u32(NFS_VERSION);
        enc.encode_u32(NFS3_MKDIR);

        let mut auth = XdrEncoder::new();
        auth.encode_u32(AUTH_NONE);
        auth.encode_opaque_variable(b"");
        let auth_bytes = auth.finish();
        enc.encode_u32(AUTH_NONE);
        enc.encode_opaque_variable(&auth_bytes);
        enc.encode_u32(AUTH_NONE);
        enc.encode_opaque_variable(&auth_bytes);

        let root_fh = FileHandle3::from_inode(1);
        root_fh.encode_xdr(&mut enc);
        enc.encode_string("testdir");
        enc.encode_u32(0o755);

        let call_data = enc.finish().to_vec();
        let call = RpcCall::decode(&call_data).unwrap();
        let reply = disp.dispatch(&call);

        let mut dec = XdrDecoder::new(prost::bytes::Bytes::from(reply));
        assert_eq!(dec.decode_u32().unwrap(), 14);
        assert_eq!(dec.decode_u32().unwrap(), 1);
    }
}
