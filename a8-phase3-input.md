# A8 Phase 3: Query Gateway, Web API, CLI & Dashboards Implementation

## Context

Agent A8 (Management) owns the `claudefs-mgmt` crate. Phase 2 is complete with 965 tests and 43 modules covering:
- Prometheus metrics collection
- Metadata journal consumer (A2 integration)
- DuckDB analytics engine with query methods (top_users, top_dirs, stale_files, reduction_stats)
- Metadata indexing (Parquet writer and flushing)

Phase 3 builds on this foundation to deliver the Query Gateway, Web API, CLI enhancements, and Grafana dashboard templates. Target: 1100+ tests (+30-40 new tests).

## Phase 3 Architecture

ClaudeFS separates monitoring into three daemons:
1. `claudefs-monitor` (Prometheus exporter) — Real-time telemetry, alerts
2. `claudefs-index` (Metadata indexer) — Tails Raft log, writes Parquet files
3. `claudefs-admin` (Query gateway + Web UI) — DuckDB query interface, REST API, CLI, dashboards

A8 implements claudefs-admin with query gateway, web API, RBAC, CLI shortcuts, and pre-built Grafana dashboards.

## Parquet Schema

The indexer produces Parquet files with this schema (already written by analytics.rs):

```
/index/year=2026/month=02/day=28/metadata_00001.parquet
├── inode: uint64
├── path: string
├── filename: string
├── parent_path: string
├── owner_uid: uint32
├── owner_name: string
├── group_gid: uint32
├── group_name: string
├── size_bytes: uint64
├── blocks_stored: uint64
├── mtime: timestamp
├── ctime: timestamp
├── file_type: string
└── is_replicated: bool
```

## Block 1: Query Gateway (10-12 tests) — NEW MODULES

### 1.1 query_gateway.rs (~10-12 tests)

**Purpose:** DuckDB connection pooling, parameterized queries, query caching, and execution timeouts.

**Key Types:**
```rust
pub struct QueryGateway {
    connections: Arc<DuckDbConnectionPool>,
    cache: Arc<QueryResultCache>,
    query_timeout_secs: u32,
    max_result_rows: usize,
}

pub struct DuckDbConnectionPool {
    available: Arc<tokio::sync::Mutex<Vec<Connection>>>,
    in_use: Arc<AtomicUsize>,
    max_connections: usize,
}

pub struct QueryResultCache {
    cache: Arc<DashMap<String, CachedResult>>,
    ttl_secs: u32,
}

pub struct CachedResult {
    rows: Vec<Row>,
    cached_at: Instant,
    ttl_secs: u32,
}

pub enum QueryError {
    PoolExhausted,
    Timeout,
    InvalidQuery(String),
    NoResults,
}
```

**Methods:**
- `QueryGateway::new(path: &str, max_connections: usize, query_timeout_secs: u32) -> Result<Self>`
- `async fn execute_query(&self, query: &str, use_cache: bool) -> Result<Vec<Row>>`
- `async fn execute_parameterized(&self, query: &str, params: &[String], use_cache: bool) -> Result<Vec<Row>>`
- `fn get_pool_stats() -> PoolStats` (available, in_use, max)
- `fn clear_cache(&self)`
- `fn get_cache_stats() -> CacheStats` (hits, misses, entries, memory_bytes)

**Tests (10-12):**
1. `test_query_gateway_new_succeeds`
2. `test_query_gateway_execute_query_succeeds`
3. `test_query_gateway_execute_query_timeout`
4. `test_query_gateway_execute_query_with_cache_hit`
5. `test_query_gateway_execute_query_with_cache_miss`
6. `test_query_gateway_execute_query_with_cache_ttl_expiry`
7. `test_query_gateway_parameterized_query_sql_injection_safe`
8. `test_query_gateway_execute_query_no_results`
9. `test_query_gateway_connection_pool_exhaustion`
10. `test_query_gateway_connection_pool_concurrent_requests`
11. `test_query_gateway_clear_cache`
12. `test_query_gateway_get_pool_stats`

---

## Block 2: Web API (8-10 tests) — NEW MODULE

### 2.1 web_api.rs (~8-10 tests)

**Purpose:** Axum HTTP server providing REST endpoints for analytics queries and cluster health.

**Key Types:**
```rust
pub struct ApiServer {
    listener: TcpListener,
    gateway: Arc<QueryGateway>,
    rbac: Arc<RbacManager>,
    health: Arc<HealthChecker>,
}

#[derive(serde::Serialize)]
pub struct ApiResponse<T> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
    timestamp: i64,
}

pub struct ClusterHealthStatus {
    status: HealthState, // Healthy, Degraded, Critical
    nodes: Vec<NodeStatus>,
    replication_lag_ms: u64,
    capacity_used_percent: u32,
    last_check_at: i64,
}

pub enum HealthState {
    Healthy,
    Degraded,
    Critical,
}
```

