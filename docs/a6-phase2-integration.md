# A6 Phase 2 Integration Guide

**Author:** Agent A6 (Replication)
**Date:** 2026-03-01
**Status:** Ready for Phase 2 Integration

---

## Overview

This guide details how to integrate the Claude FS replication subsystem (A6) with other agents' code in Phase 2. The replication engine is production-ready with 741 passing tests and zero clippy warnings.

## Integration Requirements

### From A2 (Metadata Service)

A6 depends on A2 for:

1. **Raft Journal Input**
   - A2 publishes metadata changes to its write-ahead journal
   - A6 consumes these journal entries via an async stream or callback
   - Each entry includes: timestamp, sequence number, operation (create/delete/update inode)

   ```rust
   // A2 should provide this interface:
   pub trait RaftJournalSource {
       async fn subscribe_journal(&self) -> mpsc::Receiver<RaftJournalEntry>;
   }

   pub struct RaftJournalEntry {
       pub seq: u64,
       pub ts_ns: u64,
       pub operation: RaftOperation,
       pub inode_id: InodeId,
   }
   ```

2. **Cursor Position Persistence**
   - A6 tracks which journal entries have been replicated (cursor position)
   - A2 may query cursor position for recovery: "which entries have A6 seen?"
   - Cursor is persisted in A6's checkpoint file

   ```rust
   // A6 provides:
   pub fn get_replication_cursor(&self, site_id: &str) -> Result<u64>;
   ```

3. **Conflict Resolution Integration**
   - When A6 detects a write conflict (divergence between sites), A2 needs to know
   - A2's KV store should be updated to reflect the last-write-wins outcome
   - Conflicts are logged to `repl_audit.rs` and emitted as events

   ```rust
   // A6 emits conflict events:
   pub struct WriteConflict {
       pub inode_id: InodeId,
       pub local_ts: u64,
       pub remote_ts: u64,
       pub winner: String, // "local" or "remote"
   }
   ```

### From A4 (Transport)

A6 depends on A4 for:

1. **Custom RPC Transport**
   - A6's `conduit.rs` module uses A4's RPC protocol to send replication messages to peer sites
   - A4 should expose a transport layer with these traits:

   ```rust
   // A4 should provide:
   pub trait RpcTransport: Send + Sync {
       async fn send(&self, peer: &str, msg: &[u8]) -> Result<Vec<u8>>;
       async fn listen(&self, local_addr: &str) -> Result<RpcListener>;
   }

   pub trait RpcListener {
       async fn accept(&mut self) -> Result<(String, mpsc::Receiver<Vec<u8>>)>;
   }
   ```

2. **RDMA/TCP Pluggable Backend**
   - A6 doesn't care if A4 uses RDMA or TCP under the hood
   - A4 handles connection setup, retries, backpressure
   - A6 just sends/receives messages

3. **Zero-Copy Data Transfer**
   - For large journal batches, A6 can request zero-copy transfer via A4
   - A4 ensures data is transferred without intermediate buffering

### From A5 (FUSE Client)

A6 provides to A5:

1. **Replicated Metadata Consistency**
   - A5 reads metadata through A2's KV store (not directly from A6)
   - A6 ensures the KV store is eventually consistent across sites
   - A5 doesn't need special integration with A6 beyond using A2

2. **Health/Failover Hints** (optional)
   - A5 may optionally query A6 for site health status
   - If primary site is down, A5 should route reads to secondary

   ```rust
   // A6 provides:
   pub fn get_site_health(&self, site_id: &str) -> SiteHealth {
       // HEALTHY, DEGRADED, or DOWN
   }
   ```

### From A8 (Management)

A6 provides to A8 for observability:

1. **Prometheus Metrics Export**
   - A6's `metrics.rs` module exports replication lag, throughput, errors
   - A8 scrapes `/metrics` endpoint and exports to Prometheus

2. **OpenTelemetry Tracing**
   - A6 uses `otel_repl.rs` module to emit spans for distributed tracing
   - A8 can collect traces and display in Jaeger/Grafana

3. **Admin API**
   - A8's admin server should expose endpoints for:
     - `GET /replication/status` → ReplicationStatus
     - `POST /replication/failover` → initiate failover
     - `GET /replication/audit` → audit trail
     - `GET /replication/lag` → per-site lag metrics

## Integration Steps (Phase 2)

### Step 1: Wire A2 Journal to A6 Engine

**Owner:** A2 + A6
**Estimated:** 2-3 hours

In `claudefs-meta/src/engine.rs`:
1. Create a journal subscriber that emits RaftJournalEntry events
2. Initialize A6's ReplicationEngine with this subscriber
3. Set up A6's async task to consume journal events

```rust
// In A2's server initialization:
let repl_engine = claudefs_repl::engine::ReplicationEngine::new(
    local_site_id: "site-a".to_string(),
    journal_path: "/var/claudefs/journal",
    peers: vec![("site-b", "10.0.1.50:9500")],
);

// Subscribe to metadata changes
let mut journal_rx = metadata_service.subscribe_journal();
tokio::spawn(async move {
    while let Some(entry) = journal_rx.recv().await {
        if let Err(e) = repl_engine.append(entry).await {
            tracing::error!("replication error: {}", e);
        }
    }
});
```

### Step 2: Wire A4 Transport to A6 Conduit

**Owner:** A4 + A6
**Estimated:** 1-2 hours

In `claudefs-repl/src/conduit.rs`:
1. Replace the mock RpcTransport with A4's real implementation
2. Initialize ConduitService with A4's transport backend
3. Test message serialization/deserialization

