//! portmapper/rpcbind registration stubs

use crate::rpc::{MOUNT_PROGRAM, MOUNT_VERSION, NFS_PROGRAM, NFS_VERSION};
use std::sync::{Arc, Mutex};

pub const PORTMAP_PORT: u16 = 111;
pub const NFS_PORT: u16 = 2049;
pub const MOUNT_PORT: u16 = 20048;

pub const IPPROTO_TCP: u32 = 6;
pub const IPPROTO_UDP: u32 = 17;

#[derive(Debug, Clone)]
pub struct PortmapEntry {
    pub prog: u32,
    pub vers: u32,
    pub proto: u32,
    pub port: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct PortmapResult {
    pub port: u16,
}

pub struct Portmapper {
    registrations: Arc<Mutex<Vec<PortmapEntry>>>,
}

impl Portmapper {
    pub fn new() -> Self {
        Self {
            registrations: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn register_defaults(&mut self) {
        self.register(PortmapEntry {
            prog: NFS_PROGRAM,
            vers: NFS_VERSION,
            proto: IPPROTO_TCP,
            port: NFS_PORT,
        });
        self.register(PortmapEntry {
            prog: NFS_PROGRAM,
            vers: NFS_VERSION,
            proto: IPPROTO_UDP,
            port: NFS_PORT,
        });
        self.register(PortmapEntry {
            prog: MOUNT_PROGRAM,
            vers: MOUNT_VERSION,
            proto: IPPROTO_TCP,
            port: MOUNT_PORT,
        });
        self.register(PortmapEntry {
            prog: MOUNT_PROGRAM,
            vers: MOUNT_VERSION,
            proto: IPPROTO_UDP,
            port: MOUNT_PORT,
        });
    }

    pub fn register(&mut self, entry: PortmapEntry) {
        if let Ok(mut regs) = self.registrations.lock() {
            regs.retain(|r| {
                !(r.prog == entry.prog && r.vers == entry.vers && r.proto == entry.proto)
            });
            regs.push(entry);
        }
    }

    pub fn unregister(&mut self, prog: u32, vers: u32, proto: u32) -> bool {
        if let Ok(mut regs) = self.registrations.lock() {
            let len_before = regs.len();
            regs.retain(|r| !(r.prog == prog && r.vers == vers && r.proto == proto));
            regs.len() < len_before
        } else {
            false
        }
    }

    pub fn get_port(&self, prog: u32, vers: u32, proto: u32) -> u16 {
        self.registrations
            .lock()
            .ok()
            .and_then(|regs| {
                regs.iter()
                    .find(|r| r.prog == prog && r.vers == vers && r.proto == proto)
                    .map(|r| r.port)
            })
            .unwrap_or(0)
    }

    pub fn dump(&self) -> Vec<PortmapEntry> {
        self.registrations
            .lock()
            .map(|r| r.clone())
            .unwrap_or_default()
    }

    pub fn clear(&mut self) {
        if let Ok(mut regs) = self.registrations.lock() {
            regs.clear();
        }
    }

    pub fn count(&self) -> usize {
        self.registrations.lock().map(|r| r.len()).unwrap_or(0)
    }
}

impl Default for Portmapper {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_portmapper() {
        let pm = Portmapper::new();
        assert_eq!(pm.count(), 0);
    }

    #[test]
    fn test_register_defaults() {
        let mut pm = Portmapper::new();
        pm.register_defaults();
        assert_eq!(pm.count(), 4);

        let nfs_tcp = pm.get_port(NFS_PROGRAM, NFS_VERSION, IPPROTO_TCP);
        assert_eq!(nfs_tcp, NFS_PORT);

        let nfs_udp = pm.get_port(NFS_PROGRAM, NFS_VERSION, IPPROTO_UDP);
        assert_eq!(nfs_udp, NFS_PORT);

        let mount_tcp = pm.get_port(MOUNT_PROGRAM, MOUNT_VERSION, IPPROTO_TCP);
        assert_eq!(mount_tcp, MOUNT_PORT);

        let mount_udp = pm.get_port(MOUNT_PROGRAM, MOUNT_VERSION, IPPROTO_UDP);
        assert_eq!(mount_udp, MOUNT_PORT);
    }

    #[test]
    fn test_get_port_not_registered() {
        let pm = Portmapper::new();
        let port = pm.get_port(999999, 1, IPPROTO_TCP);
        assert_eq!(port, 0);
    }

    #[test]
    fn test_unregister() {
        let mut pm = Portmapper::new();
        pm.register_defaults();
        assert_eq!(pm.count(), 4);

        let removed = pm.unregister(NFS_PROGRAM, NFS_VERSION, IPPROTO_TCP);
        assert!(removed);
        assert_eq!(pm.count(), 3);

        let port = pm.get_port(NFS_PROGRAM, NFS_VERSION, IPPROTO_TCP);
        assert_eq!(port, 0);
    }

    #[test]
    fn test_dump() {
        let mut pm = Portmapper::new();
        pm.register_defaults();
        let entries = pm.dump();
        assert_eq!(entries.len(), 4);
    }

    #[test]
    fn test_clear() {
        let mut pm = Portmapper::new();
        pm.register_defaults();
        assert_eq!(pm.count(), 4);
        pm.clear();
        assert_eq!(pm.count(), 0);
    }

    #[test]
    fn test_register_replace() {
        let mut pm = Portmapper::new();
        pm.register(PortmapEntry {
            prog: 100003,
            vers: 3,
            proto: 6,
            port: 2000,
        });
        pm.register(PortmapEntry {
            prog: 100003,
            vers: 3,
            proto: 6,
            port: 3000,
        });

        let port = pm.get_port(100003, 3, 6);
        assert_eq!(port, 3000);
    }

    #[test]
    fn test_count() {
        let mut pm = Portmapper::new();
        assert_eq!(pm.count(), 0);

        pm.register(PortmapEntry {
            prog: 100000,
            vers: 2,
            proto: 6,
            port: 111,
        });
        assert_eq!(pm.count(), 1);
    }
}
