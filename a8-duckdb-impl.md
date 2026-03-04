# DuckDB Analytics Implementation for ClaudeFS A8

## Task
Implement DuckDB integration in `crates/claudefs-mgmt/src/analytics.rs` to replace TODO stubs with working queries.

## File: crates/claudefs-mgmt/src/analytics.rs

Replace the `AnalyticsEngine` struct and methods with DuckDB-backed implementation.

Key changes:
1. Add `connection: Option<duckdb::Connection>` field to store DuckDB connection
2. Implement `query()` method to execute raw SQL against Parquet files
3. Implement `top_users()` to return aggregated storage per user
4. Implement `top_dirs()` to return top directories by size
5. Implement `find_files()` to search by filename pattern
6. Implement `stale_files()` to find old files
7. Implement `reduction_report()` to show dedupe/compression stats

All methods should:
- Use `read_parquet('{self.index_dir}/**/*.parquet')` in queries
- Convert DuckDB result rows to appropriate Rust structs
- Handle empty/missing Parquet files gracefully (return empty Vec)
- Wrap duckdb::Error in anyhow::Error

Use DuckDB's Rust API: `Connection::open_in_memory()` or similar, then `prepare()` and `query_map()` to execute SQL.

Return Vec of typed structs (UserStorageUsage, DirStorageUsage, MetadataRecord, etc.) based on method.

Keep all existing tests unchanged - they should still pass.
