# Development Agents: Parallel Implementation Plan

ClaudeFS is designed to be implemented by multiple AI coding agents working in parallel. Each agent owns a well-defined subsystem with clear interfaces to other subsystems. This document defines the agent breakdown, dependencies, phasing, and infrastructure.

## Developer Experience: One Command to Launch

The entire development lifecycle is triggered by a single command from the developer's local machine:

```bash
cfs-dev up
```

This command:

1. **Provisions or reuses the orchestrator node** — checks if a persistent AWS instance is already running (tagged `cfs-orchestrator`). If yes, reconnects. If no, provisions a `c7a.2xlarge` (or configured instance type) with the ClaudeFS repo cloned and Rust toolchain installed.
2. **Starts Claude Code on the orchestrator** — Claude Code launches with the full CLAUDE.md context, spawning up to 11 agents in parallel based on the current phase.
3. **Agents provision test infrastructure** — A11 (Infrastructure) brings up the spot instance cluster as needed (storage nodes, clients, conduit, Jepsen controller). Nodes that already exist are reused.
4. **Agents build, test, and iterate** — each builder agent works on its crate, pushes code, triggers CI. Cross-cutting agents validate, audit, and maintain infrastructure.
5. **Progress is pushed to GitHub continuously** — see Changelog Protocol below.

```
Developer laptop                    AWS
┌──────────┐    cfs-dev up     ┌─────────────────────────────┐
│           │ ───────────────► │  Orchestrator (persistent)  │
│  Browser  │                  │  ┌─────────────────────┐    │
│  watches  │ ◄─── git push ── │  │ Claude Code          │    │
│  GitHub   │                  │  │ ├── A1: Storage       │    │
│  commits  │                  │  │ ├── A2: Metadata      │    │
│           │                  │  │ ├── A3: Reduction     │    │
└──────────┘                  │  │ ├── A4: Transport     │    │
                               │  │ ├── A5: FUSE         │    │
                               │  │ ├── ...              │    │
                               │  │ └── A11: Infra ──────┼──► Spot Cluster
                               │  └─────────────────────┘    │  (5 storage,
                               └─────────────────────────────┘   2 clients,
                                                                  1 conduit,
                                                                  1 Jepsen)
```

### cfs-dev Commands

```bash
cfs-dev up                    # Provision/reuse orchestrator, start agents
cfs-dev up --phase 2          # Start at a specific phase
cfs-dev status                # Show orchestrator status, running agents, cluster nodes
cfs-dev logs                  # Stream agent activity logs
cfs-dev logs --agent A2       # Stream specific agent's logs
cfs-dev down                  # Tear down spot cluster (keep orchestrator)
cfs-dev destroy               # Tear down everything including orchestrator
cfs-dev cost                  # Show current AWS spend
```

`cfs-dev` is a small shell script or Python CLI that wraps `aws ec2` / `ssh` / `gh` commands. It lives in the repo at `tools/cfs-dev`.

### Changelog Protocol: Keeping the Developer in the Loop

All agents push to `https://github.com/dirkpetersen/claudefs` frequently so the developer can follow progress by watching the GitHub commit history. This is the primary communication channel between the AI agents and the developer.

**Rules for all agents:**

1. **Commit early, commit often** — every meaningful unit of work gets a commit. Don't accumulate hours of work in uncommitted state.
2. **Push after every commit** — the developer watches the GitHub repo. Unpushed commits are invisible.
3. **Descriptive commit messages** — the commit message is the changelog entry. Format:
   ```
   [A2] Implement Raft leader election for metadata shards

   - Raft group per virtual shard (256 default per D4)
   - Leader election with randomized timeout (150-300ms)
   - Log replication to 2 followers with majority ack
   - Passes single-node unit tests, multi-node pending A11 cluster

   Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
   ```
