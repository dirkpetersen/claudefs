use crate::{FuseError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, trace};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum IoPattern {
    Sequential,
    Random,
    Strided { stride: u64 },
    Mixed,
}

#[derive(Debug, Clone, PartialEq)]
enum PassthroughMode {
    Unavailable,
    Available { kernel_version: (u32, u32) },
    Active,
}

#[derive(Debug, Clone, PartialEq)]
enum TransferSize {
    Small,
    Medium,
    Large,
    Huge,
}

impl TransferSize {
    pub fn from_bytes(bytes: u64) -> Self {
        if bytes < 4096 {
            TransferSize::Small
        } else if bytes < 131072 {
            TransferSize::Medium
        } else if bytes < 1048576 {
            TransferSize::Large
        } else {
            TransferSize::Huge
        }
    }
}

#[derive(Debug, Clone)]
struct HotpathConfig {
    passthrough_mode: PassthroughMode,
    large_io_threshold: u64,
    zero_copy_threshold: u64,
    max_inflight: usize,
    enable_readahead: bool,
}

impl HotpathConfig {
    fn default() -> Self {
        HotpathConfig {
            passthrough_mode: PassthroughMode::Unavailable,
            large_io_threshold: 131072,
            zero_copy_threshold: 65536,
            max_inflight: 32,
            enable_readahead: true,
        }
    }

    fn with_passthrough(kernel_major: u32, kernel_minor: u32) -> Self {
        let passthrough_mode = if kernel_major > 6 || (kernel_major == 6 && kernel_minor >= 8) {
            PassthroughMode::Available {
                kernel_version: (kernel_major, kernel_minor),
            }
        } else {
            PassthroughMode::Unavailable
        };
        HotpathConfig {
            passthrough_mode,
            large_io_threshold: 131072,
            zero_copy_threshold: 65536,
            max_inflight: 32,
            enable_readahead: true,
        }
    }
}

#[derive(Debug, Clone)]
struct IoRequest {
    request_id: u64,
    inode: u64,
    offset: u64,
    size: u64,
    is_write: bool,
}

impl IoRequest {
    fn new(request_id: u64, inode: u64, offset: u64, size: u64, is_write: bool) -> Self {
        IoRequest {
            request_id,
            inode,
            offset,
            size,
            is_write,
        }
    }

    fn transfer_size(&self) -> TransferSize {
        TransferSize::from_bytes(self.size)
    }

    fn is_large(&self, threshold: u64) -> bool {
        self.size > threshold
    }
}

#[derive(Debug)]
struct InflightTracker {
    requests: HashMap<u64, IoRequest>,
    max_inflight: usize,
}

impl InflightTracker {
    fn new(max_inflight: usize) -> Self {
        InflightTracker {
            requests: HashMap::new(),
            max_inflight,
        }
    }

    fn submit(&mut self, req: IoRequest) -> Result<()> {
        if self.is_at_capacity() {
            return Err(FuseError::Busy);
        }
        self.requests.insert(req.request_id, req);
        Ok(())
    }

    fn complete(&mut self, request_id: u64) -> Option<IoRequest> {
        self.requests.remove(&request_id)
    }

    fn inflight_count(&self) -> usize {
        self.requests.len()
    }

    fn is_at_capacity(&self) -> bool {
        self.requests.len() >= self.max_inflight
    }

    fn has_request(&self, request_id: u64) -> bool {
        self.requests.contains_key(&request_id)
    }
}

#[derive(Debug)]
struct PatternDetector {
    last_offset: HashMap<u64, u64>,
    access_count: HashMap<u64, u64>,
    sequential_run: HashMap<u64, u64>,
}

impl PatternDetector {
    fn new() -> Self {
        PatternDetector {
            last_offset: HashMap::new(),
            access_count: HashMap::new(),
            sequential_run: HashMap::new(),
        }
    }

    fn record_access(&mut self, inode: u64, offset: u64, size: u64) {
        let access_count = self.access_count.entry(inode).or_insert(0);
        *access_count += 1;

        if let Some(&last_offset) = self.last_offset.get(&inode) {
            let expected_offset = last_offset;
            let sequential_run = self.sequential_run.entry(inode).or_insert(1);

            if offset == expected_offset {
                *sequential_run += 1;
            } else if offset > expected_offset && offset <= expected_offset + size {
                *sequential_run += 1;
            } else {
                *sequential_run = 1;
            }
        } else {
            self.sequential_run.insert(inode, 1);
        }

        self.last_offset.insert(inode, offset + size);
    }

