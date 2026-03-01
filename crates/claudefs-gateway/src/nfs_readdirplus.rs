//! READDIRPLUS XDR encoding for NFSv3

use crate::protocol::{Entryplus3, Fattr3, FileHandle3, FsInfoResult, FsStatResult};
use crate::xdr::XdrEncoder;

pub fn encode_readdirplus_ok(
    dir_attributes: Option<&Fattr3>,
    cookieverf: u64,
    entries: &[Entryplus3],
    eof: bool,
) -> Vec<u8> {
    let mut enc = XdrEncoder::new();
    enc.encode_u32(0);

    match dir_attributes {
        Some(attr) => {
            enc.encode_bool(true);
            attr.encode_xdr(&mut enc);
        }
        None => {
            enc.encode_bool(false);
        }
    }

    enc.encode_u64(cookieverf);

    for entry in entries {
        enc.encode_u32(1);
        enc.encode_u64(entry.fileid);
        enc.encode_string(&entry.name);
        enc.encode_u64(entry.cookie);

        match &entry.name_attributes {
            Some(attr) => {
                enc.encode_bool(true);
                attr.encode_xdr(&mut enc);
            }
            None => {
                enc.encode_bool(false);
            }
        }

        match &entry.name_handle {
            Some(fh) => {
                enc.encode_u32(1);
                fh.encode_xdr(&mut enc);
            }
            None => {
                enc.encode_u32(0);
            }
        }
    }

    enc.encode_u32(0);
    enc.encode_bool(eof);

    enc.finish().to_vec()
}

pub fn encode_readdirplus_err(status: u32, dir_attributes: Option<&Fattr3>) -> Vec<u8> {
    let mut enc = XdrEncoder::new();
    enc.encode_u32(status);

    match dir_attributes {
        Some(attr) => {
            enc.encode_bool(true);
            attr.encode_xdr(&mut enc);
        }
        None => {
            enc.encode_bool(false);
        }
    }

    enc.finish().to_vec()
}

pub fn encode_getattr_ok(attr: &Fattr3) -> Vec<u8> {
    let mut enc = XdrEncoder::new();
    enc.encode_u32(0);
    attr.encode_xdr(&mut enc);
    enc.finish().to_vec()
}

pub fn encode_getattr_err(status: u32) -> Vec<u8> {
    let mut enc = XdrEncoder::new();
    enc.encode_u32(status);
    enc.finish().to_vec()
}

pub fn encode_lookup_ok(
    object_fh: &FileHandle3,
    obj_attr: Option<&Fattr3>,
    dir_attr: Option<&Fattr3>,
) -> Vec<u8> {
    let mut enc = XdrEncoder::new();
    enc.encode_u32(0);

    object_fh.encode_xdr(&mut enc);

    match obj_attr {
        Some(attr) => {
            enc.encode_bool(true);
            attr.encode_xdr(&mut enc);
        }
        None => {
            enc.encode_bool(false);
        }
    }

    match dir_attr {
        Some(attr) => {
            enc.encode_bool(true);
            attr.encode_xdr(&mut enc);
        }
        None => {
            enc.encode_bool(false);
        }
    }

    enc.finish().to_vec()
}

pub fn encode_read_ok(attr: Option<&Fattr3>, data: &[u8], eof: bool) -> Vec<u8> {
    let mut enc = XdrEncoder::new();
    enc.encode_u32(0);

    match attr {
        Some(a) => {
            enc.encode_bool(true);
            a.encode_xdr(&mut enc);
        }
        None => {
            enc.encode_bool(false);
        }
    }

    enc.encode_u32(data.len() as u32);
    enc.encode_bool(eof);
    enc.encode_opaque_variable(data);

    enc.finish().to_vec()
}

pub fn encode_write_ok(count: u32, stable: u32, verf: u64) -> Vec<u8> {
    let mut enc = XdrEncoder::new();
    enc.encode_u32(0);
    enc.encode_u32(count);
    enc.encode_u32(stable);
    enc.encode_u64(verf);
    enc.finish().to_vec()
}

pub fn encode_fsstat_ok(attr: Option<&Fattr3>, stat: &FsStatResult) -> Vec<u8> {
    let mut enc = XdrEncoder::new();
    enc.encode_u32(0);

    match attr {
        Some(a) => {
            enc.encode_bool(true);
            a.encode_xdr(&mut enc);
        }
        None => {
            enc.encode_bool(false);
        }
    }

    enc.encode_u64(stat.tbytes);
    enc.encode_u64(stat.fbytes);
    enc.encode_u64(stat.abytes);
    enc.encode_u64(stat.tfiles);
    enc.encode_u64(stat.ffiles);
    enc.encode_u32(stat.afiles);
    enc.encode_u32(stat.invarsec);

    enc.finish().to_vec()
}

