#![allow(missing_docs)]

//! Manual S3 XML response serialization

use crate::s3::{ListBucketsResult, ListObjectsResult};

pub struct XmlBuilder {
    buf: String,
}

impl XmlBuilder {
    pub fn new() -> Self {
        Self { buf: String::new() }
    }

    pub fn header(&mut self) -> &mut Self {
        self.buf
            .push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
        self
    }

    pub fn open(&mut self, tag: &str) -> &mut Self {
        self.buf.push('<');
        self.buf.push_str(tag);
        self.buf.push('>');
        self
    }

    pub fn close(&mut self, tag: &str) -> &mut Self {
        self.buf.push_str("</");
        self.buf.push_str(tag);
        self.buf.push('>');
        self
    }

    pub fn elem(&mut self, tag: &str, value: &str) -> &mut Self {
        self.buf.push('<');
        self.buf.push_str(tag);
        self.buf.push('>');
        self.buf.push_str(&self.escape(value));
        self.buf.push_str("</");
        self.buf.push_str(tag);
        self.buf.push('>');
        self
    }

    fn escape(&self, s: &str) -> String {
        let mut result = String::with_capacity(s.len());
        for c in s.chars() {
            match c {
                '&' => result.push_str("&amp;"),
                '<' => result.push_str("&lt;"),
                '>' => result.push_str("&gt;"),
                '"' => result.push_str("&quot;"),
                '\'' => result.push_str("&apos;"),
                _ => result.push(c),
            }
        }
        result
    }

    pub fn elem_u64(&mut self, tag: &str, value: u64) -> &mut Self {
        self.elem(tag, &value.to_string())
    }

    pub fn elem_u32(&mut self, tag: &str, value: u32) -> &mut Self {
        self.elem(tag, &value.to_string())
    }

    pub fn elem_bool(&mut self, tag: &str, value: bool) -> &mut Self {
        self.elem(tag, if value { "true" } else { "false" })
    }

    pub fn elem_opt(&mut self, tag: &str, value: Option<&str>) -> &mut Self {
        if let Some(v) = value {
            self.elem(tag, v);
        }
        self
    }

    pub fn finish(self) -> String {
        self.buf
    }
}

impl Default for XmlBuilder {
    fn default() -> Self {
        Self::new()
    }
}

