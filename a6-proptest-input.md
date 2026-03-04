# A6 claudefs-repl: Add proptest property-based tests and SLA enforcement

You are working on the `claudefs-repl` crate in the ClaudeFS project.
The crate is at `/home/cfs/claudefs/crates/claudefs-repl/`.

## Your task

1. Add `proptest = "1"` as a `[dev-dependencies]` entry in `Cargo.toml`
2. Add property-based tests (using proptest) to five existing modules
3. Add `ReplicationSla` struct and `SlaMonitor` to `repl_qos.rs`

---

## STEP 1: Modify `Cargo.toml`

Add this to the end of `/home/cfs/claudefs/crates/claudefs-repl/Cargo.toml`:

```toml
[dev-dependencies]
proptest = "1"
```

---

## STEP 2: Add proptest tests to `journal.rs`

At the bottom of `/home/cfs/claudefs/crates/claudefs-repl/src/journal.rs`, **inside** the existing `#[cfg(test)] mod tests { ... }` block, add the following import at the top of that block and then add the proptest tests at the bottom of the block.

Add this import at the top of the `tests` module (after `use super::*;`):
```rust
    #[cfg(test)]
    use proptest::prelude::*;
```

Wait — the proptest macro must be imported differently. Here is the correct way:

At the very bottom of the file (after the closing `}` of the `tests` module), add a completely new test module:

```rust
#[cfg(test)]
mod proptest_journal {
    use super::*;
    use proptest::prelude::*;

    fn arb_op_kind() -> impl Strategy<Value = OpKind> {
        prop_oneof![
            Just(OpKind::Create),
            Just(OpKind::Unlink),
            Just(OpKind::Rename),
            Just(OpKind::Write),
            Just(OpKind::Truncate),
            Just(OpKind::SetAttr),
            Just(OpKind::Link),
            Just(OpKind::Symlink),
            Just(OpKind::MkDir),
            Just(OpKind::SetXattr),
            Just(OpKind::RemoveXattr),
        ]
    }

    proptest! {
        /// Bincode serialization of JournalEntry is a lossless roundtrip for any input.
        #[test]
        fn prop_journal_entry_bincode_roundtrip(
            seq in 0u64..u64::MAX,
            shard_id in 0u32..256,
            site_id in 0u64..1000,
            timestamp_us in 0u64..u64::MAX,
            inode in 0u64..u64::MAX,
            op in arb_op_kind(),
            payload in prop::collection::vec(0u8..=255, 0..4096),
        ) {
            let entry = JournalEntry::new(seq, shard_id, site_id, timestamp_us, inode, op, payload.clone());
            let encoded = bincode::serialize(&entry).unwrap();
            let decoded: JournalEntry = bincode::deserialize(&encoded).unwrap();
            prop_assert_eq!(decoded.seq, seq);
            prop_assert_eq!(decoded.shard_id, shard_id);
            prop_assert_eq!(decoded.site_id, site_id);
            prop_assert_eq!(decoded.timestamp_us, timestamp_us);
            prop_assert_eq!(decoded.inode, inode);
            prop_assert_eq!(decoded.op, op);
            prop_assert_eq!(decoded.payload, payload);
            prop_assert!(decoded.validate_crc());
        }

        /// CRC32 is deterministic: same inputs always produce same CRC.
        #[test]
        fn prop_journal_entry_crc_deterministic(
            seq in 0u64..u64::MAX,
            shard_id in 0u32..256,
            site_id in 0u64..1000,
            timestamp_us in 0u64..u64::MAX,
            inode in 0u64..u64::MAX,
            op in arb_op_kind(),
            payload in prop::collection::vec(0u8..=255, 0..1024),
        ) {
            let e1 = JournalEntry::new(seq, shard_id, site_id, timestamp_us, inode, op, payload.clone());
            let e2 = JournalEntry::new(seq, shard_id, site_id, timestamp_us, inode, op, payload);
            prop_assert_eq!(e1.crc32, e2.crc32);
        }

        /// CRC32 validates successfully for any freshly constructed entry.
        #[test]
        fn prop_journal_entry_new_crc_valid(
            seq in 0u64..u64::MAX,
            shard_id in 0u32..256,
            site_id in 0u64..1000,
            timestamp_us in 0u64..u64::MAX,
            inode in 0u64..u64::MAX,
            op in arb_op_kind(),
            payload in prop::collection::vec(0u8..=255, 0..1024),
        ) {
            let entry = JournalEntry::new(seq, shard_id, site_id, timestamp_us, inode, op, payload);
            prop_assert!(entry.validate_crc());
        }

        /// A corrupted CRC field fails validation.
        #[test]
        fn prop_journal_entry_corrupted_crc_fails(
            seq in 0u64..u64::MAX,
            shard_id in 0u32..256,
            site_id in 0u64..1000,
            timestamp_us in 0u64..u64::MAX,
            inode in 0u64..u64::MAX,
            op in arb_op_kind(),
            payload in prop::collection::vec(0u8..=255, 1..512),
            corrupt_xor in 1u32..u32::MAX,
        ) {
            let mut entry = JournalEntry::new(seq, shard_id, site_id, timestamp_us, inode, op, payload);
            entry.crc32 ^= corrupt_xor;
            prop_assert!(!entry.validate_crc());
        }

        /// JournalTailer returns entries in shard+seq order for any permutation.
        #[test]
        fn prop_tailer_sorted_order(
            seqs in prop::collection::vec(0u64..100, 1..20),
        ) {
            let entries: Vec<JournalEntry> = seqs.iter().enumerate().map(|(i, &seq)| {
                JournalEntry::new(seq, (i % 4) as u32, 1, seq * 1000, i as u64, OpKind::Write, vec![])
            }).collect();
            let tailer = JournalTailer::new_in_memory(entries);
            // Verify sorted by (shard_id, seq)
            let sorted = tailer.filter_by_shard(0);
            for w in sorted.windows(2) {
                prop_assert!(w[0].seq <= w[1].seq);
            }
        }
    }
}
```

