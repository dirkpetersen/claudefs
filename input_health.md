# Task: Implement device_health_monitor.rs module for claudefs-storage

## Location
Create file: `/home/cfs/claudefs/crates/claudefs-storage/src/device_health_monitor.rs`

## Purpose
Aggregate health monitoring for storage devices, combining SMART data, wear level information, and capacity usage into a unified health score and alert system.

## Requirements

### Data Structures

```rust
/// SMART data snapshot from a device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartSnapshot {
    pub reallocated_sectors: u32,
    pub media_errors: u64,
    pub unsafe_shutdowns: u32,
    pub temperature_celsius: u8,
    pub percentage_used: u8,  // 0-100, NVMe SMART attribute
}

/// Wear level data snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WearSnapshot {
    pub wear_percentage_used: u8,  // 0-100
    pub power_on_hours: u64,
}

/// Summary of a device's health
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceHealthSummary {
    pub device_idx: u16,
    pub device_path: String,
    pub health_score: f64,  // 0.0-1.0
    pub capacity_pct_free: f64,
    pub wear_pct_used: u8,
    pub temperature_celsius: u8,
    pub last_updated: u64,  // unix timestamp secs
}

/// Type of health alert
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthAlertType {
    LowCapacity,      // free < 20%
    HighWear,          // wear > 80%
    MediaErrors,       // media_errors > 0
    HighTemperature,   // temp > 70°C
    CriticalHealth,    // overall score < 0.3
}

/// Severity level for alerts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertSeverity {
    Warning,
    Critical,
}

/// A health alert for a device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthAlert {
    pub device_idx: u16,
    pub alert_type: HealthAlertType,
    pub severity: AlertSeverity,
    pub message: String,
}
```

### DeviceHealthMonitor

```rust
/// Device health monitor tracking multiple devices
pub struct DeviceHealthMonitor {
    // Internal state:
    // - HashMap<device_idx, DeviceState>
    // DeviceState includes: path, smart, wear, capacity, last_updated
}

impl DeviceHealthMonitor {
    pub fn new() -> Self;
    
    /// Register a device for monitoring
    pub fn register_device(&mut self, device_idx: u16, device_path: String);
    
    /// Update SMART data for a device
    pub fn update_smart(&mut self, device_idx: u16, smart: SmartSnapshot);
    
    /// Update wear level data for a device
    pub fn update_wear(&mut self, device_idx: u16, wear: WearSnapshot);
    
    /// Update capacity data for a device
    pub fn update_capacity(&mut self, device_idx: u16, total_bytes: u64, free_bytes: u64);
    
    /// Compute health score for a device (0.0-1.0)
    /// Returns 0.0 for unregistered devices or devices with no data
    pub fn compute_health_score(&self, device_idx: u16) -> f64;
    
    /// Get health summary for all devices, sorted by score ascending (unhealthiest first)
    pub fn health_summary(&self) -> Vec<DeviceHealthSummary>;
    
    /// Check for alerts on all devices
    pub fn check_alerts(&self) -> Vec<HealthAlert>;
}

impl Default for DeviceHealthMonitor {
    fn default() -> Self {
        Self::new()
    }
}
```

### Health Score Calculation

Compute a weighted average of three component scores:

1. **Wear Score**: `(100 - wear_pct) / 100.0`
   - 0% wear used → 1.0 score
   - 50% wear used → 0.5 score
   - 100% wear used → 0.0 score

2. **Capacity Score**: `free_bytes / total_bytes` clamped to [0.0, 1.0]
   - 100% free → 1.0 score
   - 50% free → 0.5 score
   - 0% free → 0.0 score

3. **SMART Score**: Start at 1.0, apply penalties:
   - For each media error: subtract 0.05 (min 0.0)
   - For each reallocated sector: subtract 0.01 (min 0.0)
   - Use formula: `1.0 - (media_errors as f64 * 0.05).min(1.0) - (reallocated_sectors as f64 * 0.01).min(1.0)`
   - Clamp to [0.0, 1.0]

