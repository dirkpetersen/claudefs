//! Gateway NFS export manager, MOUNT protocol, and portmapper security tests.
//!
//! Part of A10 Phase 21: Gateway export/mount/portmap security audit

use claudefs_gateway::config::ExportConfig;
use claudefs_gateway::export_manager::{ActiveExport, ExportManager, ExportStatus};
use claudefs_gateway::mount::{
    ExportEntry, MntResult, MountEntry, MountHandler, MNT_ERR_ACCESS, MNT_ERR_NOENT, MNT_ERR_PERM,
    MNT_OK,
};
use claudefs_gateway::portmap::{
    PortmapEntry, Portmapper, IPPROTO_TCP, IPPROTO_UDP, MOUNT_PORT, NFS_PORT, PORTMAP_PORT,
};
use claudefs_gateway::protocol::FileHandle3;
use claudefs_gateway::rpc::{MOUNT_PROGRAM, MOUNT_VERSION, NFS_PROGRAM, NFS_VERSION};

#[cfg(test)]
mod tests {
    use super::*;

    fn make_export_config(path: &str) -> ExportConfig {
        ExportConfig::default_rw(path)
    }

    fn make_mount_export(path: String, groups: Vec<String>) -> ExportEntry {
        ExportEntry {
            dirpath: path,
            groups,
        }
    }

    // Category 1: Export Manager Lifecycle (5 tests)

    #[test]
    fn test_export_add_and_get() {
        let manager = ExportManager::new();
        let config = make_export_config("/export");
        let result = manager.add_export(config, 100);

        assert!(result.is_ok());
        assert_eq!(manager.count(), 1);

        let export = manager.get_export("/export").expect("export should exist");
        assert_eq!(export.config.path, "/export");
        assert_eq!(export.status, ExportStatus::Active);
        assert_eq!(export.root_inode, 100);
        // FINDING-GW-NFS-01: add_export creates export with correct path, status=Active, and root_inode
    }

    #[test]
    fn test_export_duplicate_rejected() {
        let manager = ExportManager::new();
        let config = make_export_config("/export");
        let _ = manager.add_export(config.clone(), 100);
        let result = manager.add_export(config, 200);

        assert!(result.is_err());
        assert_eq!(manager.count(), 1);
        // FINDING-GW-NFS-02: duplicate export paths rejected — prevents path confusion
    }

    #[test]
    fn test_export_draining_with_clients() {
        let manager = ExportManager::new();
        let config = make_export_config("/export");
        let _ = manager.add_export(config, 100);

        manager.increment_clients("/export");
        manager.remove_export("/export");

        let export = manager.get_export("/export").expect("export should exist");
        assert_eq!(export.status, ExportStatus::Draining);
        assert!(!export.is_active());
        assert!(!export.can_remove());

        manager.decrement_clients("/export");

        let export = manager.get_export("/export").expect("export should exist");
        assert!(export.can_remove());
        // FINDING-GW-NFS-03: graceful draining prevents data corruption during removal
    }

    #[test]
    fn test_export_force_remove() {
        let manager = ExportManager::new();
        let config = make_export_config("/export");
        let _ = manager.add_export(config, 100);

        manager.increment_clients("/export");
        let result = manager.force_remove_export("/export");

        assert!(result);
        assert_eq!(manager.count(), 0);
        // FINDING-GW-NFS-04: force remove overrides client safety — only for admin use
    }

    #[test]
    fn test_export_reload() {
        let manager = ExportManager::new();
        let _ = manager.add_export(make_export_config("/export1"), 100);
        let _ = manager.add_export(make_export_config("/export2"), 200);

        manager.reload(vec![
            make_export_config("/export1"),
            make_export_config("/export3"),
        ]);

        assert!(manager.get_export("/export1").is_some());
        assert!(manager.get_export("/export2").is_none());
        assert!(manager.get_export("/export3").is_some());
        // FINDING-GW-NFS-05: reload correctly adds new and removes old exports
    }

    // Category 2: Export Client Tracking (5 tests)

    #[test]
    fn test_export_client_counting() {
        let manager = ExportManager::new();
        let config = make_export_config("/export");
        let _ = manager.add_export(config, 100);

        manager.increment_clients("/export");
        manager.increment_clients("/export");
        manager.increment_clients("/export");

        let export = manager.get_export("/export").expect("export should exist");
        assert_eq!(export.client_count, 3);
        assert_eq!(manager.total_clients(), 3);

        manager.decrement_clients("/export");

        let export = manager.get_export("/export").expect("export should exist");
        assert_eq!(export.client_count, 2);
    }

