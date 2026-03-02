# A5 FUSE Documentation - Smart Batch Strategy

## Goal
Eliminate remaining ~1645 missing_docs warnings by processing files with highest warning density first.

## High-Impact Files (Process These First)

### Tier 1: Core Type-Heavy Files (Start Here - 250+ fields/methods)

1. **filesystem.rs** — Main FUSE handler
   - Large impl block for filesystem operations
   - Many pub methods to document
   - ~200 warnings

2. **cache_coherence.rs** — Cache protocol
   - CacheCoherence struct/methods
   - Protocol types
   - ~80 warnings

3. **workload_class.rs** — Workload classification
   - WorkloadProfile, AccessPattern, etc.
   - Many fields and methods
   - ~80 warnings

4. **sec_policy.rs** — Security policy
   - SecurityPolicy, policy enforcement
   - Many configuration fields
   - ~100 warnings

5. **client_auth.rs** — mTLS authentication
   - ClientAuth struct
   - Authentication methods
   - ~80 warnings

### Tier 2: Data Structure Files (80-150 warnings each)

6. inode.rs — Inode table and entries
7. openfile.rs — Open file handles
8. writebuf.rs — Write buffering
9. prefetch.rs — Read-ahead
10. quota_enforce.rs — Quota management
11. worm.rs — WORM compliance
12. mount_opts.rs — Mount options

### Tier 3: Remaining Modules (40-80 warnings each)

Process alphabetically to ensure systematic coverage of remaining 48 files.

## Documentation Standard

For each file, add `///` doc comments following this priority:
1. Struct/Enum definitions and fields (high impact, many fields = many warnings)
2. Public impl methods (high impact, many methods)
3. Helper functions and types
4. Associated constants and type aliases

## Example Pattern

```rust
/// Description of what this type represents.
///
/// Thread-safety: [if relevant]
/// Performance: [if relevant]
pub struct MyStruct {
    /// What this field stores/represents.
    pub field1: Type1,

    /// Purpose of this field.
    pub field2: Type2,
}

impl MyStruct {
    /// What this method does.
    ///
    /// # Arguments
    /// * `param` - description of param
    ///
    /// # Returns
    /// Brief description of return value
    pub fn my_method(&self, param: Type) -> Result<ReturnType> { ... }
}
```

## Batching Strategy

Process each file through 1-2 complete reads + writes to capture:
- All struct fields
- All enum variants
- All public methods/functions
- All associated items

This ensures 90%+ of warnings per file are eliminated.

## Expected Outcome

Target: 0 warnings for A5 crate
Approach: Process all 55 modules systematically
Current state: 1645 warnings (reduced from 1700 after attr.rs)

## Implementation Sequence

1. Start with filesystem.rs (highest impact)
2. Then cache_coherence.rs, workload_class.rs, sec_policy.rs, client_auth.rs (Tier 1)
3. Then all Tier 2 files
4. Then Tier 3 files alphabetically
5. Verify with `cargo clippy -p claudefs-fuse` after each batch

This phased approach ensures rapid progress on high-impact files first.
