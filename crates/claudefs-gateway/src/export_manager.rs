//! Dynamic NFS export management

use crate::config::ExportConfig;
use crate::protocol::FileHandle3;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

/// Export status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportStatus {
    /// Export is active and serving clients
    Active,
    /// Export is being removed (waiting for clients to disconnect)
    Draining,
    /// Export is disabled
    Disabled,
}

/// An active export with runtime state
#[derive(Debug, Clone)]
pub struct ActiveExport {
    pub config: ExportConfig,
    pub status: ExportStatus,
    pub client_count: u32,
    pub root_fh: FileHandle3,
    pub root_inode: u64,
}

impl ActiveExport {
    pub fn new(config: ExportConfig, root_fh: FileHandle3, root_inode: u64) -> Self {
        Self {
            config,
            status: ExportStatus::Active,
            client_count: 0,
            root_fh,
            root_inode,
        }
    }

    pub fn is_active(&self) -> bool {
        self.status == ExportStatus::Active
    }

    pub fn can_remove(&self) -> bool {
        self.status == ExportStatus::Draining && self.client_count == 0
    }
}

/// Dynamic NFS export manager
pub struct ExportManager {
    exports: RwLock<HashMap<String, ActiveExport>>,
    next_inode: AtomicU64,
}

impl ExportManager {
    pub fn new() -> Self {
        Self {
            exports: RwLock::new(HashMap::new()),
            next_inode: AtomicU64::new(1),
        }
    }

    pub fn add_export(
        &self,
        config: ExportConfig,
        root_inode: u64,
    ) -> crate::error::Result<FileHandle3> {
        let path = config.path.clone();

        let exports =
            self.exports
                .read()
                .map_err(|_| crate::error::GatewayError::ProtocolError {
                    reason: "failed to acquire read lock".to_string(),
                })?;

        if exports.contains_key(&path) {
            return Err(crate::error::GatewayError::ProtocolError {
                reason: format!("export already exists: {}", path),
            });
        }
        drop(exports);

        let root_fh = FileHandle3::from_inode(root_inode);
        let active = ActiveExport::new(config, root_fh.clone(), root_inode);

        let mut exports =
            self.exports
                .write()
                .map_err(|_| crate::error::GatewayError::ProtocolError {
                    reason: "failed to acquire write lock".to_string(),
                })?;

        exports.insert(path, active);

        Ok(root_fh)
    }

    pub fn remove_export(&self, path: &str) -> bool {
        let mut exports = match self.exports.write() {
            Ok(e) => e,
            Err(_) => return false,
        };

        if let Some(export) = exports.get_mut(path) {
            if export.client_count > 0 {
                export.status = ExportStatus::Draining;
                true
            } else {
                exports.remove(path);
                true
            }
        } else {
            false
        }
    }

    pub fn force_remove_export(&self, path: &str) -> bool {
        let mut exports = match self.exports.write() {
            Ok(e) => e,
            Err(_) => return false,
        };

        if exports.contains_key(path) {
            exports.remove(path);
            true
        } else {
            false
        }
    }

    pub fn get_export(&self, path: &str) -> Option<ActiveExport> {
        let exports = match self.exports.read() {
            Ok(e) => e,
            Err(_) => return None,
        };

        exports.get(path).cloned()
    }

    pub fn list_exports(&self) -> Vec<ActiveExport> {
        let exports = match self.exports.read() {
            Ok(e) => e,
            Err(_) => return vec![],
        };

        exports.values().cloned().collect()
    }

    pub fn export_paths(&self) -> Vec<String> {
        let exports = match self.exports.read() {
            Ok(e) => e,
            Err(_) => return vec![],
        };

        exports.keys().cloned().collect()
    }

    pub fn is_exported(&self, path: &str) -> bool {
        let exports = match self.exports.read() {
            Ok(e) => e,
            Err(_) => return false,
        };

        exports.get(path).map(|e| e.is_active()).unwrap_or(false)
    }

    pub fn increment_clients(&self, path: &str) -> bool {
        let mut exports = match self.exports.write() {
            Ok(e) => e,
            Err(_) => return false,
        };

        if let Some(export) = exports.get_mut(path) {
            export.client_count += 1;
            true
        } else {
            false
        }
    }

