# Architecture Decisions

Resolved design decisions for ClaudeFS implementation. Each decision was evaluated and accepted during the planning phase.

## D1: Erasure Coding

**Decision:** Reed-Solomon EC at the segment level, Raft replication for metadata.

- **Default stripe:** 4+2 (4 data + 2 parity) for clusters with 6+ nodes
- **Small clusters:** 2+1 for 3-5 nodes
- **EC unit:** 2MB packed segments (post-dedup, post-compression), not individual CAS chunks — chunk-level EC would create massive metadata overhead
- **Metadata protection:** Raft 3-way replication (not EC) — metadata needs fast random reads; EC reconstruction latency is unacceptable for inode lookups

## D2: Cluster Membership and Discovery

**Decision:** SWIM protocol with bootstrap seed list.

- **SWIM** (Scalable Weakly-consistent Infection-style Membership) for failure detection — O(log N) convergence, constant bandwidth per node
- **Bootstrap:** New node joins with `cfs server join --token <cluster-secret> --seed node1:9400,node2:9400`
- **Hash ring:** Consistent hash ring derived from SWIM membership list; membership changes trigger rebalancing
- **No external dependencies:** SWIM is embedded in the `cfs` binary. No etcd, no ZooKeeper.
- Proven approach — same protocol used by Consul (HashiCorp)

## D3: Replication vs EC Within a Site

**Decision:** EC for data, Raft replication for metadata, 2x journal replication for hot ingest.

- **Data blocks:** EC 4+2 (1.5x storage overhead vs 3x for triple replication)
- **Metadata:** Raft 3-way replication — small, latency-sensitive, needs fast random access
- **Write journal:** 2x synchronous replication to two different nodes before ack to client. Fast durability.
- **Write path:** client write → 2x journal replication (sync, fast ack) → segment packing + EC 4+2 (async, background) → journal space reclaimed
- Combines fast writes (2x journal) with efficient storage (EC background)

## D4: Raft Group Topology

**Decision:** Multi-Raft with one Raft group per virtual shard.

- **Virtual shards:** 256 default (configurable at cluster creation, immutable after). 1024+ for clusters >100 nodes.
- **Each shard:** Independent Raft group with 3 replicas on 3 different nodes
- **Metadata routing:** `hash(inode) % num_shards` determines the shard, shard's Raft leader handles the operation
- **Parallelism:** 256 independent leaders distribute metadata load across all nodes — no single-leader bottleneck
- **Shard migration:** When nodes join/leave, entire shards rebalance (not individual inodes)
- Same approach as TiKV (PingCAP) and CockroachDB

## D5: S3 Tiering Policy

**Decision:** Capacity-triggered eviction with age-weighted scoring. Two operating modes.

### Cache Mode (Default)

- Every segment is asynchronously written to S3 — flash is a cache of S3
- Flash eviction drops the local copy (S3 already has it) — always safe
- On total cluster loss, rebuild from S3 via `cfs repair --from-s3`
- Accepted data loss window = async lag (seconds to minutes of most recent writes)

### Tiered Mode (Optional)

- Only aged-out segments go to S3. Flash-only data relies on EC + cross-site replication for durability.
- Lower S3 cost and bandwidth, but no full-cluster recovery from S3 alone.

### Eviction Policy (Both Modes)

- **High watermark (80%):** Start evicting segments. Score = `last_access_age × size` — old and bulky first.
- **Low watermark (60%):** Stop evicting.
- **Manual override:** `claudefs.tier=s3` (force), `claudefs.tier=flash` (pin), `claudefs.tier=auto` (default) via xattrs on directories.
- **Safety:** Never evict a segment that hasn't been confirmed in S3.

### Snap-to-Object

- Snapshots live on flash for a configurable retention period (default: 7 days)
- After retention, snapshot-unique blocks (not referenced by active files or newer snapshots) are pushed to S3 via CAS dedup — only new blocks transfer
- Snapshot metadata (pointer tree) serialized as an S3 object
- Old snapshots browsable/restorable from S3: `cfs admin snapshot restore <name> --from-s3`

## D6: Flash Layer Full

**Decision:** Cache mode makes this a solved problem.

