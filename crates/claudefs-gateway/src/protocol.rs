//! NFSv3 protocol types and utilities

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{GatewayError, Result};
use crate::xdr::{XdrDecoder, XdrEncoder};

/// Maximum size of an NFSv3 file handle in bytes.
pub const NFS3_FHSIZE: usize = 64;

/// NFSv3 file handle - an opaque identifier for a file or directory.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileHandle3 {
    /// Raw file handle data
    pub data: Vec<u8>,
}

impl FileHandle3 {
    /// Creates a new file handle with the given data.
    pub fn new(data: Vec<u8>) -> Result<Self> {
        if data.is_empty() {
            return Err(GatewayError::ProtocolError {
                reason: "file handle cannot be empty".to_string(),
            });
        }
        if data.len() > NFS3_FHSIZE {
            return Err(GatewayError::ProtocolError {
                reason: "file handle exceeds 64 bytes".to_string(),
            });
        }
        Ok(Self { data })
    }

    /// Creates a file handle from an inode number (8-byte little-endian encoding).
    pub fn from_inode(inode: u64) -> Self {
        let bytes = inode.to_le_bytes();
        Self {
            data: bytes.to_vec(),
        }
    }

    /// Extracts an inode number from the file handle (if 8 bytes).
    pub fn as_inode(&self) -> Option<u64> {
        if self.data.len() != 8 {
            return None;
        }
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&self.data);
        Some(u64::from_le_bytes(bytes))
    }

    /// Encodes the file handle to XDR format.
    pub fn encode_xdr(&self, enc: &mut XdrEncoder) {
        enc.encode_opaque_variable(&self.data);
    }

    /// Decodes a file handle from XDR format.
    pub fn decode_xdr(dec: &mut XdrDecoder) -> Result<Self> {
        let data = dec.decode_opaque_variable()?;
        Self::new(data)
    }
}

/// NFSv3 timestamp with seconds and nanoseconds precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Nfstime3 {
    /// Seconds since Unix epoch
    pub seconds: u32,
    /// Nanoseconds component
    pub nseconds: u32,
}

impl Nfstime3 {
    /// Creates a timestamp representing the current time.
    pub fn now() -> Self {
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Self {
            seconds: duration.as_secs() as u32,
            nseconds: duration.subsec_nanos(),
        }
    }

    /// Creates a zero timestamp (Unix epoch).
    pub fn zero() -> Self {
        Self {
            seconds: 0,
            nseconds: 0,
        }
    }

    /// Encodes the timestamp to XDR format.
    pub fn encode_xdr(&self, enc: &mut XdrEncoder) {
        enc.encode_u32(self.seconds);
        enc.encode_u32(self.nseconds);
    }

    /// Decodes a timestamp from XDR format.
    pub fn decode_xdr(dec: &mut XdrDecoder) -> Result<Self> {
        Ok(Self {
            seconds: dec.decode_u32()?,
            nseconds: dec.decode_u32()?,
        })
    }
}

/// NFSv3 file type enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u32)]
pub enum Ftype3 {
    /// Regular file
    Reg = 1,
    /// Directory
    Dir = 2,
    /// Block device
    Blk = 3,
    /// Character device
    Chr = 4,
    /// Symbolic link
    Lnk = 5,
    /// Unix domain socket
    Sock = 6,
    /// Named pipe (FIFO)
    Fifo = 7,
}

impl Ftype3 {
    /// Creates a file type from a raw u32 value.
    pub fn from_u32(v: u32) -> Result<Self> {
        match v {
            1 => Ok(Ftype3::Reg),
            2 => Ok(Ftype3::Dir),
            3 => Ok(Ftype3::Blk),
            4 => Ok(Ftype3::Chr),
            5 => Ok(Ftype3::Lnk),
            6 => Ok(Ftype3::Sock),
            7 => Ok(Ftype3::Fifo),
            _ => Err(GatewayError::ProtocolError {
                reason: format!("invalid file type: {}", v),
            }),
        }
    }