    fn detect_pattern(&self, inode: u64) -> IoPattern {
        let count = self.access_count.get(&inode).unwrap_or(&0);
        let run = self.sequential_run.get(&inode).unwrap_or(&0);

        if *count >= 5 && *run >= 5 {
            IoPattern::Sequential
        } else if *count >= 3 && *run <= 2 {
            IoPattern::Random
        } else {
            IoPattern::Mixed
        }
    }

    fn reset_inode(&mut self, inode: u64) {
        self.last_offset.remove(&inode);
        self.access_count.remove(&inode);
        self.sequential_run.remove(&inode);
    }

    fn tracked_inodes(&self) -> usize {
        self.access_count.len()
    }
}

#[derive(Debug, Clone, PartialEq)]
enum HotpathDecision {
    Standard,
    ZeroCopy,
    Passthrough,
    Readahead { prefetch_bytes: u64 },
}

#[derive(Debug)]
struct HotpathRouter {
    config: HotpathConfig,
    pattern_detector: PatternDetector,
    inflight: InflightTracker,
}

impl HotpathRouter {
    fn new(config: HotpathConfig) -> Self {
        let max_inflight = config.max_inflight;
        HotpathRouter {
            config,
            pattern_detector: PatternDetector::new(),
            inflight: InflightTracker::new(max_inflight),
        }
    }

    fn route(&mut self, req: &IoRequest) -> HotpathDecision {
        if self.inflight.has_request(req.request_id) {
            trace!(
                "Request {} already in-flight, using Standard",
                req.request_id
            );
            return HotpathDecision::Standard;
        }

        if let PassthroughMode::Active = self.config.passthrough_mode {
            if req.size > self.config.zero_copy_threshold {
                debug!("Passthrough mode active, routing large request to passthrough");
                return HotpathDecision::Passthrough;
            }
        }

        if req.size > self.config.large_io_threshold {
            return HotpathDecision::ZeroCopy;
        }

        self.pattern_detector
            .record_access(req.inode, req.offset, req.size);
        let pattern = self.pattern_detector.detect_pattern(req.inode);

        if let IoPattern::Sequential = pattern {
            if !req.is_write && self.config.enable_readahead {
                let prefetch_bytes = req.size * 2;
                debug!(
                    "Sequential read detected, enabling readahead: {} bytes",
                    prefetch_bytes
                );
                return HotpathDecision::Readahead { prefetch_bytes };
            }
        }

        HotpathDecision::Standard
    }

    fn submit_request(&mut self, req: IoRequest) -> Result<HotpathDecision> {
        self.inflight.submit(req.clone())?;
        let decision = self.route(&req);
        debug!(
            "Submitted request {}, decision: {:?}, inflight: {}",
            req.request_id,
            decision,
            self.inflight.inflight_count()
        );
        Ok(decision)
    }

    fn complete_request(&mut self, request_id: u64) -> Option<IoRequest> {
        let result = self.inflight.complete(request_id);
        if result.is_some() {
            trace!(
                "Completed request {}, remaining: {}",
                request_id,
                self.inflight.inflight_count()
            );
        }
        result
    }

