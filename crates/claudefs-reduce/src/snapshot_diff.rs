use std::collections::HashSet;

pub type BlockHash = [u8; 32];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotBlock {
    pub hash: BlockHash,
    pub offset: u64,
    pub len: u32,
    pub segment_id: u64,
}

#[derive(Debug, Clone)]
pub struct SnapshotDiffConfig {
    pub batch_size: usize,
}

impl Default for SnapshotDiffConfig {
    fn default() -> Self {
        Self { batch_size: 1000 }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SnapshotDiffResult {
    pub added_blocks: Vec<BlockHash>,
    pub removed_blocks: Vec<BlockHash>,
    pub total_new_blocks: usize,
    pub total_base_blocks: usize,
}

impl SnapshotDiffResult {
    pub fn transfer_count(&self) -> usize {
        self.added_blocks.len()
    }

    pub fn is_identical(&self) -> bool {
        self.added_blocks.is_empty() && self.removed_blocks.is_empty()
    }

    pub fn change_ratio(&self) -> f64 {
        if self.total_new_blocks == 0 {
            return 0.0;
        }
        self.added_blocks.len() as f64 / self.total_new_blocks as f64
    }
}

pub struct SnapshotDiff {
    config: SnapshotDiffConfig,
}

impl SnapshotDiff {
    pub fn new(config: SnapshotDiffConfig) -> Self {
        Self { config }
    }

    pub fn compute(
        &self,
        base_blocks: &[SnapshotBlock],
        new_blocks: &[SnapshotBlock],
    ) -> SnapshotDiffResult {
        let base_hashes: HashSet<BlockHash> = base_blocks.iter().map(|b| b.hash).collect();
        let new_hashes: HashSet<BlockHash> = new_blocks.iter().map(|b| b.hash).collect();

        let added: Vec<BlockHash> = new_hashes
            .difference(&base_hashes)
            .copied()
            .take(self.config.batch_size)
            .collect();

        let removed: Vec<BlockHash> = base_hashes
            .difference(&new_hashes)
            .copied()
            .take(self.config.batch_size)
            .collect();

        SnapshotDiffResult {
            added_blocks: added,
            removed_blocks: removed,
            total_new_blocks: new_blocks.len(),
            total_base_blocks: base_blocks.len(),
        }
    }

    pub fn incremental_transfer(
        &self,
        base_blocks: &[SnapshotBlock],
        new_blocks: &[SnapshotBlock],
    ) -> Vec<BlockHash> {
        let base_hashes: HashSet<BlockHash> = base_blocks.iter().map(|b| b.hash).collect();
        new_blocks
            .iter()
            .filter(|b| !base_hashes.contains(&b.hash))
            .map(|b| b.hash)
            .take(self.config.batch_size)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_block(hash: u8, offset: u64, len: u32, segment: u64) -> SnapshotBlock {
        let mut h = [0u8; 32];
        h[0] = hash;
        SnapshotBlock {
            hash: h,
            offset,
            len,
            segment_id: segment,
        }
    }

    #[test]
    fn snapshot_diff_config_default() {
        let config = SnapshotDiffConfig::default();
        assert_eq!(config.batch_size, 1000);
    }

    #[test]
    fn compute_empty_snapshots() {
        let diff = SnapshotDiff::new(SnapshotDiffConfig::default());
        let result = diff.compute(&[], &[]);
        assert!(result.is_identical());
    }

    #[test]
    fn compute_identical_snapshots() {
        let diff = SnapshotDiff::new(SnapshotDiffConfig::default());
        let block = make_block(1, 0, 4096, 1);
        let result = diff.compute(&[block.clone()], &[block]);
        assert!(result.is_identical());
    }

    #[test]
    fn compute_added_blocks() {
        let diff = SnapshotDiff::new(SnapshotDiffConfig::default());
        let base = vec![make_block(1, 0, 4096, 1)];
        let new = vec![make_block(1, 0, 4096, 1), make_block(2, 4096, 4096, 1)];
        let result = diff.compute(&base, &new);
        assert_eq!(result.added_blocks.len(), 1);
    }

    #[test]
    fn compute_removed_blocks() {
        let diff = SnapshotDiff::new(SnapshotDiffConfig::default());
        let base = vec![make_block(1, 0, 4096, 1), make_block(2, 4096, 4096, 1)];
        let new = vec![make_block(1, 0, 4096, 1)];
        let result = diff.compute(&base, &new);
        assert_eq!(result.removed_blocks.len(), 1);
    }

    #[test]
    fn compute_both_added_and_removed() {
        let diff = SnapshotDiff::new(SnapshotDiffConfig::default());
        let base = vec![make_block(1, 0, 4096, 1)];
        let new = vec![make_block(2, 0, 4096, 1)];
        let result = diff.compute(&base, &new);
        assert_eq!(result.added_blocks.len(), 1);
        assert_eq!(result.removed_blocks.len(), 1);
    }

    #[test]
    fn total_new_blocks_count() {
        let diff = SnapshotDiff::new(SnapshotDiffConfig::default());
        let new = vec![make_block(1, 0, 4096, 1), make_block(2, 4096, 4096, 1)];
        let result = diff.compute(&[], &new);
        assert_eq!(result.total_new_blocks, 2);
    }

    #[test]
    fn total_base_blocks_count() {
        let diff = SnapshotDiff::new(SnapshotDiffConfig::default());
        let base = vec![make_block(1, 0, 4096, 1), make_block(2, 4096, 4096, 1)];
        let result = diff.compute(&base, &[]);
        assert_eq!(result.total_base_blocks, 2);
    }

    #[test]
    fn transfer_count_equals_added() {
        let diff = SnapshotDiff::new(SnapshotDiffConfig::default());
        let new = vec![make_block(1, 0, 4096, 1), make_block(2, 4096, 4096, 1)];
        let result = diff.compute(&[], &new);
        assert_eq!(result.transfer_count(), result.added_blocks.len());
    }

    #[test]
    fn is_identical_when_same() {
        let diff = SnapshotDiff::new(SnapshotDiffConfig::default());
        let block = make_block(1, 0, 4096, 1);
        let result = diff.compute(&[block.clone()], &[block]);
        assert!(result.is_identical());
    }

    #[test]
    fn is_identical_when_different() {
        let diff = SnapshotDiff::new(SnapshotDiffConfig::default());
        let result = diff.compute(&[make_block(1, 0, 4096, 1)], &[make_block(2, 0, 4096, 1)]);
        assert!(!result.is_identical());
    }

    #[test]
    fn change_ratio_zero_empty_new() {
        let diff = SnapshotDiff::new(SnapshotDiffConfig::default());
        let result = diff.compute(&[], &[]);
        assert_eq!(result.change_ratio(), 0.0);
    }

    #[test]
    fn change_ratio_all_new() {
        let diff = SnapshotDiff::new(SnapshotDiffConfig::default());
        let new = vec![make_block(1, 0, 4096, 1)];
        let result = diff.compute(&[], &new);
        assert_eq!(result.change_ratio(), 1.0);
    }

    #[test]
    fn change_ratio_no_change() {
        let diff = SnapshotDiff::new(SnapshotDiffConfig::default());
        let block = make_block(1, 0, 4096, 1);
        let result = diff.compute(&[block.clone()], &[block]);
        assert_eq!(result.change_ratio(), 0.0);
    }

    #[test]
    fn change_ratio_partial() {
        let diff = SnapshotDiff::new(SnapshotDiffConfig::default());
        let base = vec![make_block(1, 0, 4096, 1), make_block(2, 4096, 4096, 1)];
        let new = vec![make_block(1, 0, 4096, 1), make_block(3, 8192, 4096, 1)];
        let result = diff.compute(&base, &new);
        assert!(result.change_ratio() > 0.0 && result.change_ratio() < 1.0);
    }

    #[test]
    fn incremental_transfer_empty() {
        let diff = SnapshotDiff::new(SnapshotDiffConfig::default());
        let block = make_block(1, 0, 4096, 1);
        let result = diff.incremental_transfer(&[block.clone()], &[block]);
        assert!(result.is_empty());
    }

    #[test]
    fn incremental_transfer_all_new() {
        let diff = SnapshotDiff::new(SnapshotDiffConfig::default());
        let new = vec![make_block(1, 0, 4096, 1)];
        let result = diff.incremental_transfer(&[], &new);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn incremental_transfer_skips_existing() {
        let diff = SnapshotDiff::new(SnapshotDiffConfig::default());
        let base = vec![make_block(1, 0, 4096, 1)];
        let new = vec![make_block(1, 0, 4096, 1), make_block(2, 4096, 4096, 1)];
        let result = diff.incremental_transfer(&base, &new);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn batch_size_limits_added() {
        let diff = SnapshotDiff::new(SnapshotDiffConfig { batch_size: 2 });
        let new: Vec<_> = (0..5)
            .map(|i| make_block(i, i as u64 * 4096, 4096, 1))
            .collect();
        let result = diff.compute(&[], &new);
        assert_eq!(result.added_blocks.len(), 2);
    }

    #[test]
    fn batch_size_limits_removed() {
        let diff = SnapshotDiff::new(SnapshotDiffConfig { batch_size: 2 });
        let base: Vec<_> = (0..5)
            .map(|i| make_block(i, i as u64 * 4096, 4096, 1))
            .collect();
        let result = diff.compute(&base, &[]);
        assert_eq!(result.removed_blocks.len(), 2);
    }

    #[test]
    fn snapshot_block_equality() {
        let b1 = make_block(1, 100, 4096, 1);
        let b2 = make_block(1, 100, 4096, 1);
        assert_eq!(b1, b2);
    }

    #[test]
    fn large_snapshot_diff() {
        let diff = SnapshotDiff::new(SnapshotDiffConfig::default());
        let base: Vec<_> = (0..100)
            .map(|i| make_block(i, i as u64 * 4096, 4096, 1))
            .collect();
        let new: Vec<_> = (50..150)
            .map(|i| make_block(i, i as u64 * 4096, 4096, 1))
            .collect();
        let result = diff.compute(&base, &new);
        assert_eq!(result.added_blocks.len(), 50);
        assert_eq!(result.removed_blocks.len(), 50);
    }
}
