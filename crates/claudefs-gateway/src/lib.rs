#![warn(missing_docs)]

//! ClaudeFS gateway subsystem: NFSv3 gateway, pNFS layouts, S3 API endpoint

pub mod auth;
pub mod error;
pub mod mount;
pub mod nfs;
pub mod pnfs;
pub mod portmap;
pub mod protocol;
pub mod rpc;
pub mod s3;
pub mod server;
pub mod smb;
pub mod xdr;
