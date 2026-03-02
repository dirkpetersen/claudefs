//! S3 Object Versioning support.
//!
//! Manages object version history for S3-compatible buckets. When versioning is enabled,
//! PUT operations create new versions rather than overwriting, and DELETE creates a
//! delete marker. Supports listing, fetching, and permanently deleting specific versions.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use tracing::{debug, info};

/// Versioning state of a bucket
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VersioningState {
    /// Never enabled
    Unversioned,
    /// Versioning is enabled
    Enabled,
    /// Was enabled, now suspended — new objects get null version
    Suspended,
}

/// A unique version identifier (alphanumeric string, ~32 chars)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionId(pub String);

impl VersionId {
    /// Generate a new unique version ID from timestamp + random suffix
    /// Use format: "{unix_secs_hex}-{8_hex_chars}" e.g. "67b5c2a1-f3a8b2c1"
    pub fn generate(timestamp_secs: u64, random_suffix: u32) -> Self {
        let ts_hex = format!("{:08x}", timestamp_secs);
        let rand_hex = format!("{:08x}", random_suffix);
        VersionId(format!("{}-{}", ts_hex, rand_hex))
    }

    /// Returns the version ID as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns true if this is the special "null" version for Unversioned objects
    pub fn is_null(&self) -> bool {
        self.0 == "null"
    }

    /// Returns the special null version ID for unversioned objects
    pub fn null() -> Self {
        VersionId("null".to_string())
    }
}

/// Type of a version entry
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VersionType {
    /// A real object version
    Object,
    /// A delete marker
    DeleteMarker,
}

/// A version entry in the version history of an object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionEntry {
    /// The unique version identifier
    pub version_id: VersionId,
    /// The type of version (object or delete marker)
    pub version_type: VersionType,
    /// Unix timestamp when this version was created
    pub last_modified_secs: u64,
    /// Size of the object (0 for delete markers)
    pub size: u64,
    /// ETag of the object (empty for delete markers)
    pub etag: String,
    /// Whether this is the latest version
    pub is_latest: bool,
}

impl VersionEntry {
    /// Returns true if this version entry is a delete marker
    pub fn is_delete_marker(&self) -> bool {
        self.version_type == VersionType::DeleteMarker
    }

    /// Returns true if this version entry is a real object (not a delete marker)
    pub fn is_object(&self) -> bool {
        self.version_type == VersionType::Object
    }
}

/// Version list for a single object key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectVersionList {
    versions: Vec<VersionEntry>,
}

impl ObjectVersionList {
    /// Creates a new empty version list
    pub fn new() -> Self {
        Self {
            versions: Vec::new(),
        }
    }

    /// Add a new version (automatically sets is_latest on newest, clears on others)
    pub fn add_version(&mut self, mut entry: VersionEntry) {
        for v in &mut self.versions {
            v.is_latest = false;
        }
        entry.is_latest = true;
        self.versions.push(entry);
    }

    /// Get the latest version (None if list is empty)
    pub fn latest(&self) -> Option<&VersionEntry> {
        self.versions.last()
    }

    /// Get a specific version by ID
    pub fn get_version(&self, version_id: &str) -> Option<&VersionEntry> {
        self.versions
            .iter()
            .find(|v| v.version_id.as_str() == version_id)
    }

    /// Returns true if the latest version is a delete marker (object is "deleted")
    pub fn is_deleted(&self) -> bool {
        self.latest().map(|v| v.is_delete_marker()).unwrap_or(false)
    }

    /// List all versions, newest first
    pub fn list_versions(&self) -> &[VersionEntry] {
        &self.versions
    }

    /// Count of versions (including delete markers)
    pub fn len(&self) -> usize {
        self.versions.len()
    }

    /// Returns true if there are no versions
    pub fn is_empty(&self) -> bool {
        self.versions.is_empty()
    }
}

impl Default for ObjectVersionList {
    fn default() -> Self {
        Self::new()
    }
}

/// Versioning configuration for a bucket
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketVersioning {
    /// Current versioning state
    pub state: VersioningState,
    /// Whether MFA is required to delete versions
    pub mfa_delete: bool,
}

