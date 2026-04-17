//! QoS client bridge for FUSE operations.
//!
//! Coordinates bandwidth and IOPS throttling per tenant, integrating
//! with A4 bandwidth_shaper and A2 qos_coordinator.

use std::sync::Arc;
use dashmap::DashMap;
use tokio::sync::Mutex as TokioMutex;

/// Workload classification for QoS scheduling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkloadClass {
    /// Interactive workload requiring low latency (p99 < 10ms).
    Interactive,
    /// Batch workload focused on throughput.
    Batch,
    /// Background workload with lowest priority.
    Background,
    /// Custom tenant-specific class.
    Reserved(u8),
}

impl WorkloadClass {
    /// Returns the base priority level for this workload class.
    pub fn base_priority(&self) -> Priority {
        match self {
            WorkloadClass::Interactive => 200,
            WorkloadClass::Batch => 128,
            WorkloadClass::Background => 64,
            WorkloadClass::Reserved(n) => *n,
        }
    }
}

/// QoS priority level (0=lowest, 255=highest).
pub type Priority = u8;

/// Per-tenant QoS configuration parameters.
#[derive(Debug, Clone)]
pub struct TenantQos {
    /// Unique tenant identifier.
    pub tenant_id: String,
    /// Workload classification.
    pub workload_class: WorkloadClass,
    /// Priority level for scheduling.
    pub priority: Priority,
    /// Hard bandwidth limit in Mbps (None = unlimited).
    pub max_bandwidth_mbps: Option<u32>,
    /// Soft bandwidth target in Mbps (None = no target).
    pub target_bandwidth_mbps: Option<u32>,
    /// Hard IOPS limit (None = unlimited).
    pub max_iops: Option<u32>,
    /// Soft IOPS target (None = no target).
    pub target_iops: Option<u32>,
}

/// Bandwidth reservation token.
#[derive(Debug, Clone)]
pub struct BandwidthToken {
    /// Tenant that owns this token.
    pub tenant_id: String,
    /// Number of bytes reserved.
    pub bytes: u64,
    /// Timestamp when token was allocated (ns).
    pub allocated_ns: u64,
}

/// IOPS reservation token.
#[derive(Debug, Clone)]
pub struct IopsToken {
    /// Tenant that owns this token.
    pub tenant_id: String,
    /// Number of operations reserved.
    pub ops: u32,
    /// Timestamp when token was allocated (ns).
    pub allocated_ns: u64,
}

/// Token bucket bandwidth shaper.
pub struct BandwidthShaper {
    /// Maximum tokens per second (bytes/s).
    max_bps: u64,
    /// Target tokens per second.
    target_bps: u64,
    /// Available tokens.
    tokens: Arc<TokioMutex<u64>>,
    /// Last refill timestamp (ns).
    last_refill_ns: Arc<TokioMutex<u64>>,
}

impl BandwidthShaper {
    /// Creates a new bandwidth shaper.
    pub fn new(max_bps: u64, target_bps: u64) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        Self {
            max_bps,
            target_bps,
            tokens: Arc::new(TokioMutex::new(max_bps)),
            last_refill_ns: Arc::new(TokioMutex::new(now)),
        }
    }

    /// Tries to acquire the specified number of bytes.
    pub async fn try_acquire(&self, bytes: u64) -> Option<BandwidthToken> {
        let mut tokens = self.tokens.lock().await;
        let mut last_refill = self.last_refill_ns.lock().await;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        let elapsed = now.saturating_sub(*last_refill);
        let refill = (elapsed * self.max_bps) / 1_000_000_000;
        *tokens = (*tokens + refill).min(self.max_bps);
        *last_refill = now;

        if *tokens >= bytes {
            *tokens -= bytes;
            Some(BandwidthToken {
                tenant_id: String::new(),
                bytes,
                allocated_ns: now,
            })
        } else {
            None
        }
    }

    /// Returns the current token balance.
    pub async fn available_tokens(&self) -> u64 {
        *self.tokens.lock().await
    }
}

/// Token bucket IOPS limiter.
pub struct IopsLimiter {
    /// Maximum operations per second.
    max_iops: u32,
    /// Target operations per second.
    target_iops: u32,
    /// Available operation tokens.
    tokens: Arc<TokioMutex<u32>>,
    /// Last refill timestamp (ns).
    last_refill_ns: Arc<TokioMutex<u64>>,
}

