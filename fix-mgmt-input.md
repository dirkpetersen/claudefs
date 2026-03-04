# Fix 5 Compile Errors in claudefs-mgmt

You are fixing compile errors in the `claudefs-mgmt` crate of the ClaudeFS project.
Do NOT change any logic or structure beyond what's listed below.
Make ONLY these exact targeted fixes.

## Error 1: analytics.rs — Wrong DuckDB ValueRef variants (line ~134)

The DuckDB crate version is 1.4.4. `ValueRef::Integer` does not exist.
The actual variants are: `BigInt(i64)`, `Int(i32)`, `UBigInt(u64)`, `Float(f32)`, `Double(f64)`, `Boolean(bool)`, `Text(&[u8])`, `Blob(&[u8])`.

Replace the entire match block in `crates/claudefs-mgmt/src/analytics.rs` lines 132–139:

**OLD:**
```rust
                    let value: serde_json::Value = match row.get_ref(i) {
                        Ok(duckdb::types::ValueRef::Null) => serde_json::Value::Null,
                        Ok(duckdb::types::ValueRef::Integer(i)) => serde_json::json!(i),
                        Ok(duckdb::types::ValueRef::Float(f)) => serde_json::json!(f),
                        Ok(duckdb::types::ValueRef::Text(s)) => serde_json::json!(s),
                        Ok(duckdb::types::ValueRef::Blob(b)) => serde_json::json!(format!("[{} bytes]", b.len())),
                        _ => serde_json::Value::Null,
                    };
```

**NEW:**
```rust
                    let value: serde_json::Value = match row.get_ref(i) {
                        Ok(duckdb::types::ValueRef::Null) => serde_json::Value::Null,
                        Ok(duckdb::types::ValueRef::Boolean(b)) => serde_json::json!(b),
                        Ok(duckdb::types::ValueRef::TinyInt(n)) => serde_json::json!(n),
                        Ok(duckdb::types::ValueRef::SmallInt(n)) => serde_json::json!(n),
                        Ok(duckdb::types::ValueRef::Int(n)) => serde_json::json!(n),
                        Ok(duckdb::types::ValueRef::BigInt(n)) => serde_json::json!(n),
                        Ok(duckdb::types::ValueRef::HugeInt(n)) => serde_json::json!(n),
                        Ok(duckdb::types::ValueRef::UTinyInt(n)) => serde_json::json!(n),
                        Ok(duckdb::types::ValueRef::USmallInt(n)) => serde_json::json!(n),
                        Ok(duckdb::types::ValueRef::UInt(n)) => serde_json::json!(n),
                        Ok(duckdb::types::ValueRef::UBigInt(n)) => serde_json::json!(n),
                        Ok(duckdb::types::ValueRef::Float(f)) => serde_json::json!(f as f64),
                        Ok(duckdb::types::ValueRef::Double(f)) => serde_json::json!(f),
                        Ok(duckdb::types::ValueRef::Text(s)) => serde_json::json!(std::str::from_utf8(s).unwrap_or("")),
                        Ok(duckdb::types::ValueRef::Blob(b)) => serde_json::json!(format!("[{} bytes]", b.len())),
                        _ => serde_json::Value::Null,
                    };
```

## Error 2: analytics.rs — Type annotation needed (line ~153)

In `crates/claudefs-mgmt/src/analytics.rs`, around line 153, change:

**OLD:**
```rust
        .map_err(|e| AnalyticsError::QueryFailed(e.to_string()).into())
```

**NEW:**
```rust
        .map_err(|e: duckdb::Error| AnalyticsError::QueryFailed(e.to_string()).into())
```

## Error 3: api.rs — reduction_report method doesn't exist (line ~368)

In `crates/claudefs-mgmt/src/api.rs`, the `reduction_report_handler` calls
`state.analytics.reduction_report(limit)` but the method is named `reduction_stats()`
(with no arguments).

Replace in `crates/claudefs-mgmt/src/api.rs`:

**OLD:**
```rust
async fn reduction_report_handler(
    State(state): State<Arc<AdminApi>>,
    Query(params): Query<ReductionReportParams>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(20);
    match state.analytics.reduction_report(limit).await {
        Ok(results) => (StatusCode::OK, Json(results)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}
```

**NEW:**
```rust
async fn reduction_report_handler(
    State(state): State<Arc<AdminApi>>,
    _params: Query<ReductionReportParams>,
) -> impl IntoResponse {
    match state.analytics.reduction_stats().await {
        Ok(results) => (StatusCode::OK, Json(results)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}
```

## Error 4: cli.rs — moved value used after move (line ~337)

In `crates/claudefs-mgmt/src/cli.rs`, change:

**OLD:**
```rust
        let api = AdminApi::new(metrics, config, config.index_dir.clone());
```

**NEW:**
```rust
        let api = AdminApi::new(metrics, config.clone(), config.index_dir.clone());
```

## Instructions

1. Make ONLY the changes listed above.
2. Do NOT add or remove any other code.
3. Output the COMPLETE modified file contents for each file you change:
   - `crates/claudefs-mgmt/src/analytics.rs`
   - `crates/claudefs-mgmt/src/api.rs`
   - `crates/claudefs-mgmt/src/cli.rs`

The project is at `/home/cfs/claudefs`. You can read the current file contents before making changes.
