//! S3 HTTP request routing

use std::collections::HashMap;

use crate::error::{GatewayError, Result};
use crate::s3::{S3Handler, S3Operation};
use crate::s3_xml;

#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl HttpRequest {
    pub fn new(method: &str, path: &str) -> Self {
        Self {
            method: method.to_string(),
            path: path.to_string(),
            query: HashMap::new(),
            headers: HashMap::new(),
            body: Vec::new(),
        }
    }

    pub fn with_query(mut self, key: &str, value: &str) -> Self {
        self.query.insert(key.to_string(), value.to_string());
        self
    }

    pub fn with_header(mut self, key: &str, value: &str) -> Self {
        self.headers.insert(key.to_lowercase(), value.to_string());
        self
    }

    pub fn with_body(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self
    }

    pub fn query_param(&self, key: &str) -> Option<&str> {
        self.query.get(key).map(|s| s.as_str())
    }

    pub fn header(&self, key: &str) -> Option<&str> {
        self.headers.get(&key.to_lowercase()).map(|s| s.as_str())
    }

    pub fn parse_path(&self) -> (String, String) {
        let path = self.path.trim_start_matches('/');
        if path.is_empty() {
            return (String::new(), String::new());
        }
        if let Some(pos) = path.find('/') {
            (path[..pos].to_string(), path[pos + 1..].to_string())
        } else {
            (path.to_string(), String::new())
        }
    }
}

pub fn route_s3_request(req: &HttpRequest) -> Result<S3Operation> {
    let (bucket, key) = req.parse_path();
    let method = req.method.as_str();

    match method {
        "GET" => {
            if bucket.is_empty() {
                return Ok(S3Operation::ListBuckets);
            }
            if req.query_param("list-type").is_some()
                || req.query_param("prefix").is_some()
                || req.query_param("delimiter").is_some()
            {
                return Ok(S3Operation::ListObjects {
                    bucket,
                    prefix: req.query_param("prefix").unwrap_or("").to_string(),
                    delimiter: req.query_param("delimiter").map(|s| s.to_string()),
                    max_keys: req
                        .query_param("max-keys")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(1000),
                });
            }
            if key.is_empty() {
                return Ok(S3Operation::HeadBucket { bucket });
            }
            Ok(S3Operation::GetObject { bucket, key })
        }
        "PUT" => {
            if key.is_empty() {
                return Ok(S3Operation::CreateBucket { bucket });
            }
            if req.header("x-amz-copy-source").is_some() {
                let src = req.header("x-amz-copy-source").unwrap();
                let src_parts: Vec<&str> = src.splitn(2, '/').collect();
                if src_parts.len() != 2 {
                    return Err(GatewayError::ProtocolError {
                        reason: "invalid copy source".to_string(),
                    });
                }
                let src_bucket = src_parts[0];
                let src_key = src_parts[1].trim_start_matches('/');
                return Ok(S3Operation::CopyObject {
                    src_bucket: src_bucket.to_string(),
                    src_key: src_key.to_string(),
                    dst_bucket: bucket,
                    dst_key: key,
                });
            }
            let content_type = req
                .header("content-type")
                .unwrap_or("application/octet-stream");
            Ok(S3Operation::PutObject {
                bucket,
                key,
                content_type: content_type.to_string(),
            })
        }
        "DELETE" => {
            if key.is_empty() {
                return Ok(S3Operation::DeleteBucket { bucket });
            }
            if req.query_param("uploadId").is_some() {
                return Ok(S3Operation::AbortMultipartUpload {
                    bucket,
                    key,
                    upload_id: req.query_param("uploadId").unwrap().to_string(),
                });
            }
            Ok(S3Operation::DeleteObject { bucket, key })
        }
        "HEAD" => {
            if key.is_empty() {
                return Ok(S3Operation::HeadBucket { bucket });
            }
            Ok(S3Operation::HeadObject { bucket, key })
        }
        "POST" => {
            if let Some(uploads) = req.query_param("uploads") {
                if uploads == "" || uploads == "true" {
                    return Ok(S3Operation::CreateMultipartUpload { bucket, key });
                }
            }
            if let Some(upload_id) = req.query_param("uploadId") {
                return Ok(S3Operation::CompleteMultipartUpload {
                    bucket,
                    key,
                    upload_id: upload_id.to_string(),
                });
            }
            if let Some(part_number) = req.query_param("partNumber") {
                let part_number: u32 =
                    part_number
                        .parse()
                        .map_err(|_| GatewayError::ProtocolError {
                            reason: "invalid part number".to_string(),
                        })?;
                let upload_id = req
                    .query_param("uploadId")
                    .ok_or(GatewayError::ProtocolError {
                        reason: "missing uploadId".to_string(),
                    })?;
                return Ok(S3Operation::UploadPart {
                    bucket,
                    key,
                    upload_id: upload_id.to_string(),
                    part_number,
                });
            }
            Err(GatewayError::ProtocolError {
                reason: "unknown POST operation".to_string(),
            })
        }
        _ => Err(GatewayError::ProtocolError {
            reason: format!("unsupported HTTP method: {}", method),
        }),
    }
}