---

## STEP 3: Add proptest tests to `uidmap.rs`

At the bottom of `/home/cfs/claudefs/crates/claudefs-repl/src/uidmap.rs`, after all existing tests, add:

```rust
#[cfg(test)]
mod proptest_uidmap {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Passthrough mapper never changes any UID value.
        #[test]
        fn prop_passthrough_uid_unchanged(
            site_id in 0u64..1000,
            uid in 0u32..u32::MAX,
        ) {
            let mapper = UidMapper::passthrough();
            prop_assert_eq!(mapper.translate_uid(site_id, uid), uid);
        }

        /// Passthrough mapper never changes any GID value.
        #[test]
        fn prop_passthrough_gid_unchanged(
            site_id in 0u64..1000,
            gid in 0u32..u32::MAX,
        ) {
            let mapper = UidMapper::passthrough();
            prop_assert_eq!(mapper.translate_gid(site_id, gid), gid);
        }

        /// Mapped UID is correctly translated for known mappings.
        #[test]
        fn prop_uid_mapping_applied(
            site_id in 1u64..100,
            src_uid in 1u32..10000,
            dest_uid in 10001u32..20000,
        ) {
            let mapper = UidMapper::new(
                vec![UidMapping { source_site_id: site_id, source_uid: src_uid, dest_uid }],
                vec![],
            );
            prop_assert_eq!(mapper.translate_uid(site_id, src_uid), dest_uid);
        }

        /// Unknown UID is passed through unchanged when no mapping exists.
        #[test]
        fn prop_unknown_uid_passthrough(
            site_id in 1u64..100,
            known_uid in 1u32..1000,
            dest_uid in 10001u32..20000,
            unknown_uid in 2000u32..5000,
        ) {
            prop_assume!(unknown_uid != known_uid);
            let mapper = UidMapper::new(
                vec![UidMapping { source_site_id: site_id, source_uid: known_uid, dest_uid }],
                vec![],
            );
            // Unknown UID not in the mapping → passthrough
            prop_assert_eq!(mapper.translate_uid(site_id, unknown_uid), unknown_uid);
        }

        /// GID mapping is applied correctly for known mappings.
        #[test]
        fn prop_gid_mapping_applied(
            site_id in 1u64..100,
            src_gid in 1u32..10000,
            dest_gid in 10001u32..20000,
        ) {
            let mapper = UidMapper::new(
                vec![],
                vec![GidMapping { source_site_id: site_id, source_gid: src_gid, dest_gid }],
            );
            prop_assert_eq!(mapper.translate_gid(site_id, src_gid), dest_gid);
        }
    }
}
```

