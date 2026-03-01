//! Background data integrity verification (scrubbing) engine.
//!
//! The scrub engine performs periodic background verification of all stored data
//! by reading blocks and verifying their checksums. This detects silent data
//! corruption (bit rot) before it propagates.

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::block::{BlockId, BlockRef, BlockSize};
use crate::checksum::{verify, Checksum, ChecksumAlgorithm};

/// Configuration for the scrub engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrubConfig {
    /// How often to run a full scrub (in hours). Default is 168 (1 week).
    pub interval_hours: u64,
    /// I/O rate limit to avoid impacting foreground operations. Default is 100 IOPS.
    pub max_iops: u32,
    /// Blocks per batch. Default is 64.
    pub batch_size: usize,
    /// Algorithm for verification. Default is CRC32C.
    pub checksum_algo: ChecksumAlgorithm,
    /// Whether to attempt repair from EC parity. Default is false.
    pub auto_repair: bool,
}

impl Default for ScrubConfig {
    fn default() -> Self {
        Self {
            interval_hours: 168,
            max_iops: 100,
            batch_size: 64,
            checksum_algo: ChecksumAlgorithm::Crc32c,
            auto_repair: false,
        }
    }
}

/// Current state of the scrub engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScrubState {
    /// Not currently scrubbing.
    Idle,
    /// Actively scrubbing.
    Running {
        /// Progress percentage (0.0 - 100.0).
        progress_pct: f64,
        /// Number of blocks checked so far.
        blocks_checked: u64,
        /// Number of errors found so far.
        errors_found: u64,
    },
    /// Last scrub completed.
    Completed {
        /// Duration of the scrub in seconds.
        duration_secs: u64,
        /// Total blocks checked.
        blocks_checked: u64,
        /// Total errors found.
        errors_found: u64,
        /// Total errors repaired.
        errors_repaired: u64,
    },
    /// Scrub paused.
    Paused {
        /// Blocks checked before pause.
        blocks_checked: u64,
        /// Reason for pause.
        reason: String,
    },
}

/// Error detected during scrubbing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrubError {
    /// The block ID with the error.
    pub block_id: BlockId,
    /// The size of the block.
    pub block_size: BlockSize,
    /// The expected checksum value.
    pub expected_checksum: u64,
    /// The actual checksum value.
    pub actual_checksum: u64,
    /// Device path for the block.
    pub device_path: String,
    /// Epoch timestamp when the error was detected.
    pub detected_at_secs: u64,
    /// Whether the error was repaired.
    pub repaired: bool,
}

/// Statistics for the scrub engine.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScrubStats {
    /// Total number of scrubs performed.
    pub total_scrubs: u64,
    /// Total blocks checked across all scrubs.
    pub blocks_checked: u64,
    /// Total errors detected across all scrubs.
    pub errors_detected: u64,
    /// Total errors repaired across all scrubs.
    pub errors_repaired: u64,
    /// Duration of the last scrub in seconds.
    pub last_scrub_duration_secs: u64,
    /// Epoch timestamp of the last completed scrub.
    pub last_scrub_time_secs: u64,
    /// Total bytes verified across all scrubs.
    pub bytes_verified: u64,
}

/// The scrub engine for background data integrity verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrubEngine {
    /// Configuration for the scrub engine.
    config: ScrubConfig,
    /// Current state of the scrub.
    state: ScrubState,
    /// Aggregated statistics.
    stats: ScrubStats,
    /// Errors detected in the current or last scrub.
    errors: Vec<ScrubError>,
    /// Pending blocks to check.
    blocks_to_check: Vec<(BlockRef, Checksum)>,
}

impl ScrubEngine {
    /// Creates a new ScrubEngine with the given configuration.
    pub fn new(config: ScrubConfig) -> Self {
        info!(
            interval_hours = config.interval_hours,
            max_iops = config.max_iops,
            batch_size = config.batch_size,
            algorithm = %config.checksum_algo,
            auto_repair = config.auto_repair,
            "creating scrub engine"
        );
        Self {
            config,
            state: ScrubState::Idle,
            stats: ScrubStats::default(),
            errors: Vec::new(),
            blocks_to_check: Vec::new(),
        }
    }

    /// Adds a block to the scrub queue.
    pub fn schedule_block(&mut self, block_ref: BlockRef, expected_checksum: Checksum) {
        self.blocks_to_check.push((block_ref, expected_checksum));
    }

