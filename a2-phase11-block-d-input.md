# A2 Phase 11 Block D: billing_aggregator.rs Implementation

**Crate:** claudefs-meta
**File:** crates/claudefs-meta/src/billing_aggregator.rs
**Target:** 350 LOC, 15 tests, 0 warnings
**Model:** minimax-m2p5

---

## Context

Building on Phase 11 Block B (audit logging) and Phase 10's quota_tracker, Block D implements tenant billing and chargeback metering.

**Goal:** Track storage/IOPS/network usage per tenant, compute hourly costs, generate monthly invoices.

---

## Requirements

### 1. Core Data Structures

```rust
/// Single billing record for a tenant in a 1-hour window
#[derive(Clone, Debug)]
pub struct BillingRecord {
    /// Unique record ID (UUID)
    pub record_id: String,
    /// Tenant ID being billed
    pub tenant_id: String,
    /// Time window: (start_ms, end_ms) — 1-hour window
    pub time_window: (u64, u64),
    /// Storage usage in GB-hours
    pub storage_gb_hours: f64,
    /// IOPS count in this window
    pub iops_count: u64,
    /// Network egress in GB
    pub network_gb: f64,
    /// Snapshot storage in GB-hours
    pub snapshot_gb: f64,
    /// Total cost in USD
    pub cost_usd: f64,
    /// Cost breakdown by service
    pub cost_breakdown: CostBreakdown,
    /// Status: Pending/Finalized/Disputed
    pub status: BillingStatus,
}

/// Cost breakdown by service
#[derive(Clone, Debug, Default)]
pub struct CostBreakdown {
    /// Storage cost (USD)
    pub storage_cost: f64,
    /// IOPS cost (USD)
    pub iops_cost: f64,
    /// Network cost (USD)
    pub network_cost: f64,
    /// Snapshot cost (USD)
    pub snapshot_cost: f64,
    /// Volume discounts applied (USD, negative)
    pub discounts: f64,
    /// Taxes (USD, varies by region)
    pub taxes: f64,
}

/// Status of billing record
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BillingStatus {
    /// Record created but not yet finalized
    Pending,
    /// Finalized and ready for invoice
    Finalized,
    /// Disputed by tenant (reason included)
    Disputed(String),
    /// Invoice paid
    Paid,
}

/// Billing pricing configuration
#[derive(Clone, Debug)]
pub struct BillingPricing {
    /// $/GB-hour for storage
    pub storage_gb_hour_usd: f64,
    /// $/1M IOPS
    pub iops_million_usd: f64,
    /// $/GB egress
    pub network_gb_usd: f64,
    /// $/GB-hour for snapshots
    pub snapshot_gb_hour_usd: f64,
    /// GB threshold for volume discount
    pub volume_discount_threshold: u32,
    /// Percentage discount (e.g., 0.1 for 10%)
    pub volume_discount_pct: f64,
}

/// Monthly billing report
#[derive(Clone, Debug)]
pub struct BillingReport {
    /// Report start time (Unix epoch ms)
    pub start_time: u64,
    /// Report end time (Unix epoch ms)
    pub end_time: u64,
    /// Number of unique tenants billed
    pub total_tenants_billed: usize,
    /// Total revenue (USD)
    pub total_revenue_usd: f64,
    /// Revenue by service: { "storage", "iops", "network", "snapshots" }
    pub revenue_by_service: std::collections::HashMap<String, f64>,
    /// Top 10 tenants by cost: [(tenant_id, cost_usd)]
    pub top_10_tenants: Vec<(String, f64)>,
    /// Disputed records count
    pub disputed_records: usize,
}

/// Billing aggregator (holds billing state)
pub struct BillingAggregator {
    /// Records: "tenant-time" → record
    pub hourly_records: Arc<DashMap<String, BillingRecord>>,
    /// Pricing configuration
    pub pricing: BillingPricing,
    /// Currency exchange rates: currency code → USD rate
    pub exchange_rates: Arc<DashMap<String, f64>>,
}
```

---

### 2. Public Functions

#### 2.1 `new`

```rust
pub fn new(pricing: BillingPricing) -> Self
```

**Behavior:**
- Create BillingAggregator with empty records map
- Initialize pricing
- Initialize exchange rates with USD (1.0), EUR, GBP, JPY (standard rates)
- Return initialized aggregator

---

#### 2.2 `record_usage`

```rust
pub fn record_usage(
    &self,
    tenant_id: &str,
    storage_bytes: u64,
    iops_count: u64,
    network_bytes: u64,
    snapshot_bytes: u64,
    timestamp_ms: u64,
) -> Result<String, String>
```

**Behavior:**
- Determine 1-hour window: window_start = (timestamp_ms / 3600000) * 3600000
- Build key: format!("{tenant_id}-{window_start}")
- If key exists: update record (accumulate usage)
- If new: create BillingRecord with Pending status
- Convert bytes to GB (/ 1_000_000_000)
- Return record_id

---

#### 2.3 `finalize_billing_window`