---

## STEP 4: Add proptest tests to `conflict_resolver.rs`

At the bottom of `/home/cfs/claudefs/crates/claudefs-repl/src/conflict_resolver.rs`, after all existing tests, add:

```rust
#[cfg(test)]
mod proptest_conflict {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// LWW: higher timestamp always wins, regardless of sequence numbers.
        #[test]
        fn prop_lww_higher_ts_wins(
            inode in 1u64..100000,
            site_a_id in 1u64..50,
            site_b_id in 51u64..100,
            seq_a in 0u64..1000,
            seq_b in 0u64..1000,
            ts_a in 1u64..u64::MAX,
            ts_b_delta in 1u64..1000,
        ) {
            let ts_b = ts_a.saturating_sub(ts_b_delta); // ts_a > ts_b always
            prop_assume!(ts_a > ts_b);
            let site_a = SiteId(site_a_id);
            let site_b = SiteId(site_b_id);
            let mut resolver = ConflictResolver::new();
            let record = resolver.resolve(inode, site_a, seq_a, ts_a, site_b, seq_b, ts_b);
            prop_assert_eq!(record.winner, site_a);
        }

        /// LWW: when timestamps tie, higher sequence wins.
        #[test]
        fn prop_lww_seq_tiebreak(
            inode in 1u64..100000,
            site_a_id in 1u64..50,
            site_b_id in 51u64..100,
            ts in 1u64..u64::MAX,
            seq_a in 1u64..1000,
            seq_b_delta in 1u64..500,
        ) {
            let seq_b = seq_a.saturating_sub(seq_b_delta); // seq_a > seq_b
            prop_assume!(seq_a > seq_b);
            let site_a = SiteId(site_a_id);
            let site_b = SiteId(site_b_id);
            let mut resolver = ConflictResolver::new();
            let record = resolver.resolve(inode, site_a, seq_a, ts, site_b, seq_b, ts);
            prop_assert_eq!(record.winner, site_a);
        }

        /// Conflict records are persisted and retrievable for any number of conflicts.
        #[test]
        fn prop_conflict_history_length(
            count in 1usize..50,
        ) {
            let mut resolver = ConflictResolver::new();
            for i in 0..count {
                let ts_a = (i as u64) * 100 + 100;
                let ts_b = ts_a + 1; // site_b always wins
                resolver.resolve(
                    i as u64,
                    SiteId(1),
                    i as u64,
                    ts_a,
                    SiteId(2),
                    i as u64,
                    ts_b,
                );
            }
            prop_assert_eq!(resolver.conflicts().len(), count);
        }

        /// Conflict IDs are monotonically increasing.
        #[test]
        fn prop_conflict_id_monotonic(
            count in 2usize..30,
        ) {
            let mut resolver = ConflictResolver::new();
            for i in 0..count {
                resolver.resolve(
                    i as u64,
                    SiteId(1),
                    i as u64,
                    (i as u64) * 1000,
                    SiteId(2),
                    i as u64,
                    (i as u64) * 1000 + 1,
                );
            }
            let conflicts = resolver.conflicts();
            for w in conflicts.windows(2) {
                prop_assert!(w[1].conflict_id > w[0].conflict_id);
            }
        }
    }
}
```

---

## STEP 5: Add proptest tests to `compression.rs`

At the bottom of `/home/cfs/claudefs/crates/claudefs-repl/src/compression.rs`, after all existing test code, add:

```rust
#[cfg(test)]
mod proptest_compression {
    use super::*;
    use crate::journal::{JournalEntry, OpKind};
    use crate::conduit::EntryBatch;
    use proptest::prelude::*;

    fn make_batch(entries_n: usize, payload_size: usize) -> EntryBatch {
        let payload = vec![42u8; payload_size];
        let entries: Vec<JournalEntry> = (0..entries_n)
            .map(|i| JournalEntry::new(i as u64, 0, 1, i as u64 * 1000, i as u64, OpKind::Write, payload.clone()))
            .collect();
        EntryBatch::new(1, entries, 0)
    }

    proptest! {
        /// LZ4 compress→decompress is a lossless roundtrip for arbitrary entry counts and payloads.
        #[test]
        fn prop_lz4_batch_roundtrip(
            entries_n in 0usize..50,
            payload_size in 0usize..512,
        ) {
            let config = CompressionConfig {
                algo: CompressionAlgo::Lz4,
                zstd_level: 3,
                min_compress_bytes: 0, // always compress
            };
            let compressor = BatchCompressor::new(config);
            let batch = make_batch(entries_n, payload_size);
            let compressed = compressor.compress(&batch).unwrap();
            let decompressed = compressor.decompress(&compressed).unwrap();
            prop_assert_eq!(decompressed.source_site_id, batch.source_site_id);
            prop_assert_eq!(decompressed.batch_seq, batch.batch_seq);
            prop_assert_eq!(decompressed.entries.len(), batch.entries.len());
        }

        /// Zstd compress→decompress is a lossless roundtrip.
        #[test]
        fn prop_zstd_batch_roundtrip(
            entries_n in 0usize..50,
            payload_size in 0usize..512,
        ) {
            let config = CompressionConfig {
                algo: CompressionAlgo::Zstd,
                zstd_level: 3,
                min_compress_bytes: 0,
            };
            let compressor = BatchCompressor::new(config);
            let batch = make_batch(entries_n, payload_size);
            let compressed = compressor.compress(&batch).unwrap();
            let decompressed = compressor.decompress(&compressed).unwrap();
            prop_assert_eq!(decompressed.entries.len(), batch.entries.len());
        }

        /// None algorithm passes data through unchanged (byte-for-byte roundtrip).
        #[test]
        fn prop_none_batch_roundtrip(
            entries_n in 0usize..30,
            payload_size in 0usize..256,
        ) {
            let config = CompressionConfig {
                algo: CompressionAlgo::None,
                zstd_level: 3,
                min_compress_bytes: 0,
            };
            let compressor = BatchCompressor::new(config);
            let batch = make_batch(entries_n, payload_size);
            let compressed = compressor.compress(&batch).unwrap();
            prop_assert_eq!(compressed.algo, CompressionAlgo::None);
            prop_assert_eq!(compressed.original_bytes, compressed.compressed_bytes);
            let decompressed = compressor.decompress(&compressed).unwrap();
            prop_assert_eq!(decompressed.entries.len(), batch.entries.len());
        }

        /// Raw bytes roundtrip through compress_bytes/decompress_bytes for LZ4.
        #[test]
        fn prop_lz4_raw_bytes_roundtrip(
            data in prop::collection::vec(0u8..=255, 0..10000),
        ) {
            let config = CompressionConfig {
                algo: CompressionAlgo::Lz4,
                zstd_level: 3,
                min_compress_bytes: 0,
            };
            let compressor = BatchCompressor::new(config);
            let (compressed, algo) = compressor.compress_bytes(&data).unwrap();
            let decompressed = compressor.decompress_bytes(&compressed, algo).unwrap();
            prop_assert_eq!(decompressed, data);
        }

        /// Raw bytes roundtrip through compress_bytes/decompress_bytes for Zstd.
        #[test]
        fn prop_zstd_raw_bytes_roundtrip(
            data in prop::collection::vec(0u8..=255, 0..10000),
        ) {
            let config = CompressionConfig {
                algo: CompressionAlgo::Zstd,
                zstd_level: 3,
                min_compress_bytes: 0,
            };
            let compressor = BatchCompressor::new(config);
            let (compressed, algo) = compressor.compress_bytes(&data).unwrap();
            let decompressed = compressor.decompress_bytes(&compressed, algo).unwrap();
            prop_assert_eq!(decompressed, data);
        }
    }
}
```