impl BucketVersioning {
    /// Creates a new unversioned bucket configuration
    pub fn new() -> Self {
        Self {
            state: VersioningState::Unversioned,
            mfa_delete: false,
        }
    }

    /// Enable versioning on the bucket
    pub fn enable(&mut self) {
        info!("Enabling versioning on bucket");
        self.state = VersioningState::Enabled;
    }

    /// Suspend versioning on the bucket
    pub fn suspend(&mut self) {
        info!("Suspending versioning on bucket");
        self.state = VersioningState::Suspended;
    }

    /// Returns true if versioning is enabled
    pub fn is_enabled(&self) -> bool {
        self.state == VersioningState::Enabled
    }

    /// Returns true if versioning is suspended
    pub fn is_suspended(&self) -> bool {
        self.state == VersioningState::Suspended
    }

    /// Returns the effective version ID for a new object:
    /// - Enabled: generate a real version ID
    /// - Suspended/Unversioned: return VersionId::null()
    pub fn effective_version_id(&self, timestamp_secs: u64, random_suffix: u32) -> VersionId {
        if self.is_enabled() {
            VersionId::generate(timestamp_secs, random_suffix)
        } else {
            VersionId::null()
        }
    }
}

impl Default for BucketVersioning {
    fn default() -> Self {
        Self::new()
    }
}

/// Registry managing versioning state for all buckets
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersioningRegistry {
    /// Bucket name -> versioning configuration
    buckets: HashMap<String, BucketVersioning>,
    /// version lists: bucket -> key -> ObjectVersionList
    versions: HashMap<String, HashMap<String, ObjectVersionList>>,
}

impl VersioningRegistry {
    /// Creates a new empty versioning registry
    pub fn new() -> Self {
        Self {
            buckets: HashMap::new(),
            versions: HashMap::new(),
        }
    }

    /// Set versioning state for a bucket
    pub fn set_versioning(&mut self, bucket: &str, state: VersioningState) {
        let bucket_config = self.buckets.entry(bucket.to_string()).or_default();
        match state {
            VersioningState::Enabled => bucket_config.enable(),
            VersioningState::Suspended => bucket_config.suspend(),
            VersioningState::Unversioned => {
                info!("Disabling versioning on bucket {}", bucket);
                bucket_config.state = VersioningState::Unversioned;
            }
        }
    }

    /// Get versioning state for a bucket
    pub fn get_versioning(&self, bucket: &str) -> VersioningState {
        self.buckets
            .get(bucket)
            .map(|b| b.state)
            .unwrap_or(VersioningState::Unversioned)
    }

    /// Record a new object version (call on PUT)
    pub fn put_version(
        &mut self,
        bucket: &str,
        key: &str,
        entry: VersionEntry,
    ) -> Result<(), VersioningError> {
        debug!("Putting version for bucket={}, key={}", bucket, key);

        let bucket_versions = self.versions.entry(bucket.to_string()).or_default();

        let key_versions = bucket_versions.entry(key.to_string()).or_default();

        key_versions.add_version(entry);

        Ok(())
    }

    /// Get the latest visible object version for a key
    /// Returns None if key doesn't exist or latest is a delete marker
    pub fn get_current(&self, bucket: &str, key: &str) -> Option<&VersionEntry> {
        self.versions
            .get(bucket)
            .and_then(|b| b.get(key))
            .and_then(|versions| {
                let latest = versions.latest()?;
                if latest.is_delete_marker() {
                    None
                } else {
                    Some(latest)
                }
            })
    }

    /// Get a specific version
    pub fn get_version(&self, bucket: &str, key: &str, version_id: &str) -> Option<&VersionEntry> {
        self.versions
            .get(bucket)
            .and_then(|b| b.get(key))
            .and_then(|versions| versions.get_version(version_id))
    }

    /// List all versions for a key
    pub fn list_versions(&self, bucket: &str, key: &str) -> &[VersionEntry] {
        self.versions
            .get(bucket)
            .and_then(|b| b.get(key))
            .map(|versions| versions.list_versions())
            .unwrap_or(&[])
    }