#[derive(Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn new(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    pub fn with_header(mut self, key: &str, value: &str) -> Self {
        self.headers.push((key.to_string(), value.to_string()));
        self
    }

    pub fn with_body(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self
    }

    pub fn with_xml_body(mut self, xml: String) -> Self {
        self.body = xml.into_bytes();
        if !self
            .headers
            .iter()
            .any(|(k, _)| k.to_lowercase() == "content-type")
        {
            self.headers
                .push(("Content-Type".to_string(), "application/xml".to_string()));
        }
        self
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let status_line = match self.status {
            200 => "HTTP/1.1 200 OK",
            201 => "HTTP/1.1 201 Created",
            204 => "HTTP/1.1 204 No Content",
            400 => "HTTP/1.1 400 Bad Request",
            403 => "HTTP/1.1 403 Forbidden",
            404 => "HTTP/1.1 404 Not Found",
            409 => "HTTP/1.1 409 Conflict",
            500 => "HTTP/1.1 500 Internal Server Error",
            _ => "HTTP/1.1 500 Internal Server Error",
        };

        let mut result = format!("{}\r\n", status_line).into_bytes();

        for (key, value) in &self.headers {
            result.extend(format!("{}: {}\r\n", key, value).as_bytes());
        }

        if !self.body.is_empty() {
            result.extend(format!("Content-Length: {}\r\n", self.body.len()).as_bytes());
        }

        result.extend(b"\r\n");
        result.extend(&self.body);
        result
    }

    pub fn ok() -> Self {
        Self::new(200)
    }

    pub fn created() -> Self {
        Self::new(201)
    }

    pub fn no_content() -> Self {
        Self::new(204)
    }

    pub fn not_found(resource: &str) -> Self {
        let xml = s3_xml::error_xml(
            "NoSuchResource",
            "The resource does not exist",
            resource,
            "unknown",
        );
        Self::new(404).with_xml_body(xml)
    }

    pub fn forbidden() -> Self {
        let xml = s3_xml::error_xml("AccessDenied", "Access Denied", "", "unknown");
        Self::new(403).with_xml_body(xml)
    }

    pub fn internal_error(msg: &str) -> Self {
        let xml = s3_xml::error_xml("InternalError", msg, "", "unknown");
        Self::new(500).with_xml_body(xml)
    }

    pub fn conflict(msg: &str) -> Self {
        let xml = s3_xml::error_xml("Conflict", msg, "", "unknown");
        Self::new(409).with_xml_body(xml)
    }
}

