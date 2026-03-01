//! AUTH_SYS (AUTH_UNIX) credential parsing

use crate::error::{GatewayError, Result};
use crate::rpc::{OpaqueAuth, AUTH_NONE, AUTH_SYS};
use crate::xdr::{XdrDecoder, XdrEncoder};

/// Maximum number of supplementary GIDs in AUTH_SYS
pub const AUTH_SYS_MAX_GIDS: usize = 16;

const NOBODY_UID: u32 = 65534;
const NOBODY_GID: u32 = 65534;

#[derive(Debug, Clone)]
pub struct AuthSysCred {
    pub stamp: u32,
    pub machinename: String,
    pub uid: u32,
    pub gid: u32,
    pub gids: Vec<u32>,
}

impl AuthSysCred {
    pub fn from_opaque_auth(auth: &OpaqueAuth) -> Result<Self> {
        if auth.flavor != AUTH_SYS {
            return Err(GatewayError::ProtocolError {
                reason: format!("expected AUTH_SYS, got flavor {}", auth.flavor),
            });
        }
        Self::decode_xdr(&auth.body)
    }

    pub fn decode_xdr(body: &[u8]) -> Result<Self> {
        let mut dec = XdrDecoder::new(prost::bytes::Bytes::copy_from_slice(body));

        let stamp = dec.decode_u32()?;
        let machinename = dec.decode_string()?;
        let uid = dec.decode_u32()?;
        let gid = dec.decode_u32()?;

        let gids_count = dec.decode_u32()? as usize;
        if gids_count > AUTH_SYS_MAX_GIDS {
            return Err(GatewayError::ProtocolError {
                reason: format!("too many gids: {}", gids_count),
            });
        }

        let mut gids = Vec::with_capacity(gids_count);
        for _ in 0..gids_count {
            gids.push(dec.decode_u32()?);
        }

        Ok(Self {
            stamp,
            machinename,
            uid,
            gid,
            gids,
        })
    }

    pub fn encode_xdr(&self) -> Vec<u8> {
        let mut enc = XdrEncoder::new();
        enc.encode_u32(self.stamp);
        enc.encode_string(&self.machinename);
        enc.encode_u32(self.uid);
        enc.encode_u32(self.gid);
        enc.encode_u32(self.gids.len() as u32);
        for gid in &self.gids {
            enc.encode_u32(*gid);
        }
        enc.finish().to_vec()
    }

    pub fn has_uid(&self, uid: u32) -> bool {
        self.uid == uid
    }

    pub fn has_gid(&self, gid: u32) -> bool {
        if self.gid == gid {
            return true;
        }
        self.gids.contains(&gid)
    }

