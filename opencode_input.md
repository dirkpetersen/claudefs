# ClaudeFS Cargo Workspace Setup

## Task
Create a Cargo workspace root and 8 crate stubs for the ClaudeFS distributed file system project.

## Context
- Project: ClaudeFS — distributed, scale-out POSIX file system
- Implementation: Rust, organized as Cargo workspace with one crate per agent
- Phase: Phase 1 foundation
- Each crate will be owned by an AI agent (A1–A8)

## Workspace Structure to Create

```
claudefs/
├── Cargo.toml              # Workspace root (NEW)
├── crates/                 # NEW
│   ├── claudefs-storage/   # A1: io_uring NVMe, block allocator
│   ├── claudefs-meta/      # A2: Raft consensus, KV store
│   ├── claudefs-reduce/    # A3: dedupe, compression, encryption
│   ├── claudefs-transport/ # A4: RDMA/TCP, custom RPC
│   ├── claudefs-fuse/      # A5: FUSE v3 daemon, passthrough
│   ├── claudefs-repl/      # A6: cross-site journal replication
│   ├── claudefs-gateway/   # A7: NFSv3, pNFS, S3 API, Samba VFS
│   └── claudefs-mgmt/      # A8: Prometheus, DuckDB, Web UI, CLI
```

## Specifications

### Workspace Root (Cargo.toml)

1. **Workspace definition:**
   - `[workspace]` with `members = ["crates/*"]`
   - `resolver = "2"`

2. **Shared dependencies** (specified at workspace level to ensure consistency):
   - `tokio = { version = "1.40", features = ["full"] }` — async runtime
   - `thiserror = "1.0"` — error handling for libraries
   - `anyhow = "1.0"` — error handling for binaries
   - `serde = { version = "1.0", features = ["derive"] }` — serialization
   - `bincode = "1.3"` — binary serialization for wire format
   - `prost = "0.13"` — gRPC/Protobuf
   - `tonic = "0.12"` — gRPC runtime
   - `tracing = "0.1"` — structured logging
   - `tracing-subscriber = { version = "0.3", features = ["fmt", "json"] }`

3. **No default-run or [[bin]]** — each crate defines its own binaries

### Per-Crate Structure

Each crate in `crates/` follows this minimal structure:

```
crates/claudefs-X/
├── Cargo.toml
├── src/
│   └── lib.rs    # or main.rs if crate produces a binary
├── tests/        # integration tests directory (created, empty)
└── benches/      # benchmarks directory (created, empty)
```

### Cargo.toml for Each Crate

Each crate's Cargo.toml:
- `name = "claudefs-X"`
- `version = "0.1.0"`
- `edition = "2021"`
- `description = "ClaudeFS subsystem: [description]"`
- `license = "MIT"`
- `authors = ["Dirk Petersen"]`

1. **Crate-specific dependencies:**
   - Only add direct dependencies needed for that crate
   - Reference workspace-level dependencies without version specifier

2. **Lib crates:**
   - `claudefs-storage`, `claudefs-reduce`, `claudefs-transport`: libraries (only `[lib]`)
   - `claudefs-meta`, `claudefs-fuse`, `claudefs-repl`, `claudefs-gateway`, `claudefs-mgmt`: mostly libraries, produce server/client binaries

3. **Don't include Bedrock/OpenCode-specific deps** — those are only for the orchestrator

### Initial Crate Descriptions

- `claudefs-storage`: Local NVMe I/O via io_uring, FDP/ZNS placement, block allocator
- `claudefs-meta`: Distributed metadata, Raft consensus, inode/directory operations
- `claudefs-reduce`: Inline dedupe (BLAKE3), compression (LZ4/Zstd), encryption (AES-GCM)
- `claudefs-transport`: RDMA via libfabric, TCP via io_uring, custom RPC protocol
- `claudefs-fuse`: FUSE v3 daemon, passthrough mode, client-side caching
- `claudefs-repl`: Cross-site journal replication, cloud conduit (gRPC/mTLS)
- `claudefs-gateway`: NFSv3 gateway, pNFS layouts, S3 API endpoint
- `claudefs-mgmt`: Prometheus exporter, DuckDB analytics, Web UI, CLI, admin API

## Output Format

Generate complete `Cargo.toml` files for:
1. Workspace root: `/home/cfs/claudefs/Cargo.toml`
2. Each crate: `crates/claudefs-X/Cargo.toml` (8 files)

For each crate, also generate a minimal `src/lib.rs` with:
- Module doc comment explaining the crate's purpose
- Empty module stubs for major subsystems (to be filled by the agent)
- Example: for storage, stubs like `pub mod allocator;`, `pub mod io_uring_bridge;`, etc.

## Key Requirements

- Workspace uses `resolver = "2"` for better dependency resolution
- All unsafe code will be confined to A1/A4/A5/A7 (no unsafe in A2/A3/A6/A8)
- All crates use Tokio async runtime (single runtime per binary)
- Error handling: `thiserror` for library errors, `anyhow` for binaries
- No Bedrock or agent orchestration code in crates — that's in the orchestrator layer
- Ready for `cargo build` and `cargo test` immediately after creation
- Each crate's `lib.rs` should have `#![warn(missing_docs)]` to encourage documentation

## Expected Output

Provide complete file contents for all Cargo.toml files and initial src/lib.rs files. I will write them to the filesystem using Write tool.
