# Linux Kernel Features Relevant to ClaudeFS

ClaudeFS targets Linux kernel 6.x exclusively. This document catalogs kernel features that directly inform the architecture and implementation.

## FUSE Passthrough Mode (kernel 6.9+)

The single most impactful feature for ClaudeFS. Historically, FUSE filesystems suffer a performance penalty because every I/O request bounces between kernel and userspace. `FUSE_PASSTHROUGH` allows the kernel to read/write directly to a backing file descriptor, bypassing the userspace daemon for data I/O on cached files.

This closes much of the performance gap between FUSE and kernel-native filesystems and makes the "FUSE v3 in userspace" approach viable for high-performance workloads.

**Impact on ClaudeFS:** Enables a userspace Rust implementation without sacrificing data-path performance. The FUSE daemon handles metadata and control operations; bulk data flows through passthrough to the local flash layer.

## io_uring (matured in 6.x)

Async I/O framework that replaces both `epoll` and Linux AIO. Applicable to multiple ClaudeFS subsystems:

- **FUSE daemon I/O handling** — process many concurrent requests with minimal syscall overhead via submission/completion ring buffers
- **Async S3 writes** — non-blocking 64MB blob flushes to the object store
- **Node-to-node data transfer** — high-throughput inter-node communication
- **`io_uring_cmd` (kernel 6.0+)** — custom passthrough commands, potentially useful for direct NVMe access on the flash layer without going through the filesystem stack

## io_uring Zero-Copy Networking (kernel 6.0+)

`IORING_OP_SEND_ZC` enables zero-copy send for network operations. Directly applicable to node-to-node transfer of 64MB data blobs, avoiding `memcpy` on the hot data path.

## ID-Mapped Mounts (kernel 5.12+, matured in 6.x)

Remaps UIDs/GIDs at mount time without changing on-disk data. Relevant for:

- **Container environments** where different nodes have different UID mappings
- **Multi-tenant deployments** with per-tenant UID namespaces
- **Cross-site replication** where UID spaces may differ between sites

## Kernel TLS (kTLS)

Offloads TLS encryption/decryption to the kernel, and potentially to NIC hardware. Useful for:

- **Node-to-node encryption** without userspace overhead
- **S3 HTTPS connections** with reduced CPU cost on the data path

## BPF / eBPF Enhancements in 6.x

Several BPF capabilities are directly applicable:

- **fuse-bpf (experimental)** — BPF programs intercept FUSE requests, enabling kernel-side decisions (e.g., "serve from local cache") without bouncing to userspace. Could complement passthrough mode for metadata caching.
- **BPF struct_ops / sched_ext (kernel 6.12)** — custom CPU schedulers that can prioritize metadata operations or I/O completion threads
- **BPF iterators** — custom `/proc`-style observability for filesystem state, useful for monitoring and debugging distributed cache behavior

## NFS v4.2 Features

If ClaudeFS exports via kernel NFS (the alternative to FUSE mounting), these v4.2 features are available:

- **Server-side copy** (`copy_file_range`) — copy between files without moving data through the client
- **Sparse file support** (`SEEK_HOLE`/`SEEK_DATA`) — efficient handling of sparse files, relevant for scientific workloads
- **Space reservations** (`fallocate`) — preallocate space to reduce fragmentation
- **Labeled NFS** — SELinux label propagation for security-sensitive deployments

## Folios (ongoing since 5.16, maturing in 6.x)

The kernel's migration from `struct page` to `struct folio` improves memory management for filesystems — better large-page (huge page) support and reduced per-page overhead. FUSE filesystems benefit from this through the page cache. Understanding folio behavior matters for tuning cache performance, especially for the flash layer.

## Multi-Path TCP (MPTCP, stable in 6.x)

Uses multiple network links simultaneously for a single TCP connection. Applicable for:

- **Redundant network paths** between storage nodes for fault tolerance
- **Bandwidth aggregation** across multiple NICs without application-level changes
- **Transparent failover** when a network link goes down

## CXL — Compute Express Link (kernel 6.x)

Enables shared memory pools across nodes over PCIe. Still early-stage hardware adoption, but on CXL-capable systems could enable:

- **Shared metadata caches** across nodes without network round-trips
- **Disaggregated memory** for large metadata indexes that exceed a single node's DRAM

Worth tracking for future optimization, not a day-one dependency.

## Summary

The combination of **FUSE passthrough + io_uring** is the key enabler: it allows a userspace Rust implementation to approach kernel-native performance while retaining memory safety and development velocity. The remaining features (kTLS, eBPF, MPTCP, ID-mapped mounts) provide optimization and operational capabilities that can be adopted incrementally.