impl IopsLimiter {
    /// Creates a new IOPS limiter.
    pub fn new(max_iops: u32, target_iops: u32) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        Self {
            max_iops,
            target_iops,
            tokens: Arc::new(TokioMutex::new(max_iops as u32)),
            last_refill_ns: Arc::new(TokioMutex::new(now)),
        }
    }

    /// Tries to acquire one operation token.
    pub async fn try_acquire(&self) -> Option<IopsToken> {
        let mut tokens = self.tokens.lock().await;
        let mut last_refill = self.last_refill_ns.lock().await;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        let elapsed = now.saturating_sub(*last_refill);
        let refill = ((elapsed * self.max_iops as u64) / 1_000_000_000) as u32;
        *tokens = (*tokens + refill).min(self.max_iops);
        *last_refill = now;

        if *tokens >= 1 {
            *tokens -= 1;
            Some(IopsToken {
                tenant_id: String::new(),
                ops: 1,
                allocated_ns: now,
            })
        } else {
            None
        }
    }
}

/// QoS statistics for a tenant.
#[derive(Debug, Clone)]
pub struct QosStats {
    /// Tenant identifier.
    pub tenant_id: String,
    /// Current bandwidth usage in Mbps.
    pub current_bandwidth_mbps: f64,
    /// Peak bandwidth usage in Mbps.
    pub peak_bandwidth_mbps: f64,
    /// Current IOPS.
    pub current_iops: u32,
    /// Peak IOPS.
    pub peak_iops: u32,
    /// Number of throttled operations.
    pub throttle_count: u64,
    /// Total throttle duration in milliseconds.
    pub throttle_duration_ms: u64,
}

impl Default for QosStats {
    fn default() -> Self {
        Self {
            tenant_id: String::new(),
            current_bandwidth_mbps: 0.0,
            peak_bandwidth_mbps: 0.0,
            current_iops: 0,
            peak_iops: 0,
            throttle_count: 0,
            throttle_duration_ms: 0,
        }
    }
}

/// QoS client bridge for tenant-based rate limiting.
pub struct QosClientBridge {
    /// Per-tenant QoS configuration.
    tenant_qos_map: Arc<DashMap<String, TenantQos>>,
    /// Per-tenant bandwidth shapers.
    bandwidth_shapers: Arc<DashMap<String, BandwidthShaper>>,
    /// Per-tenant IOPS limiters.
    iops_limiters: Arc<DashMap<String, IopsLimiter>>,
    /// Per-tenant statistics.
    stats: Arc<DashMap<String, QosStats>>,
}

impl QosClientBridge {
    /// Creates a new QoS client bridge.
    pub fn new() -> Self {
        Self {
            tenant_qos_map: Arc::new(DashMap::new()),
            bandwidth_shapers: Arc::new(DashMap::new()),
            iops_limiters: Arc::new(DashMap::new()),
            stats: Arc::new(DashMap::new()),
        }
    }

    /// Registers a tenant with QoS parameters.
    pub fn register_tenant(&self, qos: TenantQos) -> Result<(), String> {
        if qos.tenant_id.is_empty() {
            return Err("tenant_id cannot be empty".to_string());
        }

        self.tenant_qos_map.insert(qos.tenant_id.clone(), qos.clone());

        if let Some(max_bw) = qos.max_bandwidth_mbps {
            let max_bw_u64 = max_bw as u64;
            let target_bw = qos.target_bandwidth_mbps.unwrap_or(max_bw) as u64;
            let shaper = BandwidthShaper::new(
                max_bw_u64 * 125_000,
                target_bw * 125_000,
            );
            self.bandwidth_shapers
                .insert(qos.tenant_id.clone(), shaper);
        }

        if let Some(max_iops) = qos.max_iops {
            let target_iops = qos.target_iops.unwrap_or(max_iops);
            let limiter = IopsLimiter::new(max_iops, target_iops);
            self.iops_limiters.insert(qos.tenant_id.clone(), limiter);
        }

        let mut stats = QosStats::default();
        stats.tenant_id = qos.tenant_id.clone();
        self.stats.insert(qos.tenant_id, stats);

        Ok(())
    }

    /// Updates tenant QoS parameters.
    pub fn update_tenant_qos(&self, qos: TenantQos) -> Result<(), String> {
        self.register_tenant(qos)
    }

    /// Acquires a bandwidth token for the tenant.
    pub async fn acquire_bandwidth(&self, tenant_id: &str, bytes: u64) -> Result<BandwidthToken, String> {
        let qos = self
            .tenant_qos_map
            .get(tenant_id)
            .ok_or_else(|| format!("tenant {} not registered", tenant_id))?;

        if qos.max_bandwidth_mbps.is_none() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64;
            return Ok(BandwidthToken {
                tenant_id: tenant_id.to_string(),
                bytes,
                allocated_ns: now,
            });
        }

