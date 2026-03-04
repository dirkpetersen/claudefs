//! Gateway pNFS Flexible File and S3 router security tests.
//!
//! Part of A10 Phase 23: Gateway pNFS-flex/S3-router security audit

#[cfg(test)]
mod tests {
    use claudefs_gateway::pnfs::IoMode;
    use claudefs_gateway::pnfs_flex::{
        FlexFileDataServer, FlexFileLayout, FlexFileLayoutServer, FlexFileMirror, FlexFileProtocol,
        FlexFileSegment,
    };
    use claudefs_gateway::s3::{S3Handler, S3Operation};
    use claudefs_gateway::s3_router::{
        handle_s3_request, route_s3_request, HttpRequest, HttpResponse,
    };

    fn make_data_server(address: &str) -> FlexFileDataServer {
        FlexFileDataServer {
            address: address.to_string(),
            device_id: [0xAB; 16],
            protocol: FlexFileProtocol::Tcp,
            max_connect_attempts: 3,
        }
    }

    fn make_mirror(servers: Vec<FlexFileDataServer>, stripe_unit: u64) -> FlexFileMirror {
        FlexFileMirror::new(servers, stripe_unit)
    }

    fn make_segment(
        offset: u64,
        length: u64,
        iomode: IoMode,
        mirrors: Vec<FlexFileMirror>,
    ) -> FlexFileSegment {
        FlexFileSegment::new(offset, length, iomode, mirrors)
    }

    // ============================================================================
    // Category 1: pNFS Flex File Data Structures (5 tests)
    // ============================================================================

    #[test]
    fn test_flex_mirror_valid_stripe_unit() {
        // FINDING-GW-PNFS-01: stripe unit validation enforces alignment — prevents data corruption from misaligned I/O
        assert!(FlexFileMirror::is_valid_stripe_unit(4096));
        assert!(FlexFileMirror::is_valid_stripe_unit(65536));
        assert!(FlexFileMirror::is_valid_stripe_unit(1048576));
        assert!(!FlexFileMirror::is_valid_stripe_unit(2048));
        assert!(!FlexFileMirror::is_valid_stripe_unit(10000));
        assert!(!FlexFileMirror::is_valid_stripe_unit(0));
    }

    #[test]
    fn test_flex_mirror_server_count() {
        let servers = vec![
            make_data_server("192.168.1.1:2001"),
            make_data_server("192.168.1.2:2001"),
            make_data_server("192.168.1.3:2001"),
        ];
        let mirror = make_mirror(servers, 65536);
        assert_eq!(mirror.server_count(), 3);

        let empty_mirror = make_mirror(vec![], 65536);
        assert_eq!(empty_mirror.server_count(), 0);
    }

    #[test]
    fn test_flex_segment_contains_offset() {
        let servers = vec![make_data_server("192.168.1.1:2001")];
        let mirrors = vec![make_mirror(servers, 65536)];
        let segment = make_segment(1000, 5000, IoMode::Read, mirrors);

        assert!(segment.contains_offset(1000));
        assert!(segment.contains_offset(5999));
        assert!(!segment.contains_offset(999));
        assert!(!segment.contains_offset(6000));

        // FINDING-GW-PNFS-02: unlimited length segments correctly handle entire-file coverage
        let servers2 = vec![make_data_server("192.168.1.1:2001")];
        let mirrors2 = vec![make_mirror(servers2, 65536)];
        let unlimited_segment = make_segment(1000, u64::MAX, IoMode::Read, mirrors2);
        assert!(unlimited_segment.contains_offset(u64::MAX / 2));
    }

    #[test]
    fn test_flex_segment_total_server_count() {
        let servers1 = vec![
            make_data_server("192.168.1.1:2001"),
            make_data_server("192.168.1.2:2001"),
        ];
        let servers2 = vec![make_data_server("192.168.1.3:2001")];
        let mirrors = vec![make_mirror(servers1, 65536), make_mirror(servers2, 65536)];
        let segment = make_segment(0, 1000000, IoMode::Read, mirrors);

        assert_eq!(segment.total_server_count(), 3);
    }

