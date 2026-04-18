# A1 Phase 11 Block 3: Intelligent Tiering — OpenCode Implementation

**Context:** ClaudeFS Storage Engine (A1), Phase 11 Block 3 implementation. Builds on Phase 10 (1301 tests). All code must pass `cargo build` and `cargo test --release` with zero clippy warnings.

**Target:** Implement 3 modules (access_pattern_learner.rs, tiering_policy_engine.rs, tiering_analytics.rs) with 18 comprehensive tests totaling ~600 LOC.

---

## Architecture Overview

### System Context
- **Crate:** `crates/claudefs-storage`
- **Related Modules:** `tier_orchestrator.rs` (S3 tiering), `background_scheduler.rs` (scheduling)
- **Key Design:** D5 (S3 Tiering Policy) — Cache mode + eviction scoring
- **Optimization Goal:** Minimize (flash_cost + S3_cost) while maintaining p99 latency

### Existing Related Code
- **tier_orchestrator.rs** — S3 eviction decisions
- **write_journal.rs** — Segment packing, write tracking
- **background_scheduler.rs** — Task scheduling with priorities

### Dependencies
- `tokio` — async runtime
- `parking_lot` — efficient locks
- `dashmap` — concurrent HashMap
- `tracing` — distributed tracing
- Standard: `std::sync::{Arc, Mutex, atomic::*}`, `std::collections::{VecDeque, HashMap, BTreeMap}`

---

## Module 1: access_pattern_learner.rs (~250 LOC)

**Purpose:** Track segment access counts + recency, classify into hot/warm/cold tiers.

### Public API
```rust
pub struct AccessPatternLearner {
    segment_access_log: Arc<DashMap<SegmentId, AccessStats>>,
    ema_alpha: f64, // Smoothing factor, default 0.1
    update_interval_ms: u64, // Recompute stats every N ms
}

impl AccessPatternLearner {
    pub fn new(ema_alpha: f64, update_interval_ms: u64) -> Self { ... }

    /// Record segment access
    pub fn record_access(
        &self,
        segment_id: SegmentId,
        access_type: AccessType,
    ) { ... }

    /// Classify segment into tier
    pub fn classify_tier(
        &self,
        segment_id: SegmentId,
    ) -> Result<Tier, TieringError> { ... }

    /// Get access score (0-100, higher = hotter)
    pub fn get_access_score(
        &self,
        segment_id: SegmentId,
    ) -> Result<f64, TieringError> { ... }

    /// Adjust classification thresholds based on cluster temperature
    pub fn adapt_thresholds(
        &self,
        cluster_temperature: f64, // 0-1: cold to hot
    ) { ... }

    /// Get statistics for a segment
    pub fn get_segment_stats(
        &self,
        segment_id: SegmentId,
    ) -> Result<AccessStats, TieringError> { ... }

    /// Detect multi-day patterns
    pub fn detect_daily_pattern(
        &self,
        segment_id: SegmentId,
    ) -> Result<Option<DailyPattern>, TieringError> { ... }
}

#[derive(thiserror::Error, Debug)]
pub enum TieringError {
    #[error("segment not tracked")]
    NotFound,
    #[error("insufficient data for classification")]
    InsufficientData,
    #[error("invalid threshold configuration")]
    InvalidConfig,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Hot,
    Warm,
    Cold,
}

#[derive(Clone, Copy, Debug)]
pub enum AccessType {
    Read,
    Write,
}

pub struct AccessStats {
    pub segment_id: SegmentId,
    pub read_count: u64,
    pub write_count: u64,
    pub last_access_time: std::time::Instant,
    pub ema_score: f64, // 0-100
    pub tier: Tier,
    pub recency_hours: u32,
}

pub struct DailyPattern {
    pub peak_hour: u8,
    pub valley_hour: u8,
    pub confidence: f64, // 0-1
}
```

### Implementation Details

**Algorithm: EMA-based Access Scoring**
1. For each segment: track `read_count`, `write_count`, `last_access_time`
2. Score = EMA(read_count + 2*write_count, history)
   - EMA smooths out bursty access patterns
   - Write weighted 2x read (more valuable)