---

## STEP 6: Add proptest tests to `wal.rs`

At the bottom of `/home/cfs/claudefs/crates/claudefs-repl/src/wal.rs`, after all existing tests, add:

```rust
#[cfg(test)]
mod proptest_wal {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Advancing cursor always moves forward (monotonicity).
        #[test]
        fn prop_cursor_monotonic(
            site_id in 1u64..100,
            shard_id in 0u32..256,
            seqs in prop::collection::vec(0u64..10000, 1..30),
        ) {
            let mut wal = ReplicationWal::new();
            let mut last_seq = 0u64;
            for seq in seqs {
                let new_seq = last_seq.max(seq);
                wal.advance(site_id, shard_id, new_seq, new_seq * 1000, 1);
                let cursor = wal.cursor(site_id, shard_id);
                prop_assert!(cursor >= last_seq);
                last_seq = cursor;
            }
        }

        /// Cursor for unknown site/shard returns 0.
        #[test]
        fn prop_unknown_cursor_is_zero(
            site_id in 100u64..200,
            shard_id in 0u32..256,
        ) {
            let wal = ReplicationWal::new();
            prop_assert_eq!(wal.cursor(site_id, shard_id), 0);
        }

        /// WAL history grows by one per advance call.
        #[test]
        fn prop_history_grows_per_advance(
            count in 1usize..50,
        ) {
            let mut wal = ReplicationWal::new();
            for i in 0..count {
                wal.advance(1, 0, i as u64 * 10, i as u64 * 1000, 10);
            }
            prop_assert_eq!(wal.history().len(), count);
        }

        /// After compaction, all_cursors still has the latest sequence per site/shard.
        #[test]
        fn prop_compaction_preserves_latest(
            site_id in 1u64..10,
            shard_id in 0u32..4,
            seqs in prop::collection::vec(1u64..1000, 2..20),
        ) {
            let mut wal = ReplicationWal::new();
            let mut max_seq = 0u64;
            for seq in &seqs {
                wal.advance(site_id, shard_id, *seq, *seq * 1000, 1);
                max_seq = max_seq.max(*seq);
            }
            wal.compact(seqs.len() * 2); // keep_recent larger than history
            prop_assert_eq!(wal.cursor(site_id, shard_id), max_seq);
        }
    }
}
```

---

## STEP 7: Add `ReplicationSla` and `SlaMonitor` to `repl_qos.rs`

In `/home/cfs/claudefs/crates/claudefs-repl/src/repl_qos.rs`, append the following new types and implementation at the end of the file (before the `#[cfg(test)]` block):

