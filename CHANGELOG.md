# ClaudeFS Changelog

All notable changes to the ClaudeFS project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Phase 1: Foundation

#### 2026-03-01 (Session 4 - Latest)

##### A1: Storage Engine — Phase 1+ Hardening (172 tests ✅)

**New modules and fixes:**
- ✅ **Fixed buddy allocator merge bug**: Replaced broken XOR-based buddy pairing with correct
  N-ary child group merge (16:1 for 4KB→64KB/64KB→1MB, 64:1 for 1MB→64MB). The previous
  merge_buddies used XOR which only works for binary (2:1) splits, causing free_blocks_4k to
  exceed total_blocks_4k after alloc/free cycles. Proptest caught this invariant violation.
- ✅ **UringIoEngine**: Real io_uring-based NVMe I/O engine behind `uring` feature gate.
  O_DIRECT for NVMe passthrough, configurable queue depth, IOPOLL/SQPOLL options,
  CString path handling, proper Fd type wrapping, spawn_blocking async bridge.
- ✅ **Flash defragmentation module**: DefragEngine with fragmentation analysis per size class,
  DefragPlan generation with relocation suggestions, cooldown-based scheduling, statistics.
- ✅ **Proptest property-based tests**: 16 tests covering allocator invariants (total_blocks ==
  free + allocated), unique offsets, in-bounds offsets, checksum determinism, segment packer
  roundtrip, BlockHeader serialization, BlockSize/PlacementHint/SegmentEntry serialization.
- ✅ Workspace Cargo.toml updated with io-uring and proptest workspace deps
- ✅ Storage Cargo.toml uses workspace deps, adds `uring` feature gate, proptest dev-dep
- ✅ 172 tests passing (156 unit + 16 proptest), 0 clippy warnings

**Commits:**
- 485dbe0: Fix buddy allocator merge bug, add io_uring engine, defrag, and proptest
- f3ead30: Add doc comments to uring_engine.rs, fix clippy warnings

##### A11: Infrastructure & CI — All Tests Passing, CI Ready ✅

**Test Summary (by crate):**
- ✅ A1 Storage: **172 tests passing** (100%) — 156 unit + 16 proptest
- ✅ A2 Metadata: **233 tests passing** (100%) - includes new FileHandleManager tests
- ✅ A3 Reduce: **25 tests passing** (100%)
- ✅ A4 Transport: **49 tests passing** (100%) - TLS tests fixed
- ✅ **TOTAL: 479 tests passing, 0 failures, 0 clippy warnings**

**Work Completed:**
- ✅ Completed FileHandleManager implementation for A2 metadata crate (via OpenCode)
  - FileHandle struct: fh, ino, client, flags, opened_at (full serde support)
  - FileHandleManager: thread-safe with RwLock + AtomicU64 for unique IDs
  - 10 unit tests passing: open/close, get, is_open, is_open_for_write, handles_for_*, close_all_for_client, open_count
- ✅ Fixed remaining clippy errors blocking full workspace pass
  - Removed unused imports from defrag.rs test module (AllocatorConfig, BlockId)
  - Fixed absurd u64 >= 0 comparison in defrag.rs (always true, removed assertion)
  - Fixed unused variable in pathres.rs test (_parent callback parameter)
  - Added #[allow(dead_code)] to create_test_attr in readindex.rs
- ✅ All 8 crates now pass `cargo clippy --all-targets -- -D warnings`
- ✅ All 8 crates pass `cargo test --lib` with 463 passing tests

**Build Status:** ✅ CI-READY
- Zero compilation errors
- Zero clippy warnings
- 463 tests passing across all crates
- Ready for Phase 2 (A5 FUSE, A6 Replication, A7 Gateways)

**Commits:** 1 new
- 6f70f24: Fix clippy errors and complete FileHandleManager for A2 metadata crate

#### 2026-02-28 (Session 3)

##### A11: Infrastructure & CI — Clippy Fixes & CI Issues Identified ✅

**Test Summary (by crate):**
- ✅ A1 Storage: **141 tests passing** (100%)
- ⚠️ A2 Metadata: **183 passing, 1 failing** (99.5%) - negative cache logic
- ✅ A3 Reduce: **25 tests passing** (100%)
- ⚠️ A4 Transport: **47 passing, 2 failing** (95.9%) - TLS cert validation
- ✅ A5-A8 (Stubs): 0 tests (frameworks ready)

**Work Completed:**
- ✅ Fixed all A1 (Storage) clippy errors blocking CI (Commit aeeea1c)
  - Fixed erasing_op in allocator.rs:535: Save config before moving, use saved value
  - Fixed div_ceil in superblock.rs:454: Use u64::div_ceil() instead of manual calculation
  - Fixed unused loop variable in proptest_storage.rs:83: Iterate over slice directly
  - Added #[allow(dead_code)] to unused test helpers
  - Storage crate now passes `cargo clippy --all-targets --all-features -- -D warnings` ✅

**Issues Created for Other Agents:**
- Issue #8: A2 metadata crate - clippy errors + 1 test failure in negative cache logic
- Issue #9: A4 transport - 2 TLS test failures (cert DNS validation for localhost)

**Status:** A1 storage crate CI-ready ✅, 249/251 tests passing (99.2%), A2/A4 needed fixes

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

