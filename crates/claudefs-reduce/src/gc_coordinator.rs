use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GcPhase {
    Scan,
    Mark,
    Sweep,
    Compact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcCoordinatorConfig {
    pub chunks_per_wave: usize,
    pub bytes_per_wave: u64,
    pub trigger_threshold_pct: u8,
    pub target_free_pct: u8,
}

impl Default for GcCoordinatorConfig {
    fn default() -> Self {
        Self {
            chunks_per_wave: 10_000,
            bytes_per_wave: 256 * 1024 * 1024,
            trigger_threshold_pct: 80,
            target_free_pct: 60,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GcWaveStats {
    pub wave_id: u64,
    pub phase: Option<GcPhase>,
    pub chunks_scanned: u64,
    pub chunks_reclaimed: u64,
    pub bytes_reclaimed: u64,
    pub duration_ms: u64,
}

impl GcWaveStats {
    pub fn had_reclaimable_work(&self) -> bool {
        self.chunks_reclaimed > 0
    }
}

#[derive(Debug, Clone)]
pub struct GcCandidate {
    pub hash: [u8; 32],
    pub ref_count: u32,
    pub size_bytes: u32,
    pub segment_id: u64,
}

pub struct GcCoordinator {
    config: GcCoordinatorConfig,
    current_wave: u64,
    current_phase: GcPhase,
    candidates: Vec<GcCandidate>,
    completed_waves: Vec<GcWaveStats>,
    total_bytes_reclaimed: u64,
}

impl GcCoordinator {
    pub fn new(config: GcCoordinatorConfig) -> Self {
        Self {
            config,
            current_wave: 0,
            current_phase: GcPhase::Scan,
            candidates: Vec::new(),
            completed_waves: Vec::new(),
            total_bytes_reclaimed: 0,
        }
    }

    pub fn add_candidate(&mut self, candidate: GcCandidate) {
        self.candidates.push(candidate);
    }

    pub fn current_phase(&self) -> &GcPhase {
        &self.current_phase
    }

    pub fn advance_phase(&mut self) {
        self.current_phase = match self.current_phase {
            GcPhase::Scan => GcPhase::Mark,
            GcPhase::Mark => GcPhase::Sweep,
            GcPhase::Sweep => GcPhase::Compact,
            GcPhase::Compact => {
                self.current_wave += 1;
                self.candidates.clear();
                GcPhase::Scan
            }
        };
    }

    pub fn execute_sweep(&mut self) -> GcWaveStats {
        let mut stats = GcWaveStats {
            wave_id: self.current_wave,
            phase: Some(GcPhase::Sweep),
            ..Default::default()
        };

        let before = self.candidates.len() as u64;
        let reclaimed_bytes: u64 = self
            .candidates
            .iter()
            .filter(|c| c.ref_count == 0)
            .map(|c| c.size_bytes as u64)
            .sum();

        self.candidates.retain(|c| c.ref_count > 0);
        let after = self.candidates.len() as u64;

        stats.chunks_scanned = before;
        stats.chunks_reclaimed = before - after;
        stats.bytes_reclaimed = reclaimed_bytes;
        self.total_bytes_reclaimed += reclaimed_bytes;

        stats
    }

    pub fn should_trigger(&self, pressure_pct: u8) -> bool {
        pressure_pct >= self.config.trigger_threshold_pct
    }

    pub fn wave_budget_exhausted(&self) -> bool {
        self.candidates.len() >= self.config.chunks_per_wave
    }

    pub fn record_wave(&mut self, stats: GcWaveStats) {
        self.completed_waves.push(stats);
    }

    pub fn completed_wave_count(&self) -> usize {
        self.completed_waves.len()
    }

    pub fn current_wave_id(&self) -> u64 {
        self.current_wave
    }

    pub fn total_bytes_reclaimed(&self) -> u64 {
        self.total_bytes_reclaimed
    }

    pub fn wave_history(&self) -> &[GcWaveStats] {
        &self.completed_waves
    }

    pub fn candidate_count(&self) -> usize {
        self.candidates.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_candidate(ref_count: u32, size: u32) -> GcCandidate {
        GcCandidate {
            hash: [0u8; 32],
            ref_count,
            size_bytes: size,
            segment_id: 1,
        }
    }

    #[test]
    fn gc_coordinator_config_default() {
        let config = GcCoordinatorConfig::default();
        assert_eq!(config.chunks_per_wave, 10_000);
        assert_eq!(config.bytes_per_wave, 256 * 1024 * 1024);
        assert_eq!(config.trigger_threshold_pct, 80);
        assert_eq!(config.target_free_pct, 60);
    }

    #[test]
    fn gc_phase_scan_is_initial() {
        let coord = GcCoordinator::new(GcCoordinatorConfig::default());
        assert_eq!(*coord.current_phase(), GcPhase::Scan);
    }

    #[test]
    fn advance_phase_scan_to_mark() {
        let mut coord = GcCoordinator::new(GcCoordinatorConfig::default());
        coord.advance_phase();
        assert_eq!(*coord.current_phase(), GcPhase::Mark);
    }

    #[test]
    fn advance_phase_mark_to_sweep() {
        let mut coord = GcCoordinator::new(GcCoordinatorConfig::default());
        coord.advance_phase();
        coord.advance_phase();
        assert_eq!(*coord.current_phase(), GcPhase::Sweep);
    }

    #[test]
    fn advance_phase_sweep_to_compact() {
        let mut coord = GcCoordinator::new(GcCoordinatorConfig::default());
        for _ in 0..3 {
            coord.advance_phase();
        }
        assert_eq!(*coord.current_phase(), GcPhase::Compact);
    }

    #[test]
    fn advance_phase_compact_to_scan() {
        let mut coord = GcCoordinator::new(GcCoordinatorConfig::default());
        for _ in 0..4 {
            coord.advance_phase();
        }
        assert_eq!(*coord.current_phase(), GcPhase::Scan);
    }

    #[test]
    fn advance_compact_increments_wave() {
        let mut coord = GcCoordinator::new(GcCoordinatorConfig::default());
        assert_eq!(coord.current_wave_id(), 0);
        for _ in 0..4 {
            coord.advance_phase();
        }
        assert_eq!(coord.current_wave_id(), 1);
    }

    #[test]
    fn advance_compact_clears_candidates() {
        let mut coord = GcCoordinator::new(GcCoordinatorConfig::default());
        coord.add_candidate(make_candidate(0, 100));
        coord.add_candidate(make_candidate(1, 200));
        for _ in 0..4 {
            coord.advance_phase();
        }
        assert_eq!(coord.candidate_count(), 0);
    }

    #[test]
    fn add_candidate_increments_count() {
        let mut coord = GcCoordinator::new(GcCoordinatorConfig::default());
        assert_eq!(coord.candidate_count(), 0);
        coord.add_candidate(make_candidate(0, 100));
        assert_eq!(coord.candidate_count(), 1);
    }

    #[test]
    fn execute_sweep_removes_zero_ref() {
        let mut coord = GcCoordinator::new(GcCoordinatorConfig::default());
        coord.add_candidate(make_candidate(0, 100));
        coord.add_candidate(make_candidate(1, 200));
        let stats = coord.execute_sweep();
        assert_eq!(stats.chunks_reclaimed, 1);
        assert_eq!(coord.candidate_count(), 1);
    }

    #[test]
    fn execute_sweep_keeps_live_chunks() {
        let mut coord = GcCoordinator::new(GcCoordinatorConfig::default());
        coord.add_candidate(make_candidate(1, 200));
        coord.execute_sweep();
        assert_eq!(coord.candidate_count(), 1);
    }

    #[test]
    fn execute_sweep_bytes_reclaimed() {
        let mut coord = GcCoordinator::new(GcCoordinatorConfig::default());
        coord.add_candidate(make_candidate(0, 100));
        coord.add_candidate(make_candidate(0, 200));
        let stats = coord.execute_sweep();
        assert_eq!(stats.bytes_reclaimed, 300);
    }

    #[test]
    fn execute_sweep_stats_wave_id() {
        let mut coord = GcCoordinator::new(GcCoordinatorConfig::default());
        let stats = coord.execute_sweep();
        assert_eq!(stats.wave_id, 0);
    }

    #[test]
    fn should_trigger_at_threshold() {
        let coord = GcCoordinator::new(GcCoordinatorConfig::default());
        assert!(coord.should_trigger(80));
    }

    #[test]
    fn should_trigger_below_threshold() {
        let coord = GcCoordinator::new(GcCoordinatorConfig::default());
        assert!(!coord.should_trigger(79));
    }

    #[test]
    fn should_trigger_above_threshold() {
        let coord = GcCoordinator::new(GcCoordinatorConfig::default());
        assert!(coord.should_trigger(90));
    }

    #[test]
    fn wave_budget_exhausted_false() {
        let coord = GcCoordinator::new(GcCoordinatorConfig::default());
        assert!(!coord.wave_budget_exhausted());
    }

    #[test]
    fn wave_budget_exhausted_true() {
        let mut coord = GcCoordinator::new(GcCoordinatorConfig {
            chunks_per_wave: 2,
            ..Default::default()
        });
        coord.add_candidate(make_candidate(1, 100));
        coord.add_candidate(make_candidate(1, 100));
        assert!(coord.wave_budget_exhausted());
    }

    #[test]
    fn record_wave_stores_stats() {
        let mut coord = GcCoordinator::new(GcCoordinatorConfig::default());
        let stats = GcWaveStats::default();
        coord.record_wave(stats);
        assert_eq!(coord.completed_wave_count(), 1);
    }

    #[test]
    fn total_bytes_reclaimed_accumulates() {
        let mut coord = GcCoordinator::new(GcCoordinatorConfig::default());
        coord.add_candidate(make_candidate(0, 100));
        coord.execute_sweep();
        coord.add_candidate(make_candidate(0, 200));
        coord.execute_sweep();
        assert_eq!(coord.total_bytes_reclaimed(), 300);
    }

    #[test]
    fn wave_history_returns_all() {
        let mut coord = GcCoordinator::new(GcCoordinatorConfig::default());
        coord.record_wave(GcWaveStats {
            wave_id: 0,
            ..Default::default()
        });
        coord.record_wave(GcWaveStats {
            wave_id: 1,
            ..Default::default()
        });
        let history = coord.wave_history();
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn gc_wave_stats_had_reclaimable_work() {
        let mut stats = GcWaveStats::default();
        assert!(!stats.had_reclaimable_work());
        stats.chunks_reclaimed = 1;
        assert!(stats.had_reclaimable_work());
    }
}
