# A11: Fix web_api.rs Module & Resolve Issue #27 (Send+Sync Compilation Errors)

## Context

**Issue #27:** QueryGateway Arc is not Send in AdminApiState tokio::spawn. Also cannot call set_timeout on Arc<QueryGateway>. Need to fix interior mutability and Send bounds.

**Current Status:**
- web_api.rs module is missing (declared in lib.rs but file doesn't exist)
- Compilation fails: `error[E0583]: file not found for module web_api`
- QueryGateway has interior mutability issues preventing Send+Sync impl
- AdminApiState cannot be moved into tokio::spawn tasks

## Requirements

### 1. Create web_api.rs Module

Must export these types (from lib.rs):
- `create_router()` — Axum router factory function
- `AppState` — Application state struct
- `ApiError` — Error type
- `TopUser`, `TopDir`, `StaleFile` — Query result types
- `FileTypeStats` — Statistics struct
- `ReductionReport` — Report struct
- `ClusterHealth` — Health struct
- `CustomQueryRequest` — Request struct

### 2. Fix Interior Mutability Issues

**Current problem:**
- QueryGateway holds DuckDB Connection in an Arc, but needs mutable access
- Cannot call set_timeout on Arc<QueryGateway> (requires &mut self)
- AdminApiState wraps Arc<QueryGateway> which is not Send

**Solution approach:**
- Use `Arc<RwLock<QueryGateway>>` or `Arc<Mutex<QueryGateway>>` for interior mutability
- Update QueryGateway type signature to use `RwLock` for read-heavy workloads
- Ensure AdminApiState impl Send+Sync with proper lifetime handling
- Update tokio::spawn calls to work with cloned Arc references

### 3. Implementation Details

**File structure:**
```rust
// crates/claudefs-mgmt/src/web_api.rs

use axum::{...};
use std::sync::{Arc, RwLock};
use crate::query_gateway::QueryGateway;
use crate::AppState; // or define here

// Core types (from lib.rs exports)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    pub gateway: Arc<RwLock<QueryGateway>>,
    // ... other fields
}

// Error type
#[derive(Debug)]
pub enum ApiError {
    // ... variants
}

// Result types
#[derive(Debug, Serialize, Deserialize)]
pub struct TopUser { /* ... */ }
#[derive(Debug, Serialize, Deserialize)]
pub struct TopDir { /* ... */ }
#[derive(Debug, Serialize, Deserialize)]
pub struct StaleFile { /* ... */ }
#[derive(Debug, Serialize, Deserialize)]
pub struct FileTypeStats { /* ... */ }
#[derive(Debug, Serialize, Deserialize)]
pub struct ReductionReport { /* ... */ }
#[derive(Debug, Serialize, Deserialize)]
pub struct ClusterHealth { /* ... */ }

#[derive(Debug, Deserialize)]
pub struct CustomQueryRequest {
    pub query: String,
    // ... other fields
}

// Router factory
pub fn create_router(state: AppState) -> axum::Router {
    // Build Axum router with:
    // - GET /metrics — Prometheus metrics
    // - POST /query — DuckDB query gateway
    // - GET /health — Cluster health
    // - ... other endpoints
}

// Handler functions
async fn query_handler(...) -> Result<Json<...>, ApiError> { }
async fn health_handler(...) -> Result<Json<ClusterHealth>, ApiError> { }
// ... other handlers
```

**Key patterns:**
1. Use `Arc<RwLock<QueryGateway>>` for interior mutability + Send+Sync
2. In async handlers, clone the Arc and acquire the lock within the async context
3. Avoid holding locks across await points where possible
4. Use separate read-only queries (RwLock::read) where mutations aren't needed

### 4. Integration Points

- **QueryGateway** (query_gateway.rs): Must impl Send+Sync after using interior mutability
- **AdminApi** (api.rs): Should use web_api::create_router internally if not already
- **lib.rs exports**: All 8 types listed above must be re-exported

### 5. Testing

- Compilation must pass: `cargo check` and `cargo build --release`
- All tokio::spawn uses must compile without Send bound errors
- QueryGateway timeouts must work through Arc<RwLock<_>> wrapping

## File Location

`crates/claudefs-mgmt/src/web_api.rs`

## Success Criteria

✅ `cargo check` passes (no errors)
✅ `cargo build --release` passes
✅ web_api module exports all 8 required types
✅ AppState impl Send+Sync
✅ QueryGateway compiles with interior mutability (Arc<RwLock<_>>)
✅ tokio::spawn tasks can use AppState without Send errors
✅ Issue #27 is resolved (verified by explicit Send+Sync impl)

## Additional Context

- This is blocking A11 Phase 4 Block 2 (Metrics Integration)
- All builder agents (A1-A8) depend on this fix to enable monitoring
- Production deployment readiness hinges on this fix
- Async runtime: Tokio with io_uring backend
- Serialization: serde + bincode for wire protocol