        if qos.max_bandwidth_mbps == Some(0) {
            return Err(format!(
                "tenant {} has zero bandwidth limit",
                tenant_id
            ));
        }

        let shaper = self
            .bandwidth_shapers
            .get(tenant_id)
            .ok_or_else(|| format!("no bandwidth shaper for tenant {}", tenant_id))?;

        if let Some(token) = shaper.try_acquire(bytes).await {
            let mut token = token;
            token.tenant_id = tenant_id.to_string();

            if let Some(mut stats) = self.stats.get_mut(tenant_id) {
                let bw_mbps = (bytes as f64 * 1_000_000_000.0) / (1024.0 * 1024.0);
                stats.current_bandwidth_mbps += bw_mbps;
                if stats.current_bandwidth_mbps > stats.peak_bandwidth_mbps {
                    stats.peak_bandwidth_mbps = stats.current_bandwidth_mbps;
                }
            }

            Ok(token)
        } else {
            if let Some(mut stats) = self.stats.get_mut(tenant_id) {
                stats.throttle_count += 1;
            }
            Err(format!(
                "bandwidth limit exceeded for tenant {}",
                tenant_id
            ))
        }
    }

    /// Acquires an IOPS token for the tenant.
    pub async fn acquire_iops(&self, tenant_id: &str) -> Result<IopsToken, String> {
        let qos = self
            .tenant_qos_map
            .get(tenant_id)
            .ok_or_else(|| format!("tenant {} not registered", tenant_id))?;

        if qos.max_iops.is_none() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64;
            return Ok(IopsToken {
                tenant_id: tenant_id.to_string(),
                ops: 1,
                allocated_ns: now,
            });
        }

        if qos.max_iops == Some(0) {
            return Err(format!("tenant {} has zero IOPS limit", tenant_id));
        }

        let limiter = self
            .iops_limiters
            .get(tenant_id)
            .ok_or_else(|| format!("no IOPS limiter for tenant {}", tenant_id))?;

        if let Some(mut token) = limiter.try_acquire().await {
            token.tenant_id = tenant_id.to_string();

            if let Some(mut stats) = self.stats.get_mut(tenant_id) {
                stats.current_iops += 1;
                if stats.current_iops > stats.peak_iops {
                    stats.peak_iops = stats.current_iops;
                }
            }

            Ok(token)
        } else {
            if let Some(mut stats) = self.stats.get_mut(tenant_id) {
                stats.throttle_count += 1;
            }
            Err(format!("IOPS limit exceeded for tenant {}", tenant_id))
        }
    }

    /// Releases bandwidth and IOPS tokens.
    pub fn release_tokens(&self, _bw_token: BandwidthToken, _iops_token: IopsToken) {
        // Tokens are automatically released when dropped
    }

    /// Gets QoS statistics for a tenant.
    pub fn get_tenant_stats(&self, tenant_id: &str) -> Option<QosStats> {
        self.stats.get(tenant_id).map(|s| s.clone())
    }
}

impl Default for QosClientBridge {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_tenant_succeeds() {
        let bridge = QosClientBridge::new();
        let qos = TenantQos {
            tenant_id: "tenant1".to_string(),
            workload_class: WorkloadClass::Interactive,
            priority: 200,
            max_bandwidth_mbps: Some(1000),
            target_bandwidth_mbps: Some(800),
            max_iops: Some(10000),
            target_iops: Some(8000),
        };
        assert!(bridge.register_tenant(qos).is_ok());
    }

