# Fix divide-by-zero in splice.rs plan_file_to_socket

## Context

File: `crates/claudefs-transport/src/splice.rs`

The `plan_file_to_socket` method has a divide-by-zero bug when `splice_chunk_size` is 0.

## The Bug

At line 213-214:
```rust
let chunk_size = self.config.splice_chunk_size;
let num_chunks = (length + chunk_size - 1) / chunk_size;
```

When `chunk_size == 0`, the division by zero panics.

## The Test That Fails

```rust
#[test]
fn test_config_validation() {
    // Zero chunk size should work (edge case)
    let config = SpliceConfig {
        max_pipe_size: 1024,
        splice_chunk_size: 0,
        sendfile_fallback: true,
        use_splice_move: true,
        use_splice_more: true,
    };
    let pipeline = SplicePipeline::new(config);
    let plan = pipeline.plan_file_to_socket("/data/file.bin", "192.168.1.1:8080", 0, 100);

    // With 0 chunk size, should still produce operations but may have 1 chunk
    assert!(plan.estimated_chunks >= 1);
}
```

## The Fix Required

In `plan_file_to_socket`, guard against zero chunk size by treating 0 as a "use the full length in one chunk" case. When `chunk_size == 0`, use `length` as the effective chunk size (i.e., treat as one big chunk).

Specifically, replace:
```rust
let chunk_size = self.config.splice_chunk_size;
let num_chunks = (length + chunk_size - 1) / chunk_size;
```

With:
```rust
let chunk_size = if self.config.splice_chunk_size == 0 {
    length
} else {
    self.config.splice_chunk_size
};
let num_chunks = (length + chunk_size - 1) / chunk_size;
```

## Instructions

Edit ONLY the `plan_file_to_socket` function in `crates/claudefs-transport/src/splice.rs`.

Change lines 213-214 (which currently read):
```rust
        let chunk_size = self.config.splice_chunk_size;
        let num_chunks = (length + chunk_size - 1) / chunk_size;
```

To:
```rust
        let chunk_size = if self.config.splice_chunk_size == 0 {
            length
        } else {
            self.config.splice_chunk_size
        };
        let num_chunks = (length + chunk_size - 1) / chunk_size;
```

Apply this change to the file, keeping all other code unchanged.
Output the full modified function `plan_file_to_socket` so I can verify it is correct.