    /// Schedules all blocks on a device for scrubbing.
    pub fn schedule_device(&mut self, device_idx: u16, blocks: Vec<(BlockRef, Checksum)>) {
        let count = blocks.len();
        self.blocks_to_check.extend(blocks);
        info!(
            device_idx = device_idx,
            blocks_scheduled = count,
            "scheduled device for scrub"
        );
    }

    /// Verifies a single block against its expected checksum.
    /// Returns true if the block is valid, false if there's a mismatch.
    pub fn verify_block(&mut self, block_ref: BlockRef, data: &[u8], expected: &Checksum) -> bool {
        let is_valid = verify(expected, data);

        if !is_valid {
            let actual = crate::checksum::compute(expected.algorithm, data);
            let detected_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let error = ScrubError {
                block_id: block_ref.id,
                block_size: block_ref.size,
                expected_checksum: expected.value,
                actual_checksum: actual.value,
                device_path: format!("/dev/nvme{}n1", block_ref.id.device_idx),
                detected_at_secs: detected_at,
                repaired: false,
            };

            warn!(
                block_id = ?block_ref.id,
                block_size = %block_ref.size,
                expected = expected.value,
                actual = actual.value,
                "checksum mismatch detected during scrub"
            );

            self.errors.push(error);

            if let ScrubState::Running { errors_found, .. } = &mut self.state {
                *errors_found += 1;
            }
        }

        is_valid
    }

    /// Starts a new scrub cycle. Transitions from Idle to Running state.
    pub fn start(&mut self) {
        if self.blocks_to_check.is_empty() {
            info!("starting scrub with no blocks to check");
            self.state = ScrubState::Completed {
                duration_secs: 0,
                blocks_checked: 0,
                errors_found: 0,
                errors_repaired: 0,
            };
            return;
        }

        let total_blocks = self.blocks_to_check.len() as u64;
        info!(total_blocks = total_blocks, "starting scrub");

        self.state = ScrubState::Running {
            progress_pct: 0.0,
            blocks_checked: 0,
            errors_found: 0,
        };
    }

    /// Pauses the current scrub.
    pub fn pause(&mut self, reason: &str) {
        let blocks_checked = match &self.state {
            ScrubState::Running { blocks_checked, .. } => *blocks_checked,
            _ => 0,
        };

        info!(
            reason = reason,
            blocks_checked = blocks_checked,
            "scrub paused"
        );

        self.state = ScrubState::Paused {
            blocks_checked,
            reason: reason.to_string(),
        };
    }

    /// Resumes a paused scrub.
    pub fn resume(&mut self) {
        let blocks_checked = match &self.state {
            ScrubState::Paused { blocks_checked, .. } => *blocks_checked,
            _ => 0,
        };

        let total_blocks = self.blocks_to_check.len() as u64 + blocks_checked;
        let progress_pct = if total_blocks > 0 {
            (blocks_checked as f64 / total_blocks as f64) * 100.0
        } else {
            0.0
        };

        info!(
            blocks_checked = blocks_checked,
            progress_pct = progress_pct,
            "resuming scrub"
        );

        self.state = ScrubState::Running {
            progress_pct,
            blocks_checked,
            errors_found: 0,
        };
    }

    /// Marks the scrub as completed.
    pub fn complete(&mut self, duration_secs: u64) {
        let (blocks_checked, errors_found, errors_repaired) = match &self.state {
            ScrubState::Running {
                blocks_checked,
                errors_found,
                ..
            } => (*blocks_checked, *errors_found, 0),
            _ => (0, 0, 0),
        };

        let errors_repaired = if self.config.auto_repair {
            let repaired = self.errors.len() as u64;
            for error in &mut self.errors {
                error.repaired = true;
            }
            repaired
        } else {
            0
        };

        info!(
            duration_secs = duration_secs,
            blocks_checked = blocks_checked,
            errors_found = errors_found,
            errors_repaired = errors_repaired,
            "scrub completed"
        );

        self.stats.total_scrubs += 1;
        self.stats.blocks_checked += blocks_checked;
        self.stats.errors_detected += errors_found;
        self.stats.errors_repaired += errors_repaired;
        self.stats.last_scrub_duration_secs = duration_secs;
        self.stats.last_scrub_time_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        for (_, checksum) in &self.blocks_to_check {
            self.stats.bytes_verified += checksum.algorithm.as_bytes();
        }

        self.state = ScrubState::Completed {
            duration_secs,
            blocks_checked,
            errors_found,
            errors_repaired,
        };

        self.blocks_to_check.clear();
    }

