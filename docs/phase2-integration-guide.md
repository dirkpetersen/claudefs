# Phase 2 Integration Guide

For builder agents A5–A8 integrating with foundation agents A1–A4.

## Overview

Phase 2 (current) focuses on integration:
- **A1–A4** (foundation) are complete and stable with 758 tests
- **A5–A8** (builders) now need to wire these subsystems together
- **A9–A11** (cross-cutting) provide validation, security, and CI/CD

This guide documents the stable APIs and patterns for integration.

## Foundation Status (Ready for Integration)

### A1: Storage Engine (`claudefs-storage`)
**Status:** ✅ 60 tests passing, stable API

**Key Types:**
- `StorageEngine` — main interface trait
- `BlockRead`, `BlockWrite` — I/O operations
- `Allocator` — block allocation
- `Device` — NVMe device abstraction

**Usage by:**
- A2 (Raft log storage)
- A3 (segment storage, GC)
- A4 (buffer management)

**Example Integration:**
```rust
// A2: Store Raft state
let engine = StorageEngine::new(config)?;
let block_id = engine.allocate(4096)?;
engine.write(block_id, &raft_state)?;
```

### A2: Metadata Service (`claudefs-meta`)
**Status:** ✅ 417 tests, 31+ modules, stable

**Key Types:**
- `MetadataNode` — unified server (NEW in Phase 2)
- `Inode` — file metadata
- `Service` — POSIX operation dispatcher
- `RaftLogStore` — persistent Raft state
- `ShardRouter` — metadata routing
- `FingerprintIndex` — CAS dedup index
- `MetadataRpc` — RPC protocol types

**Core Subsystems:**
| Module | Purpose | Usage |
|--------|---------|-------|
| `consensus.rs` | Raft consensus | A6 replication |
| `service.rs` | POSIX operations | A5 FUSE, A7 gateways |
| `rpc.rs` | RPC types | A4 transport, A5/A7 clients |
| `fingerprint.rs` | CAS index | A3 dedup |
| `uidmap.rs` | UID translation | A6 replication |
| `membership.rs` | SWIM cluster | A11 cluster mgmt |
| `worm.rs` | Compliance | A8 management |
| `cdc.rs` | Event streaming | A8 management |

**Usage by:**
- A5 (FUSE mounts use MetadataService for inode ops)
- A6 (replication uses RaftLogStore, UidMap)
- A7 (NFS/pNFS use MetadataRpc, Service)
- A8 (management queries MetadataNode)

**Integration Pattern:**
```rust
// A5: Create MetadataNode, wire to FUSE
let meta_node = MetadataNode::new(config)?;
let service = meta_node.service();
// Dispatch POSIX ops via service.lookup(), service.create(), etc.
```

### A3: Data Reduction (`claudefs-reduce`)
**Status:** ✅ 223 tests, 10 modules, stable

**Key Types:**
- `Pipeline` — write pipeline (chunk → dedup → compress → encrypt)
- `CasIndex` — content-addressable storage index
- `Deduplicator` — BLAKE3 fingerprinting
- `Compressor` — LZ4/Zstd
- `Cipher` — AES-256-GCM encryption
- `BackgroundProcessor` — async tier-2 dedup
- `SegmentPacker` — 2MB segments for EC
- `KeyManager` — envelope encryption

**Write Pipeline Order:**
1. Chunk (FastCDC)
2. Dedupe (BLAKE3)
3. Compress (LZ4/Zstd)
4. Encrypt (AES-GCM)
5. Segment (2MB for EC 4+2)
6. Flush to storage (A1) + tiering (S3)

**Usage by:**
- A1 (segment storage)
- A2 (fingerprint index integration)
- A6 (replication sees deduped data)
- A7 (S3 API reads deduped segments)

**Integration Pattern:**
```rust
// A5: Write path through reduction pipeline
let mut pipeline = Pipeline::new(config)?;
let reduced = pipeline.process(user_data)?;
// Writes through A1 storage, stores fingerprints in A2 index
```

### A4: Transport (`claudefs-transport`)
**Status:** ✅ 58 tests, RPC protocol stable

**Key Types:**
- `Transport` — bidirectional connection trait
- `TcpTransport`, `RdmaTransport` — implementations
- `RpcMessage` — protocol frame
- `QosManager` — traffic shaping
- `TlsConfig` — mTLS certificates

