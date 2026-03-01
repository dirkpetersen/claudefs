# A6 Replication — Algorithms Deep Dive

**Author:** Agent A6 (Replication)
**Date:** 2026-03-01
**Audience:** Architects, advanced operators, researchers
**Purpose:** Document algorithms, trade-offs, and design decisions

---

## Core Algorithms

### 1. Last-Write-Wins (LWW) Conflict Resolution

**Problem:** When two sites write to the same inode simultaneously during a partition, both versions are valid. Which one wins?

**Solution:** LWW compares logical timestamps (not wall-clock time) to determine the winner.

**Algorithm:**
```
function resolve_conflict(local_version, remote_version):
  if local_version.ts > remote_version.ts:
    winner = local_version
  elif remote_version.ts > local_version.ts:
    winner = remote_version
  else:
    // Tie: use site_id as tiebreaker
    winner = (local_site_id > remote_site_id) ? local_version : remote_version

  audit_log("conflict", local_version, remote_version, winner)
  return winner
```

**Trade-offs:**
- ✅ **Pro:** Simple, fast (<1ms), deterministic
- ✅ **Pro:** Works offline (no coordination needed)
- ❌ **Con:** May lose recent writes (if clocks are skewed)
- ❌ **Con:** Tie-breaking by site_id is arbitrary

**Improvements (Phase 3+):**
- Use version vectors for causal ordering (requires more metadata)
- Use Lamport clocks with explicit synchronization
- Per-field LWW (different fields have different winners)

**Code location:** `conflict_resolver.rs::`

---

### 2. Version Vectors (Causality Tracking)

**Problem:** LWW doesn't capture causality. If A→B→A, should the last write from A really win?

**Solution:** Version vectors track causal dependencies.

**Algorithm:**
```
// For each site, track highest seen sequence number
struct VersionVector {
  site_a: u64,
  site_b: u64,
  site_c: u64,
}

function happened_before(vv1, vv2) -> bool:
  // vv1 happened before vv2 if:
  // - vv1 ≤ vv2 in all components, AND
  // - vv1 < vv2 in at least one component

  for each site in ALL_SITES:
    if vv1[site] > vv2[site]:
      return false  // vv1 is newer in at least one dimension

  return vv1 != vv2

function causally_related(vv1, vv2) -> bool:
  return happened_before(vv1, vv2) or happened_before(vv2, vv1)
```

**Example:**
```
Write A on site-a (ts=1): vv = [1, 0, 0]
Write B on site-b (ts=2): vv = [1, 1, 0]
Write C on site-a (ts=3): vv = [2, 1, 0]

C happened after B, because vv_C > vv_B in all dimensions.
So write C should win over B.
```

**Trade-offs:**
- ✅ **Pro:** Captures causality, prevents lost updates
- ❌ **Con:** Requires per-site version tracking
- ❌ **Con:** Overhead grows with number of sites (O(n) metadata per write)

**Optimization:** Prune old entries (from inactive sites)

**Code location:** `conflict_resolver.rs::VersionVector`

---

### 3. Split-Brain Detection: Fencing Tokens

**Problem:** If network partitions into 2 halves and both think they're primary, you get split-brain. Both halves can accept writes, causing data divergence.

**Solution:** Monotonic fencing tokens ensure only one half can write at a time.

**Algorithm:**
```
struct FencingToken {
  epoch: u64,        // Incremented on each fencing event
  site_id: String,   // Site that holds the token
  timestamp_ns: u64,
}

function issue_fence(new_site) -> FencingToken:
  epoch = atomic_increment(current_epoch)
  return FencingToken {
    epoch: epoch,
    site_id: new_site,
    timestamp_ns: now(),
  }

function can_write(token: FencingToken) -> bool:
  // This write is only valid if it has the current token
  return token.epoch == current_token.epoch and
         token.site_id == current_token.site_id
```

**Example:**
```
Initial: token = [epoch=1, site_id="site-a"]

Network partition: site-a and site-b split
- site-b thinks site-a is down, issues new token: [epoch=2, site_id="site-b"]
- Conflict detection on site-b recognizes higher epoch, accepts writes

- site-a still has old token [epoch=1], all writes REJECTED by peers
- When partition heals, site-a gets new token [epoch=2]
- Data on site-a is discarded (if different from site-b)
```

