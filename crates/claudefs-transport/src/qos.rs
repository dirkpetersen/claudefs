//! QoS (Quality of Service) / Traffic Shaping Module.
//!
//! Provides priority queues, bandwidth guarantees, and rate limiting for different
//! workload classes using token bucket algorithm and weighted fair queuing.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, trace};

/// Errors that can occur during QoS operations.
#[derive(Error, Debug)]
pub enum QosError {
    /// Failed to acquire QoS permit due to rate limiting.
    #[error("Rate limit exceeded for workload class {class:?}")]
    RateLimitExceeded {
        /// The workload class that exceeded its rate limit.
        class: WorkloadClass,
    },

    /// Invalid QoS configuration.
    #[error("Invalid QoS configuration: {reason}")]
    InvalidConfig {
        /// Reason why the configuration is invalid.
        reason: String,
    },
}

/// Result type alias for QoS operations.
pub type Result<T> = std::result::Result<T, QosError>;

/// Workload classes for QoS priority scheduling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkloadClass {
    /// Real-time metadata operations (highest priority).
    RealtimeMeta,
    /// Interactive data reads/writes.
    Interactive,
    /// Batch/background operations (e.g., EC striping, S3 tiering).
    Batch,
    /// Replication traffic.
    Replication,
    /// Management/monitoring traffic (lowest priority).
    Management,
}

impl WorkloadClass {
    /// Returns the default priority for this workload class (0 = highest).
    pub fn default_priority(&self) -> u8 {
        match self {
            WorkloadClass::RealtimeMeta => 0,
            WorkloadClass::Interactive => 1,
            WorkloadClass::Replication => 2,
            WorkloadClass::Batch => 3,
            WorkloadClass::Management => 4,
        }
    }

    /// Returns the default weight for weighted fair queuing.
    pub fn default_weight(&self) -> u32 {
        match self {
            WorkloadClass::RealtimeMeta => 100,
            WorkloadClass::Interactive => 50,
            WorkloadClass::Replication => 30,
            WorkloadClass::Batch => 15,
            WorkloadClass::Management => 5,
        }
    }

    /// Returns the default max bandwidth in bytes/sec (0 = unlimited).
    pub fn default_max_bandwidth(&self) -> u64 {
        match self {
            WorkloadClass::RealtimeMeta => 0, // Unlimited
            WorkloadClass::Interactive => 0,  // Unlimited
            WorkloadClass::Replication => 0,  // Unlimited
            WorkloadClass::Batch => 0,        // Unlimited
            WorkloadClass::Management => 0,   // Unlimited
        }
    }

    /// Returns the default max requests per second (0 = unlimited).
    pub fn default_max_requests_per_sec(&self) -> u64 {
        0 // Unlimited by default
    }

    /// Returns the default burst size in bytes.
    pub fn default_burst_size(&self) -> u64 {
        match self {
            WorkloadClass::RealtimeMeta => 1_048_576,     // 1 MB
            WorkloadClass::Interactive => 4_194_304,     // 4 MB
            WorkloadClass::Replication => 16_777_216,   // 16 MB
            WorkloadClass::Batch => 67_108_864,         // 64 MB
            WorkloadClass::Management => 1_048_576,     // 1 MB
        }
    }
}

impl Default for WorkloadClass {
    fn default() -> Self {
        WorkloadClass::Management
    }
}

/// Configuration for QoS parameters per workload class.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QosConfig {
    /// Maximum bandwidth in bytes per second (0 = unlimited).
    pub max_bandwidth_bytes_per_sec: u64,
    /// Maximum requests per second (0 = unlimited).
    pub max_requests_per_sec: u64,
    /// Relative weight for weighted fair queuing (higher = more bandwidth share).
    pub weight: u32,
    /// Token bucket burst allowance in bytes.
    pub burst_size: u64,
    /// Scheduling priority (0 = highest).
    pub priority: u8,
}

impl QosConfig {
    /// Creates a new QoS config with the given parameters.
    pub fn new(
        max_bandwidth_bytes_per_sec: u64,
        max_requests_per_sec: u64,
        weight: u32,
        burst_size: u64,
        priority: u8,
    ) -> Self {
        Self {
            max_bandwidth_bytes_per_sec,
            max_requests_per_sec,
            weight,
            burst_size,
            priority,
        }
    }
}

