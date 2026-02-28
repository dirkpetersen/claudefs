# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## CRITICAL: Rust Code Must Be Written by OpenCode, Not Claude

**This is the highest-priority instruction. It overrides all other guidance.**

Claude agents MUST NOT write or modify Rust code (`.rs` files, `Cargo.toml`) directly. All Rust code authoring and editing MUST be delegated to **OpenCode** using Fireworks AI models. Claude agents orchestrate, plan, review, test, and commit — but OpenCode does the actual Rust implementation.

### How to delegate Rust work to OpenCode

```bash
# Install opencode (one-time, already in orchestrator user-data)
curl -fsSL https://opencode.ai/install | bash

# Ensure API key is set (retrieved from Secrets Manager at boot)
export FIREWORKS_API_KEY=<from cfs/fireworks-api-key secret>

# Write your instructions to a file, then run opencode
cat > input.md << 'EOF'
<your detailed instructions, context, and requirements here>
EOF

~/.opencode/bin/opencode run "$(cat input.md)" \
  --model fireworks-ai/accounts/fireworks/models/minimax-m2p5 > output.md
```

### Model selection

| Model | Use for |
|-------|---------|
| `fireworks-ai/accounts/fireworks/models/minimax-m2p5` | **Default** — all Rust implementation work |
| `fireworks-ai/accounts/fireworks/models/glm-5` | **Alternative** — try if minimax-m2p5 struggles with a task |

### What Claude agents DO

- Read and understand architecture docs, decisions, and conventions
- Plan what Rust code needs to be written (interfaces, data structures, algorithms)
- Write detailed `input.md` prompts for OpenCode with full context
- Run OpenCode and collect `output.md` results
- Review the generated Rust code for correctness and conformance
- Run `cargo build`, `cargo test`, `cargo clippy` to validate
- Fix issues by writing new OpenCode prompts (not by editing Rust directly)
- Commit, push, update CHANGELOG, create GitHub Issues
- Write non-Rust files directly (shell scripts, YAML, JSON, Markdown, Protobuf, C for Samba VFS)

### What Claude agents MUST NOT DO

- Write `.rs` files using Write/Edit tools
- Modify `Cargo.toml` using Write/Edit tools
- Inline-edit Rust code to "fix" compiler errors (delegate back to OpenCode)

### Workflow example

```bash
# 1. Claude agent writes the prompt
cat > input.md << 'EOF'
Implement the block allocator for claudefs-storage crate.
Requirements:
- Buddy allocator for NVMe block allocation
- Thread-safe with lock-free fast path
- Supports 4KB, 64KB, 1MB, 64MB block sizes
- See docs/decisions.md D1 for erasure coding context
EOF

# 2. Run opencode with default model
~/.opencode/bin/opencode run "$(cat input.md)" \
  --model fireworks-ai/accounts/fireworks/models/minimax-m2p5 > output.md

# 3. Review output.md, extract code, place in crate directory

# 4. Build and test
cd /home/cfs/claudefs && cargo build && cargo test

# 5. If errors, write a new prompt with the error context and re-run opencode
```

### Secret: `cfs/fireworks-api-key` in AWS Secrets Manager (us-west-2)

The orchestrator user-data script retrieves this at boot and exports `FIREWORKS_API_KEY` for all agent sessions.

## Autonomous Supervision

Three layers keep agents running unattended. Agents should never need manual intervention.

- **Watchdog** (`/opt/cfs-watchdog.sh`, tmux session `cfs-watchdog`): checks every 2 min if each agent's tmux session is alive and has active claude/opencode/cargo processes. Relaunches dead or idle agents. Pushes unpushed commits.
- **Supervisor** (`/opt/cfs-supervisor.sh`, cron every 15 min): runs Claude Sonnet to inspect full system diagnostics, fix `cargo check` errors via OpenCode, commit forgotten files, restart dead watchdog/agents.
- **Cost monitor** (`/opt/cfs-cost-monitor.sh`, cron every 15 min): kills spot instances if daily AWS spend exceeds $100.

Logs: `/var/log/cfs-agents/watchdog.log`, `/var/log/cfs-agents/supervisor.log`

---

## Project Overview

**ClaudeFS** is a distributed, scale-out POSIX file system implemented in Rust with 8 crates in a Cargo workspace.

License: MIT. Author: Dirk Petersen.

## Architecture Vision

- **Distributed flash layer** spanning multiple nodes, hosting both data and metadata
- **S3-compatible object store backend** for tiered storage — only uses GET, PUT, DELETE operations with 64MB blob chunks, written asynchronously to tolerate high-latency or unreliable stores
- **Distributed metadata servers** with asynchronous cross-site replication; eventually-consistent with last-write-wins conflict resolution and administrator alerting on write conflicts
- **Single FUSE client** with pluggable transport (RDMA or TCP); pNFS/NFS gateway for legacy access
- **Cross-site replication** designed from day one — two metadata servers syncing asynchronously
- **No single points of failure** — erasure coding, cross-site replication, end-to-end checksums
- **Zero external dependencies** — no ZooKeeper/etcd/external DB; single binary per client

## Target Platform

- **Server nodes:** kernel 6.20+ (Ubuntu 26.04, April 2026) — atomic writes, dynamic io_uring, EEVDF scheduler
- **Clients:** kernel 5.14+ (RHEL 9, Ubuntu 22.04+) — FUSE passthrough requires 6.8+; degrades gracefully on older kernels
- Supported distros: Ubuntu 24.04, Ubuntu 26.04, RHEL 9, RHEL 10
- Standard Linux deployment model (similar to Weka IO)