4. **Agent prefix** — every commit is prefixed with the agent tag: `[A1]`, `[A2]`, ..., `[A11]`. This lets the developer filter by agent in the git log.
5. **CHANGELOG.md** — a running file at the repo root, updated by each agent when it completes a milestone:
   ```markdown
   ## 2026-03-15

   ### A1: Storage Engine
   - io_uring NVMe passthrough working on kernel 6.20
   - FDP hint tagging implemented for Solidigm drives
   - Block allocator passes basic alloc/free tests

   ### A2: Metadata Service
   - Raft leader election working across 3 nodes
   - Inode create/lookup/delete operations passing pjdfstest subset

   ### A9: Test & Validation
   - pjdfstest integrated into CI, 847/1200 tests passing
   - fsx running overnight soak test on single node
   ```
6. **GitHub Issues for blockers** — if an agent is blocked on another agent's work, it creates a GitHub Issue tagged with both agents. The developer sees these in the issue tracker.
7. **GitHub Releases for phase milestones** — at the end of each phase, A11 creates a GitHub Release with a summary of what's working, what's not, and what's next.

### What the Developer Sees

By watching the GitHub repo (email notifications, GitHub mobile, or just refreshing the commit page), the developer gets:

- **Real-time progress** — commits arrive every 15-60 minutes from active agents
- **Per-agent filtering** — `git log --grep='\[A2\]'` shows all metadata service progress
- **Daily summary** — CHANGELOG.md updated by each agent at the end of its work session
- **Blockers and decisions** — GitHub Issues for anything that needs human input
- **Phase milestones** — GitHub Releases for major checkpoints

The developer never needs to SSH into the orchestrator or read logs — GitHub is the single pane of glass.

## Agent Roster: 11 Agents

### Builder Agents (A1–A8): Write Code

| Agent | Crate | Owns | Depends On |
|-------|-------|------|-----------|
| **A1: Storage Engine** | `claudefs-storage` | Local NVMe I/O via io_uring, FDP/ZNS data placement, flash allocator, block read/write | Kernel 6.20+ |
| **A2: Metadata Service** | `claudefs-meta` | Distributed metadata, Raft consensus, inode/directory operations, distributed locking, speculative path resolution | A1 (storage for Raft log) |
| **A3: Data Reduction** | `claudefs-reduce` | Inline dedupe (BLAKE3 CAS), similarity pipeline, LZ4/Zstd compression, AES-GCM encryption, reference counting, GC | A1 (block storage), A2 (fingerprint index) |
| **A4: Transport** | `claudefs-transport` | RDMA via libfabric, TCP via io_uring, custom RPC protocol, connection management, zero-copy data transfer | A1 (block read/write) |
| **A5: FUSE Client** | `claudefs-fuse` | FUSE v3 daemon, passthrough mode, client-side metadata cache, mount/unmount, POSIX syscall handling | A2 (metadata), A4 (transport) |
| **A6: Replication** | `claudefs-repl` | Cross-site journal replication, cloud conduit (gRPC/mTLS), UID mapping, conflict detection, batch compaction | A2 (Raft journal), A4 (transport) |
| **A7: Protocol Gateways** | `claudefs-gateway` | NFSv3 gateway, pNFS layout server, NFS v4.2 exports, Samba VFS plugin for SMB3, S3 API endpoint | A2 (metadata), A4 (transport) |
| **A8: Management** | `claudefs-mgmt` | Prometheus exporter, Parquet indexer, DuckDB query gateway, Web UI (React), CLI, admin API (Axum) | A2 (metadata journal) |

### Cross-Cutting Agents (A9–A11): Validate, Secure, Deploy

| Agent | Owns | When Active |
|-------|------|-------------|
| **A9: Test & Validation** | POSIX test suites (pjdfstest, xfstests, fsx), distributed tests (Connectathon, Jepsen), crash consistency (CrashMonkey), performance benchmarks (FIO), integration tests, regression suite | Phase 1 (unit tests) → Phase 2+ (full suite) |
| **A10: Security Audit** | `unsafe` code review, fuzzing (network protocol, FUSE, NFS), authentication/authorization audit, encryption implementation review, dependency CVE scanning, penetration testing of management API and cloud conduit | Phase 2 → Phase 3 |
| **A11: Infrastructure & CI** | AWS provisioning (Terraform/Pulumi), test cluster orchestration, preemptible node management, CI/CD pipeline (GitHub Actions), artifact builds, multi-node deployment automation | Phase 1 (CI setup) → always |

### Why Separate Test, Security, and Infra Agents

