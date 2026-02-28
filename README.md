# ClaudeFS

A distributed, scale-out POSIX file system with a high-performance flash layer and S3-compatible object store backend.

## Overview

ClaudeFS is a distributed file system designed for research and HPC environments. It combines a low-latency flash tier spanning multiple nodes with asynchronous tiering to S3-compatible object storage. The system is implemented as a userspace FUSE v3 filesystem in Rust, leveraging Linux 6.x kernel features (FUSE passthrough, io_uring) to achieve near-kernel-native performance.

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

### Mounting
- **Primary:** FUSE v3 userspace mount with passthrough mode (kernel 6.9+)
- **Alternative:** NFS v4.2+ kernel export for clients that cannot run the FUSE daemon

## Design Influences

- **JuiceFS** — S3 backend architecture, metadata separation
- **Weka IO** — flash-first design, standard Linux deployment, 64MB object tiering
- **CephFS** — distributed metadata, scale-out architecture
- **BeeGFS** — HPC-oriented parallel filesystem, simplicity of deployment

## Target Platform

- Linux kernel 6.0+ (6.9+ recommended for FUSE passthrough)
- Ubuntu 24.04, Ubuntu 26.04, Red Hat 10
- Standard Linux server hardware with NVMe/SSD storage

## License

MIT — see [LICENSE](LICENSE).