**Trade-offs:**
- ✅ **Pro:** Prevents data corruption from split-brain
- ✅ **Pro:** Automatic (no admin intervention needed)
- ❌ **Con:** Requires external arbiter (e.g., quorum) to issue tokens
- ❌ **Con:** Not Byzantine-fault-tolerant (single arbiter can be compromised)

**Code location:** `split_brain.rs::SplitBrainFencer`

---

### 4. Journal Cursor Tracking: Exactly-Once Delivery

**Problem:** If replication message is lost, sender and receiver disagree on what's been replicated. Retransmitting could cause duplicates.

**Solution:** Maintain per-site cursor (sequence number) of last replicated entry.

**Algorithm:**
```
struct JournalCursor {
  site_id: String,
  last_acked_seq: u64,    // Highest sequence acked by this site
  checkpoint_path: String, // Persisted to survive crashes
}

function replicate(entries: Vec<JournalEntry>):
  for entry in entries:
    if entry.seq <= cursor.last_acked_seq:
      continue  // Already acked, skip

    send_to_remote(entry)

    // Only increment cursor after remote ack
    on_ack(entry.seq):
      cursor.last_acked_seq = entry.seq
      persist_cursor()

function on_recover():
  cursor = load_cursor_from_checkpoint()
  // Resume from cursor.last_acked_seq + 1
```

**Example:**
```
Cursor for site-b: [seq=1000]

Send entries 1001, 1002, 1003
- site-b acks 1001 → cursor = 1001
- site-b acks 1002 → cursor = 1002
- Network failure, 1003 lost

On resume:
- Cursor = 1002, so start from entry 1003 (not 1001!)
- Send 1003, get ack, cursor = 1003
```

**Trade-offs:**
- ✅ **Pro:** Guarantees exactly-once delivery (no duplicates, no loss)
- ✅ **Pro:** Recovers automatically from network failures
- ❌ **Con:** Requires persistent cursor (I/O overhead)
- ❌ **Con:** Slower than at-most-once (must wait for acks)

**Optimization:** Batch acks (acknowledge N entries in one message)

**Code location:** `wal.rs::JournalCursor`, `checkpoint.rs`

---

### 5. Compression: Reducing Bandwidth

**Problem:** Cross-site replication can consume lots of bandwidth. If entries are compressible, we can save network cost.

**Solution:** Optionally compress journal entries before sending.

**Algorithm:**
```
enum CompressionAlgorithm {
  None,
  LZ4,      // Fast (100-500 MB/s), lower ratio (40-60%)
  Zstd,     // Slower (50-100 MB/s), better ratio (30-50%)
}

function compress(entries: Vec<u8>, algo: CompressionAlgorithm) -> Vec<u8>:
  match algo:
    None => return entries
    LZ4 => return lz4_compress(entries)
    Zstd => return zstd_compress(entries, level=3)  // balance speed/ratio

function should_compress(data_size: usize, algo: CompressionAlgorithm) -> bool:
  // Only compress if:
  // 1. Algorithm selected
  // 2. Data large enough to overcome framing overhead (>1KB)
  // 3. Estimated compression ratio would save bandwidth

  return algo != None and
         data_size > 1024 and
         estimate_compressed_size(data_size) < data_size * 0.9
```

**Trade-offs:**
- ✅ **Pro:** Can reduce bandwidth by 30-60%
- ✅ **Pro:** Configurable (no compression for incompressible data)
- ❌ **Con:** CPU overhead (especially Zstd)
- ❌ **Con:** Adds latency (microseconds to milliseconds)

**Heuristics:**
- Use LZ4 for latency-sensitive paths
- Use Zstd for overnight batch replication
- Disable for already-compressed data (video, archives)

**Code location:** `compression.rs`

---

### 6. Backpressure: Flow Control

**Problem:** If remote site is slow or overloaded, replication can queue up unbounded, consuming local memory.

**Solution:** Apply backpressure (slow down sends) when remote queue is full.

