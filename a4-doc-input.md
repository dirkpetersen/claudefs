# Task: Add Missing Documentation to claudefs-transport Source Files

The `claudefs-transport` crate builds successfully with 667 passing tests but has 393 `missing_docs` warnings. Your task is to add doc comments to all public items that are missing them in the following files:

1. `crates/claudefs-transport/src/observability.rs`
2. `crates/claudefs-transport/src/multipath.rs`
3. `crates/claudefs-transport/src/conn_auth.rs`
4. `crates/claudefs-transport/src/congestion.rs`
5. `crates/claudefs-transport/src/adaptive.rs`
6. `crates/claudefs-transport/src/bandwidth.rs`
7. `crates/claudefs-transport/src/connmigrate.rs`
8. `crates/claudefs-transport/src/request_dedup.rs`

## Rules

- Add `/// doc comment` before every public struct field, enum variant, method, and associated function that is missing one.
- Keep doc comments concise (one line is fine for fields/variants, up to 3 lines for methods).
- Do NOT change any logic, function signatures, trait implementations, or test code.
- Do NOT add or remove any items.
- Preserve all existing doc comments exactly as they are.
- For struct fields: describe what the field stores or controls.
- For enum variants: describe what state/mode the variant represents.
- For methods: describe what the method does and any important preconditions.
- The goal is to eliminate ALL `missing_docs` warnings from these files.

## Context: ClaudeFS Transport Layer

This is the transport layer of ClaudeFS, a distributed POSIX filesystem. Key concepts:
- **Spans/observability**: W3C-compatible distributed tracing spans for per-request debugging
- **Multipath**: Multiple network paths (RDMA/TCP) with automatic failover for resilience
- **Connection auth**: mTLS certificate-based authentication with revocation support
- **Congestion control**: AIMD/Cubic/BBR algorithms to avoid network congestion
- **Adaptive timeout**: Self-tuning RPC timeouts based on observed latency percentiles
- **Bandwidth**: Per-tenant bandwidth allocation with strict/shaping/monitor enforcement

## File-by-File Changes Required

### observability.rs

Add docs to:
- `Attribute::key` — the attribute name
- `Attribute::value` — the attribute value
- `Attribute::new` — constructor
- `AttributeValue::String`, `::Int`, `::Float`, `::Bool` variants
- `AttributeValue::string`, `::int`, `::float`, `::bool` constructors
- `SpanEvent::name` — event name string
- `SpanEvent::severity` — event severity level
- `SpanEvent::timestamp_us` — microsecond timestamp
- `SpanEvent::attributes` — key-value attributes
- `SpanEvent::new`, `SpanEvent::with_attributes`
- `Span::id`, `::parent_id`, `::name`, `::status`, `::start_us`, `::end_us`, `::attributes`, `::events`
- `Span::new`, `::with_attributes`, `::add_event`, `::duration_us`
- `ObservabilityConfig::max_spans`, `::max_events_per_span`, `::max_attributes`, `::sample_rate`, `::enabled`
- `SpanBuilder` struct
- `SpanBuilder::new`, `::parent`, `::attribute`, `::string_attr`, `::int_attr`, `::bool_attr`, `::float_attr`, `::start_us`, `::build`
- `ObservabilityStats` struct, `::new`, `::inc_*` methods, `::snapshot`
- `ObservabilityStatsSnapshot::spans_created`, `::spans_completed`, `::spans_dropped`, `::events_recorded`, `::error_spans`
- `SpanCollector` struct, `::new`, `::start_span`, `::add_event`, `::add_event_with_attrs`, `::end_span`, `::get_span`, `::drain_completed`, `::completed_count`, `::stats`

### multipath.rs

Add docs to:
- `PathId` struct — unique path identifier
- `PathId::new`, `::as_u64`
- `PathState::Active`, `::Degraded`, `::Failed`, `::Draining`
- `PathMetrics::latency_us`, `::min_latency_us`, `::jitter_us`, `::loss_rate`, `::bandwidth_bps`, `::bytes_sent`, `::bytes_received`, `::errors`, `::last_probe_us`
- `PathInfo::id`, `::name`, `::state`, `::metrics`, `::weight`, `::priority`
- `PathSelectionPolicy::RoundRobin`, `::LowestLatency`, `::WeightedRandom`, `::Failover`
- `MultipathConfig::policy`, `::max_paths`, `::probe_interval_ms`, `::failure_threshold`, `::recovery_threshold`, `::latency_ewma_alpha`, `::max_loss_rate`
- `MultipathStats::total_paths`, `::active_paths`, `::failed_paths`, `::total_requests`, `::failover_events`, `::paths`
- `MultipathError::PathNotFound`, `::MaxPathsExceeded`, `::NoAvailablePaths`
- `MultipathRouter` struct
- `MultipathRouter::new`, `::add_path`, `::remove_path`, `::select_path`, `::record_success`, `::record_failure`, `::mark_failed`, `::mark_active`, `::active_paths`, `::path_info`, `::stats`

