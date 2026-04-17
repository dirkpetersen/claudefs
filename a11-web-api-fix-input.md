# A11: Fix claudefs-mgmt web_api.rs Compilation Errors

## Problem
The web_api.rs file has compilation errors in the route handlers. The issue is that:
1. Some handlers don't take `State<AppState>` as a parameter but are used in routes that expect it
2. Return types are not properly inferred by the compiler

## Specific Errors to Fix

From `crates/claudefs-mgmt/src/web_api.rs`:

1. **file_types_handler** (line 309-335)
   - Currently: `async fn file_types_handler(State(state): State<AppState>)`
   - Issue: Function signature needs to match Axum handler requirements
   - Fix: Ensure return type is correct and extract is properly ordered

2. **reduction_handler** (line 337-378)
   - Currently: `async fn reduction_handler(State(state): State<AppState>, Query(params): Query<ReductionReportParams>)`
   - Issue: Parameter order or extraction issue with Axum 0.7
   - Fix: Ensure proper extraction order (Query should come after State if State is used)

3. **top_users_handler** (line 205-237)
   - Issue: Possible return type inference problem
   - Fix: Verify return type is correct Result<Json<Vec<TopUser>>, ApiError>

4. **top_dirs_handler** (line 239-271)
   - Issue: Similar to above
   - Fix: Verify return type

5. **stale_files_handler** (line 273-307)
   - Issue: Parameter extraction problem
   - Fix: Verify extraction order and return type

## Solution

The issue is that in Axum 0.7, extractors must be in the correct order. Generally:
- State should be first (or alone if it's the only extractor)
- Query/Path/Json come after State
- The compiler needs explicit type hints sometimes

For handlers that only take State and return a Result<Json<T>, ApiError>, ensure:
1. The function signature is: `async fn handler(State(state): State<AppState>) -> Result<Json<T>, ApiError>`
2. For handlers with Query params: `async fn handler(State(state): State<AppState>, Query(params): Query<Params>) -> Result<Json<T>, ApiError>`

## Fix Strategy

Review each handler function and ensure:
1. All handlers that need State must explicitly extract it
2. Return types are properly annotated
3. Extraction order is correct (State first, then other extractors)
4. Handler functions that only read from cache don't need State (but current code has State, so keep it)

## Current Handlers to Fix

- health_handler: No state needed (remove State parameter) or adjust signature
- metrics_handler: Needs State (already correct)
- query_handler: Needs State (already correct)
- custom_query_handler: Needs State (already correct)
- top_users_handler: Needs State and Query
- top_dirs_handler: Needs State and Query
- stale_files_handler: Needs State and Query
- file_types_handler: Needs State only
- reduction_handler: Needs State and Query

For **health_handler**, it should NOT take State since it doesn't use it:
```rust
async fn health_handler() -> Json<serde_json::Value> {
    ...
}
```

For **file_types_handler**, it needs State:
```rust
async fn file_types_handler(
    State(state): State<AppState>,
) -> Result<Json<Vec<FileTypeStats>>, ApiError> {
    ...
}
```

This should match what's already in the code, so the issue might be with Axum version compatibility or trait bounds. Please review the entire file and fix any trait bound issues with Handler trait implementation.

## References
- File: crates/claudefs-mgmt/src/web_api.rs
- Errors: Multiple E0277 (Handler trait not implemented) and E0308 (type mismatch)
- Root cause: Axum 0.7 handler trait bounds may not be satisfied due to async fn signature issues