3. Recency bonus: decrease score exponentially over time (half-life = 24h)
4. Classify:
   - Hot: score > hot_threshold (default 70)
   - Warm: score in [warm_threshold, hot_threshold] (default 30-70)
   - Cold: score < warm_threshold (default <30)
5. Adapt thresholds based on cluster_temperature:
   - If temperature rising, lower thresholds (promote more to hot)
   - If temperature falling, raise thresholds (demote to cold)

**Key Properties:**
- ✅ Smooth classification (no thrashing via hysteresis)
- ✅ EMA reduces memory vs full history
- ✅ Detects weekly patterns (peak/valley hours)
- ✅ Recency-weighted (recent access = hotter)

### Test Specifications

```rust
#[cfg(test)]
mod tests {
    // Test 1: Access tracking
    #[tokio::test]
    async fn test_access_tracking() {
        // Given: empty learner
        // When: record_access(seg_id, Read) 10 times
        // Then: get_segment_stats() shows read_count = 10
        // When: record_access(seg_id, Write) 5 times
        // Then: get_segment_stats() shows write_count = 5
    }

    // Test 2: EMA smoothing
    #[tokio::test]
    async fn test_ema_smoothing() {
        // Given: ema_alpha = 0.1 (smooth)
        // When: access pattern: 10 reads, then burst of 1000 reads
        // Then: EMA score increases gradually (not spike)
        //       no thrashing between tiers
    }

    // Test 3: Hot/warm/cold classification
    #[tokio::test]
    async fn test_hot_warm_cold_classification() {
        // Given: segment with score = 80
        // Then: tier = Hot (hot_threshold = 70)
        // Given: segment with score = 50
        // Then: tier = Warm (warm_threshold = 30)
        // Given: segment with score = 10
        // Then: tier = Cold (< warm_threshold)
    }

    // Test 4: Threshold adaptation
    #[tokio::test]
    async fn test_threshold_adaptation() {
        // Given: cluster_temperature = 0.8 (hot cluster)
        // When: adapt_thresholds(0.8)
        // Then: hot_threshold lowered (promote more segments)
        // Given: cluster_temperature = 0.2 (cold cluster)
        // When: adapt_thresholds(0.2)
        // Then: hot_threshold raised (demote segments to cold)
    }

    // Test 5: Hot promotion
    #[tokio::test]
    async fn test_hot_segment_promotion() {
        // Given: segment in Cold tier, then accessed 100 times
        // When: classify_tier() called
        // Then: tier transitions to Warm (after EMA updates)
        //       then to Hot (after more access)
    }

    // Test 6: Cold demotion
    #[tokio::test]
    async fn test_cold_segment_demotion() {
        // Given: segment in Hot tier, no access for 24 hours
        // When: classify_tier() called (recency weighted)
        // Then: tier transitions to Warm
        //       then to Cold (after more inactivity)
    }

    // Test 7: Multi-day patterns
    #[tokio::test]
    async fn test_multiday_pattern_learning() {
        // Given: segment with recorded access at: 8am, 2pm peak hours (3 days)
        // When: detect_daily_pattern()
        // Then: returns Some(pattern) with peak_hour ≈ 14 (2pm)
        //       confidence > 0.7
    }

    // Test 8: Hysteresis (no thrashing)
    #[tokio::test]
    async fn test_tier_transition_hysteresis() {
        // Given: segment at Hot tier boundary (score = 70.5)
        // When: score fluctuates 70-71-70-71 over 10 updates
        // Then: tier stays Hot (no oscillation to Warm)
        //       only transitions on clear crossing
    }
}
```

---

## Module 2: tiering_policy_engine.rs (~200 LOC)

**Purpose:** Decide keep vs. tier based on cost, support tenant hints, minimize total cost.

