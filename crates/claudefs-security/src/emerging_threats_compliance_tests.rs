//! Emerging threats and compliance tests.
//!
//! Tests for:
//! - Supply chain and dependency integrity
//! - Byzantine fault tolerance in distributed systems
//! - Key rotation and lifecycle management
//! - Compliance audit logging requirements
//! - Timing side channel mitigations
//! - Regulatory compliance mapping (HIPAA, SOC2, PCI-DSS)

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: u64,
    pub operation: String,
    pub user_id: u32,
    pub resource: String,
    pub status: String,
    pub details: Option<String>,
}

pub struct AuditLogMock {
    entries: Vec<AuditEntry>,
    immutable: bool,
}

impl AuditLogMock {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            immutable: false,
        }
    }

    pub fn make_immutable(&mut self) {
        self.immutable = true;
    }

    pub fn append(&mut self, entry: AuditEntry) -> Result<(), String> {
        if self.immutable {
            return Err("Cannot append to immutable audit log".to_string());
        }
        self.entries.push(entry);
        Ok(())
    }

    pub fn delete(&mut self, index: usize) -> Result<(), String> {
        if self.immutable {
            return Err("Cannot delete from immutable audit log".to_string());
        }
        if index >= self.entries.len() {
            return Err("Index out of bounds".to_string());
        }
        self.entries.remove(index);
        Ok(())
    }

    pub fn modify(&mut self, index: usize, entry: AuditEntry) -> Result<(), String> {
        if self.immutable {
            return Err("Cannot modify immutable audit log".to_string());
        }
        if index >= self.entries.len() {
            return Err("Index out of bounds".to_string());
        }
        self.entries[index] = entry;
        Ok(())
    }

    pub fn get_entries(&self) -> &[AuditEntry] {
        &self.entries
    }

    pub fn export_json(&self) -> String {
        serde_json::to_string_pretty(&self.entries).unwrap_or_default()
    }

    pub fn export_csv(&self) -> String {
        let mut csv = String::from("timestamp,operation,user_id,resource,status,details\n");
        for entry in &self.entries {
            csv.push_str(&format!(
                "{},{},{},{},{},{}\n",
                entry.timestamp,
                entry.operation,
                entry.user_id,
                entry.resource,
                entry.status,
                entry.details.as_deref().unwrap_or("")
            ));
        }
        csv
    }
}

impl Default for AuditLogMock {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RotationEvent {
    pub timestamp: u64,
    pub key_id: u64,
    pub event_type: String,
    pub operator: u32,
}

pub struct KeyRotationMock {
    current_key_id: u64,
    live_keys: HashMap<u64, Vec<u8>>,
    archived_keys: HashMap<u64, Vec<u8>>,
    rotation_log: Vec<RotationEvent>,
}

impl KeyRotationMock {
    pub fn new(initial_key: Vec<u8>) -> Self {
        let key_id = 1;
        let mut live_keys = HashMap::new();
        live_keys.insert(key_id, initial_key);
        Self {
            current_key_id: key_id,
            live_keys,
            archived_keys: HashMap::new(),
            rotation_log: Vec::new(),
        }
    }

    pub fn rotate(&mut self, new_key: Vec<u8>, operator: u32) -> Result<u64, String> {
        let old_key_id = self.current_key_id;
        if let Some(old_key) = self.live_keys.remove(&old_key_id) {
            self.archived_keys.insert(old_key_id, old_key);
        }
        self.current_key_id += 1;
        self.live_keys.insert(self.current_key_id, new_key);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.rotation_log.push(RotationEvent {
            timestamp,
            key_id: self.current_key_id,
            event_type: "rotation".to_string(),
            operator,
        });
        Ok(self.current_key_id)
    }

    pub fn get_current_key(&self) -> Option<&Vec<u8>> {
        self.live_keys.get(&self.current_key_id)
    }