- **Normal (>80%):** Evict cached segments (confirmed in S3) per D5 scoring
- **Critical (>95%):** Switch to write-through mode — writes go synchronously to S3 before ack. Slower but no data loss. Alert administrator.
- **Double failure (100% flash + S3 unreachable):** Return ENOSPC to clients. The only scenario where writes fail.
- **Never silently drop data.** No eviction of blocks not confirmed in S3.

## D7: Client Authentication

**Decision:** mTLS with auto-provisioned certificates from a self-contained cluster CA.

- **Cluster CA:** Generated at cluster creation. Stored encrypted in metadata service. Issues all certificates.
- **Client enrollment:** `cfs mount --token <one-time-token> /mnt/data`. Client presents token, receives signed certificate, stored at `~/.cfs/client.crt`. All subsequent mounts use mTLS automatically.
- **Certificate lifecycle:** 1-year default, automatic renewal. Revocation via `cfs admin client revoke <id>` with CRL distributed via SWIM gossip.
- **Kerberos (optional):** For Active Directory environments. Accept Kerberos tickets, issue session certificates. Also used by the Samba VFS plugin.
- **NFS clients:** Standard `sec=krb5p` for Kerberos, or `sec=sys` with IP-based trust.
- **Inter-daemon:** All internal communication uses mTLS with cluster-issued certificates.

## D8: Data Placement on Write

**Decision:** Metadata-local primary write, distributed EC stripes.

1. **Sync:** Client writes to the metadata-owning node (determined by consistent hash). This node runs the data reduction pipeline and writes to its local journal. Journal is 2x replicated (D3). Client gets ack.
2. **Async:** Background segment packer collects journal entries into 2MB segments, applies EC 4+2, distributes stripes across 6 different nodes via consistent hash.
3. **Async:** Cache mode pushes segment to S3 (D5).

**Read path:** Small reads hit the primary node's cache. Large sequential reads fetch EC stripes from multiple nodes in parallel — the metadata tells the client which nodes hold which stripes.

## D9: Single Binary (`cfs`)

**Decision:** One binary, subcommands determine role. Binary name: `cfs`.

```
cfs server                              # Storage + metadata node (all subsystems as Tokio tasks)
cfs mount /mnt/data                     # FUSE client
cfs admin status                        # Cluster health
cfs admin top-users                     # DuckDB analytics
cfs admin query "SELECT ..."            # Raw SQL over Parquet index
cfs admin node drain <id>               # Node management
cfs admin snapshot create /data --name weekly
cfs admin snapshot restore weekly --from-s3
cfs repair --from-s3 <bucket>           # S3 disaster recovery
```

**Server mode:** `cfs server` runs storage engine, metadata service, transport, data reduction, monitoring, indexing, and admin API as async Tokio tasks in one process. No separate daemons.

**Separate files (not in `cfs` binary):**
- `cfs-samba-vfs.so` — Samba VFS plugin (C, GPLv3, loaded by Samba, separate package)

## D10: Embedded KV Engine

**Decision:** Purpose-built KV in Rust on top of A1's block store. No RocksDB.

**Why not RocksDB:**
- 500K lines of C++ — contradicts the Rust safety story
- LSM compaction creates unpredictable latency spikes
- Manages its own memory, bypassing Rust's allocator
- Uses its own filesystem I/O, can't use our io_uring passthrough

**What to build:**
- Simple B+tree or LSM-tree in Rust, storing KV pairs via A1's io_uring block store
- Metadata values are small and uniform (inodes ~256 bytes) — known, narrow access patterns
- NVMe atomic writes (kernel 6.11+) for crash-consistent index updates — no software WAL needed
- Start from `sled` (pure Rust, embedded) or build custom

**What we keep as external dependencies:**
- **DuckDB** — cold analytics path only (admin queries over Parquet). Read-only, not on the I/O hot path. If DuckDB crashes, zero data loss. No Rust equivalent for analytical SQL + WASM browser execution.
- **Samba** — SMB3 protocol (separate process, GPLv3). 30 years of protocol engineering is not worth reimplementing.
- **Grafana/Prometheus** — monitoring visualization. Standard, optional, not embedded.

**Rule:** Own what's on the hot path (KV engine, storage engine, transport). Use best-in-class libraries for everything else.
