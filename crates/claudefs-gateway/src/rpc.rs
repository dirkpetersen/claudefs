//! ONC RPC / SunRPC wire protocol (RFC 5531)

use crate::error::{GatewayError, Result};
use crate::xdr::{XdrDecoder, XdrEncoder};

/// NFS program number (100003)
pub const NFS_PROGRAM: u32 = 100003;
/// NFS version 3
pub const NFS_VERSION: u32 = 3;
/// MOUNT program number (100005)
pub const MOUNT_PROGRAM: u32 = 100005;
/// MOUNT version 3
pub const MOUNT_VERSION: u32 = 3;
/// PORTMAP/RPCBIND program number (100000)
pub const PORTMAP_PROGRAM: u32 = 100000;
/// PORTMAP/RPCBIND version 2
pub const PORTMAP_VERSION: u32 = 2;

/// RPC message type: CALL
pub const RPC_CALL: u32 = 0;
/// RPC message type: REPLY
pub const RPC_REPLY: u32 = 1;

/// RPC authentication flavor: none
pub const AUTH_NONE: u32 = 0;
/// RPC authentication flavor: sys (AUTH_UNIX)
pub const AUTH_SYS: u32 = 1;
/// RPC authentication flavor: GSSAPI
pub const AUTH_GSS: u32 = 6;

/// RPC accept status: success
pub const ACCEPT_SUCCESS: u32 = 0;
/// RPC accept status: program not available
pub const ACCEPT_PROG_UNAVAIL: u32 = 1;
/// RPC accept status: program version mismatch
pub const ACCEPT_PROG_MISMATCH: u32 = 2;
/// RPC accept status: procedure not available
pub const ACCEPT_PROC_UNAVAIL: u32 = 3;
/// RPC accept status: garbage arguments
pub const ACCEPT_GARBAGE_ARGS: u32 = 4;

/// RPC reject status: RPC version mismatch
pub const REJECT_RPC_MISMATCH: u32 = 0;
/// RPC reject status: authentication error
pub const REJECT_AUTH_ERROR: u32 = 1;

pub const NFS3_NULL: u32 = 0;
pub const NFS3_GETATTR: u32 = 1;
pub const NFS3_SETATTR: u32 = 2;
pub const NFS3_LOOKUP: u32 = 3;
pub const NFS3_ACCESS: u32 = 4;
pub const NFS3_READLINK: u32 = 5;
pub const NFS3_READ: u32 = 6;
pub const NFS3_WRITE: u32 = 7;
pub const NFS3_CREATE: u32 = 8;
pub const NFS3_MKDIR: u32 = 9;
pub const NFS3_SYMLINK: u32 = 10;
pub const NFS3_MKNOD: u32 = 11;
pub const NFS3_REMOVE: u32 = 12;
pub const NFS3_RMDIR: u32 = 13;
pub const NFS3_RENAME: u32 = 14;
pub const NFS3_LINK: u32 = 15;
pub const NFS3_READDIR: u32 = 16;
pub const NFS3_READDIRPLUS: u32 = 17;
pub const NFS3_FSSTAT: u32 = 18;
pub const NFS3_FSINFO: u32 = 19;
pub const NFS3_PATHCONF: u32 = 20;
pub const NFS3_COMMIT: u32 = 21;

pub const MNT3_NULL: u32 = 0;
pub const MNT3_MNT: u32 = 1;
pub const MNT3_DUMP: u32 = 2;
pub const MNT3_UMNT: u32 = 3;
pub const MNT3_UMNTALL: u32 = 4;
pub const MNT3_EXPORT: u32 = 5;

#[derive(Debug, Clone)]
pub struct OpaqueAuth {
    pub flavor: u32,
    pub body: Vec<u8>,
}

impl OpaqueAuth {
    pub fn none() -> Self {
        Self {
            flavor: AUTH_NONE,
            body: vec![],
        }
    }

    pub fn encode_xdr(&self, enc: &mut XdrEncoder) {
        enc.encode_u32(self.flavor);
        enc.encode_opaque_variable(&self.body);
    }

    pub fn decode_xdr(dec: &mut XdrDecoder) -> Result<Self> {
        let flavor = dec.decode_u32()?;
        let body = dec.decode_opaque_variable()?;
        Ok(Self { flavor, body })
    }
}

#[derive(Debug, Clone)]
pub struct RpcCall {
    pub xid: u32,
    pub rpcvers: u32,
    pub prog: u32,
    pub vers: u32,
    pub proc: u32,
    pub cred: OpaqueAuth,
    pub verf: OpaqueAuth,
    pub args_bytes: Vec<u8>,
}

