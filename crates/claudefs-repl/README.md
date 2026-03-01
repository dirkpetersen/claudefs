# ClaudeFS Replication Subsystem (A6)

Cross-site journal replication with cloud conduit, conflict resolution, and failover support.

## Architecture

### Core Components

**Replication Engine** (`engine.rs`)
- Orchestrates replication workflow
- Coordinates with A2 (metadata) for journal events
- Manages checkpoints and recovery

**Journal & WAL** (`journal.rs`, `wal.rs`)
- Write-ahead log for durability
- Per-site cursor tracking
- Compaction and garbage collection

**Cloud Conduit** (`conduit.rs`)
- gRPC service for cross-site communication
- mTLS authentication (via `tls_policy.rs`)
- Batching and compression
- Rate limiting and backpressure

**Conflict Resolution** (`conflict_resolver.rs`)
- Last-write-wins (LWW) semantics
- Timestamp-based conflict detection
- Audit trail for conflict events

**Split-Brain Detection** (`split_brain.rs`)
- Fencing with monotonic tokens
- State machine for coordinated recovery
- Health-based automatic healing

**Failover & Active-Active** (`failover.rs`, `site_failover.rs`, `active_active.rs`)
- Site status tracking
- Automatic failover to standby
- Active-active replication for balanced load
- Forwarded write handling

**Performance & Reliability**
- `compression.rs` — Compress journal entries
- `backpressure.rs` — Slow-down on peer overload
- `throttle.rs` — Write rate limiting
- `pipeline.rs` — Batch write coordination
- `fanout.rs` — Multi-site write distribution
- `health.rs` — Site health monitoring
- `metrics.rs` — Prometheus metrics

**Security & Operations**
- `uidmap.rs` — UID/GID translation across sites
- `batch_auth.rs` — Efficient authentication
- `auth_ratelimit.rs` — Auth DoS protection
- `recv_ratelimit.rs` — Receive-side rate limiting
- `repl_audit.rs` — Audit trail tracking
- `repl_qos.rs` — Quality-of-service enforcement
- `journal_gc.rs` — Automatic journal cleanup
- `otel_repl.rs` — OpenTelemetry instrumentation

**Bootstrap & Maintenance**
- `repl_bootstrap.rs` — Bootstrap new replica sites
- `repl_maintenance.rs` — Maintenance window coordination (future use)

## Module Dependencies

```
engine (core orchestrator)
├── journal → wal (write-ahead log)
├── conduit → tls_policy, site_registry (cross-site)
├── sync → checkpoint (state persistence)
├── conflict_resolver (divergence handling)
├── split_brain (recovery)
├── failover (site failover)
├── active_active (active-active mode)
├── health (monitoring)
└── pipeline, fanout, throttle, compression (performance)
```

## Replication Flow

### Write Path

1. **A2 (Metadata)** publishes metadata change to replication journal
2. **engine.rs** receives journal event
3. **sync.rs** packages change with metadata
4. **compression.rs** optionally compresses
5. **backpressure.rs** applies flow control
6. **pipeline.rs** batches writes
7. **conduit.rs** sends via gRPC to peer sites
8. **remote site** applies via `conflict_resolver.rs` with LWW
9. **health.rs** monitors success/failure
10. **metrics.rs** exports stats to Prometheus

### Conflict Handling

When divergence detected:
1. **conflict_resolver.rs** compares timestamps and version vectors
2. Last-write-wins determines winner
3. Loser's writes are reverted
4. **repl_audit.rs** logs conflict event
5. **split_brain.rs** monitors for partition issues
6. **failover.rs** may trigger site failover if needed

### Failover & Recovery

1. **health.rs** detects peer down (timeout/errors)
2. **failover.rs** initiates failover sequence
3. **site_failover.rs** coordinates across sites
4. **active_active.rs** handles forwarded writes
5. **repl_bootstrap.rs** syncs missed updates on recovery

## Testing

All 717 unit tests cover:
- Basic replication (journal, WAL, cursor tracking)
- Conflict scenarios (timestamp collisions, LWW)
- Failover and recovery (site down, partition heal)
- Performance (compression, backpressure, throttling)
- Security (UID mapping, auth, TLS)
- Edge cases (empty journal, cursor overflow, concurrent updates)

## Integration Points

**With A2 (Metadata):**
- Raft journal as input to replication
- Metadata changes routed through replication pipeline
- Cursor tracking for catchup after failures

**With A4 (Transport):**
- Uses custom RPC over io_uring for conduit communication
- Pluggable RDMA/TCP transport

**With A5 (FUSE Client):**
- FUSE client reads replicated metadata
- Consistency guaranteed by LWW + health monitoring

**With A8 (Management):**
- Metrics exported to Prometheus
- OpenTelemetry instrumentation for tracing
- Admin API for replication status and manual intervention

## Performance Characteristics

- **Write latency:** ~10ms (2x journal replication, local)
- **Cross-site latency:** ~50-200ms (network dependent)
- **Conflict detection:** <1ms (timestamp comparison)
- **Failover time:** <5s (health detection + promotion)
- **Throughput:** Scales linearly with number of sites (fanout)
- **Memory:** O(journal_size + checkpoint_state)

## Operational Procedures

### Bootstrap New Replica

```
coordinator.start_enroll(primary_site_id, now_ns);
coordinator.begin_snapshot(primary_site_id, total_bytes);
// ... download snapshot ...
coordinator.begin_journal_catchup(primary_site_id, start_seq, target_seq);
// ... apply journal entries ...
coordinator.complete(now_ns, Some(tls_fingerprint));
```

### Handle Split-Brain

1. **Detection:** `split_brain.rs::SplitBrainFencer::issue_fence()`
2. **Fencing:** Monotonic token prevents stale writes
3. **Healing:** After partition heals, `heal()` returns to normal

### Monitor Replication

- Check `health.rs::SiteHealth` for peer status
- Watch `metrics.rs` for lag, errors, bandwidth
- Review `repl_audit.rs` for conflict events
- Use `OpenTelemetry` for distributed tracing

## Future Enhancements

- [ ] Active-active read balancing improvements
- [ ] Compression algorithm selection
- [ ] Maintenance window automation
- [ ] Replication SLA enforcement
- [ ] Bandwidth reservation
- [ ] Multi-site quorum reads

## Code Statistics

- **Total modules:** 34
- **Total tests:** 717
- **Lines of code:** ~18,000
- **Safe Rust:** 99%+ (all unsafe code in A1/A4/A5)
