use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PathId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathState {
    Active,
    Degraded,
    Failed,
    Reconnecting,
}

impl PathState {
    pub fn is_usable(&self) -> bool {
        matches!(self, PathState::Active | PathState::Degraded)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathPriority(pub u8);

impl PathPriority {
    pub const DEFAULT: u8 = 100;
}

#[derive(Debug, Clone)]
pub struct PathMetrics {
    pub latency_us: u64,
    pub error_count: u64,
    pub bytes_sent: u64,
    pub bytes_recv: u64,
    pub last_error_at_secs: u64,
}

impl PathMetrics {
    pub fn new() -> Self {
        Self {
            latency_us: 1000,
            error_count: 0,
            bytes_sent: 0,
            bytes_recv: 0,
            last_error_at_secs: 0,
        }
    }

    pub fn record_success(&mut self, latency_us: u64) {
        self.latency_us = (7 * self.latency_us + latency_us) / 8;
    }

    pub fn record_error(&mut self, now_secs: u64) {
        self.error_count += 1;
        self.last_error_at_secs = now_secs;
    }

    pub fn error_rate_recent(&self, now_secs: u64, window_secs: u64) -> f64 {
        if self.last_error_at_secs >= now_secs.saturating_sub(window_secs) && self.error_count > 0 {
            1.0
        } else {
            0.0
        }
    }

    pub fn score(&self) -> u64 {
        self.latency_us + self.error_count * 1000
    }
}

impl Default for PathMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct PathInfo {
    pub id: PathId,
    pub state: PathState,
    pub priority: u8,
    pub remote_addr: String,
    pub metrics: PathMetrics,
}

impl PathInfo {
    pub fn new(id: PathId, remote_addr: String, priority: u8) -> Self {
        Self {
            id,
            state: PathState::Active,
            priority,
            remote_addr,
            metrics: PathMetrics::new(),
        }
    }

    pub fn mark_degraded(&mut self) {
        self.state = PathState::Degraded;
    }

    pub fn mark_failed(&mut self) {
        self.state = PathState::Failed;
    }

    pub fn mark_reconnecting(&mut self) {
        self.state = PathState::Reconnecting;
    }

    pub fn mark_active(&mut self) {
        self.state = PathState::Active;
    }

    pub fn is_usable(&self) -> bool {
        self.state.is_usable()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadBalancePolicy {
    RoundRobin,
    LeastLatency,
    Primary,
}

pub struct MultipathRouter {
    policy: LoadBalancePolicy,
    paths: Vec<PathInfo>,
    round_robin_index: usize,
}

impl MultipathRouter {
    pub fn new(policy: LoadBalancePolicy) -> Self {
        Self {
            policy,
            paths: Vec::new(),
            round_robin_index: 0,
        }
    }

    pub fn add_path(&mut self, info: PathInfo) -> Result<()> {
        if self.paths.len() >= 16 {
            return Err(crate::error::FuseError::InvalidArgument {
                msg: "max paths (16) exceeded".to_string(),
            }
            .into());
        }

        if self.paths.iter().any(|p| p.id == info.id) {
            return Err(crate::error::FuseError::AlreadyExists {
                name: format!("path {:?} already exists", info.id),
            }
            .into());
        }

        self.paths.push(info);
        Ok(())
    }

    pub fn remove_path(&mut self, id: PathId) -> Result<()> {
        let pos = self
            .paths
            .iter()
            .position(|p| p.id == id)
            .ok_or_else(|| crate::error::FuseError::NotFound { ino: id.0 })?;

        self.paths.remove(pos);
        if self.round_robin_index >= self.paths.len() {
            self.round_robin_index = 0;
        }
        Ok(())
    }

    fn usable_paths(&self) -> Vec<&PathInfo> {
        self.paths.iter().filter(|p| p.is_usable()).collect()
    }

    pub fn select_path(&mut self) -> Option<PathId> {
        let usable = self.usable_paths();
        if usable.is_empty() {
            return None;
        }

        match self.policy {
            LoadBalancePolicy::RoundRobin => {
                let count = usable.len();
                let idx = self.round_robin_index % count;
                let selected_id = usable[idx].id;
                self.round_robin_index = (self.round_robin_index + 1) % count;
                Some(selected_id)
            }
            LoadBalancePolicy::LeastLatency => usable
                .iter()
                .min_by_key(|p| p.metrics.score())
                .map(|p| p.id),
            LoadBalancePolicy::Primary => {
                let mut sorted: Vec<_> = usable.into_iter().collect();
                sorted.sort_by(|a, b| {
                    b.priority
                        .cmp(&a.priority)
                        .then(a.metrics.score().cmp(&b.metrics.score()))
                });
                Some(sorted[0].id)
            }
        }
    }

    pub fn record_success(&mut self, id: PathId, latency_us: u64) -> Result<()> {
        let path = self
            .paths
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| crate::error::FuseError::NotFound { ino: id.0 })?;

        path.metrics.record_success(latency_us);
        Ok(())
    }

    pub fn record_error(&mut self, id: PathId, now_secs: u64) -> Result<()> {
        let path = self
            .paths
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| crate::error::FuseError::NotFound { ino: id.0 })?;

        path.metrics.record_error(now_secs);

        if path.metrics.error_count >= 10 {
            path.state = PathState::Failed;
        } else if path.metrics.error_count >= 3 {
            path.state = PathState::Degraded;
        }

        Ok(())
    }

    pub fn mark_reconnecting(&mut self, id: PathId) -> Result<()> {
        let path = self
            .paths
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| crate::error::FuseError::NotFound { ino: id.0 })?;

        path.state = PathState::Reconnecting;
        Ok(())
    }

    pub fn mark_active(&mut self, id: PathId) -> Result<()> {
        let path = self
            .paths
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| crate::error::FuseError::NotFound { ino: id.0 })?;

        path.state = PathState::Active;
        path.metrics.error_count = 0;
        Ok(())
    }

