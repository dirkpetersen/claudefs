# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**ClaudeFS** is a distributed, scale-out POSIX file system. The project is in early planning/requirements phase — no source code or build system exists yet.

License: MIT. Author: Dirk Petersen.

## Architecture Vision

- **Distributed flash layer** spanning multiple nodes, hosting both data and metadata
- **S3-compatible object store backend** for tiered storage — only uses GET, PUT, DELETE operations with 64MB blob chunks, written asynchronously to tolerate high-latency or unreliable stores
- **Distributed metadata servers** with asynchronous cross-site replication; eventually-consistent with last-write-wins conflict resolution and administrator alerting on write conflicts
- **Single FUSE client** with pluggable transport (RDMA or TCP); pNFS/NFS gateway for legacy access
- **Cross-site replication** designed from day one — two metadata servers syncing asynchronously
- **No single points of failure** — erasure coding, cross-site replication, end-to-end checksums
- **Zero external dependencies** — no ZooKeeper/etcd/external DB; single binary per client

## Target Platform

- **Server nodes:** kernel 6.20+ (Ubuntu 26.04, April 2026) — atomic writes, dynamic io_uring, EEVDF scheduler
- **Clients:** kernel 5.14+ (RHEL 9, Ubuntu 22.04+) — FUSE passthrough requires 6.8+; degrades gracefully on older kernels
- Supported distros: Ubuntu 24.04, Ubuntu 26.04, RHEL 9, RHEL 10
- Standard Linux deployment model (similar to Weka IO)

## Client Architecture

Single FUSE v3 client binary (`claudefs`) with pluggable network transport:

- FUSE v3 with passthrough mode (6.8+) — metadata through daemon, data I/O at native NVMe speed
- io_uring for all async I/O (disk + network)
- **RDMA transport** via `libfabric` — one-sided verbs, zero-copy (requires InfiniBand/RoCE)
- **TCP transport** via io_uring zero-copy — automatic fallback, no special hardware
- Per-core NVMe queue alignment, speculative metadata resolution
- Full POSIX by default, optional relaxation flags
- Runs on kernel 5.14+ (RHEL 9+); passthrough on 6.8+

### Access without installing ClaudeFS
- **pNFS (NFSv4.1+)** — parallel direct-to-node via standard kernel NFS client
- **NFS gateway (NFSv3)** — legacy translation proxy

## Implementation

- **Language:** Rust — compiler-enforced memory safety and data-race freedom; `unsafe` isolated to io_uring/RDMA/FUSE FFI boundaries — see [docs/language.md](docs/language.md)
- **Key crates:** `fuser` (FUSE v3), `io-uring`, `libfabric` bindings, `aws-sdk-rust` (S3)
- **Async runtime:** Tokio with io_uring backend
- **Key kernel features:** FUSE passthrough (6.8+), io_uring + NVMe passthrough, atomic writes (6.11+), kTLS, ID-mapped mounts — see [docs/kernel.md](docs/kernel.md)
- **Research foundations:** InfiniFS, Orion, Assise, MadFS, DAOS, LineFS, FLEX — see [docs/literature.md](docs/literature.md)
- **Transport layer:** Custom RPC over io_uring (core), pNFS layouts (modern clients), NFS gateway (legacy) — see [docs/transport.md](docs/transport.md)
- **Metadata service:** Distributed hash-based metadata, Raft consensus intra-site, async journal replication cross-site via cloud conduit — see [docs/metadata.md](docs/metadata.md)
- **Data reduction:** Inline dedupe -> compress -> encrypt pipeline, CAS model, CoW snapshots — see [docs/reduction.md](docs/reduction.md)
- **Hardware reference:** Solidigm FDP/QLC, AMD EPYC, NVIDIA/Broadcom/Intel NICs, Supermicro chassis — see [docs/hardware.md](docs/hardware.md)
- **AI inference:** NVIDIA Dynamo/NIXL KV cache on the storage mesh; GPUs in compute nodes, not storage — see [docs/inference.md](docs/inference.md)
- **Management:** Prometheus monitoring, Parquet/DuckDB metadata search lakehouse, Grafana dashboards — see [docs/management.md](docs/management.md)
- **POSIX validation:** pjdfstest, xfstests, fsx, LTP, Connectathon, Jepsen, FIO, CrashMonkey — see [docs/posix.md](docs/posix.md)

## Development: Parallel Agent Plan

11 agents, 3 phases, up to 11 parallel — see [docs/agents.md](docs/agents.md):
- **Builders (A1–A8):** Storage, Metadata, Reduction, Transport, FUSE, Replication, Gateways (NFS+Samba VFS+S3), Management
- **Cross-cutting (A9–A11):** Test & Validation (POSIX suites, Jepsen, CrashMonkey), Security Audit (unsafe review, fuzzing, crypto audit), Infrastructure & CI (AWS spot cluster, deployment)
- **Test cluster:** 1 orchestrator + 9 preemptible nodes (5 storage, 2 clients, 1 conduit, 1 Jepsen)

## Competitive Landscape

See [docs/market.md](docs/market.md) for detailed analysis. Key competitors: VAST Data, Weka, DAOS, BeeGFS, CephFS, JuiceFS, Lustre, GPFS. ClaudeFS targets: Weka-class performance, VAST-class economics, open-source freedom, operational simplicity.

## Feature Gaps vs VAST/Weka (Tracked)

Priority 1: multi-tenancy/quotas, QoS/traffic shaping, online node scaling, flash defrag, distributed tracing. Priority 2: WORM/compliance, key rotation, S3 API, SMB gateway, migration tools. Priority 3: intelligent tiering, CDC events, active-active failover, performance SLAs. Full breakdown in [docs/agents.md](docs/agents.md).
