//! NFSv3 export configuration with security options

use crate::auth::SquashPolicy;

/// Access mode for an NFS export.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ExportAccess {
    /// Only allow read operations
    #[default]
    ReadOnly,
    /// Allow both read and write operations
    ReadWrite,
}

/// Client specification for export access control.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientSpec {
    /// CIDR notation (e.g., "192.168.1.0/24" or "*" for any)
    pub cidr: String,
}

impl ClientSpec {
    /// Creates a ClientSpec that matches any IP address.
    pub fn any() -> Self {
        Self {
            cidr: "*".to_string(),
        }
    }
    /// Creates a ClientSpec from a CIDR string.
    pub fn from_cidr(cidr: &str) -> Self {
        Self {
            cidr: cidr.to_string(),
        }
    }
    /// Checks if the given IP address is allowed by this client spec.
    pub fn allows(&self, ip: &str) -> bool {
        if self.cidr == "*" {
            return true;
        }
        self.cidr == ip || self.cidr.starts_with(&format!("{}/", ip))
    }
}

/// NFS export configuration.
#[derive(Debug, Clone)]
pub struct ExportConfig {
    /// Actual filesystem path being exported
    pub path: String,
    /// Export path as seen by NFS clients
    pub export_path: String,
    /// List of allowed clients
    pub clients: Vec<ClientSpec>,
    /// Access mode (read-only or read-write)
    pub access: ExportAccess,
    /// Root squash policy
    pub squash: SquashPolicy,
    /// UID to map remote root to (nobody)
    pub squash_uid: u32,
    /// GID to map remote root to (nobody)
    pub squash_gid: u32,
    /// Whether this export is hidden from clients
    pub hidden: bool,
}

impl ExportConfig {
    /// Creates a new ExportConfig with the given paths.
    pub fn new(path: &str, export_path: &str) -> Self {
        Self {
            path: path.to_string(),
            export_path: export_path.to_string(),
            clients: vec![],
            access: ExportAccess::default(),
            squash: SquashPolicy::default(),
            squash_uid: 65534,
            squash_gid: 65534,
            hidden: false,
        }
    }
    pub fn with_client(mut self, spec: ClientSpec) -> Self {
        self.clients.push(spec);
        self
    }
    pub fn with_access(mut self, access: ExportAccess) -> Self {
        self.access = access;
        self
    }
    pub fn with_squash(mut self, squash: SquashPolicy) -> Self {
        self.squash = squash;
        self
    }
    pub fn read_write(mut self) -> Self {
        self.access = ExportAccess::ReadWrite;
        self
    }
    pub fn no_squash(mut self) -> Self {
        self.squash = SquashPolicy::None;
        self
    }
    pub fn allows_client(&self, ip: &str) -> bool {
        if self.clients.is_empty() {
            return false;
        }
        self.clients.iter().any(|c| c.allows(ip))
    }
    pub fn is_read_only(&self) -> bool {
        self.access == ExportAccess::ReadOnly
    }
    pub fn is_read_write(&self) -> bool {
        self.access == ExportAccess::ReadWrite
    }
}

pub struct ExportRegistry {
    exports: Vec<ExportConfig>,
}

impl ExportRegistry {
    pub fn new() -> Self {
        Self { exports: vec![] }
    }
    pub fn add(&mut self, export: ExportConfig) {
        self.exports.push(export);
    }
    pub fn find(&self, export_path: &str) -> Option<&ExportConfig> {
        self.exports.iter().find(|e| e.export_path == export_path)
    }
    pub fn list_visible(&self) -> Vec<&ExportConfig> {
        self.exports.iter().filter(|e| !e.hidden).collect()
    }
    pub fn count(&self) -> usize {
        self.exports.len()
    }
    pub fn remove(&mut self, export_path: &str) -> bool {
        let before = self.exports.len();
        self.exports.retain(|e| e.export_path != export_path);
        self.exports.len() < before
    }
}

