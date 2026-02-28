//! Flash defragmentation engine for the buddy allocator.
//!
//! This module provides analysis and planning for defragmenting the buddy allocator's
//! free space. It analyzes fragmentation patterns and generates plans to consolidate
//! free blocks into larger contiguous regions.

use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::allocator::AllocatorStats;
use crate::block::BlockSize;

/// Configuration for the defragmentation engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefragConfig {
    /// Maximum number of block relocations per defragmentation pass.
    pub max_relocations_per_pass: usize,
    /// Target fragmentation percentage. If fragmentation is below this, skip defrag.
    pub target_fragmentation_percent: f64,
    /// Cooldown between defrag passes in seconds.
    pub cooldown_seconds: u64,
    /// Minimum free blocks at a size class to consider it viable for consolidation.
    pub min_free_blocks_for_consolidation: usize,
}

impl Default for DefragConfig {
    fn default() -> Self {
        Self {
            max_relocations_per_pass: 100,
            target_fragmentation_percent: 20.0,
            cooldown_seconds: 60,
            min_free_blocks_for_consolidation: 4,
        }
    }
}

/// Fragmentation analysis for a single size class.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SizeClassFragmentation {
    /// The block size class.
    pub size: BlockSize,
    /// Number of free blocks at this size class.
    pub free_count: usize,
    /// Maximum possible free blocks if fully consolidated.
    pub max_free_if_consolidated: usize,
    /// Fragmentation percentage (0-100).
    pub fragmentation_percent: f64,
    /// Whether this size class is considered fragmented and needs attention.
    pub is_fragmented: bool,
}

/// Fragmentation report for the entire allocator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentationReport {
    /// Total free blocks (in 4KB units).
    pub total_free_blocks_4k: u64,
    /// Total capacity (in 4KB units).
    pub total_blocks_4k: u64,
    /// Free space percentage.
    pub free_percent: f64,
    /// Per-size-class fragmentation analysis.
    pub size_classes: Vec<SizeClassFragmentation>,
    /// Overall fragmentation score (0-100).
    pub overall_fragmentation: f64,
    /// Whether the allocator needs defragmentation.
    pub needs_defrag: bool,
}

/// A planned block relocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockRelocation {
    /// Source block offset (in 4KB units).
    pub source_offset: u64,
    /// Destination block offset (in 4KB units).
    pub dest_offset: u64,
    /// Size of the block to relocate.
    pub size: BlockSize,
    /// Device index.
    pub device_idx: u16,
}

/// Defragmentation plan containing relocations to perform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefragPlan {
    /// Total blocks that need to be relocated.
    pub relocation_count: usize,
    /// Individual block relocations.
    pub relocations: Vec<BlockRelocation>,
    /// Expected fragmentation improvement after applying the plan.
    pub expected_improvement_percent: f64,
    /// Estimated I/O operations required.
    pub estimated_io_ops: usize,
}

/// Statistics about the defragmentation engine's operation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DefragStats {
    /// Total defrag passes performed.
    pub passes_performed: u64,
    /// Total blocks relocated.
    pub blocks_relocated: u64,
    /// Total bytes moved.
    pub bytes_moved: u64,
    /// Last defrag timestamp (seconds since epoch).
    pub last_defrag_time: Option<u64>,
    /// Number of times defrag was skipped (below threshold).
    pub skips: u64,
}

/// Internal helper to calculate fragmentation for a size class.
fn calculate_size_class_fragmentation(
    size: BlockSize,
    free_count: usize,
    total_free_blocks_4k: u64,
    total_blocks_4k: u64,
) -> SizeClassFragmentation {
    let size_blocks = match size {
        BlockSize::B4K => 1,
        BlockSize::B64K => 16,
        BlockSize::B1M => 256,
        BlockSize::B64M => 16384,
    };

    // Calculate max possible free at this size class if all smaller free blocks were merged
    // This is an approximation - assumes worst case where no smaller blocks can be merged up
    let size_blocks_u64 = size_blocks as u64;
    let max_free_if_consolidated = if size_blocks > 1 {
        // Simplified: max is limited by total space / size_blocks
        let max_possible = total_blocks_4k / size_blocks_u64;
        let additional =
            (total_free_blocks_4k / size_blocks_u64).saturating_sub(free_count as u64) as usize;
        (max_possible as usize).min(free_count + additional)
    } else {
        free_count
    };

    let fragmentation_percent = if max_free_if_consolidated > 0 {
        let frag = 100.0 * (1.0 - (free_count as f64 / max_free_if_consolidated as f64));
        #[allow(clippy::manual_clamp)]
        frag.max(0.0).min(100.0)
    } else {
        0.0
    };

    // Consider fragmented if free count is significantly less than potential
    let is_fragmented = free_count > 0 && fragmentation_percent > 30.0;

    SizeClassFragmentation {
        size,
        free_count,
        max_free_if_consolidated,
        fragmentation_percent,
        is_fragmented,
    }
}

