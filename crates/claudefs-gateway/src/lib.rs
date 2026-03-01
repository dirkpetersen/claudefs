#![warn(missing_docs)]

//! ClaudeFS gateway subsystem: NFSv3 gateway, pNFS layouts, S3 API endpoint

pub mod access_log;
pub mod auth;
pub mod config;
pub mod error;
pub mod mount;
pub mod nfs;
pub mod nfs_cache;
pub mod nfs_readdirplus;
pub mod pnfs;
pub mod pnfs_flex;
pub mod portmap;
pub mod protocol;
pub mod quota;
pub mod rpc;
pub mod s3;
pub mod s3_multipart;
pub mod s3_router;
pub mod s3_xml;
pub mod server;
pub mod smb;
pub mod token_auth;
pub mod xdr;