impl Default for ExportRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_export_access_default_is_read_only() {
        assert_eq!(ExportAccess::default(), ExportAccess::ReadOnly);
    }

    #[test]
    fn test_client_spec_any_allows_asterisk() {
        let spec = ClientSpec::any();
        assert_eq!(spec.cidr, "*");
    }

    #[test]
    fn test_client_spec_any_allows_any_ip() {
        let spec = ClientSpec::any();
        assert!(spec.allows("1.2.3.4"));
    }

    #[test]
    fn test_client_spec_from_cidr_exact_match() {
        let spec = ClientSpec::from_cidr("192.168.1.5");
        assert!(spec.allows("192.168.1.5"));
    }

    #[test]
    fn test_client_spec_from_cidr_no_match_different_ip() {
        let spec = ClientSpec::from_cidr("192.168.1.5");
        assert!(!spec.allows("10.0.0.1"));
    }

    #[test]
    fn test_export_config_new_has_default_squash_policy() {
        let export = ExportConfig::new("/data/project1", "/project1");
        assert_eq!(export.squash, SquashPolicy::RootSquash);
    }

    #[test]
    fn test_export_config_new_default_access_is_read_only() {
        let export = ExportConfig::new("/data/project1", "/project1");
        assert!(export.is_read_only());
    }

    #[test]
    fn test_export_config_new_empty_clients_allows_no_client() {
        let export = ExportConfig::new("/data/project1", "/project1");
        assert!(!export.allows_client("any"));
    }

    #[test]
    fn test_export_config_with_client_allows_any() {
        let export =
            ExportConfig::new("/data/project1", "/project1").with_client(ClientSpec::any());
        assert!(export.allows_client("any"));
    }

    #[test]
    fn test_export_config_with_access_read_write() {
        let export =
            ExportConfig::new("/data/project1", "/project1").with_access(ExportAccess::ReadWrite);
        assert!(export.is_read_write());
    }

    #[test]
    fn test_export_config_read_write_convenience() {
        let export = ExportConfig::new("/data/project1", "/project1").read_write();
        assert!(export.is_read_write());
    }

    #[test]
    fn test_export_config_no_squash_sets_policy() {
        let export = ExportConfig::new("/data/project1", "/project1").no_squash();
        assert_eq!(export.squash, SquashPolicy::None);
    }

    #[test]
    fn test_export_config_with_squash_all_squash() {
        let export =
            ExportConfig::new("/data/project1", "/project1").with_squash(SquashPolicy::AllSquash);
        assert_eq!(export.squash, SquashPolicy::AllSquash);
    }

    #[test]
    fn test_export_config_multiple_clients_both_allowed() {
        let export = ExportConfig::new("/data/project1", "/project1")
            .with_client(ClientSpec::from_cidr("192.168.1.5"))
            .with_client(ClientSpec::from_cidr("10.0.0.5"));
        assert!(export.allows_client("192.168.1.5"));
        assert!(export.allows_client("10.0.0.5"));
    }

    #[test]
    fn test_export_registry_new_starts_empty() {
        let registry = ExportRegistry::new();
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn test_export_registry_add_increases_count() {
        let mut registry = ExportRegistry::new();
        registry.add(ExportConfig::new("/data/p1", "/p1"));
        assert_eq!(registry.count(), 1);
    }

    #[test]
    fn test_export_registry_find_returns_some() {
        let mut registry = ExportRegistry::new();
        registry.add(ExportConfig::new("/data/p1", "/p1"));
        let found = registry.find("/p1");
        assert!(found.is_some());
        assert_eq!(found.unwrap().export_path, "/p1");
    }

    #[test]
    fn test_export_registry_find_returns_none_unknown() {
        let registry = ExportRegistry::new();
        let found = registry.find("/nonexistent");
        assert!(found.is_none());
    }

    #[test]
    fn test_export_registry_list_visible_returns_non_hidden() {
        let mut registry = ExportRegistry::new();
        registry.add(ExportConfig::new("/data/p1", "/p1"));
        registry.add(ExportConfig::new("/data/p2", "/p2").with_client(ClientSpec::any()));
        let visible = registry.list_visible();
        assert_eq!(visible.len(), 2);
    }

    #[test]
    fn test_export_registry_hidden_export_not_in_list_visible() {
        let mut registry = ExportRegistry::new();
        let mut export = ExportConfig::new("/data/p1", "/p1");
        export.hidden = true;
        registry.add(export);
        let visible = registry.list_visible();
        assert!(visible.is_empty());
    }

    #[test]
    fn test_export_registry_remove_returns_true_known_path() {
        let mut registry = ExportRegistry::new();
        registry.add(ExportConfig::new("/data/p1", "/p1"));
        let removed = registry.remove("/p1");
        assert!(removed);
    }

    #[test]
    fn test_export_registry_remove_returns_false_unknown_path() {
        let mut registry = ExportRegistry::new();
        registry.add(ExportConfig::new("/data/p1", "/p1"));
        let removed = registry.remove("/nonexistent");
        assert!(!removed);
    }

    #[test]
    fn test_export_registry_remove_decreases_count() {
        let mut registry = ExportRegistry::new();
        registry.add(ExportConfig::new("/data/p1", "/p1"));
        registry.add(ExportConfig::new("/data/p2", "/p2"));
        registry.remove("/p1");
        assert_eq!(registry.count(), 1);
    }

    #[test]
    fn test_export_config_squash_uid_default() {
        let export = ExportConfig::new("/data/p1", "/p1");
        assert_eq!(export.squash_uid, 65534);
    }

    #[test]
    fn test_export_config_squash_gid_default() {
        let export = ExportConfig::new("/data/p1", "/p1");
        assert_eq!(export.squash_gid, 65534);
    }

    #[test]
    fn test_export_config_is_read_only_default() {
        let export = ExportConfig::new("/data/p1", "/p1");
        assert!(export.is_read_only());
    }

    #[test]
    fn test_export_path_different_from_local_path() {
        let export = ExportConfig::new("/mnt/data/project1", "/project1");
        assert_eq!(export.path, "/mnt/data/project1");
        assert_eq!(export.export_path, "/project1");
    }
}