    /// Delete a specific version permanently
    pub fn delete_version(
        &mut self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<bool, VersioningError> {
        let bucket_versions = self
            .versions
            .get_mut(bucket)
            .ok_or_else(|| VersioningError::BucketNotFound(bucket.to_string()))?;

        let key_versions = bucket_versions.get_mut(key).ok_or_else(|| {
            VersioningError::VersionNotFound(version_id.to_string(), key.to_string())
        })?;

        let pos = key_versions
            .versions
            .iter()
            .position(|v| v.version_id.as_str() == version_id)
            .ok_or_else(|| {
                VersioningError::VersionNotFound(version_id.to_string(), key.to_string())
            })?;

        key_versions.versions.remove(pos);

        if let Some(new_latest) = key_versions.versions.last_mut() {
            new_latest.is_latest = true;
        }

        Ok(true)
    }

    /// Add a delete marker (call on DELETE without version_id when versioning enabled)
    pub fn add_delete_marker(
        &mut self,
        bucket: &str,
        key: &str,
        version_id: VersionId,
        timestamp_secs: u64,
    ) -> Result<(), VersioningError> {
        debug!("Adding delete marker for bucket={}, key={}", bucket, key);

        let entry = VersionEntry {
            version_id,
            version_type: VersionType::DeleteMarker,
            last_modified_secs: timestamp_secs,
            size: 0,
            etag: String::new(),
            is_latest: true,
        };

        self.put_version(bucket, key, entry)
    }
}

impl Default for VersioningRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors for versioning operations
#[derive(Debug, Error)]
pub enum VersioningError {
    /// The specified bucket does not exist
    #[error("bucket {0} does not exist")]
    BucketNotFound(String),

    /// The specified version was not found for the given key
    #[error("version {0} not found for key {1}")]
    VersionNotFound(String, String),

    /// MFA delete is enabled but MFA token was not provided
    #[error("MFA delete required but not provided")]
    MfaDeleteRequired,

    /// Cannot modify versioning because bucket has existing versioned objects
    #[error("cannot modify versioning: bucket has existing versions")]
    BucketHasVersions,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_id_generate() {
        let id = VersionId::generate(1234567890, 0xDEADBEEF);
        let s = id.as_str();
        assert!(!s.is_empty());
        assert!(s.contains('-'));
    }

    #[test]
    fn test_version_id_null() {
        let id = VersionId::null();
        assert_eq!(id.as_str(), "null");
    }

    #[test]
    fn test_version_id_is_null() {
        assert!(VersionId::null().is_null());
        assert!(!VersionId::generate(123, 456).is_null());
    }

    #[test]
    fn test_version_entry_is_delete_marker() {
        let entry = VersionEntry {
            version_id: VersionId::generate(123, 456),
            version_type: VersionType::DeleteMarker,
            last_modified_secs: 123,
            size: 0,
            etag: String::new(),
            is_latest: true,
        };
        assert!(entry.is_delete_marker());
        assert!(!entry.is_object());
    }

    #[test]
    fn test_version_entry_is_object() {
        let entry = VersionEntry {
            version_id: VersionId::generate(123, 456),
            version_type: VersionType::Object,
            last_modified_secs: 123,
            size: 100,
            etag: "abc123".to_string(),
            is_latest: true,
        };
        assert!(entry.is_object());
        assert!(!entry.is_delete_marker());
    }

    #[test]
    fn test_object_version_list_add_version() {
        let mut list = ObjectVersionList::new();

        let entry1 = VersionEntry {
            version_id: VersionId::generate(100, 1),
            version_type: VersionType::Object,
            last_modified_secs: 100,
            size: 100,
            etag: "etag1".to_string(),
            is_latest: false,
        };
        list.add_version(entry1);

        assert!(list.latest().unwrap().is_latest);

        let entry2 = VersionEntry {
            version_id: VersionId::generate(200, 2),
            version_type: VersionType::Object,
            last_modified_secs: 200,
            size: 200,
            etag: "etag2".to_string(),
            is_latest: false,
        };
        list.add_version(entry2);

        assert!(list.latest().unwrap().is_latest);
        for v in list.list_versions() {
            if v.version_id.as_str() != list.latest().unwrap().version_id.as_str() {
                assert!(!v.is_latest);
            }
        }
    }