**Routes:**
- `GET /api/v1/health` → cluster health status
- `GET /api/v1/analytics/top-users?limit=20&days=7` → top N users by storage consumption
- `GET /api/v1/analytics/top-dirs?limit=20&days=7` → top N directories
- `GET /api/v1/analytics/stale-files?days_threshold=30&limit=100` → files not accessed
- `GET /api/v1/analytics/file-types` → file type distribution
- `GET /api/v1/analytics/reduction-report` → dedup/compression statistics
- `POST /api/v1/analytics/query` → parameterized SQL query (requires admin role)

**Error Handling:**
- 400: Invalid parameters (e.g., days=-1, limit=0)
- 401: Missing/invalid auth token
- 403: Insufficient RBAC permissions
- 404: Resource not found
- 408: Query timeout
- 500: Internal error

**Tests (8-10):**
1. `test_api_server_new_succeeds`
2. `test_api_server_health_endpoint_returns_cluster_status`
3. `test_api_server_top_users_endpoint_returns_results`
4. `test_api_server_top_users_endpoint_validates_parameters`
5. `test_api_server_query_endpoint_requires_admin_role`
6. `test_api_server_query_endpoint_timeout_returns_408`
7. `test_api_server_json_error_response_format`
8. `test_api_server_stale_files_endpoint`
9. `test_api_server_file_types_endpoint`
10. `test_api_server_reduction_report_endpoint`

---

## Block 3: Authentication & RBAC (5-7 tests) — NEW MODULE

### 3.1 web_auth.rs (~5-7 tests)

**Purpose:** JWT validation, OIDC token verification, and RBAC middleware.

**Key Types:**
```rust
pub struct AuthManager {
    jwt_secret: String,
    oidc_enabled: bool,
    oidc_provider_url: Option<String>,
    role_cache: Arc<DashMap<String, UserRole>>,
}

pub enum UserRole {
    Admin,
    Operator,
    Viewer,
    TenantAdmin(String), // tenant_id
}

pub struct AuthClaim {
    sub: String,       // Subject (user ID)
    iat: i64,          // Issued at
    exp: i64,          // Expiration
    roles: Vec<String>,
}

pub enum AuthError {
    MissingToken,
    InvalidToken(String),
    TokenExpired,
    InsufficientPermission,
    OidcError(String),
}
```

**Methods:**
- `AuthManager::new(jwt_secret: &str, oidc_enabled: bool, oidc_provider_url: Option<&str>) -> Self`
- `async fn verify_token(&self, token: &str) -> Result<AuthClaim>`
- `fn has_permission(&self, role: &UserRole, action: &str, resource: &str) -> bool`
- `async fn extract_bearer_token(headers: &HeaderMap) -> Result<String>`
- `fn require_role(required: UserRole) -> impl Middleware` (Axum middleware)

**Roles & Permissions:**
- Admin: all operations
- Operator: read, top_users, top_dirs, stale_files, health checks
- Viewer: read-only (no parameterized queries)
- TenantAdmin(tenant_id): read + write within tenant namespace only

**Tests (5-7):**
1. `test_auth_manager_new_succeeds`
2. `test_auth_manager_verify_token_valid`
3. `test_auth_manager_verify_token_expired`
4. `test_auth_manager_verify_token_invalid_signature`
5. `test_auth_manager_has_permission_admin`
6. `test_auth_manager_has_permission_viewer_read_only`
7. `test_auth_manager_require_role_middleware_blocks_insufficient_role`

---

## Block 4: CLI & Dashboards (6-8 tests) — ENHANCED + NEW MODULE

### 4.1 Enhanced cli.rs (~2-3 tests)

**Add new subcommands to existing cli.rs:**
```rust
// New shortcuts:
// cfs-mgmt top-users [--days 7] [--limit 20]
// cfs-mgmt top-dirs [--days 7] [--limit 20]
// cfs-mgmt find [--name pattern] [--size-gt bytes] [--mtime-days days]
// cfs-mgmt stale [--days threshold] [--limit 100]
// cfs-mgmt reduction-report
// cfs-mgmt cluster status

pub struct QueryCommand {
    #[command(subcommand)]
    pub command: QuerySubcommand,
}

pub enum QuerySubcommand {
    TopUsers { days: u32, limit: usize },
    TopDirs { days: u32, limit: usize },
    Find { name: Option<String>, size_gt: Option<u64>, mtime_days: Option<u32> },
    Stale { days: u32, limit: usize },
    ReductionReport,
    ClusterStatus,
}
```

**Tests (2-3):**
1. `test_cli_top_users_command_parsed`
2. `test_cli_find_command_with_pattern_parsed`
3. `test_cli_cluster_status_command_parsed`

