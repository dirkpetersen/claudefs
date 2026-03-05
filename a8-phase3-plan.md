# A8: Management — Phase 3 Planning: Query Gateway, Web UI, CLI Integration

**Status:** Phase 2 ✅ Complete (965 tests) → Phase 3 Planning
**Current:** 965 tests, 38+ modules
**Target:** 1100+ tests with ~50 total modules (30+ new tests in Phase 3)

## Phase 3 Overview: From Metrics to Insight

Phase 2 built the **data pipeline** (journal consumer → Parquet writer, metrics collection). Phase 3 builds the **query/visualization layer** that lets operators explore that data via SQL, Web UI, and CLI.

### Architecture: Complete Three-Daemon Stack

```
Phase 2 ✅                          Phase 3 🎯
─────────────────────────────────────────────────
claudefs-monitor (metrics)   →   Prometheus Scraper
Claude-index (Parquet)       →   + Grafana Dashboards
                             →   Query Gateway API (DuckDB)
                             →   Web UI (React)
                             →   CLI Tools
```

## Phase 3 Modules (Target: 30+ new tests)

### Block 1: Query Gateway (10-12 tests)

**`query_gateway.rs`** — HTTP/gRPC API for DuckDB queries
- Opens persistent DuckDB connection to Parquet index directory
- Handles parameterized queries (sanitize inputs, prevent SQL injection)
- Result streaming for large datasets (chunked JSON responses)
- Query timeout & cancellation support
- Query result caching (10-min TTL) for expensive aggregates
- Tests:
  - `test_query_gateway_new` — Initialize with index path
  - `test_query_gateway_execute_top_users` — Execute predefined query
  - `test_query_gateway_sql_injection_prevention` — Reject malicious SQL
  - `test_query_gateway_query_timeout` — Enforce timeout on slow queries
  - `test_query_gateway_result_caching` — Cache and retrieve cached results
  - `test_query_gateway_streaming_results` — Large result streaming
  - `test_query_gateway_error_handling` — DuckDB errors → HTTP errors
  - + 4-5 more

**`parquet_schema.rs`** — Schema definitions + type conversions
- Define standard metadata schema (inode, path, owner_uid, size_bytes, etc.)
- Arrow/DuckDB type mapping (u64 ↔ int64, String ↔ varchar)
- Schema versioning & migration support
- Tests: schema_new, schema_validate, type_conversions (4 tests)

### Block 2: Web UI Backend (8-10 tests)

**`web_api.rs`** — Axum HTTP routes for dashboards
- `GET /api/v1/analytics/top-users?limit=20` — Space consumption by user
- `GET /api/v1/analytics/top-dirs?depth=2&limit=10` — Directory breakdown
- `GET /api/v1/analytics/stale-files?days=180` — Unused files report
- `GET /api/v1/analytics/file-types` — Distribution by extension/MIME
- `GET /api/v1/analytics/reduction-report` — Dedupe/compression savings
- `GET /api/v1/cluster/health` — Real-time cluster status
- `POST /api/v1/query` — Execute custom SQL
- Tests:
  - `test_web_api_routes_registered` — All routes present
  - `test_web_api_top_users_endpoint` — Returns JSON array
  - `test_web_api_top_dirs_endpoint` — Aggregates by depth
  - `test_web_api_stale_files_endpoint` — Filters by mtime
  - `test_web_api_reduction_report_endpoint` — Calculates compression ratio
  - `test_web_api_custom_query_endpoint` — Accepts SQL
  - `test_web_api_error_handling_malformed_params` — 400 on bad input
  - `test_web_api_error_handling_query_timeout` — 504 on timeout
  - + 2-3 more

**`web_auth.rs`** — Authentication & RBAC
- OIDC integration (issuer, client_id, client_secret)
- JWT token validation (exp, aud, sub claims)
- RBAC roles: admin, operator, viewer, tenant_admin
- Bearer token extraction & validation middleware
- Tests:
  - `test_oidc_provider_discovery` — Fetch OIDC metadata
  - `test_jwt_validation_valid_token` — Accept valid JWT
  - `test_jwt_validation_expired_token` — Reject expired
  - `test_rbac_admin_can_query` — admin role allows queries
  - `test_rbac_viewer_cannot_modify` — viewer role read-only
  - + 3-4 more

### Block 3: CLI Tools (6-8 tests)