    fn inflight_count(&self) -> usize {
        self.inflight.inflight_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transfer_size_small() {
        assert_eq!(TransferSize::from_bytes(0), TransferSize::Small);
        assert_eq!(TransferSize::from_bytes(4095), TransferSize::Small);
    }

    #[test]
    fn test_transfer_size_medium() {
        assert_eq!(TransferSize::from_bytes(4096), TransferSize::Medium);
        assert_eq!(TransferSize::from_bytes(131071), TransferSize::Medium);
    }

    #[test]
    fn test_transfer_size_large() {
        assert_eq!(TransferSize::from_bytes(131072), TransferSize::Large);
        assert_eq!(TransferSize::from_bytes(1048575), TransferSize::Large);
    }

    #[test]
    fn test_transfer_size_huge() {
        assert_eq!(TransferSize::from_bytes(1048576), TransferSize::Huge);
        assert_eq!(TransferSize::from_bytes(10000000), TransferSize::Huge);
    }

    #[test]
    fn test_hotpath_config_default() {
        let config = HotpathConfig::default();
        assert_eq!(config.passthrough_mode, PassthroughMode::Unavailable);
        assert_eq!(config.large_io_threshold, 131072);
        assert_eq!(config.zero_copy_threshold, 65536);
        assert_eq!(config.max_inflight, 32);
        assert!(config.enable_readahead);
    }

    #[test]
    fn test_hotpath_config_with_passthrough_68() {
        let config = HotpathConfig::with_passthrough(6, 8);
        assert_eq!(
            config.passthrough_mode,
            PassthroughMode::Available {
                kernel_version: (6, 8)
            }
        );
    }

    #[test]
    fn test_hotpath_config_with_passthrough_69() {
        let config = HotpathConfig::with_passthrough(6, 9);
        assert_eq!(
            config.passthrough_mode,
            PassthroughMode::Available {
                kernel_version: (6, 9)
            }
        );
    }

    #[test]
    fn test_hotpath_config_with_passthrough_67() {
        let config = HotpathConfig::with_passthrough(6, 7);
        assert_eq!(config.passthrough_mode, PassthroughMode::Unavailable);
    }

    #[test]
    fn test_hotpath_config_with_passthrough_70() {
        let config = HotpathConfig::with_passthrough(7, 0);
        assert_eq!(
            config.passthrough_mode,
            PassthroughMode::Available {
                kernel_version: (7, 0)
            }
        );
    }

    #[test]
    fn test_io_request_new() {
        let req = IoRequest::new(1, 100, 0, 4096, false);
        assert_eq!(req.request_id, 1);
        assert_eq!(req.inode, 100);
        assert_eq!(req.offset, 0);
        assert_eq!(req.size, 4096);
        assert!(!req.is_write);
    }

    #[test]
    fn test_io_request_transfer_size() {
        let small = IoRequest::new(1, 1, 0, 1000, false);
        let medium = IoRequest::new(2, 1, 0, 50000, false);
        let large = IoRequest::new(3, 1, 0, 200000, false);
        let huge = IoRequest::new(4, 1, 0, 2000000, false);

        assert_eq!(small.transfer_size(), TransferSize::Small);
        assert_eq!(medium.transfer_size(), TransferSize::Medium);
        assert_eq!(large.transfer_size(), TransferSize::Large);
        assert_eq!(huge.transfer_size(), TransferSize::Huge);
    }

    #[test]
    fn test_io_request_is_large() {
        let req = IoRequest::new(1, 1, 0, 100000, false);
        assert!(req.is_large(50000));
        assert!(!req.is_large(150000));
    }

    #[test]
    fn test_inflight_tracker_submit() {
        let mut tracker = InflightTracker::new(3);
        let req = IoRequest::new(1, 1, 0, 4096, false);

        assert!(tracker.submit(req).is_ok());
        assert_eq!(tracker.inflight_count(), 1);
    }

    #[test]
    fn test_inflight_tracker_at_capacity() {
        let mut tracker = InflightTracker::new(2);
        tracker
            .submit(IoRequest::new(1, 1, 0, 4096, false))
            .unwrap();
        tracker
            .submit(IoRequest::new(2, 1, 0, 4096, false))
            .unwrap();

        assert!(tracker.is_at_capacity());
        assert!(tracker
            .submit(IoRequest::new(3, 1, 0, 4096, false))
            .is_err());
    }

    #[test]
    fn test_inflight_tracker_complete() {
        let mut tracker = InflightTracker::new(10);
        let req = IoRequest::new(1, 1, 0, 4096, false);
        tracker.submit(req).unwrap();

        let completed = tracker.complete(1);
        assert!(completed.is_some());
        assert_eq!(tracker.inflight_count(), 0);
    }

    #[test]
    fn test_inflight_tracker_complete_not_found() {
        let mut tracker = InflightTracker::new(10);
        let result = tracker.complete(999);
        assert!(result.is_none());
    }

    #[test]
    fn test_inflight_tracker_has_request() {
        let mut tracker = InflightTracker::new(10);
        let req = IoRequest::new(1, 1, 0, 4096, false);
        tracker.submit(req).unwrap();

        assert!(tracker.has_request(1));
        assert!(!tracker.has_request(2));
    }

    #[test]
    fn test_pattern_detector_record_access() {
        let mut detector = PatternDetector::new();
        detector.record_access(1, 0, 4096);
        assert_eq!(detector.tracked_inodes(), 1);
    }

    #[test]
    fn test_pattern_detector_sequential_detection() {
        let mut detector = PatternDetector::new();
        let inode = 1;

        for i in 0..5 {
            detector.record_access(inode, i * 4096, 4096);
        }

        assert_eq!(detector.detect_pattern(inode), IoPattern::Sequential);
    }

    #[test]
    fn test_pattern_detector_random_access() {
        let mut detector = PatternDetector::new();
        let inode = 1;

        detector.record_access(inode, 0, 4096);
        detector.record_access(inode, 10000, 4096);
        detector.record_access(inode, 50000, 4096);

        assert_eq!(detector.detect_pattern(inode), IoPattern::Random);
    }

    #[test]
    fn test_pattern_detector_reset() {
        let mut detector = PatternDetector::new();
        detector.record_access(1, 0, 4096);
        assert_eq!(detector.tracked_inodes(), 1);

        detector.reset_inode(1);
        assert_eq!(detector.tracked_inodes(), 0);
    }

    #[test]
    fn test_hotpath_router_route_standard() {
        let config = HotpathConfig::default();
        let mut router = HotpathRouter::new(config);
        let req = IoRequest::new(1, 1, 0, 4096, false);

        let decision = router.route(&req);
        assert_eq!(decision, HotpathDecision::Standard);
    }

    #[test]
    fn test_hotpath_router_route_zero_copy() {
        let config = HotpathConfig::default();
        let mut router = HotpathRouter::new(config);
        let req = IoRequest::new(1, 1, 0, 200000, false);

        let decision = router.route(&req);
        assert_eq!(decision, HotpathDecision::ZeroCopy);
    }

    #[test]
    fn test_hotpath_router_route_passthrough() {
        let mut config = HotpathConfig::default();
        config.passthrough_mode = PassthroughMode::Active;
        let mut router = HotpathRouter::new(config);
        let req = IoRequest::new(1, 1, 0, 100000, false);

        let decision = router.route(&req);
        assert_eq!(decision, HotpathDecision::Passthrough);
    }

    #[test]
    fn test_hotpath_router_route_readahead() {
        let mut config = HotpathConfig::default();
        config.passthrough_mode = PassthroughMode::Unavailable;
        let mut router = HotpathRouter::new(config);

        let inode = 1;
        for i in 0..5 {
            router.route(&IoRequest::new(i + 1, inode, i * 4096, 4096, false));
        }

        let req = IoRequest::new(6, inode, 5 * 4096, 4096, false);
        let decision = router.route(&req);
        assert_eq!(
            decision,
            HotpathDecision::Readahead {
                prefetch_bytes: 8192
            }
        );
    }

    #[test]
    fn test_hotpath_router_submit_and_complete() {
        let config = HotpathConfig::default();
        let mut router = HotpathRouter::new(config);
        let req = IoRequest::new(1, 1, 0, 4096, false);

        let decision = router.submit_request(req).unwrap();
        assert_eq!(router.inflight_count(), 1);

        let completed = router.complete_request(1);
        assert!(completed.is_some());
        assert_eq!(router.inflight_count(), 0);
    }

    #[test]
    fn test_hotpath_router_double_route_prevention() {
        let config = HotpathConfig::default();
        let mut router = HotpathRouter::new(config);
        let req = IoRequest::new(1, 1, 0, 200000, false);

        router.submit_request(req).unwrap();
        let decision = router.route(&IoRequest::new(1, 1, 0, 200000, false));

        assert_eq!(decision, HotpathDecision::Standard);
    }

    #[test]
    fn test_hotpath_router_no_readahead_for_writes() {
        let config = HotpathConfig::default();
        let mut router = HotpathRouter::new(config);

        let inode = 1;
        for i in 0..5 {
            router.route(&IoRequest::new(i + 1, inode, i * 4096, 4096, false));
        }

        let req = IoRequest::new(6, inode, 5 * 4096, 4096, true);
        let decision = router.route(&req);
        assert_eq!(decision, HotpathDecision::Standard);
    }

    #[test]
    fn test_hotpath_router_readahead_disabled() {
        let config = HotpathConfig {
            passthrough_mode: PassthroughMode::Unavailable,
            large_io_threshold: 131072,
            zero_copy_threshold: 65536,
            max_inflight: 32,
            enable_readahead: false,
        };
        let mut router = HotpathRouter::new(config);

        let inode = 1;
        for i in 0..5 {
            router.route(&IoRequest::new(i + 1, inode, i * 4096, 4096, false));
        }

        let req = IoRequest::new(6, inode, 5 * 4096, 4096, false);
        let decision = router.route(&req);
        assert_eq!(decision, HotpathDecision::Standard);
    }
}