    /// Encodes the file type to XDR format.
    pub fn encode_xdr(&self, enc: &mut XdrEncoder) {
        enc.encode_u32(*self as u32);
    }

    /// Decodes a file type from XDR format.
    pub fn decode_xdr(dec: &mut XdrDecoder) -> Result<Self> {
        let v = dec.decode_u32()?;
        Self::from_u32(v)
    }
}

/// NFSv3 file attributes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fattr3 {
    /// File type
    pub ftype: Ftype3,
    /// Unix permission bits
    pub mode: u32,
    /// Number of hard links
    pub nlink: u32,
    /// Owner user ID
    pub uid: u32,
    /// Owner group ID
    pub gid: u32,
    /// File size in bytes
    pub size: u64,
    /// Bytes allocated on disk
    pub used: u64,
    /// Device number (for block/char devices)
    pub rdev: (u32, u32),
    /// Filesystem ID
    pub fsid: u64,
    /// Inode number
    pub fileid: u64,
    /// Last access time
    pub atime: Nfstime3,
    /// Last modification time
    pub mtime: Nfstime3,
    /// Last metadata change time
    pub ctime: Nfstime3,
}

impl Fattr3 {
    /// Encodes file attributes to XDR format.
    pub fn encode_xdr(&self, enc: &mut XdrEncoder) {
        self.ftype.encode_xdr(enc);
        enc.encode_u32(self.mode);
        enc.encode_u32(self.nlink);
        enc.encode_u32(self.uid);
        enc.encode_u32(self.gid);
        enc.encode_u64(self.size);
        enc.encode_u64(self.used);
        enc.encode_u32(self.rdev.0);
        enc.encode_u32(self.rdev.1);
        enc.encode_u64(self.fsid);
        enc.encode_u64(self.fileid);
        self.atime.encode_xdr(enc);
        self.mtime.encode_xdr(enc);
        self.ctime.encode_xdr(enc);
    }

    /// Decodes file attributes from XDR format.
    pub fn decode_xdr(dec: &mut XdrDecoder) -> Result<Self> {
        Ok(Self {
            ftype: Ftype3::decode_xdr(dec)?,
            mode: dec.decode_u32()?,
            nlink: dec.decode_u32()?,
            uid: dec.decode_u32()?,
            gid: dec.decode_u32()?,
            size: dec.decode_u64()?,
            used: dec.decode_u64()?,
            rdev: (dec.decode_u32()?, dec.decode_u32()?),
            fsid: dec.decode_u64()?,
            fileid: dec.decode_u64()?,
            atime: Nfstime3::decode_xdr(dec)?,
            mtime: Nfstime3::decode_xdr(dec)?,
            ctime: Nfstime3::decode_xdr(dec)?,
        })
    }

    /// Creates default attributes for a directory.
    pub fn default_dir(inode: u64, fsid: u64) -> Self {
        let now = Nfstime3::now();
        Self {
            ftype: Ftype3::Dir,
            mode: 0o755,
            nlink: 2,
            uid: 0,
            gid: 0,
            size: 4096,
            used: 4096,
            rdev: (0, 0),
            fsid,
            fileid: inode,
            atime: now,
            mtime: now,
            ctime: now,
        }
    }

    /// Creates default attributes for a regular file.
    pub fn default_file(inode: u64, size: u64, fsid: u64) -> Self {
        let now = Nfstime3::now();
        let used = size.div_ceil(4096) * 4096;
        Self {
            ftype: Ftype3::Reg,
            mode: 0o644,
            nlink: 1,
            uid: 0,
            gid: 0,
            size,
            used,
            rdev: (0, 0),
            fsid,
            fileid: inode,
            atime: now,
            mtime: now,
            ctime: now,
        }
    }
}

/// Result of NFS LOOKUP operation - contains the resolved file handle and attributes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LookupResult {
    /// File handle of the resolved file or directory
    pub object: FileHandle3,
    /// Attributes of the resolved object
    pub obj_attributes: Option<Fattr3>,
    /// Attributes of the parent directory
    pub dir_attributes: Option<Fattr3>,
}

