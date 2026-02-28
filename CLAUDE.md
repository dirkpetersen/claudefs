# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**ClaudeFS** is a distributed, scale-out POSIX file system. The project is in early planning/requirements phase — no source code or build system exists yet.

License: MIT. Author: Dirk Petersen.

## Architecture Vision

- **Distributed flash layer** spanning multiple nodes, hosting both data and metadata
- **S3-compatible object store backend** for tiered storage — only uses GET, PUT, DELETE operations with 64MB blob chunks, written asynchronously to tolerate high-latency or unreliable stores
- **Distributed metadata servers** with asynchronous cross-site replication; eventually-consistent with last-write-wins conflict resolution and administrator alerting on write conflicts
- **Two client modes** developed independently (see below)
- **Cross-site replication** designed from day one — two metadata servers syncing asynchronously
- **No single points of failure** — erasure coding, cross-site replication, end-to-end checksums
- **Zero external dependencies** — no ZooKeeper/etcd/external DB; single binary per client

## Target Platform

- Linux kernel 6+ only
- Ubuntu 24.04, Ubuntu 26.04, Red Hat 10
- Standard Linux deployment model (similar to Weka IO)

## Dual-Client Architecture

Both clients share the same cluster, metadata protocol, storage backend, and replication. They are developed as independent workstreams.

### Performance Client (`claudefs-rdma`)
- `LD_PRELOAD` libc interception — bypasses kernel entirely
- RDMA one-sided verbs via `libfabric` — zero-copy, no remote CPU
- Per-core NVMe queue alignment, speculative metadata resolution
- Relaxed POSIX mount flags for line-rate throughput
- Requires RDMA NICs (InfiniBand, RoCE)

### Universal Client (`claudefs-fuse`)
- FUSE v3 with passthrough mode (kernel 6.8+)
- io_uring async I/O, standard TCP/IP networking
- Full POSIX by default, optional relaxation flags
- NFS v4.2+ kernel export as additional fallback
- Runs on any Linux 6.x system

## Implementation

- **Language:** Rust
- **Key crates:** `fuser` (FUSE v3), `io-uring`, `libfabric` bindings, `aws-sdk-rust` (S3)
- **Async runtime:** Tokio with io_uring backend
- **Key kernel features:** FUSE passthrough (6.8+), io_uring + NVMe passthrough, atomic writes (6.11+), kTLS, ID-mapped mounts — see [docs/kernel.md](docs/kernel.md)
- **Research foundations:** InfiniFS, Orion, Assise, MadFS, DAOS, LineFS, FLEX — see [docs/literature.md](docs/literature.md)
- **POSIX validation:** pjdfstest, xfstests, fsx, LTP, Connectathon, Jepsen, FIO, CrashMonkey — see [docs/posix.md](docs/posix.md)

## Reference Systems

Design draws from: JuiceFS, CephFS, Weka IO, BeeGFS, DAOS.
