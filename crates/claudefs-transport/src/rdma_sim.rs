//! RDMA simulation for testing without hardware.
//!
//! This module provides a software simulation of RDMA semantics for testing
//! the transport layer without requiring InfiniBand or RoCE hardware.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

static MR_COUNTER: AtomicU64 = AtomicU64::new(1);
static QP_COUNTER: AtomicU64 = AtomicU64::new(1);
static CQ_COUNTER: AtomicU64 = AtomicU64::new(1);
static PD_COUNTER: AtomicU64 = AtomicU64::new(1);
static FABRIC_COUNTER: AtomicU64 = AtomicU64::new(1);
static WR_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessFlags(u32);

impl AccessFlags {
    pub const LOCAL_READ: Self = Self(1);
    pub const LOCAL_WRITE: Self = Self(2);
    pub const REMOTE_READ: Self = Self(4);
    pub const REMOTE_WRITE: Self = Self(8);
    pub const ALL: Self = Self(0xF);

    pub fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    pub fn empty() -> Self {
        Self(0)
    }
}

impl std::ops::BitOr for AccessFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl Default for AccessFlags {
    fn default() -> Self {
        Self::empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QpState {
    Reset,
    Init,
    ReadyToReceive,
    ReadyToSend,
    Error,
}

impl Default for QpState {
    fn default() -> Self {
        Self::Reset
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WrOpcode {
    Send,
    Recv,
    RdmaRead,
    RdmaWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompletionStatus {
    Success,
    LocalError,
    RemoteError,
    Timeout,
    Cancelled,
}

impl Default for CompletionStatus {
    fn default() -> Self {
        Self::Success
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkRequestStatus {
    Pending,
    Completed,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScatterGatherElement {
    pub address: u64,
    pub length: u32,
    pub lkey: u32,
    pub rkey: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkRequest {
    pub id: u64,
    pub opcode: WrOpcode,
    pub sg_list: Vec<ScatterGatherElement>,
    pub remote_addr: Option<u64>,
    pub remote_rkey: Option<u32>,
    pub status: WorkRequestStatus,
    pub bytes_transferred: u32,
}

impl WorkRequest {
    fn new(opcode: WrOpcode) -> Self {
        Self {
            id: WR_COUNTER.fetch_add(1, Ordering::Relaxed),
            opcode,
            sg_list: Vec::new(),
            remote_addr: None,
            remote_rkey: None,
            status: WorkRequestStatus::Pending,
            bytes_transferred: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionEntry {
    pub wr_id: u64,
    pub status: CompletionStatus,
    pub opcode: WrOpcode,
    pub bytes_transferred: u32,
    pub qp_num: u32,
}

impl CompletionEntry {
    fn new(
        wr_id: u64,
        status: CompletionStatus,
        opcode: WrOpcode,
        bytes_transferred: u32,
        qp_num: u32,
    ) -> Self {
        Self {
            wr_id,
            status,
            opcode,
            bytes_transferred,
            qp_num,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MemoryRegion {
    pub id: u64,
    buffer: Arc<Mutex<Vec<u8>>>,
    pub access_flags: AccessFlags,
    pub remote_key: u32,
    pub length: usize,
}

impl MemoryRegion {
    pub fn new(buffer: Vec<u8>, access_flags: AccessFlags) -> Self {
        let length = buffer.len();
        let remote_key = (MR_COUNTER.fetch_add(1, Ordering::Relaxed) as u32) | 0x80000000;
        Self {
            id: MR_COUNTER.load(Ordering::Relaxed) - 1,
            buffer: Arc::new(Mutex::new(buffer)),
            access_flags,
            remote_key,
            length,
        }
    }

    pub fn local_key(&self) -> u32 {
        self.id as u32 | 0x40000000
    }

    pub fn read_at(&self, offset: usize, len: usize) -> Option<Vec<u8>> {
        if !self.access_flags.contains(AccessFlags::LOCAL_READ)
            && !self.access_flags.contains(AccessFlags::REMOTE_READ)
        {
            return None;
        }
        let buf = self.buffer.lock().ok()?;
        let end = offset.saturating_add(len);
        if end > buf.len() {
            return None;
        }
        Some(buf[offset..end].to_vec())
    }

    pub fn write_at(&self, offset: usize, data: &[u8]) -> bool {
        if !self.access_flags.contains(AccessFlags::LOCAL_WRITE)
            && !self.access_flags.contains(AccessFlags::REMOTE_WRITE)
        {
            return false;
        }
        let mut buf = match self.buffer.lock() {
            Ok(b) => b,
            Err(_) => return false,
        };
        let end = offset.saturating_add(data.len());
        if end > buf.len() {
            return false;
        }
        buf[offset..end].copy_from_slice(data);
        true
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectionDomain {
    pub id: u64,
    pub name: String,
}

impl ProtectionDomain {
    pub fn new(name: &str) -> Self {
        Self {
            id: PD_COUNTER.fetch_add(1, Ordering::Relaxed),
            name: name.to_string(),
        }
    }
}

pub struct CompletionQueue {
    pub id: u64,
    entries: Arc<Mutex<VecDeque<CompletionEntry>>>,
    pub capacity: usize,
}

impl CompletionQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            id: CQ_COUNTER.fetch_add(1, Ordering::Relaxed),
            entries: Arc::new(Mutex::new(VecDeque::new())),
            capacity,
        }
    }

    pub fn push(&self, entry: CompletionEntry) -> bool {
        let mut entries = match self.entries.lock() {
            Ok(e) => e,
            Err(_) => return false,
        };
        if entries.len() >= self.capacity {
            return false;
        }
        entries.push_back(entry);
        true
    }

    pub fn poll(&self) -> Option<CompletionEntry> {
        let mut entries = match self.entries.lock() {
            Ok(e) => e,
            Err(_) => return None,
        };
        entries.pop_front()
    }

    pub fn poll_all(&self) -> Vec<CompletionEntry> {
        let mut entries = match self.entries.lock() {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };
        entries.drain(..).collect()
    }
}

pub struct QueuePair {
    pub qp_num: u32,
    pub state: QpState,
    pub pd: Arc<ProtectionDomain>,
    pub send_cq: Arc<CompletionQueue>,
    pub recv_cq: Arc<CompletionQueue>,
    send_queue: Arc<Mutex<VecDeque<WorkRequest>>>,
    recv_queue: Arc<Mutex<VecDeque<WorkRequest>>>,
    pub remote_qp_num: Option<u32>,
    pub remote_addr: Option<u64>,
    pub remote_rkey: Option<u32>,
}

impl QueuePair {
    pub fn new(
        pd: Arc<ProtectionDomain>,
        send_cq: Arc<CompletionQueue>,
        recv_cq: Arc<CompletionQueue>,
    ) -> Self {
        Self {
            qp_num: QP_COUNTER.fetch_add(1, Ordering::Relaxed) as u32,
            state: QpState::Reset,
            pd,
            send_cq,
            recv_cq,
            send_queue: Arc::new(Mutex::new(VecDeque::new())),
            recv_queue: Arc::new(Mutex::new(VecDeque::new())),
            remote_qp_num: None,
            remote_addr: None,
            remote_rkey: None,
        }
    }

    pub fn modify_to_init(&mut self) -> bool {
        if self.state != QpState::Reset {
            return false;
        }
        self.state = QpState::Init;
        true
    }

    pub fn modify_to_rtr(
        &mut self,
        remote_qp_num: u32,
        remote_addr: u64,
        remote_rkey: u32,
    ) -> bool {
        if self.state != QpState::Init {
            return false;
        }
        self.remote_qp_num = Some(remote_qp_num);
        self.remote_addr = Some(remote_addr);
        self.remote_rkey = Some(remote_rkey);
        self.state = QpState::ReadyToReceive;
        true
    }

    pub fn modify_to_rts(&mut self) -> bool {
        if self.state != QpState::ReadyToReceive {
            return false;
        }
        self.state = QpState::ReadyToSend;
        true
    }

    pub fn post_send(&self, wr: WorkRequest) -> bool {
        if self.state != QpState::ReadyToSend && self.state != QpState::ReadyToReceive {
            return false;
        }
        let mut queue = match self.send_queue.lock() {
            Ok(q) => q,
            Err(_) => return false,
        };
        queue.push_back(wr);
        true
    }

    pub fn post_recv(&self, wr: WorkRequest) -> bool {
        if self.state != QpState::ReadyToReceive
            && self.state != QpState::ReadyToSend
            && self.state != QpState::Init
        {
            return false;
        }
        let mut queue = match self.recv_queue.lock() {
            Ok(q) => q,
            Err(_) => return false,
        };
        queue.push_back(wr);
        true
    }

    pub fn get_send_wr(&self) -> Option<WorkRequest> {
        let mut queue = match self.send_queue.lock() {
            Ok(q) => q,
            Err(_) => return None,
        };
        queue.pop_front()
    }

    pub fn get_recv_wr(&self) -> Option<WorkRequest> {
        let mut queue = match self.recv_queue.lock() {
            Ok(q) => q,
            Err(_) => return None,
        };
        queue.pop_front()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulatedFabricConfig {
    pub latency_ns: u64,
    pub failure_rate: f64,
    pub max_queue_depth: usize,
}

impl Default for SimulatedFabricConfig {
    fn default() -> Self {
        Self {
            latency_ns: 1000,
            failure_rate: 0.0,
            max_queue_depth: 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FabricStats {
    pub operations_completed: u64,
    pub operations_failed: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

pub struct SimulatedFabric {
    id: u64,
    config: SimulatedFabricConfig,
    queue_pairs: Arc<Mutex<HashMap<u32, Arc<Mutex<QueuePair>>>>>,
    memory_regions: Arc<Mutex<HashMap<u32, Arc<MemoryRegion>>>>,
    stats: Arc<Mutex<FabricStats>>,
}

impl SimulatedFabric {
    pub fn new(config: SimulatedFabricConfig) -> Self {
        Self {
            id: FABRIC_COUNTER.fetch_add(1, Ordering::Relaxed),
            config,
            queue_pairs: Arc::new(Mutex::new(HashMap::new())),
            memory_regions: Arc::new(Mutex::new(HashMap::new())),
            stats: Arc::new(Mutex::new(FabricStats::default())),
        }
    }

    pub fn register_qp(&self, qp: Arc<Mutex<QueuePair>>) {
        let qp_num = {
            let qp_lock = qp.lock().unwrap();
            qp_lock.qp_num
        };
        let mut qps = self.queue_pairs.lock().unwrap();
        qps.insert(qp_num, qp);
    }

    pub fn register_memory_region(&self, mr: Arc<MemoryRegion>) {
        let mut mrs = self.memory_regions.lock().unwrap();
        mrs.insert(mr.remote_key, mr);
    }

    pub fn execute_rdma_write(
        &self,
        src_mr: &MemoryRegion,
        dst_mr_key: u32,
        dst_offset: usize,
        len: usize,
    ) -> Result<u32, String> {
        if !src_mr.access_flags.contains(AccessFlags::LOCAL_READ) {
            return Err("Source MR lacks LOCAL_READ".to_string());
        }

        if self.should_fail() {
            return Err("Simulated failure".to_string());
        }

        let mrs = self.memory_regions.lock().unwrap();
        let dst_mr = mrs.get(&dst_mr_key).ok_or("Destination MR not found")?;

        if !dst_mr.access_flags.contains(AccessFlags::REMOTE_WRITE) {
            return Err("Destination MR lacks REMOTE_WRITE".to_string());
        }

        let data = src_mr.read_at(0, len).ok_or("Failed to read source")?;
        if !dst_mr.write_at(dst_offset, &data) {
            return Err("Failed to write destination".to_string());
        }

        let mut stats = self.stats.lock().unwrap();
        stats.operations_completed += 1;
        stats.bytes_sent += len as u64;

        Ok(len as u32)
    }

    pub fn execute_rdma_read(
        &self,
        dst_mr: &MemoryRegion,
        src_mr_key: u32,
        src_offset: usize,
        len: usize,
    ) -> Result<u32, String> {
        if !dst_mr.access_flags.contains(AccessFlags::LOCAL_WRITE) {
            return Err("Destination MR lacks LOCAL_WRITE".to_string());
        }

        if self.should_fail() {
            return Err("Simulated failure".to_string());
        }

        let mrs = self.memory_regions.lock().unwrap();
        let src_mr = mrs.get(&src_mr_key).ok_or("Source MR not found")?;

        if !src_mr.access_flags.contains(AccessFlags::REMOTE_READ) {
            return Err("Source MR lacks REMOTE_READ".to_string());
        }

        let data = src_mr
            .read_at(src_offset, len)
            .ok_or("Failed to read source")?;
        if !dst_mr.write_at(0, &data) {
            return Err("Failed to write destination".to_string());
        }

        let mut stats = self.stats.lock().unwrap();
        stats.operations_completed += 1;
        stats.bytes_received += len as u64;

        Ok(len as u32)
    }

    pub fn execute_send(&self, src_data: &[u8], _qp: &QueuePair) -> Result<u32, String> {
        if self.should_fail() {
            return Err("Simulated failure".to_string());
        }

        let mut stats = self.stats.lock().unwrap();
        stats.operations_completed += 1;
        stats.bytes_sent += src_data.len() as u64;

        Ok(src_data.len() as u32)
    }

    pub fn execute_recv(&self, dst_buf: &mut [u8], _qp: &QueuePair) -> Result<u32, String> {
        if self.should_fail() {
            return Err("Simulated failure".to_string());
        }

        let mut stats = self.stats.lock().unwrap();
        stats.operations_completed += 1;
        stats.bytes_received += dst_buf.len() as u64;

        Ok(dst_buf.len() as u32)
    }

    fn should_fail(&self) -> bool {
        if self.config.failure_rate <= 0.0 {
            return false;
        }
        let seed = rand_simple();
        (seed as f64 / u32::MAX as f64) < self.config.failure_rate
    }

    pub fn get_stats(&self) -> FabricStats {
        self.stats.lock().unwrap().clone()
    }

    pub fn config(&self) -> &SimulatedFabricConfig {
        &self.config
    }
}

fn rand_simple() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    nanos.wrapping_mul(1103515245).wrapping_add(12345)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_access_flags_basic() {
        let flags = AccessFlags::LOCAL_READ | AccessFlags::LOCAL_WRITE;
        assert!(flags.contains(AccessFlags::LOCAL_READ));
        assert!(flags.contains(AccessFlags::LOCAL_WRITE));
        assert!(!flags.contains(AccessFlags::REMOTE_READ));
    }

    #[test]
    fn test_access_flags_all() {
        let all = AccessFlags::ALL;
        assert!(all.contains(AccessFlags::LOCAL_READ));
        assert!(all.contains(AccessFlags::LOCAL_WRITE));
        assert!(all.contains(AccessFlags::REMOTE_READ));
        assert!(all.contains(AccessFlags::REMOTE_WRITE));
    }

    #[test]
    fn test_access_flags_empty() {
        let empty = AccessFlags::empty();
        assert!(!empty.contains(AccessFlags::LOCAL_READ));
        assert!(!empty.contains(AccessFlags::LOCAL_WRITE));
    }

    #[test]
    fn test_memory_region_create() {
        let buffer = vec![0u8; 1024];
        let mr = MemoryRegion::new(buffer, AccessFlags::ALL);
        assert!(mr.id > 0);
        assert_eq!(mr.length, 1024);
        assert!(mr.access_flags.contains(AccessFlags::LOCAL_READ));
    }

    #[test]
    fn test_memory_region_read_write() {
        let buffer = vec![0u8; 1024];
        let mr = MemoryRegion::new(buffer, AccessFlags::ALL);

        let data = vec![1u8, 2, 3, 4];
        assert!(mr.write_at(100, &data));

        let read = mr.read_at(100, 4).unwrap();
        assert_eq!(read, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_memory_region_read_only() {
        let buffer = vec![0u8; 1024];
        let mr = MemoryRegion::new(buffer, AccessFlags::REMOTE_READ);

        assert!(!mr.write_at(0, &[1, 2, 3]));
        assert!(mr.read_at(0, 4).is_some());
    }

    #[test]
    fn test_memory_region_write_only() {
        let buffer = vec![0u8; 1024];
        let mr = MemoryRegion::new(buffer, AccessFlags::LOCAL_WRITE);

        assert!(mr.write_at(0, &[1, 2, 3]));
        assert!(mr.read_at(0, 4).is_none());
    }

    #[test]
    fn test_protection_domain() {
        let pd = ProtectionDomain::new("test-pd");
        assert!(pd.id > 0);
        assert_eq!(pd.name, "test-pd");
    }

    #[test]
    fn test_completion_queue() {
        let cq = CompletionQueue::new(10);
        assert!(cq.push(CompletionEntry::new(
            1,
            CompletionStatus::Success,
            WrOpcode::Send,
            100,
            1
        )));

        let entry = cq.poll().unwrap();
        assert_eq!(entry.wr_id, 1);
        assert_eq!(entry.status, CompletionStatus::Success);
    }

    #[test]
    fn test_completion_queue_full() {
        let cq = CompletionQueue::new(2);
        assert!(cq.push(CompletionEntry::new(
            1,
            CompletionStatus::Success,
            WrOpcode::Send,
            100,
            1
        )));
        assert!(cq.push(CompletionEntry::new(
            2,
            CompletionStatus::Success,
            WrOpcode::Send,
            100,
            1
        )));
        assert!(!cq.push(CompletionEntry::new(
            3,
            CompletionStatus::Success,
            WrOpcode::Send,
            100,
            1
        )));
    }

    #[test]
    fn test_completion_queue_poll_all() {
        let cq = CompletionQueue::new(10);
        cq.push(CompletionEntry::new(
            1,
            CompletionStatus::Success,
            WrOpcode::Send,
            100,
            1,
        ));
        cq.push(CompletionEntry::new(
            2,
            CompletionStatus::Success,
            WrOpcode::Recv,
            200,
            1,
        ));

        let entries = cq.poll_all();
        assert_eq!(entries.len(), 2);
        assert!(cq.poll().is_none());
    }

    #[test]
    fn test_queue_pair_create() {
        let pd = Arc::new(ProtectionDomain::new("test"));
        let send_cq = Arc::new(CompletionQueue::new(100));
        let recv_cq = Arc::new(CompletionQueue::new(100));

        let qp = QueuePair::new(pd, send_cq.clone(), recv_cq.clone());

        assert_eq!(qp.state, QpState::Reset);
        assert!(qp.qp_num > 0);
    }

    #[test]
    fn test_queue_pair_state_transitions() {
        let pd = Arc::new(ProtectionDomain::new("test"));
        let send_cq = Arc::new(CompletionQueue::new(100));
        let recv_cq = Arc::new(CompletionQueue::new(100));

        let mut qp = QueuePair::new(pd, send_cq.clone(), recv_cq.clone());

        assert!(qp.modify_to_init());
        assert_eq!(qp.state, QpState::Init);

        assert!(qp.modify_to_rtr(42, 0x1000, 0x12345678));
        assert_eq!(qp.state, QpState::ReadyToReceive);
        assert_eq!(qp.remote_qp_num, Some(42));

        assert!(qp.modify_to_rts());
        assert_eq!(qp.state, QpState::ReadyToSend);
    }

    #[test]
    fn test_queue_pair_invalid_transitions() {
        let pd = Arc::new(ProtectionDomain::new("test"));
        let send_cq = Arc::new(CompletionQueue::new(100));
        let recv_cq = Arc::new(CompletionQueue::new(100));

        let mut qp = QueuePair::new(pd, send_cq.clone(), recv_cq.clone());

        assert!(!qp.modify_to_rtr(1, 0, 0));
        assert!(!qp.modify_to_rts());

        assert!(qp.modify_to_init());
        assert!(!qp.modify_to_init());
    }

    #[test]
    fn test_queue_pair_post_send_recv() {
        let pd = Arc::new(ProtectionDomain::new("test"));
        let send_cq = Arc::new(CompletionQueue::new(100));
        let recv_cq = Arc::new(CompletionQueue::new(100));

        let mut qp = QueuePair::new(pd, send_cq.clone(), recv_cq.clone());

        qp.modify_to_init();
        qp.modify_to_rtr(1, 0, 0);
        qp.modify_to_rts();

        let mut wr = WorkRequest::new(WrOpcode::Send);
        wr.sg_list.push(ScatterGatherElement {
            address: 0,
            length: 100,
            lkey: 1,
            rkey: 2,
        });

        assert!(qp.post_send(wr.clone()));

        let mut wr_recv = WorkRequest::new(WrOpcode::Recv);
        wr_recv.sg_list.push(ScatterGatherElement {
            address: 0,
            length: 100,
            lkey: 1,
            rkey: 2,
        });

        assert!(qp.post_recv(wr_recv));
    }

    #[test]
    fn test_queue_pair_post_in_wrong_state() {
        let pd = Arc::new(ProtectionDomain::new("test"));
        let send_cq = Arc::new(CompletionQueue::new(100));
        let recv_cq = Arc::new(CompletionQueue::new(100));

        let qp = QueuePair::new(pd, send_cq.clone(), recv_cq.clone());

        let wr = WorkRequest::new(WrOpcode::Send);
        assert!(!qp.post_send(wr));
    }

    #[test]
    fn test_fabric_config_defaults() {
        let config = SimulatedFabricConfig::default();
        assert_eq!(config.latency_ns, 1000);
        assert_eq!(config.failure_rate, 0.0);
        assert_eq!(config.max_queue_depth, 1024);
    }

    #[test]
    fn test_simulated_fabric_register() {
        let fabric = SimulatedFabric::new(SimulatedFabricConfig::default());

        let pd = Arc::new(ProtectionDomain::new("test"));
        let send_cq = Arc::new(CompletionQueue::new(100));
        let recv_cq = Arc::new(CompletionQueue::new(100));

        let qp = Arc::new(Mutex::new(QueuePair::new(
            pd,
            send_cq.clone(),
            recv_cq.clone(),
        )));
        fabric.register_qp(qp.clone());

        let mr = Arc::new(MemoryRegion::new(vec![0u8; 1024], AccessFlags::ALL));
        fabric.register_memory_region(mr.clone());

        assert_eq!(fabric.get_stats().operations_completed, 0);
    }

    #[test]
    fn test_fabric_rdma_write() {
        let fabric = SimulatedFabric::new(SimulatedFabricConfig::default());

        let src_buffer = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let src_mr = Arc::new(MemoryRegion::new(src_buffer, AccessFlags::LOCAL_READ));
        fabric.register_memory_region(src_mr.clone());

        let dst_buffer = vec![0u8; 1024];
        let dst_mr = Arc::new(MemoryRegion::new(
            dst_buffer,
            AccessFlags::REMOTE_WRITE | AccessFlags::LOCAL_READ,
        ));
        fabric.register_memory_region(dst_mr.clone());

        let result = fabric.execute_rdma_write(src_mr.as_ref(), dst_mr.remote_key, 0, 8);
        assert!(result.is_ok(), "Error: {:?}", result.err());
        assert_eq!(result.unwrap(), 8);

        let stats = fabric.get_stats();
        assert_eq!(stats.operations_completed, 1);
        assert_eq!(stats.bytes_sent, 8);
    }

    #[test]
    fn test_fabric_rdma_write_no_permission() {
        let fabric = SimulatedFabric::new(SimulatedFabricConfig::default());

        let src_buffer = vec![1, 2, 3, 4];
        let src_mr = Arc::new(MemoryRegion::new(src_buffer, AccessFlags::LOCAL_READ));
        fabric.register_memory_region(src_mr.clone());

        let dst_buffer = vec![0u8; 1024];
        let dst_mr = Arc::new(MemoryRegion::new(dst_buffer, AccessFlags::REMOTE_READ));
        fabric.register_memory_region(dst_mr.clone());

        let result = fabric.execute_rdma_write(src_mr.as_ref(), dst_mr.remote_key, 0, 4);
        assert!(result.is_err());

        let stats = fabric.get_stats();
        assert_eq!(stats.operations_failed, 0);
    }

    #[test]
    fn test_fabric_rdma_read() {
        let fabric = SimulatedFabric::new(SimulatedFabricConfig::default());

        let src_buffer = vec![9, 8, 7, 6, 5, 4, 3, 2, 1, 0];
        let src_mr = Arc::new(MemoryRegion::new(src_buffer, AccessFlags::REMOTE_READ));
        fabric.register_memory_region(src_mr.clone());

        let dst_buffer = vec![0u8; 1024];
        let dst_mr = Arc::new(MemoryRegion::new(
            dst_buffer,
            AccessFlags::LOCAL_WRITE | AccessFlags::LOCAL_READ,
        ));
        fabric.register_memory_region(dst_mr.clone());

        let result = fabric.execute_rdma_read(dst_mr.as_ref(), src_mr.remote_key, 2, 6);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 6);

        let read_data = dst_mr.read_at(0, 6).unwrap();
        assert_eq!(read_data, vec![7, 6, 5, 4, 3, 2]);

        let stats = fabric.get_stats();
        assert_eq!(stats.operations_completed, 1);
        assert_eq!(stats.bytes_received, 6);
    }

    #[test]
    fn test_fabric_rdma_read_no_permission() {
        let fabric = SimulatedFabric::new(SimulatedFabricConfig::default());

        let src_buffer = vec![1, 2, 3, 4];
        let src_mr = Arc::new(MemoryRegion::new(src_buffer, AccessFlags::LOCAL_WRITE));
        fabric.register_memory_region(src_mr.clone());

        let dst_buffer = vec![0u8; 1024];
        let dst_mr = Arc::new(MemoryRegion::new(dst_buffer, AccessFlags::LOCAL_WRITE));
        fabric.register_memory_region(dst_mr.clone());

        let result = fabric.execute_rdma_read(dst_mr.as_ref(), src_mr.remote_key, 0, 4);
        assert!(result.is_err());
    }

    #[test]
    fn test_fabric_send_recv() {
        let fabric = SimulatedFabric::new(SimulatedFabricConfig::default());

        let pd = Arc::new(ProtectionDomain::new("test"));
        let send_cq = Arc::new(CompletionQueue::new(100));
        let recv_cq = Arc::new(CompletionQueue::new(100));

        let qp = QueuePair::new(pd, send_cq, recv_cq);

        let data = vec![1u8, 2, 3, 4, 5];
        let result = fabric.execute_send(&data, &qp);
        assert!(result.is_ok());

        let mut recv_buf = vec![0u8; 100];
        let result = fabric.execute_recv(&mut recv_buf, &qp);
        assert!(result.is_ok());
    }

    #[test]
    fn test_fabric_error_injection() {
        let config = SimulatedFabricConfig {
            latency_ns: 0,
            failure_rate: 1.0,
            max_queue_depth: 1024,
        };

        let fabric = SimulatedFabric::new(config);

        let pd = Arc::new(ProtectionDomain::new("test"));
        let send_cq = Arc::new(CompletionQueue::new(100));
        let recv_cq = Arc::new(CompletionQueue::new(100));

        let qp = QueuePair::new(pd, send_cq, recv_cq);

        let result = fabric.execute_send(&[1, 2, 3], &qp);
        assert!(result.is_err());

        let stats = fabric.get_stats();
        assert_eq!(stats.operations_failed, 0);
    }

    #[test]
    fn test_work_request_id_unique() {
        let wr1 = WorkRequest::new(WrOpcode::Send);
        let wr2 = WorkRequest::new(WrOpcode::Recv);
        let wr3 = WorkRequest::new(WrOpcode::RdmaRead);

        assert_ne!(wr1.id, wr2.id);
        assert_ne!(wr2.id, wr3.id);
    }

    #[test]
    fn test_completion_entry_fields() {
        let entry = CompletionEntry::new(
            42,
            CompletionStatus::RemoteError,
            WrOpcode::RdmaWrite,
            1024,
            7,
        );

        assert_eq!(entry.wr_id, 42);
        assert_eq!(entry.status, CompletionStatus::RemoteError);
        assert_eq!(entry.opcode, WrOpcode::RdmaWrite);
        assert_eq!(entry.bytes_transferred, 1024);
        assert_eq!(entry.qp_num, 7);
    }

    #[test]
    fn test_multiple_mrs_isolation() {
        let mr1 = MemoryRegion::new(vec![1, 2, 3, 4], AccessFlags::ALL);
        let mr2 = MemoryRegion::new(vec![5, 6, 7, 8], AccessFlags::ALL);

        assert_ne!(mr1.remote_key, mr2.remote_key);
        assert_ne!(mr1.local_key(), mr2.local_key());

        mr1.write_at(0, &[9, 9, 9, 9]);

        let data1 = mr1.read_at(0, 4).unwrap();
        let data2 = mr2.read_at(0, 4).unwrap();

        assert_eq!(data1, vec![9, 9, 9, 9]);
        assert_eq!(data2, vec![5, 6, 7, 8]);
    }

    #[test]
    fn test_qp_get_send_recv_wr() {
        let pd = Arc::new(ProtectionDomain::new("test"));
        let send_cq = Arc::new(CompletionQueue::new(100));
        let recv_cq = Arc::new(CompletionQueue::new(100));

        let mut qp = QueuePair::new(pd, send_cq.clone(), recv_cq.clone());

        qp.modify_to_init();
        qp.modify_to_rtr(1, 0, 0);

        let wr1 = WorkRequest::new(WrOpcode::Send);
        let wr2 = WorkRequest::new(WrOpcode::Recv);

        qp.post_send(wr1);
        qp.post_recv(wr2);

        let send_wr = qp.get_send_wr();
        let recv_wr = qp.get_recv_wr();

        assert!(send_wr.is_some());
        assert!(recv_wr.is_some());
        assert_eq!(send_wr.unwrap().opcode, WrOpcode::Send);
        assert_eq!(recv_wr.unwrap().opcode, WrOpcode::Recv);
    }

    #[test]
    fn test_memory_region_out_of_bounds() {
        let buffer = vec![0u8; 100];
        let mr = MemoryRegion::new(buffer, AccessFlags::ALL);

        assert!(mr.read_at(50, 60).is_none());
        assert!(!mr.write_at(50, &[1; 60]));
        assert!(mr.read_at(0, 100).is_some());
    }

    #[test]
    fn test_fabric_stats_default() {
        let stats = FabricStats::default();
        assert_eq!(stats.operations_completed, 0);
        assert_eq!(stats.operations_failed, 0);
        assert_eq!(stats.bytes_sent, 0);
        assert_eq!(stats.bytes_received, 0);
    }
}
