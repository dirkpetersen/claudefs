//! Gateway S3 versioning and multipart upload security tests.
//!
//! Part of A10 Phase 18: S3 versioning & multipart security audit

use claudefs_gateway::s3_multipart::{
    MultipartManager, MultipartState, MultipartUpload, UploadPart,
};
use claudefs_gateway::s3_versioning::{
    BucketVersioning, ObjectVersionList, VersionEntry, VersionId, VersionType, VersioningError,
    VersioningRegistry, VersioningState,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn make_version_id(timestamp: u64, suffix: u32) -> VersionId {
        VersionId::generate(timestamp, suffix)
    }

    fn make_object_version_entry(
        version_id: VersionId,
        last_modified: u64,
        size: u64,
        etag: &str,
    ) -> VersionEntry {
        VersionEntry {
            version_id,
            version_type: VersionType::Object,
            last_modified_secs: last_modified,
            size,
            etag: etag.to_string(),
            is_latest: false,
        }
    }

    fn make_delete_marker(version_id: VersionId, last_modified: u64) -> VersionEntry {
        VersionEntry {
            version_id,
            version_type: VersionType::DeleteMarker,
            last_modified_secs: last_modified,
            size: 0,
            etag: String::new(),
            is_latest: true,
        }
    }

    fn make_bucket_versioning() -> BucketVersioning {
        BucketVersioning::new()
    }

    fn make_versioning_registry() -> VersioningRegistry {
        VersioningRegistry::new()
    }

    fn make_multipart_upload(
        id: &str,
        bucket: &str,
        key: &str,
        content_type: &str,
    ) -> MultipartUpload {
        MultipartUpload::new(id, bucket, key, content_type)
    }

    fn make_upload_part(part_number: u32, etag: &str, size: u64) -> UploadPart {
        UploadPart {
            part_number,
            etag: etag.to_string(),
            size,
        }
    }

    fn make_multipart_manager() -> MultipartManager {
        MultipartManager::new()
    }

    // =========================================================================
    // Category 1: S3 Versioning State (5 tests)
    // =========================================================================

    #[test]
    fn test_version_id_generate_unique() {
        let mut ids = HashSet::new();
        for i in 0..50u32 {
            let ts = 1000000 + (i as u64) * 1000;
            let suffix = 0x1000 + i;
            let id = make_version_id(ts, suffix);
            assert!(
                !id.as_str().is_empty(),
                "VersionId as_str() should be non-empty"
            );
            assert!(!id.is_null(), "Generated version ID should not be null");
            ids.insert(id.as_str().to_string());
        }
        assert_eq!(ids.len(), 50, "All 50 version IDs should be unique");
    }

    #[test]
    fn test_version_id_null() {
        let null_id = VersionId::null();
        assert!(null_id.is_null(), "null() version ID should be null");
        assert_eq!(
            null_id.as_str(),
            "null",
            "null() should return 'null' string"
        );
        let generated = VersionId::generate(123, 456);
        assert!(
            !generated.is_null(),
            "Generated version ID should not be null"
        );
    }

    #[test]
    fn test_bucket_versioning_state_machine() {
        let mut config = make_bucket_versioning();
        assert_eq!(
            config.state,
            VersioningState::Unversioned,
            "Initial state should be Unversioned"
        );

        config.enable();
        assert!(
            config.is_enabled(),
            "After enable, is_enabled() should be true"
        );

        config.suspend();
        assert!(
            config.is_suspended(),
            "After suspend, is_suspended() should be true"
        );
        assert!(
            !config.is_enabled(),
            "After suspend, is_enabled() should be false"
        );

        let mut enabled_config = BucketVersioning::new();
        enabled_config.enable();
        let effective_enabled = enabled_config.effective_version_id(1234567890, 0xDEADBEEF);
        assert!(
            !effective_enabled.is_null(),
            "Effective version ID when enabled should be non-null"
        );

        let mut suspended_config = BucketVersioning::new();
        suspended_config.suspend();
        let effective_suspended = suspended_config.effective_version_id(1234567890, 0xDEADBEEF);
        assert!(
            effective_suspended.is_null(),
            "Effective version ID when suspended should be null"
        );
    }

    #[test]
    fn test_versioning_registry_set_get() {
        let mut registry = make_versioning_registry();

        assert_eq!(
            registry.get_versioning("bucket1"),
            VersioningState::Unversioned,
            "Default versioning should be Unversioned"
        );

        registry.set_versioning("bucket1", VersioningState::Enabled);
        assert_eq!(
            registry.get_versioning("bucket1"),
            VersioningState::Enabled,
            "After set to Enabled, should be Enabled"
        );

        registry.set_versioning("bucket1", VersioningState::Suspended);
        assert_eq!(
            registry.get_versioning("bucket1"),
            VersioningState::Suspended,
            "After set to Suspended, should be Suspended"
        );

        registry.set_versioning("bucket2", VersioningState::Enabled);
        assert_eq!(
            registry.get_versioning("bucket2"),
            VersioningState::Enabled,
            "bucket2 should be independent"
        );
        assert_eq!(
            registry.get_versioning("bucket1"),
            VersioningState::Suspended,
            "bucket1 should remain Suspended"
        );
    }

    #[test]
    fn test_version_entry_types() {
        let object_entry =
            make_object_version_entry(VersionId::generate(100, 1), 100, 1000, "etag1");
        assert!(
            object_entry.is_object(),
            "Object entry should return true for is_object()"
        );
        assert!(
            !object_entry.is_delete_marker(),
            "Object entry should return false for is_delete_marker()"
        );

        let dm_entry = make_delete_marker(VersionId::generate(200, 2), 200);
        assert!(
            dm_entry.is_delete_marker(),
            "Delete marker should return true for is_delete_marker()"
        );
        assert!(
            !dm_entry.is_object(),
            "Delete marker should return false for is_object()"
        );
    }

    // =========================================================================
    // Category 2: S3 Version Operations (5 tests)
    // =========================================================================

    #[test]
    fn test_version_list_add_and_latest() {
        let mut list = ObjectVersionList::new();

        list.add_version(make_object_version_entry(
            make_version_id(100, 1),
            100,
            100,
            "etag1",
        ));
        list.add_version(make_object_version_entry(
            make_version_id(200, 2),
            200,
            200,
            "etag2",
        ));
        list.add_version(make_object_version_entry(
            make_version_id(300, 3),
            300,
            300,
            "etag3",
        ));

        let latest = list.latest();
        assert!(latest.is_some(), "latest() should return Some");
        assert!(
            latest.unwrap().is_latest,
            "Latest version should have is_latest = true"
        );
        assert_eq!(list.len(), 3, "List should have 3 versions");

        for v in list.list_versions() {
            if v.version_id.as_str() != latest.unwrap().version_id.as_str() {
                assert!(
                    !v.is_latest,
                    "Non-latest entries should have is_latest = false"
                );
            }
        }
    }

    #[test]
    fn test_version_list_delete_marker_hides_object() {
        let mut list = ObjectVersionList::new();

        list.add_version(make_object_version_entry(
            make_version_id(100, 1),
            100,
            1000,
            "etag1",
        ));
        list.add_version(make_delete_marker(make_version_id(200, 2), 200));

        assert!(
            list.is_deleted(),
            "is_deleted() should return true when delete marker is latest"
        );
        let latest = list.latest();
        assert!(latest.is_some(), "latest() should still return Some");
        assert!(
            latest.unwrap().is_delete_marker(),
            "latest() should return the delete marker"
        );
    }

    #[test]
    fn test_registry_put_and_get_current() {
        let mut registry = make_versioning_registry();
        registry.set_versioning("bucket", VersioningState::Enabled);

        let entry = make_object_version_entry(make_version_id(100, 1), 100, 1000, "etag1");
        registry.put_version("bucket", "key1", entry).unwrap();

        let current = registry.get_current("bucket", "key1");
        assert!(current.is_some(), "get_current should return the object");

        let dm_entry = make_delete_marker(make_version_id(200, 2), 200);
        registry.put_version("bucket", "key1", dm_entry).unwrap();

        let current_after_delete = registry.get_current("bucket", "key1");
        assert!(
            current_after_delete.is_none(),
            "get_current should return None after delete marker"
        );
    }

    #[test]
    fn test_registry_get_specific_version() {
        let mut registry = make_versioning_registry();
        registry.set_versioning("bucket", VersioningState::Enabled);

        let v1 = make_version_id(100, 1);
        let v2 = make_version_id(200, 2);
        let v3 = make_version_id(300, 3);

        registry
            .put_version(
                "bucket",
                "key",
                make_object_version_entry(v1.clone(), 100, 100, "etag1"),
            )
            .unwrap();
        registry
            .put_version(
                "bucket",
                "key",
                make_object_version_entry(v2.clone(), 200, 200, "etag2"),
            )
            .unwrap();
        registry
            .put_version(
                "bucket",
                "key",
                make_object_version_entry(v3.clone(), 300, 300, "etag3"),
            )
            .unwrap();

        let fetched_v1 = registry.get_version("bucket", "key", v1.as_str());
        assert!(fetched_v1.is_some(), "Should find version v1");

        let fetched_v2 = registry.get_version("bucket", "key", v2.as_str());
        assert!(fetched_v2.is_some(), "Should find version v2");

        let fetched_v3 = registry.get_version("bucket", "key", v3.as_str());
        assert!(fetched_v3.is_some(), "Should find version v3");

        let nonexistent = registry.get_version("bucket", "key", "nonexistent-version-id");
        assert!(nonexistent.is_none(), "Should not find nonexistent version");
    }

    #[test]
    fn test_registry_delete_specific_version() {
        let mut registry = make_versioning_registry();
        registry.set_versioning("bucket", VersioningState::Enabled);

        let v1 = make_version_id(100, 1);
        let v2 = make_version_id(200, 2);
        let v3 = make_version_id(300, 3);

        registry
            .put_version(
                "bucket",
                "key",
                make_object_version_entry(v1.clone(), 100, 100, "etag1"),
            )
            .unwrap();
        registry
            .put_version(
                "bucket",
                "key",
                make_object_version_entry(v2.clone(), 200, 200, "etag2"),
            )
            .unwrap();
        registry
            .put_version(
                "bucket",
                "key",
                make_object_version_entry(v3.clone(), 300, 300, "etag3"),
            )
            .unwrap();

        assert_eq!(
            registry.list_versions("bucket", "key").len(),
            3,
            "Should have 3 versions before delete"
        );

        registry
            .delete_version("bucket", "key", v2.as_str())
            .unwrap();

        assert_eq!(
            registry.list_versions("bucket", "key").len(),
            2,
            "Should have 2 versions after delete"
        );

        let deleted = registry.get_version("bucket", "key", v2.as_str());
        assert!(
            deleted.is_none(),
            "Deleted version should not be accessible"
        );

        let latest = registry.get_current("bucket", "key");
        assert!(latest.is_some(), "Latest should still be accessible");
    }

    // =========================================================================
    // Category 3: Multipart Upload State Machine (5 tests)
    // =========================================================================

    #[test]
    fn test_multipart_upload_create() {
        let upload = make_multipart_upload("id1", "bucket", "key", "text/plain");

        assert_eq!(
            upload.state,
            MultipartState::Active,
            "New upload should be Active"
        );
        assert!(
            upload.parts.is_empty(),
            "New upload should have empty parts"
        );
        assert_eq!(
            upload.total_size(),
            0,
            "New upload should have total_size 0"
        );
    }

    #[test]
    fn test_multipart_add_part_validation() {
        let mut upload = make_multipart_upload("id1", "bucket", "key", "text/plain");

        let result1 = upload.add_part(make_upload_part(1, "etag1", 1024));
        assert!(result1.is_ok(), "Part 1 should be accepted");

        let result10000 = upload.add_part(make_upload_part(10000, "etag10000", 1024));
        assert!(result10000.is_ok(), "Part 10000 should be accepted");

        let result0 = upload.add_part(make_upload_part(0, "etag0", 1024));
        assert!(result0.is_err(), "Part 0 should be rejected");

        let result10001 = upload.add_part(make_upload_part(10001, "etag10001", 1024));
        assert!(result10001.is_err(), "Part 10001 should be rejected");
    }

    #[test]
    fn test_multipart_state_transitions() {
        let mut upload = make_multipart_upload("id1", "bucket", "key", "text/plain");

        upload.start_complete().unwrap();
        assert_eq!(
            upload.state,
            MultipartState::Completing,
            "After start_complete, state should be Completing"
        );

        upload.mark_completed().unwrap();
        assert_eq!(
            upload.state,
            MultipartState::Completed,
            "After mark_completed, state should be Completed"
        );

        let abort_result = upload.abort();
        assert!(abort_result.is_err(), "Abort after Completed should fail");
    }

    #[test]
    fn test_multipart_abort_active() {
        let mut upload = make_multipart_upload("id1", "bucket", "key", "text/plain");

        upload.abort().unwrap();
        assert_eq!(
            upload.state,
            MultipartState::Aborted,
            "After abort, state should be Aborted"
        );

        let add_result = upload.add_part(make_upload_part(1, "etag1", 1024));
        assert!(
            add_result.is_err(),
            "Adding part to aborted upload should fail"
        );
    }

    #[test]
    fn test_multipart_validate_completion() {
        let mut upload = make_multipart_upload("id1", "bucket", "key", "text/plain");

        upload.add_part(make_upload_part(1, "etag1", 1024)).unwrap();
        upload.add_part(make_upload_part(2, "etag2", 2048)).unwrap();
        upload.add_part(make_upload_part(3, "etag3", 512)).unwrap();

        let result_contiguous = upload.validate_completion(&[1, 2, 3]);
        assert!(
            result_contiguous.is_ok(),
            "Contiguous parts [1,2,3] should be valid"
        );

        let result_non_contiguous = upload.validate_completion(&[1, 3]);
        assert!(
            result_non_contiguous.is_err(),
            "Non-contiguous parts [1,3] should be invalid"
        );

        let result_empty = upload.validate_completion(&[]);
        assert!(result_empty.is_err(), "Empty parts should be invalid");

        let result_not_starting_from_1 = upload.validate_completion(&[2, 3]);
        assert!(
            result_not_starting_from_1.is_err(),
            "Parts not starting from 1 should be invalid"
        );
    }

    // =========================================================================
    // Category 4: Multipart Manager (5 tests)
    // =========================================================================

    #[test]
    fn test_manager_create_unique_ids() {
        let manager = make_multipart_manager();

        let id1 = manager.create("bucket", "key1", "text/plain");
        let id2 = manager.create("bucket", "key2", "text/plain");
        let id3 = manager.create("bucket", "key3", "text/plain");

        assert_ne!(id1, id2, "First and second upload IDs should differ");
        assert_ne!(id2, id3, "Second and third upload IDs should differ");
        assert_ne!(id1, id3, "First and third upload IDs should differ");

        assert_eq!(manager.active_count(), 3, "active_count should be 3");
    }

    #[test]
    fn test_manager_upload_and_complete() {
        let manager = make_multipart_manager();

        let upload_id = manager.create("bucket", "key/file", "text/plain");

        manager.upload_part(&upload_id, 1, b"part1data").unwrap();
        manager.upload_part(&upload_id, 2, b"part2data").unwrap();

        let (bucket, key, etag) = manager.complete(&upload_id, &[1, 2]).unwrap();

        assert_eq!(bucket, "bucket", "Returned bucket should match");
        assert_eq!(key, "key/file", "Returned key should match");
        assert!(!etag.is_empty(), "ETag should not be empty");

        assert_eq!(
            manager.active_count(),
            0,
            "active_count should be 0 after complete"
        );
    }

    #[test]
    fn test_manager_unknown_upload() {
        let manager = make_multipart_manager();

        let upload_result = manager.upload_part("nonexistent", 1, b"data");
        assert!(
            upload_result.is_err(),
            "upload_part for unknown should fail"
        );

        let complete_result = manager.complete("nonexistent", &[1]);
        assert!(complete_result.is_err(), "complete for unknown should fail");

        let abort_result = manager.abort("nonexistent");
        assert!(abort_result.is_err(), "abort for unknown should fail");
    }

    #[test]
    fn test_manager_list_uploads_filtered() {
        let manager = make_multipart_manager();

        manager.create("bucket1", "key1", "text/plain");
        manager.create("bucket1", "key2", "text/plain");
        manager.create("bucket2", "key3", "text/plain");

        let uploads_bucket1 = manager.list_uploads("bucket1");
        assert_eq!(uploads_bucket1.len(), 2, "bucket1 should have 2 uploads");

        let uploads_bucket2 = manager.list_uploads("bucket2");
        assert_eq!(uploads_bucket2.len(), 1, "bucket2 should have 1 upload");

        let uploads_bucket3 = manager.list_uploads("bucket3");
        assert_eq!(uploads_bucket3.len(), 0, "bucket3 should have 0 uploads");
    }

    #[test]
    fn test_manager_abort_prevents_complete() {
        let manager = make_multipart_manager();

        let upload_id = manager.create("bucket", "key", "text/plain");

        manager.upload_part(&upload_id, 1, b"partdata").unwrap();

        manager.abort(&upload_id).unwrap();

        let complete_result = manager.complete(&upload_id, &[1]);
        assert!(complete_result.is_err(), "Complete after abort should fail");
    }

    // =========================================================================
    // Category 5: Edge Cases & Security (5 tests)
    // =========================================================================

    #[test]
    fn test_version_list_empty() {
        let list = ObjectVersionList::new();

        assert!(
            list.is_empty(),
            "Empty list should return true for is_empty()"
        );
        assert_eq!(list.len(), 0, "Empty list should have len() == 0");
        assert!(
            list.latest().is_none(),
            "Empty list should return None for latest()"
        );
        assert!(
            !list.is_deleted(),
            "Empty list should return false for is_deleted()"
        );
    }

    #[test]
    fn test_multipart_sorted_parts() {
        let mut upload = make_multipart_upload("id1", "bucket", "key", "text/plain");

        upload.add_part(make_upload_part(3, "etag3", 100)).unwrap();
        upload.add_part(make_upload_part(1, "etag1", 100)).unwrap();
        upload.add_part(make_upload_part(2, "etag2", 100)).unwrap();

        let sorted = upload.sorted_parts();

        assert_eq!(sorted[0].part_number, 1, "First sorted part should be 1");
        assert_eq!(sorted[1].part_number, 2, "Second sorted part should be 2");
        assert_eq!(sorted[2].part_number, 3, "Third sorted part should be 3");
    }

    #[test]
    fn test_multipart_total_size() {
        let mut upload = make_multipart_upload("id1", "bucket", "key", "text/plain");

        upload.add_part(make_upload_part(1, "etag1", 1024)).unwrap();
        upload.add_part(make_upload_part(2, "etag2", 2048)).unwrap();
        upload.add_part(make_upload_part(3, "etag3", 512)).unwrap();

        assert_eq!(
            upload.total_size(),
            3584,
            "Total size should be 1024 + 2048 + 512 = 3584"
        );
    }

    #[test]
    fn test_version_registry_list_versions() {
        let mut registry = make_versioning_registry();
        registry.set_versioning("bucket", VersioningState::Enabled);

        for i in 1u32..=5 {
            let ts = 100 * i as u64;
            let size = ts;
            registry
                .put_version(
                    "bucket",
                    "key",
                    make_object_version_entry(
                        make_version_id(ts, i),
                        ts,
                        size,
                        &format!("etag{}", i),
                    ),
                )
                .unwrap();
        }

        let versions = registry.list_versions("bucket", "key");
        assert_eq!(versions.len(), 5, "Should have 5 versions");
    }

    #[test]
    fn test_multipart_replace_part() {
        let mut upload = make_multipart_upload("id1", "bucket", "key", "text/plain");

        upload.add_part(make_upload_part(1, "etag1", 1000)).unwrap();
        assert_eq!(upload.parts.len(), 1, "Should have 1 part after first add");

        upload
            .add_part(make_upload_part(1, "etag1_new", 2000))
            .unwrap();
        assert_eq!(
            upload.parts.len(),
            1,
            "Should still have 1 part after replace"
        );
        assert_eq!(
            upload.total_size(),
            2000,
            "Total size should be updated to 2000"
        );
    }
}