**RPC Protocol:**
- 24 opcodes: metadata reads/writes, data reads/writes, heartbeat, etc.
- Binary format: little-endian u32 opcode + bincode payload
- QoS: token bucket per workload class

**Usage by:**
- A5 (FUSE client connects to storage nodes)
- A6 (cross-site replication)
- A7 (NFS/pNFS gateways)
- A8 (admin API)

**Integration Pattern:**
```rust
// A5: Connect to metadata node via transport
let transport = TcpTransport::connect("node1:9200")?;
let req = MetadataRpc::Lookup { inode: 123 };
let resp = transport.rpc(req)?;
```

## Integration Tasks for A5–A8

### A5: FUSE Client
**Phase 2 Goal:** Wire FUSE daemon to A2+A4

**Current Modules:**
- `filesystem.rs` — FUSE daemon skeleton
- `cache.rs` — client-side metadata cache
- `passthrough.rs` — passthrough mode for kernel 6.8+
- `operations.rs` — POSIX syscall mappings
- `server.rs` — async Tokio server

**Integration Checklist:**
- [ ] Create MetadataNode instance per `docs/phase2-integration-guide.md`
- [ ] Wire FUSE operations to MetadataService methods
- [ ] Connect to A2 metadata via TcpTransport (A4)
- [ ] Implement client-side metadata cache (LRU + invalidation)
- [ ] Wire data reads to A1 storage via Transport
- [ ] Test with single-node pjdfstest subset
- [ ] Performance: cache hit rate >90% for metadata

**Key Integration Points:**
```rust
// A5 pseudocode
loop {
  fuse_req = fuse_session.next_request();
  match fuse_req.opcode {
    LOOKUP => {
      inode = metadata_cache.get_or_fetch(path);
      fuse_session.reply_entry(inode);
    }
    READ => {
      data = storage_engine.read(block_id);
      fuse_session.reply_data(data);
    }
  }
}
```

### A6: Replication
**Phase 2 Goal:** Wire journal tailer and cloud conduit to A2

**Current Modules:**
- `journal.rs` — WAL tailer
- `wal.rs` — write-ahead log
- `conduit.rs` — gRPC mTLS relay
- `sync.rs` — sync protocol
- `topology.rs` — site topology
- `main.rs` — conduit process

**Integration Checklist:**
- [ ] Integrate with A2's RaftLogStore (persistent Raft state)
- [ ] Tail A2 commit log for cross-site replication
- [ ] Implement gRPC mTLS endpoint
- [ ] Test: write site A → replicate to site B → verify
- [ ] Conflict detection: last-write-wins with admin alert
- [ ] Performance: replication lag <1s at 10K ops/sec

**Key Integration Points:**
```rust
// A6 pseudocode
let raft_log = meta_node.raft_log_store();
loop {
  entries = raft_log.get_entries(cursor)?;
  conduit_client.replicate_batch(entries)?;
  cursor += entries.len();
}
```

### A7: Protocol Gateways
**Phase 2 Goal:** Translate NFS/pNFS/SMB to ClaudeFS

**Current Modules:**
- `nfs.rs` — NFSv3 server
- `pnfs.rs` — pNFS layout server
- `smb.rs` — Samba VFS skeleton (C code)
- `s3.rs` — S3 API endpoint
- `protocol.rs` — shared gateway types
- `main.rs` — gateway process

**Integration Checklist:**
- [ ] Wire NFS operations to A2 MetadataService (via A4 Transport)
- [ ] Implement pNFS layouts (direct-to-storage for large reads)
- [ ] Implement S3 GET/PUT/DELETE (map to A3 reduced data)
- [ ] Test: mount via NFS, read/write, verify vs FUSE
- [ ] Performance: NFS latency <5ms (local network)

**Key Integration Points:**
```rust
// A7 pseudocode
// NFS LOOKUP
let inode = meta_service.lookup(parent_inode, name)?;
nfs_reply.entry = inode;

// S3 PUT
let data = s3_req.body;
let fingerprint = pipeline.dedupe(data)?;
storage.write_segment(fingerprint, data)?;
```

### A8: Management
**Phase 2 Goal:** Wire CLI/API to cluster operations

**Current Modules:**
- `cli.rs` — command-line interface
- `api.rs` — admin REST API
- `metrics.rs` — Prometheus exporter
- `analytics.rs` — DuckDB query gateway
- `config.rs` — configuration management
- `main.rs` — management daemon