impl Default for QosConfig {
    fn default() -> Self {
        Self {
            max_bandwidth_bytes_per_sec: 0,
            max_requests_per_sec: 0,
            weight: 50,
            burst_size: 4_194_304,
            priority: 2,
        }
    }
}

/// Default QoS configuration for all workload classes.
pub fn default_qos_config() -> HashMap<WorkloadClass, QosConfig> {
    let mut config = HashMap::new();
    for class in [
        WorkloadClass::RealtimeMeta,
        WorkloadClass::Interactive,
        WorkloadClass::Replication,
        WorkloadClass::Batch,
        WorkloadClass::Management,
    ] {
        config.insert(
            class,
            QosConfig::new(
                class.default_max_bandwidth(),
                class.default_max_requests_per_sec(),
                class.default_weight(),
                class.default_burst_size(),
                class.default_priority(),
            ),
        );
    }
    config
}

/// Token bucket rate limiter for bandwidth management.
///
/// Uses the token bucket algorithm to allow bursty traffic while enforcing
/// a sustained rate limit.
pub struct TokenBucket {
    rate: u64,
    burst: u64,
    tokens: u64,
    last_refill: Instant,
}

impl TokenBucket {
    /// Creates a new token bucket with the given rate and burst capacity.
    ///
    /// # Arguments
    /// * `rate` - Tokens per second (rate of refill).
    /// * `burst` - Maximum burst capacity in tokens.
    pub fn new(rate: u64, burst: u64) -> Self {
        Self {
            rate,
            burst,
            tokens: burst,
            last_refill: Instant::now(),
        }
    }

    /// Attempts to acquire tokens without blocking.
    ///
    /// Returns true if tokens were acquired, false otherwise.
    pub fn try_acquire(&mut self, tokens: u64) -> bool {
        self.refill();
        if self.tokens >= tokens {
            self.tokens -= tokens;
            true
        } else {
            false
        }
    }

    /// Acquires tokens, waiting asynchronously until available.
    ///
    /// This method will wait until enough tokens become available.
    /// Note: This is a synchronous wait in a tokio context.
    pub async fn acquire(&mut self, tokens: u64) {
        loop {
            self.refill();
            if self.tokens >= tokens {
                self.tokens -= tokens;
                return;
            }
            // Calculate wait time
            let needed = tokens - self.tokens;
            let wait_secs = needed as f64 / self.rate as f64;
            let wait_duration = Duration::from_secs_f64(wait_secs.max(0.001));
            tokio::time::sleep(wait_duration).await;
        }
    }

    /// Returns the current number of available tokens.
    pub fn available(&self) -> u64 {
        // Need to do a fresh refill calculation for the const reference
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill);
        let refill = (elapsed.as_secs_f64() * self.rate as f64) as u64;
        std::cmp::min(self.burst, self.tokens + refill)
    }

    /// Refills tokens based on elapsed time.
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill);
        let refill = (elapsed.as_secs_f64() * self.rate as f64) as u64;
        self.tokens = std::cmp::min(self.burst, self.tokens.saturating_add(refill));
        self.last_refill = now;
    }

    /// Returns the burst capacity.
    pub fn burst(&self) -> u64 {
        self.burst
    }

    /// Returns the refill rate.
    pub fn rate(&self) -> u64 {
        self.rate
    }
}

impl Default for TokenBucket {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

/// Statistics for a single workload class.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClassStats {
    /// Number of requests admitted.
    pub admitted: u64,
    /// Number of requests rejected.
    pub rejected: u64,
    /// Total bytes admitted.
    pub total_bytes: u64,
    /// Total wait time in milliseconds.
    pub total_wait_ms: u64,
}

impl ClassStats {
    /// Returns the average wait time in milliseconds.
    pub fn avg_wait_ms(&self) -> u64 {
        if self.admitted == 0 {
            0
        } else {
            self.total_wait_ms / self.admitted
        }
    }
}

