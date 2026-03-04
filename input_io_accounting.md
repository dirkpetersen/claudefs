# Task: Implement `io_accounting.rs` for claudefs-storage

## Working directory
/home/cfs/claudefs

## Output file
Write to: `crates/claudefs-storage/src/io_accounting.rs`

## Purpose
Per-tenant I/O accounting with sliding-window statistics. Tracks bytes read/written,
IOPS, and latency histograms per tenant ID. Used for quota enforcement and observability.

## Code Conventions from existing crate
- File header: `//! <purpose description>`
- Error handling: `thiserror`
- Serialization: `serde` with `Serialize, Deserialize` derives
- Logging: `tracing` crate (use `debug!`, `info!`, `warn!` macros)
- All public items: `///` doc comments (avoid `#[warn(missing_docs)]`)
- 20+ unit tests per module inside `#[cfg(test)] mod tests { ... }`
- NO `#[allow(dead_code)]` or suppression attributes
- NO async — all synchronous
- Use `std::collections::HashMap` for tenant tracking

## Data structures to implement

```rust
/// Unique tenant identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TenantId(pub u64);

/// I/O operation direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IoDirection { Read, Write }

/// Aggregate I/O counters for a tenant over a time window.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TenantIoStats {
    pub tenant_id: TenantId,
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub read_ops: u64,
    pub write_ops: u64,
    pub total_latency_us: u64,  // sum of all op latencies for avg calculation
    pub max_latency_us: u64,
    pub window_start_secs: u64,
}

/// Configuration for the I/O accounting module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IoAccountingConfig {
    pub window_secs: u64,     // sliding window size in seconds (default: 60)
    pub max_tenants: usize,   // max tracked tenants (default: 1024)
}

impl Default for IoAccountingConfig {
    fn default() -> Self {
        Self {
            window_secs: 60,
            max_tenants: 1024,
        }
    }
}
```

## IoAccounting struct and methods

```rust
pub struct IoAccounting { 
    config: IoAccountingConfig,
    tenants: HashMap<TenantId, TenantIoStats>,
    // Add any other fields you need
}

impl IoAccounting {
    /// Creates a new IoAccounting with the given configuration.
    pub fn new(config: IoAccountingConfig) -> Self
    
    /// Records an I/O operation for the given tenant.
    /// If tenant doesn't exist and max_tenants not reached, creates new entry.
    /// If max_tenants reached, silently drops the operation (no error).
    pub fn record_op(&mut self, tenant: TenantId, dir: IoDirection, bytes: u64, latency_us: u64)
    
    /// Returns the stats for the given tenant, or None if not tracked.
    pub fn get_stats(&self, tenant: TenantId) -> Option<TenantIoStats>
    
    /// Returns all tenant stats.
    pub fn all_stats(&self) -> Vec<TenantIoStats>
    
    /// Rotates the window: expires old data if current_secs > window_start + window_secs.
    /// Resets window_start_secs to current_secs. Clears all tenant data.
    pub fn rotate_window(&mut self, current_secs: u64)
    
    /// Returns the number of tracked tenants.
    pub fn tenant_count(&self) -> usize
    
    /// Returns total bytes read across all tenants.
    pub fn total_bytes_read(&self) -> u64
    
    /// Returns total bytes written across all tenants.
    pub fn total_bytes_written(&self) -> u64
    
    /// Returns top N tenants by total bytes (read + written), sorted descending.
    /// If n > tenant count, returns all tenants sorted.
    pub fn top_tenants_by_bytes(&self, n: usize) -> Vec<TenantIoStats>
}
```

## Required Tests (25 tests)
1. New accounting has no tenants
2. Record single read op
3. Record single write op
4. Multiple ops same tenant accumulate
5. Different tenants tracked independently
6. total_bytes_read returns sum across tenants
7. total_bytes_written returns sum
8. top_tenants_by_bytes returns sorted desc
9. top_tenants_by_bytes with n > total returns all
10. top_tenants_by_bytes empty returns empty
11. rotate_window clears old data
12. After rotate_window, new ops start fresh
13. get_stats for unknown tenant returns None
14. max_latency_us tracks maximum
15. tenant_count returns correct count
16. tenant_count reaches max_tenants limit (don't exceed)
17. IoDirection distinguishes read from write
18. TenantId(0) is valid
19. Record op for TenantId(u64::MAX)
20. all_stats returns all tracked tenants
21. Window keeps data within window_secs
22. Multiple windows accumulate correctly
23. Bytes counted per op (not per record call)
24. Default config has window_secs=60
25. Rotate window does not lose in-window data (i.e., rotate only clears if window expired)

## Implementation Notes
- Use HashMap<TenantId, TenantIoStats> for tenant storage
- The window_start_secs should be set when first op is recorded, or when new() is called (use 0 initially)
- rotate_window should check if current_secs > window_start_secs + window_secs before clearing
- top_tenants_by_bytes: collect all stats, sort by (bytes_read + bytes_written) descending
- For max_latency_us: update if new latency > current max

## Final output
Write the complete Rust file with all structures, implementations, and tests.