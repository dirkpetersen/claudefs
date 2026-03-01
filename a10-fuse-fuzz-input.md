# Task: Create FUSE protocol fuzz harness for claudefs-security

## Context
You are working on the `claudefs-security` crate at `crates/claudefs-security/src/fuzz_fuse.rs`.
This is a NEW file to add a FUSE protocol security fuzz harness.

## Requirements

Create a module that tests the FUSE client crate's handling of malformed/adversarial inputs.

### What to test (from claudefs-fuse crate):

The `claudefs-fuse` crate has these key types we can test:
- `FuseConfig` — FUSE mount configuration (serialization/deserialization)
- `CacheConfig` / `CachePolicy` — client-side metadata caching
- `PassthroughConfig` — FUSE passthrough mode settings
- `MountOptions` — mount option parsing and validation

### Test groups:

1. **Malformed mount options** — test FuseConfig with extreme values, empty strings, special chars
2. **Cache policy boundary tests** — test CacheConfig with zero TTL, max TTL, negative-ish values
3. **Passthrough config validation** — test PassthroughConfig with invalid kernel version combos
4. **Serialization fuzzing** — feed random/malformed JSON into FuseConfig deserialization
5. **FUSE operation boundary tests** — test with extreme inode numbers, file sizes, etc.

### Module structure:

```rust
//! Phase 3 FUSE protocol security fuzzing for claudefs-fuse.
//!
//! Findings: FINDING-FUSE-01 through FINDING-FUSE-15
//!
//! Tests adversarial inputs to FUSE configuration and operation handling.
```

### Important: Check what's actually available in claudefs-fuse

Look at these imports from claudefs-fuse to determine what's public:
```rust
use claudefs_fuse::{FuseConfig, CacheConfig, CachePolicy, PassthroughConfig};
```

If some types aren't public or don't exist, create tests around whatever IS public in the crate. At minimum, test:
- Configuration struct serialization/deserialization with malformed input
- Boundary values for any numeric fields
- Empty/null string handling

### Dependencies:
- claudefs-fuse (already a workspace dep but may need adding to Cargo.toml)
- serde_json
- proptest (for property-based testing)

### Constraints:
- Write at least 15 tests
- Use `#[test]` or `proptest!` macros
- No async tests needed (just data validation)
- If a type isn't accessible, write the test with a `#[ignore]` and note what's needed

## First: Read crates/claudefs-fuse/src/lib.rs to see what's public

Before writing the module, read the lib.rs to understand the public API. Then write tests against whatever types are actually exported.

## Output
Output ONLY the complete Rust file content for `crates/claudefs-security/src/fuzz_fuse.rs`.
Do not include markdown code fences or explanations.
