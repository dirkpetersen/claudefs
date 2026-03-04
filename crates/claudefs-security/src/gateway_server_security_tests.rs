//! Gateway server security tests.
//!
//! Audits gateway::server (RpcDispatcher, NfsServerConfig, NFS/MOUNT protocol dispatch).

use claudefs_gateway::mount::{ExportEntry, MountHandler};
use claudefs_gateway::nfs::MockVfsBackend;
use claudefs_gateway::protocol::FileHandle3;
use claudefs_gateway::rpc::{
    OpaqueAuth, RpcCall, TcpRecordMark, ACCEPT_GARBAGE_ARGS, ACCEPT_PROC_UNAVAIL, ACCEPT_SUCCESS,
    AUTH_NONE, MNT3_EXPORT, MNT3_MNT, MNT3_NULL, MNT3_UMNT, MNT3_UMNTALL, MOUNT_PROGRAM,
    MOUNT_VERSION, NFS3_COMMIT, NFS3_GETATTR, NFS3_LOOKUP, NFS3_MKDIR, NFS3_NULL, NFS3_READDIR,
    NFS3_RENAME, NFS3_SETATTR, NFS3_WRITE, NFS_PROGRAM, NFS_VERSION, REJECT_RPC_MISMATCH, RPC_CALL,
};
use claudefs_gateway::server::{NfsServerConfig, RpcDispatcher};
use claudefs_gateway::xdr::{XdrDecoder, XdrEncoder};
use prost::bytes::Bytes;
use std::sync::Arc;