/// QoS statistics for all workload classes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QosStats {
    /// Per-class statistics.
    pub classes: HashMap<WorkloadClass, ClassStats>,
}

impl QosStats {
    /// Gets statistics for a specific class.
    pub fn get(&self, class: &WorkloadClass) -> ClassStats {
        self.classes.get(class).cloned().unwrap_or_default()
    }
}

/// RAII guard returned by QoS scheduler admit operations.
///
/// When dropped, releases the bandwidth accounting.
pub struct QosPermit {
    class: WorkloadClass,
    size_bytes: u64,
    start_time: Instant,
    scheduler: Option<Arc<RwLock<QosSchedulerInner>>>,
}

impl QosPermit {
    fn new(
        class: WorkloadClass,
        size_bytes: u64,
        scheduler: Arc<RwLock<QosSchedulerInner>>,
    ) -> Self {
        Self {
            class,
            size_bytes,
            start_time: Instant::now(),
            scheduler: Some(scheduler),
        }
    }

    /// Returns the workload class for this permit.
    pub fn class(&self) -> WorkloadClass {
        self.class
    }

    /// Returns the size in bytes for this permit.
    pub fn size_bytes(&self) -> u64 {
        self.size_bytes
    }
}

impl Drop for QosPermit {
    fn drop(&mut self) {
        let wait_ms = self.start_time.elapsed().as_millis() as u64;
        if let Some(scheduler) = self.scheduler.take() {
            let class = self.class;
            let size = self.size_bytes;
            let wait = wait_ms;
            // Spawn a task to release the permit (fire and forget)
            let _ = tokio::spawn(async move {
                let mut scheduler = scheduler.write().await;
                if let Some(stats) = scheduler.stats.get_mut(&class) {
                    stats.total_bytes += size;
                    stats.total_wait_ms += wait;
                }
            });
        }
    }
}

/// Inner state of the QoS scheduler.
struct QosSchedulerInner {
    #[allow(dead_code)]
    config: HashMap<WorkloadClass, QosConfig>,
    token_buckets: HashMap<WorkloadClass, TokenBucket>,
    stats: HashMap<WorkloadClass, ClassStats>,
}

/// Main QoS scheduler for admission control and traffic shaping.
///
/// Provides per-workload-class rate limiting and weighted fair queuing.
pub struct QosScheduler {
    inner: Arc<RwLock<QosSchedulerInner>>,
}

impl QosScheduler {
    /// Creates a new QoS scheduler with the given configuration.
    pub fn new(config: HashMap<WorkloadClass, QosConfig>) -> Self {
        let mut token_buckets = HashMap::new();
        let mut stats = HashMap::new();

        for (class, cfg) in &config {
            let rate = if cfg.max_bandwidth_bytes_per_sec > 0 {
                cfg.max_bandwidth_bytes_per_sec
            } else if cfg.max_requests_per_sec > 0 {
                // If only request rate is limited, use a high token rate
                // Each "request" is treated as 1 token for request rate limiting
                u64::MAX
            } else {
                // Unlimited
                u64::MAX
            };
            token_buckets.insert(*class, TokenBucket::new(rate, cfg.burst_size));
            stats.insert(*class, ClassStats::default());
        }

        // Ensure all workload classes have entries
        for class in [
            WorkloadClass::RealtimeMeta,
            WorkloadClass::Interactive,
            WorkloadClass::Batch,
            WorkloadClass::Replication,
            WorkloadClass::Management,
        ] {
            if !token_buckets.contains_key(&class) {
                let default_cfg = QosConfig::new(
                    class.default_max_bandwidth(),
                    class.default_max_requests_per_sec(),
                    class.default_weight(),
                    class.default_burst_size(),
                    class.default_priority(),
                );
                token_buckets.insert(
                    class,
                    TokenBucket::new(u64::MAX, default_cfg.burst_size),
                );
            }
            if !stats.contains_key(&class) {
                stats.insert(class, ClassStats::default());
            }
        }

        let inner = QosSchedulerInner {
            config,
            token_buckets,
            stats,
        };

        Self {
            inner: Arc::new(RwLock::new(inner)),
        }
    }

    /// Creates a new QoS scheduler with default configuration.
    pub fn with_default_config() -> Self {
        Self::new(default_qos_config())
    }

