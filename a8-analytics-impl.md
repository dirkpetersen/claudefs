# A8 DuckDB Analytics Implementation

## Task

Implement the DuckDB query interface for the `AnalyticsEngine` struct in `claudefs-mgmt`.

## File to Modify

`crates/claudefs-mgmt/src/analytics.rs`

## Current State

The `AnalyticsEngine` struct has a `query()` method with a TODO stub. It needs implementation to:
1. Open a DuckDB connection
2. Register Parquet files from the index_dir
3. Execute SQL queries
4. Return results as Vec<HashMap<String, serde_json::Value>>

## Implementation Requirements

Implement these 6 methods:

### 1. `pub fn query(&self, sql: &str) -> anyhow::Result<Vec<HashMap<String, serde_json::Value>>>`

- Open a DuckDB :memory: connection (or use self.db_path if available)
- Register Parquet files: CREATE TABLE metadata AS SELECT * FROM read_parquet('{index_dir}/metadata*.parquet')
- Execute the user's SQL query using conn.prepare() and execute()
- Collect rows into Vec<HashMap<metric_name, value>>
- Return AnalyticsError::QueryFailed on error

### 2. `pub async fn top_users(&self, limit: usize) -> anyhow::Result<Vec<UserStorageUsage>>`

Build and execute SQL:
```sql
SELECT owner_name, SUM(size_bytes) as total_size_bytes, COUNT(*) as file_count
FROM metadata
GROUP BY owner_name
ORDER BY total_size_bytes DESC
LIMIT {limit}
```

Convert results to Vec<UserStorageUsage> structs.

### 3. `pub async fn top_dirs(&self, depth: usize, limit: usize) -> anyhow::Result<Vec<DirStorageUsage>>`

Extract directory path at specified depth using SUBSTR or path manipulation.
Group by extracted prefix, order by size DESC, limit.
Return Vec<DirStorageUsage>.

### 4. `pub async fn reduction_stats(&self) -> anyhow::Result<Vec<ReductionStats>>`

Query path, total_logical_bytes, total_stored_bytes from metadata.
Calculate reduction_ratio = logical / stored.
Return Vec<ReductionStats>.

### 5. `pub async fn find_files(&self, pattern: &str) -> anyhow::Result<Vec<MetadataRecord>>`

Use SQL LIKE or regex on path column to find matching files.
Return matching MetadataRecord structs.

### 6. `pub async fn stale_files(&self, days: u64) -> anyhow::Result<Vec<MetadataRecord>>`

Query WHERE (unix_timestamp() - mtime) > days * 86400.
Return old MetadataRecord structs.

## Implementation Notes

- Use `tokio::task::spawn_blocking()` to wrap sync DuckDB calls since this is an async context
- Handle file discovery: glob or directory scan for metadata*.parquet files
- All methods should handle empty result sets gracefully (return empty Vec, not error)
- Use standard Result type for errors
- Keep async/await consistent with rest of codebase

## Do NOT Modify

- Do not change struct definitions
- Do not change method signatures
- Do not modify Cargo.toml
- Do not add unsafe code

## Output

Output only the complete updated `analytics.rs` file content. No markdown. No explanation. Just Rust code.
