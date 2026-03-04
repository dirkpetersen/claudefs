# A8: DuckDB Analytics Engine Implementation

## Objective
Implement DuckDB integration for claudefs-mgmt analytics module to query Parquet files over the ClaudeFS metadata index.

## Context
ClaudeFS management service (crate: `claudefs-mgmt`) provides operational dashboards and CLI for cluster administration. The analytics engine queries metadata stored in Parquet files (written by claudefs-index daemon). Currently, all analytics methods return empty results with TODO comments.

## Requirements

### 1. Add DuckDB Dependency
Update `crates/claudefs-mgmt/Cargo.toml`:
- Add: `duckdb = "1.0"` with features for Parquet support
- Ensure compatibility with existing workspace deps (tokio, serde, thiserror, etc.)

### 2. Implement AnalyticsEngine DuckDB Integration

**File:** `crates/claudefs-mgmt/src/analytics.rs`

Modify `AnalyticsEngine` struct to:
- Add a private `duckdb_connection: duckdb::Connection` field
- OR use lazy initialization: create Connection on first query
- Store connection with thread-safe wrapper if needed (std::sync::Arc)

Implement these methods with real DuckDB queries:

#### 2.1 `query(&self, sql: &str) -> anyhow::Result<Vec<HashMap<String, serde_json::Value>>>`
- Accept raw SQL from users
- Replace `"metadata"` table placeholder with actual Parquet glob: `read_parquet('{index_dir}/**/*.parquet')`
- Execute query via DuckDB
- Convert result rows to HashMap<String, serde_json::Value>
- Return in JSON-friendly format for API responses
- Error handling: wrap duckdb::Error as anyhow::Error

#### 2.2 `top_users(&self, limit: usize) -> anyhow::Result<Vec<UserStorageUsage>>`
Query:
```sql
SELECT
  owner_name,
  SUM(size_bytes) as total_size_bytes,
  COUNT(*) as file_count
FROM read_parquet('{index_dir}/**/*.parquet')
GROUP BY owner_name
ORDER BY total_size_bytes DESC
LIMIT {limit}
```
Parse results into UserStorageUsage structs.

#### 2.3 `top_dirs(&self, depth: usize, limit: usize) -> anyhow::Result<Vec<DirStorageUsage>>`
Query:
```sql
SELECT
  parent_path as path,
  SUM(size_bytes) as total_size_bytes,
  COUNT(*) as file_count
FROM read_parquet('{index_dir}/**/*.parquet')
WHERE list_length(string_split(path, '/')) <= {depth + 1}
GROUP BY parent_path
ORDER BY total_size_bytes DESC
LIMIT {limit}
```
Parse results into DirStorageUsage structs.

#### 2.4 `find_files(&self, pattern: &str, limit: usize) -> anyhow::Result<Vec<MetadataRecord>>`
Use existing `pattern_to_sql_glob()` to convert shell patterns to SQL LIKE patterns.
Query:
```sql
SELECT * FROM read_parquet('{index_dir}/**/*.parquet')
WHERE filename LIKE '{glob_pattern}'
LIMIT {limit}
```
Parse results into full MetadataRecord structs.

#### 2.5 `stale_files(&self, days: u64, limit: usize) -> anyhow::Result<Vec<MetadataRecord>>`
Query:
```sql
SELECT * FROM read_parquet('{index_dir}/**/*.parquet')
WHERE mtime < {cutoff_timestamp}
ORDER BY mtime ASC
LIMIT {limit}
```
Parse results into MetadataRecord structs.

#### 2.6 `reduction_report(&self, limit: usize) -> anyhow::Result<Vec<ReductionStats>>`
Query:
```sql
SELECT
  parent_path as path,
  SUM(size_bytes) as total_logical_bytes,
  SUM(blocks_stored * 4096) as total_stored_bytes,
  CASE WHEN SUM(blocks_stored * 4096) = 0 THEN 0
       ELSE SUM(size_bytes) / CAST(SUM(blocks_stored * 4096) AS DOUBLE)
  END as reduction_ratio
FROM read_parquet('{index_dir}/**/*.parquet')
GROUP BY parent_path
ORDER BY total_logical_bytes DESC
LIMIT {limit}
```
Parse results into ReductionStats structs.

### 3. Error Handling
- Create `AnalyticsError` enum if not already present (should be in error.rs or define new)
- Variants needed:
  - `DuckDbError(String)` — wrap duckdb errors
  - `ParquetNotFound` — when no Parquet files exist in index_dir
  - `QueryFailed(String)` — SQL execution failed
  - `ParseError(String)` — failed to parse result row into struct
- Implement `From<duckdb::Error>` for AnalyticsError
- Return anyhow::Result<T> by converting AnalyticsError to anyhow::Error

### 4. Connection Management
- DuckDB Connection should be created lazily on first query (Option<Connection> pattern)
- OR use Arc<Mutex<Connection>> if multiple threads access simultaneously
- Consider read-only connection mode: `.execute("PRAGMA query_only = true")`

### 5. NULL Handling
- DuckDB may return NULLs in SUM() results if no rows match
- Convert NULLs to 0 for count/size fields
- Use `COALESCE(SUM(size_bytes), 0)` in queries

### 6. Testing
Maintain existing test structure (no changes to test signatures):
- `test_top_users_empty_index()` — should still return empty Vec, not error
- `test_pattern_to_sql_glob_*()` — unchanged, already working
- Add new tests only if needed (optional, keep test count stable)
- Tests should work even if /tmp/test has no Parquet files (graceful degradation)

## Implementation Notes

1. **Lazy Connection Pattern (Recommended):**
```rust
impl AnalyticsEngine {
    fn get_connection(&self) -> Result<&duckdb::Connection> {
        // Return reference to connection, create if needed
    }
}
```

2. **Result Parsing:**
```rust
// For each row, extract columns and construct struct
let user_name: String = row.get(0)?;
let total_bytes: i64 = row.get(1)?;
// etc.
```

3. **SQL Injection Protection:**
- Use parameterized queries where possible (bind parameters)
- `limit` parameter: validate as usize (safe)
- `depth` parameter: validate as usize (safe)
- `pattern`: already validated by pattern_to_sql_glob()
- `index_dir`: use PathBuf safely with .display()

4. **Parquet Glob Handling:**
- DuckDB supports glob patterns in read_parquet()
- `read_parquet('/path/**/*.parquet')` will find all Parquet files recursively
- Handle case where no files exist: return empty Vec gracefully

## Constraints
- Must maintain backward compatibility with existing API (method signatures unchanged)
- Keep test pass rate at 814+ (current)
- Must not break CLI (cli.rs depends on analytics.rs)
- No changes to Cargo.toml beyond adding duckdb crate
- No changes to existing test code (only implementation changes)

## Success Criteria
1. All 6 analytics methods implemented and return real results (not empty Vecs)
2. DuckDB queries execute successfully when Parquet files exist
3. Gracefully handle missing/empty Parquet index (return empty results, not errors)
4. All 814+ existing tests pass
5. No clippy warnings introduced
6. Error handling: user-facing errors are descriptive (not generic "DuckDB failed")