    #[test]
    fn test_export_decrement_below_zero() {
        let manager = ExportManager::new();
        let config = make_export_config("/export");
        let _ = manager.add_export(config, 100);

        manager.decrement_clients("/export");

        let export = manager.get_export("/export").expect("export should exist");
        assert_eq!(export.client_count, 0);
        // FINDING-GW-NFS-06: client count underflow protected
    }

    #[test]
    fn test_export_increment_nonexistent() {
        let manager = ExportManager::new();

        let result = manager.increment_clients("/nonexistent");
        assert!(!result);

        let result = manager.decrement_clients("/nonexistent");
        assert!(!result);
    }

    #[test]
    fn test_export_root_fh() {
        let manager = ExportManager::new();
        let config = make_export_config("/export");
        let _ = manager.add_export(config, 12345);

        let fh = manager.root_fh("/export").expect("should have root fh");
        assert_eq!(fh.as_inode(), Some(12345));

        let fh = manager.root_fh("/nonexistent");
        assert!(fh.is_none());
    }

    #[test]
    fn test_export_is_exported() {
        let manager = ExportManager::new();
        let config = make_export_config("/export");
        let _ = manager.add_export(config, 100);

        assert!(manager.is_exported("/export"));
        assert!(!manager.is_exported("/nonexistent"));

        manager.remove_export("/export");

        assert!(!manager.is_exported("/export"));
    }

    // Category 3: MOUNT Protocol (5 tests)

    #[test]
    fn test_mount_valid_path() {
        let exports = vec![make_mount_export("/export".to_string(), vec![])];
        let root_fh = FileHandle3::from_inode(1);
        let handler = MountHandler::new(exports, root_fh);

        let result = handler.mnt("/export", "client1");

        assert_eq!(result.status, MNT_OK);
        assert!(result.filehandle.is_some());
        assert_eq!(handler.mount_count(), 1);
        // FINDING-GW-NFS-07: valid mount returns file handle
    }

    #[test]
    fn test_mount_invalid_path() {
        let exports = vec![make_mount_export("/export".to_string(), vec![])];
        let root_fh = FileHandle3::from_inode(1);
        let handler = MountHandler::new(exports, root_fh);

        let result = handler.mnt("/nonexistent", "client1");

        assert_eq!(result.status, MNT_ERR_NOENT);
        assert!(result.filehandle.is_none());
        assert_eq!(handler.mount_count(), 0);
        // FINDING-GW-NFS-08: mount to non-exported path correctly rejected
    }

    #[test]
    fn test_mount_client_access_control() {
        let exports = vec![make_mount_export(
            "/secure".to_string(),
            vec!["allowedhost".to_string()],
        )];
        let root_fh = FileHandle3::from_inode(1);
        let handler = MountHandler::new(exports, root_fh);

        let result = handler.mnt("/secure", "otherhost");
        assert_eq!(result.status, MNT_ERR_ACCESS);

        let result = handler.mnt("/secure", "allowedhost");
        assert_eq!(result.status, MNT_OK);
        // FINDING-GW-NFS-09: client-based access control enforced at mount time
    }

    #[test]
    fn test_mount_umnt_and_umntall() {
        let exports = vec![make_mount_export("/export".to_string(), vec![])];
        let root_fh = FileHandle3::from_inode(1);
        let handler = MountHandler::new(exports, root_fh);

        handler.mnt("/export", "host1");
        handler.mnt("/export", "host2");

        assert_eq!(handler.dump().len(), 2);

        handler.umnt("/export");
        assert!(handler.dump().is_empty());

        handler.mnt("/export", "host1");
        handler.mnt("/export", "host2");

        handler.umntall();
        assert!(handler.dump().is_empty());
    }

    #[test]
    fn test_mount_is_allowed_wildcard() {
        let wildcard_export = make_mount_export("/test".to_string(), vec!["*".to_string()]);
        let root_fh = FileHandle3::from_inode(1);
        let handler = MountHandler::new(vec![wildcard_export], root_fh.clone());

        assert!(handler.is_allowed(
            &ExportEntry {
                dirpath: "/test".to_string(),
                groups: vec!["*".to_string()]
            },
            "anyhost"
        ));

        let empty_export = make_mount_export("/test2".to_string(), vec![]);
        let handler2 = MountHandler::new(vec![empty_export], root_fh);

        assert!(handler2.is_allowed(
            &ExportEntry {
                dirpath: "/test2".to_string(),
                groups: vec![]
            },
            "anyhost"
        ));
        // FINDING-GW-NFS-10: wildcard and empty groups both allow all clients
    }

    // Category 4: Portmapper Registration (5 tests)

    #[test]
    fn test_portmapper_register_defaults() {
        let mut pm = Portmapper::new();
        pm.register_defaults();

        assert_eq!(pm.count(), 4);
        assert_eq!(pm.get_port(NFS_PROGRAM, NFS_VERSION, IPPROTO_TCP), NFS_PORT);
        assert_eq!(
            pm.get_port(MOUNT_PROGRAM, MOUNT_VERSION, IPPROTO_TCP),
            MOUNT_PORT
        );
    }