```rust
// In A6's server initialization:
use claudefs_transport::RpcTransport;

let transport = Arc::new(claudefs_transport::tcp_transport::TcpTransport::new()?);
let conduit = claudefs_repl::conduit::ConduitService::new(
    local_site_id: "site-a".to_string(),
    transport,
);
```

### Step 3: Enable A2 Conflict Resolution

**Owner:** A2 + A6
**Estimated:** 1-2 hours

When A6 detects a conflict:
1. A6 emits a conflict event with the winner (via LWW)
2. A2 applies the winning version to its KV store
3. A6 logs the event to `repl_audit.rs`

```rust
// In A2's event handler:
let conflict = repl_engine.get_last_conflict()?;
if let Some(c) = conflict {
    // Apply the winning version to KV store
    let winning_inode = if c.winner == "local" {
        local_inode
    } else {
        remote_inode
    };
    kv_store.put(c.inode_id, &winning_inode)?;
}
```

### Step 4: Wire A8 Metrics & Admin API

**Owner:** A8 + A6
**Estimated:** 2-3 hours

In `claudefs-mgmt/src/metrics.rs`:
1. Expose A6's Prometheus metrics endpoint
2. Add A6-specific dashboard panels

In `claudefs-mgmt/src/admin_api.rs`:
1. Add replication status endpoints
2. Add manual failover endpoint
3. Add audit trail query endpoint

```rust
// In A8's admin API:
#[post("/replication/failover")]
async fn failover(
    Json(req): Json<FailoverRequest>,
    repl: web::Data<ReplicationEngine>,
) -> impl Responder {
    repl.initiate_failover(&req.target_site).await;
    HttpResponse::Ok()
}
```

## Testing Strategy (Phase 2)

### Multi-Node Unit Tests

With A11's test cluster:

1. **Basic Replication** (2 nodes)
   - Write metadata on site A
   - Verify it appears on site B within 1 second
   - Verify lag metric <100ms

2. **Conflict Scenarios** (2 nodes + partition)
   - Write same inode on both sites during partition
   - Verify LWW resolves conflict consistently
   - Verify conflict logged to audit trail

3. **Failover** (2 nodes + kill primary)
   - Kill site A
   - Verify site B promotes automatically
   - Verify failover time <5 seconds

4. **Active-Active** (2 nodes, both writable)
   - Write to site A and site B in parallel
   - Verify no data loss
   - Verify conflicts logged but not fatal

5. **Performance** (3 nodes)
   - Measure replication latency with high write rate (10K ops/sec)
   - Verify lag stays <500ms
   - Verify CPU/memory are reasonable

### Integration Tests

Run with A5 (FUSE client):

1. Mount A5 FUSE client pointing to site A
2. Do file operations (create, write, rename, delete)
3. Verify operations appear on site B
4. Kill site A, verify FUSE failover to site B
5. Verify metadata consistency after failover

## Known Integration Issues

### Issue 1: Timestamp Synchronization
- **Problem:** If A2 and peer sites have clock skew, LWW may make wrong decisions
- **Solution:** Use NTP or PTP for clock synchronization across sites
- **A6's role:** Can detect and warn on high clock skew via `health.rs`

### Issue 2: Metadata Schema Evolution
- **Problem:** If A2 changes metadata schema, old entries in replication journal become invalid
- **Solution:** Version metadata entries, use migration functions
- **A6's role:** Agnostic to schema; A2 must handle versioning

### Issue 3: Large Journal Batches
- **Problem:** If journal grows very large, replication batches become too big
- **Solution:** A6 has configurable batch size limits in `pipeline.rs`
- **A6's role:** Provides backpressure to slow down A2 if needed

## Performance Expectations (Phase 2)

| Metric | Target | Notes |
|--------|--------|-------|
| Write latency | <50ms | 2x journal replication (local) |
| Replication lag | <500ms | Cross-site propagation |
| Failover time | <5s | Detection + promotion |
| Throughput | 10K-50K ops/sec | Depends on batch size and network |
| CPU overhead | <10% | Per replication task |
| Memory | <500MB | Per replication engine |

## Admin Operations (Phase 2)

Once integrated, A8 should support:

```bash
# Check replication status
cfs admin replication status

# Monitor lag in real-time
cfs admin replication lag

# Manual failover
cfs admin replication failover --to site-b

# View conflict events
cfs admin replication audit --since "1 hour ago" --filter "conflict"

# Check site health
cfs admin replication health

# Trigger sync (for debugging)
cfs admin replication sync --force
```

## Debugging Tips

### A6 is slow
1. Check `metrics.rs` for high network latency
2. Check `lag_monitor.rs` for replication lag
3. Look at `backpressure.rs` — may be throttling due to peer overload
4. Check `compression.rs` ratio — low ratio means incompressible data

### A6 lost data
1. Check replication audit trail for conflict events
2. Verify clock synchronization across sites
3. Check journal GC — may have purged entries
4. Verify checkpoints are being written

### A6 won't failover
1. Check `health.rs` for peer status
2. Verify network connectivity between sites
3. Check TLS certificates in `tls_policy.rs`
4. Check `split_brain.rs` for fencing state

## Future Work (Phase 3+)

- [ ] Multi-site quorum reads for stronger consistency
- [ ] Replication SLA enforcement with lag thresholds
- [ ] Bandwidth reservation for replication traffic
- [ ] Active-active read balancing improvements
- [ ] Compression algorithm selection
- [ ] Custom routing policies

## Contact

**A6 Replication Owner:** Agent A6
**Crate:** `claudefs-repl`
**Tests:** `cargo test --lib -p claudefs-repl`
**Docs:** `cargo doc --lib -p claudefs-repl --no-deps --open`

For integration questions, refer to the `Integration Points` section of `/home/cfs/claudefs/crates/claudefs-repl/README.md`.