```rust
/// SLA target thresholds for replication performance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationSla {
    /// Maximum allowed replication lag in number of journal entries.
    pub max_lag_entries: u64,
    /// Maximum allowed replication lag in seconds.
    pub max_lag_secs: u64,
    /// Minimum required replication bandwidth in bytes/sec.
    pub min_bandwidth_bps: u64,
    /// Target maximum batch latency in milliseconds.
    pub max_batch_latency_ms: u64,
}

impl ReplicationSla {
    /// Create a new SLA definition.
    pub fn new(
        max_lag_entries: u64,
        max_lag_secs: u64,
        min_bandwidth_bps: u64,
        max_batch_latency_ms: u64,
    ) -> Self {
        Self {
            max_lag_entries,
            max_lag_secs,
            min_bandwidth_bps,
            max_batch_latency_ms,
        }
    }

    /// Strict SLA: low lag, high bandwidth (for critical workloads).
    pub fn strict() -> Self {
        Self::new(100, 5, 100_000_000, 50)
    }

    /// Relaxed SLA: higher tolerance (for background workloads).
    pub fn relaxed() -> Self {
        Self::new(10_000, 300, 1_000_000, 5000)
    }
}

/// SLA violation types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlaViolation {
    /// Replication lag exceeded the entry threshold.
    LagEntries { current: u64, max: u64 },
    /// Replication lag exceeded the time threshold.
    LagTime { current_secs: u64, max_secs: u64 },
    /// Bandwidth dropped below minimum.
    LowBandwidth { current_bps: u64, min_bps: u64 },
    /// Batch latency exceeded threshold.
    HighLatency { current_ms: u64, max_ms: u64 },
}

/// SLA sample: a snapshot of current replication metrics.
#[derive(Debug, Clone)]
pub struct SlaSample {
    /// Current replication lag in entries.
    pub lag_entries: u64,
    /// Current lag in seconds.
    pub lag_secs: u64,
    /// Current bandwidth in bytes/sec.
    pub bandwidth_bps: u64,
    /// Last batch latency in ms.
    pub last_batch_latency_ms: u64,
}

/// SLA monitor that evaluates samples against the configured SLA.
#[derive(Debug)]
pub struct SlaMonitor {
    sla: ReplicationSla,
    violations: Vec<SlaViolation>,
    total_checks: u64,
    total_violations: u64,
}

impl SlaMonitor {
    /// Create a new SLA monitor.
    pub fn new(sla: ReplicationSla) -> Self {
        Self {
            sla,
            violations: Vec::new(),
            total_checks: 0,
            total_violations: 0,
        }
    }

    /// Evaluate a sample against the SLA. Returns any new violations found.
    pub fn check(&mut self, sample: &SlaSample) -> Vec<SlaViolation> {
        self.total_checks += 1;
        let mut new_violations = Vec::new();

        if sample.lag_entries > self.sla.max_lag_entries {
            let v = SlaViolation::LagEntries {
                current: sample.lag_entries,
                max: self.sla.max_lag_entries,
            };
            new_violations.push(v);
        }

        if sample.lag_secs > self.sla.max_lag_secs {
            let v = SlaViolation::LagTime {
                current_secs: sample.lag_secs,
                max_secs: self.sla.max_lag_secs,
            };
            new_violations.push(v);
        }

        if sample.bandwidth_bps < self.sla.min_bandwidth_bps && sample.bandwidth_bps > 0 {
            let v = SlaViolation::LowBandwidth {
                current_bps: sample.bandwidth_bps,
                min_bps: self.sla.min_bandwidth_bps,
            };
            new_violations.push(v);
        }

        if sample.last_batch_latency_ms > self.sla.max_batch_latency_ms {
            let v = SlaViolation::HighLatency {
                current_ms: sample.last_batch_latency_ms,
                max_ms: self.sla.max_batch_latency_ms,
            };
            new_violations.push(v);
        }

        if !new_violations.is_empty() {
            self.total_violations += new_violations.len() as u64;
            tracing::warn!(
                violations = new_violations.len(),
                "SLA violation(s) detected"
            );
        }
        self.violations.extend(new_violations.clone());
        new_violations
    }

    /// Return all accumulated violations.
    pub fn violations(&self) -> &[SlaViolation] {
        &self.violations
    }

    /// Clear the violations log.
    pub fn clear_violations(&mut self) {
        self.violations.clear();
    }

    /// Return total checks performed.
    pub fn total_checks(&self) -> u64 {
        self.total_checks
    }

    /// Return total violations detected.
    pub fn total_violations(&self) -> u64 {
        self.total_violations
    }

    /// Return the configured SLA.
    pub fn sla(&self) -> &ReplicationSla {
        &self.sla
    }

    /// Returns true if the SLA is currently being met (no violations in the last check).
    pub fn is_healthy(&self, sample: &SlaSample) -> bool {
        sample.lag_entries <= self.sla.max_lag_entries
            && sample.lag_secs <= self.sla.max_lag_secs
            && (sample.bandwidth_bps == 0 || sample.bandwidth_bps >= self.sla.min_bandwidth_bps)
            && sample.last_batch_latency_ms <= self.sla.max_batch_latency_ms
    }
}
```