pub fn encode_fsinfo_ok(attr: Option<&Fattr3>, info: &FsInfoResult) -> Vec<u8> {
    let mut enc = XdrEncoder::new();
    enc.encode_u32(0);

    match attr {
        Some(a) => {
            enc.encode_bool(true);
            a.encode_xdr(&mut enc);
        }
        None => {
            enc.encode_bool(false);
        }
    }

    enc.encode_u32(info.rtmax);
    enc.encode_u32(info.rtpref);
    enc.encode_u32(info.rtmult);
    enc.encode_u32(info.wtmax);
    enc.encode_u32(info.wtpref);
    enc.encode_u32(info.wtmult);
    enc.encode_u32(info.dtpref);
    enc.encode_u64(info.maxfilesize);
    info.time_delta.encode_xdr(&mut enc);
    enc.encode_u32(info.properties);

    enc.finish().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Ftype3, Nfstime3};

    fn test_fattr() -> Fattr3 {
        Fattr3 {
            ftype: Ftype3::Reg,
            mode: 0o644,
            nlink: 1,
            uid: 0,
            gid: 0,
            size: 1024,
            used: 4096,
            rdev: (0, 0),
            fsid: 1,
            fileid: 100,
            atime: Nfstime3::zero(),
            mtime: Nfstime3::zero(),
            ctime: Nfstime3::zero(),
        }
    }

    fn test_entry() -> Entryplus3 {
        Entryplus3 {
            fileid: 101,
            name: "test.txt".to_string(),
            cookie: 1,
            name_attributes: Some(Fattr3::default_file(101, 1024, 1)),
            name_handle: Some(FileHandle3::from_inode(101)),
        }
    }

    #[test]
    fn test_encode_readdirplus_ok_empty() {
        let data = encode_readdirplus_ok(Some(&test_fattr()), 0, &[], true);
        let mut dec = crate::xdr::XdrDecoder::new(prost::bytes::Bytes::from(data));
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert!(dec.decode_bool().unwrap());
        let _ = Fattr3::decode_xdr(&mut dec).unwrap();
        assert_eq!(dec.decode_u64().unwrap(), 0);
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert!(dec.decode_bool().unwrap());
    }

    #[test]
    fn test_encode_readdirplus_ok_single_entry() {
        let entry = test_entry();
        let data = encode_readdirplus_ok(Some(&test_fattr()), 12345, &[entry], false);

        let mut dec = crate::xdr::XdrDecoder::new(prost::bytes::Bytes::from(data));
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert!(dec.decode_bool().unwrap());
        let _ = Fattr3::decode_xdr(&mut dec).unwrap();
        assert_eq!(dec.decode_u64().unwrap(), 12345);

        assert_eq!(dec.decode_u32().unwrap(), 1);
        assert_eq!(dec.decode_u64().unwrap(), 101);
        assert_eq!(dec.decode_string().unwrap(), "test.txt");
        assert_eq!(dec.decode_u64().unwrap(), 1);

        assert!(dec.decode_bool().unwrap());
        let _ = Fattr3::decode_xdr(&mut dec).unwrap();

        assert_eq!(dec.decode_u32().unwrap(), 1);
        let _ = FileHandle3::decode_xdr(&mut dec).unwrap();

        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert!(!dec.decode_bool().unwrap());
    }

    #[test]
    fn test_encode_readdirplus_ok_multiple_entries() {
        let entries = vec![
            Entryplus3 {
                fileid: 101,
                name: "file1.txt".to_string(),
                cookie: 1,
                name_attributes: None,
                name_handle: None,
            },
            Entryplus3 {
                fileid: 102,
                name: "file2.txt".to_string(),
                cookie: 2,
                name_attributes: None,
                name_handle: None,
            },
        ];
        let data = encode_readdirplus_ok(None, 0, &entries, true);

        let mut dec = crate::xdr::XdrDecoder::new(prost::bytes::Bytes::from(data));
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert!(!dec.decode_bool().unwrap());
        assert_eq!(dec.decode_u64().unwrap(), 0);

        assert_eq!(dec.decode_u32().unwrap(), 1);
        assert_eq!(dec.decode_u64().unwrap(), 101);
        assert_eq!(dec.decode_string().unwrap(), "file1.txt");
        assert_eq!(dec.decode_u64().unwrap(), 1);

        assert!(!dec.decode_bool().unwrap());
        assert_eq!(dec.decode_u32().unwrap(), 0);

        assert_eq!(dec.decode_u32().unwrap(), 1);
        assert_eq!(dec.decode_u64().unwrap(), 102);
        assert_eq!(dec.decode_string().unwrap(), "file2.txt");
        assert_eq!(dec.decode_u64().unwrap(), 2);

        assert!(!dec.decode_bool().unwrap());
        assert_eq!(dec.decode_u32().unwrap(), 0);

        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert!(dec.decode_bool().unwrap());
    }

    #[test]
    fn test_encode_readdirplus_err() {
        let data = encode_readdirplus_err(2, Some(&test_fattr()));
        let mut dec = crate::xdr::XdrDecoder::new(prost::bytes::Bytes::from(data));
        assert_eq!(dec.decode_u32().unwrap(), 2);
        assert!(dec.decode_bool().unwrap());
        let _ = Fattr3::decode_xdr(&mut dec).unwrap();
    }

    #[test]
    fn test_encode_getattr_ok() {
        let attr = test_fattr();
        let data = encode_getattr_ok(&attr);

        let mut dec = crate::xdr::XdrDecoder::new(prost::bytes::Bytes::from(data));
        assert_eq!(dec.decode_u32().unwrap(), 0);
        let decoded = Fattr3::decode_xdr(&mut dec).unwrap();
        assert_eq!(decoded.fileid, 100);
    }

    #[test]
    fn test_encode_getattr_err() {
        let data = encode_getattr_err(2);
        let mut dec = crate::xdr::XdrDecoder::new(prost::bytes::Bytes::from(data));
        assert_eq!(dec.decode_u32().unwrap(), 2);
    }

    #[test]
    fn test_encode_lookup_ok() {
        let fh = FileHandle3::from_inode(200);
        let data = encode_lookup_ok(&fh, Some(&test_fattr()), Some(&test_fattr()));

        let mut dec = crate::xdr::XdrDecoder::new(prost::bytes::Bytes::from(data));
        assert_eq!(dec.decode_u32().unwrap(), 0);

        let decoded_fh = FileHandle3::decode_xdr(&mut dec).unwrap();
        assert_eq!(decoded_fh.as_inode(), Some(200));

        assert!(dec.decode_bool().unwrap());
        let _ = Fattr3::decode_xdr(&mut dec).unwrap();

        assert!(dec.decode_bool().unwrap());
        let _ = Fattr3::decode_xdr(&mut dec).unwrap();
    }

    #[test]
    fn test_encode_read_ok() {
        let data = b"test data content";
        let encoded = encode_read_ok(Some(&test_fattr()), data, true);

        let mut dec = crate::xdr::XdrDecoder::new(prost::bytes::Bytes::from(encoded));
        assert_eq!(dec.decode_u32().unwrap(), 0);

        assert!(dec.decode_bool().unwrap());
        let _ = Fattr3::decode_xdr(&mut dec).unwrap();

        assert_eq!(dec.decode_u32().unwrap(), 17);
        assert!(dec.decode_bool().unwrap());

        let opaque = dec.decode_opaque_variable().unwrap();
        assert_eq!(opaque, data);
    }

    #[test]
    fn test_encode_read_ok_with_eof_false() {
        let data = b"partial";
        let encoded = encode_read_ok(Some(&test_fattr()), data, false);

        let mut dec = crate::xdr::XdrDecoder::new(prost::bytes::Bytes::from(encoded));
        dec.decode_u32().unwrap();
        dec.decode_bool().unwrap();
        let _ = Fattr3::decode_xdr(&mut dec).unwrap();
        dec.decode_u32().unwrap();
        assert!(!dec.decode_bool().unwrap());
    }

    #[test]
    fn test_encode_write_ok() {
        let data = encode_write_ok(1000, 2, 12345);

        let mut dec = crate::xdr::XdrDecoder::new(prost::bytes::Bytes::from(data));
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert_eq!(dec.decode_u32().unwrap(), 1000);
        assert_eq!(dec.decode_u32().unwrap(), 2);
        assert_eq!(dec.decode_u64().unwrap(), 12345);
    }

    #[test]
    fn test_encode_fsstat_ok() {
        let stat = FsStatResult {
            tbytes: 1000000000,
            fbytes: 500000000,
            abytes: 500000000,
            tfiles: 10000,
            ffiles: 5000,
            afiles: 5000,
            invarsec: 30,
        };
        let data = encode_fsstat_ok(Some(&test_fattr()), &stat);

        let mut dec = crate::xdr::XdrDecoder::new(prost::bytes::Bytes::from(data));
        assert_eq!(dec.decode_u32().unwrap(), 0);

        assert!(dec.decode_bool().unwrap());
        let _ = Fattr3::decode_xdr(&mut dec).unwrap();

        assert_eq!(dec.decode_u64().unwrap(), 1000000000);
        assert_eq!(dec.decode_u64().unwrap(), 500000000);
        assert_eq!(dec.decode_u64().unwrap(), 500000000);
        assert_eq!(dec.decode_u64().unwrap(), 10000);
        assert_eq!(dec.decode_u64().unwrap(), 5000);
        assert_eq!(dec.decode_u32().unwrap(), 5000);
        assert_eq!(dec.decode_u32().unwrap(), 30);
    }
}