**Algorithm:**
```
struct BackpressureControl {
  max_in_flight: usize,        // Max entries sent but not acked
  current_in_flight: usize,
  backpressure_threshold: f64, // e.g., 0.8
}

async function send_with_backpressure(entry: JournalEntry):
  loop:
    if current_in_flight < max_in_flight * backpressure_threshold:
      send(entry)
      current_in_flight += 1
      break
    else:
      // Backpressure: wait for some acks
      wait_for_any_ack()
      current_in_flight -= 1

on_remote_ack(seq: u64):
  current_in_flight -= 1
```

**Example:**
```
max_in_flight = 100
backpressure_threshold = 0.8

Send 79 entries: OK, no backpressure
Send 80th entry: in_flight=80, 80 > 100*0.8 = 80, apply backpressure
Wait for ack, in_flight drops to 79, continue

Result: Maintains ~79 entries in flight, never exceeds ~100
```

**Trade-offs:**
- ✅ **Pro:** Prevents memory explosion
- ✅ **Pro:** Fair (don't starve primary when replica is slow)
- ❌ **Con:** Adds latency (stalls sender while waiting)
- ❌ **Con:** Cascading backpressure (primary stalls A2 metadata)

**Tuning:**
- `max_in_flight`: Higher = more throughput, more memory. Typical: 100-1000
- `backpressure_threshold`: Higher = more latency variance. Typical: 0.7-0.9

**Code location:** `backpressure.rs`

---

### 7. Rate Limiting: Auth DoS Protection

**Problem:** Replication service receives RPC calls from remote sites. Malicious or buggy site could flood with requests, causing DoS.

**Solution:** Rate limit per remote site, per auth principal.

**Algorithm:**
```
struct RateLimiter {
  tokens_per_second: usize,
  buckets: HashMap<RemoteId, TokenBucket>,
}

struct TokenBucket {
  tokens: f64,           // Current tokens (float for precision)
  last_refill: Instant,
}

async function check_rate_limit(remote_id: &str) -> Result<()>:
  bucket = buckets.entry(remote_id).or_insert(TokenBucket::new())

  now = Instant::now()
  elapsed = (now - bucket.last_refill).as_secs_f64()
  bucket.tokens += elapsed * tokens_per_second
  bucket.tokens = min(bucket.tokens, max_tokens)  // Cap at burst size
  bucket.last_refill = now

  if bucket.tokens >= 1.0:
    bucket.tokens -= 1.0
    return Ok(())
  else:
    return Err(RateLimitExceeded)
```

**Example:**
```
tokens_per_second = 1000
burst_size = 5000

Remote site sends 6000 requests in 1 second:
- First 5000: OK (burst allowance)
- Next 1000: OK (1000 tokens/sec)
- Remaining 0: REJECTED

After 1 second of no traffic:
- Bucket refilled to 1000 tokens (then capped at 5000)
```

**Trade-offs:**
- ✅ **Pro:** Prevents DoS from single remote
- ✅ **Pro:** Fair queueing (high-traffic sites don't starve low-traffic)
- ❌ **Con:** Rejected requests must be retried (adds complexity)
- ❌ **Con:** Tuning `tokens_per_second` is workload-dependent

**Code location:** `auth_ratelimit.rs`, `recv_ratelimit.rs`

---

### 8. Batch Processing: Amortizing RPC Overhead

**Problem:** Sending each journal entry individually = N RPC calls per N entries = high latency.

**Solution:** Batch multiple entries into one RPC call.

**Algorithm:**
```
struct BatchBuffer {
  entries: Vec<JournalEntry>,
  size_bytes: usize,
  created_at: Instant,
}

async function append_to_batch(entry: JournalEntry):
  batch.entries.push(entry)
  batch.size_bytes += entry.len()

  should_flush = batch.size_bytes >= batch_size_threshold or
                 batch.created_at.elapsed() >= batch_timeout

  if should_flush:
    flush_batch()

function flush_batch():
  compressed = compress(batch.entries)
  send_rpc("replicate", compressed)
  wait_for_ack()
  batch.clear()
```

**Example:**
```
batch_size_threshold = 1MB
batch_timeout = 100ms

Scenario 1: Many small entries
- Accumulate ~10K entries (100KB each)
- Reach 1MB threshold in ~100ms, flush
- Result: 1 RPC call per ~10K entries

Scenario 2: Large entries
- 10 entries × 100KB = 1MB
- Flush immediately when 10th entry arrives
- Result: 1 RPC call per 10 entries

Scenario 3: Idle cluster
- 1 entry arrives
- Wait 100ms timeout
- If no more entries, flush single entry
- Result: 1 RPC call per entry (acceptable because cluster is idle)
```

**Trade-offs:**
- ✅ **Pro:** Reduces RPC overhead (1 call per 100 entries vs per entry)
- ✅ **Pro:** Higher throughput (100x better in some cases)
- ❌ **Con:** Increases latency (must wait for batch to fill)
- ❌ **Con:** Complex tuning (batch_size vs batch_timeout trade-off)

**Code location:** `pipeline.rs`

---

### 9. Health Monitoring: Detecting Failures

**Problem:** How do you know if a remote site is down? TCP connections can hang indefinitely.

**Solution:** Active health probes with timeout-based detection.

**Algorithm:**
```
enum SiteHealth {
  HEALTHY,   // Last probe succeeded <1s ago
  DEGRADED,  // Last probe succeeded 1-5s ago
  DOWN,      // Last probe failed >5s ago, or timeout
}

async function probe_remote_site(site_id: &str) -> Result<()>:
  timeout = 5 seconds

  match tokio::time::timeout(timeout, rpc_ping(site_id)).await:
    Ok(Ok(())) => return Ok(())
    Ok(Err(e)) => return Err(e)
    Err(_) => return Err(Timeout)

async function health_monitor_loop():
  loop:
    for site in all_remote_sites:
      result = probe_remote_site(site).await

      match result:
        Ok(()) =>
          site.health = HEALTHY
          site.last_probe_ok = now()
        Err(_) =>
          if site.last_probe_ok < now() - 5s:
            site.health = DOWN
            trigger_failover()
          else:
            site.health = DEGRADED

    sleep(1 second)  // Probe every second
```

**Trade-offs:**
- ✅ **Pro:** Quick detection of site failure (5-10 seconds)
- ✅ **Pro:** Soft states (DEGRADED) before hard DOWN
- ❌ **Con:** False positives if network jitter
- ❌ **Con:** Probes consume bandwidth/CPU

**Tuning:**
- Probe interval: 1-5 seconds (fast detection vs low overhead)
- Timeout: 5-30 seconds (depends on WAN latency)
- Downtime before failover: 5-60 seconds

**Code location:** `health.rs`

---

### 10. Garbage Collection: Journal Retention

**Problem:** Write-ahead journal grows forever. Disk fills up.

**Solution:** Purge old entries that have been replicated and no longer needed.

**Algorithm:**
```
struct JournalGC {
  retention_seconds: u64,  // e.g., 7 days
  min_checkpoint_entries: usize,  // e.g., 1000
}

async function gc_loop():
  loop:
    sleep(60 seconds)  // Run GC every minute

    now = now_ns()
    cutoff_age = now - (retention_seconds * 1_000_000_000)

    for site in all_remote_sites:
      last_acked_seq = site.replication_cursor.last_acked_seq

      // Find all entries before cursor
      old_entries = journal.entries_before(last_acked_seq)

      // Delete old entries, but keep at least min_checkpoint_entries
      safe_to_delete = max(0, len(old_entries) - min_checkpoint_entries)

      if safe_to_delete > 0:
        journal.delete_entries(0, safe_to_delete)
        metrics.journal_entries_deleted += safe_to_delete
        metrics.journal_size_bytes = journal.size()
```

**Example:**
```
Now: 2026-03-01 12:00:00
Retention: 7 days = 604,800 seconds
Cutoff age: 2026-02-22 12:00:00

All entries before 2026-02-22 12:00:00 are eligible for deletion.

Constraints:
- site-b acked up to seq=1000
- site-c acked up to seq=1500 (slower site)

Safe to delete: entries < seq=1000 (limited by slowest site)
```

**Trade-offs:**
- ✅ **Pro:** Prevents disk exhaustion
- ✅ **Pro:** Bounded journal size
- ❌ **Con:** Can't recover old entries after deletion
- ❌ **Con:** GC must respect slowest replica (can't delete until it acks)

**Optimization:**
- Async GC (don't block replication)
- Tiered journal (hot entries in fast storage, old in S3)
- Smart deletion (delete from slowest replica's perspective)

**Code location:** `journal_gc.rs`

---

## Performance Analysis

### Write Latency Breakdown

```
A2 writes metadata change
  ↓ (0.1ms)
A6 receives in engine
  ↓ (0.05ms compress)
Apply compression
  ↓ (0.05ms serialize)
Add to pipeline batch
  ↓ (wait for batch) 50-100ms ← MAJOR LATENCY
Batch ready
  ↓ (0.1ms send)
Send via conduit (gRPC/A4)
  ↓ (network latency) 10-50ms ← DEPENDS ON WAN
Remote site receives
  ↓ (0.1ms apply)
Apply LWW conflict resolution
  ↓ (0.1ms write)
Write to remote A2
  ↓ (0.1ms ack)
Send ack back to primary
  ↓ (network latency) 10-50ms ← DEPENDS ON WAN

Total: 50-100ms (batch wait) + 10-50ms (network) + 10-50ms (network back) = 70-200ms
```

### Throughput Analysis

**Single-threaded (baseline):**
- Entry size: 500 bytes
- Batch size: 100 entries = 50KB
- RPC latency: 10ms
- Throughput: 100 entries / 10ms = 10,000 entries/sec

**With compression (LZ4, 50% ratio):**
- Compressed size: 25KB per batch
- RPC latency: 5ms (smaller payload)
- Throughput: 100 entries / 5ms = 20,000 entries/sec

**With N threads:**
- Throughput: baseline × N threads
- Example: 8 threads → 80,000 entries/sec

**With fanout (3 sites):**
- Each site gets replicated independently
- Throughput per site: same as single thread
- Total throughput: baseline (no change)

---

## Consistency Model

### Eventual Consistency

ClaudeFS provides **eventual consistency** with a **causal consistency** option:

**Timeline (default):**
```
T0: Client writes X to site-a
T1: A6 replicates X to site-b (100ms later)
T2: Client reads from site-b, may see old value

Between T0 and T1: inconsistent (different reads to different sites)
After T1 + network: consistent (all sites have X)
```

**Causal consistency (with version vectors):**
```
T0: Client writes X to site-a (gets version [1,0,0])
T1: Client reads from site-b
  - If site-b has seen [1,0,0], returns X
  - If site-b hasn't seen [1,0,0] yet, blocks/waits for replication

Result: Causally consistent (client always sees its own writes)
```

### Stronger Guarantees (Phase 3+)

- **Read-your-writes:** Remember client's version vector, ensure reads see at least that version
- **Per-object strong consistency:** Pin important inodes to primary, treat as read-only replicas
- **Quorum reads:** Read from 2+ sites, take majority version

---

## References

1. **Last-Write-Wins:** Shapiro et al. "A comprehensive study of CRDT" (2016)
2. **Version Vectors:** Parker et al. "Detection of Mutual Inconsistency in Distributed Systems" (1983)
3. **Fencing Tokens:** Chandra et al. "The Chubby lock service for loosely-coupled distributed systems" (2006)
4. **Token Bucket:** Andrew S. Tanenbaum, "Computer Networks" (Prentice Hall)
5. **Batching:** Michael, M. M., & Scott, M. L. (2005). "Nonblocking algorithms and scalable multicore programming."

---

## Conclusion

The A6 replication subsystem combines well-known algorithms in a novel way optimized for ClaudeFS's architecture:

- **LWW** for fast conflict resolution without coordination
- **Version vectors** for optional causal consistency
- **Fencing tokens** for split-brain safety
- **Cursor tracking** for exactly-once delivery
- **Compression + batching** for network efficiency
- **Backpressure** for stability under overload
- **Health probes** for fast failure detection
- **GC** for bounded journal size

The design prioritizes **simplicity** (single-threaded design, no consensus required) while providing **strong guarantees** (exactly-once delivery, split-brain safety).

Future work may add stronger consistency modes (quorum reads, CRDT data structures) when the workload demands it.