impl RpcCall {
    pub fn decode(data: &[u8]) -> Result<Self> {
        let mut dec = XdrDecoder::new(prost::bytes::Bytes::copy_from_slice(data));

        let xid = dec.decode_u32()?;
        let msg_type = dec.decode_u32()?;

        if msg_type != RPC_CALL {
            return Err(GatewayError::ProtocolError {
                reason: format!("expected RPC CALL, got {}", msg_type),
            });
        }

        let rpcvers = dec.decode_u32()?;
        let prog = dec.decode_u32()?;
        let vers = dec.decode_u32()?;
        let proc = dec.decode_u32()?;

        let cred = OpaqueAuth::decode_xdr(&mut dec)?;
        let verf = OpaqueAuth::decode_xdr(&mut dec)?;

        let args_bytes = dec.remaining_bytes();

        Ok(Self {
            xid,
            rpcvers,
            prog,
            vers,
            proc,
            cred,
            verf,
            args_bytes,
        })
    }
}

#[derive(Debug)]
pub struct RpcReply {
    pub xid: u32,
    pub verf: OpaqueAuth,
    pub result_bytes: Vec<u8>,
}

impl RpcReply {
    pub fn encode_success(xid: u32, result: &[u8]) -> Vec<u8> {
        let mut enc = XdrEncoder::new();
        enc.encode_u32(xid);
        enc.encode_u32(RPC_REPLY);
        enc.encode_u32(ACCEPT_SUCCESS);
        OpaqueAuth::none().encode_xdr(&mut enc);
        enc.encode_opaque_variable(result);
        enc.finish().to_vec()
    }

    pub fn encode_proc_unavail(xid: u32) -> Vec<u8> {
        let mut enc = XdrEncoder::new();
        enc.encode_u32(xid);
        enc.encode_u32(RPC_REPLY);
        enc.encode_u32(ACCEPT_PROC_UNAVAIL);
        OpaqueAuth::none().encode_xdr(&mut enc);
        enc.finish().to_vec()
    }

    pub fn encode_prog_mismatch(xid: u32, low: u32, high: u32) -> Vec<u8> {
        let mut enc = XdrEncoder::new();
        enc.encode_u32(xid);
        enc.encode_u32(RPC_REPLY);
        enc.encode_u32(REJECT_RPC_MISMATCH);
        enc.encode_u32(low);
        enc.encode_u32(high);
        enc.finish().to_vec()
    }

    pub fn encode_auth_error(xid: u32, stat: u32) -> Vec<u8> {
        let mut enc = XdrEncoder::new();
        enc.encode_u32(xid);
        enc.encode_u32(RPC_REPLY);
        enc.encode_u32(REJECT_AUTH_ERROR);
        enc.encode_u32(stat);
        enc.finish().to_vec()
    }

    pub fn encode_garbage_args(xid: u32) -> Vec<u8> {
        let mut enc = XdrEncoder::new();
        enc.encode_u32(xid);
        enc.encode_u32(RPC_REPLY);
        enc.encode_u32(ACCEPT_GARBAGE_ARGS);
        OpaqueAuth::none().encode_xdr(&mut enc);
        enc.finish().to_vec()
    }
}

pub struct TcpRecordMark;

impl TcpRecordMark {
    pub fn encode(data: &[u8]) -> Vec<u8> {
        let mut result = Vec::with_capacity(4 + data.len());
        let fragment_len = data.len() as u32 | 0x80000000;
        result.extend_from_slice(&fragment_len.to_be_bytes());
        result.extend_from_slice(data);
        result
    }

