use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;
use thiserror::Error;
use tracing::{debug, info};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MultichannelRole {
    Primary,
    Secondary,
    Standby,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceCapabilities {
    pub rdma: bool,
    pub rss: bool,
    pub tso: bool,
    pub checksum_offload: bool,
}

impl Default for InterfaceCapabilities {
    fn default() -> Self {
        Self {
            rdma: false,
            rss: false,
            tso: false,
            checksum_offload: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NicCapabilities {
    pub interface_name: String,
    pub ip_address: String,
    pub port: u16,
    pub link_speed_mbps: u64,
    pub capabilities: InterfaceCapabilities,
    pub enabled: bool,
}

impl NicCapabilities {
    pub fn new(interface_name: String, ip_address: String) -> Self {
        Self {
            interface_name,
            ip_address,
            port: 445,
            link_speed_mbps: 0,
            capabilities: InterfaceCapabilities::default(),
            enabled: true,
        }
    }

    pub fn with_speed(mut self, speed_mbps: u64) -> Self {
        self.link_speed_mbps = speed_mbps;
        self
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn with_rdma(mut self) -> Self {
        self.capabilities.rdma = true;
        self
    }

    pub fn with_rss(mut self) -> Self {
        self.capabilities.rss = true;
        self
    }

    pub fn with_tso(mut self) -> Self {
        self.capabilities.tso = true;
        self
    }

    pub fn with_checksum_offload(mut self) -> Self {
        self.capabilities.checksum_offload = true;
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelSelectionPolicy {
    RoundRobin,
    WeightedBySpeed,
    PreferRdma,
    PinToInterface(String),
}

impl Default for ChannelSelectionPolicy {
    fn default() -> Self {
        Self::WeightedBySpeed
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultichannelConfig {
    pub enabled: bool,
    pub max_channels: u32,
    pub min_channels: u32,
    pub prefer_rdma: bool,
    pub interfaces: Vec<NicCapabilities>,
    pub channel_selection: ChannelSelectionPolicy,
}

impl Default for MultichannelConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_channels: 8,
            min_channels: 2,
            prefer_rdma: false,
            interfaces: Vec::new(),
            channel_selection: ChannelSelectionPolicy::default(),
        }
    }
}

impl MultichannelConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_max_channels(mut self, max: u32) -> Self {
        self.max_channels = max;
        self
    }

    pub fn with_min_channels(mut self, min: u32) -> Self {
        self.min_channels = min;
        self
    }

    pub fn with_prefer_rdma(mut self, prefer: bool) -> Self {
        self.prefer_rdma = prefer;
        self
    }

    pub fn with_channel_selection(mut self, policy: ChannelSelectionPolicy) -> Self {
        self.channel_selection = policy;
        self
    }

    pub fn with_interface(mut self, nic: NicCapabilities) -> Self {
        self.interfaces.push(nic);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelInfo {
    pub channel_id: u32,
    pub interface: String,
    pub role: MultichannelRole,
    pub is_active: bool,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultichannelSession {
    pub session_id: u64,
    pub channels: Vec<ChannelInfo>,
    pub created_at: SystemTime,
    pub total_bytes_sent: u64,
    pub total_bytes_received: u64,
}

impl MultichannelSession {
    pub fn new(session_id: u64) -> Self {
        Self {
            session_id,
            channels: Vec::new(),
            created_at: SystemTime::now(),
            total_bytes_sent: 0,
            total_bytes_received: 0,
        }
    }

    pub fn add_channel(&mut self, channel: ChannelInfo) {
        self.channels.push(channel);
    }

    pub fn update_stats(&mut self, channel_id: u32, sent: u64, received: u64) {
        if let Some(ch) = self
            .channels
            .iter_mut()
            .find(|c| c.channel_id == channel_id)
        {
            ch.bytes_sent += sent;
            ch.bytes_received += received;
        }
        self.total_bytes_sent += sent;
        self.total_bytes_received += received;
    }
}

#[derive(Debug, Error)]
pub enum MultichannelError {
    #[error("multichannel is not enabled in config")]
    Disabled,
    #[error("interface already registered: {0}")]
    DuplicateInterface(String),
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("no enabled interfaces available")]
    NoInterfacesAvailable,
}

pub struct MultichannelManager {
    config: MultichannelConfig,
    sessions: HashMap<u64, MultichannelSession>,
    round_robin_index: usize,
}

impl MultichannelManager {
    pub fn new(config: MultichannelConfig) -> Self {
        info!(
            "MultichannelManager created with enabled={}, max_channels={}, min_channels={}",
            config.enabled, config.max_channels, config.min_channels
        );
        Self {
            config,
            sessions: HashMap::new(),
            round_robin_index: 0,
        }
    }

    pub fn config(&self) -> &MultichannelConfig {
        &self.config
    }

    pub fn add_interface(&mut self, nic: NicCapabilities) -> Result<(), MultichannelError> {
        if self
            .config
            .interfaces
            .iter()
            .any(|i| i.interface_name == nic.interface_name)
        {
            debug!("Duplicate interface added: {}", nic.interface_name);
            return Err(MultichannelError::DuplicateInterface(nic.interface_name));
        }
        debug!(
            "Adding interface: {} ({}:{}, speed={}Mbps)",
            nic.interface_name, nic.ip_address, nic.port, nic.link_speed_mbps
        );
        self.config.interfaces.push(nic);
        Ok(())
    }

    pub fn remove_interface(&mut self, name: &str) -> bool {
        let initial_len = self.config.interfaces.len();
        self.config.interfaces.retain(|i| i.interface_name != name);
        let removed = self.config.interfaces.len() < initial_len;
        if removed {
            debug!("Removed interface: {}", name);
        }
        removed
    }

    pub fn available_interfaces(&self) -> Vec<&NicCapabilities> {
        self.config
            .interfaces
            .iter()
            .filter(|i| i.enabled)
            .collect()
    }

    pub fn select_interfaces_for_client(&self, n: u32) -> Vec<&NicCapabilities> {
        if !self.config.enabled {
            return Vec::new();
        }

        let enabled: Vec<&NicCapabilities> = self.available_interfaces();
        if enabled.is_empty() {
            return Vec::new();
        }

        let n = n as usize;
        match &self.config.channel_selection {
            ChannelSelectionPolicy::WeightedBySpeed => {
                let mut sorted: Vec<_> = enabled;
                sorted.sort_by(|a, b| b.link_speed_mbps.cmp(&a.link_speed_mbps));
                sorted.into_iter().take(n).collect()
            }
            ChannelSelectionPolicy::PreferRdma => {
                let mut sorted: Vec<_> = enabled;
                sorted.sort_by(|a, b| {
                    let a_rdma = a.capabilities.rdma as u8;
                    let b_rdma = b.capabilities.rdma as u8;
                    if a_rdma != b_rdma {
                        b_rdma.cmp(&a_rdma)
                    } else {
                        b.link_speed_mbps.cmp(&a.link_speed_mbps)
                    }
                });
                sorted.into_iter().take(n).collect()
            }
            ChannelSelectionPolicy::RoundRobin => {
                let len = enabled.len();
                if len == 0 {
                    return Vec::new();
                }
                let start = self.round_robin_index % len;
                (0..n).map(|i| enabled[(start + i) % len]).collect()
            }
            ChannelSelectionPolicy::PinToInterface(iface_name) => enabled
                .into_iter()
                .filter(|i| &i.interface_name == iface_name)
                .take(1)
                .collect(),
        }
    }

    pub fn create_session(&mut self, session_id: u64) -> MultichannelSession {
        debug!("Creating multichannel session: {}", session_id);
        let session = MultichannelSession::new(session_id);
        self.sessions.insert(session_id, session.clone());
        session
    }

    pub fn get_session(&self, session_id: u64) -> Option<&MultichannelSession> {
        self.sessions.get(&session_id)
    }

    pub fn remove_session(&mut self, session_id: u64) -> bool {
        debug!("Removing multichannel session: {}", session_id);
        self.sessions.remove(&session_id).is_some()
    }

    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    pub fn total_channel_count(&self) -> usize {
        self.sessions.values().map(|s| s.channels.len()).sum()
    }
}

impl Default for MultichannelManager {
    fn default() -> Self {
        Self::new(MultichannelConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_interface(name: &str, speed: u64, rdma: bool, enabled: bool) -> NicCapabilities {
        NicCapabilities::new(
            name.to_string(),
            format!("192.168.1.{}", name.chars().last().unwrap() as u8 - 48),
        )
        .with_speed(speed)
        .with_rdma_flag(rdma)
        .with_enabled(enabled)
    }

    impl NicCapabilities {
        fn with_rdma_flag(mut self, rdma: bool) -> Self {
            self.capabilities.rdma = rdma;
            self
        }
    }

    #[test]
    fn test_multichannel_config_defaults() {
        let config = MultichannelConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.max_channels, 8);
        assert_eq!(config.min_channels, 2);
        assert!(!config.prefer_rdma);
        assert!(config.interfaces.is_empty());
    }

    #[test]
    fn test_multichannel_config_builder() {
        let config = MultichannelConfig::new()
            .with_enabled(true)
            .with_max_channels(4)
            .with_min_channels(1)
            .with_prefer_rdma(true)
            .with_channel_selection(ChannelSelectionPolicy::PreferRdma);

        assert!(config.enabled);
        assert_eq!(config.max_channels, 4);
        assert_eq!(config.min_channels, 1);
        assert!(config.prefer_rdma);
        assert!(matches!(
            config.channel_selection,
            ChannelSelectionPolicy::PreferRdma
        ));
    }

    #[test]
    fn test_add_interface() {
        let config = MultichannelConfig::new().with_enabled(true);
        let mut manager = MultichannelManager::new(config);

        let nic =
            NicCapabilities::new("eth0".to_string(), "192.168.1.1".to_string()).with_speed(10000);
        assert!(manager.add_interface(nic).is_ok());
        assert_eq!(manager.available_interfaces().len(), 1);
    }

    #[test]
    fn test_add_duplicate_interface() {
        let config = MultichannelConfig::new().with_enabled(true);
        let mut manager = MultichannelManager::new(config);

        let nic = NicCapabilities::new("eth0".to_string(), "192.168.1.1".to_string());
        assert!(manager.add_interface(nic.clone()).is_ok());
        assert!(matches!(
            manager.add_interface(nic),
            Err(MultichannelError::DuplicateInterface(_))
        ));
    }

    #[test]
    fn test_remove_interface() {
        let config = MultichannelConfig::new().with_enabled(true);
        let mut manager = MultichannelManager::new(config);

        let nic = NicCapabilities::new("eth0".to_string(), "192.168.1.1".to_string());
        manager.add_interface(nic).unwrap();
        assert!(manager.remove_interface("eth0"));
        assert!(!manager.remove_interface("eth0"));
        assert!(manager.available_interfaces().is_empty());
    }

    #[test]
    fn test_available_interfaces_filters_disabled() {
        let config = MultichannelConfig::new().with_enabled(true);
        let mut manager = MultichannelManager::new(config);

        manager
            .add_interface(
                NicCapabilities::new("eth0".to_string(), "192.168.1.1".to_string())
                    .with_enabled(true),
            )
            .unwrap();
        manager
            .add_interface(
                NicCapabilities::new("eth1".to_string(), "192.168.1.2".to_string())
                    .with_enabled(false),
            )
            .unwrap();
        manager
            .add_interface(
                NicCapabilities::new("eth2".to_string(), "192.168.1.3".to_string())
                    .with_enabled(true),
            )
            .unwrap();

        let available = manager.available_interfaces();
        assert_eq!(available.len(), 2);
        assert!(available.iter().all(|i| i.enabled));
    }

    #[test]
    fn test_select_interfaces_weighted_by_speed() {
        let config = MultichannelConfig::new()
            .with_enabled(true)
            .with_channel_selection(ChannelSelectionPolicy::WeightedBySpeed);
        let mut manager = MultichannelManager::new(config);

        manager
            .add_interface(create_test_interface("eth0", 1000, false, true))
            .unwrap();
        manager
            .add_interface(create_test_interface("eth1", 10000, false, true))
            .unwrap();
        manager
            .add_interface(create_test_interface("eth2", 5000, false, true))
            .unwrap();
        manager
            .add_interface(create_test_interface("eth3", 25000, true, true))
            .unwrap();

        let selected = manager.select_interfaces_for_client(3);
        assert_eq!(selected.len(), 3);
        assert_eq!(selected[0].interface_name, "eth3");
        assert_eq!(selected[1].interface_name, "eth1");
        assert_eq!(selected[2].interface_name, "eth2");
    }

    #[test]
    fn test_select_interfaces_prefer_rdma() {
        let config = MultichannelConfig::new()
            .with_enabled(true)
            .with_channel_selection(ChannelSelectionPolicy::PreferRdma);
        let mut manager = MultichannelManager::new(config);

        manager
            .add_interface(create_test_interface("eth0", 10000, false, true))
            .unwrap();
        manager
            .add_interface(create_test_interface("eth1", 5000, true, true))
            .unwrap();
        manager
            .add_interface(create_test_interface("eth2", 25000, false, true))
            .unwrap();
        manager
            .add_interface(create_test_interface("eth3", 1000, true, true))
            .unwrap();

        let selected = manager.select_interfaces_for_client(4);
        assert_eq!(selected.len(), 4);
        assert!(selected[0].capabilities.rdma);
        assert!(selected[1].capabilities.rdma);
        assert!(!selected[2].capabilities.rdma);
        assert!(!selected[3].capabilities.rdma);
    }

    #[test]
    fn test_select_interfaces_round_robin() {
        let config = MultichannelConfig::new()
            .with_enabled(true)
            .with_channel_selection(ChannelSelectionPolicy::RoundRobin);
        let mut manager = MultichannelManager::new(config);

        manager
            .add_interface(create_test_interface("eth0", 1000, false, true))
            .unwrap();
        manager
            .add_interface(create_test_interface("eth1", 2000, false, true))
            .unwrap();
        manager
            .add_interface(create_test_interface("eth2", 3000, false, true))
            .unwrap();

        let first = manager.select_interfaces_for_client(2);
        assert_eq!(first[0].interface_name, "eth0");
        assert_eq!(first[1].interface_name, "eth1");
    }

    #[test]
    fn test_select_interfaces_pin_to_interface() {
        let config = MultichannelConfig::new()
            .with_enabled(true)
            .with_channel_selection(ChannelSelectionPolicy::PinToInterface("eth1".to_string()));
        let mut manager = MultichannelManager::new(config);

        manager
            .add_interface(create_test_interface("eth0", 1000, false, true))
            .unwrap();
        manager
            .add_interface(create_test_interface("eth1", 2000, false, true))
            .unwrap();
        manager
            .add_interface(create_test_interface("eth2", 3000, false, true))
            .unwrap();

        let selected = manager.select_interfaces_for_client(5);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].interface_name, "eth1");
    }

    #[test]
    fn test_select_interfaces_pin_to_nonexistent() {
        let config = MultichannelConfig::new()
            .with_enabled(true)
            .with_channel_selection(ChannelSelectionPolicy::PinToInterface("eth99".to_string()));
        let mut manager = MultichannelManager::new(config);

        manager
            .add_interface(create_test_interface("eth0", 1000, false, true))
            .unwrap();

        let selected = manager.select_interfaces_for_client(5);
        assert!(selected.is_empty());
    }

    #[test]
    fn test_disabled_multichannel_returns_empty() {
        let config = MultichannelConfig::new()
            .with_enabled(false)
            .with_channel_selection(ChannelSelectionPolicy::WeightedBySpeed);
        let mut manager = MultichannelManager::new(config);

        manager
            .add_interface(create_test_interface("eth0", 1000, false, true))
            .unwrap();

        let selected = manager.select_interfaces_for_client(3);
        assert!(selected.is_empty());
    }

    #[test]
    fn test_create_and_get_session() {
        let config = MultichannelConfig::new().with_enabled(true);
        let mut manager = MultichannelManager::new(config);

        let session = manager.create_session(12345);
        assert_eq!(session.session_id, 12345);

        let retrieved = manager.get_session(12345);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().session_id, 12345);
    }

    #[test]
    fn test_get_nonexistent_session() {
        let config = MultichannelConfig::new().with_enabled(true);
        let manager = MultichannelManager::new(config);

        assert!(manager.get_session(99999).is_none());
    }

    #[test]
    fn test_remove_session() {
        let config = MultichannelConfig::new().with_enabled(true);
        let mut manager = MultichannelManager::new(config);

        manager.create_session(12345);
        assert!(manager.remove_session(12345));
        assert!(manager.get_session(12345).is_none());
    }

    #[test]
    fn test_remove_nonexistent_session() {
        let config = MultichannelConfig::new().with_enabled(true);
        let mut manager = MultichannelManager::new(config);

        assert!(!manager.remove_session(99999));
    }

    #[test]
    fn test_session_count() {
        let config = MultichannelConfig::new().with_enabled(true);
        let mut manager = MultichannelManager::new(config);

        assert_eq!(manager.session_count(), 0);
        manager.create_session(1);
        manager.create_session(2);
        assert_eq!(manager.session_count(), 2);
        manager.remove_session(1);
        assert_eq!(manager.session_count(), 1);
    }

    #[test]
    fn test_total_channel_count() {
        let config = MultichannelConfig::new().with_enabled(true);
        let mut manager = MultichannelManager::new(config);

        let mut session1 = manager.create_session(1);
        session1.add_channel(ChannelInfo {
            channel_id: 0,
            interface: "eth0".to_string(),
            role: MultichannelRole::Primary,
            is_active: true,
            bytes_sent: 100,
            bytes_received: 200,
        });
        session1.add_channel(ChannelInfo {
            channel_id: 1,
            interface: "eth1".to_string(),
            role: MultichannelRole::Secondary,
            is_active: true,
            bytes_sent: 150,
            bytes_received: 250,
        });
        *manager.sessions.get_mut(&1).unwrap() = session1;

        let mut session2 = manager.create_session(2);
        session2.add_channel(ChannelInfo {
            channel_id: 0,
            interface: "eth0".to_string(),
            role: MultichannelRole::Primary,
            is_active: true,
            bytes_sent: 50,
            bytes_received: 75,
        });
        *manager.sessions.get_mut(&2).unwrap() = session2;

        assert_eq!(manager.total_channel_count(), 3);
    }

    #[test]
    fn test_multichannel_session_stats() {
        let mut session = MultichannelSession::new(1);

        session.add_channel(ChannelInfo {
            channel_id: 0,
            interface: "eth0".to_string(),
            role: MultichannelRole::Primary,
            is_active: true,
            bytes_sent: 0,
            bytes_received: 0,
        });

        session.update_stats(0, 1000, 2000);

        assert_eq!(session.total_bytes_sent, 1000);
        assert_eq!(session.total_bytes_received, 2000);

        let channel = &session.channels[0];
        assert_eq!(channel.bytes_sent, 1000);
        assert_eq!(channel.bytes_received, 2000);
    }

    #[test]
    fn test_interface_capabilities_default() {
        let caps = InterfaceCapabilities::default();
        assert!(!caps.rdma);
        assert!(!caps.rss);
        assert!(!caps.tso);
        assert!(!caps.checksum_offload);
    }

    #[test]
    fn test_nic_capabilities_builder() {
        let nic = NicCapabilities::new("eth0".to_string(), "192.168.1.1".to_string())
            .with_speed(10000)
            .with_port(8445)
            .with_rdma()
            .with_rss()
            .with_tso()
            .with_checksum_offload()
            .with_enabled(true);

        assert_eq!(nic.interface_name, "eth0");
        assert_eq!(nic.ip_address, "192.168.1.1");
        assert_eq!(nic.port, 8445);
        assert_eq!(nic.link_speed_mbps, 10000);
        assert!(nic.capabilities.rdma);
        assert!(nic.capabilities.rss);
        assert!(nic.capabilities.tso);
        assert!(nic.capabilities.checksum_offload);
        assert!(nic.enabled);
    }

    #[test]
    fn test_no_interfaces_available() {
        let config = MultichannelConfig::new().with_enabled(true);
        let mut manager = MultichannelManager::new(config);

        manager
            .add_interface(create_test_interface("eth0", 1000, false, false))
            .unwrap();

        let selected = manager.select_interfaces_for_client(3);
        assert!(selected.is_empty());
    }
}
