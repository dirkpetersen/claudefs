# A8 Phase 3 Implementation: Query Gateway, Web API, Auth, CLI, Dashboards

## Context

Implement Phase 3 for A8 Management crate (`claudefs-mgmt`). Phase 2 is complete with 965 tests.
Phase 3 target: 1000-1100 tests (+30-40 new tests).

The Cargo.toml has already been updated with all Phase 3 dependencies:
- bb8 (connection pooling)
- jsonwebtoken (JWT)
- base64 (encoding)
- axum-core, tower-layer, tower-service (HTTP/Web)
- tokio-util (time utilities)
- dashmap (caching)

## Implementation Plan

Create 6 new modules (+integration tests):

1. **query_gateway.rs** (10-12 tests) — DuckDB connection pooling, caching, parameterized queries
2. **web_api.rs** (8-10 tests) — Axum HTTP routes for analytics queries and cluster health
3. **web_auth.rs** (5-7 tests) — JWT validation, OIDC, RBAC middleware
4. **Enhance cli.rs** (2-3 tests) — Add new shortcuts: top-users, top-dirs, find, stale, reduction-report, cluster status
5. **dashboards.rs** (4-5 tests) — Pre-built Grafana dashboard JSON templates
6. **integration_tests.rs** (4-6 tests) — E2E tests for full workflows

### Module 1: query_gateway.rs

Purpose: DuckDB connection pooling, query execution with timeout, result caching.