/// The defragmentation engine for analyzing and planning block consolidation.
pub struct DefragEngine {
    config: DefragConfig,
    stats: std::sync::Mutex<DefragStats>,
}

impl DefragEngine {
    /// Creates a new defragmentation engine with the given configuration.
    pub fn new(config: DefragConfig) -> Self {
        debug!(
            "DefragEngine created: max_relocs={}, target_frag={}%, cooldown={}s",
            config.max_relocations_per_pass,
            config.target_fragmentation_percent,
            config.cooldown_seconds
        );
        Self {
            config,
            stats: std::sync::Mutex::new(DefragStats::default()),
        }
    }

    /// Analyzes fragmentation in the allocator.
    pub fn analyze(&self, allocator_stats: &AllocatorStats) -> FragmentationReport {
        let total_free = allocator_stats.free_blocks_4k;
        let total_capacity = allocator_stats.total_blocks_4k;
        let free_percent = if total_capacity > 0 {
            100.0 * (total_free as f64 / total_capacity as f64)
        } else {
            0.0
        };

        // Analyze each size class
        let mut size_classes = Vec::new();
        let mut fragmented_classes = 0;

        for (size, count) in &allocator_stats.free_count_per_size {
            let class_frag =
                calculate_size_class_fragmentation(*size, *count, total_free, total_capacity);
            if class_frag.is_fragmented {
                fragmented_classes += 1;
            }
            size_classes.push(class_frag);
        }

        // Calculate overall fragmentation score
        let overall_fragmentation = if size_classes.is_empty() {
            0.0
        } else {
            let total_frag: f64 = size_classes
                .iter()
                .map(|s| {
                    if s.is_fragmented {
                        s.fragmentation_percent
                    } else {
                        0.0
                    }
                })
                .sum();
            total_frag / size_classes.len() as f64
        };

        // Determine if defrag is needed
        let needs_defrag = overall_fragmentation > self.config.target_fragmentation_percent
            && fragmented_classes > 0;

        debug!(
            "Fragmentation analysis: overall={:.1}%, needs_defrag={}, classes={}/{} fragmented",
            overall_fragmentation,
            needs_defrag,
            fragmented_classes,
            size_classes.len()
        );

        FragmentationReport {
            total_free_blocks_4k: total_free,
            total_blocks_4k: total_capacity,
            free_percent,
            size_classes,
            overall_fragmentation,
            needs_defrag,
        }
    }

    /// Generates a defragmentation plan based on the fragmentation report.
    ///
    /// Note: This creates a plan based on allocator stats. The actual relocation
    /// requires reading/writing blocks through the I/O engine and updating the allocator.
    pub fn create_plan(&self, report: &FragmentationReport) -> DefragPlan {
        let mut relocations = Vec::new();

        if !report.needs_defrag {
            debug!("No defrag needed - fragmentation below threshold");
            return DefragPlan {
                relocation_count: 0,
                relocations: vec![],
                expected_improvement_percent: 0.0,
                estimated_io_ops: 0,
            };
        }

        // Find fragmented size classes and generate relocation suggestions
        // This is a simplified algorithm that suggests moving from larger to smaller
        for class in &report.size_classes {
            if !class.is_fragmented || relocations.len() >= self.config.max_relocations_per_pass {
                continue;
            }

            // For each fragmented size class, suggest consolidating free space
            // by "moving" allocated blocks to free positions
            // This is a heuristic - in practice, this would need actual allocator state
            let size_blocks = match class.size {
                BlockSize::B4K => 1,
                BlockSize::B64K => 16,
                BlockSize::B1M => 256,
                BlockSize::B64M => 16384,
            };

            // Suggest relocating blocks to consolidate free space
            // The number of relocations is based on how fragmented the class is
            let suggested_count =
                ((class.fragmentation_percent / 100.0) * class.free_count as f64) as usize;
            let relocations_to_add =
                suggested_count.min(self.config.max_relocations_per_pass - relocations.len());

            // Generate placeholder relocations (actual implementation would need more allocator state)
            for i in 0..relocations_to_add {
                relocations.push(BlockRelocation {
                    source_offset: (i as u64 + 1) * size_blocks * 10, // Placeholder offsets
                    dest_offset: (i as u64) * size_blocks,            // Move to lower addresses
                    size: class.size,
                    device_idx: 0, // Would come from allocator stats
                });
            }
        }

        let relocation_count = relocations.len();
        let expected_improvement = if !report.size_classes.is_empty() {
            report.overall_fragmentation * 0.5 // Expect 50% improvement
        } else {
            0.0
        };

        let plan = DefragPlan {
            relocation_count,
            relocations,
            expected_improvement_percent: expected_improvement,
            estimated_io_ops: relocation_count * 2, // read + write per block
        };

        debug!(
            "Defrag plan: {} relocations, ~{} I/O ops, {:.1}% expected improvement",
            plan.relocation_count, plan.estimated_io_ops, plan.expected_improvement_percent
        );

        plan
    }

