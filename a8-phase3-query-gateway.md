# Query Gateway Module - A8 Phase 3

Create `crates/claudefs-mgmt/src/query_gateway.rs` with the following:

## Purpose
DuckDB connection pooling, query caching, parameterized query execution with timeouts.

## Implementation Requirements

1. **QueryError enum** with variants:
   - PoolExhausted
   - Timeout
   - InvalidQuery(String)
   - NoResults
   - DbError(String)
   Use #[derive(thiserror::Error)]

2. **QueryConnectionPool struct**
   - available: Arc<Mutex<Vec<Connection>>>
   - in_use: Arc<AtomicUsize>
   - max_connections: usize

3. **QueryResultCache struct**
   - cache: Arc<DashMap<String, CachedResult>>
   - ttl_secs: u32

4. **CachedResult struct**
   - rows: Vec<serde_json::Value>
   - cached_at: Instant

5. **QueryGateway struct**
   - connections: Arc<QueryConnectionPool>
   - cache: Arc<QueryResultCache>
   - query_timeout_secs: u32
   - max_result_rows: usize

6. **PoolStats struct**
   - available: usize
   - in_use: usize
   - max: usize

7. **CacheStats struct**
   - hits: u64
   - misses: u64
   - entries: usize

## Methods to Implement

QueryGateway methods:
- `new(path: &str, max_connections: usize, query_timeout_secs: u32) -> Result<Self, QueryError>`
- `async fn execute_query(&self, query: &str, use_cache: bool) -> Result<Vec<serde_json::Value>, QueryError>`
- `async fn execute_parameterized(&self, query: &str, params: &[String], use_cache: bool) -> Result<Vec<serde_json::Value>, QueryError>`
- `fn get_pool_stats() -> PoolStats`
- `fn clear_cache(&self)`
- `fn get_cache_stats() -> CacheStats`

## Tests (10-12 tests)

1. test_query_gateway_new_succeeds
2. test_query_gateway_pool_stats
3. test_query_gateway_clear_cache
4. test_query_error_display
5. test_query_gateway_execute_query_no_results
6. test_pool_stats_creation
7. test_cache_stats_creation
8. test_cached_result_structure
9. test_query_gateway_connection_pool_limit
10. test_query_gateway_get_cache_stats
11. test_query_gateway_parameterized_safe
12. test_query_gateway_cache_ttl

## Key Implementation Details

- Use tokio::task::block_in_place for sync DuckDB operations
- Connection pooling uses Arc + Mutex for thread safety
- Cache key = "query:" + query string
- Cache hit check: if (use_cache && in_cache && not_expired) return cached
- Test with tempfile::NamedTempFile for temporary DuckDB databases
- Module doc comment: "/// Query gateway with DuckDB connection pooling and caching"

Output ONLY the Rust code for query_gateway.rs. Include:
- Module-level doc comment
- All structs with doc comments
- All methods with doc comments
- All tests with #[tokio::test] or #[test] as appropriate
- Use existing patterns from analytics.rs in the crate

Keep total line count < 400 lines. Be efficient with code but complete.