    #[test]
    fn test_portmapper_lookup_not_registered() {
        let pm = Portmapper::new();
        let port = pm.get_port(999999, 1, IPPROTO_TCP);
        assert_eq!(port, 0);
        // FINDING-GW-NFS-11: unregistered programs return port 0 — prevents port confusion
    }

    #[test]
    fn test_portmapper_register_replace() {
        let mut pm = Portmapper::new();
        pm.register(PortmapEntry {
            prog: 100003,
            vers: 3,
            proto: IPPROTO_TCP,
            port: 2000,
        });
        pm.register(PortmapEntry {
            prog: 100003,
            vers: 3,
            proto: IPPROTO_TCP,
            port: 3000,
        });

        let port = pm.get_port(100003, 3, IPPROTO_TCP);
        assert_eq!(port, 3000);
        assert_eq!(pm.count(), 1);
        // FINDING-GW-NFS-12: re-registration replaces port — prevents stale entries
    }

    #[test]
    fn test_portmapper_unregister() {
        let mut pm = Portmapper::new();
        pm.register_defaults();

        assert_eq!(pm.count(), 4);

        pm.unregister(NFS_PROGRAM, NFS_VERSION, IPPROTO_TCP);

        assert_eq!(pm.count(), 3);
        assert_eq!(pm.get_port(NFS_PROGRAM, NFS_VERSION, IPPROTO_TCP), 0);
        assert_eq!(pm.get_port(NFS_PROGRAM, NFS_VERSION, IPPROTO_UDP), NFS_PORT);
        // FINDING-GW-NFS-13: unregister is protocol-specific — doesn't affect other protocols
    }

    #[test]
    fn test_portmapper_dump_and_clear() {
        let mut pm = Portmapper::new();
        pm.register_defaults();

        assert_eq!(pm.dump().len(), 4);

        pm.clear();

        assert_eq!(pm.count(), 0);
        assert!(pm.dump().is_empty());
    }

    // Category 5: Cross-Module & Edge Cases (5 tests)

    #[test]
    fn test_export_status_variants() {
        let active = ExportStatus::Active;
        let draining = ExportStatus::Draining;
        let disabled = ExportStatus::Disabled;

        assert_eq!(active, ExportStatus::Active);
        assert_ne!(active, draining);
        assert_ne!(active, disabled);
    }

    #[test]
    fn test_mount_auth_flavors() {
        let exports = vec![make_mount_export("/export".to_string(), vec![])];
        let root_fh = FileHandle3::from_inode(1);
        let handler = MountHandler::new(exports, root_fh);

        let result = handler.mnt("/export", "client1");

        assert!(result.auth_flavors.contains(&0));
        assert!(result.auth_flavors.contains(&1));
        // FINDING-GW-NFS-14: mount returns supported auth flavors for client negotiation
    }

    #[test]
    fn test_mount_localhost_bypass() {
        let export = ExportEntry {
            dirpath: "/test".to_string(),
            groups: vec!["host1".to_string()],
        };
        let root_fh = FileHandle3::from_inode(1);
        let handler = MountHandler::new(vec![export], root_fh);

        assert!(handler.is_allowed(
            &ExportEntry {
                dirpath: "/test".to_string(),
                groups: vec!["host1".to_string()]
            },
            "127.0.0.1"
        ));
        assert!(handler.is_allowed(
            &ExportEntry {
                dirpath: "/test".to_string(),
                groups: vec!["host1".to_string()]
            },
            ""
        ));
        // FINDING-GW-NFS-15: localhost always allowed — standard NFS behavior
    }

    #[test]
    fn test_portmapper_constants() {
        assert_eq!(PORTMAP_PORT, 111);
        assert_eq!(NFS_PORT, 2049);
        assert_eq!(MOUNT_PORT, 20048);
        assert_eq!(IPPROTO_TCP, 6);
        assert_eq!(IPPROTO_UDP, 17);
    }

    #[test]
    fn test_export_list_paths() {
        let manager = ExportManager::new();
        let _ = manager.add_export(make_export_config("/export1"), 100);
        let _ = manager.add_export(make_export_config("/export2"), 200);
        let _ = manager.add_export(make_export_config("/export3"), 300);

        let exports = manager.list_exports();
        assert_eq!(exports.len(), 3);

        let paths = manager.export_paths();
        assert!(paths.contains(&"/export1".to_string()));
        assert!(paths.contains(&"/export2".to_string()));
        assert!(paths.contains(&"/export3".to_string()));
    }
}