### Public API
```rust
pub struct TieringPolicyEngine {
    cost_params: Arc<RwLock<CostParams>>,
    tenant_hints: Arc<DashMap<DirectoryId, TieringHint>>,
    access_learner: Arc<AccessPatternLearner>,
}

impl TieringPolicyEngine {
    pub fn new(
        cost_params: CostParams,
        access_learner: Arc<AccessPatternLearner>,
    ) -> Self { ... }

    /// Decide: keep on flash vs. tier to S3
    pub async fn decide_tiering(
        &self,
        segment_id: SegmentId,
        segment_size_bytes: u64,
        last_access_age_hours: u32,
    ) -> Result<TieringDecision, PolicyError> { ... }

    /// Set tenant-specific tiering hint (via xattr)
    pub fn set_tenant_hint(
        &self,
        dir_id: DirectoryId,
        hint: TieringHint,
    ) -> Result<(), PolicyError> { ... }

    /// Set global cost parameters
    pub fn set_cost_params(&self, params: CostParams) { ... }

    /// Get current tiering decision statistics
    pub fn get_decision_stats(&self) -> DecisionStats { ... }

    /// Estimate cost of keeping segment on flash
    pub fn estimate_flash_cost(
        &self,
        segment_size_bytes: u64,
        retention_days: u32,
    ) -> f64 { ... }

    /// Estimate cost of tiering to S3
    pub fn estimate_s3_cost(
        &self,
        segment_size_bytes: u64,
        access_count: u64,
    ) -> f64 { ... }
}

#[derive(thiserror::Error, Debug)]
pub enum PolicyError {
    #[error("decision engine error")]
    DecisionError,
    #[error("cost calculation failed")]
    CostCalculationFailed,
    #[error("invalid tenant hint")]
    InvalidHint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TieringDecision {
    KeepFlash,
    TierToS3,
    Defer, // Not ready to tier yet
}

#[derive(Clone, Copy, Debug)]
pub enum TieringHint {
    PinFlash,     // Keep on flash (ignore cost)
    PreferFlash,  // Prefer flash but respect cost
    Auto,         // Default: cost-based
    PreferS3,     // Prefer S3 but respect cost
    ForceS3,      // Always tier (ignore cost)
}

pub struct CostParams {
    pub flash_cost_per_gb_month: f64, // $0.05
    pub s3_cost_per_gb_month: f64,    // $0.023
    pub s3_access_cost_per_million: f64, // $0.40
    pub s3_put_cost_per_million: f64, // $5.00
}

pub struct DecisionStats {
    pub total_decisions: u64,
    pub kept_on_flash: u64,
    pub tiered_to_s3: u64,
    pub deferred: u64,
    pub estimated_monthly_savings: f64,
}
```

### Implementation Details

**Algorithm: Cost Minimization**
1. For segment:
   - Calculate flash_cost = size_bytes * retention_days * flash_cost_per_gb_day
   - Calculate s3_cost = size_bytes * retention_days * s3_cost_per_gb_day
   - Calculate access_cost = access_count * s3_access_cost
   - Total_s3_cost = s3_cost + access_cost + transfer_cost
2. Decision:
   - If total_s3_cost < flash_cost: TierToS3 (unless PinFlash hint)
   - Else: KeepFlash
   - If just barely (<5% difference): Defer (wait for more data)
3. Respect tenant hints:
   - PinFlash: always KeepFlash
   - ForceS3: always TierToS3
   - Auto: cost-based decision

**Key Properties:**
- ✅ Minimizes TCO (total cost of ownership)
- ✅ Respects tenant preferences
- ✅ Avoids oscillation (defer close decisions)
- ✅ Accounts for access patterns

### Test Specifications

```rust
#[cfg(test)]
mod tests {
    // Test 1: Cost-based tiering
    #[tokio::test]
    async fn test_cost_based_tiering() {
        // Given: 1GB segment, flash=0.05/GB/month, S3=0.023/GB/month
        // When: decide_tiering(seg_id, 1GB, 1 day)
        // Then: TierToS3 (S3 cheaper over 30 days)
        // Given: 1GB segment, only 1 hour old (recent)
        // Then: KeepFlash (flash cheaper for short retention)
    }

    // Test 2: Tenant hints
    #[tokio::test]
    async fn test_tenant_tiering_hints() {
        // Given: segment with PinFlash hint
        // When: decide_tiering()
        // Then: KeepFlash (regardless of cost)
        // Given: segment with ForceS3 hint
        // Then: TierToS3 (regardless of cost)
    }

    // Test 3: Defer decision
    #[tokio::test]
    async fn test_tiering_deferral() {
        // Given: segment where S3_cost = 1.00, flash_cost = 0.98 (2% difference)
        // When: decide_tiering()
        // Then: Defer (wait for clearer decision)
        //       avoid oscillation near break-even
    }

    // Test 4: Multi-tier optimization
    #[tokio::test]
    async fn test_multi_tier_optimization() {
        // Given: 10 segments with varying sizes + ages
        // When: bulk decide_tiering() for all
        // Then: total_cost minimized vs random tiering
    }

    // Test 5: Flash pressure
    #[tokio::test]
    async fn test_tiering_under_flash_pressure() {
        // Given: flash utilization = 95% (high pressure)
        // When: adjust cost params (lower flash_cost?)
        // Then: more segments tier to S3 (relieve pressure)
    }

    // Test 6: Recovery latency
    #[tokio::test]
    async fn test_tiering_recovery_latency() {
        // Given: segment tiered to S3
        // When: client requests, must restore from S3
        // Then: restore latency <1s for typical S3 (estimated)
    }
}
```