##### A1: Storage Engine (PHASE 1+ COMPLETE ✅ — 172 tests)
- Core types: BlockId, BlockRef, BlockSize, PlacementHint with serde/Display impls
- StorageError: 10 error variants covering I/O, allocation, alignment, checksum, corruption, serialization
- Buddy block allocator: 4KB/64KB/1MB/64MB size classes, N-ary group merge, thread-safe
  - **Fixed**: merge_buddies now correctly handles 16:1 and 64:1 child-to-parent ratios
- NVMe device manager: NvmeDeviceInfo, DeviceConfig, DeviceRole, DevicePool
- IoEngine trait: async block read/write/flush/discard with Send futures
- MockIoEngine: in-memory HashMap implementation for testing
- **UringIoEngine**: Real io_uring NVMe I/O with O_DIRECT, behind `uring` feature gate
- StorageEngine<E>: unified API combining device pool + allocator + I/O engine
- ZNS zone management: ZoneManager with state transitions, append, GC candidates
- Write journal: crash-consistent coalescing per D3/D8, replication state tracking
- **Checksum module**: Pure-Rust CRC32C (Castagnoli) + xxHash64, BlockHeader with magic/version
- **Segment packer**: 2MB packed segments per D1 for EC 4+2 striping, auto-seal on overflow
- **Capacity tracker**: Watermark eviction (D5/D6) — 80% high, 60% low, 95% critical
  - Age-weighted scoring (age_secs × size_bytes), S3-confirmation check, tier overrides
- **FDP hint manager**: Maps PlacementHints to NVMe Reclaim Unit Handles, per-RUH stats
- **Superblock**: Device identity (UUIDs), layout (bitmap + data offsets), CRC32C integrity, crash recovery
- **Flash defragmentation**: DefragEngine with per-size-class analysis, relocation planning, scheduling
- 172 tests passing (156 unit + 16 proptest), 0 clippy warnings, 0 unsafe code in allocator/engine
- Ready for integration with A2 (metadata), A3 (reduction), A4 (transport), A5 (FUSE)

##### A2: Metadata Service (PHASE 2 COMPLETE — 233 tests ✅, 25 modules)

**Phase 1 (Complete):**
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

**Phase 2 (Complete):**
- ShardRouter: maps inodes to 256 virtual shards and shards to cluster nodes,
  round-robin distribution via ShardAssigner, leader tracking, node removal
- Symlink/hardlink POSIX operations: symlink(), link(), readlink() with
  symlink_target field in InodeAttr, nlink management, directory-hardlink prohibition
- MultiRaftManager: manages one RaftNode per virtual shard on this node,
  routes operations to correct shard's Raft group, per-shard election/replication
- PathResolver: speculative path resolution with (parent, name) cache,
  partial cache hits, parent invalidation, sequential fallback resolution
- **Negative caching**: "entry not found" results cached with configurable TTL,
  auto-invalidated on creates, expired entry cleanup — common build system optimization
- LeaseManager: time-limited metadata caching leases (read/write) for FUSE clients,
  lease revocation on mutations, client disconnect cleanup, lease renewal
- RaftMetadataService: unified API integrating local service, Multi-Raft, leases,
  and path cache — mutations revoke leases/invalidate cache, reads use local state
- **TransactionManager**: two-phase commit coordinator for cross-shard rename/link,
  begin/vote/commit/abort lifecycle, timeout-based cleanup for timed-out transactions
- **SnapshotManager**: Raft log snapshot and compaction, configurable thresholds,
  compaction point calculation, snapshot restore for follower catch-up
- **QuotaManager**: per-user/group storage quotas (Priority 1 feature gap),
  byte and inode limits, usage tracking, enforcement via check_quota(), over-quota detection
- **ConflictDetector**: vector clock conflict detection for cross-site replication,
  Last-Write-Wins resolution (sequence first, site_id tiebreaker), concurrent
  modification detection, conflict event logging with per-inode filtering
- **ReadIndexManager**: linearizable reads via ReadIndex protocol (Raft paper §8),
  pending read tracking, heartbeat quorum confirmation, apply-index waiting
- **WatchManager**: inotify-like watch/notify for directory change events,
  per-client event queuing, recursive watches, 6 event types (Create, Delete,
  Rename, AttrChange, DataChange, XattrChange)
- **POSIX access control**: check_access with owner/group/other bit evaluation,
  root bypass, sticky bit enforcement, supplementary group support
- **FileHandleManager**: open file descriptor tracking for FUSE integration,
  per-inode and per-client indexing, is_open_for_write check, disconnect cleanup
- **MetricsCollector**: per-operation counts/errors/latencies for Prometheus export,
  cache hit/miss counters, point-in-time snapshot, 15 MetricOp types
- 233 unit tests passing, 0 clippy warnings, 0 unsafe code
- 25 modules total: types, kvstore, inode, directory, consensus, journal, locking,
  lease, xattr, shard, replication, pathres, multiraft, service, raftservice,
  transaction, snapshot, quota, conflict, readindex, watch, access, filehandle,
  metrics, main
- Ready for integration with A5 (FUSE), A6 (Replication), A7 (Gateways), A8 (Mgmt)

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