**A9 (Test & Validation)** must be separate from builders because:
- Adversarial mindset — builders want code to work, testers want to break it
- Cross-cutting scope — tests exercise code across all crates simultaneously (a rename test hits A5+A2+A4+A1)
- Test suites are large standalone projects (Jepsen alone is a significant effort)
- Regression ownership — when a builder's change breaks a test, A9 files the issue and the builder fixes it

**A10 (Security Audit)** must be separate because:
- Reviews `unsafe` blocks in A1/A4/A5 with fresh eyes — the author of `unsafe` code is the worst person to audit it
- Fuzzing is a specialized discipline — libfuzzer/AFL++ against the RPC protocol, FUSE interface, NFS gateway, management API
- Crypto review — A3's encryption implementation needs independent verification (correct AES-GCM nonce handling, HKDF key derivation, no timing side channels)
- Dependency auditing — `cargo audit` in CI, plus manual review of `unsafe` in transitive dependencies

**A11 (Infrastructure & CI)** must be separate because:
- Builder agents should never manage infrastructure — they write code, push it, and CI handles the rest
- Preemptible instance management is a full-time operational concern
- Test cluster lifecycle (provision → deploy → test → teardown) must be automated and reproducible

## Dependency Graph

```
                  A11: Infrastructure & CI
                  (provisions everything)
                          |
                  A1: Storage Engine
                /        |         \
          A2: Metadata  A3: Reduce  A4: Transport
            |    \        |           /      |
            |     \       |          /       |
          A5: FUSE  A6: Replication  A7: Gateways
            |                                |
          A8: Management              (Samba VFS, S3)
                          |
                  A9: Test & Validation
                  (tests across all crates)
                          |
                  A10: Security Audit
                  (reviews all crates)
```

## Phasing

### Phase 1: Foundation

**5 agents active (4 builders + infra):**

- **A1: Storage Engine** — io_uring NVMe passthrough, block allocator, FDP hint tagging, read/write API
- **A2: Metadata Service** — Raft consensus, KV store, inode/directory ops. Starts with in-memory mock of A1.
- **A4: Transport** — RPC protocol, RDMA + TCP backends, connection lifecycle. Develops against localhost.
- **A3: Data Reduction** — BLAKE3, FastCDC, LZ4/Zstd, AES-GCM as standalone library. Pure data transforms.
- **A11: Infrastructure** — set up CI/CD, provision first 3-node test cluster, automate build+deploy pipeline

**A9 begins writing:** unit test harnesses, property-based tests for each crate's trait interface, basic pjdfstest wrapper.

**End of Phase 1:** Single-node storage + metadata + transport. Data reduction as library. CI deploying to test cluster.

### Phase 2: Integration

**7 agents active (6 builders + infra), A9 and A10 ramp up:**

- **A5: FUSE Client** — wire FUSE daemon to A2+A4, passthrough mode, client caching
- **A3: Data Reduction** — integrate into write path, wire fingerprint index to A2
- **A6: Replication** — journal tailer, cloud conduit, UID mapping, conflict detection
- **A7: Protocol Gateways** — NFSv3 translation, pNFS layouts. **Samba VFS plugin** development begins (see below).
- **A8: Management** — Prometheus exporter, Parquet indexer, DuckDB gateway
- **A9: Test & Validation** — full POSIX suites (pjdfstest, fsx, xfstests) on multi-node cluster, Connectathon across nodes, begin Jepsen partition tests
- **A10: Security** — begin `unsafe` code review (A1, A4, A5), fuzz the RPC protocol, audit authentication in A6 conduit and A8 admin API
- **A11: Infrastructure** — scale to full test cluster (see below), preemptible instance automation

**End of Phase 2:** Multi-node cluster with FUSE + NFS + replication + data reduction + monitoring. POSIX tests passing.

### Phase 3: Production Readiness

**All 11 agents active:**

- **Builders (A1–A8):** bug fixes from test/security findings, performance optimization, feature gaps (quotas, QoS, node scaling)
- **A9:** Jepsen split-brain tests, CrashMonkey crash consistency, FIO performance benchmarks, long-running soak tests
- **A10:** full penetration test of management API, crypto audit, dependency CVE sweep, final `unsafe` review
- **A11:** production deployment templates, documentation of operational procedures