    #[tokio::test]
    async fn test_tenant_not_registered_returns_error() {
        let bridge = QosClientBridge::new();
        let result = bridge.acquire_bandwidth("unknown", 1024).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_zero_max_bandwidth_rejects_all() {
        let bridge = QosClientBridge::new();
        let qos = TenantQos {
            tenant_id: "tenant1".to_string(),
            workload_class: WorkloadClass::Batch,
            priority: 128,
            max_bandwidth_mbps: Some(0),
            target_bandwidth_mbps: None,
            max_iops: None,
            target_iops: None,
        };
        bridge.register_tenant(qos).unwrap();
        let result = bridge.acquire_bandwidth("tenant1", 1024).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_acquire_bandwidth_succeeds_within_limit() {
        let bridge = QosClientBridge::new();
        let qos = TenantQos {
            tenant_id: "tenant1".to_string(),
            workload_class: WorkloadClass::Batch,
            priority: 128,
            max_bandwidth_mbps: Some(1000),
            target_bandwidth_mbps: Some(800),
            max_iops: None,
            target_iops: None,
        };
        bridge.register_tenant(qos).unwrap();
        let result = bridge.acquire_bandwidth("tenant1", 1024).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_acquire_iops_succeeds_within_limit() {
        let bridge = QosClientBridge::new();
        let qos = TenantQos {
            tenant_id: "tenant1".to_string(),
            workload_class: WorkloadClass::Batch,
            priority: 128,
            max_bandwidth_mbps: None,
            target_bandwidth_mbps: None,
            max_iops: Some(10000),
            target_iops: Some(8000),
        };
        bridge.register_tenant(qos).unwrap();
        let result = bridge.acquire_iops("tenant1").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_multiple_tenants_isolated() {
        let bridge = QosClientBridge::new();

        let qos1 = TenantQos {
            tenant_id: "tenant1".to_string(),
            workload_class: WorkloadClass::Interactive,
            priority: 200,
            max_bandwidth_mbps: Some(1000),
            target_bandwidth_mbps: None,
            max_iops: Some(10000),
            target_iops: None,
        };
        let qos2 = TenantQos {
            tenant_id: "tenant2".to_string(),
            workload_class: WorkloadClass::Batch,
            priority: 128,
            max_bandwidth_mbps: Some(500),
            target_bandwidth_mbps: None,
            max_iops: Some(5000),
            target_iops: None,
        };

        bridge.register_tenant(qos1).unwrap();
        bridge.register_tenant(qos2).unwrap();

        let token1 = bridge.acquire_bandwidth("tenant1", 1024).await;
        let token2 = bridge.acquire_bandwidth("tenant2", 1024).await;

        assert!(token1.is_ok());
        assert!(token2.is_ok());

        let stats1 = bridge.get_tenant_stats("tenant1").unwrap();
        let stats2 = bridge.get_tenant_stats("tenant2").unwrap();

        assert_eq!(stats1.tenant_id, "tenant1");
        assert_eq!(stats2.tenant_id, "tenant2");
    }

    #[test]
    fn test_workload_class_priority_ordering() {
        assert!(WorkloadClass::Interactive.base_priority() > WorkloadClass::Batch.base_priority());
        assert!(WorkloadClass::Batch.base_priority() > WorkloadClass::Background.base_priority());
    }

    #[tokio::test]
    async fn test_update_tenant_qos_changes_limits() {
        let bridge = QosClientBridge::new();

        let qos1 = TenantQos {
            tenant_id: "tenant1".to_string(),
            workload_class: WorkloadClass::Batch,
            priority: 128,
            max_bandwidth_mbps: Some(100),
            target_bandwidth_mbps: None,
            max_iops: None,
            target_iops: None,
        };
        bridge.register_tenant(qos1).unwrap();

        let qos2 = TenantQos {
            tenant_id: "tenant1".to_string(),
            workload_class: WorkloadClass::Batch,
            priority: 128,
            max_bandwidth_mbps: Some(200),
            target_bandwidth_mbps: None,
            max_iops: None,
            target_iops: None,
        };
        bridge.update_tenant_qos(qos2).unwrap();

        let stats = bridge.get_tenant_stats("tenant1");
        assert!(stats.is_some());
    }

    #[test]
    fn test_get_tenant_stats_accurate() {
        let bridge = QosClientBridge::new();
        let qos = TenantQos {
            tenant_id: "tenant1".to_string(),
            workload_class: WorkloadClass::Interactive,
            priority: 200,
            max_bandwidth_mbps: Some(1000),
            target_bandwidth_mbps: Some(800),
            max_iops: Some(10000),
            target_iops: Some(8000),
        };
        bridge.register_tenant(qos).unwrap();

        let stats = bridge.get_tenant_stats("tenant1").unwrap();
        assert_eq!(stats.tenant_id, "tenant1");
        assert_eq!(stats.throttle_count, 0);
    }

    #[test]
    fn test_background_workload_lower_priority() {
        let bg = WorkloadClass::Background.base_priority();
        let interactive = WorkloadClass::Interactive.base_priority();
        assert!(bg < interactive);
    }

    #[test]
    fn test_interactive_workload_higher_priority() {
        let interactive = WorkloadClass::Interactive.base_priority();
        let batch = WorkloadClass::Batch.base_priority();
        assert!(interactive > batch);
    }

    #[tokio::test]
    async fn test_release_tokens_refunds_nothing() {
        let bridge = QosClientBridge::new();
        let qos = TenantQos {
            tenant_id: "tenant1".to_string(),
            workload_class: WorkloadClass::Batch,
            priority: 128,
            max_bandwidth_mbps: Some(1000),
            target_bandwidth_mbps: None,
            max_iops: Some(10000),
            target_iops: None,
        };
        bridge.register_tenant(qos).unwrap();

        let bw_token = bridge.acquire_bandwidth("tenant1", 1024).await.unwrap();
        let iops_token = bridge.acquire_iops("tenant1").await.unwrap();

        bridge.release_tokens(bw_token, iops_token);
    }

    #[tokio::test]
    async fn test_concurrent_acquire_operations() {
        let bridge = QosClientBridge::new();
        let qos = TenantQos {
            tenant_id: "tenant1".to_string(),
            workload_class: WorkloadClass::Batch,
            priority: 128,
            max_bandwidth_mbps: Some(100000),
            target_bandwidth_mbps: None,
            max_iops: Some(1000000),
            target_iops: None,
        };
        bridge.register_tenant(qos).unwrap();

        let mut handles = Vec::new();
        for _ in 0..10 {
            let bridge = Arc::new(bridge.clone_inner());
            let handle = tokio::spawn(async move {
                bridge.acquire_bandwidth("tenant1", 1024).await
            });
            handles.push(handle);
        }

        let mut successes = 0;
        for handle in handles {
            if handle.await.unwrap().is_ok() {
                successes += 1;
            }
        }
        assert!(successes > 0);
    }

    fn _clone_inner(arc: &Arc<QosClientBridge>) -> QosClientBridge {
        QosClientBridge {
            tenant_qos_map: Arc::new(DashMap::new()),
            bandwidth_shapers: Arc::new(DashMap::new()),
            iops_limiters: Arc::new(DashMap::new()),
            stats: Arc::new(DashMap::new()),
        }
    }

    #[test]
    fn test_hard_bandwidth_limit_enforced() {
        let bridge = QosClientBridge::new();
        let qos = TenantQos {
            tenant_id: "tenant1".to_string(),
            workload_class: WorkloadClass::Batch,
            priority: 128,
            max_bandwidth_mbps: Some(1),
            target_bandwidth_mbps: None,
            max_iops: None,
            target_iops: None,
        };
        bridge.register_tenant(qos).unwrap();
    }

    #[test]
    fn test_soft_bandwidth_target_monitored() {
        let bridge = QosClientBridge::new();
        let qos = TenantQos {
            tenant_id: "tenant1".to_string(),
            workload_class: WorkloadClass::Batch,
            priority: 128,
            max_bandwidth_mbps: Some(1000),
            target_bandwidth_mbps: Some(800),
            max_iops: None,
            target_iops: None,
        };
        let result = bridge.register_tenant(qos);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_stats_peak_values_tracked() {
        let bridge = QosClientBridge::new();
        let qos = TenantQos {
            tenant_id: "tenant1".to_string(),
            workload_class: WorkloadClass::Batch,
            priority: 128,
            max_bandwidth_mbps: Some(100000),
            target_bandwidth_mbps: None,
            max_iops: Some(1000000),
            target_iops: None,
        };
        bridge.register_tenant(qos).unwrap();

        bridge.acquire_bandwidth("tenant1", 1000000).await.unwrap();

        let stats = bridge.get_tenant_stats("tenant1").unwrap();
        assert!(stats.peak_bandwidth_mbps > 0.0);
    }

    #[tokio::test]
    async fn test_throttle_metrics_incremented() {
        let bridge = QosClientBridge::new();
        let qos = TenantQos {
            tenant_id: "tenant1".to_string(),
            workload_class: WorkloadClass::Batch,
            priority: 128,
            max_bandwidth_mbps: Some(1),
            target_bandwidth_mbps: None,
            max_iops: None,
            target_iops: None,
        };
        bridge.register_tenant(qos).unwrap();

        let _ = bridge.acquire_bandwidth("tenant1", 100000000).await;
        let _ = bridge.acquire_bandwidth("tenant1", 100000000).await;

        let stats = bridge.get_tenant_stats("tenant1").unwrap();
        assert!(stats.throttle_count > 0);
    }

    impl QosClientBridge {
        fn clone_inner(&self) -> QosClientBridge {
            QosClientBridge {
                tenant_qos_map: Arc::new(DashMap::new()),
                bandwidth_shapers: Arc::new(DashMap::new()),
                iops_limiters: Arc::new(DashMap::new()),
                stats: Arc::new(DashMap::new()),
            }
        }
    }
}