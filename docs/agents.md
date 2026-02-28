# Development Agents: Parallel Implementation Plan

ClaudeFS is designed to be implemented by multiple AI coding agents working in parallel. Each agent owns a well-defined subsystem with clear interfaces to other subsystems. This document defines the agent breakdown, dependencies, phasing, and infrastructure.

## Agent Roster: 11 Agents

### Builder Agents (A1–A8): Write Code

| Agent | Crate | Owns | Depends On |
|-------|-------|------|-----------|
| **A1: Storage Engine** | `claudefs-storage` | Local NVMe I/O via io_uring, FDP/ZNS data placement, flash allocator, block read/write | Kernel 6.20+ |
| **A2: Metadata Service** | `claudefs-meta` | Distributed metadata, Raft consensus, inode/directory operations, distributed locking, speculative path resolution | A1 (storage for Raft log) |
| **A3: Data Reduction** | `claudefs-reduce` | Inline dedupe (BLAKE3 CAS), similarity pipeline, LZ4/Zstd compression, AES-GCM encryption, reference counting, GC | A1 (block storage), A2 (fingerprint index) |
| **A4: Transport** | `claudefs-transport` | RDMA via libfabric, TCP via io_uring, custom RPC protocol, connection management, zero-copy data transfer | A1 (block read/write) |
| **A5: FUSE Client** | `claudefs-fuse` | FUSE v3 daemon, passthrough mode, client-side metadata cache, mount/unmount, POSIX syscall handling | A2 (metadata), A4 (transport) |
| **A6: Replication** | `claudefs-repl` | Cross-site journal replication, cloud conduit (gRPC/mTLS), UID mapping, conflict detection, batch compaction | A2 (Raft journal), A4 (transport) |
| **A7: Protocol Gateways** | `claudefs-gateway` | NFSv3 gateway, pNFS layout server, NFS v4.2 exports, Samba VFS plugin for SMB3, S3 API endpoint | A2 (metadata), A4 (transport) |
| **A8: Management** | `claudefs-mgmt` | Prometheus exporter, Parquet indexer, DuckDB query gateway, Web UI (React), CLI, admin API (Axum) | A2 (metadata journal) |

### Cross-Cutting Agents (A9–A11): Validate, Secure, Deploy

| Agent | Owns | When Active |
|-------|------|-------------|
| **A9: Test & Validation** | POSIX test suites (pjdfstest, xfstests, fsx), distributed tests (Connectathon, Jepsen), crash consistency (CrashMonkey), performance benchmarks (FIO), integration tests, regression suite | Phase 1 (unit tests) → Phase 2+ (full suite) |
| **A10: Security Audit** | `unsafe` code review, fuzzing (network protocol, FUSE, NFS), authentication/authorization audit, encryption implementation review, dependency CVE scanning, penetration testing of management API and cloud conduit | Phase 2 → Phase 3 |
| **A11: Infrastructure & CI** | AWS provisioning (Terraform/Pulumi), test cluster orchestration, preemptible node management, CI/CD pipeline (GitHub Actions), artifact builds, multi-node deployment automation | Phase 1 (CI setup) → always |

### Why Separate Test, Security, and Infra Agents

**A9 (Test & Validation)** must be separate from builders because:
- Adversarial mindset — builders want code to work, testers want to break it
- Cross-cutting scope — tests exercise code across all crates simultaneously (a rename test hits A5+A2+A4+A1)
- Test suites are large standalone projects (Jepsen alone is a significant effort)
- Regression ownership — when a builder's change breaks a test, A9 files the issue and the builder fixes it

**A10 (Security Audit)** must be separate because:
- Reviews `unsafe` blocks in A1/A4/A5 with fresh eyes — the author of `unsafe` code is the worst person to audit it
- Fuzzing is a specialized discipline — libfuzzer/AFL++ against the RPC protocol, FUSE interface, NFS gateway, management API
- Crypto review — A3's encryption implementation needs independent verification (correct AES-GCM nonce handling, HKDF key derivation, no timing side channels)
- Dependency auditing — `cargo audit` in CI, plus manual review of `unsafe` in transitive dependencies

**A11 (Infrastructure & CI)** must be separate because:
- Builder agents should never manage infrastructure — they write code, push it, and CI handles the rest
- Preemptible instance management is a full-time operational concern
- Test cluster lifecycle (provision → deploy → test → teardown) must be automated and reproducible

## Dependency Graph

```
                  A11: Infrastructure & CI
                  (provisions everything)
                          |
                  A1: Storage Engine
                /        |         \
          A2: Metadata  A3: Reduce  A4: Transport
            |    \        |           /      |
            |     \       |          /       |
          A5: FUSE  A6: Replication  A7: Gateways
            |                                |
          A8: Management              (Samba VFS, S3)
                          |
                  A9: Test & Validation
                  (tests across all crates)
                          |
                  A10: Security Audit
                  (reviews all crates)
```

## Phasing

### Phase 1: Foundation

**5 agents active (4 builders + infra):**