**End of Phase 3:** Production-ready, validated, security-audited system.

## SMB3 Gateway: Samba VFS Plugin

The SMB3 gateway is **not** a from-scratch SMB implementation. SMB3 is an enormous protocol — reimplementing it would be years of work. Instead:

**Approach:** Write a Samba VFS module (~2,000–5,000 lines of C) that translates Samba's VFS operations to ClaudeFS's internal RPC protocol.

- **Samba handles:** all SMB3.1.1 protocol complexity, Kerberos/NTLM authentication, encryption, signing, compound requests, credit-based flow control, oplock/lease management
- **VFS plugin handles:** mapping `open`/`read`/`write`/`stat`/`rename`/`mkdir` to ClaudeFS transport calls (A4)
- **Deployment:** Samba runs as a gateway process on dedicated nodes or co-located on ClaudeFS servers
- **License:** Samba is GPLv3 but runs as a separate process communicating over ClaudeFS's RPC protocol — no license contamination of the MIT codebase. The VFS plugin itself would be GPLv3 (required for Samba modules), distributed as a separate package.
- **SMB3+ only** — no SMB1/SMB2 support needed. Samba configuration enforces `min protocol = SMB3`.

This is what VAST, Weka, CephFS (`vfs_ceph`), and GlusterFS (`vfs_glusterfs`) all do. It's the proven approach.

**Owner:** A7 (Protocol Gateways), Phase 2.

## Test Infrastructure: AWS Development Cluster

### Orchestrator Node (always running)

One persistent AMD instance where Claude Code runs, managing all agents:

| Component | Specification |
|-----------|--------------|
| Instance | `c7a.2xlarge` (8 vCPU AMD EPYC, 16 GB RAM) or similar |
| Role | Claude Code host, CI/CD controller, agent orchestration |
| Storage | 100 GB gp3 for repos, build artifacts, CI state |
| Always on | Yes — this is the development hub |

### Test Cluster Nodes (preemptible / spot instances)

Spun up for testing, torn down when idle. A11 automates the lifecycle.

| Role | Count | Instance Type | Why |
|------|-------|--------------|-----|
| **Storage servers** | 5 | `i4i.2xlarge` (NVMe, 8 vCPU, 64 GB) | 3 for site A (Raft quorum), 2 for site B (replication) |
| **FUSE client** | 1 | `c7a.xlarge` (4 vCPU, 8 GB) | Runs pjdfstest, fsx, xfstests |
| **NFS/SMB client** | 1 | `c7a.xlarge` (4 vCPU, 8 GB) | Connectathon, multi-protocol testing |
| **Cloud conduit** | 1 | `t3.medium` (2 vCPU, 4 GB) | gRPC relay for cross-site replication |
| **Jepsen controller** | 1 | `c7a.xlarge` (4 vCPU, 8 GB) | Runs Jepsen tests, fault injection |

**Total: 10 nodes** (1 persistent + 9 preemptible)

### Why These Numbers

- **5 storage servers** — minimum for: 3-node Raft quorum (site A) + 2-node site B (replication testing) + ability to test node failure (lose 1, still have quorum)
- **2 client nodes** — separate FUSE and NFS clients ensure multi-protocol tests don't interfere
- **1 conduit** — the gRPC relay needs its own instance to simulate real cross-site latency
- **1 Jepsen controller** — Jepsen needs a separate node to inject faults (network partitions, process kills) without disrupting its own control plane

### Cost Management

- **Daily budget: $100** (EC2 + Bedrock combined)
- All 9 test nodes are **spot/preemptible instances** — 60-90% cheaper than on-demand
- A11 provisions the cluster on demand: `cfs-dev up` before a test run, `cfs-dev down` after
- Estimated cost: ~$5-10/hour when the full cluster is running, $0 when idle (only the orchestrator runs)
- CI runs nightly on the full cluster; developers trigger on-demand runs for specific test suites
- Instance types can be downgraded for basic functional testing, upgraded for performance benchmarks

### AWS Budget Breakdown