And in the existing `#[cfg(test)]` block at the bottom of `repl_qos.rs`, add tests for SlaMonitor. Also add a new proptest module after that block:

Add these tests inside the existing `mod tests { ... }` block:

```rust
    #[test]
    fn test_sla_monitor_no_violation() {
        let sla = ReplicationSla::new(1000, 60, 10_000_000, 200);
        let mut monitor = SlaMonitor::new(sla);
        let sample = SlaSample {
            lag_entries: 100,
            lag_secs: 5,
            bandwidth_bps: 50_000_000,
            last_batch_latency_ms: 10,
        };
        let violations = monitor.check(&sample);
        assert!(violations.is_empty());
        assert_eq!(monitor.total_checks(), 1);
        assert_eq!(monitor.total_violations(), 0);
        assert!(monitor.is_healthy(&sample));
    }

    #[test]
    fn test_sla_monitor_lag_entries_violation() {
        let sla = ReplicationSla::new(100, 60, 0, 1000);
        let mut monitor = SlaMonitor::new(sla);
        let sample = SlaSample {
            lag_entries: 500,
            lag_secs: 5,
            bandwidth_bps: 0,
            last_batch_latency_ms: 10,
        };
        let violations = monitor.check(&sample);
        assert_eq!(violations.len(), 1);
        assert!(matches!(violations[0], SlaViolation::LagEntries { current: 500, max: 100 }));
    }

    #[test]
    fn test_sla_monitor_lag_time_violation() {
        let sla = ReplicationSla::new(10000, 30, 0, 1000);
        let mut monitor = SlaMonitor::new(sla);
        let sample = SlaSample {
            lag_entries: 100,
            lag_secs: 120,
            bandwidth_bps: 0,
            last_batch_latency_ms: 10,
        };
        let violations = monitor.check(&sample);
        assert_eq!(violations.len(), 1);
        assert!(matches!(violations[0], SlaViolation::LagTime { current_secs: 120, max_secs: 30 }));
    }

    #[test]
    fn test_sla_monitor_low_bandwidth_violation() {
        let sla = ReplicationSla::new(10000, 300, 100_000_000, 1000);
        let mut monitor = SlaMonitor::new(sla);
        let sample = SlaSample {
            lag_entries: 100,
            lag_secs: 5,
            bandwidth_bps: 5_000_000,
            last_batch_latency_ms: 10,
        };
        let violations = monitor.check(&sample);
        assert_eq!(violations.len(), 1);
        assert!(matches!(violations[0], SlaViolation::LowBandwidth { .. }));
    }

    #[test]
    fn test_sla_monitor_high_latency_violation() {
        let sla = ReplicationSla::new(10000, 300, 0, 100);
        let mut monitor = SlaMonitor::new(sla);
        let sample = SlaSample {
            lag_entries: 100,
            lag_secs: 5,
            bandwidth_bps: 0,
            last_batch_latency_ms: 500,
        };
        let violations = monitor.check(&sample);
        assert_eq!(violations.len(), 1);
        assert!(matches!(violations[0], SlaViolation::HighLatency { current_ms: 500, max_ms: 100 }));
    }

    #[test]
    fn test_sla_monitor_multiple_violations() {
        let sla = ReplicationSla::strict();
        let mut monitor = SlaMonitor::new(sla);
        let sample = SlaSample {
            lag_entries: 1000, // > 100
            lag_secs: 100,     // > 5
            bandwidth_bps: 1_000, // < 100_000_000
            last_batch_latency_ms: 5000, // > 50
        };
        let violations = monitor.check(&sample);
        assert_eq!(violations.len(), 4);
    }

    #[test]
    fn test_sla_monitor_clear_violations() {
        let sla = ReplicationSla::new(100, 60, 0, 1000);
        let mut monitor = SlaMonitor::new(sla);
        let sample = SlaSample { lag_entries: 500, lag_secs: 5, bandwidth_bps: 0, last_batch_latency_ms: 10 };
        monitor.check(&sample);
        assert!(!monitor.violations().is_empty());
        monitor.clear_violations();
        assert!(monitor.violations().is_empty());
        assert_eq!(monitor.total_violations(), 1); // total count not cleared
    }

    #[test]
    fn test_sla_strict_relaxed_presets() {
        let strict = ReplicationSla::strict();
        let relaxed = ReplicationSla::relaxed();
        assert!(strict.max_lag_entries < relaxed.max_lag_entries);
        assert!(strict.max_lag_secs < relaxed.max_lag_secs);
        assert!(strict.min_bandwidth_bps > relaxed.min_bandwidth_bps);
        assert!(strict.max_batch_latency_ms < relaxed.max_batch_latency_ms);
    }
```