    pub fn path_count(&self) -> usize {
        self.paths.len()
    }

    pub fn usable_path_count(&self) -> usize {
        self.usable_paths().len()
    }

    pub fn all_paths_failed(&self) -> bool {
        !self.paths.is_empty() && self.paths.iter().all(|p| p.state == PathState::Failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_path() {
        let mut router = MultipathRouter::new(LoadBalancePolicy::RoundRobin);
        let info = PathInfo::new(PathId(1), "192.168.1.1:8000".to_string(), 100);
        assert!(router.add_path(info).is_ok());
        assert_eq!(router.path_count(), 1);
    }

    #[test]
    fn test_remove_path() {
        let mut router = MultipathRouter::new(LoadBalancePolicy::RoundRobin);
        let info = PathInfo::new(PathId(1), "192.168.1.1:8000".to_string(), 100);
        router.add_path(info).unwrap();

        router.remove_path(PathId(1)).unwrap();
        assert_eq!(router.path_count(), 0);
    }

    #[test]
    fn test_duplicate_path_id() {
        let mut router = MultipathRouter::new(LoadBalancePolicy::RoundRobin);
        let info = PathInfo::new(PathId(1), "192.168.1.1:8000".to_string(), 100);
        router.add_path(info.clone()).unwrap();

        let result = router.add_path(info);
        assert!(result.is_err());
    }

    #[test]
    fn test_select_path_roundrobin_cycles() {
        let mut router = MultipathRouter::new(LoadBalancePolicy::RoundRobin);
        router
            .add_path(PathInfo::new(PathId(1), "addr1".to_string(), 100))
            .unwrap();
        router
            .add_path(PathInfo::new(PathId(2), "addr2".to_string(), 100))
            .unwrap();
        router
            .add_path(PathInfo::new(PathId(3), "addr3".to_string(), 100))
            .unwrap();

        let sel1 = router.select_path();
        let sel2 = router.select_path();
        let sel3 = router.select_path();
        let sel4 = router.select_path();

        assert_ne!(sel1, sel2);
        assert_ne!(sel2, sel3);
        assert_eq!(sel1, sel4);
    }

    #[test]
    fn test_select_path_leastlatency_picks_lowest_score() {
        let mut router = MultipathRouter::new(LoadBalancePolicy::LeastLatency);

        let mut p1 = PathInfo::new(PathId(1), "addr1".to_string(), 100);
        p1.metrics.latency_us = 100;

        let mut p2 = PathInfo::new(PathId(2), "addr2".to_string(), 100);
        p2.metrics.latency_us = 50;

        router.add_path(p1).unwrap();
        router.add_path(p2).unwrap();

        let selected = router.select_path();
        assert_eq!(selected, Some(PathId(2)));
    }

    #[test]
    fn test_select_path_primary_picks_highest_priority() {
        let mut router = MultipathRouter::new(LoadBalancePolicy::Primary);

        let mut p1 = PathInfo::new(PathId(1), "addr1".to_string(), 50);
        p1.metrics.latency_us = 10;

        let mut p2 = PathInfo::new(PathId(2), "addr2".to_string(), 100);
        p2.metrics.latency_us = 100;

        router.add_path(p1).unwrap();
        router.add_path(p2).unwrap();

        let selected = router.select_path();
        assert_eq!(selected, Some(PathId(2)));
    }

    #[test]
    fn test_primary_falls_back_when_primary_fails() {
        let mut router = MultipathRouter::new(LoadBalancePolicy::Primary);

        let mut p1 = PathInfo::new(PathId(1), "addr1".to_string(), 100);
        p1.state = PathState::Failed;

        let mut p2 = PathInfo::new(PathId(2), "addr2".to_string(), 50);

        router.add_path(p1).unwrap();
        router.add_path(p2).unwrap();

        let selected = router.select_path();
        assert_eq!(selected, Some(PathId(2)));
    }

    #[test]
    fn test_record_error_increments() {
        let mut router = MultipathRouter::new(LoadBalancePolicy::RoundRobin);
        router
            .add_path(PathInfo::new(PathId(1), "addr1".to_string(), 100))
            .unwrap();

        router.record_error(PathId(1), 1000).unwrap();
        router.record_error(PathId(1), 1001).unwrap();

        let path = &router.paths[0];
        assert_eq!(path.metrics.error_count, 2);
    }

    #[test]
    fn test_degraded_after_3_errors() {
        let mut router = MultipathRouter::new(LoadBalancePolicy::RoundRobin);
        router
            .add_path(PathInfo::new(PathId(1), "addr1".to_string(), 100))
            .unwrap();

        for i in 0..3 {
            router.record_error(PathId(1), 1000 + i).unwrap();
        }

        assert_eq!(router.paths[0].state, PathState::Degraded);
    }

    #[test]
    fn test_failed_after_10_errors() {
        let mut router = MultipathRouter::new(LoadBalancePolicy::RoundRobin);
        router
            .add_path(PathInfo::new(PathId(1), "addr1".to_string(), 100))
            .unwrap();

        for i in 0..10 {
            router.record_error(PathId(1), 1000 + i).unwrap();
        }

        assert_eq!(router.paths[0].state, PathState::Failed);
    }

    #[test]
    fn test_usable_path_count_excludes_failed() {
        let mut router = MultipathRouter::new(LoadBalancePolicy::RoundRobin);

        router
            .add_path(PathInfo::new(PathId(1), "addr1".to_string(), 100))
            .unwrap();

        let mut p2 = PathInfo::new(PathId(2), "addr2".to_string(), 100);
        p2.state = PathState::Failed;
        router.add_path(p2).unwrap();

        assert_eq!(router.usable_path_count(), 1);
    }

    #[test]
    fn test_all_paths_failed() {
        let mut router = MultipathRouter::new(LoadBalancePolicy::RoundRobin);

        let mut p1 = PathInfo::new(PathId(1), "addr1".to_string(), 100);
        p1.state = PathState::Failed;

        let mut p2 = PathInfo::new(PathId(2), "addr2".to_string(), 100);
        p2.state = PathState::Failed;

        router.add_path(p1).unwrap();
        router.add_path(p2).unwrap();

        assert!(router.all_paths_failed());
    }

    #[test]
    fn test_max_16_paths_limit() {
        let mut router = MultipathRouter::new(LoadBalancePolicy::RoundRobin);

        for i in 0..16 {
            let result = router.add_path(PathInfo::new(PathId(i), format!("addr{}", i), 100));
            assert!(result.is_ok(), "path {} should succeed", i);
        }

        let result = router.add_path(PathInfo::new(PathId(100), "addr100".to_string(), 100));
        assert!(result.is_err());
    }

    #[test]
    fn test_select_path_returns_none_when_no_usable_paths() {
        let mut router = MultipathRouter::new(LoadBalancePolicy::RoundRobin);

        let mut p1 = PathInfo::new(PathId(1), "addr1".to_string(), 100);
        p1.state = PathState::Failed;

        router.add_path(p1).unwrap();

        let selected = router.select_path();
        assert_eq!(selected, None);
    }

    #[test]
    fn test_record_success_updates_latency() {
        let mut router = MultipathRouter::new(LoadBalancePolicy::RoundRobin);
        router
            .add_path(PathInfo::new(PathId(1), "addr1".to_string(), 100))
            .unwrap();

        router.record_success(PathId(1), 500).unwrap();

        let ema = router.paths[0].metrics.latency_us;
        assert_eq!(ema, (7 * 1000 + 500) / 8);
    }

    #[test]
    fn test_path_state_is_usable() {
        assert!(PathState::Active.is_usable());
        assert!(PathState::Degraded.is_usable());
        assert!(!PathState::Failed.is_usable());
        assert!(!PathState::Reconnecting.is_usable());
    }
}