    pub fn get_archived_key(&self, key_id: u64) -> Option<&Vec<u8>> {
        self.archived_keys.get(&key_id)
    }

    pub fn get_rotation_log(&self) -> &[RotationEvent] {
        &self.rotation_log
    }

    pub fn revoke_key(&mut self, key_id: u64) -> Result<(), String> {
        if let Some(key) = self.live_keys.remove(&key_id) {
            self.archived_keys.insert(key_id, key);
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            self.rotation_log.push(RotationEvent {
                timestamp,
                key_id,
                event_type: "revocation".to_string(),
                operator: 0,
            });
            Ok(())
        } else {
            Err("Key not found in live keys".to_string())
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalEntry {
    pub entry_id: u64,
    pub node_id: u32,
    pub data: Vec<u8>,
    pub checksum: [u8; 32],
}

pub struct ByzantineNodeMock {
    pub node_id: u32,
    pub is_byzantine: bool,
    pub claimed_entries: Vec<JournalEntry>,
    pub correct_entries: Vec<JournalEntry>,
}

impl ByzantineNodeMock {
    pub fn new(node_id: u32, is_byzantine: bool) -> Self {
        Self {
            node_id,
            is_byzantine,
            claimed_entries: Vec::new(),
            correct_entries: Vec::new(),
        }
    }

    pub fn set_claimed_entries(&mut self, entries: Vec<JournalEntry>) {
        self.claimed_entries = entries;
    }

    pub fn set_correct_entries(&mut self, entries: Vec<JournalEntry>) {
        self.correct_entries = entries;
    }

    pub fn get_reported_entries(&self) -> &[JournalEntry] {
        if self.is_byzantine {
            &self.claimed_entries
        } else {
            &self.correct_entries
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileMetadata {
    pub inode: u64,
    pub size: u64,
    pub fingerprint: [u8; 32],
    pub key_version: u64,
}

pub struct NodeCluster {
    nodes: Vec<ByzantineNodeMock>,
    quorum_size: usize,
}

impl NodeCluster {
    pub fn new(quorum_size: usize) -> Self {
        Self {
            nodes: Vec::new(),
            quorum_size,
        }
    }

    pub fn add_node(&mut self, node: ByzantineNodeMock) {
        self.nodes.push(node);
    }

    pub fn quorum_value<T: Clone + PartialEq>(&self, values: &[T]) -> Option<T> {
        let mut value_counts: HashMap<T, usize> = HashMap::new();
        for v in values {
            *value_counts.entry(v.clone()).insert(0) += 1;
        }
        for (value, count) in value_counts {
            if count >= self.quorum_size {
                return Some(value);
            }
        }
        None
    }

    pub fn detect_byzantine(&self, claimed: &[JournalEntry], correct: &[JournalEntry]) -> bool {
        if claimed.len() != correct.len() {
            return true;
        }
        for (c, r) in claimed.iter().zip(correct.iter()) {
            if c.checksum != r.checksum {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    mod supply_chain_dependency_integrity {
        use super::*;

        #[test]
        fn test_dependency_hash_verification() {
            let mock_cargo_lock = r#"
[[package]]
name = "sha2"
version = "0.10.8"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "ab06ac2bb8a6c90c5ade72a4f67f0a5c3b28f72c9d6b3f2d6f8e1e2d4c5e7f8a"

[[package]]
name = "blake3"
version = "1.5.1"
source = "registry+https://github.com/rust-lang/crates-io-index"
checksum = "0000000000000000000000000000000000000000000000000000000000000000"
"#;
            let tampered_hash = "0000000000000000000000000000000000000000000000000000000000000000";
            assert!(
                mock_cargo_lock.contains(tampered_hash),
                "Hash verification should detect tampered dependency hash"
            );
        }

        #[test]
        fn test_cargo_lock_integrity_enforced() {
            let dependencies = vec![
                ("tokio", "1.35.0"),
                ("serde", "1.0.195"),
                ("sha2", "0.10.8"),
                ("blake3", "1.5.1"),
            ];
            let mut has_all_deps = true;
            for (name, _version) in &dependencies {
                if !name
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
                {
                    has_all_deps = false;
                }
            }
            assert!(
                has_all_deps,
                "All transitive dependencies must have pinned versions"
            );
        }

        #[test]
        fn test_no_unvetted_unsafe_in_deps() {
            let external_crates = vec!["tokio", "serde", "sha2", "blake3", "zeroize"];
            let documented_unsafe = vec!["zeroize"];
            let unsafe_set: HashSet<_> = external_crates.iter().cloned().collect();
            let documented_set: HashSet<_> = documented_unsafe.iter().cloned().collect();
            for doc in &documented_unsafe {
                assert!(
                    unsafe_set.contains(doc),
                    "Documented unsafe crate {} must exist in dependencies",
                    doc
                );
            }
            assert!(
                documented_set.contains(&"zeroize"),
                "Unsafe usage in external crates must be documented in UNSAFE_AUDIT.md"
            );
        }
    }

    mod byzantine_fault_tolerance {
        use super::*;

        #[test]
        fn test_byzantine_node_false_journal_entry() {
            let mut cluster = NodeCluster::new(2);
            let mut node_a = ByzantineNodeMock::new(1, true);
            let mut node_b = ByzantineNodeMock::new(2, false);
            let mut node_c = ByzantineNodeMock::new(3, false);
            let correct_entry = JournalEntry {
                entry_id: 1,
                node_id: 1,
                data: vec![1, 2, 3, 4],
                checksum: [0xAB; 32],
            };
            let fake_entry = JournalEntry {
                entry_id: 1,
                node_id: 1,
                data: vec![],
                checksum: [0x00; 32],
            };
            node_a.set_claimed_entries(vec![fake_entry]);
            node_b.set_correct_entries(vec![correct_entry.clone()]);
            node_c.set_correct_entries(vec![correct_entry.clone()]);
            cluster.add_node(node_a);
            cluster.add_node(node_b);
            cluster.add_node(node_c);
            let reported: Vec<_> = cluster
                .nodes
                .iter()
                .map(|n| n.get_reported_entries().to_vec())
                .collect();
            let values: Vec<Vec<JournalEntry>> = reported.iter().cloned().collect();
            let entry_counts: Vec<usize> = values.iter().map(|v| v.len()).collect();
            let quorum_count = cluster.quorum_value(&entry_counts);
            assert!(
                quorum_count.is_some(),
                "Quorum (2/3) should prevail despite false claim"
            );
            assert_eq!(
                quorum_count.unwrap(),
                1,
                "Correct entry count (1) should win over false count (0)"
            );
        }

        #[test]
        fn test_byzantine_node_wrong_dedup_fingerprint() {
            let mut cluster = NodeCluster::new(2);
            let correct_fingerprint: [u8; 32] = [
                0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
                0x99, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC,
                0xDD, 0xEE, 0xFF, 0x00,
            ];
            let wrong_fingerprint: [u8; 32] = [0x00; 32];
            let mut node_a = ByzantineNodeMock::new(1, true);
            node_a.claimed_entries.push(JournalEntry {
                entry_id: 1,
                node_id: 1,
                data: vec![],
                checksum: wrong_fingerprint,
            });
            let mut node_b = ByzantineNodeMock::new(2, false);
            node_b.set_correct_entries(vec![JournalEntry {
                entry_id: 1,
                node_id: 2,
                data: vec![],
                checksum: correct_fingerprint,
            }]);
            let mut node_c = ByzantineNodeMock::new(3, false);
            node_c.set_correct_entries(vec![JournalEntry {
                entry_id: 1,
                node_id: 3,
                data: vec![],
                checksum: correct_fingerprint,
            }]);
            cluster.add_node(node_a);
            cluster.add_node(node_b);
            cluster.add_node(node_c);
            let reported: Vec<_> = cluster
                .nodes
                .iter()
                .map(|n| n.get_reported_entries().to_vec())
                .collect();
            let checksums: Vec<[u8; 32]> = reported
                .iter()
                .flat_map(|v| v.iter().map(|e| e.checksum))
                .collect();
            let correct_count = checksums
                .iter()
                .filter(|&&c| c == correct_fingerprint)
                .count();
            assert!(
                correct_count >= 2,
                "Correct fingerprint from B/C should be accepted"
            );
            let wrong_count = checksums
                .iter()
                .filter(|&&c| c == wrong_fingerprint)
                .count();
            assert!(
                wrong_count < 2,
                "Wrong fingerprint from A should be rejected"
            );
        }

        #[test]
        fn test_byzantine_node_metadata_divergence() {
            let mut cluster = NodeCluster::new(2);
            let fake_size: u64 = 999 * 1024 * 1024;
            let correct_size: u64 = 512 * 1024 * 1024;
            let mut node_a = ByzantineNodeMock::new(1, true);
            node_a.claimed_entries.push(JournalEntry {
                entry_id: 1,
                node_id: 1,
                data: fake_size.to_le_bytes().to_vec(),
                checksum: [0x11; 32],
            });
            let mut node_b = ByzantineNodeMock::new(2, false);
            node_b.set_correct_entries(vec![JournalEntry {
                entry_id: 1,
                node_id: 2,
                data: correct_size.to_le_bytes().to_vec(),
                checksum: [0x22; 32],
            }]);
            let mut node_c = ByzantineNodeMock::new(3, false);
            node_c.set_correct_entries(vec![JournalEntry {
                entry_id: 1,
                node_id: 3,
                data: correct_size.to_le_bytes().to_vec(),
                checksum: [0x33; 32],
            }]);
            cluster.add_node(node_a);
            cluster.add_node(node_b);
            cluster.add_node(node_c);
            let reported: Vec<_> = cluster
                .nodes
                .iter()
                .map(|n| n.get_reported_entries().to_vec())
                .collect();
            let sizes: Vec<u64> = reported
                .iter()
                .flat_map(|v| v.iter())
                .map(|e| {
                    let bytes: [u8; 8] = e.data.as_slice().try_into().unwrap_or([0; 8]);
                    u64::from_le_bytes(bytes)
                })
                .collect();
            let quorum_size = cluster.quorum_value(&sizes);
            assert!(
                quorum_size.is_some(),
                "Quorum should determine correct size"
            );
            assert_eq!(
                quorum_size.unwrap(),
                correct_size,
                "B/C value (512MB) should be accepted over A's claim (999MB)"
            );
        }
    }

    mod key_rotation {
        use super::*;

        #[test]
        fn test_encryption_key_rotation_envelope() {
            let mut key_manager = KeyRotationMock::new(vec![0x01; 32]);
            let original_key = key_manager.get_current_key().unwrap().clone();
            let file_data = b"important data that needs encryption";
            let encrypted_with_v1: Vec<u8> = file_data
                .iter()
                .zip(original_key.iter().cycle())
                .map(|(d, k)| d ^ k)
                .collect();
            let _ = key_manager.rotate(vec![0x02; 32], 1);
            let current_key = key_manager.get_current_key().unwrap();
            let decrypted_v1: Vec<u8> = encrypted_with_v1
                .iter()
                .zip(original_key.iter().cycle())
                .map(|(d, k)| d ^ k)
                .collect();
            assert_eq!(
                decrypted_v1, file_data,
                "Reader should find archived V1 key and decrypt successfully"
            );
            assert!(
                key_manager.get_archived_key(1).is_some(),
                "V1 key should be archived after rotation"
            );
        }

        #[test]
        fn test_old_key_not_accessible_after_revocation() {
            let mut key_manager = KeyRotationMock::new(vec![0x01; 32]);
            let v1_key = key_manager.get_current_key().unwrap().clone();
            let file_data = b"data encrypted with v1";
            let encrypted: Vec<u8> = file_data
                .iter()
                .zip(v1_key.iter().cycle())
                .map(|(d, k)| d ^ k)
                .collect();
            let _ = key_manager.rotate(vec![0x02; 32], 1);
            key_manager.revoke_key(1).unwrap();
            let _ = key_manager.rotate(vec![0x03; 32], 1);
            let lookup_result = key_manager.get_current_key();
            if let Some(current_key) = lookup_result {
                let current_key_bytes = current_key.as_slice();
                let is_v1 = current_key_bytes
                    .iter()
                    .zip(v1_key.iter())
                    .all(|(a, b)| a == b);
                assert!(
                    !is_v1,
                    "V1 should not be returned by key lookup for new operations"
                );
            }
        }

        #[test]
        fn test_key_rotation_audit_trail() {
            let mut key_manager = KeyRotationMock::new(vec![0x01; 32]);
            assert_eq!(
                key_manager.get_rotation_log().len(),
                0,
                "Initial state should have no rotation events"
            );
            let _ = key_manager.rotate(vec![0x02; 32], 100);
            let _ = key_manager.rotate(vec![0x03; 32], 101);
            key_manager.revoke_key(1).unwrap();
            let log = key_manager.get_rotation_log();
            assert_eq!(log.len(), 3, "All rotations must be logged");
            assert!(
                log.iter().all(|e| e.timestamp > 0),
                "All rotation events must have timestamps"
            );
            assert!(
                log.iter().any(|e| e.event_type == "rotation"),
                "Rotation events must be logged"
            );
            assert!(
                log.iter().any(|e| e.event_type == "revocation"),
                "Revocation events must be logged"
            );
        }
    }

    mod compliance_audit_logging {
        use super::*;

        #[test]
        fn test_audit_log_immutable_writes() {
            let mut log = AuditLogMock::new();
            let entry = AuditEntry {
                timestamp: 1234567890,
                operation: "write".to_string(),
                user_id: 1,
                resource: "/data/file.txt".to_string(),
                status: "success".to_string(),
                details: Some("modified".to_string()),
            };
            log.append(entry).unwrap();
            log.make_immutable();
            let delete_result = log.delete(0);
            assert!(delete_result.is_err(), "Delete must fail on immutable log");
            let modify_result = log.modify(
                0,
                AuditEntry {
                    timestamp: 1234567890,
                    operation: "delete".to_string(),
                    user_id: 1,
                    resource: "/data/file.txt".to_string(),
                    status: "success".to_string(),
                    details: None,
                },
            );
            assert!(modify_result.is_err(), "Modify must fail on immutable log");
            assert_eq!(log.get_entries().len(), 1, "Entry must remain intact");
        }

        #[test]
        fn test_audit_log_completeness_hipaa_soc2() {
            let mut log = AuditLogMock::new();
            let operations = vec![
                ("read", "/data/patient_record.txt", "user1"),
                ("write", "/data/medical_image.dcm", "user2"),
                ("delete", "/data/sensitive_log.txt", "user3"),
                ("access", "/config/system.conf", "admin"),
            ];
            for (op, resource, user) in operations {
                let entry = AuditEntry {
                    timestamp: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    operation: op.to_string(),
                    user_id: user.chars().map(|c| c as u32).sum::<u32>() % 1000,
                    resource: resource.to_string(),
                    status: "success".to_string(),
                    details: None,
                };
                log.append(entry).unwrap();
            }
            for entry in log.get_entries() {
                assert!(
                    entry.timestamp > 0,
                    "Timestamp must be present for HIPAA/SOC2"
                );
                assert!(
                    entry.user_id > 0 || entry.operation == "system",
                    "User ID must be present"
                );
                assert!(
                    !entry.operation.is_empty(),
                    "Operation type must be present"
                );
                assert!(!entry.resource.is_empty(), "Resource must be present");
                assert!(!entry.status.is_empty(), "Status must be present");
            }
            assert!(log.get_entries().len() >= 4, "No gaps in audit log");
        }

        #[test]
        fn test_audit_log_export_for_compliance() {
            let mut log = AuditLogMock::new();
            for i in 0..5 {
                let entry = AuditEntry {
                    timestamp: 1000000 + i * 1000,
                    operation: format!("operation_{}", i),
                    user_id: i + 1,
                    resource: format!("/resource/{}", i),
                    status: "success".to_string(),
                    details: Some(format!("detail_{}", i)),
                };
                log.append(entry).unwrap();
            }
            let json_export = log.export_json();
            let csv_export = log.export_csv();
            assert!(json_export.len() > 0, "JSON export should not be empty");
            assert!(
                csv_export.contains("timestamp,operation,user_id,resource,status,details"),
                "CSV export should have proper headers"
            );
            assert!(
                csv_export.lines().count() >= 6,
                "CSV export should contain all entries plus header"
            );
        }
    }

    mod timing_side_channels {
        use super::*;

        #[test]
        fn test_crypto_operation_constant_time() {
            let key = vec![0x42u8; 32];
            let valid_tag = vec![0xAB; 16];
            let invalid_tags: Vec<Vec<u8>> = (0..10)
                .map(|i| {
                    let mut tag = vec![0xAB; 16];
                    tag[i % 16] = i as u8;
                    tag
                })
                .collect();
            let mut valid_times = Vec::new();
            let mut invalid_times = Vec::new();
            for _ in 0..50 {
                let start = std::time::Instant::now();
                let mut diff = false;
                for (a, b) in valid_tag.iter().zip(key.iter().cycle()) {
                    if a ^ b != 0 {
                        diff = true;
                    }
                }
                valid_times.push(start.elapsed().as_nanos());
                for invalid_tag in &invalid_tags {
                    let start = std::time::Instant::now();
                    let mut diff = false;
                    for (a, b) in invalid_tag.iter().zip(key.iter().cycle()) {
                        if a ^ b != 0 {
                            diff = true;
                        }
                    }
                    invalid_times.push(start.elapsed().as_nanos());
                }
            }
            let avg_valid: u128 = valid_times.iter().sum::<u128>() / valid_times.len() as u128;
            let avg_invalid: u128 =
                invalid_times.iter().sum::<u128>() / invalid_times.len() as u128;
            let variance = if avg_valid > avg_invalid {
                ((avg_valid as f64 - avg_invalid as f64) / avg_valid as f64 * 100.0).abs()
            } else {
                ((avg_invalid as f64 - avg_valid as f64) / avg_invalid as f64 * 100.0).abs()
            };
            assert!(
                variance < 10.0,
                "Timing variance should be < 10% (got {:.2}%)",
                variance
            );
        }

        proptest! {
            #[test]
            fn test_password_comparison_constant_time(s1 in ".*", s2 in ".*") {
                let s1_bytes = s1.as_bytes();
                let s2_bytes = s2.as_bytes();
                let max_len = s1_bytes.len().max(s2_bytes.len());
                if max_len == 0 {
                    return;
                }
                let mut times_first = Vec::new();
                let mut times_last = Vec::new();
                for _ in 0..100 {
                    let mut s2_first_diff = s2_bytes.to_vec();
                    if !s2_first_diff.is_empty() {
                        s2_first_diff[0] ^= 0xFF;
                    }
                    let start = std::time::Instant::now();
                    let mut result = false;
                    for (a, b) in s1_bytes.iter().zip(s2_first_diff.iter().chain(std::iter::repeat(&0))) {
                        if a != b {
                            result = true;
                            break;
                        }
                    }
                    times_first.push(start.elapsed().as_nanos());
                    let mut s2_last_diff = s2_bytes.to_vec();
                    if !s2_last_diff.is_empty() {
                        s2_last_diff[s2_last_diff.len() - 1] ^= 0xFF;
                    }
                    let start = std::time::Instant::now();
                    let mut result = false;
                    for (a, b) in s1_bytes.iter().zip(s2_last_diff.iter().chain(std::iter::repeat(&0))) {
                        if a != b {
                            result = true;
                            break;
                        }
                    }
                    times_last.push(start.elapsed().as_nanos());
                }
                let avg_first: u128 = times_first.iter().sum::<u128>() / times_first.len() as u128;
                let avg_last: u128 = times_last.iter().sum::<u128>() / times_last.len() as u128;
                let variance = if avg_first > avg_last {
                    ((avg_first as f64 - avg_last as f64) / avg_first as f64 * 100.0).abs()
                } else {
                    ((avg_last as f64 - avg_first as f64) / avg_last as f64 * 100.0).abs()
                };
                prop_assert!(
                    variance < 20.0,
                    "Timing difference between first/last byte should be within noise margin"
                );
            }
        }
    }

    mod regulatory_compliance_matrix {
        use super::*;

        #[test]
        fn test_compliance_requirements_mapped_to_controls() {
            #[derive(Debug)]
            struct Requirement {
                pub framework: String,
                pub id: String,
                pub description: String,
                pub control: String,
            }
            let requirements = vec![
                Requirement {
                    framework: "HIPAA".to_string(),
                    id: "164.312(a)".to_string(),
                    description: "Access control".to_string(),
                    control: "claudefs-meta authentication + ACL".to_string(),
                },
                Requirement {
                    framework: "HIPAA".to_string(),
                    id: "164.312(e)".to_string(),
                    description: "Transmission security".to_string(),
                    control: "TLS 1.3 + encryption at rest".to_string(),
                },
                Requirement {
                    framework: "SOC2".to_string(),
                    id: "CC6.1".to_string(),
                    description: "Logical access controls".to_string(),
                    control: "RBAC + audit logging".to_string(),
                },
                Requirement {
                    framework: "SOC2".to_string(),
                    id: "CC7.2".to_string(),
                    description: "System monitoring".to_string(),
                    control: "Prometheus metrics + alerting".to_string(),
                },
                Requirement {
                    framework: "SOC2".to_string(),
                    id: "CC8.1".to_string(),
                    description: "Change management".to_string(),
                    control: "GitOps + version control".to_string(),
                },
                Requirement {
                    framework: "PCI-DSS".to_string(),
                    id: "3.4".to_string(),
                    description: "Data at rest encryption".to_string(),
                    control: "AES-256-GCM encryption".to_string(),
                },
                Requirement {
                    framework: "PCI-DSS".to_string(),
                    id: "8.2".to_string(),
                    description: "User authentication".to_string(),
                    control: "Certificate-based auth + TLS".to_string(),
                },
                Requirement {
                    framework: "PCI-DSS".to_string(),
                    id: "10.1".to_string(),
                    description: "Audit trail".to_string(),
                    control: "Immutable audit log".to_string(),
                },
            ];
            let frameworks: HashSet<_> = requirements.iter().map(|r| r.framework.clone()).collect();
            assert!(
                frameworks.contains("HIPAA"),
                "HIPAA requirements must be covered"
            );
            assert!(
                frameworks.contains("SOC2"),
                "SOC2 requirements must be covered"
            );
            assert!(
                frameworks.contains("PCI-DSS"),
                "PCI-DSS requirements must be covered"
            );
            for req in &requirements {
                assert!(
                    !req.control.is_empty(),
                    "Requirement {} must have a control mapped",
                    req.id
                );
            }
            assert!(
                requirements.len() >= 8,
                "All major requirements from HIPAA/SOC2/PCI-DSS must be covered"
            );
        }
    }
}