pub fn list_buckets_xml(result: &ListBucketsResult) -> String {
    let mut xb = XmlBuilder::new();
    xb.header();
    xb.open("ListAllMyBucketsResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"");
    xb.open("Owner");
    xb.elem("ID", &result.owner_id);
    xb.elem("DisplayName", &result.owner_display_name);
    xb.close("Owner");
    xb.open("Buckets");
    for bucket in &result.buckets {
        xb.open("Bucket");
        xb.elem("Name", &bucket.name);
        xb.elem("CreationDate", &bucket.creation_date);
        xb.close("Bucket");
    }
    xb.close("Buckets");
    xb.close("ListAllMyBucketsResult");
    xb.finish()
}

pub fn list_objects_xml(result: &ListObjectsResult) -> String {
    let mut xb = XmlBuilder::new();
    xb.header();
    xb.open("ListBucketResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"");
    xb.elem("Name", &result.bucket);
    xb.elem("Prefix", &result.prefix);
    xb.elem_u32("MaxKeys", result.max_keys);
    xb.elem_bool("IsTruncated", result.is_truncated);
    for obj in &result.objects {
        xb.open("Contents");
        xb.elem("Key", &obj.key);
        xb.elem_u64("Size", obj.size);
        xb.elem("ETag", &obj.etag);
        xb.elem("LastModified", &obj.last_modified);
        xb.elem("StorageClass", "STANDARD");
        xb.close("Contents");
    }
    for prefix in &result.common_prefixes {
        xb.open("CommonPrefixes");
        xb.elem("Prefix", prefix);
        xb.close("CommonPrefixes");
    }
    if let Some(ref token) = result.next_continuation_token {
        xb.elem("NextContinuationToken", token);
    }
    xb.close("ListBucketResult");
    xb.finish()
}

pub fn error_xml(code: &str, message: &str, resource: &str, request_id: &str) -> String {
    let mut xb = XmlBuilder::new();
    xb.header();
    xb.open("Error");
    xb.elem("Code", code);
    xb.elem("Message", message);
    xb.elem("Resource", resource);
    xb.elem("RequestId", request_id);
    xb.close("Error");
    xb.finish()
}

pub fn create_multipart_upload_xml(bucket: &str, key: &str, upload_id: &str) -> String {
    let mut xb = XmlBuilder::new();
    xb.header();
    xb.open("CompleteMultipartUploadResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"");
    xb.elem("Bucket", bucket);
    xb.elem("Key", key);
    xb.elem("UploadId", upload_id);
    xb.close("CompleteMultipartUploadResult");
    xb.finish()
}

pub fn complete_multipart_upload_xml(
    location: &str,
    bucket: &str,
    key: &str,
    etag: &str,
) -> String {
    let mut xb = XmlBuilder::new();
    xb.header();
    xb.open("CompleteMultipartUploadResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"");
    xb.elem("Location", location);
    xb.elem("Bucket", bucket);
    xb.elem("Key", key);
    xb.elem("ETag", etag);
    xb.close("CompleteMultipartUploadResult");
    xb.finish()
}

pub fn copy_object_xml(etag: &str, last_modified: &str) -> String {
    let mut xb = XmlBuilder::new();
    xb.header();
    xb.open("CopyObjectResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"");
    xb.open("LastModified");
    xb.elem("LastModified", last_modified);
    xb.close("LastModified");
    xb.open("ETag");
    xb.elem("ETag", etag);
    xb.close("ETag");
    xb.close("CopyObjectResult");
    xb.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::s3::{Bucket, ObjectMeta};

    #[test]
    fn test_xml_builder_new() {
        let xb = XmlBuilder::new();
        assert_eq!(xb.finish(), "");
    }

    #[test]
    fn test_xml_builder_open_close() {
        let mut xb = XmlBuilder::new();
        xb.open("Tag").close("Tag");
        assert_eq!(xb.finish(), "<Tag></Tag>");
    }

    #[test]
    fn test_xml_builder_elem() {
        let mut xb = XmlBuilder::new();
        xb.elem("Key", "value");
        assert_eq!(xb.finish(), "<Key>value</Key>");
    }

    #[test]
    fn test_xml_builder_escape_amp() {
        let mut xb = XmlBuilder::new();
        xb.elem("Key", "a & b");
        assert_eq!(xb.finish(), "<Key>a &amp; b</Key>");
    }

    #[test]
    fn test_xml_builder_escape_lt() {
        let mut xb = XmlBuilder::new();
        xb.elem("Key", "a < b");
        assert_eq!(xb.finish(), "<Key>a &lt; b</Key>");
    }

    #[test]
    fn test_xml_builder_escape_gt() {
        let mut xb = XmlBuilder::new();
        xb.elem("Key", "a > b");
        assert_eq!(xb.finish(), "<Key>a &gt; b</Key>");
    }

    #[test]
    fn test_xml_builder_escape_quote() {
        let mut xb = XmlBuilder::new();
        xb.elem("Key", "a \"b\"");
        assert_eq!(xb.finish(), "<Key>a &quot;b&quot;</Key>");
    }

    #[test]
    fn test_xml_builder_escape_apos() {
        let mut xb = XmlBuilder::new();
        xb.elem("Key", "a 'b'");
        assert_eq!(xb.finish(), "<Key>a &apos;b&apos;</Key>");
    }

    #[test]
    fn test_xml_builder_elem_u64() {
        let mut xb = XmlBuilder::new();
        xb.elem_u64("Size", 12345);
        assert_eq!(xb.finish(), "<Size>12345</Size>");
    }

    #[test]
    fn test_xml_builder_elem_u32() {
        let mut xb = XmlBuilder::new();
        xb.elem_u32("MaxKeys", 1000);
        assert_eq!(xb.finish(), "<MaxKeys>1000</MaxKeys>");
    }

    #[test]
    fn test_xml_builder_elem_bool_true() {
        let mut xb = XmlBuilder::new();
        xb.elem_bool("Flag", true);
        assert_eq!(xb.finish(), "<Flag>true</Flag>");
    }

    #[test]
    fn test_xml_builder_elem_bool_false() {
        let mut xb = XmlBuilder::new();
        xb.elem_bool("Flag", false);
        assert_eq!(xb.finish(), "<Flag>false</Flag>");
    }

    #[test]
    fn test_xml_builder_elem_opt_some() {
        let mut xb = XmlBuilder::new();
        xb.elem_opt("Token", Some("abc"));
        assert_eq!(xb.finish(), "<Token>abc</Token>");
    }

    #[test]
    fn test_xml_builder_elem_opt_none() {
        let mut xb = XmlBuilder::new();
        xb.elem_opt("Token", None::<&str>);
        assert_eq!(xb.finish(), "");
    }

    #[test]
    fn test_list_buckets_xml() {
        let result = ListBucketsResult {
            buckets: vec![
                Bucket {
                    name: "bucket1".to_string(),
                    creation_date: "2024-01-01T00:00:00.000Z".to_string(),
                },
                Bucket {
                    name: "bucket2".to_string(),
                    creation_date: "2024-01-02T00:00:00.000Z".to_string(),
                },
            ],
            owner_id: "owner123".to_string(),
            owner_display_name: "Owner Name".to_string(),
        };
        let xml = list_buckets_xml(&result);
        assert!(xml.contains("<Name>bucket1</Name>"));
        assert!(xml.contains("<Name>bucket2</Name>"));
        assert!(xml.contains("<ID>owner123</ID>"));
    }

    #[test]
    fn test_list_objects_xml() {
        let result = ListObjectsResult {
            bucket: "mybucket".to_string(),
            prefix: "dir/".to_string(),
            delimiter: Some("/".to_string()),
            objects: vec![ObjectMeta {
                key: "dir/file.txt".to_string(),
                size: 100,
                etag: "\"abc123\"".to_string(),
                last_modified: "2024-01-01T00:00:00.000Z".to_string(),
                content_type: "text/plain".to_string(),
            }],
            common_prefixes: vec!["dir/sub/".to_string()],
            is_truncated: false,
            next_continuation_token: None,
            max_keys: 100,
        };
        let xml = list_objects_xml(&result);
        assert!(xml.contains("<Name>mybucket</Name>"));
        assert!(xml.contains("<Key>dir/file.txt</Key>"));
        assert!(xml.contains("<Size>100</Size>"));
        assert!(xml.contains("<Prefix>dir/sub/</Prefix>"));
    }

    #[test]
    fn test_list_objects_xml_truncated() {
        let result = ListObjectsResult {
            bucket: "mybucket".to_string(),
            prefix: "".to_string(),
            delimiter: None,
            objects: vec![],
            common_prefixes: vec![],
            is_truncated: true,
            next_continuation_token: Some("token123".to_string()),
            max_keys: 10,
        };
        let xml = list_objects_xml(&result);
        assert!(xml.contains("<IsTruncated>true</IsTruncated>"));
        assert!(xml.contains("<NextContinuationToken>token123</NextContinuationToken>"));
    }

    #[test]
    fn test_list_objects_xml_with_common_prefixes() {
        let result = ListObjectsResult {
            bucket: "mybucket".to_string(),
            prefix: "".to_string(),
            delimiter: Some("/".to_string()),
            objects: vec![],
            common_prefixes: vec!["dir1/".to_string(), "dir2/".to_string()],
            is_truncated: false,
            next_continuation_token: None,
            max_keys: 100,
        };
        let xml = list_objects_xml(&result);
        assert!(xml.contains("<Prefix>dir1/</Prefix>"));
        assert!(xml.contains("<Prefix>dir2/</Prefix>"));
    }

    #[test]
    fn test_error_xml() {
        let xml = error_xml(
            "NoSuchBucket",
            "The bucket does not exist",
            "mybucket",
            "req123",
        );
        assert!(xml.contains("<Code>NoSuchBucket</Code>"));
        assert!(xml.contains("<Message>The bucket does not exist</Message>"));
        assert!(xml.contains("<Resource>mybucket</Resource>"));
        assert!(xml.contains("<RequestId>req123</RequestId>"));
    }

    #[test]
    fn test_create_multipart_upload_xml() {
        let xml = create_multipart_upload_xml("bucket", "key", "upload123");
        assert!(xml.contains("<Bucket>bucket</Bucket>"));
        assert!(xml.contains("<Key>key</Key>"));
        assert!(xml.contains("<UploadId>upload123</UploadId>"));
    }

    #[test]
    fn test_complete_multipart_upload_xml() {
        let xml = complete_multipart_upload_xml(
            "http://localhost/bucket/key",
            "bucket",
            "key",
            "\"etag123\"",
        );
        assert!(xml.contains("<Location>http://localhost/bucket/key</Location>"));
        assert!(xml.contains("<Bucket>bucket</Bucket>"));
        assert!(xml.contains("<Key>key</Key>"));
        assert!(xml.contains("<ETag>&quot;etag123&quot;</ETag>"));
    }

    #[test]
    fn test_copy_object_xml() {
        let xml = copy_object_xml("\"etag123\"", "2024-01-01T00:00:00.000Z");
        assert!(xml.contains("<ETag>&quot;etag123&quot;</ETag>"));
        assert!(xml.contains("<LastModified>2024-01-01T00:00:00.000Z</LastModified>"));
    }
}
