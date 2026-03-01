//! pNFS layout server implementation

use std::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum LayoutType {
    Nfs4Block = 2,
    ObjLayout = 3,
    Files = 1,
}

#[derive(Debug, Clone)]
pub struct DataServerLocation {
    pub address: String,
    pub device_id: [u8; 16],
}

#[derive(Debug, Clone)]
pub struct LayoutSegment {
    pub layout_type: LayoutType,
    pub offset: u64,
    pub length: u64,
    pub iomode: IoMode,
    pub data_servers: Vec<DataServerLocation>,
    pub stripe_unit: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum IoMode {
    Read = 1,
    ReadWrite = 2,
    Any = 3,
}

impl IoMode {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            1 => Some(IoMode::Read),
            2 => Some(IoMode::ReadWrite),
            3 => Some(IoMode::Any),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LayoutGetResult {
    pub layout_type: LayoutType,
    pub segments: Vec<LayoutSegment>,
    pub stateid: [u8; 16],
}

pub struct PnfsLayoutServer {
    data_servers: RwLock<Vec<DataServerLocation>>,
    #[allow(dead_code)]
    fsid: u64,
}

impl PnfsLayoutServer {
    pub fn new(data_servers: Vec<DataServerLocation>, fsid: u64) -> Self {
        Self {
            data_servers: RwLock::new(data_servers),
            fsid,
        }
    }

    pub fn get_layout(
        &self,
        inode: u64,
        offset: u64,
        length: u64,
        iomode: IoMode,
    ) -> LayoutGetResult {
        let servers = self.data_servers.read().unwrap();
        let server_count = servers.len();

        if server_count == 0 {
            return LayoutGetResult {
                layout_type: LayoutType::Files,
                segments: vec![],
                stateid: [0; 16],
            };
        }

        let stripe_unit = 65536;
        let server_idx = (inode % server_count as u64) as usize;

        LayoutGetResult {
            layout_type: LayoutType::Files,
            segments: vec![LayoutSegment {
                layout_type: LayoutType::Files,
                offset,
                length,
                iomode,
                data_servers: vec![servers[server_idx].clone()],
                stripe_unit,
            }],
            stateid: {
                let mut stateid = [0u8; 16];
                stateid[0..8].copy_from_slice(&inode.to_le_bytes());
                stateid
            },
        }
    }

    pub fn server_count(&self) -> usize {
        self.data_servers.read().unwrap().len()
    }

    pub fn add_server(&mut self, location: DataServerLocation) {
        self.data_servers.write().unwrap().push(location);
    }

    pub fn remove_server(&mut self, address: &str) -> bool {
        let mut servers = self.data_servers.write().unwrap();
        if let Some(pos) = servers.iter().position(|s| s.address == address) {
            servers.remove(pos);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_server(address: &str) -> DataServerLocation {
        DataServerLocation {
            address: address.to_string(),
            device_id: [0xAB; 16],
        }
    }

    #[test]
    fn test_new_server() {
        let servers = vec![make_test_server("192.168.1.1:2001")];
        let server = PnfsLayoutServer::new(servers, 1);
        assert_eq!(server.server_count(), 1);
    }

    #[test]
    fn test_empty_server() {
        let server = PnfsLayoutServer::new(vec![], 1);
        assert_eq!(server.server_count(), 0);
    }

    #[test]
    fn test_single_server_layout() {
        let servers = vec![make_test_server("192.168.1.1:2001")];
        let server = PnfsLayoutServer::new(servers, 1);
        let layout = server.get_layout(123, 0, 1_000_000, IoMode::Read);

        assert_eq!(layout.segments.len(), 1);
        assert_eq!(layout.segments[0].layout_type, LayoutType::Files);
        assert_eq!(layout.segments[0].offset, 0);
        assert_eq!(layout.segments[0].length, 1_000_000);
        assert_eq!(layout.segments[0].iomode, IoMode::Read);
    }

    #[test]
    fn test_multiple_servers_stripe() {
        let servers = vec![
            make_test_server("192.168.1.1:2001"),
            make_test_server("192.168.1.2:2001"),
            make_test_server("192.168.1.3:2001"),
        ];
        let server = PnfsLayoutServer::new(servers, 1);

        let layout = server.get_layout(0, 0, 1_000_000, IoMode::ReadWrite);
        assert_eq!(
            layout.segments[0].data_servers[0].address,
            "192.168.1.1:2001"
        );

        let layout2 = server.get_layout(1, 0, 1_000_000, IoMode::ReadWrite);
        assert_eq!(
            layout2.segments[0].data_servers[0].address,
            "192.168.1.2:2001"
        );

        let layout3 = server.get_layout(2, 0, 1_000_000, IoMode::ReadWrite);
        assert_eq!(
            layout3.segments[0].data_servers[0].address,
            "192.168.1.3:2001"
        );
    }

    #[test]
    fn test_iomode_from_u32() {
        assert_eq!(IoMode::from_u32(1), Some(IoMode::Read));
        assert_eq!(IoMode::from_u32(2), Some(IoMode::ReadWrite));
        assert_eq!(IoMode::from_u32(3), Some(IoMode::Any));
        assert_eq!(IoMode::from_u32(99), None);
    }

    #[test]
    fn test_add_server() {
        let servers = vec![make_test_server("192.168.1.1:2001")];
        let mut server = PnfsLayoutServer::new(servers, 1);

        server.add_server(make_test_server("192.168.1.2:2001"));
        assert_eq!(server.server_count(), 2);
    }

    #[test]
    fn test_remove_server_existing() {
        let servers = vec![
            make_test_server("192.168.1.1:2001"),
            make_test_server("192.168.1.2:2001"),
        ];
        let mut server = PnfsLayoutServer::new(servers, 1);

        let removed = server.remove_server("192.168.1.1:2001");
        assert!(removed);
        assert_eq!(server.server_count(), 1);
    }

    #[test]
    fn test_remove_server_not_existing() {
        let servers = vec![make_test_server("192.168.1.1:2001")];
        let mut server = PnfsLayoutServer::new(servers, 1);

        let removed = server.remove_server("192.168.1.99:2001");
        assert!(!removed);
        assert_eq!(server.server_count(), 1);
    }

    #[test]
    fn test_layout_stateid() {
        let servers = vec![make_test_server("192.168.1.1:2001")];
        let server = PnfsLayoutServer::new(servers, 1);
        let layout = server.get_layout(12345, 0, 1_000_000, IoMode::Read);

        assert_eq!(layout.stateid[0..8], 12345u64.to_le_bytes());
    }

    #[test]
    fn test_stripe_unit() {
        let servers = vec![make_test_server("192.168.1.1:2001")];
        let server = PnfsLayoutServer::new(servers, 1);
        let layout = server.get_layout(123, 0, 1_000_000, IoMode::Read);

        assert_eq!(layout.segments[0].stripe_unit, 65536);
    }

    #[test]
    fn test_layout_type_files() {
        let servers = vec![make_test_server("192.168.1.1:2001")];
        let server = PnfsLayoutServer::new(servers, 1);
        let layout = server.get_layout(123, 0, 1_000_000, IoMode::Read);

        assert_eq!(layout.layout_type, LayoutType::Files);
    }

    #[test]
    fn test_layout_offset_length() {
        let servers = vec![make_test_server("192.168.1.1:2001")];
        let server = PnfsLayoutServer::new(servers, 1);
        let layout = server.get_layout(123, 1000, 5000, IoMode::ReadWrite);

        assert_eq!(layout.segments[0].offset, 1000);
        assert_eq!(layout.segments[0].length, 5000);
    }
}
