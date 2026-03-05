Add missing Rust doc comments to TWO modules for the claudefs-transport crate.

The crate has #![warn(missing_docs)] enabled, so ALL public items need doc comments.

Rules:
- Add `/// <doc comment>` immediately before each public item that lacks one
- Do NOT modify any existing code, logic, tests, or existing doc comments
- Do NOT add comments to private/internal items (only pub items)
- Keep doc comments concise and accurate to what the code does
- For EACH file, output the COMPLETE file with ALL added doc comments

=== FILE 1: congestion.rs ===

Items needing docs:
- CongestionAlgorithm enum and its variants (Aimd, Cubic, Bbr)
- CongestionState enum and its variants (SlowStart, CongestionAvoidance, Recovery)
- CongestionConfig struct and all its fields (algorithm, initial_window, min_window, max_window, aimd_increase, aimd_decrease_factor, cubic_beta, cubic_c, slow_start_threshold, rtt_smoothing_alpha)
- CongestionStats struct and all its fields (window_size, ssthresh, bytes_in_flight, smoothed_rtt_us, min_rtt_us, total_sent, total_acked, total_lost, loss_events, state)
- CongestionWindow struct and all its public methods (new, available_window, can_send, on_send, on_ack, on_loss, state, window_size, smoothed_rtt_us, stats, set_ssthresh)

=== FILE 2: request_dedup.rs ===

Items needing docs:
- RequestId struct
- DedupConfig struct and its fields (max_entries, ttl_ms, cleanup_interval_ms)
- DedupEntry struct and its fields (request_id, response_hash, created_at_ms, hit_count)
- DedupResult enum and its variants (New, Duplicate, Expired)
- DedupStats struct and its fields (total_checks, total_duplicates, total_evictions, current_entries, hit_rate)
- DedupTracker struct and all its methods (new, check, record, evict_expired, advance_time, set_time, len, is_empty, stats)

Output format:
First output the complete congestion.rs, then output the complete request_dedup.rs.
Mark each file with a comment like: // === FILE: congestion.rs ===
Output ONLY the Rust source code, no markdown fences.