    pub fn decrement_clients(&self, path: &str) -> bool {
        let mut exports = match self.exports.write() {
            Ok(e) => e,
            Err(_) => return false,
        };

        if let Some(export) = exports.get_mut(path) {
            if export.client_count > 0 {
                export.client_count -= 1;
            }
            true
        } else {
            false
        }
    }

    pub fn root_fh(&self, path: &str) -> Option<FileHandle3> {
        let exports = match self.exports.read() {
            Ok(e) => e,
            Err(_) => return None,
        };

        exports.get(path).map(|e| e.root_fh.clone())
    }

    pub fn reload(&self, configs: Vec<ExportConfig>) {
        let current_paths: Vec<String> = {
            let exports = match self.exports.read() {
                Ok(e) => e,
                Err(_) => return,
            };
            exports.keys().cloned().collect()
        };

        let new_paths: Vec<String> = configs.iter().map(|c| c.path.clone()).collect();

        for path in &current_paths {
            if !new_paths.contains(path) {
                self.remove_export(path);
            }
        }

        for config in configs {
            let path = config.path.clone();
            let exists = {
                if let Ok(exports) = self.exports.read() {
                    exports.contains_key(&path)
                } else {
                    false
                }
            };

            if !exists {
                let _ = self.add_export(config, self.next_inode.fetch_add(1, Ordering::Relaxed));
            }
        }
    }

    pub fn count(&self) -> usize {
        let exports = match self.exports.read() {
            Ok(e) => e,
            Err(_) => return 0,
        };

        exports.len()
    }

    pub fn total_clients(&self) -> u32 {
        let exports = match self.exports.read() {
            Ok(e) => e,
            Err(_) => return 0,
        };

        exports.values().map(|e| e.client_count).sum()
    }
}

