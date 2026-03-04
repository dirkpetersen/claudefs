# A4 Phase 7c: Add wire_diag and credit_window to lib.rs

The `claudefs-transport` crate has two new source files that were generated but NOT yet added to `lib.rs`:

1. `crates/claudefs-transport/src/wire_diag.rs` — Wire diagnostics module
2. `crates/claudefs-transport/src/credit_window.rs` — Credit-window flow control module

Your task: Update `crates/claudefs-transport/src/lib.rs` to:
1. Add `pub mod wire_diag;` and `pub mod credit_window;` declarations in the alphabetical section
2. Add `pub use` re-exports for the public types from each module

## Types to re-export from wire_diag.rs

From `wire_diag.rs`, re-export these types:
- `InFlightPing`
- `RttSample`
- `RttSeries`
- `RttSeriesSnapshot`
- `TraceHop`
- `TracePath`
- `WireDiag`
- `WireDiagConfig`
- `WireDiagStats`
- `WireDiagStatsSnapshot`

## Types to re-export from credit_window.rs

From `credit_window.rs`, re-export these types:
- `CreditGrant`
- `CreditWindow`
- `CreditWindowConfig`
- `CreditWindowState`
- `CreditWindowStats`
- `CreditWindowStatsSnapshot`

## Instructions

1. Read `crates/claudefs-transport/src/lib.rs` to find where to add the module declarations and re-exports
2. Add `pub mod wire_diag;` near the other `pub mod` declarations (alphabetical order)
3. Add `pub mod credit_window;` near the other `pub mod` declarations (alphabetical order)
4. Add the `pub use wire_diag::{ ... }` block at the end of the file
5. Add the `pub use credit_window::{ ... }` block at the end of the file
6. Run `cargo build -p claudefs-transport` to verify it compiles
7. Run `cargo test -p claudefs-transport` to verify all tests pass

Important: First read the actual wire_diag.rs and credit_window.rs files to confirm which exact types exist before adding the re-exports (the files may have slightly different type names than listed above).
