# Linux Kernel Features Relevant to ClaudeFS

The traditional POSIX I/O path (VFS -> Page Cache -> Block Layer -> NVMe Driver) is too slow for modern NVMe and RDMA. Kernel developers have introduced groundbreaking features to bypass these bottlenecks, support disaggregated storage, and enable high-performance user-space file systems.

## Kernel Version Requirements

ClaudeFS has different kernel requirements for servers and clients:

| Role | Minimum Kernel | Rationale | Ships With |
|------|---------------|-----------|------------|
| **Server nodes** | 6.20+ | Atomic writes stabilized, io_uring dynamic resize, EEVDF scheduler, Rust VFS, MGLRU refinements | Ubuntu 26.04 (Apr 2026) |
| **Client (claudefs)** | 5.14+ | FUSE v3 basic support; passthrough mode requires 6.8+ | RHEL 9, Ubuntu 22.04+ |

The client degrades gracefully: on pre-6.8 kernels, FUSE operates without passthrough (slower but functional). Network transport (RDMA or TCP) is selected at runtime based on available hardware. Servers require 6.20+ to take full advantage of atomic writes, dynamic io_uring, and scheduler improvements.

## 1. The Async I/O Revolution: io_uring & NVMe Passthrough

`io_uring` is the most significant addition to Linux storage in a decade — a lockless, ring-buffer-based async I/O interface that eliminates syscall overhead.

### Raw NVMe Passthrough via io_uring (kernel 5.19+)

Using `IORING_OP_URING_CMD`, user-space applications can send raw NVMe commands directly to the NVMe driver, completely bypassing the Linux block layer and VFS.

**Impact on ClaudeFS:** The ClaudeFS client uses `io_uring` passthrough to pull data off local NVMe at full hardware speed with zero kernel translation overhead, then pipes it into RDMA queues (or TCP via io_uring zero-copy). This is the local storage plane.

### io_uring Async I/O for the FUSE Daemon

Beyond NVMe passthrough, `io_uring` is the async engine for the universal client:

- **FUSE daemon I/O handling** — process concurrent requests with minimal syscall overhead via submission/completion ring buffers
- **Async S3 writes** — non-blocking 64MB blob flushes to the object store
- **Node-to-node data transfer** — high-throughput inter-node communication

### Zero-Copy Network Transmit (kernel 6.0+)

`io_uring` supports zero-copy network sends (`IORING_OP_SEND_ZC`, `IORING_RECVSEND_FIXED_BUF`). For TCP/IP fallback (e.g., NVMe/TCP instead of RDMA/RoCE), data can DMA straight from NVMe to network card without CPU memory copies. Directly applicable to node-to-node transfer of 64MB data blobs.

## 2. High-Performance User-Space POSIX: FUSE Passthrough

Running distributed FS logic in the kernel is discouraged today. The modern approach is a user-space daemon mounted via FUSE. Historically FUSE was too slow for NVMe — every `read`/`write` required multiple context switches.

### FUSE Passthrough (kernel 6.8+)

Allows the FUSE daemon to tell the kernel: "For this file, stop asking me for data. Route all reads and writes directly to this backing file." The daemon handles complex distributed POSIX metadata over RDMA (`open`, `chmod`, `rename`), but once the file is opened, the data path goes straight to local NVMe at native kernel speed, completely bypassing the FUSE daemon.

**Impact on ClaudeFS:** Enables the FUSE client to achieve near-native performance. The FUSE daemon handles metadata and control; bulk data flows through passthrough to the local flash layer. This eliminates the need for `LD_PRELOAD` libc interception — FUSE passthrough closes the local I/O performance gap.

## 3. Page Cache: The Folio API (kernel 5.15+, enforced in 6.x)

The largest internal memory management rewrite in Linux history. The page cache historically managed memory in 4KB `struct page` chunks, causing massive fragmentation and CPU overhead at multi-GB/s NVMe throughput. The kernel is migrating to `struct folio`, which handles multi-page (large/transparent huge pages) natively.

**What changed:** The old `readpage`/`writepage` file system APIs are deprecated. Modern file systems must implement `read_folio` and `writepages` to push large, contiguous I/O blocks down to the NVMe queue. This is relevant if ClaudeFS ever implements a kernel module, and affects page cache tuning for both FUSE clients.

## 4. Advanced NVMe Hardware Capabilities

### Atomic / Untorn Writes (kernel 6.11+)

Allows user-space applications and file systems to issue large writes (16KB, 64KB) and rely on NVMe hardware to guarantee atomicity. If power is lost, either all of the write lands or none of it does.

**Impact on ClaudeFS:** Enables crash-consistent POSIX metadata journaling without the performance penalty of traditional software write-ahead logging (WAL) or double-writes. The metadata servers can write journal entries atomically to NVMe without a separate WAL flush.

### Zoned Namespace — ZNS (kernel 5.9+)

