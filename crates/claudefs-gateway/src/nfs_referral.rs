//! NFSv4.1 referrals and migrations for multi-namespace federation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use tracing::debug;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferralTarget {
    pub server: String,
    pub port: u16,
    pub export_path: String,
}

impl ReferralTarget {
    pub fn new(server: String, port: u16, export_path: String) -> Self {
        ReferralTarget {
            server,
            port,
            export_path,
        }
    }

    pub fn validate(&self) -> Result<(), ReferralError> {
        if self.server.is_empty() {
            return Err(ReferralError::InvalidTarget(
                "server cannot be empty".to_string(),
            ));
        }
        if self.server.contains('/') {
            return Err(ReferralError::InvalidTarget(format!(
                "server '{}' contains invalid character '/'",
                self.server
            )));
        }
        if self.port == 0 {
            return Err(ReferralError::InvalidTarget(format!(
                "port {} is invalid",
                self.port
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReferralType {
    #[default]
    Referral,
    Migration,
    Replication,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferralEntry {
    pub local_path: String,
    pub targets: Vec<ReferralTarget>,
    pub enabled: bool,
    pub referral_type: ReferralType,
}

impl ReferralEntry {
    pub fn new(
        local_path: String,
        targets: Vec<ReferralTarget>,
        referral_type: ReferralType,
    ) -> Self {
        ReferralEntry {
            local_path,
            targets,
            enabled: true,
            referral_type,
        }
    }

    pub fn validate(&self) -> Result<(), ReferralError> {
        if !self.local_path.starts_with('/') {
            return Err(ReferralError::InvalidPath(format!(
                "path '{}' must be absolute",
                self.local_path
            )));
        }
        if self.local_path.contains("//") {
            return Err(ReferralError::InvalidPath(format!(
                "path '{}' contains empty component",
                self.local_path
            )));
        }
        if self.targets.is_empty() {
            return Err(ReferralError::EmptyTargets);
        }
        for target in &self.targets {
            target.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ReferralError {
    #[error("referral already exists for path: {0}")]
    DuplicatePath(String),
    #[error("invalid path: {0}")]
    InvalidPath(String),
    #[error("referral must have at least one target")]
    EmptyTargets,
    #[error("invalid target: {0}")]
    InvalidTarget(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsServer {
    pub server: String,
    pub port: u16,
}

impl FsServer {
    pub fn new(server: String, port: u16) -> Self {
        FsServer { server, port }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsLocation {
    pub servers: Vec<FsServer>,
    pub rootpath: Vec<String>,
}

impl FsLocation {
    pub fn new(servers: Vec<FsServer>, rootpath: Vec<String>) -> Self {
        FsLocation { servers, rootpath }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsLocations {
    pub root: Vec<String>,
    pub locations: Vec<FsLocation>,
}

impl FsLocations {
    pub fn new(root: Vec<String>, locations: Vec<FsLocation>) -> Self {
        FsLocations { root, locations }
    }
}

pub struct ReferralSerializer;

impl ReferralSerializer {
    pub fn new() -> Self {
        ReferralSerializer
    }

    pub fn to_fs_locations(&self, entry: &ReferralEntry) -> FsLocations {
        let root: Vec<String> = entry
            .local_path
            .split('/')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();

        let locations: Vec<FsLocation> = entry
            .targets
            .iter()
            .map(|target| {
                let servers = vec![FsServer::new(target.server.clone(), target.port)];
                let rootpath: Vec<String> = target
                    .export_path
                    .split('/')
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect();
                FsLocation::new(servers, rootpath)
            })
            .collect();

        FsLocations::new(root, locations)
    }
}

impl Default for ReferralSerializer {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ReferralDatabase {
    entries: HashMap<String, ReferralEntry>,
}

impl ReferralDatabase {
    pub fn new() -> Self {
        ReferralDatabase {
            entries: HashMap::new(),
        }
    }

    pub fn add_referral(&mut self, entry: ReferralEntry) -> Result<(), ReferralError> {
        entry.validate()?;

        if self.entries.contains_key(&entry.local_path) {
            return Err(ReferralError::DuplicatePath(entry.local_path));
        }

        self.entries.insert(entry.local_path.clone(), entry);
        Ok(())
    }

    pub fn remove_referral(&mut self, path: &str) -> bool {
        self.entries.remove(path).is_some()
    }

    pub fn lookup(&self, path: &str) -> Option<&ReferralEntry> {
        debug!("Looking up referral for path: {}", path);
        self.entries.get(path)
    }

    pub fn lookup_by_prefix(&self, path: &str) -> Option<&ReferralEntry> {
        debug!("Looking up referral by prefix for path: {}", path);

        let mut best_match: Option<(&String, &ReferralEntry)> = None;

        for (key, entry) in &self.entries {
            if path.starts_with(key) || key.as_str() == path {
                match &best_match {
                    None => {
                        best_match = Some((key, entry));
                    }
                    Some((best_key, _)) => {
                        if key.len() > best_key.len() {
                            best_match = Some((key, entry));
                        }
                    }
                }
            }
        }

        best_match.map(|(_, entry)| entry)
    }

    pub fn list_referrals(&self) -> Vec<&ReferralEntry> {
        self.entries.values().collect()
    }

    pub fn enable_referral(&mut self, path: &str) -> bool {
        if let Some(entry) = self.entries.get_mut(path) {
            entry.enabled = true;
            true
        } else {
            false
        }
    }

    pub fn disable_referral(&mut self, path: &str) -> bool {
        if let Some(entry) = self.entries.get_mut(path) {
            entry.enabled = false;
            true
        } else {
            false
        }
    }

    pub fn referral_count(&self) -> usize {
        self.entries.len()
    }
}

impl Default for ReferralDatabase {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_referral_target_validation_valid() {
        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        assert!(target.validate().is_ok());
    }

    #[test]
    fn test_referral_target_validation_empty_server() {
        let target = ReferralTarget::new("".to_string(), 2049, "/exports/data".to_string());
        assert!(matches!(
            target.validate(),
            Err(ReferralError::InvalidTarget(_))
        ));
    }

    #[test]
    fn test_referral_target_validation_invalid_port() {
        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            0,
            "/exports/data".to_string(),
        );
        assert!(matches!(
            target.validate(),
            Err(ReferralError::InvalidTarget(_))
        ));
    }

    #[test]
    fn test_referral_entry_validation_valid() {
        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let entry = ReferralEntry::new("/data".to_string(), vec![target], ReferralType::Referral);
        assert!(entry.validate().is_ok());
    }

    #[test]
    fn test_referral_entry_validation_invalid_path_not_absolute() {
        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let entry = ReferralEntry::new("data".to_string(), vec![target], ReferralType::Referral);
        assert!(matches!(
            entry.validate(),
            Err(ReferralError::InvalidPath(_))
        ));
    }

    #[test]
    fn test_referral_entry_validation_empty_targets() {
        let entry = ReferralEntry::new("/data".to_string(), vec![], ReferralType::Referral);
        assert!(matches!(entry.validate(), Err(ReferralError::EmptyTargets)));
    }

    #[test]
    fn test_referral_entry_validation_double_slash() {
        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let entry = ReferralEntry::new(
            "/data//subdir".to_string(),
            vec![target],
            ReferralType::Referral,
        );
        assert!(matches!(
            entry.validate(),
            Err(ReferralError::InvalidPath(_))
        ));
    }

    #[test]
    fn test_add_referral_success() {
        let mut db = ReferralDatabase::new();
        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let entry = ReferralEntry::new("/data".to_string(), vec![target], ReferralType::Referral);
        assert!(db.add_referral(entry).is_ok());
        assert_eq!(db.referral_count(), 1);
    }

    #[test]
    fn test_add_referral_duplicate() {
        let mut db = ReferralDatabase::new();
        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let entry = ReferralEntry::new("/data".to_string(), vec![target], ReferralType::Referral);
        db.add_referral(entry.clone()).unwrap();
        let result = db.add_referral(entry);
        assert!(matches!(result, Err(ReferralError::DuplicatePath(_))));
    }

    #[test]
    fn test_remove_referral_exists() {
        let mut db = ReferralDatabase::new();
        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let entry = ReferralEntry::new("/data".to_string(), vec![target], ReferralType::Referral);
        db.add_referral(entry).unwrap();
        assert!(db.remove_referral("/data"));
        assert_eq!(db.referral_count(), 0);
    }

    #[test]
    fn test_remove_referral_not_exists() {
        let mut db = ReferralDatabase::new();
        assert!(!db.remove_referral("/nonexistent"));
    }

    #[test]
    fn test_lookup_exact_match() {
        let mut db = ReferralDatabase::new();
        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let entry = ReferralEntry::new("/data".to_string(), vec![target], ReferralType::Referral);
        db.add_referral(entry).unwrap();

        let result = db.lookup("/data");
        assert!(result.is_some());
        assert_eq!(result.unwrap().local_path, "/data");
    }

    #[test]
    fn test_lookup_not_found() {
        let db = ReferralDatabase::new();
        assert!(db.lookup("/nonexistent").is_none());
    }

    #[test]
    fn test_lookup_by_prefix_exact_match() {
        let mut db = ReferralDatabase::new();
        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let entry = ReferralEntry::new("/data".to_string(), vec![target], ReferralType::Referral);
        db.add_referral(entry).unwrap();

        let result = db.lookup_by_prefix("/data");
        assert!(result.is_some());
        assert_eq!(result.unwrap().local_path, "/data");
    }

    #[test]
    fn test_lookup_by_prefix_longest_match() {
        let mut db = ReferralDatabase::new();

        let target1 = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let entry1 = ReferralEntry::new("/data".to_string(), vec![target1], ReferralType::Referral);
        db.add_referral(entry1).unwrap();

        let target2 = ReferralTarget::new(
            "server2.example.com".to_string(),
            2049,
            "/exports/data/sub".to_string(),
        );
        let entry2 = ReferralEntry::new(
            "/data/sub".to_string(),
            vec![target2],
            ReferralType::Migration,
        );
        db.add_referral(entry2).unwrap();

        let result = db.lookup_by_prefix("/data/sub/file.txt");
        assert!(result.is_some());
        assert_eq!(result.unwrap().local_path, "/data/sub");
    }

    #[test]
    fn test_lookup_by_prefix_nested_paths() {
        let mut db = ReferralDatabase::new();

        let target1 = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports".to_string(),
        );
        let entry1 = ReferralEntry::new("/".to_string(), vec![target1], ReferralType::Referral);
        db.add_referral(entry1).unwrap();

        let target2 = ReferralTarget::new(
            "server2.example.com".to_string(),
            2049,
            "/exports/project1".to_string(),
        );
        let entry2 = ReferralEntry::new(
            "/project1".to_string(),
            vec![target2],
            ReferralType::Migration,
        );
        db.add_referral(entry2).unwrap();

        let target3 = ReferralTarget::new(
            "server3.example.com".to_string(),
            2049,
            "/exports/project1/docs".to_string(),
        );
        let entry3 = ReferralEntry::new(
            "/project1/docs".to_string(),
            vec![target3],
            ReferralType::Replication,
        );
        db.add_referral(entry3).unwrap();

        let result = db.lookup_by_prefix("/project1/docs/readme.txt");
        assert!(result.is_some());
        assert_eq!(result.unwrap().local_path, "/project1/docs");
    }

    #[test]
    fn test_list_referrals() {
        let mut db = ReferralDatabase::new();

        let target1 = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let entry1 = ReferralEntry::new("/data".to_string(), vec![target1], ReferralType::Referral);
        db.add_referral(entry1).unwrap();

        let target2 = ReferralTarget::new(
            "server2.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let entry2 =
            ReferralEntry::new("/data2".to_string(), vec![target2], ReferralType::Migration);
        db.add_referral(entry2).unwrap();

        let list = db.list_referrals();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_enable_referral() {
        let mut db = ReferralDatabase::new();
        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let mut entry =
            ReferralEntry::new("/data".to_string(), vec![target], ReferralType::Referral);
        entry.enabled = false;
        db.add_referral(entry).unwrap();

        assert!(db.enable_referral("/data"));
        assert!(db.lookup("/data").unwrap().enabled);
    }

    #[test]
    fn test_enable_referral_not_exists() {
        let mut db = ReferralDatabase::new();
        assert!(!db.enable_referral("/nonexistent"));
    }

    #[test]
    fn test_disable_referral() {
        let mut db = ReferralDatabase::new();
        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let entry = ReferralEntry::new("/data".to_string(), vec![target], ReferralType::Referral);
        db.add_referral(entry).unwrap();

        assert!(db.disable_referral("/data"));
        assert!(!db.lookup("/data").unwrap().enabled);
    }

    #[test]
    fn test_disable_referral_not_exists() {
        let mut db = ReferralDatabase::new();
        assert!(!db.disable_referral("/nonexistent"));
    }

    #[test]
    fn test_referral_type_default() {
        let entry: ReferralEntry = ReferralEntry::new(
            "/data".to_string(),
            vec![ReferralTarget::new(
                "server1.example.com".to_string(),
                2049,
                "/exports".to_string(),
            )],
            ReferralType::default(),
        );
        assert_eq!(entry.referral_type, ReferralType::Referral);
    }

    #[test]
    fn test_to_fs_locations_conversion() {
        let serializer = ReferralSerializer::new();
        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let entry = ReferralEntry::new("/data".to_string(), vec![target], ReferralType::Referral);

        let fs_locations = serializer.to_fs_locations(&entry);

        assert_eq!(fs_locations.root, vec!["data"]);
        assert_eq!(fs_locations.locations.len(), 1);
        assert_eq!(fs_locations.locations[0].servers.len(), 1);
        assert_eq!(
            fs_locations.locations[0].servers[0].server,
            "server1.example.com"
        );
        assert_eq!(fs_locations.locations[0].servers[0].port, 2049);
        assert_eq!(fs_locations.locations[0].rootpath, vec!["exports", "data"]);
    }

    #[test]
    fn test_to_fs_locations_multiple_targets() {
        let serializer = ReferralSerializer::new();
        let target1 = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let target2 = ReferralTarget::new(
            "server2.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let entry = ReferralEntry::new(
            "/data".to_string(),
            vec![target1, target2],
            ReferralType::Replication,
        );

        let fs_locations = serializer.to_fs_locations(&entry);

        assert_eq!(fs_locations.locations.len(), 2);
        assert_eq!(
            fs_locations.locations[0].servers[0].server,
            "server1.example.com"
        );
        assert_eq!(
            fs_locations.locations[1].servers[0].server,
            "server2.example.com"
        );
    }

    #[test]
    fn test_to_fs_locations_nested_path() {
        let serializer = ReferralSerializer::new();
        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports".to_string(),
        );
        let entry = ReferralEntry::new(
            "/project/data/subdir".to_string(),
            vec![target],
            ReferralType::Migration,
        );

        let fs_locations = serializer.to_fs_locations(&entry);

        assert_eq!(fs_locations.root, vec!["project", "data", "subdir"]);
    }

    #[test]
    fn test_multiple_referrals_different_paths() {
        let mut db = ReferralDatabase::new();

        let target1 = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let entry1 = ReferralEntry::new("/data".to_string(), vec![target1], ReferralType::Referral);
        db.add_referral(entry1).unwrap();

        let target2 = ReferralTarget::new(
            "server2.example.com".to_string(),
            2049,
            "/exports/data2".to_string(),
        );
        let entry2 =
            ReferralEntry::new("/data2".to_string(), vec![target2], ReferralType::Referral);
        db.add_referral(entry2).unwrap();

        assert_eq!(db.referral_count(), 2);
        assert!(db.lookup("/data").is_some());
        assert!(db.lookup("/data2").is_some());
    }

    #[test]
    fn test_lookup_returns_disabled_referral() {
        let mut db = ReferralDatabase::new();
        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports/data".to_string(),
        );
        let entry = ReferralEntry::new("/data".to_string(), vec![target], ReferralType::Referral);
        db.add_referral(entry).unwrap();
        assert!(db.disable_referral("/data"));

        let result = db.lookup("/data");
        assert!(result.is_some());
        assert!(!result.unwrap().enabled);
    }

    #[test]
    fn test_lookup_by_prefix_with_disabled_entry() {
        let mut db = ReferralDatabase::new();

        let target1 = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports".to_string(),
        );
        let entry1 = ReferralEntry::new("/data".to_string(), vec![target1], ReferralType::Referral);
        db.add_referral(entry1).unwrap();

        let target2 = ReferralTarget::new(
            "server2.example.com".to_string(),
            2049,
            "/exports/data/sub".to_string(),
        );
        let entry2 = ReferralEntry::new(
            "/data/sub".to_string(),
            vec![target2],
            ReferralType::Migration,
        );
        db.add_referral(entry2).unwrap();

        assert!(db.disable_referral("/data/sub"));

        let result = db.lookup_by_prefix("/data/sub/file.txt");
        assert!(result.is_some());
        assert_eq!(result.unwrap().local_path, "/data/sub");
        assert!(!result.unwrap().enabled);
    }

    #[test]
    fn test_empty_database_operations() {
        let db = ReferralDatabase::new();

        assert_eq!(db.referral_count(), 0);
        assert!(db.lookup("/any").is_none());
        assert!(db.lookup_by_prefix("/any").is_none());
        assert!(db.list_referrals().is_empty());
    }

    #[test]
    fn test_root_path_referral() {
        let mut db = ReferralDatabase::new();
        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports".to_string(),
        );
        let entry = ReferralEntry::new("/".to_string(), vec![target], ReferralType::Referral);

        assert!(db.add_referral(entry).is_ok());

        let result = db.lookup("/");
        assert!(result.is_some());
    }

    #[test]
    fn test_lookup_by_prefix_root_match() {
        let mut db = ReferralDatabase::new();
        let target = ReferralTarget::new(
            "server1.example.com".to_string(),
            2049,
            "/exports".to_string(),
        );
        let entry = ReferralEntry::new("/".to_string(), vec![target], ReferralType::Referral);
        db.add_referral(entry).unwrap();

        let result = db.lookup_by_prefix("/some/deep/path");
        assert!(result.is_some());
        assert_eq!(result.unwrap().local_path, "/");
    }
}
