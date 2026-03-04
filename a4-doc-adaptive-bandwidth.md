Add missing Rust doc comments to TWO modules for the claudefs-transport crate.

The crate has #![warn(missing_docs)] enabled, so ALL public items need doc comments.

Rules:
- Add `/// <doc comment>` immediately before each public item that lacks one
- Do NOT modify any existing code, logic, tests, or existing doc comments
- Do NOT add comments to private/internal items (only pub items)
- Keep doc comments concise and accurate to what the code does

=== FILE 1: adaptive.rs ===

Items needing docs in AdaptiveConfig struct:
- initial_timeout_ms: Initial timeout value in milliseconds before adaptive tuning begins
- min_timeout_ms: Minimum timeout in milliseconds (lower bound after adaptive adjustment)
- max_timeout_ms: Maximum timeout in milliseconds (upper bound after adaptive adjustment)
- percentile_target: Target percentile for timeout calculation (e.g., 0.99 = p99 latency)
- safety_margin: Multiplier applied to target percentile latency (e.g., 1.5 = 50% headroom)
- window_size: Number of latency samples to retain in the sliding window
- adjustment_interval_ms: How often to recalculate the adaptive timeout
- enabled: Whether adaptive timeout adjustment is enabled

Items needing docs in LatencyHistogram:
- new: Creates a new histogram with the given sample capacity
- record: Records a latency sample in microseconds
- percentile: Computes the given percentile (0.0–1.0) of recorded latencies
- snapshot: Returns a full percentile snapshot (p50/p90/p95/p99/p999/min/max/mean)
- sample_count: Returns the number of samples currently in the window
- reset: Clears all recorded samples

Items needing docs in PercentileSnapshot struct fields:
- p50, p90, p95, p99, p999: 50th/90th/95th/99th/99.9th percentile latency in microseconds
- min: Minimum recorded latency in microseconds
- max: Maximum recorded latency in microseconds
- mean: Mean latency in microseconds
- sample_count: Number of samples in this snapshot

Items needing docs in AdaptiveTimeout:
- new: Creates a new adaptive timeout manager with the given configuration
- record_latency: Records an observed latency sample in microseconds
- record_timeout: Records that a request timed out
- current_timeout_ms: Returns the current timeout value in milliseconds
- adjust: Recalculates and updates the timeout based on recorded latencies
- percentiles: Returns a snapshot of the current latency percentiles
- stats: Returns current statistics

Items needing docs in AdaptiveStats/AdaptiveStatsSnapshot fields:
- samples_recorded: Total number of latency samples recorded
- timeout_adjustments: Number of times the timeout was adjusted
- timeouts_hit: Number of requests that timed out
- current_timeout_ms: Current adaptive timeout value in milliseconds
- current_p99_us: Current p99 latency in microseconds

=== FILE 2: bandwidth.rs ===

Items needing docs:
- EnforcementMode enum and its variants (Strict=drops over-limit, Shaping=delays over-limit, Monitor=observes only)
- BandwidthConfig struct fields (global_limit_bps, default_tenant_limit_bps, burst_factor, measurement_window_ms, enforcement)
- BandwidthResult enum and its variants (Allowed, Throttled, Dropped, Monitored)
- BandwidthStats struct and its fields (total_requests, total_allowed, total_throttled, total_dropped, global_usage_bps, tenant_count)
- BandwidthAllocator struct

Output format:
First output the complete adaptive.rs, then output the complete bandwidth.rs.
Mark each file with: // === FILE: adaptive.rs === and // === FILE: bandwidth.rs ===
Output ONLY Rust source code, no markdown fences.