    pub fn is_root(&self) -> bool {
        self.uid == 0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AuthNone;

impl AuthNone {
    pub fn to_opaque_auth() -> OpaqueAuth {
        OpaqueAuth::none()
    }
}

#[derive(Debug, Clone)]
pub enum AuthCred {
    None,
    Sys(AuthSysCred),
    Unknown(u32),
}

impl AuthCred {
    pub fn from_opaque_auth(auth: &OpaqueAuth) -> Self {
        match auth.flavor {
            AUTH_NONE => AuthCred::None,
            AUTH_SYS => match AuthSysCred::decode_xdr(&auth.body) {
                Ok(cred) => AuthCred::Sys(cred),
                Err(_) => AuthCred::Unknown(AUTH_SYS),
            },
            flavor => AuthCred::Unknown(flavor),
        }
    }

    pub fn uid(&self) -> u32 {
        match self {
            AuthCred::None => NOBODY_UID,
            AuthCred::Sys(cred) => cred.uid,
            AuthCred::Unknown(_) => NOBODY_UID,
        }
    }

    pub fn gid(&self) -> u32 {
        match self {
            AuthCred::None => NOBODY_GID,
            AuthCred::Sys(cred) => cred.gid,
            AuthCred::Unknown(_) => NOBODY_GID,
        }
    }

    pub fn is_root(&self) -> bool {
        match self {
            AuthCred::Sys(cred) => cred.is_root(),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::OpaqueAuth;

    #[test]
    fn test_auth_sys_cred_build() {
        let cred = AuthSysCred {
            stamp: 12345,
            machinename: "client.example.com".to_string(),
            uid: 1000,
            gid: 1000,
            gids: vec![1000, 1001, 1002],
        };
        assert_eq!(cred.uid, 1000);
        assert_eq!(cred.gid, 1000);
        assert_eq!(cred.gids.len(), 3);
    }

    #[test]
    fn test_auth_sys_cred_encode_decode_roundtrip() {
        let cred = AuthSysCred {
            stamp: 42,
            machinename: "testhost".to_string(),
            uid: 500,
            gid: 500,
            gids: vec![500, 501],
        };
        let encoded = cred.encode_xdr();
        let decoded = AuthSysCred::decode_xdr(&encoded).unwrap();

        assert_eq!(cred.stamp, decoded.stamp);
        assert_eq!(cred.machinename, decoded.machinename);
        assert_eq!(cred.uid, decoded.uid);
        assert_eq!(cred.gid, decoded.gid);
        assert_eq!(cred.gids, decoded.gids);
    }

    #[test]
    fn test_auth_sys_cred_has_uid() {
        let cred = AuthSysCred {
            stamp: 1,
            machinename: "host".to_string(),
            uid: 1000,
            gid: 1000,
            gids: vec![],
        };
        assert!(cred.has_uid(1000));
        assert!(!cred.has_uid(2000));
    }

    #[test]
    fn test_auth_sys_cred_has_gid_primary() {
        let cred = AuthSysCred {
            stamp: 1,
            machinename: "host".to_string(),
            uid: 1000,
            gid: 1000,
            gids: vec![],
        };
        assert!(cred.has_gid(1000));
        assert!(!cred.has_gid(2000));
    }

    #[test]
    fn test_auth_sys_cred_has_gid_supplementary() {
        let cred = AuthSysCred {
            stamp: 1,
            machinename: "host".to_string(),
            uid: 1000,
            gid: 1000,
            gids: vec![1001, 1002],
        };
        assert!(cred.has_gid(1001));
        assert!(cred.has_gid(1002));
        assert!(!cred.has_gid(1003));
    }

    #[test]
    fn test_auth_sys_cred_is_root() {
        let root_cred = AuthSysCred {
            stamp: 1,
            machinename: "host".to_string(),
            uid: 0,
            gid: 0,
            gids: vec![],
        };
        assert!(root_cred.is_root());

        let user_cred = AuthSysCred {
            stamp: 1,
            machinename: "host".to_string(),
            uid: 1000,
            gid: 1000,
            gids: vec![],
        };
        assert!(!user_cred.is_root());
    }

    #[test]
    fn test_auth_sys_from_opaque_auth() {
        let cred = AuthSysCred {
            stamp: 1,
            machinename: "test".to_string(),
            uid: 100,
            gid: 100,
            gids: vec![],
        };
        let opaque = OpaqueAuth {
            flavor: AUTH_SYS,
            body: cred.encode_xdr(),
        };
        let parsed = AuthSysCred::from_opaque_auth(&opaque).unwrap();
        assert_eq!(parsed.uid, 100);
    }

    #[test]
    fn test_auth_sys_from_opaque_auth_truncated() {
        let opaque = OpaqueAuth {
            flavor: AUTH_SYS,
            body: vec![0, 0, 0, 1],
        };
        let result = AuthSysCred::from_opaque_auth(&opaque);
        assert!(result.is_err());
    }

    #[test]
    fn test_auth_none() {
        let opaque = AuthNone::to_opaque_auth();
        assert_eq!(opaque.flavor, AUTH_NONE);
        assert!(opaque.body.is_empty());
    }

    #[test]
    fn test_auth_cred_from_opaque_auth_none() {
        let opaque = OpaqueAuth::none();
        let cred = AuthCred::from_opaque_auth(&opaque);
        match cred {
            AuthCred::None => (),
            _ => panic!("expected None"),
        }
    }

    #[test]
    fn test_auth_cred_from_opaque_auth_sys() {
        let cred = AuthSysCred {
            stamp: 1,
            machinename: "test".to_string(),
            uid: 1000,
            gid: 1000,
            gids: vec![],
        };
        let opaque = OpaqueAuth {
            flavor: AUTH_SYS,
            body: cred.encode_xdr(),
        };
        let parsed = AuthCred::from_opaque_auth(&opaque);
        match parsed {
            AuthCred::Sys(c) => assert_eq!(c.uid, 1000),
            _ => panic!("expected Sys"),
        }
    }

    #[test]
    fn test_auth_cred_from_opaque_auth_unknown() {
        let opaque = OpaqueAuth {
            flavor: 99,
            body: vec![],
        };
        let cred = AuthCred::from_opaque_auth(&opaque);
        match cred {
            AuthCred::Unknown(flavor) => assert_eq!(flavor, 99),
            _ => panic!("expected Unknown"),
        }
    }

    #[test]
    fn test_auth_cred_uid() {
        let cred_none = AuthCred::None;
        assert_eq!(cred_none.uid(), NOBODY_UID);

        let cred = AuthSysCred {
            stamp: 1,
            machinename: "test".to_string(),
            uid: 500,
            gid: 500,
            gids: vec![],
        };
        let opaque = OpaqueAuth {
            flavor: AUTH_SYS,
            body: cred.encode_xdr(),
        };
        let cred_sys = AuthCred::from_opaque_auth(&opaque);
        assert_eq!(cred_sys.uid(), 500);
    }

    #[test]
    fn test_auth_cred_gid() {
        let cred_none = AuthCred::None;
        assert_eq!(cred_none.gid(), NOBODY_GID);

        let cred = AuthSysCred {
            stamp: 1,
            machinename: "test".to_string(),
            uid: 500,
            gid: 600,
            gids: vec![],
        };
        let opaque = OpaqueAuth {
            flavor: AUTH_SYS,
            body: cred.encode_xdr(),
        };
        let cred_sys = AuthCred::from_opaque_auth(&opaque);
        assert_eq!(cred_sys.gid(), 600);
    }

    #[test]
    fn test_auth_cred_is_root() {
        let cred_none = AuthCred::None;
        assert!(!cred_none.is_root());

        let cred = AuthSysCred {
            stamp: 1,
            machinename: "test".to_string(),
            uid: 0,
            gid: 0,
            gids: vec![],
        };
        let opaque = OpaqueAuth {
            flavor: AUTH_SYS,
            body: cred.encode_xdr(),
        };
        let cred_sys = AuthCred::from_opaque_auth(&opaque);
        assert!(cred_sys.is_root());
    }
}