pub fn handle_s3_request(req: &HttpRequest, handler: &S3Handler) -> HttpResponse {
    let operation = match route_s3_request(req) {
        Ok(op) => op,
        Err(e) => return HttpResponse::internal_error(&e.to_string()),
    };

    match operation {
        S3Operation::ListBuckets => match handler.list_buckets() {
            Ok(result) => {
                let xml = s3_xml::list_buckets_xml(&result);
                HttpResponse::ok().with_xml_body(xml)
            }
            Err(e) => HttpResponse::internal_error(&e.to_string()),
        },
        S3Operation::CreateBucket { bucket } => match handler.create_bucket(&bucket) {
            Ok(_) => HttpResponse::created()
                .with_header("Location", &format!("/{}", bucket))
                .with_body(Vec::new()),
            Err(e) => HttpResponse::conflict(&e.to_string()),
        },
        S3Operation::DeleteBucket { bucket } => match handler.delete_bucket(&bucket) {
            Ok(_) => HttpResponse::no_content(),
            Err(_) => HttpResponse::not_found(&bucket),
        },
        S3Operation::HeadBucket { bucket } => match handler.head_bucket(&bucket) {
            Ok(_) => HttpResponse::ok(),
            Err(_) => HttpResponse::not_found(&bucket),
        },
        S3Operation::ListObjects {
            bucket,
            prefix,
            delimiter,
            max_keys,
        } => match handler.list_objects(&bucket, &prefix, delimiter.as_deref(), max_keys) {
            Ok(result) => {
                let xml = s3_xml::list_objects_xml(&result);
                HttpResponse::ok().with_xml_body(xml)
            }
            Err(_e) => HttpResponse::not_found(&bucket),
        },
        S3Operation::GetObject { bucket, key } => match handler.get_object(&bucket, &key) {
            Ok((meta, data)) => HttpResponse::ok()
                .with_header("Content-Type", &meta.content_type)
                .with_header("Content-Length", &meta.size.to_string())
                .with_header("ETag", &meta.etag)
                .with_header("Last-Modified", &meta.last_modified)
                .with_body(data),
            Err(_) => HttpResponse::not_found(&format!("{}/{}", bucket, key)),
        },
        S3Operation::PutObject {
            bucket,
            key,
            content_type,
        } => match handler.put_object(&bucket, &key, &content_type, req.body.clone()) {
            Ok(meta) => HttpResponse::created()
                .with_header("ETag", &meta.etag)
                .with_header("Last-Modified", &meta.last_modified)
                .with_body(Vec::new()),
            Err(e) => HttpResponse::internal_error(&e.to_string()),
        },
        S3Operation::DeleteObject { bucket, key } => match handler.delete_object(&bucket, &key) {
            Ok(_) => HttpResponse::ok(),
            Err(_) => HttpResponse::not_found(&format!("{}/{}", bucket, key)),
        },
        S3Operation::HeadObject { bucket, key } => match handler.head_object(&bucket, &key) {
            Ok(meta) => HttpResponse::ok()
                .with_header("Content-Type", &meta.content_type)
                .with_header("Content-Length", &meta.size.to_string())
                .with_header("ETag", &meta.etag)
                .with_header("Last-Modified", &meta.last_modified),
            Err(_) => HttpResponse::not_found(&format!("{}/{}", bucket, key)),
        },
        S3Operation::CopyObject {
            src_bucket,
            src_key,
            dst_bucket,
            dst_key,
        } => match handler.copy_object(&src_bucket, &src_key, &dst_bucket, &dst_key) {
            Ok(meta) => {
                let xml = s3_xml::copy_object_xml(&meta.etag, &meta.last_modified);
                HttpResponse::ok().with_xml_body(xml)
            }
            Err(e) => HttpResponse::internal_error(&e.to_string()),
        },
        S3Operation::CreateMultipartUpload { bucket, key } => {
            let upload_id = format!(
                "upload-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            let xml = s3_xml::create_multipart_upload_xml(&bucket, &key, &upload_id);
            HttpResponse::ok().with_xml_body(xml)
        }
        S3Operation::UploadPart { .. } => HttpResponse::ok(),
        S3Operation::CompleteMultipartUpload { .. } => HttpResponse::ok(),
        S3Operation::AbortMultipartUpload { .. } => HttpResponse::no_content(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_path_empty() {
        let req = HttpRequest::new("GET", "/");
        let (bucket, key) = req.parse_path();
        assert_eq!(bucket, "");
        assert_eq!(key, "");
    }

    #[test]
    fn test_parse_path_bucket_only() {
        let req = HttpRequest::new("GET", "/bucket");
        let (bucket, key) = req.parse_path();
        assert_eq!(bucket, "bucket");
        assert_eq!(key, "");
    }

    #[test]
    fn test_parse_path_bucket_and_key() {
        let req = HttpRequest::new("GET", "/bucket/key/name");
        let (bucket, key) = req.parse_path();
        assert_eq!(bucket, "bucket");
        assert_eq!(key, "key/name");
    }

    #[test]
    fn test_route_list_buckets() {
        let req = HttpRequest::new("GET", "/");
        let op = route_s3_request(&req).unwrap();
        assert_eq!(op, S3Operation::ListBuckets);
    }

    #[test]
    fn test_route_create_bucket() {
        let req = HttpRequest::new("PUT", "/bucket");
        let op = route_s3_request(&req).unwrap();
        assert_eq!(
            op,
            S3Operation::CreateBucket {
                bucket: "bucket".to_string()
            }
        );
    }

    #[test]
    fn test_route_delete_bucket() {
        let req = HttpRequest::new("DELETE", "/bucket");
        let op = route_s3_request(&req).unwrap();
        assert_eq!(
            op,
            S3Operation::DeleteBucket {
                bucket: "bucket".to_string()
            }
        );
    }

    #[test]
    fn test_route_head_bucket() {
        let req = HttpRequest::new("HEAD", "/bucket");
        let op = route_s3_request(&req).unwrap();
        assert_eq!(
            op,
            S3Operation::HeadBucket {
                bucket: "bucket".to_string()
            }
        );
    }

    #[test]
    fn test_route_get_object() {
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
    fn test_route_delete_object() {
        let req = HttpRequest::new("DELETE", "/bucket/key");
        let op = route_s3_request(&req).unwrap();
        assert_eq!(
            op,
            S3Operation::DeleteObject {
                bucket: "bucket".to_string(),
                key: "key".to_string()
            }
        );
    }

    #[test]
    fn test_route_head_object() {
        let req = HttpRequest::new("HEAD", "/bucket/key");
        let op = route_s3_request(&req).unwrap();
        assert_eq!(
            op,
            S3Operation::HeadObject {
                bucket: "bucket".to_string(),
                key: "key".to_string()
            }
        );
    }

    #[test]
    fn test_route_list_objects() {
        let req = HttpRequest::new("GET", "/bucket").with_query("prefix", "dir/");
        let op = route_s3_request(&req).unwrap();
        assert!(
            matches!(op, S3Operation::ListObjects { bucket, prefix, .. } if bucket == "bucket" && prefix == "dir/")
        );
    }

    #[test]
    fn test_route_put_object() {
        let req = HttpRequest::new("PUT", "/bucket/key");
        let op = route_s3_request(&req).unwrap();
        assert!(
            matches!(op, S3Operation::PutObject { bucket, key, .. } if bucket == "bucket" && key == "key")
        );
    }

    #[test]
    fn test_route_create_multipart_upload() {
        let req = HttpRequest::new("POST", "/bucket/key").with_query("uploads", "");
        let op = route_s3_request(&req).unwrap();
        assert_eq!(
            op,
            S3Operation::CreateMultipartUpload {
                bucket: "bucket".to_string(),
                key: "key".to_string()
            }
        );
    }

    #[test]
    fn test_http_response_new() {
        let resp = HttpResponse::new(200);
        assert_eq!(resp.status, 200);
    }

    #[test]
    fn test_http_response_with_header() {
        let resp = HttpResponse::new(200).with_header("Content-Type", "text/plain");
        assert_eq!(resp.headers.len(), 1);
        assert_eq!(
            resp.headers[0],
            ("Content-Type".to_string(), "text/plain".to_string())
        );
    }

    #[test]
    fn test_http_response_with_body() {
        let resp = HttpResponse::new(200).with_body(b"hello".to_vec());
        assert_eq!(resp.body, b"hello");
    }

    #[test]
    fn test_http_response_to_bytes() {
        let resp = HttpResponse::new(200).with_body(b"hello".to_vec());
        let bytes = resp.to_bytes();
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains("HTTP/1.1 200 OK"));
        assert!(s.contains("Content-Length: 5"));
        assert!(s.contains("\r\n\r\nhello"));
    }

    #[test]
    fn test_http_response_created() {
        let resp = HttpResponse::created();
        assert_eq!(resp.status, 201);
    }

    #[test]
    fn test_http_response_no_content() {
        let resp = HttpResponse::no_content();
        assert_eq!(resp.status, 204);
    }

    #[test]
    fn test_http_response_not_found() {
        let resp = HttpResponse::not_found("bucket");
        assert_eq!(resp.status, 404);
        assert!(String::from_utf8_lossy(&resp.body).contains("NoSuchResource"));
    }

    #[test]
    fn test_http_response_forbidden() {
        let resp = HttpResponse::forbidden();
        assert_eq!(resp.status, 403);
    }

    #[test]
    fn test_http_response_internal_error() {
        let resp = HttpResponse::internal_error("test error");
        assert_eq!(resp.status, 500);
    }

    #[test]
    fn test_http_response_conflict() {
        let resp = HttpResponse::conflict("conflict message");
        assert_eq!(resp.status, 409);
    }

    #[test]
    fn test_handle_s3_list_buckets() {
        let handler = S3Handler::new();
        let req = HttpRequest::new("GET", "/");
        let resp = handle_s3_request(&req, &handler);
        assert_eq!(resp.status, 200);
    }

    #[test]
    fn test_handle_s3_create_bucket() {
        let handler = S3Handler::new();
        let req = HttpRequest::new("PUT", "/test-bucket");
        let resp = handle_s3_request(&req, &handler);
        assert_eq!(resp.status, 201);
    }

    #[test]
    fn test_handle_s3_put_object() {
        let handler = S3Handler::new();
        handler.create_bucket("bucket").unwrap();
        let req = HttpRequest::new("PUT", "/bucket/key").with_body(b"test data".to_vec());
        let resp = handle_s3_request(&req, &handler);
        assert_eq!(resp.status, 201);
    }

    #[test]
    fn test_handle_s3_get_object() {
        let handler = S3Handler::new();
        handler.create_bucket("bucket").unwrap();
        handler
            .put_object("bucket", "key", "text/plain", b"test data".to_vec())
            .unwrap();
        let req = HttpRequest::new("GET", "/bucket/key");
        let resp = handle_s3_request(&req, &handler);
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"test data");
    }

    #[test]
    fn test_handle_s3_list_objects() {
        let handler = S3Handler::new();
        handler.create_bucket("bucket").unwrap();
        handler
            .put_object("bucket", "key1", "text/plain", b"data".to_vec())
            .unwrap();
        let req = HttpRequest::new("GET", "/bucket");
        let resp = handle_s3_request(&req, &handler);
        assert_eq!(resp.status, 200);
    }

    #[test]
    fn test_handle_s3_delete_object() {
        let handler = S3Handler::new();
        handler.create_bucket("bucket").unwrap();
        handler
            .put_object("bucket", "key", "text/plain", b"data".to_vec())
            .unwrap();
        let req = HttpRequest::new("DELETE", "/bucket/key");
        let resp = handle_s3_request(&req, &handler);
        assert_eq!(resp.status, 200);
    }

    #[test]
    fn test_handle_s3_not_found() {
        let handler = S3Handler::new();
        let req = HttpRequest::new("GET", "/nonexistent/key");
        let resp = handle_s3_request(&req, &handler);
        assert_eq!(resp.status, 404);
    }
}