    #[test]
    fn test_object_version_list_latest() {
        let mut list = ObjectVersionList::new();
        assert!(list.latest().is_none());

        list.add_version(VersionEntry {
            version_id: VersionId::generate(100, 1),
            version_type: VersionType::Object,
            last_modified_secs: 100,
            size: 100,
            etag: "etag1".to_string(),
            is_latest: false,
        });

        let latest = list.latest().unwrap();
        assert_eq!(latest.last_modified_secs, 100);

        list.add_version(VersionEntry {
            version_id: VersionId::generate(200, 2),
            version_type: VersionType::Object,
            last_modified_secs: 200,
            size: 200,
            etag: "etag2".to_string(),
            is_latest: false,
        });

        assert_eq!(list.latest().unwrap().last_modified_secs, 200);
    }

    #[test]
    fn test_object_version_list_get_version() {
        let mut list = ObjectVersionList::new();

        let v1 = VersionId::generate(100, 1);
        let v2 = VersionId::generate(200, 2);

        list.add_version(VersionEntry {
            version_id: v1.clone(),
            version_type: VersionType::Object,
            last_modified_secs: 100,
            size: 100,
            etag: "etag1".to_string(),
            is_latest: false,
        });

        list.add_version(VersionEntry {
            version_id: v2.clone(),
            version_type: VersionType::Object,
            last_modified_secs: 200,
            size: 200,
            etag: "etag2".to_string(),
            is_latest: false,
        });

        assert!(list.get_version(v1.as_str()).is_some());
        assert!(list.get_version(v2.as_str()).is_some());
        assert!(list.get_version("nonexistent").is_none());
    }

    #[test]
    fn test_object_version_list_is_deleted() {
        let mut list = ObjectVersionList::new();

        list.add_version(VersionEntry {
            version_id: VersionId::generate(100, 1),
            version_type: VersionType::DeleteMarker,
            last_modified_secs: 100,
            size: 0,
            etag: String::new(),
            is_latest: true,
        });

        assert!(list.is_deleted());
    }

    #[test]
    fn test_object_version_list_not_deleted() {
        let mut list = ObjectVersionList::new();

        list.add_version(VersionEntry {
            version_id: VersionId::generate(100, 1),
            version_type: VersionType::Object,
            last_modified_secs: 100,
            size: 100,
            etag: "etag1".to_string(),
            is_latest: true,
        });

        assert!(!list.is_deleted());
    }