| Resource | Type | Estimated Daily Cost |
|----------|------|---------------------|
| Orchestrator | EC2 c7a.2xlarge on-demand | ~$10/day |
| 5x storage nodes | EC2 i4i.2xlarge spot (8 hrs) | ~$14/day |
| 2x client nodes | EC2 c7a.xlarge spot (8 hrs) | ~$1/day |
| 1x conduit | EC2 t3.medium spot (8 hrs) | ~$0.15/day |
| 1x Jepsen | EC2 c7a.xlarge spot (8 hrs) | ~$0.50/day |
| Secrets Manager | 2 secrets | ~$0.03/day |
| **EC2 subtotal** | | **~$26/day** |
| **Bedrock** | 5-7 agents, Opus/Sonnet/Haiku | **~$55-70/day** |
| **Grand total** | | **~$80-96/day** |

### Budget Enforcement

- AWS Budgets: `cfs-daily-100` — $100/day with alerts at 80% and 100%
- SNS topic `cfs-budget-alerts` — notification endpoint
- Cost monitor cron: `tools/cfs-cost-monitor.sh` runs every 15 minutes on orchestrator, auto-terminates spot instances at budget limit

### Three-Layer Autonomous Supervision

The orchestrator runs three layers of supervision so agents operate unattended:

| Layer | Script | Frequency | What It Does |
|-------|--------|-----------|-------------|
| **Watchdog** | `tools/cfs-watchdog.sh` | Every 2 min | Bash loop: checks tmux sessions alive, detects idle agents (no claude/opencode/cargo), auto-relaunches dead sessions, pushes unpushed commits, reports status every 10 min |
| **Supervisor** | `tools/cfs-supervisor.sh` | Every 15 min (cron) | Runs Claude Sonnet: gathers full diagnostics (tmux, processes, git log, cargo check, code stats), fixes build errors via OpenCode, commits forgotten files, restarts dead agents/watchdog, pushes to GitHub |
| **Cost monitor** | `tools/cfs-cost-monitor.sh` | Every 15 min (cron) | Bash: checks daily AWS spend via Cost Explorer, terminates all spot instances if $100/day exceeded, publishes SNS alert |

**How they interact:**
- The **watchdog** handles fast recovery — if an agent's tmux session crashes or finishes, it relaunches within 2 minutes.
- The **supervisor** handles intelligent recovery — if `cargo check` fails, it uses Claude to diagnose the error and generate a fix via OpenCode. It also catches files that agents generated but forgot to commit.
- The **cost monitor** is the safety net — hard kill of all spot instances if budget is exceeded.

**Watchdog details:** Runs as a persistent `cfs-watchdog` tmux session. For each agent, it checks: (1) does the tmux session exist? (2) is a claude, opencode, or cargo process running inside it? If not, it kills and relaunches the session. It also runs `git push` every cycle to ensure commits reach GitHub.

**Supervisor details:** Runs via cron as the `cfs` user. Uses a lockfile (`/tmp/cfs-supervisor.lock`) to prevent overlapping runs. Gathers: tmux session list, running processes, last 5 commits, commit age, unpushed commits, dirty files, `cargo check` output, watchdog log, agent log sizes, and per-crate Rust code stats. Feeds all of this to Claude Sonnet with instructions to fix what's broken. Has a 5-minute timeout per run.

### Rust Code Delegation: OpenCode via Fireworks AI

**Claude agents MUST NOT write Rust code directly.** All `.rs` and `Cargo.toml` authoring is delegated to OpenCode using Fireworks AI models. This is the highest-priority instruction in CLAUDE.md.

**Workflow:**
1. Claude agent plans what code is needed (reads docs, designs interfaces)
2. Writes a detailed prompt to `input.md` (or `a1-input.md`, `a2-input.md`, etc.)
3. Runs: `~/.opencode/bin/opencode run "$(cat input.md)" --model fireworks-ai/accounts/fireworks/models/minimax-m2p5 > output.md`
4. Extracts Rust code from `output.md` and places it in the crate directory
5. Runs `cargo build && cargo test && cargo clippy` to validate
6. If errors, writes a new prompt with error context and re-runs OpenCode
7. Commits and pushes when tests pass

