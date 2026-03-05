Update crates/claudefs-mgmt/src/analytics.rs:

In the query() method, replace:
```
Ok(Vec::new())
```

With a working DuckDB implementation that queries Parquet files using the duckdb crate and returns real results.

Similarly for: top_users(), top_dirs(), find_files(), stale_files(), reduction_report() methods - implement with real DuckDB queries against the index_dir Parquet files.

Also add to Cargo.toml dependencies: duckdb = "1.0" with bundled feature.

Use the exact SQL queries provided in the comments within each method.