### conn_auth.rs

Add docs to:
- `AuthLevel::None`, `::TlsOnly`, `::MutualTls`, `::MutualTlsStrict`
- `CertificateInfo::subject`, `::issuer`, `::serial`, `::fingerprint_sha256`, `::not_before_ms`, `::not_after_ms`, `::is_ca`
- `AuthConfig::level`, `::allowed_subjects`, `::allowed_fingerprints`, `::max_cert_age_days`, `::require_cluster_ca`, `::cluster_ca_fingerprint`
- `AuthResult::Allowed::identity`, `::Denied::reason`, `::CertificateExpired::subject`, `::CertificateExpired::expired_at_ms`, `::CertificateRevoked::subject`, `::CertificateRevoked::serial`
- `RevocationList::revoked_serials`, `::revoked_fingerprints`, `::last_updated_ms`
- `RevocationList::new`, `::revoke_serial`, `::revoke_fingerprint`, `::is_revoked_serial`, `::is_revoked_fingerprint`, `::len`, `::is_empty`
- `AuthStats::total_allowed`, `::total_denied`, `::revoked_count`
- `ConnectionAuthenticator` struct
- `ConnectionAuthenticator::new`, `::authenticate`, `::revoke_serial`, `::revoke_fingerprint`, `::set_time`, `::stats`

### congestion.rs

Add docs to:
- `CongestionAlgorithm::Aimd`, `::Cubic`, `::Bbr`
- `CongestionState::SlowStart`, `::CongestionAvoidance`, `::Recovery`
- `CongestionConfig::algorithm`, `::initial_window`, `::min_window`, `::max_window`, `::aimd_increase`, `::aimd_decrease_factor`, `::cubic_beta`, `::cubic_c`, `::slow_start_threshold`, `::rtt_smoothing_alpha`
- `CongestionStats::window_size`, `::ssthresh`, `::bytes_in_flight`, `::smoothed_rtt_us`, `::min_rtt_us`, `::total_sent`, `::total_acked`, `::total_lost`, `::loss_events`, `::state`
- `CongestionWindow` struct
- `CongestionWindow::new`, `::available_window`, `::can_send`, `::on_send`, `::on_ack`, `::on_loss`, `::state`, `::window_size`, `::smoothed_rtt_us`, `::stats`, `::set_ssthresh`

### adaptive.rs

Add docs to:
- `AdaptiveConfig::initial_timeout_ms`, `::min_timeout_ms`, `::max_timeout_ms`, `::percentile_target`, `::safety_margin`, `::window_size`, `::adjustment_interval_ms`, `::enabled`
- All other public items that currently lack doc comments

### bandwidth.rs

Add docs to:
- `EnforcementMode::Strict`, `::Shaping`, `::Monitor`
- `BandwidthConfig::global_limit_bps`, `::default_tenant_limit_bps`, `::burst_factor`, `::measurement_window_ms`, `::enforcement`
- `BandwidthResult::Allowed`, `::Throttled::delay_ms`, `::Dropped::bytes`, `::Monitored::over_limit`
- `BandwidthStats::total_requests`, `::total_allowed`, `::total_throttled`, `::total_dropped`, `::global_usage_bps`, `::tenant_count`
- All public methods on `BandwidthAllocator`

### connmigrate.rs

Read this file and add doc comments to all public fields, variants, and methods that are missing them.

### request_dedup.rs

Read this file and add doc comments to all public fields, variants, and methods that are missing them.

## Verification

After making changes, the following command should produce zero warnings for these files:
```
cargo build -p claudefs-transport 2>&1 | grep "missing documentation" | grep -E "(observability|multipath|conn_auth|congestion|adaptive|bandwidth|connmigrate|request_dedup)"
```

All 667 existing tests must continue to pass:
```
cargo test -p claudefs-transport
```
