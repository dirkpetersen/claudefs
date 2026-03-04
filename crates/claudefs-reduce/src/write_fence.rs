#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FenceState {
    Open,
    Sealed,
    Released,
}

#[derive(Debug, Clone)]
pub struct WriteFenceConfig {
    pub max_pending_writes: usize,
    pub fence_id: u64,
}

impl Default for WriteFenceConfig {
    fn default() -> Self {
        Self {
            max_pending_writes: 1000,
            fence_id: 0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct WriteFenceStats {
    pub total_writes_submitted: u64,
    pub total_writes_completed: u64,
    pub total_seals: u64,
    pub total_releases: u64,
}

impl WriteFenceStats {
    pub fn pending_writes(&self) -> u64 {
        self.total_writes_submitted
            .saturating_sub(self.total_writes_completed)
    }
}

pub struct WriteFence {
    config: WriteFenceConfig,
    state: FenceState,
    stats: WriteFenceStats,
}

impl WriteFence {
    pub fn new(config: WriteFenceConfig) -> Self {
        Self {
            config,
            state: FenceState::Open,
            stats: WriteFenceStats::default(),
        }
    }

    pub fn submit_write(&mut self) -> Result<u64, &'static str> {
        match self.state {
            FenceState::Open => {
                let id = self.stats.total_writes_submitted;
                self.stats.total_writes_submitted += 1;
                if self.stats.total_writes_submitted >= self.config.max_pending_writes as u64 {
                    self.state = FenceState::Sealed;
                    self.stats.total_seals += 1;
                }
                Ok(id)
            }
            FenceState::Sealed => Err("fence is sealed"),
            FenceState::Released => Err("fence is released"),
        }
    }

    pub fn complete_write(&mut self) {
        self.stats.total_writes_completed += 1;
        if self.state == FenceState::Sealed && self.stats.pending_writes() == 0 {
            self.state = FenceState::Released;
            self.stats.total_releases += 1;
        }
    }

    pub fn seal(&mut self) {
        if self.state == FenceState::Open {
            self.state = FenceState::Sealed;
            self.stats.total_seals += 1;
            if self.stats.pending_writes() == 0 {
                self.state = FenceState::Released;
                self.stats.total_releases += 1;
            }
        }
    }

    pub fn state(&self) -> &FenceState {
        &self.state
    }

    pub fn is_released(&self) -> bool {
        self.state == FenceState::Released
    }

    pub fn stats(&self) -> &WriteFenceStats {
        &self.stats
    }

    pub fn has_pending_writes(&self) -> bool {
        self.stats.pending_writes() > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_fence_config_default() {
        let config = WriteFenceConfig::default();
        assert_eq!(config.max_pending_writes, 1000);
        assert_eq!(config.fence_id, 0);
    }

    #[test]
    fn new_fence_is_open() {
        let fence = WriteFence::new(WriteFenceConfig::default());
        assert_eq!(*fence.state(), FenceState::Open);
    }

    #[test]
    fn new_fence_no_pending() {
        let fence = WriteFence::new(WriteFenceConfig::default());
        assert!(!fence.has_pending_writes());
    }

    #[test]
    fn submit_write_returns_id() {
        let mut fence = WriteFence::new(WriteFenceConfig::default());
        let result = fence.submit_write();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn submit_write_increments_counter() {
        let mut fence = WriteFence::new(WriteFenceConfig::default());
        fence.submit_write();
        assert_eq!(fence.stats().total_writes_submitted, 1);
    }

    #[test]
    fn submit_write_increments_pending() {
        let mut fence = WriteFence::new(WriteFenceConfig::default());
        fence.submit_write();
        assert_eq!(fence.stats().pending_writes(), 1);
    }

    #[test]
    fn complete_write_decrements_pending() {
        let mut fence = WriteFence::new(WriteFenceConfig::default());
        fence.submit_write();
        fence.complete_write();
        assert_eq!(fence.stats().pending_writes(), 0);
    }

    #[test]
    fn stats_pending_saturates_at_zero() {
        let mut fence = WriteFence::new(WriteFenceConfig::default());
        fence.complete_write();
        assert_eq!(fence.stats().pending_writes(), 0);
    }

    #[test]
    fn submit_rejected_when_sealed() {
        let mut fence = WriteFence::new(WriteFenceConfig::default());
        fence.seal();
        let result = fence.submit_write();
        assert!(result.is_err());
    }

    #[test]
    fn submit_rejected_when_released() {
        let mut fence = WriteFence::new(WriteFenceConfig::default());
        fence.seal();
        fence.complete_write();
        let result = fence.submit_write();
        assert!(result.is_err());
    }

    #[test]
    fn seal_transitions_to_sealed() {
        let mut fence = WriteFence::new(WriteFenceConfig::default());
        fence.submit_write().unwrap();
        fence.seal();
        assert_eq!(*fence.state(), FenceState::Sealed);
    }

    #[test]
    fn seal_no_pending_releases_immediately() {
        let mut fence = WriteFence::new(WriteFenceConfig::default());
        fence.seal();
        assert_eq!(*fence.state(), FenceState::Released);
    }

    #[test]
    fn seal_with_pending_stays_sealed() {
        let mut fence = WriteFence::new(WriteFenceConfig::default());
        fence.submit_write();
        fence.seal();
        assert_eq!(*fence.state(), FenceState::Sealed);
    }

    #[test]
    fn complete_after_seal_releases() {
        let mut fence = WriteFence::new(WriteFenceConfig::default());
        fence.submit_write();
        fence.seal();
        fence.complete_write();
        assert_eq!(*fence.state(), FenceState::Released);
    }

    #[test]
    fn is_released_false_when_open() {
        let fence = WriteFence::new(WriteFenceConfig::default());
        assert!(!fence.is_released());
    }

    #[test]
    fn is_released_true_when_released() {
        let mut fence = WriteFence::new(WriteFenceConfig::default());
        fence.seal();
        assert!(fence.is_released());
    }

    #[test]
    fn auto_seal_at_max_pending() {
        let mut fence = WriteFence::new(WriteFenceConfig {
            max_pending_writes: 3,
            ..Default::default()
        });
        fence.submit_write().unwrap();
        fence.submit_write().unwrap();
        assert_eq!(*fence.state(), FenceState::Open);
        fence.submit_write().unwrap();
        assert_eq!(*fence.state(), FenceState::Sealed);
    }

    #[test]
    fn total_seals_increments() {
        let mut fence = WriteFence::new(WriteFenceConfig::default());
        fence.seal();
        assert_eq!(fence.stats().total_seals, 1);
    }

    #[test]
    fn total_releases_increments() {
        let mut fence = WriteFence::new(WriteFenceConfig::default());
        fence.seal();
        assert_eq!(fence.stats().total_releases, 1);
    }

    #[test]
    fn multiple_submits_and_completes() {
        let mut fence = WriteFence::new(WriteFenceConfig::default());
        for _ in 0..5 {
            fence.submit_write().unwrap();
        }
        for _ in 0..5 {
            fence.complete_write();
        }
        assert_eq!(fence.stats().pending_writes(), 0);
    }

    #[test]
    fn seal_idempotent() {
        let mut fence = WriteFence::new(WriteFenceConfig::default());
        fence.seal();
        fence.seal();
        assert_eq!(fence.stats().total_seals, 1);
    }

    #[test]
    fn write_fence_lifecycle() {
        let mut fence = WriteFence::new(WriteFenceConfig::default());
        assert_eq!(*fence.state(), FenceState::Open);

        fence.submit_write().unwrap();
        assert!(fence.has_pending_writes());

        fence.seal();
        assert_eq!(*fence.state(), FenceState::Sealed);

        fence.complete_write();
        assert_eq!(*fence.state(), FenceState::Released);
        assert!(!fence.has_pending_writes());
    }
}
