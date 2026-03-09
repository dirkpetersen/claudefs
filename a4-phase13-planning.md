# A4 Transport — Phase 13: Request Pipelining, Backpressure, Connection Pooling

## Overview

Phase 13 continues Priority 1 feature implementation with three advanced modules focused on request optimization and resource efficiency. These modules complete the core transport layer foundation needed for production deployment.

## Phase 13 Modules (Target: 80-100 new tests, 1450+ total)

### Module 1: reactive_backpressure.rs (~26 tests)

**Purpose:** Coordinated backpressure signal propagation across the request path

**Key Features:**
- BackpressureLevel enum: {Ok, Slow, Degraded, Overloaded}
- Per-component backpressure tracking (client, transport, metadata, storage)
- Automatic signal propagation upstream (slow response → client applies backoff)
- Exponential backoff with jitter for client retry delays
- Global backpressure thresholds (CPU, memory, queue depth)

**Data Structures:**
```rust
pub struct BackpressureSignal {
    pub level: BackpressureLevel,
    pub source_node_id: u64,
    pub timestamp_ns: u64,
    pub severity: f64,  // 0.0-1.0
    pub suggested_backoff_ms: u32,
}

pub struct BackpressureCoordinator {
    // Tracks backpressure from all components
    // Computes global level and propagates upstream
}
```

**Integration Points:**
- Client: Apply adaptive backoff based on received signals
- Transport: Monitor queue depth and CPU, emit signals when thresholds exceeded
- Metadata (A2): Track latency spike detection, emit when serving slow
- Storage (A1): Track queue depth and latency, emit when loaded

**Tests (~26):**
- Signal creation and propagation
- Threshold-based emission
- Exponential backoff computation
- Multi-component coordination
- Edge cases (rapid fluctuations, network delays)

---

### Module 2: pipelined_requests.rs (~28 tests)

**Purpose:** Request pipelining with dependency tracking and out-of-order execution

**Key Features:**
- RequestPipeline with configurable depth (default: 32)
- Dependency graph tracking (which requests must complete before others)
- Out-of-order response delivery (responses sent as soon as ready)
- Head-of-line blocking prevention
- Per-stream request ordering guarantee (but parallelism across streams)

**Data Structures:**
```rust
pub struct PipelinedRequest {
    pub request_id: u64,
    pub sequence_num: u32,  // Client-assigned
    pub depends_on: Vec<u64>,  // Request IDs this depends on
    pub payload: Vec<u8>,
    pub timeout_ms: u32,
}

pub struct RequestPipeline {
    pub max_depth: usize,
    pub in_flight: Vec<InFlightRequest>,
    // Tracks completions and re-orders responses
}
```

**Integration Points:**
- Transport RPC layer: Pipeline management
- A2 metadata: Handle out-of-order operations (maintain consistency)
- A5 FUSE client: Interleave multiple requests to storage

**Tests (~28):**
- Basic pipelining and response ordering
- Dependency satisfaction
- Timeout handling in pipeline
- Head-of-line blocking scenarios
- Concurrent stream independence
- Flow control (back-pressure when pipeline full)
- Error recovery (retry failed requests)

---

### Module 3: transport_pooling.rs (~24 tests)

**Purpose:** Connection pool management with intelligent reuse and creation

**Key Features:**
- Connection pool per remote node (default: 8 connections per node)
- Idle timeout with connection warmup (keep-alive)
- Health checks (periodically verify connections are alive)
- Automatic reconnection on failure
- Pool sizing heuristics (grow when contention, shrink when idle)

**Data Structures:**
```rust
pub struct ConnectionPool {
    pub node_id: u64,
    pub connections: Vec<Arc<Connection>>,
    pub size: usize,
    pub min_size: usize,
    pub max_size: usize,
    pub idle_timeout_secs: u64,
}

pub struct PoolStats {
    pub active: usize,
    pub idle: usize,
    pub total_created: u64,
    pub total_destroyed: u64,
    pub reuse_ratio: f64,  // active/(active+idle)
}
```

**Integration Points:**
- Client connection acquisition (get or create)
- Health checking with A2/A1 heartbeats
- Metrics export to A8 (pool efficiency, connection lifetime)

**Tests (~24):**
- Pool creation and size management
- Connection acquisition/release
- Idle timeout and cleanup
- Health checking
- Reconnection on failure
- Stats tracking
- Concurrent access patterns
- Edge cases (pool exhaustion, rapid churn)

---

## Expected Results

- **Tests:** 1450+ passing (adding ~100-110 new tests to Phase 12's 1350+)
- **Modules:** 87 total in claudefs-transport (84 + 3)
- **Build status:** ✅ Clean
- **Quality:** High (same standards as Phase 12)

## Integration & Dependencies

### Inbound Dependencies:
- A2 (metadata): Backpressure signal reception, pipelined request ordering
- A1 (storage): Backpressure signal emission, pool health checks
- A5 (FUSE): Use pipelined requests for concurrent I/O
- A8 (management): Export pool stats, backpressure metrics

### Outbound Exports:
- `BackpressureCoordinator` → A2, A1, A5 (backpressure handling)
- `RequestPipeline` → RPC layer (used internally by A4)
- `ConnectionPool` → A5, client layer

## Implementation Strategy

1. **Backpressure first** — enables both other modules to respond to load
2. **Pipelining second** — depends on backpressure for flow control
3. **Connection pooling last** — can be integrated independently

## Success Criteria

- ✅ All three modules compile cleanly
- ✅ No clippy warnings
- ✅ 100-110 new tests all passing
- ✅ Code review: proper error handling, thread safety
- ✅ Integration points documented and tested
- ✅ Performance: connection reuse >80%, backpressure latency <10ms

## Timeline Estimate

- **OpenCode generation:** 20-30 minutes
- **Build & test:** 30-40 minutes
- **Code review & fixes:** 10-20 minutes
- **Total:** ~1-1.5 hours from submission

## Notes

- Phase 13 completes the transport layer's core Priority 1 features
- Phase 14 will focus on advanced features (circuit breaker, connection migration, etc.)
- All modules maintain strict separation of concerns and clear APIs
- Ready for integration with A2, A1, A5, A8 in subsequent phases

---

**Status:** 🟡 **PLANNING** — Ready for OpenCode execution when API capacity available
**Baseline (Phase 12):** 1350+ tests ✅
**Target (Phase 13 end):** 1450+ tests