    #[test]
    fn test_object_version_list_empty() {
        let mut list = ObjectVersionList::new();
        assert!(list.is_empty());
        assert_eq!(list.len(), 0);

        list.add_version(VersionEntry {
            version_id: VersionId::generate(100, 1),
            version_type: VersionType::Object,
            last_modified_secs: 100,
            size: 100,
            etag: "etag1".to_string(),
            is_latest: false,
        });

        assert!(!list.is_empty());
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn test_bucket_versioning_enable() {
        let mut config = BucketVersioning::new();
        assert_eq!(config.state, VersioningState::Unversioned);

        config.enable();
        assert_eq!(config.state, VersioningState::Enabled);
        assert!(config.is_enabled());
    }

    #[test]
    fn test_bucket_versioning_suspend() {
        let mut config = BucketVersioning::new();
        config.enable();
        assert!(config.is_enabled());

        config.suspend();
        assert_eq!(config.state, VersioningState::Suspended);
        assert!(config.is_suspended());
        assert!(!config.is_enabled());
    }

    #[test]
    fn test_bucket_versioning_is_enabled() {
        let mut config = BucketVersioning::new();
        assert!(!config.is_enabled());

        config.enable();
        assert!(config.is_enabled());

        config.suspend();
        assert!(!config.is_enabled());
    }

    #[test]
    fn test_effective_version_id_enabled() {
        let mut config = BucketVersioning::new();
        config.enable();

        let vid = config.effective_version_id(1234567890, 0xDEADBEEF);
        assert!(!vid.is_null());
    }

    #[test]
    fn test_effective_version_id_suspended() {
        let mut config = BucketVersioning::new();
        config.suspend();

        let vid = config.effective_version_id(1234567890, 0xDEADBEEF);
        assert!(vid.is_null());

        let config2 = BucketVersioning::new();
        let vid2 = config2.effective_version_id(1234567890, 0xDEADBEEF);
        assert!(vid2.is_null());
    }

    #[test]
    fn test_registry_set_get_versioning() {
        let mut registry = VersioningRegistry::new();

        assert_eq!(
            registry.get_versioning("mybucket"),
            VersioningState::Unversioned
        );

        registry.set_versioning("mybucket", VersioningState::Enabled);
        assert_eq!(
            registry.get_versioning("mybucket"),
            VersioningState::Enabled
        );

        registry.set_versioning("mybucket", VersioningState::Suspended);
        assert_eq!(
            registry.get_versioning("mybucket"),
            VersioningState::Suspended
        );
    }

    #[test]
    fn test_registry_put_and_get_current() {
        let mut registry = VersioningRegistry::new();
        registry.set_versioning("mybucket", VersioningState::Enabled);

        let entry = VersionEntry {
            version_id: VersionId::generate(123, 456),
            version_type: VersionType::Object,
            last_modified_secs: 123,
            size: 1000,
            etag: "abc123".to_string(),
            is_latest: true,
        };

        registry.put_version("mybucket", "mykey", entry).unwrap();

        let current = registry.get_current("mybucket", "mykey");
        assert!(current.is_some());
        assert_eq!(current.unwrap().size, 1000);
    }

    #[test]
    fn test_registry_get_current_deleted() {
        let mut registry = VersioningRegistry::new();
        registry.set_versioning("mybucket", VersioningState::Enabled);

        registry
            .add_delete_marker("mybucket", "mykey", VersionId::generate(200, 2), 200)
            .unwrap();

        let current = registry.get_current("mybucket", "mykey");
        assert!(current.is_none());
    }

    #[test]
    fn test_registry_add_delete_marker() {
        let mut registry = VersioningRegistry::new();
        registry.set_versioning("mybucket", VersioningState::Enabled);

        let vid = VersionId::generate(200, 2);
        registry
            .add_delete_marker("mybucket", "mykey", vid.clone(), 200)
            .unwrap();

        let versions = registry.list_versions("mybucket", "mykey");
        assert_eq!(versions.len(), 1);
        assert!(versions[0].is_delete_marker());
    }

    #[test]
    fn test_registry_delete_specific_version() {
        let mut registry = VersioningRegistry::new();
        registry.set_versioning("mybucket", VersioningState::Enabled);

        let v1 = VersionId::generate(100, 1);
        let v2 = VersionId::generate(200, 2);

        registry
            .put_version(
                "mybucket",
                "mykey",
                VersionEntry {
                    version_id: v1.clone(),
                    version_type: VersionType::Object,
                    last_modified_secs: 100,
                    size: 100,
                    etag: "etag1".to_string(),
                    is_latest: false,
                },
            )
            .unwrap();

        registry
            .put_version(
                "mybucket",
                "mykey",
                VersionEntry {
                    version_id: v2.clone(),
                    version_type: VersionType::Object,
                    last_modified_secs: 200,
                    size: 200,
                    etag: "etag2".to_string(),
                    is_latest: false,
                },
            )
            .unwrap();

        assert_eq!(registry.list_versions("mybucket", "mykey").len(), 2);

        registry
            .delete_version("mybucket", "mykey", v1.as_str())
            .unwrap();

        assert_eq!(registry.list_versions("mybucket", "mykey").len(), 1);
        assert_eq!(
            registry.list_versions("mybucket", "mykey")[0].version_id,
            v2
        );
    }

    #[test]
    fn test_registry_list_versions() {
        let mut registry = VersioningRegistry::new();
        registry.set_versioning("mybucket", VersioningState::Enabled);

        for i in 1..=3 {
            registry
                .put_version(
                    "mybucket",
                    "mykey",
                    VersionEntry {
                        version_id: VersionId::generate(100 * i, i as u32),
                        version_type: VersionType::Object,
                        last_modified_secs: 100 * i,
                        size: 100 * i,
                        etag: format!("etag{}", i),
                        is_latest: false,
                    },
                )
                .unwrap();
        }

        let versions = registry.list_versions("mybucket", "mykey");
        assert_eq!(versions.len(), 3);

        assert!(versions[2].last_modified_secs > versions[1].last_modified_secs);
        assert!(versions[1].last_modified_secs > versions[0].last_modified_secs);
    }
}