---

## Module 3: tiering_analytics.rs (~150 LOC)

**Purpose:** Report tier distribution, predict capacity needs, recommend policy tuning.

### Public API
```rust
pub struct TieringAnalytics {
    access_learner: Arc<AccessPatternLearner>,
    policy_engine: Arc<TieringPolicyEngine>,
    historical_data: Arc<Mutex<HistoricalData>>,
}

impl TieringAnalytics {
    pub fn new(
        access_learner: Arc<AccessPatternLearner>,
        policy_engine: Arc<TieringPolicyEngine>,
    ) -> Self { ... }

    /// Get tier distribution metrics
    pub fn get_tier_distribution(&self) -> TierDistribution { ... }

    /// Forecast capacity needs (7-day, 30-day)
    pub fn forecast_capacity(
        &self,
        days_ahead: u32,
    ) -> Result<CapacityForecast, AnalyticsError> { ... }

    /// Recommend policy tuning
    pub fn recommend_policy_tuning(&self) -> Vec<PolicyRecommendation> { ... }

    /// Get tier transition trends
    pub fn get_tier_trends(&self) -> TierTrends { ... }

    /// Report top N hot/cold segments
    pub fn get_top_segments(
        &self,
        tier: Tier,
        count: usize,
    ) -> Result<Vec<SegmentRank>, AnalyticsError> { ... }
}

#[derive(thiserror::Error, Debug)]
pub enum AnalyticsError {
    #[error("insufficient historical data")]
    InsufficientData,
    #[error("forecast error: {0}")]
    ForecastError(String),
}

pub struct TierDistribution {
    pub hot_percent: f64,
    pub warm_percent: f64,
    pub cold_percent: f64,
    pub hot_bytes: u64,
    pub warm_bytes: u64,
    pub cold_bytes: u64,
}

pub struct CapacityForecast {
    pub days_ahead: u32,
    pub projected_flash_usage: u64,
    pub projected_s3_usage: u64,
    pub projected_total_cost: f64,
    pub confidence: f64, // 0-1
}

pub struct PolicyRecommendation {
    pub recommendation: String,
    pub potential_savings: f64,
    pub confidence: f64,
}

pub struct TierTrends {
    pub hot_trend: f64,     // -1 (decreasing) to +1 (increasing)
    pub warm_trend: f64,
    pub cold_trend: f64,
    pub total_tier_transitions_per_day: f64,
}

pub struct SegmentRank {
    pub segment_id: SegmentId,
    pub tier: Tier,
    pub score: f64,
    pub size_bytes: u64,
}

struct HistoricalData {
    snapshots: Vec<(std::time::Instant, TierDistribution)>, // every hour for 7 days
}
```

### Implementation Details

**Algorithm: Track & Predict**
1. Store hourly snapshots of tier distribution (last 7 days)
2. Forecast:
   - Linear trend: project tier percentages N days ahead
   - Account for weekly seasonality (if detected)
   - Estimate S3 usage growth
3. Recommend:
   - If hot > 70%: "Lower hot threshold to tier more"
   - If cold > 50%: "Raise S3 costs or auto-delete cold"
   - If daily tier churn > 5%: "Increase hysteresis"

**Key Properties:**
- ✅ Accurate tier distribution reporting
- ✅ Predicts capacity with confidence bands
- ✅ Suggests tuning to ops teams
- ✅ Tracks historical trends

### Test Specifications