Native block-layer support for ZNS NVMe drives, which require sequential, append-only writes. This eliminates the SSD's internal garbage collection overhead, reducing tail-latency spikes.

**Impact on ClaudeFS:** For absolute lowest tail-latency on the flash layer (crucial for RDMA paths where microseconds matter), the local storage engine can be designed as a log-structured store using ZNS drives. Particularly relevant for metadata servers where consistent low latency is more important than raw throughput.

## 5. Local Storage Engine: Why io_uring + ZNS Instead of a Kernel Filesystem

A scale-out POSIX filesystem must decide how to manage local NVMe on each node. There are three architectural paths, and the choice determines the performance ceiling.

### Path A: Layer on an Existing Flash Filesystem (F2FS, XFS)

Use F2FS or XFS as the local storage engine, then build scale-out logic (distribution, metadata, erasure coding) on top.

**Pros:**
- Built-in flash management — no need to write your own wear leveling or garbage collection
- Kernel maturity — F2FS has multi-stream write support and atomic operations
- Faster development — focus on the scale-out problem, not the local disk problem

**Cons:**
- **The "Double GC" problem** — if the scale-out layer reshuffles data and the underlying filesystem also runs garbage collection, you get massive write amplification and latency spikes
- Metadata bottleneck — local filesystems are optimized for single-node POSIX, which can bottleneck a distributed metadata engine
- VFS overhead on every operation, even when you only need raw block access

### Path B: Raw NVMe (SPDK / Kernel Bypass)

Bypass the filesystem layer entirely using SPDK or raw block devices for zero-copy I/O.

**Pros:**
- Maximum throughput, minimum latency — no VFS, no page cache, no local FS buffer
- End-to-end data placement control — align scale-out stripes directly with NAND geometry
- Predictable tail latency — you control exactly when garbage collection happens across the mesh

**Cons:**
- Massive complexity — you own bit-rot protection, wear leveling, bad block management, and POSIX translation
- SPDK is a full kernel bypass that takes exclusive ownership of the NVMe device, making debugging and coexistence with other tools difficult

### Path C: io_uring + ZNS (The Modern Middle Ground)

This is the ClaudeFS approach. On Linux 6.20+, `io_uring` with `IORING_OP_URING_CMD` sends raw NVMe commands directly to the driver from user-space, bypassing the block layer and VFS — but without taking exclusive device ownership like SPDK. Combined with ZNS drives, this gives you:

- **SPDK-class performance** — batch thousands of NVMe commands without context-switching into the kernel
- **User-space development** — no kernel modules to maintain, standard debugging tools work
- **ZNS data placement** — the drive handles NAND physics (wear leveling, bad blocks) while you control data layout via sequential append-only zones, eliminating the Double GC problem
- **Coexistence** — the device is still visible to the kernel for monitoring, diagnostics, and FUSE passthrough

This is the same direction taken by modern Ceph (Crimson/Seastore) and ScyllaDB — user-space engines that talk directly to NVMe via io_uring rather than going through a local filesystem.

### Comparison

| | Layering (F2FS/XFS) | Raw NVMe (SPDK) | io_uring + ZNS |
|---|---|---|---|
| **Complexity** | Moderate | Extreme | Moderate-High |
| **Latency** | Millisecond-level jitter | Microsecond-level | Microsecond-level |
| **POSIX compliance** | Inherited from base FS | Manual | Manual |
| **Hardware control** | Abstracted | Exclusive | Granular (shared) |
| **Debugging** | Standard tools | Difficult (kernel bypass) | Standard tools |
| **GC control** | Double GC risk | Full control | ZNS eliminates GC |

**ClaudeFS decision:** Path C. Use `io_uring` NVMe passthrough for the data engine, with ZNS for metadata stores where tail-latency consistency matters most. This keeps development in user-space Rust while achieving hardware-saturating performance.

## 6. Memory Disaggregation: CXL (kernel 6.0-6.8+)

