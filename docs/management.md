# Management and Monitoring

ClaudeFS separates real-time monitoring (telemetry, health) from analytical metadata search (who owns what, where is space consumed). These are fundamentally different workloads with different latency and consistency requirements.

## Architecture: Three Daemons

| Daemon | Role | Priority | Technology |
|--------|------|----------|------------|
| **claudefs-monitor** | Real-time telemetry, hardware health, alerting | High (real-time) | Rust + Prometheus exporter |
| **claudefs-index** | Metadata indexer, tails the metadata journal, writes Parquet | Medium (batch) | Rust + Arrow/Parquet |
| **claudefs-admin** | Query gateway: Web UI, CLI, SQL interface | Low (on-demand) | Rust + DuckDB + Axum |

All three run as separate processes on management nodes (or co-located on storage nodes with resource limits). None are on the I/O hot path — the storage engine never waits for monitoring or indexing.

## Real-Time Monitoring (claudefs-monitor)

### Metrics Collection

Each ClaudeFS node exposes a Prometheus-compatible `/metrics` endpoint:

- **I/O metrics** — IOPS, throughput (MB/s), latency histograms (p50/p99/p999) per node, per drive, per client
- **NVMe health** — endurance remaining, media errors, temperature, spare capacity (via NVMe SMART log pages through io_uring passthrough)
- **RDMA fabric** — port counters, retransmits, congestion events (from libfabric counters)
- **Metadata engine** — Raft commit latency, log size, replication lag to remote site (sequence number delta)
- **Data reduction** — inline dedupe hit rate, compression ratio, similarity pipeline throughput
- **S3 tiering** — queue depth, flush latency, object store error rate
- **Cluster membership** — node health, drive status, capacity utilization per node

### Alerting

- Prometheus Alertmanager for threshold-based alerts (drive failure, replication lag > 60s, capacity > 90%)
- Cross-site replication conflict alerts (from metadata service) forwarded as Prometheus alerts
- Integration with PagerDuty, Slack, email, SNMP traps via standard Alertmanager receivers

### Visualization

- Grafana dashboards with pre-built panels for cluster health, per-node performance, and capacity trends
- ClaudeFS ships Grafana dashboard JSON as part of the distribution — zero-configuration monitoring on first deploy

## Metadata Search (claudefs-index + claudefs-admin)

### The Problem

Standard POSIX metadata (inodes, directory entries) is optimized for path traversal, not analytical queries. Questions like "which user owns the most data?" or "find all files named `*.h5` modified in the last week" require a full tree walk — O(n) over billions of files.

### The Solution: Storage Lakehouse

Extract metadata into Parquet columnar format and query with DuckDB. This avoids building a separate always-on relational database while providing full SQL over the filesystem namespace.

### Indexing Pipeline (claudefs-index)

Instead of walking the filesystem tree (slow, disruptive to I/O), the indexer **tails the metadata journal** — the same Raft log that the replication agent reads:

1. **Journal tailer** — reads committed metadata operations (create, delete, rename, chmod, write) from the local Raft log in real time
2. **State accumulator** — maintains a current-state view of the namespace in memory (inode -> path, owner, size, timestamps)
3. **Parquet writer** — periodically flushes the accumulated state to Parquet files with Hive-style partitioning:

```
/index/year=2026/month=02/day=28/metadata_00001.parquet
```

**Parquet schema:**

| Column | Type | Description |
|--------|------|-------------|
| `inode` | uint64 | Inode number |
| `path` | string | Full path |
| `filename` | string | Basename (for fast name search) |
| `parent_path` | string | Directory path (for folder aggregation) |
| `owner_uid` | uint32 | File owner UID |
| `owner_name` | string | Resolved username (from passwd/LDAP) |
| `group_gid` | uint32 | File group GID |
| `group_name` | string | Resolved group name |
| `size_bytes` | uint64 | File size |
| `blocks_stored` | uint64 | Actual storage after dedupe + compression |
| `mtime` | timestamp | Last modification time |
| `ctime` | timestamp | Last metadata change time |
| `file_type` | string | Extension or MIME type |
| `is_replicated` | bool | Cross-site replication complete |

**Why Parquet:**

- Columnar compression — metadata for billions of files fits in a few GB on disk
- Predicate pushdown — DuckDB skips entire row groups when filtering by date, owner, or path prefix
- Zero-copy reads — DuckDB queries Parquet directly via memory-mapping, no ETL step
- Standard format — interoperable with Pandas, Spark, Polars for external analysis

### Query Gateway (claudefs-admin)

A lightweight HTTP server providing three interfaces to the same DuckDB engine:

**SQL via CLI:**

```bash
claudefs-admin query "SELECT owner_name, SUM(size_bytes) as total
  FROM 'index/**/*.parquet'
  GROUP BY owner_name ORDER BY total DESC LIMIT 20"
```

**SQL via Web UI:**

- Axum-based web server with embedded DuckDB-WASM for browser-side query execution
- Pre-built dashboards: top users by space, largest directories, file type distribution, stale data report
- Authentication via the cluster's existing credential system

**Pre-built queries (CLI shortcuts):**

```bash
claudefs-admin top-users              # Space consumption by user
claudefs-admin top-dirs --depth 3     # Largest directories
claudefs-admin find "*.h5"            # Filename search
claudefs-admin stale --days 180       # Files not accessed in 6 months
claudefs-admin reduction-report       # Dedupe/compression savings by directory
```

**Advanced joins:**

DuckDB can join Parquet metadata with external data sources in a single query:

```sql
-- Join filesystem metadata with an HR CSV to get department-level storage usage
SELECT d.department, SUM(f.size_bytes) as total
FROM 'index/**/*.parquet' f
JOIN 'hr_directory.csv' d ON f.owner_name = d.username
GROUP BY d.department ORDER BY total DESC
```

### Consistency Model

The metadata search index is **eventually consistent** — typically seconds behind the live filesystem state:

- Journal tailer reads committed operations with minimal lag
- Parquet flushes happen on a configurable interval (default: 60 seconds)
- The Web UI displays the index timestamp so users know how fresh results are
- For point queries ("does file X exist right now?"), use the live metadata service, not the index

### Scaling

- One `claudefs-index` instance per site indexes the local metadata journal
- For very large clusters (>1 billion files), partition the Parquet output by path hash — DuckDB parallelizes reads across partitions
- The index is read-only from the query gateway's perspective — no locking, no contention with the storage engine

## Capacity Planning

The monitoring and indexing daemons also feed capacity planning:

- **Growth trends** — Grafana panels showing storage consumption over time, projected full date
- **Reduction effectiveness** — inline dedupe hit rate, compression ratio trends, similarity pipeline savings
- **Tiering efficiency** — ratio of hot (flash) vs cold (S3) data, tiering queue backlog
- **Per-user/group quotas** — queryable via DuckDB; enforcement via the metadata service (future feature)
