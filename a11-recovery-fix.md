# A11 Phase 4 Block 3: Fix Recovery Module Compilation Errors

## Context

The claudefs-mgmt crate has three recovery-related modules that need to be fixed to compile:
1. `recovery_actions.rs` — Recovery action execution framework
2. `backup_rotation.rs` — Backup rotation and retention management
3. `graceful_shutdown.rs` — Graceful shutdown sequencing

## Current Compilation Errors

### Error 1: Missing functions in recovery_actions
health.rs imports undefined functions:
- `cpu_to_action`
- `disk_to_action`
- `memory_to_action`
- `should_remove_node`
- `RecoveryConfig`

**Location:** crates/claudefs-mgmt/src/health.rs:8

**Solution:** These functions must be implemented in recovery_actions.rs. They convert health metrics into recovery actions.

### Error 2: RecoveryExecutor::new() signature mismatch
health.rs calls `RecoveryExecutor::new(config)` but recovery_actions.rs has `RecoveryExecutor::new()` with no arguments.

**Location:** crates/claudefs-mgmt/src/health.rs:221

**Solution:** Update RecoveryExecutor::new() to accept an optional RecoveryConfig parameter.

### Error 3: execute() method signature mismatch
health.rs calls `executor.execute(action).await` but recovery_actions.rs requires two arguments: `execute(action, context).await`

**Location:** crates/claudefs-mgmt/src/health.rs:232, 240, 248, 257

**Solution:** Either update health.rs calls or simplify the execute signature to only need the action.

### Error 4: Result<T> type annotation needed in backup_rotation
The check_and_rotate() function doesn't specify the full Result<Vec<BackupAction>, Error> type.

**Location:** crates/claudefs-mgmt/src/backup_rotation.rs:62

**Solution:** Define a proper error type and use Result<Vec<BackupAction>, BackupError>

### Error 5: Type mismatches in backup_rotation date calculation
Date parsing returns u64 but function expects u32.

**Location:** crates/claudefs-mgmt/src/backup_rotation.rs:230

**Solution:** Fix type conversions in date calculation.

### Error 6: ShutdownAudit not implementing Deserialize
ShutdownAudit doesn't derive Deserialize, causing serde_json::from_str to fail.

**Location:** crates/claudefs-mgmt/src/graceful_shutdown.rs:254

**Solution:** Add #[derive(Deserialize)] to ShutdownAudit struct.

## Required Implementations

### 1. recovery_actions.rs

**Add RecoveryConfig struct:**
```
pub struct RecoveryConfig {
    pub cpu_threshold_high: f64,
    pub cpu_threshold_critical: f64,
    pub memory_threshold_high: f64,
    pub memory_threshold_critical: f64,
    pub disk_threshold_warning: f64,
    pub disk_threshold_critical: f64,
    pub heartbeat_timeout_secs: u64,
    pub max_missed_heartbeats: u32,
}
```

**Add conversion functions:**
- `pub fn cpu_to_action(usage: f64, config: &RecoveryConfig) -> Option<RecoveryAction>` — Convert CPU % to action
- `pub fn memory_to_action(usage: f64, config: &RecoveryConfig) -> Option<RecoveryAction>` — Convert memory % to action
- `pub fn disk_to_action(free_pct: f64, config: &RecoveryConfig) -> Option<RecoveryAction>` — Convert disk % to action
- `pub fn should_remove_node(missed_heartbeats: u32, config: &RecoveryConfig) -> bool` — Determine if node should be removed

**Update RecoveryExecutor:**
- Add optional config field
- Update new() to accept Option<RecoveryConfig>
- Simplify execute() to only require action argument (remove context param)

### 2. backup_rotation.rs

**Add BackupError enum:**
```
#[derive(Debug, Error)]
pub enum BackupError {
    #[error("Backup failed: {0}")]
    BackupFailed(String),
    #[error("IO error: {0}")]
    IoError(String),
    #[error("Invalid configuration")]
    InvalidConfig,
}
```

**Fix check_and_rotate() signature:**
- Change return type to `Result<Vec<BackupAction>, BackupError>`

**Fix date type handling:**
- Convert u64 date components to u32 where needed

### 3. graceful_shutdown.rs

**Fix ShutdownAudit:**
- Add `#[derive(Deserialize)]` to ShutdownAudit

## Expected Output

After these fixes:
1. `cargo build --lib claudefs-mgmt` should succeed with only warnings
2. `cargo test --lib claudefs-mgmt` should compile and run tests
3. All recovery, backup, and graceful shutdown functionality should be testable
4. Integration with health.rs should work correctly

## Files to Modify

- `crates/claudefs-mgmt/src/recovery_actions.rs` — Add config, conversion functions, update RecoveryExecutor
- `crates/claudefs-mgmt/src/backup_rotation.rs` — Add BackupError, fix type mismatches
- `crates/claudefs-mgmt/src/graceful_shutdown.rs` — Add Deserialize derive
- `crates/claudefs-mgmt/src/health.rs` — Should work once recovery_actions.rs is fixed (no changes needed)

## Testing

After fixes:
- All tests should pass: `cargo test --lib recovery_actions`
- All tests should pass: `cargo test --lib backup_rotation`
- All tests should pass: `cargo test --lib graceful_shutdown`
- health.rs integration should work with new recovery module

## Implementation Approach

For each file:
1. Review current implementation
2. Add missing types and functions
3. Fix type mismatches
4. Ensure all imports resolve
5. Keep existing test cases intact
6. Preserve all functionality from original plan

Use the existing test code as reference for expected behavior.