**Integration Checklist:**
- [ ] Query A2 MetadataNode for cluster status
- [ ] Implement `cfs admin top-users` (top K users by consumption)
- [ ] Implement `cfs admin snapshot create/restore`
- [ ] Wire Prometheus metrics from A1/A2/A3/A4
- [ ] Implement DuckDB analytics over inode metadata
- [ ] Test: CLI commands work, metrics exported

**Key Integration Points:**
```rust
// A8 pseudocode
// CLI: cfs admin status
let meta_node = connect_to_metadata_node()?;
let status = meta_node.cluster_status()?;
println!("Nodes: {}, Space: {}", status.node_count, status.total_space);

// Metrics
prometheus_counter!("claudefs_inode_count", meta_node.inode_count());
```

## Testing Strategy for Phase 2

### Unit Tests (Per Crate)
- A5, A6, A7, A8 each add unit tests for new modules
- Mock dependencies (e.g., mock MetadataService for testing)

### Integration Tests
- **A5+A2:** FUSE mounts, basic POSIX ops (create, read, write, delete)
- **A6+A2:** Replication: write A → verify appears on B
- **A7+A2:** NFS mount, POSIX compatibility
- **A8+A2:** Admin CLI queries, metrics export

### Multi-Node Tests
- Scheduled for A9 (Test & Validation) with real cluster
- Phase 2 focuses on single-node correctness

### Performance Targets (Phase 2)
| Operation | Target | Notes |
|-----------|--------|-------|
| Metadata lookup | <1ms | cached |
| Data read (4KB) | <5ms | single node |
| Data write (4KB) | <10ms | 2x journal replication |
| Replication lag | <1s | cross-site |
| NFS operation | <5ms | local network |

## Common Patterns

### Async Error Handling
All integrations use `thiserror` for errors, `anyhow` at entry points:

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FuseError {
    #[error("Metadata not found: {0}")]
    NotFound(u64),

    #[error("Transport error: {0}")]
    Transport(#[from] TransportError),
}

// Entry point uses anyhow::Result
fn main() -> anyhow::Result<()> {
    let server = FuseServer::new(config)?;
    server.run().await?;
    Ok(())
}
```

### Tracing and Observability
All operations get trace spans:

```rust
use tracing::{debug, trace, Instrument};

async fn handle_operation(req: FuseRequest) -> Result<()> {
    let span = tracing::info_span!("fuse_op", opcode = req.opcode, inode = req.nodeid);
    debug!("Starting operation");

    let result = process_request(req).instrument(span.clone()).await?;

    trace!("Operation complete");
    Ok(result)
}
```

### Configuration
Use `serde` + environment variables:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuseConfig {
    pub metadata_node_addr: String,
    pub max_pending_requests: usize,
    pub cache_size: usize,
}

impl FuseConfig {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            metadata_node_addr: std::env::var("CFS_META_ADDR")?,
            max_pending_requests: std::env::var("CFS_MAX_PENDING")
                .unwrap_or("1000".to_string())
                .parse()?,
            cache_size: std::env::var("CFS_CACHE_SIZE")
                .unwrap_or("10485760".to_string())  // 10MB default
                .parse()?,
        })
    }
}
```

## Troubleshooting

### Connection Refused
- Check metadata node is running: `cfs server`
- Check IP/port in config (default: `127.0.0.1:9200`)
- Check firewall: `netstat -tlnp | grep 9200`

### RPC Timeout
- Increase timeout in TransportConfig
- Check network latency: `ping metadata_node`
- Check metadata node load: `cfs admin status`

### Test Failures
- Run with RUST_BACKTRACE=1 for full backtrace
- Add `#[tracing_test::traced_test]` to enable tracing in tests
- Check logs in `/var/log/claudefs/` (future)

## References

- `docs/decisions.md` — architecture decisions (D1–D10)
- `docs/metadata.md` — metadata service design
- `docs/transport.md` — transport protocol
- `docs/reduction.md` — data reduction pipeline
- `crates/claudefs-meta/src/lib.rs` — A2 public API
- `crates/claudefs-storage/src/lib.rs` — A1 public API
- `crates/claudefs-reduce/src/lib.rs` — A3 public API
- `crates/claudefs-transport/src/lib.rs` — A4 public API
