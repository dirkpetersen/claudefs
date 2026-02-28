# Distributed Metadata Service

ClaudeFS's metadata service operates at two levels: a high-performance distributed metadata engine within each site (strong consistency), and an asynchronous replication layer between sites (eventual consistency with conflict resolution). Both are co-designed from day one.

## 1. Local Metadata Architecture

### Distributed Metadata Engine

Within a single site, metadata is distributed across all storage nodes using consistent hashing (InfiniFS pattern):

- **Hash-based inode distribution** — each inode is assigned to a node based on `hash(inode_number) % node_count`. No single metadata server bottleneck.
- **Directory sharding** — hot directories automatically split across nodes. When `/data/training/` receives thousands of file creates per second, the directory's metadata is partitioned across multiple nodes, each handling a subset of entries.
- **Speculative path resolution** — when resolving `/a/b/c/d/file.txt`, the client speculatively sends parallel lookups for each path component to the nodes that likely own them, rather than walking the tree sequentially. This collapses multi-hop metadata lookups into a single network round-trip.

### The Metadata Data Model

Each metadata entry is stored in the node's local KV store (RocksDB or equivalent embedded engine on NVMe):

- **Inodes** — standard POSIX inode fields (mode, uid, gid, size, timestamps, link count) plus ClaudeFS extensions (content hash, EC stripe location, replication state, snapshot references)
- **Directory entries** — name-to-inode mappings, stored as sorted key-value pairs under the parent inode
- **Extended attributes (xattrs)** — arbitrary key-value metadata per inode, used for ACLs, S3 tiering hints, replication tags
- **Distributed locks** — per-inode read/write locks for POSIX mandatory locking (`fcntl`), per-directory locks for atomic rename/link

### Consistency Within a Site

Strong consistency within a single site using a Raft-based consensus protocol for metadata mutations:

- **Write path:** Client sends metadata operation to the owning node -> node proposes to its Raft group -> majority ack -> committed -> response to client
- **Read path:** Client reads from the owning node's committed state (linearizable) or from any replica (stale but fast, for relaxed POSIX mode)
- **Atomic operations:** `rename()`, `link()`, and cross-directory moves require distributed transactions across the source and destination directory owners. Two-phase commit with the Raft log as the durable store.
- **NVMe atomic writes (kernel 6.11+)** — Raft log entries are written atomically to NVMe, eliminating the need for double-writes or software WAL overhead for crash consistency.

### Metadata Caching

- **Client-side metadata cache** — the FUSE daemon caches inode attributes and directory entries locally. Cache invalidation via lease-based protocol: the server grants time-limited leases, client must re-validate after expiry.
- **Negative caching** — "file does not exist" results are cached to avoid repeated lookups for missing files (common in build systems and package managers).
- **Relaxed POSIX mode** — mount flag to extend lease durations and allow stale reads, trading consistency for metadata performance. Suitable for read-heavy workloads where files are rarely modified.

## 2. Cross-Site Replication

### The Journal-First Model

Every metadata change within a site is committed to a local metadata journal on NVMe (the Raft log). Cross-site replication tails this journal asynchronously:

```
Site A write path:     Client -> Metadata Node -> Raft Log (NVMe) -> Ack to client
                                                       |
                                                       v (async)
Replication path:      Replication Agent -> Cloud Conduit -> Site B Replication Agent
                                                                    |
                                                                    v
                                          Site B Metadata Node -> Apply + Conflict Check
```

**Critical design principle:** The local metadata path never waits for replication. Local `stat()`, `mkdir()`, `rename()` return at local NVMe speed regardless of WAN latency or conduit availability. Replication is a side-effect, not a prerequisite.

### The Replication Agent

A background Rust service on each site that:

1. **Tails the local Raft journal** — reads committed metadata operations in order
2. **Batches operations** — groups multiple small operations (e.g., 1000 file creates) into a single replication message to amortize network overhead
3. **Applies state compaction** — if a file is created and then deleted within the same batch window, the net effect is "nothing" — don't replicate intermediate states. This is the AsyncFS "scatter and aggregate" optimization: replicate final state, not every intermediate operation.
4. **Applies UID mapping** — translates user identities before sending (see section 3)
5. **Serializes as Protobuf** — compact binary format, not JSON. Metadata operations are tiny but numerous (millions per second); serialization overhead matters.
6. **Sends via the cloud conduit** — persistent connection to the relay server

### The Cloud Conduit

Both sites initiate **outbound connections only** — no firewall holes required at either site. A lightweight relay server hosted at a cloud provider brokers the connection.

**Why not a direct VPN?**

- Many HPC sites have strict firewall policies that prohibit inbound connections
- VPN setup requires coordination between two network teams at two institutions
- A cloud conduit using standard HTTPS/WebSocket traverses any corporate firewall

**Protocol choice: gRPC over HTTP/2 (not raw WebSocket)**

Gemini suggested WebSocket, but gRPC over HTTP/2 is the better fit:

- **Structured RPC** — metadata operations are request/response patterns (create, rename, delete), not free-form streams. gRPC's service definitions enforce schema discipline.
- **Backpressure** — HTTP/2 flow control prevents a fast site from overwhelming a slow site or the conduit. WebSocket has no built-in backpressure.
- **Bidirectional streaming** — gRPC supports server-push within the same HTTP/2 connection. Site B receives updates as soon as Site A uploads them.
- **mTLS** — gRPC natively supports mutual TLS. Both sites and the conduit authenticate each other with certificates. No shared secrets.
- **Protobuf native** — gRPC uses Protobuf serialization, which is already the wire format for replication messages.
- Rust crate: `tonic` (production-grade gRPC for Rust, built on `hyper` and `tokio`)

