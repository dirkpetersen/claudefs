# Fix: claudefs-transport/src/multipath.rs — PathSelectionPolicy default should be LowestLatency

## Problem

The `PathSelectionPolicy` enum in `crates/claudefs-transport/src/multipath.rs`
has `RoundRobin` as the default variant (via `#[default]` attribute), but the
tests expect `LowestLatency` to be the default.

## Failing test

```
test transport_new_modules_tests::tests::test_path_policy_default_lowest_latency
  assertion `left == right` failed
  left: RoundRobin
  right: LowestLatency
```

## Current code

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PathSelectionPolicy {
    #[default]
    RoundRobin,
    LowestLatency,
    WeightedRandom,
    Failover,
}
```

## Required fix

Move the `#[default]` attribute from `RoundRobin` to `LowestLatency`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PathSelectionPolicy {
    RoundRobin,
    #[default]
    LowestLatency,
    WeightedRandom,
    Failover,
}
```

## File to modify

`crates/claudefs-transport/src/multipath.rs`

## Important

- Only make this minimal change (move the `#[default]` attribute)
- Do not modify any other code in the file
- The change ensures `PathSelectionPolicy::default() == PathSelectionPolicy::LowestLatency`