**Models:**
| Model | Use |
|-------|-----|
| `fireworks-ai/accounts/fireworks/models/minimax-m2p5` | Default — all Rust implementation |
| `fireworks-ai/accounts/fireworks/models/glm-5` | Alternative — try if minimax struggles |

**Secret:** `cfs/fireworks-api-key` in AWS Secrets Manager, retrieved at boot, exported as `FIREWORKS_API_KEY`.

### Bootstrap Infrastructure (tools/)

| Script | Purpose |
|--------|---------|
| `tools/cfs-dev` | Main CLI: `up`, `status`, `logs`, `down`, `destroy`, `cost`, `ssh` |
| `tools/orchestrator-user-data.sh` | Cloud-init: Rust, Node.js 22, Claude Code, OpenCode, GitHub CLI |
| `tools/storage-node-user-data.sh` | Cloud-init: NVMe setup, kernel tuning for storage |
| `tools/client-node-user-data.sh` | Cloud-init: FUSE/NFS/SMB client tools, POSIX test deps |
| `tools/cfs-agent-launcher.sh` | Launches agents as tmux sessions with per-agent model/env setup |
| `tools/cfs-watchdog.sh` | Fast supervision loop (2-min cycle, restarts dead agents) |
| `tools/cfs-supervisor.sh` | Claude-powered supervision cron (15-min, fixes build errors) |
| `tools/cfs-cost-monitor.sh` | Budget enforcement cron (15-min, kills spot at $100) |
| `tools/iam-policies/*.json` | IAM policies for orchestrator and spot nodes |

### Developer CLI: `cfs-dev`

```bash
cfs-dev up [--phase N] [--key KEY]   # Provision orchestrator, start agents + watchdog
cfs-dev status                       # Show orchestrator, nodes, agent sessions
cfs-dev logs [--agent A1|watchdog|supervisor]  # Stream agent or supervisor logs
cfs-dev ssh [target] [--key KEY]     # SSH to orchestrator or named node
cfs-dev cost                         # Today's spend, monthly total, budget status
cfs-dev down                         # Tear down spot cluster (keep orchestrator)
cfs-dev destroy                      # Tear down everything (requires confirmation)
```

Set `CFS_KEY_NAME=cfs-key` in your shell to avoid passing `--key` every time. The CLI auto-detects `.pem` key files in `~/.ssh/`.

### AWS Resources (Pre-provisioned)

| Resource | Name | Notes |
|----------|------|-------|
| Secrets | `cfs/github-token`, `cfs/ssh-private-key`, `cfs/fireworks-api-key` | Secrets Manager (us-west-2) |
| IAM role | `cfs-orchestrator-role` | Bedrock (us + global), EC2, Secrets, CloudWatch, Budgets |
| IAM role | `cfs-spot-node-role` | Secrets, CloudWatch logs, EC2 describe |
| Instance profile | `cfs-orchestrator-profile` | Attached to orchestrator |
| Instance profile | `cfs-spot-node-profile` | Attached to spot nodes |
| Security group | `cfs-cluster-sg` | All traffic within group + SSH from dev IP |
| Budget | `cfs-daily-100` | $100/day with 80%/100% alerts via SNS |
| SNS topic | `cfs-budget-alerts` | Budget alert notifications |
| AMI | Ubuntu 25.10 Questing | Kernel 6.17+ (FUSE passthrough, atomic writes, io_uring) |

### Scaling for Performance Benchmarks

For FIO benchmarks and scale testing, A11 can temporarily provision:

- 10-20 storage servers to test horizontal scaling
- Multiple client instances to simulate high concurrency
- `i4i.4xlarge` or larger for NVMe-intensive benchmarks

These scale-up clusters are short-lived (hours) and fully preemptible.

## Maximum Agent Parallelism by Phase

| Phase | Builder Agents | Cross-Cutting Agents | Total Active | Test Nodes |
|-------|---------------|---------------------|-------------|------------|
| Phase 1 | 4 (A1, A2, A3, A4) | 1 (A11) | **5** | 3 (basic cluster) |
| Phase 2 | 6 (A3, A5, A6, A7, A8 + fixes) | 3 (A9, A10, A11) | **9** | 10 (full cluster) |
| Phase 3 | 8 (all, bug fixes + features) | 3 (A9, A10, A11) | **11** | 10-20 (scale tests) |