**Conduit architecture:**

```
Site A ──(outbound gRPC/mTLS)──> Cloud Conduit <──(outbound gRPC/mTLS)── Site B
                                      |
                                 Pairs connections
                                 by Cluster ID,
                                 buffers + forwards
                                 replication streams
```

The conduit is stateless except for a small buffer of in-flight replication messages. It does not store metadata persistently — it is a relay, not a backup. If the conduit is temporarily unavailable, sites queue replication locally and resume when connectivity returns.

**Alternative: QUIC (future consideration)**

QUIC (HTTP/3) offers multiplexed streams with independent flow control, which could allow metadata and data replication to share a single connection without head-of-line blocking. Worth evaluating once the Rust QUIC ecosystem (`quinn` crate) matures for production gRPC use.

### Conflict Resolution

With asynchronous replication, two sites may modify the same file or directory concurrently. ClaudeFS uses **last-write-wins (LWW)** with administrator alerting:

- **Vector clocks** — each metadata operation carries a Lamport timestamp (site ID + local sequence number). When Site B receives an update that conflicts with a local change, the higher timestamp wins.
- **Conflict detection** — if Site B has modified the same inode since the last sync point, the replication agent logs the conflict: which operation was overwritten, by whom, and when.
- **Administrator alert** — conflicts are reported via syslog, SNMP trap, or webhook. The administrator can review the conflict log and manually intervene if needed.
- **Conflict-free operations** — most POSIX operations are naturally conflict-free in research environments: different users work on different files. Conflicts are rare by design (the README's original insight: "researchers don't really share their data very much").

**What we don't do:** We do not attempt strong consistency across sites. Cross-site distributed locking would add WAN latency to every metadata operation, destroying local performance. The explicit tradeoff is: local performance is sacred, cross-site consistency is best-effort with human oversight.

### Replication Lag Monitoring

- **Lag metric:** the difference between the latest local journal sequence number and the last sequence number confirmed received by the remote site
- **Alerting thresholds:** configurable — e.g., alert if lag exceeds 10,000 operations or 60 seconds
- **Dashboard:** expose via Prometheus metrics for Grafana integration
- **Catch-up mode:** if a site was offline, the replication agent enters "catch-up" mode, streaming journal entries at maximum throughput until the lag is zero

## 3. UID/GID Mapping

### The Problem

Site A and Site B may have different user databases. Jimmy Joe is UID 1001 at Site A but UID 200001 at Site B. Files replicated from Site A to Site B must be owned by the correct local identity.

### Where Mapping Happens

**Correction to Gemini's recommendation:** Gemini suggests mapping at the source or relay. This is wrong for the general case. Mapping should happen at the **receiving site**, not the source:

- The source site stores and replicates **canonical UIDs** (the UID as it exists at the source)
- The receiving site's replication agent applies its local mapping table before writing to the local metadata store
- This allows N sites to each have their own mapping table without the source needing to know about every destination's UID space

If mapping happened at the source, the source would need a separate mapping table for every destination site — an O(N) scaling problem.

### Mapping Implementation

- **UID mapping table** — a persistent `HashMap<(SiteId, CanonicalUid), LocalUid>` stored in the local metadata database
- **GID policy** — no automatic mapping. Administrators at both sites coordinate a shared GID namespace. This is a deliberate simplification: GIDs represent organizational groups that should be consistent across sites. New shared groups can be created for cross-site collaboration.
- **Supplementary groups and ACLs** — replicated as-is using the canonical GIDs. Since GIDs are shared, no mapping is needed.
- **Root (UID 0)** — never mapped. Root-owned files at Site A remain root-owned at Site B. The local administrator manages root trust.

### Mapping Table Synchronization

- The mapping table is configured by the administrator at each site (not auto-discovered)
- Changes to the mapping table are applied to new replication operations going forward
- Bulk re-mapping of existing files requires an administrative "re-map" tool that walks the metadata store and updates ownership

## 4. Data Replication (Future)

The metadata replication infrastructure is designed to support data replication later:

- **CAS-based incremental sync** — content-addressed blocks that already exist at the remote site (same BLAKE3 hash) don't need to be transferred. Only new/modified blocks are sent.
- **Same conduit** — data replication uses the same cloud conduit, but on a separate gRPC stream (HTTP/2 multiplexing) to avoid head-of-line blocking with metadata
- **Priority:** metadata replication takes priority over data replication. Metadata is small and latency-sensitive; data is large and throughput-sensitive.
- **Consistency:** a file's metadata is replicated first, then its data blocks. The remote site sees the file in its directory listing immediately (with metadata) but reads may block until data arrives, or return an error if data is not yet available (configurable policy).

## 5. Related Literature

See [docs/literature.md](literature.md) for the full paper catalog. Key references for metadata:

- **Section 1** — InfiniFS (distributed POSIX metadata, speculative path resolution), Orion (RDMA-friendly inode structures)
- **Section 3** — DAOS (KV-based metadata with MVCC, dfuse POSIX overlay)

Additional references:

- **SwitchDelta** (ICDE 2026) — moving metadata updates out of the critical path using async buffers. Mirrors ClaudeFS's journal-first model where remote sync is a side-effect.
- **AsyncFS** (2025) — "scatter and aggregate" metadata replication: replicate final state, not intermediate operations. Directly informs ClaudeFS's batch compaction strategy.
- **IBM Spectrum Scale (GPFS) UID/GID Remapping** — industry standard for cross-site identity mapping. ClaudeFS adopts the "map at destination" model rather than GPFS's "map at source" approach.
- **Raft consensus** — Diego Ongaro's dissertation. The foundation for ClaudeFS's intra-site metadata consistency.