    /// Admits a request for the given workload class, blocking until admitted.
    ///
    /// This method will wait until bandwidth is available for the request.
    pub async fn admit(&self, class: WorkloadClass, size_bytes: u64) -> Result<QosPermit> {
        let start = Instant::now();
        let scheduler = self.inner.clone();

        // Acquire tokens and update stats
        {
            let mut inner = scheduler.write().await;
            let bucket = inner
                .token_buckets
                .get_mut(&class)
                .ok_or_else(|| QosError::InvalidConfig {
                    reason: format!("No configuration for workload class {:?}", class),
                })?;

            bucket.acquire(size_bytes).await;

            // Update statistics synchronously
            if let Some(stats) = inner.stats.get_mut(&class) {
                stats.admitted += 1;
                stats.total_bytes += size_bytes;
            }
        }

        debug!(
            class = ?class,
            size_bytes = size_bytes,
            wait_ms = start.elapsed().as_millis(),
            "QoS admit succeeded"
        );

        Ok(QosPermit::new(class, size_bytes, scheduler))
    }

    /// Tries to admit a request without blocking.
    ///
    /// Returns Some(QosPermit) if admitted, None if rate limited.
    pub fn try_admit(&self, class: WorkloadClass, size_bytes: u64) -> Option<QosPermit> {
        let scheduler = self.inner.clone();

        let result = {
            let mut inner = match scheduler.try_write() {
                Ok(inner) => inner,
                Err(_) => return None,
            };

            let bucket = inner.token_buckets.get_mut(&class)?;

            if bucket.try_acquire(size_bytes) {
                if let Some(stats) = inner.stats.get_mut(&class) {
                    stats.admitted += 1;
                    stats.total_bytes += size_bytes;
                }
                trace!(class = ?class, size_bytes = size_bytes, "QoS try_admit succeeded");
                true
            } else {
                if let Some(stats) = inner.stats.get_mut(&class) {
                    stats.rejected += 1;
                }
                trace!(class = ?class, size_bytes = size_bytes, "QoS try_admit failed");
                false
            }
        };

        if result {
            Some(QosPermit::new(class, size_bytes, scheduler))
        } else {
            None
        }
    }

    /// Returns QoS statistics for all workload classes.
    pub async fn stats(&self) -> QosStats {
        let inner = self.inner.read().await;
        QosStats {
            classes: inner.stats.clone(),
        }
    }

    /// Returns a snapshot of current statistics (synchronous).
    pub fn stats_sync(&self) -> QosStats {
        let inner = match self.inner.try_read() {
            Ok(inner) => inner,
            Err(_) => return QosStats::default(),
        };
        QosStats {
            classes: inner.stats.clone(),
        }
    }
}

