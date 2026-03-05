# A4 Transport — Phase 12: Distributed Tracing, QoS, Adaptive Routing

## Priority 1 Feature Modules

Implement three high-priority modules for distributed tracing, QoS/traffic shaping, and adaptive routing.

### Module 1: trace_aggregator.rs (~24 tests)
- Aggregate OTEL spans across full request path (client → metadata → storage)
- Trace correlation and latency computation
- Critical path analysis for performance debugging

### Module 2: bandwidth_shaper.rs (~26 tests)
- Per-tenant bandwidth allocation and enforcement
- Token bucket + weighted fair queuing implementation
- Hard limits (reject excess) and soft warnings

### Module 3: adaptive_router.rs (~30 tests)
- Intelligent request routing based on endpoint health/latency/load
- RTT percentile tracking, availability monitoring
- Score-based selection with failover support

Target: 1380+ tests passing (adding ~80-100 tests)

See full specifications in: /home/cfs/claudefs/a4-phase12-input.md