### 4.2 dashboards.rs (NEW MODULE) (~4-5 tests)

**Purpose:** Pre-built Grafana dashboard JSON templates (cluster health, top consumers, capacity trends, reduction analytics).

**Key Types:**
```rust
pub struct DashboardGenerator;

pub struct GrafanaDashboard {
    title: String,
    panels: Vec<GrafanaPanel>,
    refresh: String,
}

pub struct GrafanaPanel {
    title: String,
    datasource: String,
    targets: Vec<PrometheusTarget>,
    panel_type: String, // "graph", "stat", "table"
}

pub enum DashboardTemplate {
    ClusterHealth,
    TopConsumers,
    CapacityTrends,
    ReductionAnalytics,
}
```

**Methods:**
- `DashboardGenerator::generate_dashboard(template: DashboardTemplate) -> GrafanaDashboard`
- `DashboardGenerator::export_dashboard_json(dashboard: &GrafanaDashboard) -> String`
- `DashboardGenerator::generate_all_dashboards() -> Vec<(String, String)>` // (name, json)

**Dashboard Panels:**
1. **ClusterHealth**: Node status, capacity, replication lag
2. **TopConsumers**: Bar chart of top users/directories
3. **CapacityTrends**: Time-series of capacity over time
4. **ReductionAnalytics**: Dedup ratio, compression ratio, savings

**Tests (4-5):**
1. `test_dashboard_generator_cluster_health_dashboard`
2. `test_dashboard_generator_top_consumers_dashboard`
3. `test_dashboard_generator_export_dashboard_json_valid_format`
4. `test_dashboard_generator_all_dashboards_generated`
5. `test_dashboard_generator_grafana_panel_targets_correct_datasource`

---

## Block 5: Integration Tests (4-6 tests) — NEW MODULE

### 5.1 integration_tests.rs (within tests/) (~4-6 tests)

**Purpose:** End-to-end workflows testing Parquet → API → CLI → Dashboard integration.

**Test Scenarios:**
1. Parquet file indexing → DuckDB query → JSON response
2. CLI query → API → result formatting
3. Grafana dashboard → Prometheus scrape → metrics present
4. Auth flow with JWT token
5. Query result caching and cache invalidation
6. Performance test: 100K records query

**Tests (4-6):**
1. `test_e2e_parquet_indexing_to_api_response`
2. `test_e2e_cli_top_users_invokes_api`
3. `test_e2e_grafana_dashboard_metrics_available`
4. `test_e2e_auth_jwt_token_flow`
5. `test_e2e_query_caching_and_invalidation`
6. `test_e2e_performance_100k_records_query_completes_in_500ms`

---

## Dependencies to Add to Cargo.toml

```toml
# JWT / Auth
jsonwebtoken = { version = "9.2", features = ["use_pem"] }
base64 = "0.21"

# HTTP / Web
axum-core = "0.4"
tower-layer = "0.1"
tower-service = "0.3"

# Database connection pooling
bb8 = "0.8"

# JSON serialization (already have serde_json)

# Time/Scheduling
tokio-util = { version = "0.7", features = ["time"] }
```

---

## Implementation Order

1. **query_gateway.rs** — Foundation for all queries (DuckDB connection pooling, caching)
2. **web_api.rs** — REST endpoints (depends on query_gateway)
3. **web_auth.rs** — Authentication middleware (depends on web_api routes)
4. **Enhanced cli.rs** — New shortcuts (depends on query_gateway)
5. **dashboards.rs** — Grafana templates (standalone but uses query_gateway for data)
6. **integration_tests.rs** — E2E tests (depends on all above)

---

## Testing Strategy

- **Unit tests** in each module (10-12 for gateway, 8-10 for web_api, etc.)
- **Integration tests** in separate module testing full workflows
- **Mock DuckDB** for query testing without real Parquet files
- **Mock JWT tokens** for auth testing
- Target: 965 + 30-40 = 995-1005 tests minimum, 1000-1100 with integration coverage

---

## Success Criteria

1. ✅ All 30-40 new tests passing
2. ✅ Build clean, no clippy errors
3. ✅ Query gateway handles 100K record queries in <500ms
4. ✅ Web API returns proper JSON responses with errors
5. ✅ RBAC middleware blocks unauthorized requests
6. ✅ CLI shortcuts work end-to-end
7. ✅ Grafana dashboards JSON is valid and Grafana-compatible

---

## Notes

- All error types should use `#[derive(thiserror::Error)]` pattern
- Use `async/await` with tokio runtime
- Structure code for testability (mock traits for DuckDB connection)
- Keep line counts reasonable (< 400 lines per file)
- Add module-level documentation comments
- Use existing patterns from analytics.rs, api.rs for consistency
