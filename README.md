# ClaudeFS

A distributed, scale-out POSIX file system with a high-performance flash layer and S3-compatible object store backend.

## Overview

ClaudeFS is a distributed file system designed for research and HPC environments. It combines a low-latency flash tier spanning multiple nodes with asynchronous tiering to S3-compatible object storage. The system is implemented in Rust as a single FUSE v3 client with pluggable network transport — RDMA for maximum throughput on HPC hardware, TCP/IP for universal compatibility. Legacy clients access the cluster via pNFS or NFS gateway without installing anything.

## Architecture

### Flash Layer
- Distributed across multiple nodes, hosting both data and metadata
- Each node contributes local NVMe/SSD storage to a shared pool
- Data is distributed across nodes for performance and resilience

### Object Store Tiering
- Asynchronous writes of 64MB blob chunks to any S3-compatible store
- Operations limited to GET, PUT, and DELETE — no multipart, no listing dependencies
- Designed to tolerate high-latency, unreliable, or cloud-hosted object stores
- Data is flushed asynchronously; the flash layer absorbs write bursts

### Metadata
- Distributed metadata servers co-located with data on each node
- Strong consistency within a single site
- Asynchronous cross-site replication with eventual consistency
- Last-write-wins conflict resolution for cross-site conflicts
- Administrator alerting and reporting when write conflicts occur

### Cross-Site Replication
- Designed from day one as a core feature, not a bolt-on
- Full bidirectional replication between two sites
- Metadata synchronization is asynchronous to avoid cross-site latency on the write path
- Conflict detection with administrative notification

### Client Architecture

A single FUSE v3 client binary (`claudefs`) with pluggable network transport:

- **FUSE v3 with passthrough mode** (kernel 6.8+) — the FUSE daemon handles metadata; data I/O goes directly to local NVMe at native kernel speed
- **io_uring** for all async I/O — disk, network sends, network receives batched through the same submission ring
- **Pluggable network transport:**
  - **RDMA** via `libfabric` — one-sided verbs, zero-copy, no remote CPU involvement. For HPC clusters with InfiniBand/RoCE.
  - **TCP/IP** via io_uring zero-copy — automatic fallback when RDMA hardware is not available
- Per-core NVMe queue alignment to eliminate locking contention (MadFS pattern)
- Speculative metadata path resolution (InfiniFS pattern)
- Full POSIX semantics by default, with optional relaxation flags (`O_LAZY`, bounded staleness) for line-rate throughput
- Target: Linux 5.14+ (RHEL 9+); FUSE passthrough active on 6.8+

### Access Without Installing ClaudeFS

For clients that cannot or prefer not to install the FUSE client:

- **pNFS (NFSv4.1+)** — modern Linux clients get parallel direct-to-node data access via standard kernel NFS. No custom software required.
- **NFS gateway (NFSv3)** — legacy clients connect through a translation gateway. Full access, single-server bandwidth.

## Design Goals

1. **Performance** — Saturate NVMe and RDMA hardware. With RDMA transport, achieve line-rate throughput with single-digit microsecond metadata latency. FUSE passthrough + io_uring NVMe passthrough on the local path.

2. **Compatibility** — The FUSE client runs on any Linux 5.14+ box (RHEL 9+). Degrades gracefully on older kernels (no passthrough, still functional). pNFS and NFS gateway for clients with zero install. Full POSIX semantics out of the box. Legacy applications work unmodified.

3. **Reliability** — No single point of failure anywhere in the system. Data survives node failures (erasure coding or replication), site failures (cross-site replication), and object store outages (flash layer absorbs writes indefinitely). Automatic failure detection, rebalancing, and recovery without administrator intervention. Silent data corruption detected via end-to-end checksums.

4. **Convenience** — Single binary per client. Cluster joins via a single token or discovery URL. No external dependencies (no ZooKeeper, no etcd, no separate database for metadata). Configuration has sane defaults — a minimal deployment should require minimal tuning.

## Design Influences

- **JuiceFS** — S3 backend architecture, metadata separation
- **Weka IO** — flash-first design, standard Linux deployment, 64MB object tiering
- **CephFS** — distributed metadata, scale-out architecture
- **BeeGFS** — HPC-oriented parallel filesystem, simplicity of deployment

## Target Platform

- **Server nodes:** Linux kernel 6.20+ (ships with Ubuntu 26.04, April 2026)
- **Clients:** Linux kernel 5.14+ (RHEL 9, Ubuntu 22.04+); FUSE passthrough requires 6.8+
- Ubuntu 24.04, Ubuntu 26.04, RHEL 9, RHEL 10
- Standard Linux server hardware with NVMe/SSD storage

## License

MIT — see [LICENSE](LICENSE).
