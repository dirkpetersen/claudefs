use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct RefEntry {
    pub hash: [u8; 32],
    pub ref_count: u32,
    pub size_bytes: u32,
}

impl RefEntry {
    pub fn is_orphaned(&self) -> bool {
        self.ref_count == 0
    }
}

#[derive(Debug, Clone)]
pub struct RefcountTableConfig {
    pub max_ref_count: u32,
}

impl Default for RefcountTableConfig {
    fn default() -> Self {
        Self {
            max_ref_count: 65535,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RefcountTableStats {
    pub total_blocks: u64,
    pub total_references: u64,
    pub orphaned_blocks: u64,
    pub max_ref_count_seen: u32,
}

pub struct RefcountTable {
    config: RefcountTableConfig,
    entries: HashMap<[u8; 32], RefEntry>,
    stats: RefcountTableStats,
}

impl RefcountTable {
    pub fn new(config: RefcountTableConfig) -> Self {
        Self {
            config,
            entries: HashMap::new(),
            stats: RefcountTableStats::default(),
        }
    }

    pub fn add_ref(&mut self, hash: [u8; 32], size_bytes: u32) {
        let entry = self.entries.entry(hash).or_insert_with(|| {
            self.stats.total_blocks += 1;
            RefEntry {
                hash,
                ref_count: 0,
                size_bytes,
            }
        });
        entry.ref_count = entry.ref_count.saturating_add(1);
        if entry.ref_count > self.config.max_ref_count {
            entry.ref_count = self.config.max_ref_count;
        }
        if entry.ref_count > self.stats.max_ref_count_seen {
            self.stats.max_ref_count_seen = entry.ref_count;
        }
        self.stats.total_references += 1;
    }

    pub fn dec_ref(&mut self, hash: &[u8; 32]) -> Option<u32> {
        if let Some(entry) = self.entries.get_mut(hash) {
            if entry.ref_count > 0 {
                entry.ref_count -= 1;
                if self.stats.total_references > 0 {
                    self.stats.total_references -= 1;
                }
                if entry.ref_count == 0 {
                    self.stats.orphaned_blocks += 1;
                }
                return Some(entry.ref_count);
            }
        }
        None
    }

    pub fn remove(&mut self, hash: &[u8; 32]) -> bool {
        if let Some(entry) = self.entries.remove(hash) {
            if self.stats.total_blocks > 0 {
                self.stats.total_blocks -= 1;
            }
            if entry.ref_count == 0 && self.stats.orphaned_blocks > 0 {
                self.stats.orphaned_blocks -= 1;
            }
            true
        } else {
            false
        }
    }

    pub fn get_ref_count(&self, hash: &[u8; 32]) -> Option<u32> {
        self.entries.get(hash).map(|e| e.ref_count)
    }

    pub fn orphaned(&self) -> Vec<&RefEntry> {
        self.entries.values().filter(|e| e.ref_count == 0).collect()
    }

    pub fn block_count(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn stats(&self) -> &RefcountTableStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refcount_table_config_default() {
        let config = RefcountTableConfig::default();
        assert_eq!(config.max_ref_count, 65535);
    }

    #[test]
    fn new_table_empty() {
        let config = RefcountTableConfig::default();
        let table = RefcountTable::new(config);
        assert!(table.is_empty());
        assert_eq!(table.block_count(), 0);
    }

    #[test]
    fn add_ref_creates_entry() {
        let mut config = RefcountTableConfig::default();
        let mut table = RefcountTable::new(config.clone());
        table.add_ref([0x11; 32], 4096);
        assert_eq!(table.block_count(), 1);
    }

    #[test]
    fn add_ref_ref_count_one() {
        let mut config = RefcountTableConfig::default();
        let mut table = RefcountTable::new(config.clone());
        table.add_ref([0x12; 32], 4096);
        assert_eq!(table.get_ref_count(&[0x12; 32]), Some(1));
    }

    #[test]
    fn add_ref_twice_increments() {
        let mut config = RefcountTableConfig::default();
        let mut table = RefcountTable::new(config.clone());
        let hash = [0x13; 32];
        table.add_ref(hash, 4096);
        table.add_ref(hash, 4096);
        assert_eq!(table.get_ref_count(&hash), Some(2));
    }

    #[test]
    fn add_ref_new_block_increments_total() {
        let mut config = RefcountTableConfig::default();
        let mut table = RefcountTable::new(config.clone());
        table.add_ref([0x14; 32], 4096);
        assert_eq!(table.stats().total_blocks, 1);
    }

    #[test]
    fn add_ref_increments_total_references() {
        let mut config = RefcountTableConfig::default();
        let mut table = RefcountTable::new(config.clone());
        table.add_ref([0x15; 32], 4096);
        assert_eq!(table.stats().total_references, 1);
    }

    #[test]
    fn dec_ref_decrements_count() {
        let mut config = RefcountTableConfig::default();
        let mut table = RefcountTable::new(config.clone());
        let hash = [0x16; 32];
        table.add_ref(hash, 4096);
        table.dec_ref(&hash);
        assert_eq!(table.get_ref_count(&hash), Some(0));
    }

    #[test]
    fn dec_ref_returns_new_count() {
        let mut config = RefcountTableConfig::default();
        let mut table = RefcountTable::new(config.clone());
        let hash = [0x17; 32];
        table.add_ref(hash, 4096);
        let result = table.dec_ref(&hash);
        assert_eq!(result, Some(0));
    }

    #[test]
    fn dec_ref_nonexistent_returns_none() {
        let mut config = RefcountTableConfig::default();
        let mut table = RefcountTable::new(config.clone());
        let result = table.dec_ref(&[0x18; 32]);
        assert_eq!(result, None);
    }

    #[test]
    fn dec_ref_to_zero_marks_orphaned() {
        let mut config = RefcountTableConfig::default();
        let mut table = RefcountTable::new(config.clone());
        let hash = [0x19; 32];
        table.add_ref(hash, 4096);
        table.dec_ref(&hash);
        assert_eq!(table.stats().orphaned_blocks, 1);
    }

    #[test]
    fn orphaned_returns_zero_ref_entries() {
        let mut config = RefcountTableConfig::default();
        let mut table = RefcountTable::new(config.clone());
        let hash = [0x1A; 32];
        table.add_ref(hash, 4096);
        table.dec_ref(&hash);
        let orphans = table.orphaned();
        assert!(!orphans.is_empty());
    }

    #[test]
    fn orphaned_empty_when_none() {
        let mut config = RefcountTableConfig::default();
        let mut table = RefcountTable::new(config.clone());
        table.add_ref([0x1B; 32], 4096);
        let orphans = table.orphaned();
        assert!(orphans.is_empty());
    }

    #[test]
    fn remove_existing_block() {
        let mut config = RefcountTableConfig::default();
        let mut table = RefcountTable::new(config.clone());
        let hash = [0x1C; 32];
        table.add_ref(hash, 4096);
        assert!(table.remove(&hash));
    }

    #[test]
    fn remove_nonexistent_block() {
        let mut config = RefcountTableConfig::default();
        let mut table = RefcountTable::new(config.clone());
        assert!(!table.remove(&[0x1D; 32]));
    }

    #[test]
    fn remove_decrements_total_blocks() {
        let mut config = RefcountTableConfig::default();
        let mut table = RefcountTable::new(config.clone());
        let hash = [0x1E; 32];
        table.add_ref(hash, 4096);
        table.remove(&hash);
        assert_eq!(table.stats().total_blocks, 0);
    }

    #[test]
    fn remove_orphaned_decrements_orphaned_count() {
        let mut config = RefcountTableConfig::default();
        let mut table = RefcountTable::new(config.clone());
        let hash = [0x1F; 32];
        table.add_ref(hash, 4096);
        table.dec_ref(&hash);
        table.remove(&hash);
        assert_eq!(table.stats().orphaned_blocks, 0);
    }

    #[test]
    fn stats_total_blocks_accumulates() {
        let mut config = RefcountTableConfig::default();
        let mut table = RefcountTable::new(config.clone());
        table.add_ref([0x20; 32], 4096);
        table.add_ref([0x21; 32], 4096);
        table.add_ref([0x22; 32], 4096);
        assert_eq!(table.stats().total_blocks, 3);
    }

    #[test]
    fn stats_max_ref_count_seen() {
        let mut config = RefcountTableConfig::default();
        let mut table = RefcountTable::new(config.clone());
        let hash = [0x23; 32];
        table.add_ref(hash, 4096);
        table.add_ref(hash, 4096);
        assert_eq!(table.stats().max_ref_count_seen, 2);
    }

    #[test]
    fn max_ref_count_clamps() {
        let mut config = RefcountTableConfig { max_ref_count: 5 };
        let mut table = RefcountTable::new(config.clone());
        let hash = [0x24; 32];
        for _ in 0..10 {
            table.add_ref(hash, 4096);
        }
        assert_eq!(table.get_ref_count(&hash), Some(5));
    }

    #[test]
    fn add_ref_existing_block_no_new_total() {
        let mut config = RefcountTableConfig::default();
        let mut table = RefcountTable::new(config.clone());
        let hash = [0x25; 32];
        table.add_ref(hash, 4096);
        table.add_ref(hash, 4096);
        assert_eq!(table.stats().total_blocks, 1);
    }

    #[test]
    fn get_ref_count_none_for_unknown() {
        let config = RefcountTableConfig::default();
        let table = RefcountTable::new(config);
        assert_eq!(table.get_ref_count(&[0x26; 32]), None);
    }

    #[test]
    fn lifecycle_add_add_dec_dec() {
        let mut config = RefcountTableConfig::default();
        let mut table = RefcountTable::new(config.clone());
        let hash = [0x27; 32];
        table.add_ref(hash, 4096);
        table.add_ref(hash, 4096);
        table.dec_ref(&hash);
        table.dec_ref(&hash);
        assert_eq!(table.get_ref_count(&hash), Some(0));
        assert_eq!(table.stats().orphaned_blocks, 1);
    }

    #[test]
    fn remove_after_dec_to_zero() {
        let mut config = RefcountTableConfig::default();
        let mut table = RefcountTable::new(config.clone());
        let hash = [0x28; 32];
        table.add_ref(hash, 4096);
        table.dec_ref(&hash);
        table.remove(&hash);
        assert!(table.is_empty());
    }
}
