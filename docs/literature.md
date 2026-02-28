# Research Literature

When NVMe (microsecond storage latency, GB/s throughput) is combined with RDMA (1-2us network latency, zero-copy CPU bypass), hardware is no longer the bottleneck. In a scale-out distributed environment, the bottleneck becomes the POSIX standard itself — hierarchical path resolution, distributed locking, and strict metadata consistency.

To design a scale-out POSIX file system over NVMe and RDMA, you must decouple the data path from the metadata path, utilize user-space networking (DPDK, `libfabric`), and use RDMA one-sided verbs (READ/WRITE) to bypass remote CPUs entirely.

## 1. Scaling POSIX Metadata over RDMA

Opening a file (`/user/data/logs/file.txt`) requires traversing the directory tree and checking permissions at every level. Over a network, this causes massive latency. Modern designs use RDMA to fetch directory data without waking the remote server's CPU.

### InfiniFS (FAST '22)

*"InfiniFS: An Efficient Metadata Service for Large-Scale Distributed Filesystems"*

Solves the distributed POSIX path-resolution bottleneck by decoupling directory tree structure from access control, caching metadata aggressively, and using speculative path resolution. Shows how to distribute POSIX metadata across a cluster using RDMA-friendly data structures — essential for supporting billions of small files with low latency.

### Orion (FAST '19)

*"Orion: A Distributed File System for Non-Volatile Main Memories and RDMA-Capable Networks"*

Built from the ground up for RDMA. Uses one-sided READ/WRITE operations so clients directly access metadata and data on remote storage nodes without remote CPU involvement. Details how to structure inodes and file system pointers so an RDMA client can traverse a remote POSIX directory tree directly in memory.

## 2. High-Throughput Data Paths (Bypassing the OS)

To get full NVMe throughput across a network, the file system must bypass the Linux VFS, the block layer, and the standard TCP/IP network stack.

### Assise (OSDI '20)

*"Assise: Performance and Availability via Client-local NVM in a Distributed File System"*

Maximizes performance by treating the client's local NVMe/NVM as the primary file system tier. Uses a custom user-space file system (via `LD_PRELOAD` or FUSE) and RDMA to synchronously replicate data for crash consistency. Provides an architectural blueprint for "client-side caching + RDMA replication" — local NVMe write speeds with distributed POSIX consistency.

### MadFS (SC '21)

*"MadFS: A Per-Core Burst Buffer over NVMe and RDMA"*

Built for HPC, completely removes centralized metadata servers from the data path. Maps file blocks directly to NVMe SSDs across the cluster and uses RDMA for per-core line-rate throughput. Demonstrates how to achieve ultra-high throughput by aligning network threads, RDMA queues, and NVMe hardware queues to individual CPU cores, eliminating locking contention.

## 3. Production Paradigms

The industry is moving toward POSIX overlays on top of high-performance object stores that natively speak NVMe and RDMA.

### DAOS (SC '20 / FAST '24)

*"DAOS: A Scale-Out High Performance Routing and Storage Architecture"*

The fastest open-source distributed storage engine (Intel, Argonne National Lab). Uses NVMe over Fabrics (NVMe-oF) and RDMA. Does not provide POSIX natively — instead provides an ultra-fast key-value/object layer with a POSIX namespace overlay (`dfuse` or `libdfs`) in user-space. DAOS is ClaudeFS's main competitor and validates the pattern of separating the NVMe/RDMA I/O engine from the POSIX compatibility layer.

## 4. Storage Disaggregation (Compute vs. Storage Nodes)

Modern cloud infrastructure separates compute from storage. The file system must handle NVMe drives attached to entirely different machines across the data center.

### LineFS (SOSP '21)

*"LineFS: SmartNIC Offloaded File System"*

Moves distributed file system logic off the host CPU onto a SmartNIC/DPU. The SmartNIC uses RDMA for inter-node communication and handles NVMe storage natively. Shows how to push distributed FS operations (tail latency management, journaling, POSIX replication) directly into the network card — the future of scale-out storage.

### FLEX (OSDI '24)

*"FLEX: A High-Performance and Highly Flexible File System for Disaggregated Memory/Storage"*

Designed for environments where memory and NVMe storage are decoupled over RDMA (like CXL). Dynamically shifts where file system tasks execute based on network load. Represents the cutting edge of POSIX semantics over high-speed RDMA fabrics.

## Architectural Takeaways for ClaudeFS

These papers collectively suggest the following design principles:

1. **User-space first** — Implement the client in user-space. Use `LD_PRELOAD` to intercept POSIX calls (`open`, `read`, `write`) at the C-library level, bypassing the kernel entirely. FUSE is the fallback when `LD_PRELOAD` interception is not feasible.

2. **Use `libfabric` or DPDK** — Do not use sockets. Use RDMA one-sided verbs (`RDMA READ` / `RDMA WRITE`) to pull data directly from remote NVMe memory buffers (using NVMe Controller Memory Buffer - CMB).

3. **Hash-based distributed metadata** — Do not use a single metadata server. Distribute inodes across the cluster using consistent hashing. See InfiniFS for speculative path resolution over distributed metadata.

4. **Relax POSIX where possible** — Strict POSIX requires updating `mtime` on every write, creating massive network chatter over RDMA. Provide mount flags to relax consistency (e.g., `O_LAZY` or bounded staleness) to achieve full line-rate NVMe throughput.