## Model Selection: Cost vs Accuracy

Five Claude models are available. Each agent task is assigned the cheapest model that can do the job reliably. The principle: **Opus for architecture, Sonnet for implementation, Haiku for repetition.**

### Available Models

| Model | ID | Strengths | Relative Cost |
|-------|-----|-----------|--------------|
| **Opus** | `global.anthropic.claude-opus-4-6-v1` | Complex architecture, multi-file reasoning, subtle correctness bugs | $$$$$ |
| **Opus Fast** | `global.anthropic.claude-opus-4-6-v1[1m]` | Same quality, faster output, larger context | $$$$ |
| **Sonnet** | `global.anthropic.claude-sonnet-4-6` | Solid implementation, single-crate work, tests, refactoring | $$$ |
| **Sonnet Fast** | `global.anthropic.claude-sonnet-4-6[1m]` | Same as Sonnet, faster, good for bulk code generation | $$ |
| **Haiku** | `us.anthropic.claude-haiku-4-5-20251001-v1:0` | Boilerplate, formatting, simple edits, repetitive transforms | $ |

### Model Assignment by Agent

| Agent | Primary Task | Model | Rationale |
|-------|-------------|-------|-----------|
| **A1: Storage Engine** | io_uring FFI, unsafe code, block allocator | **Opus** | Low-level unsafe Rust + io_uring requires highest accuracy. Bugs here corrupt data silently. |
| **A2: Metadata Service** | Raft consensus, distributed locking | **Opus** | Distributed consensus is the hardest correctness problem. Subtle bugs cause split-brain. |
| **A3: Data Reduction** | Dedupe pipeline, compression, encryption | **Sonnet** | Well-defined algorithms with existing crate wrappers. Trait implementations against known interfaces. |
| **A4: Transport** | RDMA FFI, custom RPC protocol | **Opus** | Unsafe libfabric bindings + zero-copy buffer management. Correctness-critical network code. |
| **A5: FUSE Client** | FUSE daemon, passthrough, caching | **Sonnet** | Mostly wiring A2+A4 together via fuser crate. Passthrough mode is well-documented. |
| **A6: Replication** | gRPC conduit, conflict resolution | **Sonnet** | Straightforward async Rust (tonic + tokio). Conflict logic is well-specified in decisions.md. |
| **A7: Protocol Gateways** | NFS/pNFS, Samba VFS (C), S3 API | **Sonnet** | Protocol translation layers. The Samba VFS plugin is small C, NFS is well-documented. |
| **A8: Management** | Prometheus, DuckDB, Web UI, CLI | **Sonnet Fast** | High-volume code gen (React components, CLI subcommands, Grafana JSON). Well-understood patterns. |
| **A9: Test & Validation** | Test suites, benchmarks, CI integration | **Sonnet** | Writing test harnesses, wrappers around existing test tools. Needs to understand the codebase but not architect it. |
| **A10: Security Audit** | Unsafe review, fuzzing, crypto audit | **Opus** | Reviewing other agents' unsafe code requires the deepest reasoning. Must catch what the author missed. |
| **A11: Infrastructure & CI** | Terraform, GitHub Actions, deployment | **Haiku** | Boilerplate-heavy infrastructure-as-code. Terraform modules, YAML pipelines, shell scripts. Well-templated work. |

### Model Usage by Task Type (Within Any Agent)

Agents can switch models for different sub-tasks within their work:

| Task Type | Model | Examples |
|-----------|-------|---------|
| **Architecture decisions** (trait design, API surface, cross-crate interfaces) | Opus | Defining the `StorageEngine` trait, designing the RPC protocol schema |
| **Core implementation** (new complex logic) | Opus or Sonnet | Raft state machine, EC stripe calculation, RDMA buffer management |
| **Standard implementation** (known patterns) | Sonnet | Implementing a gRPC service, writing a FUSE handler, Axum route handlers |
| **Bulk code generation** (repetitive structure) | Sonnet Fast | CLI subcommands, Protobuf message types, Parquet schema definitions |
| **Tests and fixtures** | Sonnet | Property-based tests, integration test setup, mock implementations |
| **Boilerplate and config** | Haiku | Cargo.toml manifests, GitHub Actions YAML, Dockerfile, Terraform resources, Grafana dashboard JSON |
| **Documentation and comments** | Haiku | README updates, doc comments, CHANGELOG entries |
| **Code review and security audit** | Opus | Reviewing unsafe blocks, checking for timing side channels, validating error handling |
| **Bug investigation** (cross-crate, subtle) | Opus | Why does Jepsen find a consistency violation? Why does this crash under load? |
| **Bug fix** (single-crate, obvious) | Sonnet | Fix a failing test, handle an edge case, update a type signature |