/// Single directory entry in NFSv3 READDIR response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry3 {
    /// Inode number of the entry
    pub fileid: u64,
    /// Name of the entry
    pub name: String,
    /// Opaque cookie for continuing readdir
    pub cookie: u64,
}

/// Result of NFSv3 READDIR operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadDirResult {
    /// Attributes of the directory
    pub dir_attributes: Option<Fattr3>,
    /// Opaque verifier for cookie consistency
    pub cookieverf: u64,
    /// List of directory entries
    pub entries: Vec<Entry3>,
    /// True if end of directory reached
    pub eof: bool,
}

/// Directory entry with attributes for NFSv3 READDIRPLUS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entryplus3 {
    /// Inode number of the entry
    pub fileid: u64,
    /// Name of the entry
    pub name: String,
    /// Opaque cookie for continuing readdirplus
    pub cookie: u64,
    /// Attributes of the entry
    pub name_attributes: Option<Fattr3>,
    /// File handle of the entry
    pub name_handle: Option<FileHandle3>,
}

/// Filesystem statistics result from NFSv3 FSSTAT.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FsStatResult {
    /// Total bytes on filesystem
    pub tbytes: u64,
    /// Free bytes available to client
    pub fbytes: u64,
    /// Free bytes on filesystem
    pub abytes: u64,
    /// Total files on filesystem
    pub tfiles: u64,
    /// Free files available to client
    pub ffiles: u64,
    /// Free files on filesystem
    pub afiles: u32,
    /// Interval seconds for dynamic storage
    pub invarsec: u32,
}

/// Filesystem info result from NFSv3 FSINFO.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FsInfoResult {
    /// Maximum read size
    pub rtmax: u32,
    /// Preferred read size
    pub rtpref: u32,
    /// Recommended read multiple
    pub rtmult: u32,
    /// Maximum write size
    pub wtmax: u32,
    /// Preferred write size
    pub wtpref: u32,
    /// Recommended write multiple
    pub wtmult: u32,
    /// Preferred directory read size
    pub dtpref: u32,
    /// Maximum file size
    pub maxfilesize: u64,
    /// Time granularity for timestamps
    pub time_delta: Nfstime3,
    /// FS capability flags
    pub properties: u32,
}

impl FsInfoResult {
    /// Creates default filesystem info suitable for ClaudeFS.
    pub fn defaults() -> Self {
        Self {
            rtmax: 1048576,
            rtpref: 65536,
            rtmult: 4096,
            wtmax: 1048576,
            wtpref: 65536,
            wtmult: 4096,
            dtpref: 65536,
            maxfilesize: u64::MAX,
            time_delta: Nfstime3::zero(),
            properties: 0x0F,
        }
    }
}

/// Path configuration info from NFSv3 PATHCONF.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PathConfResult {
    /// Maximum hard links per entry
    pub linkmax: u32,
    /// Maximum file name length
    pub name_max: u32,
    /// Whether long names are rejected (not truncated)
    pub no_trunc: bool,
    /// Whether chown is restricted to root
    pub chown_restricted: bool,
    /// Whether file names are case insensitive
    pub case_insensitive: bool,
    /// Whether file name case is preserved
    pub case_preserving: bool,
}