**`cli.rs`** — Pre-built query shortcuts
- `claudefs-admin top-users [--limit 20]` — Top N by space
- `claudefs-admin top-dirs [--depth 3] [--limit 20]` — Directory tree
- `claudefs-admin find <pattern>` — Filename/path search
- `claudefs-admin stale [--days 180]` — Files not accessed in N days
- `claudefs-admin reduction-report [--path /data]` — Savings by directory
- `claudefs-admin cluster status` — Cluster health snapshot
- Tests:
  - `test_cli_top_users_default_limit` — Uses limit=20
  - `test_cli_top_dirs_depth_extraction` — Parses --depth flag
  - `test_cli_find_pattern_regex` — Supports glob patterns
  - `test_cli_stale_days_default` — Defaults to 90 days
  - `test_cli_reduction_report_aggregation` — Sums savings by path
  - + 3-4 more

### Block 4: Grafana Dashboard Templates (2-3 tests)

**`dashboards.rs`** — Pre-built JSON dashboard definitions
- Load/validate Grafana JSON template
- Dashboard panels for:
  - Cluster health (node count, aggregate IOPS, capacity %)
  - Top consumers (users/groups with most data)
  - Capacity trends (growth over 30 days)
  - Data lifecycle (hot vs cold, S3 tiering status)
  - Reduction analytics (dedupe hit rate, compression ratio)
- Tests: `test_dashboard_load_cluster_health`, `test_dashboard_load_top_consumers`, etc.

### Block 5: Integration Tests (4-6 tests)

**`tests/integration_test.rs`** — End-to-end workflows
- `test_e2e_parquet_to_api` — Write Parquet → Query via API
- `test_e2e_query_gateway_performance` — 100K records query latency
- `test_e2e_web_ui_with_auth` — OIDC → authenticated query
- `test_e2e_cli_top_users_matches_api` — CLI and API return same results
- `test_e2e_caching_improves_performance` — Cached query faster than uncached
- + 1-2 more

## Implementation Strategy

### Step 1: Foundation (queries)
1. Implement `query_gateway.rs` — DuckDB connection pooling, parameterized queries
2. Implement `parquet_schema.rs` — Standard schema + type mappings
3. Wire into `analytics.rs` existing top_users/top_dirs methods

### Step 2: Web API (HTTP routes)
1. Implement `web_api.rs` — Axum router with all analytics endpoints
2. Integrate with query_gateway
3. Tests: endpoint structure, JSON responses

### Step 3: Auth & Security
1. Implement `web_auth.rs` — OIDC, JWT validation, RBAC middleware
2. Add auth to web_api routes
3. Tests: token validation, role-based access

### Step 4: CLI & Dashboards
1. Enhance `cli.rs` with new subcommands
2. Create `dashboards.rs` with pre-built templates
3. Integration tests for E2E workflows

## Dependencies

### External
- `duckdb` 1.0+ (already in Cargo.toml, Phase 2)
- `tokio` 1.40+ (async runtime)
- `axum` 0.7+ (HTTP server, already in Cargo.toml)
- `jsonwebtoken` 9.x (JWT validation, NEW)
- `reqwest` 0.12+ (HTTP client for OIDC discovery, already in Cargo.toml)

### Internal
- `claudefs-meta` (A2) — metadata journal, KvStore interface
- `analytics.rs` (Phase 2) — top_users, top_dirs queries
- `indexer.rs` (Phase 2) — Parquet files location

## Deliverables

1. ✅ Query gateway (DuckDB parameterized queries, caching, timeouts)
2. ✅ Web API (Axum routes for 5+ analytics endpoints)
3. ✅ OIDC authentication + RBAC enforcement
4. ✅ CLI tools (5+ shortcuts)
5. ✅ Grafana dashboard templates (3-5 dashboards)
6. ✅ Integration tests (E2E workflows)
7. ✅ All 1100+ tests passing (Phase 3 target: +30 new tests)

## Validation

```bash
cargo test -p claudefs-mgmt --lib         # Must pass all 1000+ tests
cargo build -p claudefs-mgmt --release   # No warnings
cargo clippy -p claudefs-mgmt --lib      # Clean
```

## Estimated Effort

- Query gateway: 4-6 hours (with OpenCode)
- Web API: 6-8 hours
- Auth: 3-4 hours
- CLI/Dashboards: 2-3 hours
- Integration tests: 2-3 hours
- **Total: 20-30 hours over 1-2 agent sessions**

---

## Phase 4 Preview (Future)

If Phase 3 is complete by 2026-03-10:
- **Alerting Integration** — Forward Prometheus alerts to PagerDuty/Slack/email
- **Performance Benchmarking** — FIO integration, nightly benchmark runs
- **Cluster Health Monitoring** — Auto-restart failed nodes, budget tracking
- **Advanced Tiering** — Policy enforcement, migration workflows