- **A1: Storage Engine** — io_uring NVMe passthrough, block allocator, FDP hint tagging, read/write API
- **A2: Metadata Service** — Raft consensus, KV store, inode/directory ops. Starts with in-memory mock of A1.
- **A4: Transport** — RPC protocol, RDMA + TCP backends, connection lifecycle. Develops against localhost.
- **A3: Data Reduction** — BLAKE3, FastCDC, LZ4/Zstd, AES-GCM as standalone library. Pure data transforms.
- **A11: Infrastructure** — set up CI/CD, provision first 3-node test cluster, automate build+deploy pipeline

**A9 begins writing:** unit test harnesses, property-based tests for each crate's trait interface, basic pjdfstest wrapper.

**End of Phase 1:** Single-node storage + metadata + transport. Data reduction as library. CI deploying to test cluster.

### Phase 2: Integration

**7 agents active (6 builders + infra), A9 and A10 ramp up:**

- **A5: FUSE Client** — wire FUSE daemon to A2+A4, passthrough mode, client caching
- **A3: Data Reduction** — integrate into write path, wire fingerprint index to A2
- **A6: Replication** — journal tailer, cloud conduit, UID mapping, conflict detection
- **A7: Protocol Gateways** — NFSv3 translation, pNFS layouts. **Samba VFS plugin** development begins (see below).
- **A8: Management** — Prometheus exporter, Parquet indexer, DuckDB gateway
- **A9: Test & Validation** — full POSIX suites (pjdfstest, fsx, xfstests) on multi-node cluster, Connectathon across nodes, begin Jepsen partition tests
- **A10: Security** — begin `unsafe` code review (A1, A4, A5), fuzz the RPC protocol, audit authentication in A6 conduit and A8 admin API
- **A11: Infrastructure** — scale to full test cluster (see below), preemptible instance automation

**End of Phase 2:** Multi-node cluster with FUSE + NFS + replication + data reduction + monitoring. POSIX tests passing.

### Phase 3: Production Readiness

**All 11 agents active:**

- **Builders (A1–A8):** bug fixes from test/security findings, performance optimization, feature gaps (quotas, QoS, node scaling)
- **A9:** Jepsen split-brain tests, CrashMonkey crash consistency, FIO performance benchmarks, long-running soak tests
- **A10:** full penetration test of management API, crypto audit, dependency CVE sweep, final `unsafe` review
- **A11:** production deployment templates, documentation of operational procedures

**End of Phase 3:** Production-ready, validated, security-audited system.

## SMB3 Gateway: Samba VFS Plugin

The SMB3 gateway is **not** a from-scratch SMB implementation. SMB3 is an enormous protocol — reimplementing it would be years of work. Instead:

**Approach:** Write a Samba VFS module (~2,000–5,000 lines of C) that translates Samba's VFS operations to ClaudeFS's internal RPC protocol.

- **Samba handles:** all SMB3.1.1 protocol complexity, Kerberos/NTLM authentication, encryption, signing, compound requests, credit-based flow control, oplock/lease management
- **VFS plugin handles:** mapping `open`/`read`/`write`/`stat`/`rename`/`mkdir` to ClaudeFS transport calls (A4)
- **Deployment:** Samba runs as a gateway process on dedicated nodes or co-located on ClaudeFS servers
- **License:** Samba is GPLv3 but runs as a separate process communicating over ClaudeFS's RPC protocol — no license contamination of the MIT codebase. The VFS plugin itself would be GPLv3 (required for Samba modules), distributed as a separate package.
- **SMB3+ only** — no SMB1/SMB2 support needed. Samba configuration enforces `min protocol = SMB3`.

This is what VAST, Weka, CephFS (`vfs_ceph`), and GlusterFS (`vfs_glusterfs`) all do. It's the proven approach.

**Owner:** A7 (Protocol Gateways), Phase 2.

## Test Infrastructure: AWS Development Cluster

### Orchestrator Node (always running)

One persistent AMD instance where Claude Code runs, managing all agents:

| Component | Specification |
|-----------|--------------|
| Instance | `c7a.2xlarge` (8 vCPU AMD EPYC, 16 GB RAM) or similar |
| Role | Claude Code host, CI/CD controller, agent orchestration |
| Storage | 100 GB gp3 for repos, build artifacts, CI state |
| Always on | Yes — this is the development hub |

### Test Cluster Nodes (preemptible / spot instances)

Spun up for testing, torn down when idle. A11 automates the lifecycle.

| Role | Count | Instance Type | Why |
|------|-------|--------------|-----|
| **Storage servers** | 5 | `i4i.2xlarge` (NVMe, 8 vCPU, 64 GB) | 3 for site A (Raft quorum), 2 for site B (replication) |
| **FUSE client** | 1 | `c7a.xlarge` (4 vCPU, 8 GB) | Runs pjdfstest, fsx, xfstests |
| **NFS/SMB client** | 1 | `c7a.xlarge` (4 vCPU, 8 GB) | Connectathon, multi-protocol testing |
| **Cloud conduit** | 1 | `t3.medium` (2 vCPU, 4 GB) | gRPC relay for cross-site replication |
| **Jepsen controller** | 1 | `c7a.xlarge` (4 vCPU, 8 GB) | Runs Jepsen tests, fault injection |

