# Task: Add 3 new modules to claudefs-storage lib.rs

## Working directory
/home/cfs/claudefs

## What to do
Read `crates/claudefs-storage/src/lib.rs`, then add 3 new modules.

## Modules to add (in order, after line 44 `pub mod tracing_storage;`):
```rust
pub mod block_verifier;
pub mod compaction_manager;
pub mod io_accounting;
```

## Re-exports to add (at the end of the file, after the last pub use line):
```rust
pub use block_verifier::{
    BlockToVerify, BlockVerifier, VerificationResult, VerifierAlgorithm, VerifierConfig,
    VerifierStats,
};
pub use compaction_manager::{
    CompactionError, CompactionJob, CompactionJobId, CompactionJobState, CompactionManager,
    CompactionManagerConfig, CompactionManagerStats,
};
pub use io_accounting::{
    IoAccounting, IoAccountingConfig, IoDirection, TenantId, TenantIoStats,
};
```

## Then run:
```bash
cd /home/cfs/claudefs && cargo test -p claudefs-storage 2>&1 | grep "^test result"
```

Show the results. All tests must pass.
