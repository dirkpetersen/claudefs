[0m
> build · accounts/fireworks/models/minimax-m2p5
[0m
[0m$ [0mcd /home/cfs/claudefs && cargo test -p claudefs-repl --lib 2>&1 | tail -100
test wal::tests::test_walrecord_struct ... ok
test wal::tests::test_many_shards ... ok
test wal::tests::test_reset ... ok
test journal::proptest_journal::prop_journal_entry_new_crc_valid ... ok
test journal::proptest_journal::prop_journal_entry_crc_deterministic ... ok
test journal::proptest_journal::prop_journal_entry_bincode_roundtrip ... ok
test auth_ratelimit::tests::test_auth_lockout_released ... ok
test entry_dedup::tests::test_large_ring_buffer_handles_many_entries ... ok

failures:

---- conduit_pool::tests::test_mark_failed_transitions_to_failed stdout ----

thread 'conduit_pool::tests::test_mark_failed_transitions_to_failed' (704258) panicked at crates/claudefs-repl/src/conduit_pool.rs:473:51:
called `Result::unwrap()` on an `Err` value: NoHealthyConnections { site_id: 1 }

---- conduit_pool::tests::test_is_site_healthy_returns_true_with_ready_connections stdout ----

thread 'conduit_pool::tests::test_is_site_healthy_returns_true_with_ready_connections' (704257) panicked at crates/claudefs-repl/src/conduit_pool.rs:610:9:
assertion failed: healthy

---- conduit_pool::tests::test_acquire_returns_conn_and_marks_in_use stdout ----

thread 'conduit_pool::tests::test_acquire_returns_conn_and_marks_in_use' (704252) panicked at crates/claudefs-repl/src/conduit_pool.rs:420:51:
called `Result::unwrap()` on an `Err` value: NoHealthyConnections { site_id: 1 }
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

---- conduit_pool::tests::test_global_stats_aggregates_all_sites stdout ----

thread 'conduit_pool::tests::test_global_stats_aggregates_all_sites' (704255) panicked at crates/claudefs-repl/src/conduit_pool.rs:542:37:
called `Result::unwrap()` on an `Err` value: NoHealthyConnections { site_id: 1 }

---- conduit_pool::tests::test_release_returns_to_ready stdout ----

thread 'conduit_pool::tests::test_release_returns_to_ready' (704263) panicked at crates/claudefs-repl/src/conduit_pool.rs:435:51:
called `Result::unwrap()` on an `Err` value: NoHealthyConnections { site_id: 1 }

---- conduit_pool::tests::test_shutdown_marks_all_connections_draining stdout ----

thread 'conduit_pool::tests::test_shutdown_marks_all_connections_draining' (704265) panicked at crates/claudefs-repl/src/conduit_pool.rs:575:9:
assertion failed: stats.ready_connections > 0 || stats.in_use_connections > 0

---- conduit_pool::tests::test_site_stats_correct_per_site stdout ----

thread 'conduit_pool::tests::test_site_stats_correct_per_site' (704266) panicked at crates/claudefs-repl/src/conduit_pool.rs:558:37:
called `Result::unwrap()` on an `Err` value: NoHealthyConnections { site_id: 1 }

---- conduit_pool::tests::test_tick_advances_reconnect_after_delay stdout ----

thread 'conduit_pool::tests::test_tick_advances_reconnect_after_delay' (704267) panicked at crates/claudefs-repl/src/conduit_pool.rs:493:9:
assertion failed: stats1.reconnecting_connections >= 1

---- conduit_pool::tests::test_round_robin_across_connections stdout ----

thread 'conduit_pool::tests::test_round_robin_across_connections' (704264) panicked at crates/claudefs-repl/src/conduit_pool.rs:455:47:
called `Result::unwrap()` on an `Err` value: NoHealthyConnections { site_id: 1 }

---- entry_dedup::tests::test_different_seq_same_fingerprint_not_duplicate stdout ----

thread 'entry_dedup::tests::test_different_seq_same_fingerprint_not_duplicate' (704324) panicked at crates/claudefs-repl/src/entry_dedup.rs:404:9:
assertion failed: !dedup.is_duplicate(fp1, 1)

---- entry_dedup::tests::test_evict_clears_old_entries_but_keeps_fresh_ones stdout ----

thread 'entry_dedup::tests::test_evict_clears_old_entries_but_keeps_fresh_ones' (704326) panicked at crates/claudefs-repl/src/entry_dedup.rs:261:9:
Entry 2 should still be present

---- entry_dedup::tests::test_is_duplicate_without_recording stdout ----

thread 'entry_dedup::tests::test_is_duplicate_without_recording' (704330) panicked at crates/claudefs-repl/src/entry_dedup.rs:305:9:
assertion `left == right` failed: Should not increment checked
  left: 1
 right: 0

---- repl_filter::tests::test_stats_track_correctly stdout ----

thread 'repl_filter::tests::test_stats_track_correctly' (704646) panicked at crates/claudefs-repl/src/repl_filter.rs:535:9:
assertion `left == right` failed
  left: 0
 right: 2


failures:
    conduit_pool::tests::test_acquire_returns_conn_and_marks_in_use
    conduit_pool::tests::test_global_stats_aggregates_all_sites
    conduit_pool::tests::test_is_site_healthy_returns_true_with_ready_connections
    conduit_pool::tests::test_mark_failed_transitions_to_failed
    conduit_pool::tests::test_release_returns_to_ready
    conduit_pool::tests::test_round_robin_across_connections
    conduit_pool::tests::test_shutdown_marks_all_connections_draining
    conduit_pool::tests::test_site_stats_correct_per_site
    conduit_pool::tests::test_tick_advances_reconnect_after_delay
    entry_dedup::tests::test_different_seq_same_fingerprint_not_duplicate
    entry_dedup::tests::test_evict_clears_old_entries_but_keeps_fresh_ones
    entry_dedup::tests::test_is_duplicate_without_recording
    repl_filter::tests::test_stats_track_correctly

test result: FAILED. 865 passed; 13 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.63s

error: test failed, to rerun pass `-p claudefs-repl --lib`
[0m