**Total: 10 nodes** (1 persistent + 9 preemptible)

### Why These Numbers

- **5 storage servers** — minimum for: 3-node Raft quorum (site A) + 2-node site B (replication testing) + ability to test node failure (lose 1, still have quorum)
- **2 client nodes** — separate FUSE and NFS clients ensure multi-protocol tests don't interfere
- **1 conduit** — the gRPC relay needs its own instance to simulate real cross-site latency
- **1 Jepsen controller** — Jepsen needs a separate node to inject faults (network partitions, process kills) without disrupting its own control plane

### Cost Management

- All 9 test nodes are **spot/preemptible instances** — 60-90% cheaper than on-demand
- A11 provisions the cluster on demand: `claudefs-infra up` before a test run, `claudefs-infra down` after
- Estimated cost: ~$5-10/hour when the full cluster is running, $0 when idle (only the orchestrator runs)
- CI runs nightly on the full cluster; developers trigger on-demand runs for specific test suites
- Instance types can be downgraded for basic functional testing, upgraded for performance benchmarks

### Scaling for Performance Benchmarks

For FIO benchmarks and scale testing, A11 can temporarily provision:

- 10-20 storage servers to test horizontal scaling
- Multiple client instances to simulate high concurrency
- `i4i.4xlarge` or larger for NVMe-intensive benchmarks

These scale-up clusters are short-lived (hours) and fully preemptible.

## Maximum Agent Parallelism by Phase

| Phase | Builder Agents | Cross-Cutting Agents | Total Active | Test Nodes |
|-------|---------------|---------------------|-------------|------------|
| Phase 1 | 4 (A1, A2, A3, A4) | 1 (A11) | **5** | 3 (basic cluster) |
| Phase 2 | 6 (A3, A5, A6, A7, A8 + fixes) | 3 (A9, A10, A11) | **9** | 10 (full cluster) |
| Phase 3 | 8 (all, bug fixes + features) | 3 (A9, A10, A11) | **11** | 10-20 (scale tests) |

## Shared Conventions (All Agents)

- **Error handling:** `thiserror` for library errors, `anyhow` at binary entry points
- **Serialization:** `serde` + `bincode` for internal wire format, `prost` for gRPC Protobuf
- **Async:** Tokio with io_uring backend. All I/O through io_uring submission rings.
- **Logging:** `tracing` crate with structured spans. Every operation gets a trace ID for distributed debugging.
- **Testing:** property-based tests (`proptest`) for data transforms, integration tests per crate, Jepsen-style tests for distributed correctness
- **`unsafe` budget:** confined to A1 (io_uring FFI), A4 (RDMA/libfabric FFI), A5 (FUSE FFI), A7 (Samba VFS plugin in C). All other crates are safe Rust.

## Feature Gaps: What VAST/Weka Have That ClaudeFS Must Address

The following features are standard in VAST and Weka but not yet designed. Tracked as future work, prioritized by impact:

### Priority 1 — Required for Production

| Feature | Description | Owner Agent |
|---------|-------------|-------------|
| **Multi-tenancy & quotas** | Per-user/group storage quotas, IOPS/bandwidth limits, tenant isolation | A2 + A8 |
| **QoS / traffic shaping** | Priority queues, bandwidth guarantees per workload class | A4 + A2 |
| **Online node scaling** | Add/remove nodes without downtime, automatic data rebalancing | A1 + A2 |
| **Flash layer defragmentation** | Background compaction and GC for the flash allocator | A1 |
| **Distributed tracing** | OpenTelemetry integration, per-request latency attribution | All (tracing crate) |

### Priority 2 — Required for Enterprise Adoption

| Feature | Description | Owner Agent |
|---------|-------------|-------------|
| **Compliance / WORM** | Immutable snapshots, legal holds, retention policies, audit trails | A2 + A3 |
| **Key rotation** | Rotate encryption keys without re-encrypting all data (envelope encryption) | A3 |
| **S3 API endpoint** | Expose ClaudeFS namespace as S3-compatible object storage | A7 |
| **SMB3 gateway** | Windows client access via Samba VFS plugin (GPLv3, separate package) | A7 |
| **Data migration tools** | Import from Lustre/CephFS/NFS, parallel copy, zero-downtime migration | A8 + A5 |

### Priority 3 — Competitive Differentiation

| Feature | Description | Owner Agent |
|---------|-------------|-------------|
| **Intelligent tiering** | Access-pattern learning, automatic hot/cold classification | A1 + A8 |
| **Change data capture** | Event stream API for filesystem changes (webhooks) | A2 |
| **Active-active failover** | Automatic site failover with read-write on both sites | A6 |
| **Performance SLAs** | Published latency targets (p99), scaling curves | A8 |