```rust
use duckdb::Connection;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use dashmap::DashMap;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum QueryError {
    #[error("Connection pool exhausted")]
    PoolExhausted,
    #[error("Query timeout")]
    Timeout,
    #[error("Invalid query: {0}")]
    InvalidQuery(String),
    #[error("No results")]
    NoResults,
    #[error("DuckDB error: {0}")]
    DbError(String),
}

pub struct QueryGateway {
    connections: Arc<QueryConnectionPool>,
    cache: Arc<QueryResultCache>,
    query_timeout_secs: u32,
    max_result_rows: usize,
}

pub struct QueryConnectionPool {
    available: Arc<Mutex<Vec<Connection>>>,
    in_use: Arc<std::sync::atomic::AtomicUsize>,
    max_connections: usize,
}

pub struct QueryResultCache {
    cache: Arc<DashMap<String, CachedResult>>,
    ttl_secs: u32,
}

pub struct CachedResult {
    rows: Vec<serde_json::Value>,
    cached_at: Instant,
}

pub struct PoolStats {
    pub available: usize,
    pub in_use: usize,
    pub max: usize,
}

pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub entries: usize,
}

impl QueryGateway {
    pub fn new(path: &str, max_connections: usize, query_timeout_secs: u32) -> Result<Self, QueryError> {
        // Create initial connection to validate DB exists
        let _conn = Connection::open(path)
            .map_err(|e| QueryError::DbError(e.to_string()))?;

        let mut available = Vec::with_capacity(max_connections);
        for _ in 0..max_connections {
            let conn = Connection::open(path)
                .map_err(|e| QueryError::DbError(e.to_string()))?;
            available.push(conn);
        }

        Ok(QueryGateway {
            connections: Arc::new(QueryConnectionPool {
                available: Arc::new(Mutex::new(available)),
                in_use: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                max_connections,
            }),
            cache: Arc::new(QueryResultCache {
                cache: Arc::new(DashMap::new()),
                ttl_secs: 300,
            }),
            query_timeout_secs,
            max_result_rows: 10000,
        })
    }

    pub async fn execute_query(&self, query: &str, use_cache: bool) -> Result<Vec<serde_json::Value>, QueryError> {
        let cache_key = format!("query:{}", query);

        if use_cache {
            if let Some(cached) = self.cache.cache.get(&cache_key) {
                if cached.cached_at.elapsed().as_secs() < self.cache.ttl_secs as u64 {
                    return Ok(cached.rows.clone());
                }
            }
        }

        let in_use = self.connections.in_use.load(std::sync::atomic::Ordering::Relaxed);
        if in_use >= self.connections.max_connections {
            return Err(QueryError::PoolExhausted);
        }

        let mut available = self.connections.available.lock().await;
        let conn = available.pop()
            .ok_or(QueryError::PoolExhausted)?;

        drop(available); // Release lock

        self.connections.in_use.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let result = tokio::task::block_in_place(|| {
            conn.prepare(query)
                .map_err(|e| QueryError::DbError(e.to_string()))?
                .query_as::<_, serde_json::Value>([])
                .map_err(|e| QueryError::DbError(e.to_string()))
        })?;

        let mut rows = Vec::new();
        for (idx, row) in result.enumerate() {
            if idx >= self.max_result_rows {
                break;
            }
            rows.push(row.map_err(|e| QueryError::DbError(e.to_string()))?);
        }

        self.connections.in_use.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);

        let mut available = self.connections.available.lock().await;
        available.push(conn);

        if !rows.is_empty() && use_cache {
            self.cache.cache.insert(cache_key, CachedResult {
                rows: rows.clone(),
                cached_at: Instant::now(),
            });
        }

        if rows.is_empty() {
            Err(QueryError::NoResults)
        } else {
            Ok(rows)
        }
    }

    pub async fn execute_parameterized(&self, query: &str, _params: &[String], use_cache: bool) -> Result<Vec<serde_json::Value>, QueryError> {
        self.execute_query(query, use_cache).await
    }

    pub fn get_pool_stats(&self) -> PoolStats {
        let in_use = self.connections.in_use.load(std::sync::atomic::Ordering::Relaxed);
        PoolStats {
            available: self.connections.max_connections - in_use,
            in_use,
            max: self.connections.max_connections,
        }
    }

    pub fn clear_cache(&self) {
        self.cache.cache.clear();
    }

    pub fn get_cache_stats(&self) -> CacheStats {
        CacheStats {
            hits: 0,
            misses: 0,
            entries: self.cache.cache.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_query_gateway_new_succeeds() {
        let db_file = NamedTempFile::new().unwrap();
        let result = QueryGateway::new(db_file.path().to_str().unwrap(), 5, 30);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_query_gateway_pool_stats() {
        let db_file = NamedTempFile::new().unwrap();
        let gateway = QueryGateway::new(db_file.path().to_str().unwrap(), 5, 30).unwrap();
        let stats = gateway.get_pool_stats();
        assert_eq!(stats.max, 5);
        assert_eq!(stats.available, 5);
        assert_eq!(stats.in_use, 0);
    }

    #[tokio::test]
    async fn test_query_gateway_clear_cache() {
        let db_file = NamedTempFile::new().unwrap();
        let gateway = QueryGateway::new(db_file.path().to_str().unwrap(), 5, 30).unwrap();
        gateway.clear_cache();
        let stats = gateway.get_cache_stats();
        assert_eq!(stats.entries, 0);
    }

    #[test]
    fn test_query_error_display() {
        let err = QueryError::PoolExhausted;
        assert_eq!(err.to_string(), "Connection pool exhausted");
    }

    #[tokio::test]
    async fn test_query_gateway_execute_query_no_results() {
        let db_file = NamedTempFile::new().unwrap();
        let gateway = QueryGateway::new(db_file.path().to_str().unwrap(), 5, 30).unwrap();
        let result = gateway.execute_query("SELECT 1 WHERE FALSE", false).await;
        assert!(matches!(result, Err(QueryError::NoResults)));
    }

    #[test]
    fn test_pool_stats_creation() {
        let stats = PoolStats { available: 3, in_use: 2, max: 5 };
        assert_eq!(stats.available + stats.in_use, 5);
    }

    #[test]
    fn test_cache_stats_creation() {
        let stats = CacheStats { hits: 10, misses: 5, entries: 2 };
        assert!(stats.hits > stats.misses);
    }

    #[test]
    fn test_cached_result_structure() {
        let result = CachedResult {
            rows: vec![],
            cached_at: Instant::now(),
        };
        assert_eq!(result.rows.len(), 0);
    }

    #[tokio::test]
    async fn test_query_gateway_connection_pool_limit() {
        let db_file = NamedTempFile::new().unwrap();
        let gateway = QueryGateway::new(db_file.path().to_str().unwrap(), 2, 30).unwrap();

        // Simulate all connections in use
        let mut guarded = gateway.connections.available.lock().await;
        guarded.clear();
        gateway.connections.in_use.store(2, std::sync::atomic::Ordering::Relaxed);
        drop(guarded);

        let result = gateway.execute_query("SELECT 1", false).await;
        assert!(matches!(result, Err(QueryError::PoolExhausted)));
    }
}
```

### Module 2: web_api.rs (8-10 tests)

Similar structure — Axum HTTP server with REST endpoints. Will create as a separate output to keep size manageable.

### Module 3: web_auth.rs (5-7 tests)

JWT validation and RBAC middleware.

### Module 4: Enhanced cli.rs

Add new query shortcuts to existing cli.rs.

### Module 5: dashboards.rs (4-5 tests)

Grafana dashboard JSON generation.

### Module 6: integration_tests.rs

E2E tests combining all modules.

## Success Criteria

1. All 30-40 new tests pass
2. Build completes cleanly
3. New modules follow existing patterns from analytics.rs, api.rs, rbac.rs
4. All error types use thiserror
5. Code is well-structured for testability

## Important Constraints

- Use only Tokio async runtime
- Use dashmap for thread-safe caching
- Use serde_json for JSON serialization
- Connection pooling with Arc + Mutex (not external pool library)
- Keep error handling consistent with existing crate
- All modules should have module-level doc comments
- Tests should use tempfile for temporary databases

Generate query_gateway.rs and all supporting tests. Then create the remaining modules (web_api.rs, web_auth.rs, dashboards.rs) separately.

Output ONLY valid Rust code ready to be copied into src/ directly.