impl Default for ExportManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(path: &str) -> ExportConfig {
        ExportConfig::default_rw(path)
    }

    #[test]
    fn test_export_manager_new() {
        let manager = ExportManager::new();
        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn test_add_export() {
        let manager = ExportManager::new();
        let config = test_config("/export");
        let result = manager.add_export(config, 100);
        assert!(result.is_ok());
        assert_eq!(manager.count(), 1);
    }

    #[test]
    fn test_add_duplicate_export_error() {
        let manager = ExportManager::new();
        let config = test_config("/export");
        let _ = manager.add_export(config.clone(), 100);
        let result = manager.add_export(config, 200);
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_export() {
        let manager = ExportManager::new();
        let config = test_config("/export");
        let _ = manager.add_export(config, 100);

        let removed = manager.remove_export("/export");
        assert!(removed);
        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn test_remove_nonexistent_export() {
        let manager = ExportManager::new();
        let removed = manager.remove_export("/nonexistent");
        assert!(!removed);
    }

    #[test]
    fn test_get_export() {
        let manager = ExportManager::new();
        let config = test_config("/export");
        let _ = manager.add_export(config, 100);

        let export = manager.get_export("/export");
        assert!(export.is_some());
        assert_eq!(export.unwrap().config.path, "/export");
    }

    #[test]
    fn test_get_nonexistent_export() {
        let manager = ExportManager::new();
        let export = manager.get_export("/nonexistent");
        assert!(export.is_none());
    }

    #[test]
    fn test_list_exports() {
        let manager = ExportManager::new();
        let _ = manager.add_export(test_config("/export1"), 100);
        let _ = manager.add_export(test_config("/export2"), 200);

        let exports = manager.list_exports();
        assert_eq!(exports.len(), 2);
    }

    #[test]
    fn test_export_paths() {
        let manager = ExportManager::new();
        let _ = manager.add_export(test_config("/export1"), 100);
        let _ = manager.add_export(test_config("/export2"), 200);

        let paths = manager.export_paths();
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&"/export1".to_string()));
        assert!(paths.contains(&"/export2".to_string()));
    }

    #[test]
    fn test_is_exported() {
        let manager = ExportManager::new();
        let config = test_config("/export");
        let _ = manager.add_export(config, 100);

        assert!(manager.is_exported("/export"));
        assert!(!manager.is_exported("/nonexistent"));
    }

    #[test]
    fn test_increment_clients() {
        let manager = ExportManager::new();
        let config = test_config("/export");
        let _ = manager.add_export(config, 100);

        let result = manager.increment_clients("/export");
        assert!(result);

        let export = manager.get_export("/export").unwrap();
        assert_eq!(export.client_count, 1);
    }

    #[test]
    fn test_decrement_clients() {
        let manager = ExportManager::new();
        let config = test_config("/export");
        let _ = manager.add_export(config, 100);

        manager.increment_clients("/export");
        manager.increment_clients("/export");

        let result = manager.decrement_clients("/export");
        assert!(result);

        let export = manager.get_export("/export").unwrap();
        assert_eq!(export.client_count, 1);
    }

    #[test]
    fn test_increment_nonexistent() {
        let manager = ExportManager::new();
        let result = manager.increment_clients("/nonexistent");
        assert!(!result);
    }

    #[test]
    fn test_root_fh() {
        let manager = ExportManager::new();
        let config = test_config("/export");
        let _ = manager.add_export(config, 12345);

        let fh = manager.root_fh("/export");
        assert!(fh.is_some());
        assert_eq!(fh.unwrap().as_inode(), Some(12345));
    }

    #[test]
    fn test_root_fh_nonexistent() {
        let manager = ExportManager::new();
        let fh = manager.root_fh("/nonexistent");
        assert!(fh.is_none());
    }

    #[test]
    fn test_reload_adds_new_exports() {
        let manager = ExportManager::new();
        let configs = vec![test_config("/export1"), test_config("/export2")];
        manager.reload(configs);
        assert_eq!(manager.count(), 2);
    }

    #[test]
    fn test_reload_removes_old_exports() {
        let manager = ExportManager::new();
        let _ = manager.add_export(test_config("/export1"), 100);
        let _ = manager.add_export(test_config("/export2"), 200);

        manager.reload(vec![test_config("/export1")]);
        assert!(manager.is_exported("/export1"));
        assert!(!manager.is_exported("/export2"));
    }

    #[test]
    fn test_force_remove_export() {
        let manager = ExportManager::new();
        let config = test_config("/export");
        let _ = manager.add_export(config, 100);

        manager.increment_clients("/export");

        let removed = manager.force_remove_export("/export");
        assert!(removed);
        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn test_total_clients() {
        let manager = ExportManager::new();
        let _ = manager.add_export(test_config("/export1"), 100);
        let _ = manager.add_export(test_config("/export2"), 200);

        manager.increment_clients("/export1");
        manager.increment_clients("/export1");
        manager.increment_clients("/export2");

        assert_eq!(manager.total_clients(), 3);
    }

    #[test]
    fn test_export_active_status() {
        let manager = ExportManager::new();
        let config = test_config("/export");
        let _ = manager.add_export(config, 100);

        let export = manager.get_export("/export").unwrap();
        assert_eq!(export.status, ExportStatus::Active);
        assert!(export.is_active());
    }

    #[test]
    fn test_export_draining_status() {
        let manager = ExportManager::new();
        let config = test_config("/export");
        let _ = manager.add_export(config, 100);

        manager.increment_clients("/export");
        manager.remove_export("/export");

        let export = manager.get_export("/export").unwrap();
        assert_eq!(export.status, ExportStatus::Draining);
    }

    #[test]
    fn test_export_can_remove() {
        let manager = ExportManager::new();
        let config = test_config("/export");
        let _ = manager.add_export(config, 100);

        manager.increment_clients("/export");
        manager.remove_export("/export");

        let export = manager.get_export("/export").unwrap();
        assert!(!export.can_remove());
    }

    #[test]
    fn test_export_can_remove_after_clients_disconnect() {
        let manager = ExportManager::new();
        let config = test_config("/export");
        let _ = manager.add_export(config, 100);

        manager.increment_clients("/export");
        manager.remove_export("/export");
        manager.decrement_clients("/export");

        let export = manager.get_export("/export").unwrap();
        assert!(export.can_remove());
    }
}
