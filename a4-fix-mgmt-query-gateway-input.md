# Fix claudefs-mgmt query_gateway.rs Compilation Errors

## Problem
The `crates/claudefs-mgmt/src/query_gateway.rs` file has several compilation errors:

1. Line 174: Reference to undefined `ApiError` type
2. Line 143: Needs type annotation for `row.get(i)` call (should be `row.get_ref(i)`)
3. Error handling with `tokio::time::timeout` returning `Result<Result<...>>` needs unwrapping

## Current Code Issues
The execute_query method attempts to:
- Call `tokio::time::timeout()` which returns `Result<Result<...>>`
- Return error with undefined `ApiError` type
- Use `row.get()` which requires type inference

## Required Fix
1. Fix the timeout error handling chain - unwrap both Results correctly
2. Fix `ApiError` reference - should convert from `QueryError` only
3. Ensure the error chain converts properly: `DuckDB error → QueryError → final Result`

## Code Location
- File: `crates/claudefs-mgmt/src/query_gateway.rs`
- Lines 92-183 (execute_query method)
- Problem area: Lines 113-174 (timeout and error handling)

## DuckDB API Reference
Current API being used:
- `row.get_ref(i)` returns `Result<ValueRef, ...>` (NOT `row.get(i)`)
- `duckdb::types::ValueRef` is the correct enum to match on (lines 144-158)
- This is already correct in the current code

## Fix Strategy
1. Remove the `ApiError::from()` conversion on line 174
2. Chain the Result unwrapping properly:
   ```
   let query_result = tokio::time::timeout(timeout, async_work)
       .await
       .map_err(|_| QueryError::Timeout)?  // timeout error
       .map_err(|e| QueryError::...)?;      // inner error
   ```
3. The inner closure already returns `Result<QueryResult, QueryError>` so the double unwrap should work

## Expected Outcome
- `cargo check -p claudefs-mgmt` passes with no errors
- All 5 compile errors resolved
- Warnings about unused imports/variables remain (out of scope for this fix)