    pub fn decode(header: [u8; 4]) -> (bool, u32) {
        let val = u32::from_be_bytes(header);
        let is_last = (val & 0x80000000) != 0;
        let fragment_len = val & 0x7FFFFFFF;
        (is_last, fragment_len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::bytes::Bytes;

    #[test]
    fn test_opaque_auth_none() {
        let auth = OpaqueAuth::none();
        assert_eq!(auth.flavor, AUTH_NONE);
        assert!(auth.body.is_empty());
    }

    #[test]
    fn test_opaque_auth_encode_decode() {
        let auth = OpaqueAuth {
            flavor: AUTH_SYS,
            body: vec![1, 2, 3, 4],
        };
        let mut enc = XdrEncoder::new();
        auth.encode_xdr(&mut enc);
        let buf = enc.finish();

        let mut dec = XdrDecoder::new(buf);
        let decoded = OpaqueAuth::decode_xdr(&mut dec).unwrap();
        assert_eq!(decoded.flavor, AUTH_SYS);
        assert_eq!(decoded.body, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_opaque_auth_encode_decode_roundtrip() {
        let auth = OpaqueAuth {
            flavor: AUTH_GSS,
            body: b"gss_token".to_vec(),
        };
        let mut enc = XdrEncoder::new();
        auth.encode_xdr(&mut enc);
        let buf = enc.finish();

        let mut dec = XdrDecoder::new(buf);
        let decoded = OpaqueAuth::decode_xdr(&mut dec).unwrap();
        assert_eq!(auth.flavor, decoded.flavor);
        assert_eq!(auth.body, decoded.body);
    }

    #[test]
    fn test_rpccall_decode_valid_call() {
        let mut enc = XdrEncoder::new();
        enc.encode_u32(12345);
        enc.encode_u32(RPC_CALL);
        enc.encode_u32(2);
        enc.encode_u32(NFS_PROGRAM);
        enc.encode_u32(NFS_VERSION);
        enc.encode_u32(NFS3_GETATTR);
        OpaqueAuth::none().encode_xdr(&mut enc);
        OpaqueAuth::none().encode_xdr(&mut enc);
        enc.encode_opaque_variable(b"arg_data");
        let data = enc.finish().to_vec();

        let call = RpcCall::decode(&data).unwrap();
        assert_eq!(call.xid, 12345);
        assert_eq!(call.rpcvers, 2);
        assert_eq!(call.prog, NFS_PROGRAM);
        assert_eq!(call.vers, NFS_VERSION);
        assert_eq!(call.proc, NFS3_GETATTR);
        assert_eq!(call.cred.flavor, AUTH_NONE);
    }

    #[test]
    fn test_rpccall_decode_wrong_msg_type() {
        let mut enc = XdrEncoder::new();
        enc.encode_u32(12345);
        enc.encode_u32(RPC_REPLY);
        enc.encode_u32(2);
        enc.encode_u32(NFS_PROGRAM);
        let data = enc.finish().to_vec();

        let result = RpcCall::decode(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_rpcreply_encode_success() {
        let reply = RpcReply::encode_success(123, b"result_data");
        let mut dec = XdrDecoder::new(Bytes::from(reply));
        assert_eq!(dec.decode_u32().unwrap(), 123);
        assert_eq!(dec.decode_u32().unwrap(), RPC_REPLY);
        assert_eq!(dec.decode_u32().unwrap(), ACCEPT_SUCCESS);
    }

    #[test]
    fn test_rpcreply_encode_proc_unavail() {
        let reply = RpcReply::encode_proc_unavail(456);
        let mut dec = XdrDecoder::new(Bytes::from(reply));
        assert_eq!(dec.decode_u32().unwrap(), 456);
        assert_eq!(dec.decode_u32().unwrap(), RPC_REPLY);
        assert_eq!(dec.decode_u32().unwrap(), ACCEPT_PROC_UNAVAIL);
    }

    #[test]
    fn test_rpcreply_encode_prog_mismatch() {
        let reply = RpcReply::encode_prog_mismatch(789, 2, 4);
        let mut dec = XdrDecoder::new(Bytes::from(reply));
        assert_eq!(dec.decode_u32().unwrap(), 789);
        assert_eq!(dec.decode_u32().unwrap(), RPC_REPLY);
        assert_eq!(dec.decode_u32().unwrap(), REJECT_RPC_MISMATCH);
        assert_eq!(dec.decode_u32().unwrap(), 2);
        assert_eq!(dec.decode_u32().unwrap(), 4);
    }

    #[test]
    fn test_rpcreply_encode_auth_error() {
        let reply = RpcReply::encode_auth_error(999, 7);
        let mut dec = XdrDecoder::new(Bytes::from(reply));
        assert_eq!(dec.decode_u32().unwrap(), 999);
        assert_eq!(dec.decode_u32().unwrap(), RPC_REPLY);
        assert_eq!(dec.decode_u32().unwrap(), REJECT_AUTH_ERROR);
        assert_eq!(dec.decode_u32().unwrap(), 7);
    }

    #[test]
    fn test_rpcreply_encode_garbage_args() {
        let reply = RpcReply::encode_garbage_args(111);
        let mut dec = XdrDecoder::new(Bytes::from(reply));
        assert_eq!(dec.decode_u32().unwrap(), 111);
        assert_eq!(dec.decode_u32().unwrap(), RPC_REPLY);
        assert_eq!(dec.decode_u32().unwrap(), ACCEPT_GARBAGE_ARGS);
    }

    #[test]
    fn test_tcp_record_mark_encode() {
        let data = b"test_data";
        let encoded = TcpRecordMark::encode(data);
        let header: [u8; 4] = encoded[..4].try_into().unwrap();
        let (is_last, len) = TcpRecordMark::decode(header);
        assert!(is_last);
        assert_eq!(len as usize, data.len());
        assert_eq!(&encoded[4..], data);
    }

    #[test]
    fn test_tcp_record_mark_decode_last_fragment() {
        let header = 0x80000009u32.to_be_bytes();
        let (is_last, len) = TcpRecordMark::decode(header);
        assert!(is_last);
        assert_eq!(len, 9);
    }

    #[test]
    fn test_tcp_record_mark_decode_not_last_fragment() {
        let header = 0x00000100u32.to_be_bytes();
        let (is_last, len) = TcpRecordMark::decode(header);
        assert!(!is_last);
        assert_eq!(len, 256);
    }

    #[test]
    fn test_rpccall_decode_truncated() {
        let data = vec![0u8; 10];
        let result = RpcCall::decode(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_rpcreply_roundtrip() {
        let original = RpcReply::encode_success(42, b"payload");
        let mut dec = XdrDecoder::new(Bytes::from(original));
        let xid = dec.decode_u32().unwrap();
        let msg_type = dec.decode_u32().unwrap();
        let accept_stat = dec.decode_u32().unwrap();

        assert_eq!(xid, 42);
        assert_eq!(msg_type, RPC_REPLY);
        assert_eq!(accept_stat, ACCEPT_SUCCESS);
    }

    #[test]
    fn test_opaque_auth_with_gss() {
        let auth = OpaqueAuth {
            flavor: AUTH_GSS,
            body: b"gss_token_data".to_vec(),
        };
        let mut enc = XdrEncoder::new();
        auth.encode_xdr(&mut enc);
        let buf = enc.finish();

        let mut dec = XdrDecoder::new(buf);
        let decoded = OpaqueAuth::decode_xdr(&mut dec).unwrap();
        assert_eq!(decoded.flavor, AUTH_GSS);
    }

    #[test]
    fn test_rpccall_with_auth_sys() {
        let mut enc = XdrEncoder::new();
        enc.encode_u32(999);
        enc.encode_u32(RPC_CALL);
        enc.encode_u32(2);
        enc.encode_u32(NFS_PROGRAM);
        enc.encode_u32(NFS_VERSION);
        enc.encode_u32(NFS3_GETATTR);

        let mut auth_body = XdrEncoder::new();
        auth_body.encode_u32(12345);
        auth_body.encode_string("clienthost");
        auth_body.encode_u32(1000);
        auth_body.encode_u32(1000);
        auth_body.encode_u32(0);

        enc.encode_u32(AUTH_SYS);
        enc.encode_opaque_variable(&auth_body.finish());

        OpaqueAuth::none().encode_xdr(&mut enc);

        let data = enc.finish().to_vec();
        let call = RpcCall::decode(&data).unwrap();

        assert_eq!(call.xid, 999);
        assert_eq!(call.cred.flavor, AUTH_SYS);
    }

    #[test]
    fn test_tcp_record_mark_fragment_boundary() {
        let data = vec![0u8; 12];
        let encoded = TcpRecordMark::encode(&data);

        let header: [u8; 4] = encoded[..4].try_into().unwrap();
        let (is_last, len) = TcpRecordMark::decode(header);

        assert!(is_last);
        assert_eq!(len, 12);
    }

    #[test]
    fn test_rpcreply_encode_proc_unavail_verification() {
        let reply = RpcReply::encode_proc_unavail(555);
        let mut dec = XdrDecoder::new(Bytes::from(reply));

        assert_eq!(dec.decode_u32().unwrap(), 555);
        assert_eq!(dec.decode_u32().unwrap(), RPC_REPLY);
        assert_eq!(dec.decode_u32().unwrap(), ACCEPT_PROC_UNAVAIL);
    }

    #[test]
    fn test_rpcreply_encode_garbage_args_verification() {
        let reply = RpcReply::encode_garbage_args(777);
        let mut dec = XdrDecoder::new(Bytes::from(reply));

        assert_eq!(dec.decode_u32().unwrap(), 777);
        assert_eq!(dec.decode_u32().unwrap(), RPC_REPLY);
        assert_eq!(dec.decode_u32().unwrap(), ACCEPT_GARBAGE_ARGS);
    }
}