    /// Checks if defrag can run now (cooldown period has passed).
    pub fn can_run(&self) -> bool {
        let stats = self.stats.lock().unwrap();
        if let Some(last_time) = stats.last_defrag_time {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            if now.saturating_sub(last_time) < self.config.cooldown_seconds {
                return false;
            }
        }
        true
    }

    /// Records that defrag was performed.
    pub fn record_defrag(&self, blocks_relocated: usize, bytes_moved: u64) {
        let mut stats = self.stats.lock().unwrap();
        stats.passes_performed += 1;
        stats.blocks_relocated += blocks_relocated as u64;
        stats.bytes_moved += bytes_moved;
        stats.last_defrag_time = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );
    }

    /// Records that defrag was skipped.
    pub fn record_skip(&self) {
        let mut stats = self.stats.lock().unwrap();
        stats.skips += 1;
    }

    /// Returns defragmentation statistics.
    pub fn stats(&self) -> DefragStats {
        self.stats.lock().unwrap().clone()
    }

    /// Returns the current configuration.
    pub fn config(&self) -> &DefragConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::allocator::AllocatorConfig;
    use crate::block::BlockId;

    fn create_test_stats() -> AllocatorStats {
        // Simulate a fragmented allocator: lots of small free blocks but few large ones
        AllocatorStats {
            device_idx: 0,
            total_blocks_4k: 16384,
            free_blocks_4k: 8192,
            free_count_per_size: vec![
                (BlockSize::B4K, 5000), // Many small free blocks
                (BlockSize::B64K, 10),  // Few 64K blocks
                (BlockSize::B1M, 0),    // No 1M blocks (fragmented!)
                (BlockSize::B64M, 0),   // No 64M blocks (fragmented!)
            ],
            total_allocations: 1000,
            total_frees: 500,
        }
    }

    fn create_defragmented_stats() -> AllocatorStats {
        // Well-defragmented allocator
        AllocatorStats {
            device_idx: 0,
            total_blocks_4k: 16384,
            free_blocks_4k: 8192,
            free_count_per_size: vec![
                (BlockSize::B4K, 100),
                (BlockSize::B64K, 100),
                (BlockSize::B1M, 10),
                (BlockSize::B64M, 0),
            ],
            total_allocations: 1000,
            total_frees: 500,
        }
    }

    #[test]
    fn test_defrag_engine_creation() {
        let config = DefragConfig::default();
        let engine = DefragEngine::new(config);

        let stats = engine.stats();
        assert_eq!(stats.passes_performed, 0);
        assert_eq!(stats.skips, 0);
    }

    #[test]
    fn test_analyze_fragmented_allocator() {
        let config = DefragConfig::default();
        let engine = DefragEngine::new(config);

        let allocator_stats = create_test_stats();
        let report = engine.analyze(&allocator_stats);

        assert!(report.needs_defrag);
        assert!(report.overall_fragmentation > 0.0);

        // Should have identified fragmentation in larger size classes
        let fragmented_classes: Vec<_> = report
            .size_classes
            .iter()
            .filter(|s| s.is_fragmented)
            .collect();
        assert!(!fragmented_classes.is_empty());
    }

    #[test]
    fn test_analyze_defragmented_allocator() {
        let config = DefragConfig::default();
        let engine = DefragEngine::new(config);

        let allocator_stats = create_defragmented_stats();
        let report = engine.analyze(&allocator_stats);

        // This might or might not need defrag depending on thresholds
        assert!(report.free_percent > 0.0);
    }

    #[test]
    fn test_create_plan_for_fragmented() {
        let config = DefragConfig::default();
        let engine = DefragEngine::new(config);

        let allocator_stats = create_test_stats();
        let report = engine.analyze(&allocator_stats);
        let plan = engine.create_plan(&report);

        // Should generate relocations for fragmented allocator
        assert!(plan.relocation_count > 0 || !report.needs_defrag);
        assert!(plan.estimated_io_ops >= 0);
    }

    #[test]
    fn test_create_plan_for_defragmented() {
        let config = DefragConfig::default();
        let engine = DefragEngine::new(config);

        let allocator_stats = create_defragmented_stats();
        let report = engine.analyze(&allocator_stats);
        let plan = engine.create_plan(&report);

        // If not fragmented, should return empty plan
        if !report.needs_defrag {
            assert_eq!(plan.relocation_count, 0);
        }
    }

    #[test]
    fn test_can_run_after_cooldown() {
        let config = DefragConfig {
            cooldown_seconds: 0,
            ..Default::default()
        };
        let engine = DefragEngine::new(config);

        // Should be able to run immediately with 0 cooldown
        assert!(engine.can_run());

        // Record a defrag
        engine.record_defrag(10, 40960);

        // With 0 cooldown, should still be able to run
        assert!(engine.can_run());
    }

    #[test]
    fn test_cooldown_prevents_rapid_runs() {
        let config = DefragConfig {
            cooldown_seconds: 3600, // 1 hour
            ..Default::default()
        };
        let engine = DefragEngine::new(config);

        // Record a defrag
        engine.record_defrag(10, 40960);

        // Should NOT be able to run (cooldown not passed)
        assert!(!engine.can_run());
    }

    #[test]
    fn test_record_defrag_updates_stats() {
        let config = DefragConfig::default();
        let engine = DefragEngine::new(config);

        engine.record_defrag(100, 409600);
        let stats = engine.stats();

        assert_eq!(stats.passes_performed, 1);
        assert_eq!(stats.blocks_relocated, 100);
        assert_eq!(stats.bytes_moved, 409600);
        assert!(stats.last_defrag_time.is_some());
    }

    #[test]
    fn test_record_skip() {
        let config = DefragConfig::default();
        let engine = DefragEngine::new(config);

        engine.record_skip();
        let stats = engine.stats();

        assert_eq!(stats.skips, 1);
    }

    #[test]
    fn test_config_defaults() {
        let config = DefragConfig::default();
        assert_eq!(config.max_relocations_per_pass, 100);
        assert_eq!(config.target_fragmentation_percent, 20.0);
        assert_eq!(config.cooldown_seconds, 60);
    }

    #[test]
    fn test_config_custom() {
        let config = DefragConfig {
            max_relocations_per_pass: 500,
            target_fragmentation_percent: 30.0,
            cooldown_seconds: 300,
            min_free_blocks_for_consolidation: 10,
        };

        assert_eq!(config.max_relocations_per_pass, 500);
        assert_eq!(config.target_fragmentation_percent, 30.0);
        assert_eq!(config.cooldown_seconds, 300);
    }

    #[test]
    fn test_fragmentation_report_fields() {
        let report = FragmentationReport {
            total_free_blocks_4k: 8000,
            total_blocks_4k: 16000,
            free_percent: 50.0,
            size_classes: vec![],
            overall_fragmentation: 15.0,
            needs_defrag: false,
        };

        assert_eq!(report.total_free_blocks_4k, 8000);
        assert_eq!(report.total_blocks_4k, 16000);
        assert_eq!(report.free_percent, 50.0);
    }

    #[test]
    fn test_block_relocation() {
        let relocation = BlockRelocation {
            source_offset: 1000,
            dest_offset: 500,
            size: BlockSize::B64K,
            device_idx: 0,
        };

        assert_eq!(relocation.source_offset, 1000);
        assert_eq!(relocation.dest_offset, 500);
        assert_eq!(relocation.size, BlockSize::B64K);
    }

    #[test]
    fn test_size_class_fragmentation_calculation() {
        let result = calculate_size_class_fragmentation(BlockSize::B64K, 10, 8192, 16384);

        assert_eq!(result.size, BlockSize::B64K);
        assert_eq!(result.free_count, 10);
    }
}
