# Development Agents: Parallel Implementation Plan

ClaudeFS is designed to be implemented by multiple AI coding agents working in parallel. Each agent owns a well-defined subsystem with clear interfaces to other subsystems. This document defines the agent breakdown, dependencies, and phasing.

## Subsystem Decomposition

The architecture decomposes into 8 implementable subsystems. Each maps to one agent working in its own Cargo workspace crate within a single monorepo.

| Agent | Crate | Owns | Depends On |
|-------|-------|------|-----------|
| **A1: Storage Engine** | `claudefs-storage` | Local NVMe I/O via io_uring, FDP/ZNS data placement, flash allocator, block read/write | Kernel 6.20+ |
| **A2: Metadata Service** | `claudefs-meta` | Distributed metadata, Raft consensus, inode/directory operations, distributed locking, speculative path resolution | A1 (storage for Raft log) |
| **A3: Data Reduction** | `claudefs-reduce` | Inline dedupe (BLAKE3 CAS), similarity pipeline, LZ4/Zstd compression, AES-GCM encryption, reference counting, GC | A1 (block storage), A2 (fingerprint index) |
| **A4: Transport** | `claudefs-transport` | RDMA via libfabric, TCP via io_uring, custom RPC protocol, connection management, zero-copy data transfer | A1 (block read/write) |
| **A5: FUSE Client** | `claudefs-fuse` | FUSE v3 daemon, passthrough mode, client-side metadata cache, mount/unmount, POSIX syscall handling | A2 (metadata), A4 (transport) |
| **A6: Replication** | `claudefs-repl` | Cross-site journal replication, cloud conduit (gRPC/mTLS), UID mapping, conflict detection, batch compaction | A2 (Raft journal), A4 (transport) |
| **A7: NFS/pNFS Gateway** | `claudefs-nfs` | NFSv3 gateway, pNFS layout server, NFS v4.2 exports | A2 (metadata), A4 (transport) |
| **A8: Management** | `claudefs-mgmt` | Prometheus exporter, Parquet indexer, DuckDB query gateway, Web UI, CLI, admin API | A2 (metadata journal) |

## Dependency Graph

```
           A1: Storage Engine
          /        |         \
    A2: Metadata   A3: Reduce  A4: Transport
      |    \         |           /      |
      |     \        |          /       |
    A5: FUSE  A6: Replication   A7: NFS/pNFS
      |
    A8: Management
```

A1 (Storage Engine) has zero internal dependencies — it starts first. A2, A3, A4 depend only on A1 and can run in parallel. A5, A6, A7 depend on A2+A4 and start once those interfaces stabilize. A8 depends primarily on A2 and can start in parallel with A5-A7.

## Phasing

### Phase 1: Foundation (Agents A1 + A2 + A4 in parallel, A3 starts)

**3 agents running simultaneously:**