Then add a proptest module after the test block:

```rust
#[cfg(test)]
mod proptest_qos {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// is_healthy and check() agree: if is_healthy, check returns no violations.
        #[test]
        fn prop_healthy_implies_no_violations(
            lag_entries in 0u64..500,
            lag_secs in 0u64..30,
            bandwidth_bps in 10_000_000u64..200_000_000,
            latency_ms in 0u64..50,
        ) {
            let sla = ReplicationSla::new(500, 30, 10_000_000, 50);
            let mut monitor = SlaMonitor::new(sla.clone());
            let sample = SlaSample { lag_entries, lag_secs, bandwidth_bps, last_batch_latency_ms: latency_ms };
            if monitor.is_healthy(&sample) {
                let violations = monitor.check(&sample);
                prop_assert!(violations.is_empty(), "is_healthy but got violations: {:?}", violations);
            }
        }

        /// zero bandwidth_bps is never flagged as low bandwidth (startup/idle state).
        #[test]
        fn prop_zero_bandwidth_not_a_violation(
            lag_entries in 0u64..100,
            lag_secs in 0u64..30,
            latency_ms in 0u64..50,
        ) {
            let sla = ReplicationSla::new(100, 30, 100_000_000, 50);
            let mut monitor = SlaMonitor::new(sla);
            let sample = SlaSample { lag_entries, lag_secs, bandwidth_bps: 0, last_batch_latency_ms: latency_ms };
            let violations = monitor.check(&sample);
            let has_bw_violation = violations.iter().any(|v| matches!(v, SlaViolation::LowBandwidth { .. }));
            prop_assert!(!has_bw_violation);
        }

        /// Total checks counter always equals the number of check() calls.
        #[test]
        fn prop_total_checks_correct(
            count in 1usize..100,
        ) {
            let sla = ReplicationSla::relaxed();
            let mut monitor = SlaMonitor::new(sla);
            let sample = SlaSample { lag_entries: 0, lag_secs: 0, bandwidth_bps: 0, last_batch_latency_ms: 0 };
            for _ in 0..count {
                monitor.check(&sample);
            }
            prop_assert_eq!(monitor.total_checks(), count as u64);
        }
    }
}
```

---

## IMPORTANT NOTES

1. Do NOT change any existing code — only ADD new code.
2. Ensure all new code compiles with `cargo check -p claudefs-repl`.
3. The `ConflictResolver` must have a public `conflicts()` method returning `&[ConflictRecord]`. If it doesn't exist, add it to the `impl ConflictResolver` block:
   ```rust
   pub fn conflicts(&self) -> &[ConflictRecord] {
       &self.conflicts
   }
   ```
4. The `ReplicationWal` must have a public `history()` method returning `&[WalRecord]`. If it doesn't exist, add it to the `impl ReplicationWal` block:
   ```rust
   pub fn history(&self) -> &[WalRecord] {
       &self.history
   }
   ```
5. Check whether `ReplicationWal::compact(keep_recent: usize)` exists; if not, add it:
   ```rust
   pub fn compact(&mut self, keep_recent: usize) {
       if self.history.len() > keep_recent {
           let drain_count = self.history.len() - keep_recent;
           self.history.drain(0..drain_count);
       }
   }
   ```
6. Make sure all new proptest modules are added AFTER the existing test modules in each file.
7. The goal is for `cargo test -p claudefs-repl` to pass with significantly more tests than the current 741.

Now make all these changes to the files listed above.