**Overall Health Score**: Weighted average
- Wear weight: 0.4
- Capacity weight: 0.3
- SMART weight: 0.3

Formula:
```rust
let wear_score = (100.0 - wear.wear_percentage_used as f64) / 100.0;
let capacity_score = (free_bytes as f64 / total_bytes as f64).clamp(0.0, 1.0);
let smart_score = calculate_smart_score(&smart);
let overall = wear_score * 0.4 + capacity_score * 0.3 + smart_score * 0.3;
```

### Alert Thresholds

Generate alerts when:

- **LowCapacity**: free_bytes / total_bytes < 0.20 (20% free)
  - Severity: Warning if 10-20%, Critical if < 10%
  
- **HighWear**: wear_percentage_used > 80
  - Severity: Warning if 80-90%, Critical if > 90%
  
- **MediaErrors**: media_errors > 0
  - Severity: Critical (any media error is serious)
  
- **HighTemperature**: temperature_celsius > 70
  - Severity: Warning if 70-80°C, Critical if > 80°C
  
- **CriticalHealth**: health_score < 0.3
  - Severity: Critical (device is nearly failed)

### DeviceState Tracking

```rust
struct DeviceState {
    device_path: String,
    smart: Option<SmartSnapshot>,
    wear: Option<WearSnapshot>,
    total_bytes: Option<u64>,
    free_bytes: Option<u64>,
    last_updated: u64,  // unix timestamp
}
```

- If any component (smart/wear/capacity) is None, use defaults:
  - Missing SMART → assume healthy (score 1.0)
  - Missing Wear → assume 0% wear (score 1.0)
  - Missing Capacity → assume 100% free (score 1.0)
  
- For `DeviceHealthSummary`, use last known values or defaults:
  - Missing temperature → report 0
  - Missing wear → report 0

### Test Coverage (at least 20 unit tests)

1. Register device and check health before any data → default healthy score (1.0 or near 1.0)
2. Update SMART with 0 errors → high SMART score (1.0)
3. Update SMART with media errors → SMART score penalized
4. Update wear at 0% → wear score 1.0
5. Update wear at 100% → wear score 0.0
6. Update wear at 50% → wear score 0.5
7. Capacity at 100% free → capacity score 1.0
8. Capacity at 0% free → capacity score 0.0
9. Capacity at 50% free → capacity score 0.5
10. Overall health score is weighted average of components
11. health_summary returns all devices sorted by score ascending (unhealthiest first)
12. check_alerts: no alerts when device is healthy
13. check_alerts: LowCapacity alert at < 20% free
14. check_alerts: HighWear alert at > 80% wear
15. check_alerts: MediaErrors alert when media_errors > 0
16. check_alerts: HighTemperature alert at > 70°C
17. check_alerts: CriticalHealth alert at score < 0.3
18. check_alerts: Warning vs Critical severity correctly assigned
19. Unregistered device: compute_health_score returns 0.0
20. Multiple devices tracked independently

## Style Rules
- All public structs/enums/fns MUST have `///` doc comments
- Use `thiserror` for any errors (probably not needed for this module)
- Use `serde` + `bincode` derives: `#[derive(Debug, Clone, Serialize, Deserialize)]`
- Use `tracing` crate: `use tracing::{debug, info, warn, error};`
- Use `std::collections::HashMap` for device tracking
- No `unwrap()` in production code
- Tests use `#[test]` (sync), not async
- Idiomatic Rust: iterators, no manual index loops
- Use `std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()` for current timestamp in tests

## Important Note on Naming Conflict
The crate already exports `AlertSeverity` from `smart.rs`. To avoid conflict, users will import this module's `AlertSeverity` with an alias. The lib.rs will export it as:
```rust
pub use device_health_monitor::{
    AlertSeverity as HealthAlertSeverity, ...
};
```

## Output
Return the complete Rust code for device_health_monitor.rs with all structs, impls, and tests.