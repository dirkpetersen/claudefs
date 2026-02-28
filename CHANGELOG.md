# ClaudeFS Changelog

All notable changes to the ClaudeFS project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Phase 1: Foundation

#### 2026-02-28 (Continuation)

##### A11: Infrastructure & CI — Fixes Applied, Critical Blocker Identified 🔴
- ✅ Added `libc 0.2` to workspace dependencies for O_DIRECT file I/O support
- ✅ Fixed all missing documentation in transport crate (error.rs, tcp.rs)
- ❌ **PROJECT BLOCKED:** Storage crate checksum.rs has 15+ compilation errors (Issue #4)
  - Tracing macro format specifiers (`%#x` invalid in tracing macros)
  - Missing PRIME1/PRIME2 constants for xxHash64
  - Invalid slice-to-u64 cast with unsafe unchecked access
  - Immutable variable reassignment (acc variable)
  - Missing serde_json dependency (tests)
  - Transport integration error (missing TcpConnection fields)
  - Status: Full workspace compilation impossible until fixed
- **Individual Crate Status:** 198 tests passing (A1: 73, A2: 83, A3: 25, A4: 17)
- **Commits:** 3 new (b5458cd, 26fc560, 57a5ce8)
- **GitHub Issues:** #4 with detailed fix guide + A11_STATUS_REPORT.md with full analysis
- **Priority:** A1 must fix checksum.rs immediately (ETA: 1-2 hours)

#### 2026-02-28 (Earlier)

##### A11: Infrastructure & CI (COMPLETE ✅)
- Cargo workspace root created with 8 agent-owned crates
- Each crate configured with shared dependencies (tokio, thiserror, serde, tracing, prost/tonic)
- Module stubs for major subsystems in each crate ready for agent implementation
- GitHub Actions CI/CD pipeline set up with per-crate testing, clippy linting, fmt checks, doc validation
- All crates compile successfully with `make check` passing (build, test, clippy, fmt, doc)
- `.gitignore` added to prevent build artifacts and temporary files from repository
- Infrastructure status: orchestrator-user-data.sh, storage-node-user-data.sh, client-node-user-data.sh, cfs-dev CLI, cfs-cost-monitor, IAM policies all complete
- PHASE1_READINESS.md created as comprehensive onboarding guide
- Development section added to README with workflow and tool documentation
- Metadata crate: Complete type definitions for Raft consensus, inode operations, replication
- Storage crate: Error types for block operations (StorageError enum with 10 variants)
- **PHASE 1 FOUNDATION: COMPLETE & READY FOR BUILDER AGENTS** ✅
  - All CI checks passing (0 errors, 0 warnings)
  - All builder agents (A1, A2, A3, A4) can begin implementation immediately
  - Infrastructure provisioned, tooling validated, documentation complete

##### A3: Data Reduction (COMPLETE ✅)
- Full `claudefs-reduce` crate Phase 1: standalone pure-Rust data reduction library
- FastCDC variable-length chunking (32KB min, 64KB avg, 512KB max) via `fastcdc` crate
- BLAKE3 content fingerprinting for exact-match CAS deduplication
- MinHash Super-Features (4 FNV-1a region hashes) for similarity detection
- LZ4 inline compression (hot write path) with compressibility heuristic check
- Zstd dictionary compression for background similarity-based delta compression
- AES-256-GCM authenticated encryption with per-chunk HKDF-SHA256 key derivation
- ChaCha20-Poly1305 fallback for hardware without AES-NI acceleration
- In-memory CAS index with reference counting for Phase 1
- Full write pipeline: chunk → dedupe → compress → encrypt → ReducedChunk
- Full read pipeline: decrypt → decompress → reassemble original data
- 25 unit + property-based tests all passing (proptest roundtrip invariants)
- Zero clippy warnings; no unsafe code (pure safe Rust per A3 spec)
- Pipeline order per docs/reduction.md: dedupe → compress → encrypt (non-negotiable)

##### A1: Storage Engine (PHASE 1 COMPLETE ✅)
- Core types: BlockId, BlockRef, BlockSize, PlacementHint with serde/Display impls
- StorageError: 8 error variants covering I/O, allocation, alignment, checksum
- Buddy block allocator: 4KB/64KB/1MB/64MB size classes, split/merge, thread-safe
- NVMe device manager: NvmeDeviceInfo, DeviceConfig, DeviceRole, DevicePool
- FDP hint tagging: PlacementHint enum (Metadata/HotData/WarmData/ColdData/Snapshot/Journal)
- IoEngine trait: async block read/write/flush/discard with Send futures
- MockIoEngine: in-memory HashMap implementation for testing
- StorageEngine<E>: unified API combining device pool + allocator + I/O engine
- ZNS zone management: ZoneManager with state transitions, append, GC candidates
- Write journal: crash-consistent coalescing per D3/D8, replication state tracking
- 73 unit tests passing, 0 clippy warnings, 0 unsafe code in allocator/engine
- Ready for integration with A2 (metadata), A3 (reduction), A4 (transport)

##### A2: Metadata Service (PHASE 1 COMPLETE ✅)
- Core types: InodeId, NodeId, ShardId, Term, LogIndex, Timestamp, VectorClock,
  MetaError, FileType, ReplicationState, InodeAttr, DirEntry, MetaOp, LogEntry,
  RaftMessage, RaftState — full serde serialization, zero unsafe code
- In-memory KV store (BTreeMap + RwLock): get, put, delete, scan_prefix,
  scan_range, contains_key, write_batch — KvStore trait for future NVMe backend (D10)
- InodeStore: atomic inode allocation, CRUD with bincode serialization
- DirectoryStore: create/delete/lookup/list entries, cross-directory rename with POSIX semantics
- Raft consensus state machine: leader election (150-300ms randomized timeout),
  log replication, RequestVote/AppendEntries, commit advancement via quorum,
  step-down on higher term — per D4 (Multi-Raft, one group per 256 virtual shards)
- MetadataJournal: append-only log with monotonic sequence numbers,
  replication tailing, batch read, compaction, lag monitoring
- ReplicationTracker: register/acknowledge remote sites, pending entries,
  compact_batch() for create+delete cancellation (AsyncFS optimization)
- MetadataService: high-level POSIX API (create_file, mkdir, lookup, getattr,
  setattr, readdir, unlink, rmdir, rename) with rollback on failure
- XattrStore: per-inode extended attributes (set, get, list, remove, remove_all)
- LockManager: per-inode read/write locks for POSIX mandatory locking (fcntl)
- 83 unit tests passing, 0 clippy warnings, 0 unsafe code
- Ready for integration with A5 (FUSE), A6 (Replication), A7 (Gateways)

##### A4: Transport (PHASE 1 COMPLETE ✅)
- Binary RPC protocol: 24-byte header (magic, version, flags, opcode, request_id, CRC32)
- 24 opcodes across 4 categories: metadata (13), data (6), cluster (5), replication (3)
- FrameFlags: COMPRESSED, ENCRYPTED, ONE_WAY, RESPONSE with bitwise ops
- CRC32 IEEE polynomial checksum for payload integrity verification
- TCP transport: async connect/listen/accept with timeout, TCP_NODELAY
- TcpConnection: concurrent send/recv via Mutex-wrapped split OwnedReadHalf/OwnedWriteHalf
- Connection pool: per-peer connection reuse with configurable max_connections_per_peer
- RPC client: request/response multiplexing with AtomicU64 IDs, oneshot response routing
- RPC server: accept loop with per-connection task spawning, dyn-compatible RpcHandler trait
- Fire-and-forget (ONE_WAY) message support
- Transport trait abstraction: async Transport, Connection, Listener traits with TCP impl
- RPC message types: serializable request/response structs for all 24 opcodes using bincode
- RpcMessage enum for typed message dispatch across all operation categories
- BufferPool: thread-safe reusable buffer pool (4KB/64KB/1MB/64MB), PooledBuffer auto-return
- RDMA transport stubs (RdmaConfig, RdmaTransport.is_available())
- 40 tests passing: protocol (14 + 4 proptest), message serialization (6), TCP (1),
  connection pool (1), RPC roundtrip (1), transport trait (5), buffer pool (6), doc-tests (0)
- Zero clippy warnings, property-based tests via proptest for frame/header/CRC32/flags
- Ready for integration with A5 (FUSE), A6 (Replication), A7 (Gateways)

### What's Next

**Phase 1 (In Progress):**
- A1: Storage Engine — io_uring NVMe passthrough, block allocator, FDP/ZNS placement
- A2: Metadata Service — Raft consensus, KV store, inode/directory operations
- A3: Data Reduction — BLAKE3 dedupe, LZ4/Zstd compression, AES-GCM encryption (as library)
- A4: Transport — RDMA + TCP backends, custom RPC protocol
- A9: Test & Validation — unit test harnesses, pjdfstest wrapper
- A10: Security Audit — unsafe code review, fuzzing, dependency audits

**Phase 2 (Planned):**
- A5: FUSE Client — wire to A2+A4
- A6: Replication — cross-site journal sync
- A7: Protocol Gateways — NFSv3, pNFS, S3, Samba VFS
- A8: Management — Prometheus, DuckDB, Web UI, CLI

**Phase 3 (Planned):**
- Bug fixes from validation findings
- Performance optimization
- Production-ready hardening

## Development Notes

### Commit Convention

All commits follow this format:
```
[AGENT] Short description

- Bullet points with details
- Link to related decisions in docs/

Co-Authored-By: Claude Model Name <noreply@anthropic.com>
```

### Agent Prefixes

- `[A1]` Storage Engine
- `[A2]` Metadata Service
- `[A3]` Data Reduction
- `[A4]` Transport
- `[A5]` FUSE Client
- `[A6]` Replication
- `[A7]` Protocol Gateways
- `[A8]` Management
- `[A9]` Test & Validation
- `[A10]` Security Audit
- `[A11]` Infrastructure & CI

### Architecture References

- Design decisions: `docs/decisions.md` (D1–D10)
- Agent plan: `docs/agents.md`
- Implementation guidance: `CLAUDE.md`
- Language specification: `docs/language.md`
- Kernel features: `docs/kernel.md`
- Hardware reference: `docs/hardware.md`