### Estimated Cost Distribution

Assuming a full development day (8 hours of agent activity):

| Model | % of Total Tokens | Relative Cost/Token | % of Total Cost |
|-------|-------------------|--------------------|-----------------|
| Opus | ~20% | 5x | ~50% |
| Sonnet / Sonnet Fast | ~60% | 1.5x | ~40% |
| Haiku | ~20% | 0.3x | ~10% |

**Cost optimization levers:**
- A1, A2, A4, A10 are Opus-heavy but they produce the smallest volume of code (low-level, careful work)
- A3, A5, A6, A7, A9 are the bulk of the codebase and run on Sonnet
- A8 and A11 produce the most lines of code (UI, infra) and use the cheapest models
- Any agent can drop to Haiku for boilerplate sub-tasks (Cargo.toml, config files, doc comments)

## Shared Conventions (All Agents)

- **Error handling:** `thiserror` for library errors, `anyhow` at binary entry points
- **Serialization:** `serde` + `bincode` for internal wire format, `prost` for gRPC Protobuf
- **Async:** Tokio with io_uring backend. All I/O through io_uring submission rings.
- **Logging:** `tracing` crate with structured spans. Every operation gets a trace ID for distributed debugging.
- **Testing:** property-based tests (`proptest`) for data transforms, integration tests per crate, Jepsen-style tests for distributed correctness
- **`unsafe` budget:** confined to A1 (io_uring FFI), A4 (RDMA/libfabric FFI), A5 (FUSE FFI), A7 (Samba VFS plugin in C). All other crates are safe Rust.

## Feature Gaps: What VAST/Weka Have That ClaudeFS Must Address

The following features are standard in VAST and Weka but not yet designed. Tracked as future work, prioritized by impact:

### Priority 1 — Required for Production

| Feature | Description | Owner Agent |
|---------|-------------|-------------|
| **Multi-tenancy & quotas** | Per-user/group storage quotas, IOPS/bandwidth limits, tenant isolation | A2 + A8 |
| **QoS / traffic shaping** | Priority queues, bandwidth guarantees per workload class | A4 + A2 |
| **Online node scaling** | Add/remove nodes without downtime, automatic data rebalancing | A1 + A2 |
| **Flash layer defragmentation** | Background compaction and GC for the flash allocator | A1 |
| **Distributed tracing** | OpenTelemetry integration, per-request latency attribution | All (tracing crate) |

### Priority 2 — Required for Enterprise Adoption

| Feature | Description | Owner Agent |
|---------|-------------|-------------|
| **Compliance / WORM** | Immutable snapshots, legal holds, retention policies, audit trails | A2 + A3 |
| **Key rotation** | Rotate encryption keys without re-encrypting all data (envelope encryption) | A3 |
| **S3 API endpoint** | Expose ClaudeFS namespace as S3-compatible object storage | A7 |
| **SMB3 gateway** | Windows client access via Samba VFS plugin (GPLv3, separate package) | A7 |
| **Data migration tools** | Import from Lustre/CephFS/NFS, parallel copy, zero-downtime migration | A8 + A5 |

### Priority 3 — Competitive Differentiation

| Feature | Description | Owner Agent |
|---------|-------------|-------------|
| **Intelligent tiering** | Access-pattern learning, automatic hot/cold classification | A1 + A8 |
| **Change data capture** | Event stream API for filesystem changes (webhooks) | A2 |
| **Active-active failover** | Automatic site failover with read-write on both sites | A6 |
| **Performance SLAs** | Published latency targets (p99), scaling curves | A8 |