```rust
pub fn finalize_billing_window(
    &self,
    start_time_ms: u64,
    quota_tracker: &crate::quota_tracker::QuotaTracker,
) -> Result<usize, String>
```

**Behavior:**
- Find all Pending records in window [start_time_ms, start_time_ms + 3600000)
- For each record:
  - Fetch current usage from quota_tracker
  - Calculate costs:
    - storage_cost = storage_gb_hours * pricing.storage_gb_hour_usd
    - iops_cost = (iops_count / 1_000_000) * pricing.iops_million_usd
    - network_cost = network_gb * pricing.network_gb_usd
    - snapshot_cost = snapshot_gb * pricing.snapshot_gb_hour_usd
  - Subtotal = storage_cost + iops_cost + network_cost + snapshot_cost

  - Apply volume discount:
    - If storage_gb_hours > volume_discount_threshold:
      - discount = subtotal * pricing.volume_discount_pct
  - Apply negotiated pricing (tenant-specific override, if exists)
  - Apply regional taxes (hardcoded 0.08 for now, 8%)

  - Total cost = subtotal - discount + taxes
  - Set status = Finalized
  - Store CostBreakdown

- Return count of finalized records

---

#### 2.4 `apply_volume_discount`

```rust
pub fn apply_volume_discount(
    &self,
    storage_gb_hours: f64,
    subtotal_usd: f64,
) -> f64
```

**Behavior:**
- If storage_gb_hours > pricing.volume_discount_threshold as f64:
  - discount = subtotal_usd * pricing.volume_discount_pct
  - return subtotal_usd - discount
- Else:
  - return subtotal_usd

---

#### 2.5 `query_tenant_bill`

```rust
pub fn query_tenant_bill(
    &self,
    tenant_id: &str,
    start_time_ms: u64,
    end_time_ms: u64,
) -> (f64, CostBreakdown)
```

**Behavior:**
- Find all finalized records for tenant in time range
- Sum costs and breakdowns
- Return (total_cost_usd, aggregated CostBreakdown)

---

#### 2.6 `dispute_billing_record`

```rust
pub fn dispute_billing_record(
    &self,
    record_id: &str,
    reason: &str,
    audit_logger: &Arc<DashMap<String, String>>,
) -> Result<(), String>
```

**Behavior:**
- Find record by ID
- Mark status = Disputed(reason)
- Log to audit: "Record {record_id} disputed: {reason}"
- Return Ok

---

#### 2.7 `generate_monthly_invoice`

```rust
pub fn generate_monthly_invoice(
    &self,
    tenant_id: &str,
    month_start_ms: u64,
    month_end_ms: u64,
) -> Result<String, String>
```

**Behavior:**
- Query all finalized records for tenant in month
- Compute totals with tax (0.08)
- Format as JSON:
  ```json
  {
    "tenant_id": "...",
    "invoice_date": "2026-04-01",
    "period": "2026-04-01 to 2026-05-01",
    "items": [
      { "description": "Storage (GB-hours)", "amount": ... },
      { "description": "IOPS", "amount": ... },
      ...
    ],
    "subtotal": ...,
    "tax": ...,
    "total": ...,
    "payment_terms": "net-30"
  }
  ```
- Return as JSON string
- Could also export to file

---

#### 2.8 `compute_mrr`

```rust
pub fn compute_mrr(
    &self,
    tenant_id: &str,
    last_30_days_cost_usd: f64,
) -> f64
```

**Behavior:**
- MRR = last_30_days_cost_usd (assuming repeating pattern)
- Return MRR

---

### 3. Tests (15 total)

1. **test_billing_record_storage** — Charge per GB-hour
2. **test_billing_record_iops** — Charge per 1M IOPS
3. **test_billing_record_network** — Charge per GB egress
4. **test_billing_record_snapshot** — Charge per snapshot GB-hour
5. **test_billing_finalize_window** — Close hour, calculate costs
6. **test_billing_volume_discount** — 10% discount for >100GB
7. **test_billing_query_tenant_bill** — Fetch bill for tenant/period
8. **test_billing_dispute_record** — Mark record disputed
9. **test_billing_generate_monthly_invoice** — Create invoice JSON
10. **test_billing_multiple_tenants** — Bill 10 tenants same window
11. **test_billing_accumulate_usage** — Multiple calls to record_usage
12. **test_billing_cost_breakdown** — Verify breakdown accuracy
13. **test_billing_tax_calculation** — Regional tax applied
14. **test_billing_mrr_forecast** — MRR from current usage
15. **test_billing_currency_conversion** — EUR/GBP/JPY rates

---

## Deliverable

**Output file:** `crates/claudefs-meta/src/billing_aggregator.rs`

**Specifications:**
- 330-380 LOC
- 15 tests
- Zero warnings
- Imports: std::*, dashmap::*, serde_json, thiserror

---

## Integration Notes

- Works with Phase 10 quota_tracker: usage source
- Works with Phase 11 Block B tenant_audit: dispute logging
- Works with A8 Prometheus: cost metrics export
- Output: Monthly invoices, MRR forecasting, disputed record tracking

**Expected integration time:** 1-2 hours