Compute Express Link allows CPUs to access memory and storage on separate physical servers via a PCIe-like fabric with cache-coherent latency (nanoseconds, vs. RDMA's microseconds). The kernel now treats CXL memory as a distinct NUMA node.

**Impact on ClaudeFS:** On CXL-capable hardware, POSIX metadata (inodes, distributed locks) can reside in CXL-attached memory pools accessible across nodes at near-local-memory speed, while bulk file data stays on NVMe-over-RDMA. Not a day-one dependency but worth designing metadata structures to be CXL-ready.

## 7. ID-Mapped Mounts (kernel 5.12+, matured in 6.x)

Remaps UIDs/GIDs at mount time without changing on-disk data. Relevant for:

- **Container environments** where different nodes have different UID mappings
- **Multi-tenant deployments** with per-tenant UID namespaces
- **Cross-site replication** where UID spaces may differ between sites

## 8. Kernel TLS (kTLS)

Offloads TLS encryption/decryption to the kernel, and potentially to NIC hardware. Useful for:

- **Node-to-node encryption** without userspace overhead
- **S3 HTTPS connections** with reduced CPU cost on the data path

## 9. BPF / eBPF Enhancements in 6.x

- **fuse-bpf (experimental)** — BPF programs intercept FUSE requests, enabling kernel-side decisions (e.g., "serve from local cache") without bouncing to userspace. Could complement passthrough mode for metadata caching.
- **BPF struct_ops / sched_ext (kernel 6.12)** — custom CPU schedulers that can prioritize metadata operations or I/O completion threads
- **BPF iterators** — custom `/proc`-style observability for filesystem state, useful for monitoring and debugging distributed cache behavior

## 10. NFS v4.2 Features

If ClaudeFS exports via kernel NFS (the fallback for the universal client), these v4.2 features are available:

- **Server-side copy** (`copy_file_range`) — copy between files without moving data through the client
- **Sparse file support** (`SEEK_HOLE`/`SEEK_DATA`) — efficient handling of sparse files, relevant for scientific workloads
- **Space reservations** (`fallocate`) — preallocate space to reduce fragmentation
- **Labeled NFS** — SELinux label propagation for security-sensitive deployments

## 11. Multi-Path TCP (MPTCP, stable in 6.x)

Uses multiple network links simultaneously for a single TCP connection. Applicable for:

- **Redundant network paths** between storage nodes for fault tolerance
- **Bandwidth aggregation** across multiple NICs without application-level changes
- **Transparent failover** when a network link goes down

## 12. The 6.12-6.20 Era: Server-Side Performance

The jump from kernel 6.11 to 6.20 represents a significant era for server-side performance. Key themes: stabilization of atomic writes, massive expansion of Rust infrastructure, and lock-contention reductions in major file systems.

### Storage & File System Enhancements

- **Atomic write stabilization (6.11–6.13)** — XFS and Ext4 mainlined atomic write support, guaranteeing block-level write atomicity. For ClaudeFS metadata servers, this eliminates double-buffer writes and simplifies crash-consistent journaling. This is why 6.20 is the server minimum.
- **XFS large block support** — XFS now supports block sizes larger than the system page size (e.g., 16KB blocks on a 4KB page system), reducing metadata overhead for large-scale storage arrays. Relevant to how ClaudeFS structures its local flash allocator.
- **io_uring dynamic resizing (6.13)** — `IORING_REGISTER_RING_RESIZE` allows servers to scale I/O submission queues on the fly without restarting. Critical for maintaining performance under variable load — ClaudeFS servers should resize rings as client connections scale up/down.

### Scheduler & Memory Management

- **EEVDF scheduler (6.12–6.13)** — The Earliest Eligible Virtual Deadline First scheduler replaced CFS, with "Lazy Preemption" reducing unnecessary context switching in CPU-bound server tasks. Benefits ClaudeFS metadata servers handling high-concurrency POSIX operations.
- **Multi-Gen LRU refinements (6.15+)** — Page reclamation under heavy memory pressure is now much smarter, preventing thrashing during peak traffic. Directly benefits ClaudeFS servers caching hot metadata and flash-tier data in memory.

### Networking

- **Adaptive polling (6.13)** — Automatically switches between interrupts and polling, boosting throughput up to 45% in benchmarks. Benefits the universal client's TCP path and server-to-server communication.
- **Per-namespace RTNL locks (6.13)** — Moves the RTNL lock to per-namespace, drastically reducing contention in container-heavy (Kubernetes) environments. Important for ClaudeFS deployments in containerized HPC clusters.
- **Device Memory TCP (6.12)** — Zero-copy data transfer directly to/from hardware buffers, bypassing the CPU. Complements io_uring zero-copy for the universal client's TCP data path.

### Rust in the Kernel

- **Rust VFS abstractions (6.13+)** — The Virtual File System layer began receiving Rust abstractions, aimed at reducing memory-safety bugs that cause I/O path hangs. Validates ClaudeFS's choice of Rust for the user-space implementation and opens the door for potential kernel-side components in Rust if ever needed.

## Tech Stack Summary

Based on these kernel features, the ClaudeFS architecture maps to four planes:

1. **Control Plane (Metadata):** User-space daemon in Rust. Handles POSIX metadata, distributed coordination, replication. Atomic NVMe writes (6.11+) for crash-consistent journaling.
2. **Network Plane:** Pluggable transport — RDMA one-sided verbs (`libfabric`) when hardware is available, TCP/IP with io_uring zero-copy as automatic fallback. pNFS layouts and NFS gateway for clients without ClaudeFS installed.
3. **Local Storage Plane:** Raw NVMe via `io_uring` passthrough (`IORING_OP_URING_CMD`). Optional ZNS for log-structured metadata stores.
4. **POSIX Mount:** FUSE v3 with passthrough (6.8+) for native-speed data I/O. Degrades gracefully on older kernels.
