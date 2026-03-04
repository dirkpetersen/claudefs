//! Gateway NFS write/RPC/S3 XML security tests.
//!
//! Part of A10 Phase 14: Gateway NFS & RPC security audit

use claudefs_gateway::nfs_write::{PendingWrite, WriteStability, WriteTracker};
use claudefs_gateway::rpc::{
    OpaqueAuth, RpcReply, TcpRecordMark, ACCEPT_PROC_UNAVAIL, ACCEPT_SUCCESS, AUTH_GSS, AUTH_NONE,
    AUTH_SYS, MOUNT_PROGRAM, MOUNT_VERSION, NFS3_COMMIT, NFS3_NULL, NFS3_READ, NFS3_WRITE,
    NFS_PROGRAM, NFS_VERSION, REJECT_AUTH_ERROR, RPC_REPLY,
};
use claudefs_gateway::s3_xml::{
    complete_multipart_upload_xml, copy_object_xml, create_multipart_upload_xml, error_xml,
    XmlBuilder,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn fh_key(v: u64) -> Vec<u8> {
        v.to_le_bytes().to_vec()
    }

    #[test]
    fn test_write_tracker_record_and_pending() {
        let tracker = WriteTracker::new(12345);
        tracker.record_write(fh_key(1), 0, 100, WriteStability::Unstable);

        assert!(tracker.has_pending_writes(&fh_key(1)));
        assert_eq!(tracker.pending_count(&fh_key(1)), 1);
    }

    #[test]
    fn test_write_tracker_commit() {
        let tracker = WriteTracker::new(100);
        tracker.record_write(fh_key(1), 0, 100, WriteStability::Unstable);
        tracker.record_write(fh_key(1), 100, 200, WriteStability::DataSync);
        tracker.record_write(fh_key(1), 300, 50, WriteStability::FileSync);

        assert_eq!(tracker.total_pending(), 3);

        let verf = tracker.commit(&fh_key(1));
        assert_eq!(verf, 100);
        assert_eq!(tracker.pending_count(&fh_key(1)), 0);
    }

    #[test]
    fn test_write_tracker_stability_ordering() {
        assert!(WriteStability::Unstable < WriteStability::DataSync);
        assert!(WriteStability::DataSync < WriteStability::FileSync);
        // FINDING-GW-NFS-01: stability ordering allows choosing minimum durability guarantee
    }

    #[test]
    fn test_write_tracker_multiple_files() {
        let tracker = WriteTracker::new(100);
        tracker.record_write(fh_key(1), 0, 100, WriteStability::Unstable);
        tracker.record_write(fh_key(1), 100, 200, WriteStability::Unstable);
        tracker.record_write(fh_key(2), 0, 300, WriteStability::DataSync);

        let initial_total = tracker.total_pending();
        assert!(initial_total >= 3);

        tracker.commit(&fh_key(1));

        assert!(!tracker.has_pending_writes(&fh_key(1)));
        assert!(tracker.has_pending_writes(&fh_key(2)));
    }

    #[test]
    fn test_write_tracker_commit_all() {
        let tracker = WriteTracker::new(100);
        tracker.record_write(fh_key(1), 0, 100, WriteStability::Unstable);
        tracker.record_write(fh_key(2), 0, 200, WriteStability::DataSync);
        tracker.record_write(fh_key(3), 0, 300, WriteStability::FileSync);

        let verf = tracker.commit_all();
        assert_eq!(verf, 100);
        assert_eq!(tracker.total_pending(), 0);
        assert!(!tracker.has_pending_writes(&fh_key(1)));
        assert!(!tracker.has_pending_writes(&fh_key(2)));
        assert!(!tracker.has_pending_writes(&fh_key(3)));
    }

    #[test]
    fn test_rpc_opaque_auth_none() {
        let auth = OpaqueAuth::none();
        assert_eq!(auth.flavor, AUTH_NONE);
        assert!(auth.body.is_empty());
    }

    #[test]
    fn test_rpc_reply_encode_success() {
        let reply = RpcReply::encode_success(42, b"ok");
        let xid = u32::from_be_bytes([reply[0], reply[1], reply[2], reply[3]]);
        let msg_type = u32::from_be_bytes([reply[4], reply[5], reply[6], reply[7]]);
        let accept = u32::from_be_bytes([reply[8], reply[9], reply[10], reply[11]]);

        assert_eq!(xid, 42);
        assert_eq!(msg_type, RPC_REPLY);
        assert_eq!(accept, ACCEPT_SUCCESS);
    }

    #[test]
    fn test_rpc_reply_encode_proc_unavail() {
        let reply = RpcReply::encode_proc_unavail(1);
        let xid = u32::from_be_bytes([reply[0], reply[1], reply[2], reply[3]]);
        let msg_type = u32::from_be_bytes([reply[4], reply[5], reply[6], reply[7]]);
        let accept = u32::from_be_bytes([reply[8], reply[9], reply[10], reply[11]]);

        assert_eq!(xid, 1);
        assert_eq!(msg_type, RPC_REPLY);
        assert_eq!(accept, ACCEPT_PROC_UNAVAIL);
    }

    #[test]
    fn test_rpc_reply_encode_auth_error() {
        let reply = RpcReply::encode_auth_error(5, 1);
        let xid = u32::from_be_bytes([reply[0], reply[1], reply[2], reply[3]]);
        let msg_type = u32::from_be_bytes([reply[4], reply[5], reply[6], reply[7]]);
        let reject_stat = u32::from_be_bytes([reply[8], reply[9], reply[10], reply[11]]);

        assert_eq!(xid, 5);
        assert_eq!(msg_type, RPC_REPLY);
        assert_eq!(reject_stat, REJECT_AUTH_ERROR);
    }

    #[test]
    fn test_rpc_constants_valid() {
        assert_eq!(NFS_PROGRAM, 100003);
        assert_eq!(MOUNT_PROGRAM, 100005);
        assert_eq!(NFS_VERSION, 3);
        assert_eq!(NFS3_NULL, 0);
        assert_eq!(NFS3_WRITE, 7);
        assert_eq!(NFS3_COMMIT, 21);
        // FINDING-GW-NFS-05: constants match RFC 1813
    }

    #[test]
    fn test_tcp_record_mark_encode() {
        let data = b"hello";
        let encoded = TcpRecordMark::encode(data);

        assert!(encoded.len() >= 4);
        let header = [encoded[0], encoded[1], encoded[2], encoded[3]];
        let (is_last, length) = TcpRecordMark::decode(header);

        assert!(is_last);
        assert_eq!(length, 5);
    }

    #[test]
    fn test_tcp_record_mark_decode() {
        let data = b"test";
        let encoded = TcpRecordMark::encode(data);
        let header: [u8; 4] = encoded[..4].try_into().unwrap();
        let (is_last, length) = TcpRecordMark::decode(header);

        assert!(is_last);
        assert_eq!(length, 4);
    }

    #[test]
    fn test_tcp_record_mark_roundtrip() {
        let data = b"roundtrip_data";
        let encoded = TcpRecordMark::encode(data);
        let header: [u8; 4] = encoded[..4].try_into().unwrap();
        let (_, decoded_len) = TcpRecordMark::decode(header);

        assert_eq!(decoded_len as usize, data.len());
        // FINDING-GW-NFS-02: record mark framing prevents message confusion
    }

    #[test]
    fn test_tcp_record_mark_empty() {
        let data = b"";
        let encoded = TcpRecordMark::encode(data);
        let header: [u8; 4] = encoded[..4].try_into().unwrap();
        let (is_last, length) = TcpRecordMark::decode(header);

        assert!(is_last);
        assert_eq!(length, 0);
    }

    #[test]
    fn test_tcp_record_mark_max_fragment() {
        let header: u32 = 0x807FFFFF;
        let bytes = header.to_be_bytes();
        let (is_last, length) = TcpRecordMark::decode(bytes);

        assert!(is_last);
        assert_eq!(length, 0x7FFFFF);
    }

    #[test]
    fn test_xml_builder_basic() {
        let mut xb = XmlBuilder::new();
        xb.header().open("Root").elem("Name", "test").close("Root");
        let result = xb.finish();

        assert!(result.starts_with("<?xml version=\"1.0\""));
        assert!(result.contains("<Root>"));
        assert!(result.contains("<Name>test</Name>"));
        assert!(result.contains("</Root>"));
    }

    #[test]
    fn test_xml_builder_escaping() {
        let mut xb = XmlBuilder::new();
        xb.elem("Value", "<>&\"'");
        let result = xb.finish();

        assert!(result.contains("&lt;"));
        assert!(result.contains("&gt;"));
        assert!(result.contains("&amp;"));
        assert!(result.contains("&quot;"));
        assert!(result.contains("&apos;"));
        // FINDING-GW-NFS-04: XML escaping prevents injection
    }

    #[test]
    fn test_xml_error_response() {
        let xml = error_xml(
            "NoSuchBucket",
            "The bucket does not exist",
            "/mybucket",
            "req-123",
        );

        assert!(xml.contains("<Code>NoSuchBucket</Code>"));
        assert!(xml.contains("<Message>The bucket does not exist</Message>"));
        assert!(xml.contains("<Resource>/mybucket</Resource>"));
        assert!(xml.contains("<RequestId>req-123</RequestId>"));
    }

    #[test]
    fn test_xml_multipart_upload() {
        let xml = create_multipart_upload_xml("mybucket", "mykey", "upload-123");

        assert!(xml.contains("<Bucket>mybucket</Bucket>"));
        assert!(xml.contains("<Key>mykey</Key>"));
        assert!(xml.contains("<UploadId>upload-123</UploadId>"));
        assert!(xml.contains("<?xml version"));
    }

    #[test]
    fn test_xml_copy_object() {
        let xml = copy_object_xml("etag-abc", "2026-01-01T00:00:00Z");

        assert!(xml.contains("ETag"));
        assert!(xml.contains("etag-abc"));
        assert!(xml.contains("LastModified"));
        assert!(xml.contains("2026-01-01T00:00:00Z"));
    }

    #[test]
    fn test_write_tracker_verf_consistency() {
        let tracker = WriteTracker::new(999);
        assert_eq!(tracker.write_verf(), 999);

        tracker.record_write(fh_key(1), 0, 100, WriteStability::Unstable);
        let verf_after = tracker.commit(&fh_key(1));
        assert_eq!(verf_after, 999);

        assert_eq!(tracker.write_verf(), 999);
        // FINDING-GW-NFS-03: verifier stability for NFS client crash recovery
    }

    #[test]
    fn test_write_tracker_remove_file() {
        let tracker = WriteTracker::new(100);
        tracker.record_write(fh_key(1), 0, 100, WriteStability::Unstable);
        tracker.record_write(fh_key(2), 0, 200, WriteStability::DataSync);

        let initial_total = tracker.total_pending();

        tracker.remove_file(&fh_key(1));

        assert!(!tracker.has_pending_writes(&fh_key(1)));
        assert!(tracker.has_pending_writes(&fh_key(2)));
        assert!(tracker.total_pending() < initial_total);
    }

    #[test]
    fn test_write_tracker_pending_writes_list() {
        let tracker = WriteTracker::new(100);
        tracker.record_write(fh_key(1), 0, 100, WriteStability::Unstable);
        tracker.record_write(fh_key(1), 100, 200, WriteStability::DataSync);
        tracker.record_write(fh_key(1), 200, 50, WriteStability::FileSync);

        let writes = tracker.pending_writes(&fh_key(1));
        assert_eq!(writes.len(), 3);
        assert_eq!(writes[0].offset, 0);
        assert_eq!(writes[1].offset, 100);
        assert_eq!(writes[2].offset, 200);
    }

    #[test]
    fn test_xml_builder_elem_types() {
        let mut xb = XmlBuilder::new();
        xb.open("Stats")
            .elem_u64("Size", 12345)
            .elem_u32("Count", 42)
            .elem_bool("Enabled", true)
            .elem_opt("Optional", None)
            .close("Stats");
        let result = xb.finish();

        assert!(result.contains("<Size>12345</Size>"));
        assert!(result.contains("<Count>42</Count>"));
        assert!(result.contains("<Enabled>true</Enabled>"));
        assert!(!result.contains("Optional"));
    }

    #[test]
    fn test_xml_builder_default() {
        let xb = XmlBuilder::default();
        assert_eq!(xb.finish(), "");

        let mut xb2 = XmlBuilder::new();
        xb2.open("Test").close("Test");
        let result = xb2.finish();
        assert!(!result.is_empty());
    }
}
