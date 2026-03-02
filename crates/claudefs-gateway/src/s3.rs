//! S3-compatible API implementation

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

use crate::error::{GatewayError, Result};

/// S3 bucket metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bucket {
    /// Bucket name
    pub name: String,
    /// Bucket creation date (ISO 8601)
    pub creation_date: String,
}

/// S3 object metadata (without the object data).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectMeta {
    /// Object key
    pub key: String,
    /// Object size in bytes
    pub size: u64,
    /// ETag (content hash)
    pub etag: String,
    /// Last modified timestamp (ISO 8601)
    pub last_modified: String,
    /// Content type (e.g., "application/octet-stream")
    pub content_type: String,
}

/// Result of ListBuckets API call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListBucketsResult {
    /// List of buckets
    pub buckets: Vec<Bucket>,
    /// Owner ID
    pub owner_id: String,
    /// Owner display name
    pub owner_display_name: String,
}

/// Result of ListObjects/ListObjectsV2 API call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListObjectsResult {
    /// Bucket name
    pub bucket: String,
    /// Prefix filter
    pub prefix: String,
    /// Delimiter for hierarchical listing
    pub delimiter: Option<String>,
    /// List of objects
    pub objects: Vec<ObjectMeta>,
    /// Common prefixes (for delimiter-based listing)
    pub common_prefixes: Vec<String>,
    /// Whether there are more results
    pub is_truncated: bool,
    /// Token for pagination (ListObjectsV2)
    pub next_continuation_token: Option<String>,
    /// Maximum keys requested
    pub max_keys: u32,
}

/// S3 operation type for routing and auditing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum S3Operation {
    /// List all buckets (ListBuckets API)
    ListBuckets,
    /// Create a new bucket (CreateBucket API)
    CreateBucket {
        /// Bucket name
        bucket: String,
    },
    /// Delete a bucket (DeleteBucket API)
    DeleteBucket {
        /// Bucket name
        bucket: String,
    },
    /// Check bucket existence (HeadBucket API)
    HeadBucket {
        /// Bucket name
        bucket: String,
    },
    /// List objects in a bucket (ListObjects API)
    ListObjects {
        /// Bucket name
        bucket: String,
        /// Prefix filter
        prefix: String,
        /// Delimiter for hierarchical listing
        delimiter: Option<String>,
        /// Maximum keys to return
        max_keys: u32,
    },
    /// Get an object (GetObject API)
    GetObject {
        /// Bucket name
        bucket: String,
        /// Object key
        key: String,
    },
    /// Put an object (PutObject API)
    PutObject {
        /// Bucket name
        bucket: String,
        /// Object key
        key: String,
        /// Content type
        content_type: String,
    },
    /// Delete an object (DeleteObject API)
    DeleteObject {
        /// Bucket name
        bucket: String,
        /// Object key
        key: String,
    },
    /// Get object metadata (HeadObject API)
    HeadObject {
        /// Bucket name
        bucket: String,
        /// Object key
        key: String,
    },
    /// Copy an object (CopyObject API)
    CopyObject {
        /// Source bucket
        src_bucket: String,
        /// Source key
        src_key: String,
        /// Destination bucket
        dst_bucket: String,
        /// Destination key
        dst_key: String,
    },
    /// Initiate multipart upload (CreateMultipartUpload API)
    CreateMultipartUpload {
        /// Bucket name
        bucket: String,
        /// Object key
        key: String,
    },
    /// Upload a part (UploadPart API)
    UploadPart {
        /// Bucket name
        bucket: String,
        /// Object key
        key: String,
        /// Upload ID from CreateMultipartUpload
        upload_id: String,
        /// Part number (1-based)
        part_number: u32,
    },
    /// Complete multipart upload (CompleteMultipartUpload API)
    CompleteMultipartUpload {
        /// Bucket name
        bucket: String,
        /// Object key
        key: String,
        /// Upload ID from CreateMultipartUpload
        upload_id: String,
    },
    /// Abort multipart upload (AbortMultipartUpload API)
    AbortMultipartUpload {
        /// Bucket name
        bucket: String,
        /// Object key
        key: String,
        /// Upload ID from CreateMultipartUpload
        upload_id: String,
    },
}