```rust
#[cfg(test)]
mod tests {
    // Test 1: Tier distribution
    #[tokio::test]
    async fn test_tier_distribution_metrics() {
        // Given: 100 segments (30 hot, 40 warm, 30 cold)
        // When: get_tier_distribution()
        // Then: hot_percent ≈ 30%, warm_percent ≈ 40%, cold_percent ≈ 30%
    }

    // Test 2: Capacity forecasting
    #[tokio::test]
    async fn test_capacity_forecasting() {
        // Given: 7 days historical data, linear growth 10%/day
        // When: forecast_capacity(7)
        // Then: projected usage ≈ current * 1.1^7
        //       confidence > 0.8
    }

    // Test 3: Policy recommendations
    #[tokio::test]
    async fn test_policy_recommendations() {
        // Given: hot_percent = 80% (high)
        // When: recommend_policy_tuning()
        // Then: includes recommendation about lowering hot threshold
        //       potential_savings > 0
    }

    // Test 4: Accuracy over time
    #[tokio::test]
    async fn test_analytics_accuracy_over_time() {
        // Given: analytics initialized
        // When: collect data for 7 days, forecast next day
        // Then: forecast error < 10% vs actual
    }
}
```

---

## Integration Testing

These 3 modules work together:

```
AccessPatternLearner
    → tracks_access
    → classifies_tier
    → detects_patterns

TieringPolicyEngine
    ← uses_patterns_from_learner
    → decides_keep_vs_tier
    → respects_tenant_hints

TieringAnalytics
    ← observes_tier_distribution
    → forecasts_capacity
    → recommends_tuning
    ← feeds_back_to_policy_engine

Loop: Learner → Policy → Analytics → (tune) → Policy
```

---

## Code Style & Conventions

**Async Runtime:**
- All I/O via Tokio
- No blocking operations in hot paths
- `#[tokio::test]` for async tests

**Error Handling:**
- Define `#[derive(thiserror::Error)]` error types
- Propagate via `?` operator

**Concurrency:**
- Use `Arc<DashMap<K, V>>` for concurrent maps
- Use `Arc<RwLock<T>>` for historical data (occasional writes)
- Use `Arc<AtomicU64>` for counters

**Testing:**
- Property-based tests with `proptest`
- Validate cost calculations with known inputs
- Test accuracy of forecasts

**Logging:**
- Use `tracing` for important policy decisions
- Include cost estimates in logs

---

## Expected Output

**Files to create:**
1. `crates/claudefs-storage/src/access_pattern_learner.rs` (~250 LOC, 8 tests)
2. `crates/claudefs-storage/src/tiering_policy_engine.rs` (~200 LOC, 6 tests)
3. `crates/claudefs-storage/src/tiering_analytics.rs` (~150 LOC, 4 tests)

**Total:** ~600 LOC, 18 tests

**Validation:**
- `cargo build -p claudefs-storage` succeeds
- `cargo test -p claudefs-storage --lib` passes 18/18 new + 1320+ Phase 10/11 tests
- `cargo clippy` has zero errors
- No regressions

---

## Notes for Implementation

1. **EMA Smoothing:** α parameter controls responsiveness (default 0.1 = smooth, 0.5 = responsive).

2. **Cost Parameters:** Calibrate to actual AWS pricing + datacenter flash costs. Make tunable via config file.

3. **Historical Data:** Keep hourly snapshots for 7 days (~168 snapshots). Aggregate to daily/weekly for older data.

4. **Forecasting:** Use exponential smoothing or ARIMA if time permits, else simple linear trend.

5. **Recommendations:** Format as actionable strings (e.g., "Lower hot_threshold from 70 to 60 to save $2.5/day").

6. **Thread Safety:** All public methods must be thread-safe. Use Arc<DashMap>, Arc<RwLock>, Arc<AtomicX> patterns.

---

## Deliverable Checklist

- [ ] All 3 modules compile
- [ ] All 18 tests pass
- [ ] Zero clippy warnings
- [ ] No regressions in Phase 10/11 tests
- [ ] Code follows ClaudeFS conventions
- [ ] Error types properly defined
- [ ] Thread safety verified
- [ ] Test coverage >90% per module
- [ ] Documentation complete
- [ ] Ready for `cargo build && cargo test --release`
