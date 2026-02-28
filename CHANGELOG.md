# ClaudeFS Changelog

All notable changes to the ClaudeFS project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Phase 1: Foundation

#### 2026-02-28

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