impl PathConfResult {
    /// Creates default path configuration for POSIX-like filesystems.
    pub fn defaults() -> Self {
        Self {
            linkmax: 255,
            name_max: 255,
            no_trunc: true,
            chown_restricted: true,
            case_insensitive: false,
            case_preserving: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::bytes::Bytes;

    #[test]
    fn test_filehandle_new_valid() {
        let fh = FileHandle3::new(vec![1, 2, 3, 4]).unwrap();
        assert_eq!(fh.data, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_filehandle_new_empty_error() {
        let result = FileHandle3::new(vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_filehandle_new_too_long_error() {
        let result = FileHandle3::new(vec![0; 65]);
        assert!(result.is_err());
    }

    #[test]
    fn test_filehandle_from_inode() {
        let fh = FileHandle3::from_inode(12345);
        assert_eq!(fh.as_inode(), Some(12345));
    }

    #[test]
    fn test_filehandle_as_inode_invalid() {
        let fh = FileHandle3::new(vec![1, 2, 3]).unwrap();
        assert_eq!(fh.as_inode(), None);
    }

    #[test]
    fn test_filehandle_xdr_roundtrip() {
        let fh = FileHandle3::new(vec![1, 2, 3, 4, 5]).unwrap();
        let mut enc = XdrEncoder::new();
        fh.encode_xdr(&mut enc);
        let buf = enc.finish();

        let mut dec = XdrDecoder::new(buf);
        let decoded = FileHandle3::decode_xdr(&mut dec).unwrap();
        assert_eq!(fh, decoded);
    }

    #[test]
    fn test_nfstime3_now() {
        let now = Nfstime3::now();
        assert!(now.seconds > 1700000000);
    }

    #[test]
    fn test_nfstime3_zero() {
        let zero = Nfstime3::zero();
        assert_eq!(zero.seconds, 0);
        assert_eq!(zero.nseconds, 0);
    }

    #[test]
    fn test_nfstime3_xdr_roundtrip() {
        let time = Nfstime3 {
            seconds: 12345,
            nseconds: 67890,
        };
        let mut enc = XdrEncoder::new();
        time.encode_xdr(&mut enc);
        let buf = enc.finish();

        let mut dec = XdrDecoder::new(buf);
        let decoded = Nfstime3::decode_xdr(&mut dec).unwrap();
        assert_eq!(time, decoded);
    }

    #[test]
    fn test_ftype3_from_u32() {
        assert_eq!(Ftype3::from_u32(1).unwrap(), Ftype3::Reg);
        assert_eq!(Ftype3::from_u32(2).unwrap(), Ftype3::Dir);
        assert_eq!(Ftype3::from_u32(5).unwrap(), Ftype3::Lnk);
    }

    #[test]
    fn test_ftype3_from_u32_invalid() {
        let result = Ftype3::from_u32(99);
        assert!(result.is_err());
    }

    #[test]
    fn test_ftype3_xdr_roundtrip() {
        let ftype = Ftype3::Dir;
        let mut enc = XdrEncoder::new();
        ftype.encode_xdr(&mut enc);
        let buf = enc.finish();

        let mut dec = XdrDecoder::new(buf);
        let decoded = Ftype3::decode_xdr(&mut dec).unwrap();
        assert_eq!(ftype, decoded);
    }

    #[test]
    fn test_fattr3_default_dir() {
        let attr = Fattr3::default_dir(123, 1);
        assert_eq!(attr.ftype, Ftype3::Dir);
        assert_eq!(attr.fileid, 123);
        assert_eq!(attr.fsid, 1);
    }

    #[test]
    fn test_fattr3_default_file() {
        let attr = Fattr3::default_file(456, 1000, 1);
        assert_eq!(attr.ftype, Ftype3::Reg);
        assert_eq!(attr.fileid, 456);
        assert_eq!(attr.size, 1000);
    }

    #[test]
    fn test_fattr3_xdr_roundtrip() {
        let attr = Fattr3::default_file(123, 5000, 1);
        let mut enc = XdrEncoder::new();
        attr.encode_xdr(&mut enc);
        let buf = enc.finish();

        let mut dec = XdrDecoder::new(buf);
        let decoded = Fattr3::decode_xdr(&mut dec).unwrap();
        assert_eq!(attr.fileid, decoded.fileid);
        assert_eq!(attr.size, decoded.size);
    }

    #[test]
    fn test_fsinfo_defaults() {
        let info = FsInfoResult::defaults();
        assert_eq!(info.rtmax, 1048576);
        assert_eq!(info.properties, 0x0F);
    }

    #[test]
    fn test_pathconf_defaults() {
        let conf = PathConfResult::defaults();
        assert_eq!(conf.linkmax, 255);
        assert_eq!(conf.name_max, 255);
        assert!(conf.no_trunc);
    }
}