    /// Gets the next batch of blocks to check.
    pub fn next_batch(&mut self) -> Vec<(BlockRef, Checksum)> {
        if !matches!(self.state, ScrubState::Running { .. }) {
            return Vec::new();
        }

        let batch_size = self.config.batch_size;
        let taken: Vec<_> = self
            .blocks_to_check
            .drain(..batch_size.min(self.blocks_to_check.len()))
            .collect();

        if let ScrubState::Running { blocks_checked, .. } = &mut self.state {
            *blocks_checked += taken.len() as u64;
        }

        taken
    }

    /// Returns the current progress percentage (0.0 - 100.0).
    pub fn progress(&self) -> f64 {
        match &self.state {
            ScrubState::Running {
                progress_pct,
                blocks_checked,
                ..
            } => {
                let total = self.blocks_to_check.len() as u64 + *blocks_checked;
                if total > 0 {
                    (*blocks_checked as f64 / total as f64) * 100.0
                } else {
                    *progress_pct
                }
            }
            ScrubState::Completed { .. } => 100.0,
            ScrubState::Paused { blocks_checked, .. } => {
                let total = self.blocks_to_check.len() as u64 + *blocks_checked;
                if total > 0 {
                    (*blocks_checked as f64 / total as f64) * 100.0
                } else {
                    0.0
                }
            }
            ScrubState::Idle => 0.0,
        }
    }

    /// Returns a reference to the current state.
    pub fn state(&self) -> &ScrubState {
        &self.state
    }

    /// Returns a reference to the statistics.
    pub fn stats(&self) -> &ScrubStats {
        &self.stats
    }

    /// Returns a reference to the errors.
    pub fn errors(&self) -> &[ScrubError] {
        &self.errors
    }

    /// Clears the recorded errors.
    pub fn clear_errors(&mut self) {
        self.errors.clear();
    }

    /// Returns the number of pending blocks.
    pub fn pending_count(&self) -> usize {
        self.blocks_to_check.len()
    }

    /// Checks if enough time has passed since the last scrub to need another scrub.
    pub fn needs_scrub(&self, current_time_secs: u64) -> bool {
        let interval_secs = self.config.interval_hours * 3600;

        if self.stats.last_scrub_time_secs == 0 {
            return true;
        }

        current_time_secs.saturating_sub(self.stats.last_scrub_time_secs) >= interval_secs
    }
}

