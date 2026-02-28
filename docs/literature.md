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

Maximizes performance by treating the client's local NVMe/NVM as the primary file system tier. Uses a user-space file system and RDMA to synchronously replicate data for crash consistency. Provides an architectural blueprint for "client-side caching + RDMA replication" — local NVMe write speeds with distributed POSIX consistency.

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

## 5. Data Reduction: Similarity Deduplication and Delta Compression

Traditional exact-match block deduplication requires massive RAM for hash tables. Similarity-based approaches find *nearly identical* blocks and store only the differences, achieving higher reduction ratios with a fraction of the index memory.

### Finesse (FAST '14)

*"Finesse: Fine-Grained Feature Locality based Fast Resemblance Detection for Post-Deduplication Delta Compression"*

The closest academic blueprint to VAST Data's similarity deduplication. Explains how to extract "Super-Features" from data chunks using Rabin fingerprints, find similar (not identical) chunks via feature matching, and apply delta compression. Mathematically proves how grouping similar chunks reduces the RAM-heavy index requirements of traditional deduplication. ClaudeFS's Tier 2 async similarity pipeline is based on this approach.

### FastCDC (USENIX ATC '16)

*"FastCDC: A Fast and Efficient Content-Defined Chunking Approach for Data Deduplication"*

Industry-standard algorithm for content-defined chunking at multi-GB/s speeds. Slices data into variable-length chunks based on content boundaries using a rolling hash, producing stable chunk boundaries even when data is inserted or deleted. Prerequisite for any similarity or exact-match deduplication — fixed-size chunking fails when data shifts. Rust crate `fastcdc` provides a production implementation.

### SiLo (USENIX ATC '11)

*"SiLo: A Similarity-Locality based Near-Exact Deduplication Scheme"*

Combines similarity detection with spatial locality to reduce the deduplication index size while maintaining high detection rates. Shows how to organize the similarity index so that chunks that are likely to be similar are checked together, reducing random I/O during dedup lookups. Relevant to how ClaudeFS organizes its distributed Super-Feature index.

### Edelta (FAST '20)

*"Edelta: A Word-Enlarging Based Fast Delta Compression Approach"*

Modern delta compression designed for flash-native throughput. Traditional delta algorithms (xdelta, zdelta) are too slow for NVMe I/O rates. Edelta achieves high-speed byte-level differencing suitable for inline or near-inline delta compression. Relevant to ClaudeFS's Tier 2 similarity pipeline where reference blocks are fetched and diffed at NVMe speed.

### Speed-Dedup (MDPI 2024)

*"Speed-Dedup: A Scale-Out Deduplication Framework"*

Discusses scale-out deduplication that avoids the Write-Ahead Log performance hit. Focuses on reducing I/O amplification — critical when combining deduplication with flash, where repeated metadata lookups can kill NVMe throughput.

### Distributed Data Deduplication Survey (ACM Computing Surveys 2025)

*"Distributed Data Deduplication for Big Data: A Survey"*

The most recent comprehensive survey on how to route data between clients and nodes to maximize deduplication ratios in big data environments. Covers routing-aware chunking, cluster-level dedup coordination, and the tradeoffs between inline and post-processing approaches.

## 6. Flash Management and ZNS

### Z-LFS (USENIX ATC 2025)

*"Z-LFS: Zoned Namespace-tailored LFS"*

Cutting-edge log-structured filesystem designed specifically for ZNS NVMe drives. Solves the problem of small-zone ZNS SSDs with speculative log stream management. Demonstrates up to 33x performance gains over standard LFS implementations on ZNS. Direct blueprint for ClaudeFS's flash layer data placement when operating in ZNS mode.

### ZNSage (2025)

*"ZNSage: Hardware-Agnostic Swap and Metadata Placement on ZNS"*

Hardware-agnostic approach to metadata and swap placement on ZNS drives. Relevant to how ClaudeFS's metadata stores use ZNS zones for log-structured journaling with predictable tail latency.

### FDP vs ZNS Comparative Study (2025)

*"FDP vs. ZNS: A Comparative Study for Hyperscale Workloads"*

Argues that Flexible Data Placement (FDP) provides 90% of ZNS's efficiency gains while maintaining the standard LBA block interface. Supports ClaudeFS's decision to use FDP as the default data placement mechanism with ZNS as an optional mode.

## 7. GPU-Storage Integration

### BaM / GIDS (Solidigm/NVIDIA 2026)

*"BaM (Bulk Analogous Memory) & GIDS: GPU-to-Storage Communication"*

Breakthrough in GPU-to-storage communication via NVMe-oF without CPU involvement. Allows GPUs to fetch data directly from the NVMe mesh. If ClaudeFS serves AI/ML training workloads, the transport layer must be compatible with BaM/GIDS memory pointers for GPUDirect Storage.

## 8. Kernel and Infrastructure

### Linux Storage Stack Diagram 6.20

Werner Fischer (Thomas-Krenn). Canonical visualization of how io_uring, NVMe-oF, FDP, and VFS interact in modern Linux kernels. Essential reference for understanding where ClaudeFS interfaces with the kernel.

### NFS LOCALIO (kernel 6.12+)

Bypasses the network stack when NFS client and server reside on the same host. Provides extreme performance for containerized deployments where the ClaudeFS NFS gateway is co-located with the application. Relevant to Kubernetes and container-orchestrated HPC.

### Rust Beyond-Refs / Field Projections (Rust Blog 2026)

Latest research into field projections and pinning in Rust. Managing memory-mapped buffers and zero-copy networking requires sophisticated use of `Pin` and `unsafe`. This project tracks the safest patterns for these operations — directly relevant to ClaudeFS's io_uring and RDMA buffer management.

## Architectural Takeaways for ClaudeFS

These papers collectively suggest the following design principles:

1. **User-space first** — Implement the client in user-space. FUSE v3 with passthrough mode (kernel 6.8+) provides native-speed data I/O while keeping the distributed logic in user-space. The `LD_PRELOAD` approach advocated by Assise and MadFS is no longer necessary — FUSE passthrough closes the local I/O performance gap that motivated libc interception.

2. **Use `libfabric` or DPDK** — Do not use sockets. Use RDMA one-sided verbs (`RDMA READ` / `RDMA WRITE`) to pull data directly from remote NVMe memory buffers (using NVMe Controller Memory Buffer - CMB).

3. **Hash-based distributed metadata** — Do not use a single metadata server. Distribute inodes across the cluster using consistent hashing. See InfiniFS for speculative path resolution over distributed metadata.

4. **Relax POSIX where possible** — Strict POSIX requires updating `mtime` on every write, creating massive network chatter over RDMA. Provide mount flags to relax consistency (e.g., `O_LAZY` or bounded staleness) to achieve full line-rate NVMe throughput.
