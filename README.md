# ClaudeFS

A distributed, scale-out POSIX file system with a high-performance flash layer and S3-compatible object store backend.

## Overview

ClaudeFS is a distributed file system designed for research and HPC environments. It combines a low-latency flash tier spanning multiple nodes with asynchronous tiering to S3-compatible object storage. The system is implemented in Rust with two client modes: a high-performance RDMA client for maximum throughput and a universal FUSE client for broad compatibility.

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

ClaudeFS provides two client modes that share the same cluster, metadata protocol, and storage backend. Both clients are first-class — they are developed independently and optimized for different goals.

#### Performance Client (`claudefs-rdma`)
Goal: **Maximum throughput, minimum latency — no compromises.**

- `LD_PRELOAD` interception of POSIX calls at the libc level, bypassing the kernel entirely
- RDMA one-sided verbs (`READ`/`WRITE`) via `libfabric` — zero-copy, no remote CPU involvement
- Per-core NVMe queue alignment to eliminate locking contention (MadFS pattern)
- Speculative metadata path resolution over RDMA (InfiniFS pattern)
- Relaxed POSIX mount flags (`O_LAZY`, bounded staleness) for full line-rate NVMe throughput
- Target: HPC clusters and compute nodes with RDMA-capable NICs (InfiniBand, RoCE)

#### Universal Client (`claudefs-fuse`)
Goal: **Works everywhere, easy to deploy, full POSIX compatibility.**

- FUSE v3 userspace mount with passthrough mode (kernel 6.8+) for near-kernel-native data path performance
- io_uring for async I/O — high concurrency with minimal syscall overhead
- Standard TCP/IP networking — no special hardware required
- Full POSIX semantics by default, with optional relaxation flags for performance
- NFS v4.2+ kernel export as additional fallback for clients that cannot run the FUSE daemon
- Target: Linux 5.14+ (RHEL 9+); passthrough mode active on 6.8+ — workstations, VMs, cloud instances, containers

#### Shared Between Both Clients
- Same distributed metadata protocol and consistent hashing scheme
- Same S3 object store tiering and 64MB blob format
- Same cross-site replication and conflict resolution
- Same authentication and authorization model
- A cluster serves both client types simultaneously — no configuration split

## Design Goals

1. **Performance** — Saturate NVMe and RDMA hardware. The performance client should achieve line-rate throughput with single-digit microsecond metadata latency. No kernel bypass left on the table.

2. **Compatibility** — The universal client runs on any Linux 6.x box with no special hardware. Full POSIX semantics out of the box. Legacy applications work unmodified.

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
