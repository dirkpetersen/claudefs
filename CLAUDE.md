# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**ClaudeFS** is a distributed, scale-out POSIX file system. The project is in early planning/requirements phase — no source code or build system exists yet.

License: MIT. Author: Dirk Petersen.

## Architecture Vision

- **Distributed flash layer** spanning multiple nodes, hosting both data and metadata
- **S3-compatible object store backend** for tiered storage — only uses GET, PUT, DELETE operations with 64MB blob chunks, written asynchronously to tolerate high-latency or unreliable stores
- **Distributed metadata servers** with asynchronous cross-site replication; eventually-consistent with last-write-wins conflict resolution and administrator alerting on write conflicts
- **Mounting**: FUSE v3 user-space mount preferred; kernel NFS v4.2+ as alternative
- **Cross-site replication** designed from day one — two metadata servers syncing asynchronously

## Target Platform

- Linux kernel 6+ only
- Ubuntu 24.04, Ubuntu 26.04, Red Hat 10
- Standard Linux deployment model (similar to Weka IO)

## Implementation

- **Language:** Rust
- **FUSE bindings:** `fuser` crate (FUSE v3)
- **Async runtime:** Tokio with io_uring backend
- **Key kernel features:** FUSE passthrough (6.9+), io_uring, kTLS, ID-mapped mounts — see [docs/kernel.md](docs/kernel.md)

## Reference Systems

Design draws from: JuiceFS, CephFS, Weka IO, BeeGFS.