- **A1: Storage Engine** — implement io_uring NVMe passthrough, block allocator, FDP hint tagging, basic read/write API. This is the lowest-level crate and blocks everything else, so it starts first with the highest priority.
- **A2: Metadata Service** — implement the Raft consensus engine, KV store on NVMe (via A1's block API), inode operations, directory operations, distributed locking. Can begin with an in-memory mock of A1 while A1 is in progress.
- **A4: Transport** — implement the custom RPC protocol, RDMA and TCP backends, connection lifecycle, zero-copy buffer management. Can be developed against loopback/localhost initially.
- **A3: Data Reduction** — begin BLAKE3 hashing, FastCDC chunking, LZ4/Zstd compression as standalone library functions. These don't need the full storage engine yet — they're pure data transforms with well-defined inputs and outputs.

**End of Phase 1:** A single node can store blocks on NVMe, manage metadata with Raft, and transfer data between processes over RDMA or TCP. Data reduction works as a library but isn't integrated into the write path yet.

### Phase 2: Integration (Agents A5 + A3 integration + A6 starts, A7 starts)

**4 agents running simultaneously:**

- **A5: FUSE Client** — wire up the FUSE daemon to A2 (metadata) and A4 (transport). Implement passthrough mode, client caching, mount/unmount. This is where POSIX compliance is tested (pjdfstest, fsx).
- **A3: Data Reduction (integration)** — integrate the reduction pipeline into A1's write path. Inline dedupe on writes, compression before flush, encryption before store. Wire up the fingerprint index to A2's metadata.
- **A6: Replication** — implement the journal tailer on A2's Raft log, the cloud conduit (gRPC/mTLS via `tonic`), UID mapping, conflict detection. Can test with two local instances initially.
- **A7: NFS/pNFS Gateway** — implement NFSv3 translation and pNFS layout serving on top of A2+A4. Can start once A2's metadata API is stable.

**End of Phase 2:** A multi-node cluster with FUSE mounts, data reduction, and cross-site replication. NFS access works for legacy clients.

### Phase 3: Production Readiness (Agents A8 + hardening across all)

**All agents active, focus shifts to testing and operations:**

- **A8: Management** — Prometheus exporter, Parquet indexer, DuckDB gateway, Web UI, CLI. Depends on A2's journal for indexing.
- **All agents:** POSIX test suites (xfstests, Connectathon, Jepsen), crash consistency testing (CrashMonkey), performance benchmarking (FIO), failure injection, multi-node integration tests.

**End of Phase 3:** Production-ready system with monitoring, management, and validated POSIX compliance.

## Maximum Parallelism

At peak (Phase 2), **4 agents work simultaneously** on independent codepaths. The trait-based Rust architecture enables this:

- Each crate defines its public trait interface first (reviewed by all agents)
- Agents implement against the trait, using mock implementations of dependencies
- Integration happens when two crates are wired together via the real implementation
- The Rust compiler catches interface mismatches at compile time — no runtime surprises when agents' code meets

## Shared Conventions (All Agents)

- **Error handling:** `thiserror` for library errors, `anyhow` at binary entry points
- **Serialization:** `serde` + `bincode` for internal wire format, `prost` for gRPC Protobuf
- **Async:** Tokio with io_uring backend. All I/O through io_uring submission rings.
- **Logging:** `tracing` crate with structured spans. Every operation gets a trace ID for distributed debugging.
- **Testing:** property-based tests (`proptest`) for data transforms, integration tests per crate, Jepsen-style tests for distributed correctness
- **`unsafe` budget:** confined to A1 (io_uring FFI), A4 (RDMA/libfabric FFI), A5 (FUSE FFI). All other crates are safe Rust.

## Feature Gaps: What VAST/Weka Have That ClaudeFS Must Address

The following features are standard in VAST and Weka but not yet covered in ClaudeFS docs. These are tracked as future work items, prioritized by impact:

### Priority 1 — Required for Production

| Feature | Description | Owner Agent |
|---------|-------------|-------------|
| **Multi-tenancy & quotas** | Per-user/group storage quotas, IOPS/bandwidth limits, tenant isolation | A2 (metadata enforcement) + A8 (reporting) |
| **QoS / traffic shaping** | Priority queues, bandwidth guarantees per workload class, SLA enforcement | A4 (transport) + A2 (policy) |
| **Online node scaling** | Add/remove nodes without downtime, automatic data rebalancing | A1 (storage) + A2 (metadata) |
| **Flash layer defragmentation** | Background compaction and garbage collection for the flash allocator | A1 (storage) |
| **Distributed tracing** | OpenTelemetry integration, per-request latency attribution across nodes | All agents (tracing crate) |

### Priority 2 — Required for Enterprise Adoption

| Feature | Description | Owner Agent |
|---------|-------------|-------------|
| **Compliance / WORM** | Immutable snapshots, legal holds, retention policies, audit trails | A2 (metadata) + A3 (snapshots) |
| **Key rotation** | Rotate encryption keys without re-encrypting all data (envelope encryption) | A3 (reduction) |
| **S3 API endpoint** | Expose ClaudeFS namespace as S3-compatible object storage | A7 (new protocol frontend) |
| **SMB/CIFS gateway** | Windows client access via Samba integration | A7 (new protocol frontend) |
| **Data migration tools** | Import from Lustre/CephFS/NFS, parallel copy, migration without downtime | A8 (management) + A5 (client) |

### Priority 3 — Competitive Differentiation

| Feature | Description | Owner Agent |
|---------|-------------|-------------|
| **Intelligent tiering** | Access-pattern learning, automatic hot/cold classification, predictive placement | A1 (storage) + A8 (analytics) |
| **Change data capture** | Event stream API for filesystem changes (create/delete/rename webhooks) | A2 (metadata journal) |
| **Active-active failover** | Automatic site failover with read-write on both sites | A6 (replication) |
| **Performance SLAs** | Published latency targets (p99), scaling curves, capacity planning models | A8 (management) |