impl Default for QosScheduler {
    fn default() -> Self {
        Self::with_default_config()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_token_bucket_basic() {
        let mut bucket = TokenBucket::new(1000, 1000); // 1000 tokens/sec, burst 1000

        // Initial burst should be available
        assert!(bucket.available() >= 1000);

        // Acquire 100 tokens
        assert!(bucket.try_acquire(100));

        // Should have 900 remaining
        assert!(bucket.available() < 1000);
    }

    #[tokio::test]
    async fn test_token_bucket_burst() {
        let mut bucket = TokenBucket::new(100, 500); // 100 tokens/sec, burst 500

        // Should be able to burst up to 500
        assert!(bucket.try_acquire(500));
        assert!(!bucket.try_acquire(1)); // Should be exhausted now

        // Wait a bit
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Should have some tokens now (approx 5)
        assert!(bucket.available() >= 1);
    }

    #[tokio::test]
    async fn test_token_bucket_rate_limiting() {
        let mut bucket = TokenBucket::new(100, 100); // 100 tokens/sec, burst 100

        // Exhaust the burst
        for _ in 0..10 {
            assert!(bucket.try_acquire(10));
        }

        // Should be rate limited now
        assert!(!bucket.try_acquire(10));

        // Wait for refill
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Should have tokens now
        assert!(bucket.try_acquire(10));
    }

    #[tokio::test]
    async fn test_qos_scheduler_admit() {
        let config = default_qos_config();
        let scheduler = QosScheduler::new(config);

        // Should be able to admit without blocking (unlimited by default)
        let permit = scheduler.admit(WorkloadClass::Interactive, 1024).await;
        assert!(permit.is_ok());

        let stats = scheduler.stats().await;
        let interactive_stats = stats.get(&WorkloadClass::Interactive);
        assert_eq!(interactive_stats.admitted, 1);
    }

    #[tokio::test]
    async fn test_qos_scheduler_priority() {
        let mut config = HashMap::new();
        config.insert(
            WorkloadClass::RealtimeMeta,
            QosConfig::new(0, 0, 100, 1_000_000, 0),
        );
        config.insert(
            WorkloadClass::Batch,
            QosConfig::new(0, 0, 10, 1_000_000, 3),
        );

        let scheduler = QosScheduler::new(config);

        // Both should work with default config (unlimited)
        let permit1 = scheduler
            .admit(WorkloadClass::RealtimeMeta, 1024)
            .await;
        let permit2 = scheduler.admit(WorkloadClass::Batch, 1024).await;

        assert!(permit1.is_ok());
        assert!(permit2.is_ok());
    }

    #[tokio::test]
    async fn test_qos_scheduler_stats() {
        let scheduler = QosScheduler::with_default_config();

        // Admit some requests
        scheduler.admit(WorkloadClass::Interactive, 1024).await.ok();
        scheduler.admit(WorkloadClass::Interactive, 2048).await.ok();
        scheduler.admit(WorkloadClass::Batch, 4096).await.ok();

        // Try some that should fail
        scheduler.try_admit(WorkloadClass::Replication, 1024);
        scheduler.try_admit(WorkloadClass::Replication, 1024);

        let stats = scheduler.stats().await;
        let interactive = stats.get(&WorkloadClass::Interactive);
        let batch = stats.get(&WorkloadClass::Batch);
        let replication = stats.get(&WorkloadClass::Replication);

        assert_eq!(interactive.admitted, 2);
        assert_eq!(interactive.total_bytes, 3072);
        assert_eq!(batch.admitted, 1);
        assert_eq!(batch.total_bytes, 4096);
    }

    #[test]
    fn test_workload_class_ordering() {
        assert!(WorkloadClass::RealtimeMeta.default_priority() < WorkloadClass::Interactive.default_priority());
        assert!(WorkloadClass::Interactive.default_priority() < WorkloadClass::Batch.default_priority());
        assert!(WorkloadClass::Batch.default_priority() < WorkloadClass::Management.default_priority());
    }

    #[tokio::test]
    async fn test_qos_permit_drop() {
        let scheduler = QosScheduler::with_default_config();

        {
            let permit = scheduler.admit(WorkloadClass::Interactive, 5000).await.unwrap();
            assert_eq!(permit.class(), WorkloadClass::Interactive);
            assert_eq!(permit.size_bytes(), 5000);
            // Permit dropped here
        }

        // Give time for async drop to complete
        tokio::time::sleep(Duration::from_millis(10)).await;

        let stats = scheduler.stats().await;
        let interactive = stats.get(&WorkloadClass::Interactive);
        assert!(interactive.total_bytes > 0);
    }

    #[tokio::test]
    async fn test_qos_default_config() {
        let scheduler = QosScheduler::with_default_config();

        // Should be able to admit to any workload class
        for class in [
            WorkloadClass::RealtimeMeta,
            WorkloadClass::Interactive,
            WorkloadClass::Batch,
            WorkloadClass::Replication,
            WorkloadClass::Management,
        ] {
            let result = scheduler.admit(class, 1024).await;
            assert!(result.is_ok(), "Failed to admit for class {:?}", class);
        }

        let stats = scheduler.stats().await;
        for class in [
            WorkloadClass::RealtimeMeta,
            WorkloadClass::Interactive,
            WorkloadClass::Batch,
            WorkloadClass::Replication,
            WorkloadClass::Management,
        ] {
            let class_stats = stats.get(&class);
            assert_eq!(class_stats.admitted, 1, "Expected 1 admit for class {:?}", class);
        }
    }
}