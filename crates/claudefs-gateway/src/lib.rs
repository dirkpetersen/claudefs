#![warn(missing_docs)]

//! ClaudeFS gateway subsystem: NFSv3 gateway, pNFS layouts, S3 API endpoint

pub mod nfs;
pub mod pnfs;
pub mod s3;
pub mod smb;
pub mod protocol;