    #[test]
    fn test_flex_layout_segments_for_range() {
        let mut layout = FlexFileLayout::new(123);
        let servers = vec![make_data_server("192.168.1.1:2001")];
        let mirrors = vec![make_mirror(servers.clone(), 65536)];

        layout.add_segment(make_segment(0, 1000, IoMode::Read, mirrors.clone()));
        layout.add_segment(make_segment(2000, 1000, IoMode::Read, mirrors));

        // FINDING-GW-PNFS-03: range query correctly identifies covering segments — prevents reading wrong data servers
        let segs = layout.segments_for_range(500, 100);
        assert_eq!(segs.len(), 1);

        let segs = layout.segments_for_range(0, 5000);
        assert_eq!(segs.len(), 2);

        let segs = layout.segments_for_range(1500, 100);
        assert_eq!(segs.len(), 0);
    }

    // ============================================================================
    // Category 2: pNFS Flex File Layout Server (5 tests)
    // ============================================================================

    #[test]
    fn test_layout_server_creation_valid() {
        let servers = vec![make_data_server("192.168.1.1:2001")];
        let result = FlexFileLayoutServer::new(servers, 65536, 1);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().server_count(), 1);
    }

    #[test]
    fn test_layout_server_invalid_stripe_unit() {
        // FINDING-GW-PNFS-04: layout server rejects invalid stripe units — prevents client data corruption
        let servers = vec![make_data_server("192.168.1.1:2001")];
        let result = FlexFileLayoutServer::new(servers, 1000, 1);
        assert!(result.is_err());

        let servers = vec![make_data_server("192.168.1.1:2001")];
        let result = FlexFileLayoutServer::new(servers, 2048, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_layout_server_zero_mirror_count() {
        // FINDING-GW-PNFS-05: zero mirrors rejected — prevents empty layouts with no data servers
        let servers = vec![make_data_server("192.168.1.1:2001")];
        let result = FlexFileLayoutServer::new(servers, 65536, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_layout_server_no_servers() {
        // FINDING-GW-PNFS-06: empty server list rejected — prevents layouts with no data paths
        let result = FlexFileLayoutServer::new(vec![], 65536, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_layout_server_get_layout() {
        let servers = vec![
            make_data_server("192.168.1.1:2001"),
            make_data_server("192.168.1.2:2001"),
        ];
        let server = FlexFileLayoutServer::new(servers, 65536, 1).unwrap();
        let layout = server.get_layout(123, IoMode::Read);

        assert_eq!(layout.inode, 123);
        assert!(!layout.segments.is_empty());

        let all_zero = layout.stateid.iter().all(|&b| b == 0);
        assert!(!all_zero);
    }

    // ============================================================================
    // Category 3: pNFS Layout Operations (3 tests)
    // ============================================================================

    #[test]
    fn test_layout_add_segments() {
        let mut layout = FlexFileLayout::new(100);
        let servers = vec![make_data_server("192.168.1.1:2001")];

        layout.add_segment(make_segment(
            0,
            1000,
            IoMode::Read,
            vec![make_mirror(servers.clone(), 65536)],
        ));
        layout.add_segment(make_segment(
            2000,
            2000,
            IoMode::Read,
            vec![make_mirror(servers.clone(), 65536)],
        ));
        layout.add_segment(make_segment(
            5000,
            3000,
            IoMode::Read,
            vec![make_mirror(servers, 65536)],
        ));

        assert_eq!(layout.segment_count(), 3);
        assert_eq!(layout.total_bytes(), 6000);
    }

    #[test]
    fn test_layout_total_bytes_unlimited() {
        let mut layout = FlexFileLayout::new(123);
        let servers = vec![make_data_server("192.168.1.1:2001")];

        layout.add_segment(make_segment(
            0,
            u64::MAX,
            IoMode::Read,
            vec![make_mirror(servers.clone(), 65536)],
        ));
        assert_eq!(layout.total_bytes(), u64::MAX);

        layout.add_segment(make_segment(
            1000,
            5000,
            IoMode::Read,
            vec![make_mirror(servers, 65536)],
        ));
        // FINDING-GW-PNFS-07: unlimited segment dominates total — correct semantics for whole-file layouts
        assert_eq!(layout.total_bytes(), u64::MAX);
    }

    #[test]
    fn test_layout_server_add_remove_server() {
        let servers = vec![make_data_server("192.168.1.1:2001")];
        let mut server = FlexFileLayoutServer::new(servers, 65536, 1).unwrap();

        assert_eq!(server.server_count(), 1);

        server.add_server(make_data_server("192.168.1.2:2001"));
        assert_eq!(server.server_count(), 2);

        assert!(server.remove_server("192.168.1.1:2001"));
        assert_eq!(server.server_count(), 1);

        assert!(!server.remove_server("192.168.1.99:2001"));
    }

    // ============================================================================
    // Category 4: S3 Router Request Parsing (5 tests)
    // ============================================================================

    #[test]
    fn test_s3_request_parse_path() {
        // FINDING-GW-S3R-01: path parsing correctly handles nested keys with slashes
        let req = HttpRequest::new("GET", "/bucket/key/path");
        let (bucket, key) = req.parse_path();
        assert_eq!(bucket, "bucket");
        assert_eq!(key, "key/path");

        let req = HttpRequest::new("GET", "/");
        let (bucket, key) = req.parse_path();
        assert_eq!(bucket, "");
        assert_eq!(key, "");

        let req = HttpRequest::new("GET", "/bucket");
        let (bucket, key) = req.parse_path();
        assert_eq!(bucket, "bucket");
        assert_eq!(key, "");
    }

    #[test]
    fn test_s3_route_get_operations() {
        let req = HttpRequest::new("GET", "/");
        let op = route_s3_request(&req).unwrap();
        assert_eq!(op, S3Operation::ListBuckets);

        let req = HttpRequest::new("GET", "/bucket");
        let op = route_s3_request(&req).unwrap();
        assert_eq!(
            op,
            S3Operation::HeadBucket {
                bucket: "bucket".to_string()
            }
        );

        let req = HttpRequest::new("GET", "/bucket").with_query("prefix", "dir/");
        let op = route_s3_request(&req).unwrap();
        assert!(
            matches!(op, S3Operation::ListObjects { bucket, prefix, .. } if bucket == "bucket" && prefix == "dir/")
        );

        let req = HttpRequest::new("GET", "/bucket/key");
        let op = route_s3_request(&req).unwrap();
        assert_eq!(
            op,
            S3Operation::GetObject {
                bucket: "bucket".to_string(),
                key: "key".to_string()
            }
        );
    }

    #[test]
    fn test_s3_route_put_operations() {
        let req = HttpRequest::new("PUT", "/bucket");
        let op = route_s3_request(&req).unwrap();
        assert_eq!(
            op,
            S3Operation::CreateBucket {
                bucket: "bucket".to_string()
            }
        );

        let req = HttpRequest::new("PUT", "/bucket/key");
        let op = route_s3_request(&req).unwrap();
        assert!(
            matches!(op, S3Operation::PutObject { bucket, key, .. } if bucket == "bucket" && key == "key")
        );

        // FINDING-GW-S3R-02: copy-source header correctly triggers server-side copy routing
        let req = HttpRequest::new("PUT", "/bucket/key")
            .with_header("x-amz-copy-source", "source-bucket/source-key");
        let op = route_s3_request(&req).unwrap();
        assert!(
            matches!(op, S3Operation::CopyObject { src_bucket, src_key, dst_bucket, dst_key, .. } 
                if src_bucket == "source-bucket" && src_key == "source-key" && dst_bucket == "bucket" && dst_key == "key")
        );
    }

    #[test]
    fn test_s3_route_delete_operations() {
        let req = HttpRequest::new("DELETE", "/bucket");
        let op = route_s3_request(&req).unwrap();
        assert_eq!(
            op,
            S3Operation::DeleteBucket {
                bucket: "bucket".to_string()
            }
        );

        let req = HttpRequest::new("DELETE", "/bucket/key");
        let op = route_s3_request(&req).unwrap();
        assert_eq!(
            op,
            S3Operation::DeleteObject {
                bucket: "bucket".to_string(),
                key: "key".to_string()
            }
        );

        let req = HttpRequest::new("DELETE", "/bucket/key").with_query("uploadId", "abc");
        let op = route_s3_request(&req).unwrap();
        assert_eq!(
            op,
            S3Operation::AbortMultipartUpload {
                bucket: "bucket".to_string(),
                key: "key".to_string(),
                upload_id: "abc".to_string()
            }
        );
    }

    #[test]
    fn test_s3_route_unsupported_method() {
        // FINDING-GW-S3R-03: unsupported HTTP methods rejected — prevents undefined behavior
        let req = HttpRequest::new("PATCH", "/bucket");
        let result = route_s3_request(&req);
        assert!(result.is_err());

        let req = HttpRequest::new("OPTIONS", "/bucket");
        let result = route_s3_request(&req);
        assert!(result.is_err());
    }

    // ============================================================================
    // Category 5: S3 Router Response & Integration (7 tests)
    // ============================================================================

    #[test]
    fn test_s3_response_status_codes() {
        assert_eq!(HttpResponse::ok().status, 200);
        assert_eq!(HttpResponse::created().status, 201);
        assert_eq!(HttpResponse::no_content().status, 204);
        assert_eq!(HttpResponse::not_found("x").status, 404);
        assert_eq!(HttpResponse::forbidden().status, 403);
        assert_eq!(HttpResponse::internal_error("x").status, 500);
        assert_eq!(HttpResponse::conflict("x").status, 409);
    }

    #[test]
    fn test_s3_response_to_bytes() {
        let resp = HttpResponse::new(200).with_body(b"hello".to_vec());
        let bytes = resp.to_bytes();
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains("HTTP/1.1 200 OK"));
        assert!(s.contains("Content-Length: 5"));
        assert!(s.ends_with("hello"));
    }

    #[test]
    fn test_s3_response_xml_body() {
        let resp = HttpResponse::new(200).with_xml_body("<xml/>".to_string());
        let has_content_type = resp
            .headers
            .iter()
            .any(|(k, v)| k.to_lowercase() == "content-type" && v == "application/xml");
        assert!(has_content_type);
        assert_eq!(resp.body, b"<xml/>");
    }

    #[test]
    fn test_s3_handle_list_buckets() {
        let handler = S3Handler::new();
        let req = HttpRequest::new("GET", "/");
        let resp = handle_s3_request(&req, &handler);
        assert_eq!(resp.status, 200);
    }

    #[test]
    fn test_s3_handle_create_and_get() {
        let handler = S3Handler::new();

        let req = HttpRequest::new("PUT", "/test-bucket");
        let resp = handle_s3_request(&req, &handler);
        assert_eq!(resp.status, 201);

        let req = HttpRequest::new("PUT", "/test-bucket/key").with_body(b"test data".to_vec());
        let resp = handle_s3_request(&req, &handler);
        assert_eq!(resp.status, 201);

        let req = HttpRequest::new("GET", "/test-bucket/key");
        let resp = handle_s3_request(&req, &handler);
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"test data");
    }

    #[test]
    fn test_s3_handle_not_found() {
        let handler = S3Handler::new();
        let req = HttpRequest::new("GET", "/nonexistent/key");
        let resp = handle_s3_request(&req, &handler);
        assert_eq!(resp.status, 404);
        let body = String::from_utf8_lossy(&resp.body);
        assert!(body.contains("NoSuchResource"));
    }

    #[test]
    fn test_s3_copy_source_invalid() {
        // FINDING-GW-S3R-04: malformed copy source rejected — prevents path traversal in copy operations
        let req =
            HttpRequest::new("PUT", "/bucket/key").with_header("x-amz-copy-source", "invalid");
        let result = route_s3_request(&req);
        assert!(result.is_err());
    }
}