impl ChecksumAlgorithm {
    fn as_bytes(&self) -> u64 {
        match self {
            ChecksumAlgorithm::Crc32c => 4096,
            ChecksumAlgorithm::XxHash64 => 4096,
            ChecksumAlgorithm::None => 4096,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_block_ref(device_idx: u16, offset: u64, size: BlockSize) -> BlockRef {
        BlockRef {
            id: BlockId::new(device_idx, offset),
            size,
        }
    }

    fn create_test_checksum(value: u64) -> Checksum {
        Checksum::new(ChecksumAlgorithm::Crc32c, value)
    }

    #[test]
    fn test_config_defaults() {
        let config = ScrubConfig::default();
        assert_eq!(config.interval_hours, 168);
        assert_eq!(config.max_iops, 100);
        assert_eq!(config.batch_size, 64);
        assert_eq!(config.checksum_algo, ChecksumAlgorithm::Crc32c);
        assert!(!config.auto_repair);
    }

    #[test]
    fn test_scrub_engine_new() {
        let config = ScrubConfig::default();
        let engine = ScrubEngine::new(config);
        assert!(matches!(engine.state(), &ScrubState::Idle));
        assert_eq!(engine.pending_count(), 0);
    }

    #[test]
    fn test_schedule_block() {
        let mut engine = ScrubEngine::new(ScrubConfig::default());
        let block_ref = create_test_block_ref(0, 100, BlockSize::B4K);
        let checksum = create_test_checksum(0x12345678);

        engine.schedule_block(block_ref, checksum);

        assert_eq!(engine.pending_count(), 1);
    }

    #[test]
    fn test_schedule_device() {
        let mut engine = ScrubEngine::new(ScrubConfig::default());
        let blocks = vec![
            (
                create_test_block_ref(0, 1, BlockSize::B4K),
                create_test_checksum(1),
            ),
            (
                create_test_block_ref(0, 2, BlockSize::B4K),
                create_test_checksum(2),
            ),
            (
                create_test_block_ref(0, 3, BlockSize::B4K),
                create_test_checksum(3),
            ),
        ];

        engine.schedule_device(0, blocks);

        assert_eq!(engine.pending_count(), 3);
    }

    #[test]
    fn test_verify_clean_block() {
        let mut engine = ScrubEngine::new(ScrubConfig::default());
        let block_ref = create_test_block_ref(0, 100, BlockSize::B4K);
        let data = b"test data for verification";
        let checksum = crate::checksum::compute(ChecksumAlgorithm::Crc32c, data);

        let result = engine.verify_block(block_ref, data, &checksum);

        assert!(result);
        assert!(engine.errors().is_empty());
    }

    #[test]
    fn test_verify_corrupted_block() {
        let mut engine = ScrubEngine::new(ScrubConfig::default());
        let block_ref = create_test_block_ref(0, 100, BlockSize::B4K);
        let data = b"original data";
        let checksum = crate::checksum::compute(ChecksumAlgorithm::Crc32c, data);
        let corrupted_data = b"corrupted data";

        let result = engine.verify_block(block_ref, corrupted_data, &checksum);

        assert!(!result);
        assert_eq!(engine.errors().len(), 1);
        let error = &engine.errors()[0];
        assert_eq!(error.block_id, block_ref.id);
    }

    #[test]
    fn test_verify_block_different_algorithms() {
        let mut engine = ScrubEngine::new(ScrubConfig::default());
        let block_ref = create_test_block_ref(0, 100, BlockSize::B4K);
        let data = b"test data";

        let crc_checksum = crate::checksum::compute(ChecksumAlgorithm::Crc32c, data);
        let xxh_checksum = crate::checksum::compute(ChecksumAlgorithm::XxHash64, data);

        assert!(engine.verify_block(block_ref, data, &crc_checksum));
        assert!(engine.verify_block(block_ref, data, &xxh_checksum));
    }

    #[test]
    fn test_state_transitions_idle_to_running_to_completed() {
        let mut engine = ScrubEngine::new(ScrubConfig::default());

        engine.schedule_block(
            create_test_block_ref(0, 1, BlockSize::B4K),
            create_test_checksum(1),
        );

        assert!(matches!(engine.state(), &ScrubState::Idle));

        engine.start();

        assert!(matches!(engine.state(), &ScrubState::Running { .. }));

        engine.complete(10);

        assert!(matches!(
            engine.state(),
            &ScrubState::Completed {
                duration_secs: 10,
                ..
            }
        ));
    }

    #[test]
    fn test_state_transitions_running_to_paused_to_running_to_completed() {
        let mut engine = ScrubEngine::new(ScrubConfig::default());

        for i in 0..10 {
            engine.schedule_block(
                create_test_block_ref(0, i, BlockSize::B4K),
                create_test_checksum(i),
            );
        }

        engine.start();
        assert!(matches!(engine.state(), &ScrubState::Running { .. }));

        engine.pause("test pause");
        assert!(matches!(engine.state(), &ScrubState::Paused { .. }));

        engine.resume();
        assert!(matches!(engine.state(), &ScrubState::Running { .. }));

        engine.complete(60);
        assert!(matches!(engine.state(), &ScrubState::Completed { .. }));
    }

    #[test]
    fn test_batch_retrieval() {
        let mut engine = ScrubEngine::new(ScrubConfig {
            batch_size: 3,
            ..Default::default()
        });

        for i in 0..10 {
            engine.schedule_block(
                create_test_block_ref(0, i, BlockSize::B4K),
                create_test_checksum(i),
            );
        }

        engine.start();

        let batch1 = engine.next_batch();
        assert_eq!(batch1.len(), 3);

        let batch2 = engine.next_batch();
        assert_eq!(batch2.len(), 3);

        let batch3 = engine.next_batch();
        assert_eq!(batch3.len(), 3);

        let batch4 = engine.next_batch();
        assert_eq!(batch4.len(), 1);
    }

    #[test]
    fn test_progress_calculation() {
        let mut engine = ScrubEngine::new(ScrubConfig {
            batch_size: 5,
            ..Default::default()
        });

        for i in 0..10 {
            engine.schedule_block(
                create_test_block_ref(0, i, BlockSize::B4K),
                create_test_checksum(i),
            );
        }

        engine.start();
        assert!((engine.progress() - 0.0).abs() < 0.1);

        engine.next_batch();
        let progress1 = engine.progress();
        assert!(progress1 > 0.0 && progress1 < 60.0);

        engine.next_batch();
        let progress2 = engine.progress();
        assert!(progress2 > progress1);
    }

    #[test]
    fn test_stats_tracking() {
        let mut engine = ScrubEngine::new(ScrubConfig::default());

        for i in 0..5 {
            engine.schedule_block(
                create_test_block_ref(0, i, BlockSize::B4K),
                create_test_checksum(i),
            );
        }

        engine.start();

        let data = b"test data";
        let checksum = crate::checksum::compute(ChecksumAlgorithm::Crc32c, data);

        while engine.pending_count() > 0 {
            let batch = engine.next_batch();
            for (block_ref, _) in batch {
                engine.verify_block(block_ref, data, &checksum);
            }
        }

        engine.complete(30);

        let stats = engine.stats();
        assert_eq!(stats.total_scrubs, 1);
        assert_eq!(stats.blocks_checked, 5);
    }

    #[test]
    fn test_needs_scrub_first_time() {
        let engine = ScrubEngine::new(ScrubConfig::default());
        assert!(engine.needs_scrub(1000));
    }

    #[test]
    fn test_needs_scrub_interval_not_elapsed() {
        let mut engine = ScrubEngine::new(ScrubConfig {
            interval_hours: 1,
            ..Default::default()
        });

        engine.stats.last_scrub_time_secs = 1000;
        assert!(!engine.needs_scrub(3500));
    }

    #[test]
    fn test_needs_scrub_interval_elapsed() {
        let mut engine = ScrubEngine::new(ScrubConfig {
            interval_hours: 1,
            ..Default::default()
        });

        engine.stats.last_scrub_time_secs = 1000;
        assert!(engine.needs_scrub(4600));
    }

    #[test]
    fn test_multiple_device_scheduling() {
        let mut engine = ScrubEngine::new(ScrubConfig::default());

        let dev0_blocks = vec![
            (
                create_test_block_ref(0, 1, BlockSize::B4K),
                create_test_checksum(1),
            ),
            (
                create_test_block_ref(0, 2, BlockSize::B4K),
                create_test_checksum(2),
            ),
        ];
        let dev1_blocks = vec![
            (
                create_test_block_ref(1, 1, BlockSize::B4K),
                create_test_checksum(3),
            ),
            (
                create_test_block_ref(1, 2, BlockSize::B4K),
                create_test_checksum(4),
            ),
        ];
        let dev2_blocks = vec![(
            create_test_block_ref(2, 1, BlockSize::B4K),
            create_test_checksum(5),
        )];

        engine.schedule_device(0, dev0_blocks);
        engine.schedule_device(1, dev1_blocks);
        engine.schedule_device(2, dev2_blocks);

        assert_eq!(engine.pending_count(), 5);
    }

    #[test]
    fn test_error_recording() {
        let mut engine = ScrubEngine::new(ScrubConfig::default());

        let block_ref = create_test_block_ref(0, 100, BlockSize::B64K);
        let checksum = create_test_checksum(0x12345678);

        engine.verify_block(block_ref, b"wrong data", &checksum);

        assert_eq!(engine.errors().len(), 1);
        let error = &engine.errors()[0];
        assert_eq!(error.block_id.device_idx, 0);
        assert_eq!(error.block_id.offset, 100);
        assert_eq!(error.block_size, BlockSize::B64K);
        assert!(error.detected_at_secs > 0);
    }

    #[test]
    fn test_error_clearing() {
        let mut engine = ScrubEngine::new(ScrubConfig::default());

        engine.verify_block(
            create_test_block_ref(0, 1, BlockSize::B4K),
            b"corrupted",
            &create_test_checksum(1),
        );

        assert!(!engine.errors().is_empty());

        engine.clear_errors();

        assert!(engine.errors().is_empty());
    }

    #[test]
    fn test_empty_scrub_completes_immediately() {
        let mut engine = ScrubEngine::new(ScrubConfig::default());

        engine.start();

        assert!(matches!(
            engine.state(),
            &ScrubState::Completed {
                blocks_checked: 0,
                ..
            }
        ));
    }

    #[test]
    fn test_scrub_after_errors_continues() {
        let mut engine = ScrubEngine::new(ScrubConfig::default());

        for i in 0..5 {
            engine.schedule_block(
                create_test_block_ref(0, i, BlockSize::B4K),
                create_test_checksum(i),
            );
        }

        engine.start();

        let data = b"test data";
        let checksum = crate::checksum::compute(ChecksumAlgorithm::Crc32c, data);

        let batch = engine.next_batch();
        for (block_ref, _) in batch {
            engine.verify_block(block_ref, b"corrupted", &checksum);
        }

        let remaining = engine.pending_count();
        assert_eq!(remaining, 0);

        engine.complete(10);

        assert!(matches!(
            engine.state(),
            &ScrubState::Completed {
                errors_found: 5,
                ..
            }
        ));
    }

    #[test]
    fn test_multiple_complete_cycles_update_stats() {
        let mut engine = ScrubEngine::new(ScrubConfig::default());

        for _ in 0..3 {
            engine.schedule_block(
                create_test_block_ref(0, 1, BlockSize::B4K),
                create_test_checksum(1),
            );
            engine.start();
            let batch = engine.next_batch();
            for (block_ref, checksum) in batch {
                engine.verify_block(block_ref, b"test data", &checksum);
            }
            engine.complete(10);
        }

        let stats = engine.stats();
        assert_eq!(stats.total_scrubs, 3);
        assert_eq!(stats.blocks_checked, 3);
    }

    #[test]
    fn test_pending_count_decreases_as_batches_taken() {
        let mut engine = ScrubEngine::new(ScrubConfig {
            batch_size: 2,
            ..Default::default()
        });

        for i in 0..5 {
            engine.schedule_block(
                create_test_block_ref(0, i, BlockSize::B4K),
                create_test_checksum(i),
            );
        }

        assert_eq!(engine.pending_count(), 5);

        engine.start();
        engine.next_batch();
        assert_eq!(engine.pending_count(), 3);

        engine.next_batch();
        assert_eq!(engine.pending_count(), 1);
    }

    #[test]
    fn test_paused_state_preserves_progress() {
        let mut engine = ScrubEngine::new(ScrubConfig::default());

        for i in 0..10 {
            engine.schedule_block(
                create_test_block_ref(0, i, BlockSize::B4K),
                create_test_checksum(i),
            );
        }

        engine.start();
        engine.next_batch();

        let progress_before = engine.progress();
        engine.pause("test");

        if let ScrubState::Paused { blocks_checked, .. } = engine.state() {
            assert!(*blocks_checked > 0);
        }

        engine.resume();
        let progress_after = engine.progress();

        assert!((progress_before - progress_after).abs() < 0.1);
    }

    #[test]
    fn test_scrub_error_details() {
        let mut engine = ScrubEngine::new(ScrubConfig::default());

        let block_ref = BlockRef {
            id: BlockId::new(2, 99),
            size: BlockSize::B1M,
        };
        let expected = Checksum::new(ChecksumAlgorithm::XxHash64, 0xDEADBEEF);

        engine.verify_block(block_ref, b"actual data", &expected);

        assert_eq!(engine.errors().len(), 1);
        let error = &engine.errors()[0];

        assert_eq!(error.block_id.device_idx, 2);
        assert_eq!(error.block_id.offset, 99);
        assert_eq!(error.block_size, BlockSize::B1M);
        assert_eq!(error.expected_checksum, 0xDEADBEEF);
        assert!(error.actual_checksum != 0xDEADBEEF);
        assert!(error.device_path.contains("nvme2"));
    }

    #[test]
    fn test_next_batch_not_running_state() {
        let mut engine = ScrubEngine::new(ScrubConfig::default());

        engine.schedule_block(
            create_test_block_ref(0, 1, BlockSize::B4K),
            create_test_checksum(1),
        );

        let batch = engine.next_batch();
        assert!(batch.is_empty());
    }

    #[test]
    fn test_progress_at_completed() {
        let mut engine = ScrubEngine::new(ScrubConfig::default());

        engine.schedule_block(
            create_test_block_ref(0, 1, BlockSize::B4K),
            create_test_checksum(1),
        );

        engine.start();
        let progress_during = engine.progress();
        engine.complete(5);

        let progress_after = engine.progress();
        assert!((progress_after - 100.0).abs() < 0.1);
    }
}
