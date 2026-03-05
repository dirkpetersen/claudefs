Implement DuckDB integration for claudefs-mgmt analytics module.

File: crates/claudefs-mgmt/src/analytics.rs

Add these imports at the top:
```rust
use duckdb::Connection;
use std::sync::Mutex;
```

Add field to AnalyticsEngine struct:
```rust
connection: Mutex<Option<Connection>>,
```

Implement these methods to execute real DuckDB queries against Parquet files in self.index_dir:

1. `query()` - execute raw SQL, return Vec<HashMap>
2. `top_users(limit)` - GROUP BY owner_name, ORDER BY total_size_bytes DESC
3. `top_dirs(depth, limit)` - GROUP BY parent_path at specified depth
4. `find_files(pattern, limit)` - WHERE filename LIKE pattern
5. `stale_files(days, limit)` - WHERE mtime < cutoff
6. `reduction_report(limit)` - calculate dedupe/compression ratios per directory

Use read_parquet() glob pattern: `{index_dir}/**/*.parquet`

All methods should handle missing/empty Parquet files gracefully (return empty Vec).