fn make_dispatcher() -> RpcDispatcher<MockVfsBackend> {
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

fn make_nfs_call(xid: u32, prog: u32, vers: u32, proc_num: u32, args: &[u8]) -> RpcCall {
    RpcCall {
        xid,
        rpcvers: 2,
        prog,
        vers,
        proc: proc_num,
        cred: OpaqueAuth::none(),
        verf: OpaqueAuth::none(),
        args_bytes: args.to_vec(),
    }
}

fn make_mount_call(xid: u32, proc_num: u32, args: &[u8]) -> RpcCall {
    make_nfs_call(xid, MOUNT_PROGRAM, MOUNT_VERSION, proc_num, args)
}

fn encode_root_fh() -> Vec<u8> {
    let mut enc = XdrEncoder::new();
    let root_fh = FileHandle3::from_inode(1);
    root_fh.encode_xdr(&mut enc);
    enc.finish().to_vec()
}

#[test]
fn test_gw_srv_sec_unknown_program_returns_prog_mismatch() {
    let disp = make_dispatcher();
    let call = make_nfs_call(100, 999999, 1, 0, &[]);
    let reply = disp.dispatch_call(&call);

    let mut dec = XdrDecoder::new(Bytes::from(reply));
    assert_eq!(dec.decode_u32().unwrap(), 100);
    assert_eq!(dec.decode_u32().unwrap(), 1);
    assert_eq!(dec.decode_u32().unwrap(), REJECT_RPC_MISMATCH);
}

#[test]
fn test_gw_srv_sec_unknown_nfs_proc_returns_proc_unavail() {
    let disp = make_dispatcher();
    let call = make_nfs_call(101, NFS_PROGRAM, NFS_VERSION, 999, &[]);
    let reply = disp.dispatch_call(&call);

    let mut dec = XdrDecoder::new(Bytes::from(reply));
    assert_eq!(dec.decode_u32().unwrap(), 101);
    assert_eq!(dec.decode_u32().unwrap(), 1);
    assert_eq!(dec.decode_u32().unwrap(), ACCEPT_PROC_UNAVAIL);
}

#[test]
fn test_gw_srv_sec_nfs3_null_returns_empty_success() {
    let disp = make_dispatcher();
    let call = make_nfs_call(102, NFS_PROGRAM, NFS_VERSION, NFS3_NULL, &[]);
    let reply = disp.dispatch_call(&call);

    let mut dec = XdrDecoder::new(Bytes::from(reply));
    assert_eq!(dec.decode_u32().unwrap(), 102);
    assert_eq!(dec.decode_u32().unwrap(), 1);
    assert_eq!(dec.decode_u32().unwrap(), ACCEPT_SUCCESS);
}

#[test]
fn test_gw_srv_sec_mount3_null_returns_empty_success() {
    let disp = make_dispatcher();
    let call = make_mount_call(103, MNT3_NULL, &[]);
    let reply = disp.dispatch_call(&call);

    let mut dec = XdrDecoder::new(Bytes::from(reply));
    assert_eq!(dec.decode_u32().unwrap(), 103);
    assert_eq!(dec.decode_u32().unwrap(), 1);
    assert_eq!(dec.decode_u32().unwrap(), ACCEPT_SUCCESS);
}

#[test]
fn test_gw_srv_sec_nfs3_commit_returns_success_with_verifier() {
    let disp = make_dispatcher();
    let args = encode_root_fh();
    let call = make_nfs_call(104, NFS_PROGRAM, NFS_VERSION, NFS3_COMMIT, &args);
    let reply = disp.dispatch_call(&call);

    let mut dec = XdrDecoder::new(Bytes::from(reply));
    assert_eq!(dec.decode_u32().unwrap(), 104);
    assert_eq!(dec.decode_u32().unwrap(), 1);
    assert_eq!(dec.decode_u32().unwrap(), ACCEPT_SUCCESS);
    assert_eq!(dec.decode_u32().unwrap(), 0);
}

#[test]
fn test_gw_srv_sec_process_tcp_empty_returns_empty() {
    let disp = make_dispatcher();
    let result = disp.process_tcp(&[]);
    assert!(result.is_empty());
}

#[test]
fn test_gw_srv_sec_process_tcp_less_than_4_bytes_returns_empty() {
    let disp = make_dispatcher();
    let result = disp.process_tcp(&[0, 1, 2]);
    assert!(result.is_empty());
}

#[test]
fn test_gw_srv_sec_process_tcp_truncated_fragment_returns_empty() {
    let disp = make_dispatcher();
    let mut enc = XdrEncoder::new();
    enc.encode_u32(200);
    enc.encode_u32(RPC_CALL);
    enc.encode_u32(2);
    enc.encode_u32(NFS_PROGRAM);
    enc.encode_u32(NFS_VERSION);
    enc.encode_u32(NFS3_NULL);
    OpaqueAuth::none().encode_xdr(&mut enc);
    OpaqueAuth::none().encode_xdr(&mut enc);
    let call_data = enc.finish().to_vec();

    let header = 0x80001000u32.to_be_bytes();
    let mut truncated = header.to_vec();
    truncated.extend_from_slice(&call_data[..10.min(call_data.len())]);

    let result = disp.process_tcp(&truncated);
    assert!(result.is_empty());
}

#[test]
fn test_gw_srv_sec_process_tcp_valid_record_returns_response() {
    let disp = make_dispatcher();

    let mut enc = XdrEncoder::new();
    enc.encode_u32(201);
    enc.encode_u32(RPC_CALL);
    enc.encode_u32(2);
    enc.encode_u32(NFS_PROGRAM);
    enc.encode_u32(NFS_VERSION);
    enc.encode_u32(NFS3_NULL);
    OpaqueAuth::none().encode_xdr(&mut enc);
    OpaqueAuth::none().encode_xdr(&mut enc);
    let call_data = enc.finish().to_vec();

    let marked = TcpRecordMark::encode(&call_data);
    let result = disp.process_tcp(&marked);

    assert!(result.len() > 4);
    let header: [u8; 4] = result[..4].try_into().unwrap();
    let (is_last, len) = TcpRecordMark::decode(header);
    assert!(is_last);
    assert!(len > 0);
}

#[test]
fn test_gw_srv_sec_config_default_with_export_creates_valid() {
    let config = NfsServerConfig::default_with_export("/data");
    assert_eq!(config.tcp_port, 2049);
    assert_eq!(config.mount_port, 20048);
    assert!(!config.exports.is_empty());
    assert_eq!(config.exports[0].dirpath, "/data");
}

#[test]
fn test_gw_srv_sec_config_default_tcp_port_is_2049() {
    let config = NfsServerConfig::default_with_export("/");
    assert_eq!(config.tcp_port, 2049);
}

#[test]
fn test_gw_srv_sec_config_default_mount_port_is_20048() {
    let config = NfsServerConfig::default_with_export("/");
    assert_eq!(config.mount_port, 20048);
}

#[test]
fn test_gw_srv_sec_getattr_on_root_returns_success() {
    let disp = make_dispatcher();
    let args = encode_root_fh();
    let call = make_nfs_call(300, NFS_PROGRAM, NFS_VERSION, NFS3_GETATTR, &args);
    let reply = disp.dispatch_call(&call);

    let mut dec = XdrDecoder::new(Bytes::from(reply));
    assert_eq!(dec.decode_u32().unwrap(), 300);
    assert_eq!(dec.decode_u32().unwrap(), 1);
    assert_eq!(dec.decode_u32().unwrap(), ACCEPT_SUCCESS);
}

#[test]
fn test_gw_srv_sec_lookup_dot_on_root_returns_success() {
    let disp = make_dispatcher();
    let mut args_enc = XdrEncoder::new();
    let root_fh = FileHandle3::from_inode(1);
    root_fh.encode_xdr(&mut args_enc);
    args_enc.encode_string(".");
    let args = args_enc.finish().to_vec();

    let call = make_nfs_call(301, NFS_PROGRAM, NFS_VERSION, NFS3_LOOKUP, &args);
    let reply = disp.dispatch_call(&call);

    let mut dec = XdrDecoder::new(Bytes::from(reply));
    assert_eq!(dec.decode_u32().unwrap(), 301);
    assert_eq!(dec.decode_u32().unwrap(), 1);
    assert_eq!(dec.decode_u32().unwrap(), ACCEPT_SUCCESS);
}

#[test]
fn test_gw_srv_sec_getattr_with_garbage_args_returns_garbage_args() {
    let disp = make_dispatcher();
    let garbage = vec![0xDE, 0xAD, 0xBE, 0xEF];
    let call = make_nfs_call(302, NFS_PROGRAM, NFS_VERSION, NFS3_GETATTR, &garbage);
    let reply = disp.dispatch_call(&call);

    let mut dec = XdrDecoder::new(Bytes::from(reply));
    assert_eq!(dec.decode_u32().unwrap(), 302);
    assert_eq!(dec.decode_u32().unwrap(), 1);
    assert_eq!(dec.decode_u32().unwrap(), ACCEPT_GARBAGE_ARGS);
}

#[test]
fn test_gw_srv_sec_lookup_with_garbage_args_returns_garbage_args() {
    let disp = make_dispatcher();
    let garbage = vec![0xBA, 0xAD, 0xF0, 0x0D];
    let call = make_nfs_call(303, NFS_PROGRAM, NFS_VERSION, NFS3_LOOKUP, &garbage);
    let reply = disp.dispatch_call(&call);

    let mut dec = XdrDecoder::new(Bytes::from(reply));
    assert_eq!(dec.decode_u32().unwrap(), 303);
    assert_eq!(dec.decode_u32().unwrap(), 1);
    assert_eq!(dec.decode_u32().unwrap(), ACCEPT_GARBAGE_ARGS);
}

#[test]
fn test_gw_srv_sec_readdir_on_root_returns_success() {
    let disp = make_dispatcher();
    let mut args_enc = XdrEncoder::new();
    let root_fh = FileHandle3::from_inode(1);
    root_fh.encode_xdr(&mut args_enc);
    args_enc.encode_u64(0);
    args_enc.encode_u64(0);
    args_enc.encode_u32(4096);
    let args = args_enc.finish().to_vec();

    let call = make_nfs_call(304, NFS_PROGRAM, NFS_VERSION, NFS3_READDIR, &args);
    let reply = disp.dispatch_call(&call);

    let mut dec = XdrDecoder::new(Bytes::from(reply));
    assert_eq!(dec.decode_u32().unwrap(), 304);
    assert_eq!(dec.decode_u32().unwrap(), 1);
    assert_eq!(dec.decode_u32().unwrap(), ACCEPT_SUCCESS);
}

#[test]
fn test_gw_srv_sec_mount_mnt_root_succeeds() {
    let disp = make_dispatcher();
    let mut args_enc = XdrEncoder::new();
    args_enc.encode_string("/");
    let args = args_enc.finish().to_vec();

    let call = make_mount_call(400, MNT3_MNT, &args);
    let reply = disp.dispatch_call(&call);

    let mut dec = XdrDecoder::new(Bytes::from(reply));
    assert_eq!(dec.decode_u32().unwrap(), 400);
    assert_eq!(dec.decode_u32().unwrap(), 1);
    assert_eq!(dec.decode_u32().unwrap(), ACCEPT_SUCCESS);
    let status = dec.decode_u32().unwrap();
    assert_eq!(status, 0);
}

#[test]
fn test_gw_srv_sec_mount_export_returns_exports_list() {
    let disp = make_dispatcher();
    let call = make_mount_call(401, MNT3_EXPORT, &[]);
    let reply = disp.dispatch_call(&call);

    let mut dec = XdrDecoder::new(Bytes::from(reply));
    assert_eq!(dec.decode_u32().unwrap(), 401);
    assert_eq!(dec.decode_u32().unwrap(), 1);
    assert_eq!(dec.decode_u32().unwrap(), ACCEPT_SUCCESS);
    let _verf_flavor = dec.decode_u32().unwrap();
    let _verf_body = dec.decode_opaque_variable().unwrap();
    let count = dec.decode_u32().unwrap();
    assert!(count >= 1);
}

#[test]
fn test_gw_srv_sec_mount_umnt_returns_success() {
    let disp = make_dispatcher();

    let mut args_enc = XdrEncoder::new();
    args_enc.encode_string("/");
    let args = args_enc.finish().to_vec();

    let call = make_mount_call(402, MNT3_UMNT, &args);
    let reply = disp.dispatch_call(&call);

    let mut dec = XdrDecoder::new(Bytes::from(reply));
    assert_eq!(dec.decode_u32().unwrap(), 402);
    assert_eq!(dec.decode_u32().unwrap(), 1);
    assert_eq!(dec.decode_u32().unwrap(), ACCEPT_SUCCESS);
}

#[test]
fn test_gw_srv_sec_mount_umntall_returns_success() {
    let disp = make_dispatcher();

    let call = make_mount_call(403, MNT3_UMNTALL, &[]);
    let reply = disp.dispatch_call(&call);

    let mut dec = XdrDecoder::new(Bytes::from(reply));
    assert_eq!(dec.decode_u32().unwrap(), 403);
    assert_eq!(dec.decode_u32().unwrap(), 1);
    assert_eq!(dec.decode_u32().unwrap(), ACCEPT_SUCCESS);
}

#[test]
fn test_gw_srv_sec_write_with_data_returns_response() {
    let disp = make_dispatcher();
    let mut args_enc = XdrEncoder::new();
    let root_fh = FileHandle3::from_inode(1);
    root_fh.encode_xdr(&mut args_enc);
    args_enc.encode_u64(0);
    args_enc.encode_u32(5);
    args_enc.encode_u32(0);
    args_enc.encode_opaque_variable(b"hello");
    let args = args_enc.finish().to_vec();

    let call = make_nfs_call(500, NFS_PROGRAM, NFS_VERSION, NFS3_WRITE, &args);
    let reply = disp.dispatch_call(&call);

    let mut dec = XdrDecoder::new(Bytes::from(reply));
    assert_eq!(dec.decode_u32().unwrap(), 500);
    assert_eq!(dec.decode_u32().unwrap(), 1);
    assert_eq!(dec.decode_u32().unwrap(), ACCEPT_SUCCESS);
}

#[test]
fn test_gw_srv_sec_mkdir_creates_directory() {
    let disp = make_dispatcher();
    let mut args_enc = XdrEncoder::new();
    let root_fh = FileHandle3::from_inode(1);
    root_fh.encode_xdr(&mut args_enc);
    args_enc.encode_string("newdir");
    args_enc.encode_u32(0o755);
    let args = args_enc.finish().to_vec();

    let call = make_nfs_call(501, NFS_PROGRAM, NFS_VERSION, NFS3_MKDIR, &args);
    let reply = disp.dispatch_call(&call);

    let mut dec = XdrDecoder::new(Bytes::from(reply));
    assert_eq!(dec.decode_u32().unwrap(), 501);
    assert_eq!(dec.decode_u32().unwrap(), 1);
    assert_eq!(dec.decode_u32().unwrap(), ACCEPT_SUCCESS);
}

#[test]
fn test_gw_srv_sec_rename_returns_proc_unavail() {
    let disp = make_dispatcher();
    let call = make_nfs_call(502, NFS_PROGRAM, NFS_VERSION, NFS3_RENAME, &[]);
    let reply = disp.dispatch_call(&call);

    let mut dec = XdrDecoder::new(Bytes::from(reply));
    assert_eq!(dec.decode_u32().unwrap(), 502);
    assert_eq!(dec.decode_u32().unwrap(), 1);
    assert_eq!(dec.decode_u32().unwrap(), ACCEPT_PROC_UNAVAIL);
}

#[test]
fn test_gw_srv_sec_setattr_returns_proc_unavail() {
    let disp = make_dispatcher();
    let call = make_nfs_call(503, NFS_PROGRAM, NFS_VERSION, NFS3_SETATTR, &[]);
    let reply = disp.dispatch_call(&call);

    let mut dec = XdrDecoder::new(Bytes::from(reply));
    assert_eq!(dec.decode_u32().unwrap(), 503);
    assert_eq!(dec.decode_u32().unwrap(), 1);
    assert_eq!(dec.decode_u32().unwrap(), ACCEPT_PROC_UNAVAIL);
}

#[test]
fn test_gw_srv_sec_dispatch_raw_bytes_via_process_tcp() {
    let disp = make_dispatcher();

    let mut enc = XdrEncoder::new();
    enc.encode_u32(600);
    enc.encode_u32(RPC_CALL);
    enc.encode_u32(2);
    enc.encode_u32(NFS_PROGRAM);
    enc.encode_u32(NFS_VERSION);
    enc.encode_u32(NFS3_NULL);
    OpaqueAuth::none().encode_xdr(&mut enc);
    OpaqueAuth::none().encode_xdr(&mut enc);
    let call_data = enc.finish().to_vec();

    let marked = TcpRecordMark::encode(&call_data);
    let reply = disp.dispatch(&marked);

    assert!(reply.len() > 4);
}

#[test]
fn test_gw_srv_sec_dispatch_decodes_reply_xid_matches_call() {
    let disp = make_dispatcher();

    let test_xid = 777u32;
    let call = make_nfs_call(test_xid, NFS_PROGRAM, NFS_VERSION, NFS3_NULL, &[]);
    let reply = disp.dispatch_call(&call);

    let mut dec = XdrDecoder::new(Bytes::from(reply));
    let reply_xid = dec.decode_u32().unwrap();
    assert_eq!(reply_xid, test_xid);
}

#[test]
fn test_gw_srv_sec_garbage_args_truncated_fh() {
    let disp = make_dispatcher();
    let truncated_fh = vec![0x01, 0x02];
    let call = make_nfs_call(800, NFS_PROGRAM, NFS_VERSION, NFS3_GETATTR, &truncated_fh);
    let reply = disp.dispatch_call(&call);

    let mut dec = XdrDecoder::new(Bytes::from(reply));
    assert_eq!(dec.decode_u32().unwrap(), 800);
    assert_eq!(dec.decode_u32().unwrap(), 1);
    assert_eq!(dec.decode_u32().unwrap(), ACCEPT_GARBAGE_ARGS);
}

#[test]
fn test_gw_srv_sec_multiple_calls_independent_replies() {
    let disp = make_dispatcher();

    let call1 = make_nfs_call(901, NFS_PROGRAM, NFS_VERSION, NFS3_NULL, &[]);
    let reply1 = disp.dispatch_call(&call1);

    let call2 = make_nfs_call(902, NFS_PROGRAM, NFS_VERSION, NFS3_NULL, &[]);
    let reply2 = disp.dispatch_call(&call2);

    let mut dec1 = XdrDecoder::new(Bytes::from(reply1));
    assert_eq!(dec1.decode_u32().unwrap(), 901);

    let mut dec2 = XdrDecoder::new(Bytes::from(reply2));
    assert_eq!(dec2.decode_u32().unwrap(), 902);
}

#[test]
fn test_gw_srv_sec_mount_umnt_after_mnt_succeeds() {
    let disp = make_dispatcher();

    let mut mnt_args = XdrEncoder::new();
    mnt_args.encode_string("/");
    let mnt_args_bytes = mnt_args.finish().to_vec();
    let mnt_call = make_mount_call(450, MNT3_MNT, &mnt_args_bytes);
    let mnt_reply = disp.dispatch_call(&mnt_call);

    let mut dec = XdrDecoder::new(Bytes::from(mnt_reply));
    assert_eq!(dec.decode_u32().unwrap(), 450);
    assert_eq!(dec.decode_u32().unwrap(), 1);
    assert_eq!(dec.decode_u32().unwrap(), ACCEPT_SUCCESS);

    let mut umnt_args = XdrEncoder::new();
    umnt_args.encode_string("/");
    let umnt_args_bytes = umnt_args.finish().to_vec();
    let umnt_call = make_mount_call(451, MNT3_UMNT, &umnt_args_bytes);
    let umnt_reply = disp.dispatch_call(&umnt_call);

    let mut dec = XdrDecoder::new(Bytes::from(umnt_reply));
    assert_eq!(dec.decode_u32().unwrap(), 451);
    assert_eq!(dec.decode_u32().unwrap(), 1);
    assert_eq!(dec.decode_u32().unwrap(), ACCEPT_SUCCESS);
}