/// Internal object storage (metadata + data).
#[derive(Debug)]
struct ObjectData {
    meta: ObjectMeta,
    data: Vec<u8>,
}

#[derive(Debug)]
struct BucketState {
    name: String,
    creation_date: String,
    objects: HashMap<String, ObjectData>,
}

impl BucketState {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            creation_date: chrono_timestamp(),
            objects: HashMap::new(),
        }
    }
}

fn chrono_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{:.3}Z", now.as_secs_f64())
}

fn generate_etag() -> String {
    format!("\"{:x}\"", rand_simple())
}

fn rand_simple() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    nanos.wrapping_mul(1103515245).wrapping_add(12345)
}

/// S3 API request handler - manages buckets and objects.
pub struct S3Handler {
    buckets: Arc<RwLock<HashMap<String, BucketState>>>,
}

impl S3Handler {
    /// Creates a new S3Handler with no buckets.
    pub fn new() -> Self {
        Self {
            buckets: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn validate_bucket_name(&self, name: &str) -> Result<()> {
        if name.len() < 3 || name.len() > 63 {
            return Err(GatewayError::S3InvalidBucketName {
                name: name.to_string(),
            });
        }
        if !name
            .chars()
            .next()
            .map(|c| c.is_ascii_alphanumeric())
            .unwrap_or(false)
        {
            return Err(GatewayError::S3InvalidBucketName {
                name: name.to_string(),
            });
        }
        if !name
            .chars()
            .last()
            .map(|c| c.is_ascii_alphanumeric())
            .unwrap_or(false)
        {
            return Err(GatewayError::S3InvalidBucketName {
                name: name.to_string(),
            });
        }
        for c in name.chars() {
            if !c.is_ascii_alphanumeric() && c != '-' {
                return Err(GatewayError::S3InvalidBucketName {
                    name: name.to_string(),
                });
            }
        }
        Ok(())
    }

    /// Lists all buckets owned by the user.
    pub fn list_buckets(&self) -> Result<ListBucketsResult> {
        let buckets = self.buckets.read().map_err(|_| GatewayError::Nfs3Io)?;
        let bucket_list: Vec<Bucket> = buckets
            .values()
            .map(|b| Bucket {
                name: b.name.clone(),
                creation_date: b.creation_date.clone(),
            })
            .collect();
        Ok(ListBucketsResult {
            buckets: bucket_list,
            owner_id: "claudefs".to_string(),
            owner_display_name: "ClaudeFS".to_string(),
        })
    }

    /// Creates a new bucket with the given name.
    pub fn create_bucket(&self, bucket: &str) -> Result<()> {
        self.validate_bucket_name(bucket)?;
        let mut buckets = self.buckets.write().map_err(|_| GatewayError::Nfs3Io)?;
        if buckets.contains_key(bucket) {
            return Err(GatewayError::S3BucketNotFound {
                bucket: bucket.to_string(),
            });
        }
        buckets.insert(bucket.to_string(), BucketState::new(bucket));
        Ok(())
    }

    /// Deletes a bucket (must be empty).
    pub fn delete_bucket(&self, bucket: &str) -> Result<()> {
        let mut buckets = self.buckets.write().map_err(|_| GatewayError::Nfs3Io)?;
        if let Some(bs) = buckets.remove(bucket) {
            if bs.objects.is_empty() {
                Ok(())
            } else {
                Err(GatewayError::ProtocolError {
                    reason: "bucket not empty".to_string(),
                })
            }
        } else {
            Err(GatewayError::S3BucketNotFound {
                bucket: bucket.to_string(),
            })
        }
    }

    /// Checks if a bucket exists.
    pub fn head_bucket(&self, bucket: &str) -> Result<()> {
        let buckets = self.buckets.read().map_err(|_| GatewayError::Nfs3Io)?;
        if buckets.contains_key(bucket) {
            Ok(())
        } else {
            Err(GatewayError::S3BucketNotFound {
                bucket: bucket.to_string(),
            })
        }
    }

    /// Lists objects in a bucket with optional prefix and delimiter.
    pub fn list_objects(
        &self,
        bucket: &str,
        prefix: &str,
        delimiter: Option<&str>,
        max_keys: u32,
    ) -> Result<ListObjectsResult> {
        let buckets = self.buckets.read().map_err(|_| GatewayError::Nfs3Io)?;
        let bs = buckets.get(bucket).ok_or(GatewayError::S3BucketNotFound {
            bucket: bucket.to_string(),
        })?;

        let mut objects = Vec::new();
        let mut common_prefixes_set = std::collections::HashSet::new();

        let max_keys = if max_keys == 0 { 1000 } else { max_keys };

        for (key, obj) in &bs.objects {
            if !key.starts_with(prefix) {
                continue;
            }

            let after_prefix = &key[prefix.len()..];

            if let Some(delim) = delimiter {
                if let Some(pos) = after_prefix.find(delim) {
                    let common_prefix = format!("{}{}", prefix, &after_prefix[..pos + delim.len()]);
                    common_prefixes_set.insert(common_prefix);
                    continue;
                }
            }

            if objects.len() < max_keys as usize {
                objects.push(ObjectMeta {
                    key: key.clone(),
                    size: obj.meta.size,
                    etag: obj.meta.etag.clone(),
                    last_modified: obj.meta.last_modified.clone(),
                    content_type: obj.meta.content_type.clone(),
                });
            }
        }

        let mut common_prefixes: Vec<String> = common_prefixes_set.into_iter().collect();
        common_prefixes.sort();

        let is_truncated = objects.len() == max_keys as usize;

        Ok(ListObjectsResult {
            bucket: bucket.to_string(),
            prefix: prefix.to_string(),
            delimiter: delimiter.map(|s| s.to_string()),
            objects,
            common_prefixes,
            is_truncated,
            next_continuation_token: None,
            max_keys,
        })
    }

    /// Gets an object's metadata and data.
    pub fn get_object(&self, bucket: &str, key: &str) -> Result<(ObjectMeta, Vec<u8>)> {
        let buckets = self.buckets.read().map_err(|_| GatewayError::Nfs3Io)?;
        let bs = buckets.get(bucket).ok_or(GatewayError::S3BucketNotFound {
            bucket: bucket.to_string(),
        })?;
        let obj = bs.objects.get(key).ok_or(GatewayError::S3ObjectNotFound {
            key: key.to_string(),
        })?;
        Ok((obj.meta.clone(), obj.data.clone()))
    }

    /// Stores an object in a bucket.
    pub fn put_object(
        &self,
        bucket: &str,
        key: &str,
        content_type: &str,
        data: Vec<u8>,
    ) -> Result<ObjectMeta> {
        let mut buckets = self.buckets.write().map_err(|_| GatewayError::Nfs3Io)?;
        let bs = buckets
            .get_mut(bucket)
            .ok_or(GatewayError::S3BucketNotFound {
                bucket: bucket.to_string(),
            })?;

        let now = chrono_timestamp();
        let etag = generate_etag();
        let size = data.len() as u64;

        let meta = ObjectMeta {
            key: key.to_string(),
            size,
            etag: etag.clone(),
            last_modified: now,
            content_type: content_type.to_string(),
        };

        bs.objects.insert(
            key.to_string(),
            ObjectData {
                meta: meta.clone(),
                data,
            },
        );

        Ok(meta)
    }

    /// Deletes an object from a bucket.
    pub fn delete_object(&self, bucket: &str, key: &str) -> Result<()> {
        let mut buckets = self.buckets.write().map_err(|_| GatewayError::Nfs3Io)?;
        let bs = buckets
            .get_mut(bucket)
            .ok_or(GatewayError::S3BucketNotFound {
                bucket: bucket.to_string(),
            })?;

        if bs.objects.remove(key).is_some() {
            Ok(())
        } else {
            Err(GatewayError::S3ObjectNotFound {
                key: key.to_string(),
            })
        }
    }

    /// Gets object metadata without the data.
    pub fn head_object(&self, bucket: &str, key: &str) -> Result<ObjectMeta> {
        let buckets = self.buckets.read().map_err(|_| GatewayError::Nfs3Io)?;
        let bs = buckets.get(bucket).ok_or(GatewayError::S3BucketNotFound {
            bucket: bucket.to_string(),
        })?;
        bs.objects
            .get(key)
            .map(|o| o.meta.clone())
            .ok_or(GatewayError::S3ObjectNotFound {
                key: key.to_string(),
            })
    }

    /// Copies an object from one location to another.
    pub fn copy_object(
        &self,
        src_bucket: &str,
        src_key: &str,
        dst_bucket: &str,
        dst_key: &str,
    ) -> Result<ObjectMeta> {
        let (meta, data) = self.get_object(src_bucket, src_key)?;
        self.put_object(dst_bucket, dst_key, &meta.content_type, data)
    }

    /// Returns the number of objects in a bucket.
    pub fn object_count(&self, bucket: &str) -> Result<usize> {
        let buckets = self.buckets.read().map_err(|_| GatewayError::Nfs3Io)?;
        let bs = buckets.get(bucket).ok_or(GatewayError::S3BucketNotFound {
            bucket: bucket.to_string(),
        })?;
        Ok(bs.objects.len())
    }

    /// Returns the total size of all objects in a bucket (in bytes).
    pub fn bucket_size(&self, bucket: &str) -> Result<u64> {
        let buckets = self.buckets.read().map_err(|_| GatewayError::Nfs3Io)?;
        let bs = buckets.get(bucket).ok_or(GatewayError::S3BucketNotFound {
            bucket: bucket.to_string(),
        })?;
        let total: u64 = bs.objects.values().map(|o| o.meta.size).sum();
        Ok(total)
    }
}

impl Default for S3Handler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_list_bucket() {
        let handler = S3Handler::new();
        handler.create_bucket("test-bucket").unwrap();

        let result = handler.list_buckets().unwrap();
        assert_eq!(result.buckets.len(), 1);
        assert_eq!(result.buckets[0].name, "test-bucket");
    }

    #[test]
    fn test_delete_bucket() {
        let handler = S3Handler::new();
        handler.create_bucket("test-bucket").unwrap();
        handler.delete_bucket("test-bucket").unwrap();

        let result = handler.list_buckets().unwrap();
        assert!(result.buckets.is_empty());
    }

    #[test]
    fn test_bucket_not_found() {
        let handler = S3Handler::new();
        let result = handler.head_bucket("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_bucket_not_empty_on_delete() {
        let handler = S3Handler::new();
        handler.create_bucket("test-bucket").unwrap();
        handler
            .put_object("test-bucket", "key1", "text/plain", b"data".to_vec())
            .unwrap();
        let result = handler.delete_bucket("test-bucket");
        assert!(result.is_err());
    }

    #[test]
    fn test_put_and_get_object() {
        let handler = S3Handler::new();
        handler.create_bucket("mybucket").unwrap();
        handler
            .put_object("mybucket", "mykey", "text/plain", b"hello world".to_vec())
            .unwrap();

        let (meta, data) = handler.get_object("mybucket", "mykey").unwrap();
        assert_eq!(data, b"hello world");
        assert_eq!(meta.content_type, "text/plain");
    }

    #[test]
    fn test_object_not_found() {
        let handler = S3Handler::new();
        handler.create_bucket("mybucket").unwrap();
        let result = handler.get_object("mybucket", "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_object() {
        let handler = S3Handler::new();
        handler.create_bucket("mybucket").unwrap();
        handler
            .put_object("mybucket", "key1", "text/plain", b"data".to_vec())
            .unwrap();
        handler.delete_object("mybucket", "key1").unwrap();

        let result = handler.get_object("mybucket", "key1");
        assert!(result.is_err());
    }

    #[test]
    fn test_list_objects_with_prefix() {
        let handler = S3Handler::new();
        handler.create_bucket("mybucket").unwrap();
        handler
            .put_object("mybucket", "dir/file1.txt", "text/plain", b"data1".to_vec())
            .unwrap();
        handler
            .put_object("mybucket", "dir/file2.txt", "text/plain", b"data2".to_vec())
            .unwrap();
        handler
            .put_object("mybucket", "other.txt", "text/plain", b"data3".to_vec())
            .unwrap();

        let result = handler.list_objects("mybucket", "dir/", None, 100).unwrap();
        assert_eq!(result.objects.len(), 2);
    }

    #[test]
    fn test_list_objects_with_delimiter() {
        let handler = S3Handler::new();
        handler.create_bucket("mybucket").unwrap();
        handler
            .put_object("mybucket", "dir/file1.txt", "text/plain", b"data1".to_vec())
            .unwrap();
        handler
            .put_object("mybucket", "dir/file2.txt", "text/plain", b"data2".to_vec())
            .unwrap();

        let result = handler
            .list_objects("mybucket", "", Some("/"), 100)
            .unwrap();
        assert_eq!(result.common_prefixes.len(), 1);
    }

    #[test]
    fn test_copy_object() {
        let handler = S3Handler::new();
        handler.create_bucket("src").unwrap();
        handler.create_bucket("dst").unwrap();
        handler
            .put_object("src", "source.txt", "text/plain", b"original data".to_vec())
            .unwrap();

        handler
            .copy_object("src", "source.txt", "dst", "dest.txt")
            .unwrap();

        let (meta, data) = handler.get_object("dst", "dest.txt").unwrap();
        assert_eq!(data, b"original data");
        assert_eq!(meta.content_type, "text/plain");
    }

    #[test]
    fn test_bucket_name_validation() {
        let handler = S3Handler::new();

        let result = handler.create_bucket("ab");
        assert!(result.is_err());

        let result = handler.create_bucket("ab!");
        assert!(result.is_err());

        let result = handler.create_bucket("-bucket");
        assert!(result.is_err());

        let result = handler.create_bucket("bucket-");
        assert!(result.is_err());

        let result = handler.create_bucket("valid-bucket");
        assert!(result.is_ok());
    }

    #[test]
    fn test_object_count() {
        let handler = S3Handler::new();
        handler.create_bucket("mybucket").unwrap();
        handler
            .put_object("mybucket", "key1", "text/plain", b"data1".to_vec())
            .unwrap();
        handler
            .put_object("mybucket", "key2", "text/plain", b"data2".to_vec())
            .unwrap();

        let count = handler.object_count("mybucket").unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_bucket_size() {
        let handler = S3Handler::new();
        handler.create_bucket("mybucket").unwrap();
        handler
            .put_object("mybucket", "key1", "text/plain", b"hello".to_vec())
            .unwrap();
        handler
            .put_object("mybucket", "key2", "text/plain", b"world".to_vec())
            .unwrap();

        let size = handler.bucket_size("mybucket").unwrap();
        assert_eq!(size, 10);
    }

    #[test]
    fn test_head_object() {
        let handler = S3Handler::new();
        handler.create_bucket("mybucket").unwrap();
        handler
            .put_object("mybucket", "key1", "text/plain", b"data".to_vec())
            .unwrap();

        let meta = handler.head_object("mybucket", "key1").unwrap();
        assert_eq!(meta.content_type, "text/plain");
    }

    #[test]
    fn test_multiple_buckets() {
        let handler = S3Handler::new();
        handler.create_bucket("bucket1").unwrap();
        handler.create_bucket("bucket2").unwrap();
        handler.create_bucket("bucket3").unwrap();

        let result = handler.list_buckets().unwrap();
        assert_eq!(result.buckets.len(), 3);
    }

    #[test]
    fn test_overwrite_object() {
        let handler = S3Handler::new();
        handler.create_bucket("mybucket").unwrap();
        handler
            .put_object("mybucket", "key", "text/plain", b"v1".to_vec())
            .unwrap();
        handler
            .put_object("mybucket", "key", "text/plain", b"v2".to_vec())
            .unwrap();

        let (meta, data) = handler.get_object("mybucket", "key").unwrap();
        assert_eq!(data, b"v2");
    }

    #[test]
    fn test_etag_generation() {
        let handler = S3Handler::new();
        handler.create_bucket("mybucket").unwrap();
        let meta = handler
            .put_object("mybucket", "key", "text/plain", b"data".to_vec())
            .unwrap();
        assert!(meta.etag.starts_with('"'));
        assert!(meta.etag.ends_with('"'));
    }
}