## Client Architecture

Single binary (`cfs`) with subcommands for all roles — see [docs/decisions.md](docs/decisions.md) for all architecture decisions (D1–D10).

Single FUSE v3 client (`cfs mount`) with pluggable network transport:

- FUSE v3 with passthrough mode (6.8+) — metadata through daemon, data I/O at native NVMe speed
- io_uring for all async I/O (disk + network)
- **RDMA transport** via `libfabric` — one-sided verbs, zero-copy (requires InfiniBand/RoCE)
- **TCP transport** via io_uring zero-copy — automatic fallback, no special hardware
- Per-core NVMe queue alignment, speculative metadata resolution
- Full POSIX by default, optional relaxation flags
- Runs on kernel 5.14+ (RHEL 9+); passthrough on 6.8+

### Access without installing ClaudeFS
- **pNFS (NFSv4.1+)** — parallel direct-to-node via standard kernel NFS client
- **NFS gateway (NFSv3)** — legacy translation proxy

## Implementation

- **Language:** Rust — compiler-enforced memory safety and data-race freedom; `unsafe` isolated to io_uring/RDMA/FUSE FFI boundaries — see [docs/language.md](docs/language.md)
- **Key crates:** `fuser` (FUSE v3), `io-uring`, `libfabric` bindings, `aws-sdk-rust` (S3)
- **Async runtime:** Tokio with io_uring backend
- **Key kernel features:** FUSE passthrough (6.8+), io_uring + NVMe passthrough, atomic writes (6.11+), kTLS, ID-mapped mounts — see [docs/kernel.md](docs/kernel.md)
- **Research foundations:** InfiniFS, Orion, Assise, MadFS, DAOS, LineFS, FLEX — see [docs/literature.md](docs/literature.md)
- **Transport layer:** Custom RPC over io_uring (core), pNFS layouts (modern clients), NFS gateway (legacy) — see [docs/transport.md](docs/transport.md)
- **Metadata service:** Distributed hash-based metadata, Raft consensus intra-site, async journal replication cross-site via cloud conduit — see [docs/metadata.md](docs/metadata.md)
- **Data reduction:** Inline dedupe -> compress -> encrypt pipeline, CAS model, CoW snapshots — see [docs/reduction.md](docs/reduction.md)
- **Hardware reference:** Solidigm FDP/QLC, AMD EPYC, NVIDIA/Broadcom/Intel NICs, Supermicro chassis — see [docs/hardware.md](docs/hardware.md)
- **AI inference:** NVIDIA Dynamo/NIXL KV cache on the storage mesh; GPUs in compute nodes, not storage — see [docs/inference.md](docs/inference.md)
- **Management:** Prometheus monitoring, Parquet/DuckDB metadata search lakehouse, Grafana dashboards — see [docs/management.md](docs/management.md)
- **POSIX validation:** pjdfstest, xfstests, fsx, LTP, Connectathon, Jepsen, FIO, CrashMonkey — see [docs/posix.md](docs/posix.md)

## Cargo Workspace Structure

The Rust codebase is organized as a Cargo workspace with one crate per agent-owned subsystem:

```
claudefs/
├── Cargo.toml              # Workspace root
├── crates/
│   ├── claudefs-storage/   # A1: io_uring NVMe, block allocator
│   ├── claudefs-meta/      # A2: Raft consensus, KV store, inode ops
│   ├── claudefs-reduce/    # A3: dedupe, compression, encryption
│   ├── claudefs-transport/ # A4: RDMA/TCP, custom RPC
│   ├── claudefs-fuse/      # A5: FUSE v3 daemon, passthrough
│   ├── claudefs-repl/      # A6: cross-site journal replication
│   ├── claudefs-gateway/   # A7: NFSv3, pNFS, S3 API
│   └── claudefs-mgmt/      # A8: Prometheus, DuckDB, Web UI, CLI
├── tools/                  # Bootstrap infrastructure (cfs-dev, user-data, etc.)
└── docs/                   # Architecture decisions and design docs
```

## Development: Parallel Agent Plan

11 agents, 3 phases, up to 11 parallel — see [docs/agents.md](docs/agents.md):
- **Builders (A1–A8):** Storage, Metadata, Reduction, Transport, FUSE, Replication, Gateways (NFS+Samba VFS+S3), Management
- **Cross-cutting (A9–A11):** Test & Validation (POSIX suites, Jepsen, CrashMonkey), Security Audit (unsafe review, fuzzing, crypto audit), Infrastructure & CI (AWS spot cluster, deployment)
- **Test cluster:** 1 orchestrator + 9 preemptible nodes (5 storage, 2 clients, 1 conduit, 1 Jepsen)

## Competitive Landscape

See [docs/market.md](docs/market.md) for detailed analysis. Key competitors: VAST Data, Weka, DAOS, BeeGFS, CephFS, JuiceFS, Lustre, GPFS. ClaudeFS targets: Weka-class performance, VAST-class economics, open-source freedom, operational simplicity.

## Feature Gaps vs VAST/Weka (Tracked)

Priority 1: multi-tenancy/quotas, QoS/traffic shaping, online node scaling, flash defrag, distributed tracing. Priority 2: WORM/compliance, key rotation, S3 API, SMB gateway, migration tools. Priority 3: intelligent tiering, CDC events, active-active failover, performance SLAs. Full breakdown in [docs/agents.md](docs/agents.md).
