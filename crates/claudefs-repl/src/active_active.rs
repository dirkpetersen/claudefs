use serde::{Deserialize, Serialize};
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SiteRole {
    Primary,
    Secondary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinkStatus {
    Up,
    Degraded,
    Down,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwardedWrite {
    pub origin_site_id: String,
    pub logical_time: u64,
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteConflict {
    pub key: Vec<u8>,
    pub local_time: u64,
    pub remote_time: u64,
    pub winner: SiteRole,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActiveActiveStats {
    pub writes_forwarded: u64,
    pub conflicts_resolved: u64,
    pub link_flaps: u64,
}

#[derive(Debug)]
pub struct ActiveActiveController {
    pub site_id: String,
    pub role: SiteRole,
    pub link_status: LinkStatus,
    logical_time: u64,
    pending_forwards: Vec<ForwardedWrite>,
    stats: ActiveActiveStats,
}

impl ActiveActiveController {
    pub fn new(site_id: String, role: SiteRole) -> Self {
        Self {
            site_id,
            role,
            link_status: LinkStatus::Down,
            logical_time: 0,
            pending_forwards: Vec::new(),
            stats: ActiveActiveStats::default(),
        }
    }

    pub fn local_write(&mut self, key: Vec<u8>, value: Vec<u8>) -> ForwardedWrite {
        self.logical_time += 1;
        let fw = ForwardedWrite {
            origin_site_id: self.site_id.clone(),
            logical_time: self.logical_time,
            key: key.clone(),
            value,
        };
        self.pending_forwards.push(fw.clone());
        self.stats.writes_forwarded += 1;
        info!(
            "Local write at site {} with logical_time {}",
            self.site_id, self.logical_time
        );
        fw
    }

    pub fn apply_remote_write(&mut self, fw: ForwardedWrite) -> Option<WriteConflict> {
        if fw.logical_time == self.logical_time {
            let winner = if self.site_id < fw.origin_site_id {
                self.role
            } else {
                SiteRole::Primary
            };
            warn!(
                "Conflict detected at site {} for key {:?}: local_time={}, remote_time={}, winner={:?}",
                self.site_id, fw.key, self.logical_time, fw.logical_time, winner
            );
            self.stats.conflicts_resolved += 1;
            return Some(WriteConflict {
                key: fw.key,
                local_time: self.logical_time,
                remote_time: fw.logical_time,
                winner,
            });
        }
        self.logical_time = self.logical_time.max(fw.logical_time + 1);
        None
    }

    pub fn set_link_status(&mut self, status: LinkStatus) {
        let old_status = self.link_status;
        if status == LinkStatus::Up && old_status != LinkStatus::Up {
            self.stats.link_flaps += 1;
            info!(
                "Link flap detected at site {}: {:?} -> {:?}",
                self.site_id, old_status, status
            );
        }
        self.link_status = status;
    }

    pub fn stats(&self) -> &ActiveActiveStats {
        &self.stats
    }

    pub fn drain_pending(&mut self) -> Vec<ForwardedWrite> {
        let pending = std::mem::take(&mut self.pending_forwards);
        info!(
            "Drained {} pending writes from site {}",
            pending.len(),
            self.site_id
        );
        pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_site_role_serialization() {
        let role = SiteRole::Primary;
        let serialized = serde_json::to_string(&role).unwrap();
        assert_eq!(serialized, "\"Primary\"");
        let deserialized: SiteRole = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, SiteRole::Primary);

        let role = SiteRole::Secondary;
        let serialized = serde_json::to_string(&role).unwrap();
        assert_eq!(serialized, "\"Secondary\"");
    }

    #[test]
    fn test_link_status_serialization() {
        for status in &[LinkStatus::Up, LinkStatus::Degraded, LinkStatus::Down] {
            let serialized = serde_json::to_string(status).unwrap();
            let deserialized: LinkStatus = serde_json::from_str(&serialized).unwrap();
            assert_eq!(*status, deserialized);
        }
    }

    #[test]
    fn test_forwarded_write_serialization() {
        let fw = ForwardedWrite {
            origin_site_id: "site-a".to_string(),
            logical_time: 42,
            key: b"key1".to_vec(),
            value: b"value1".to_vec(),
        };
        let serialized = serde_json::to_string(&fw).unwrap();
        let deserialized: ForwardedWrite = serde_json::from_str(&serialized).unwrap();
        assert_eq!(fw.origin_site_id, deserialized.origin_site_id);
        assert_eq!(fw.logical_time, deserialized.logical_time);
        assert_eq!(fw.key, deserialized.key);
        assert_eq!(fw.value, deserialized.value);
    }

    #[test]
    fn test_write_conflict_serialization() {
        let conflict = WriteConflict {
            key: b"testkey".to_vec(),
            local_time: 10,
            remote_time: 10,
            winner: SiteRole::Primary,
        };
        let serialized = serde_json::to_string(&conflict).unwrap();
        let deserialized: WriteConflict = serde_json::from_str(&serialized).unwrap();
        assert_eq!(conflict.key, deserialized.key);
        assert_eq!(conflict.local_time, deserialized.local_time);
        assert_eq!(conflict.remote_time, deserialized.remote_time);
        assert_eq!(conflict.winner, deserialized.winner);
    }

    #[test]
    fn test_active_active_stats_default() {
        let stats = ActiveActiveStats::default();
        assert_eq!(stats.writes_forwarded, 0);
        assert_eq!(stats.conflicts_resolved, 0);
        assert_eq!(stats.link_flaps, 0);
    }

    #[test]
    fn test_active_active_stats_clone() {
        let stats = ActiveActiveStats {
            writes_forwarded: 100,
            conflicts_resolved: 5,
            link_flaps: 2,
        };
        let cloned = stats.clone();
        assert_eq!(stats.writes_forwarded, cloned.writes_forwarded);
        assert_eq!(stats.conflicts_resolved, cloned.conflicts_resolved);
        assert_eq!(stats.link_flaps, cloned.link_flaps);
    }

    #[test]
    fn test_controller_new_primary() {
        let controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        assert_eq!(controller.site_id, "site-a");
        assert_eq!(controller.role, SiteRole::Primary);
        assert_eq!(controller.link_status, LinkStatus::Down);
        assert_eq!(controller.logical_time, 0);
        assert!(controller.pending_forwards.is_empty());
    }

    #[test]
    fn test_controller_new_secondary() {
        let controller = ActiveActiveController::new("site-b".to_string(), SiteRole::Secondary);
        assert_eq!(controller.site_id, "site-b");
        assert_eq!(controller.role, SiteRole::Secondary);
    }

    #[test]
    fn test_local_write_increments_time() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        let fw = controller.local_write(b"key".to_vec(), b"value".to_vec());
        assert_eq!(controller.logical_time, 1);
        assert_eq!(fw.logical_time, 1);

        let fw2 = controller.local_write(b"key2".to_vec(), b"value2".to_vec());
        assert_eq!(controller.logical_time, 2);
        assert_eq!(fw2.logical_time, 2);
    }

    #[test]
    fn test_local_write_populates_pending() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        assert!(controller.pending_forwards.is_empty());

        controller.local_write(b"key1".to_vec(), b"value1".to_vec());
        assert_eq!(controller.pending_forwards.len(), 1);

        controller.local_write(b"key2".to_vec(), b"value2".to_vec());
        assert_eq!(controller.pending_forwards.len(), 2);
    }

    #[test]
    fn test_local_write_updates_stats() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        assert_eq!(controller.stats().writes_forwarded, 0);

        controller.local_write(b"key".to_vec(), b"value".to_vec());
        assert_eq!(controller.stats().writes_forwarded, 1);

        controller.local_write(b"key2".to_vec(), b"value2".to_vec());
        assert_eq!(controller.stats().writes_forwarded, 2);
    }

    #[test]
    fn test_local_write_returns_forwarded_write() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        let fw = controller.local_write(b"testkey".to_vec(), b"testvalue".to_vec());

        assert_eq!(fw.origin_site_id, "site-a");
        assert_eq!(fw.key, b"testkey");
        assert_eq!(fw.value, b"testvalue");
    }

    #[test]
    fn test_apply_remote_write_no_conflict() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        controller.logical_time = 5;

        let fw = ForwardedWrite {
            origin_site_id: "site-b".to_string(),
            logical_time: 10,
            key: b"key".to_vec(),
            value: b"value".to_vec(),
        };
        let conflict = controller.apply_remote_write(fw);
        assert!(conflict.is_none());
        assert_eq!(controller.logical_time, 11);
    }

    #[test]
    fn test_apply_remote_write_conflict_primary_wins() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        controller.logical_time = 5;

        let fw = ForwardedWrite {
            origin_site_id: "site-b".to_string(),
            logical_time: 5,
            key: b"key".to_vec(),
            value: b"value".to_vec(),
        };
        let conflict = controller.apply_remote_write(fw);
        assert!(conflict.is_some());
        let c = conflict.unwrap();
        assert_eq!(c.key, b"key");
        assert_eq!(c.local_time, 5);
        assert_eq!(c.remote_time, 5);
        assert_eq!(c.winner, SiteRole::Primary);
    }

    #[test]
    fn test_apply_remote_write_conflict_lower_site_id_wins() {
        let mut controller = ActiveActiveController::new("site-b".to_string(), SiteRole::Secondary);
        controller.logical_time = 5;

        let fw = ForwardedWrite {
            origin_site_id: "site-a".to_string(),
            logical_time: 5,
            key: b"key".to_vec(),
            value: b"value".to_vec(),
        };
        let conflict = controller.apply_remote_write(fw);
        assert!(conflict.is_some());
        let c = conflict.unwrap();
        assert_eq!(c.winner, SiteRole::Primary);
    }

    #[test]
    fn test_apply_remote_write_increments_conflicts_resolved() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        controller.logical_time = 1;
        assert_eq!(controller.stats().conflicts_resolved, 0);

        let fw = ForwardedWrite {
            origin_site_id: "site-b".to_string(),
            logical_time: 1,
            key: b"key".to_vec(),
            value: b"value".to_vec(),
        };
        controller.apply_remote_write(fw);
        assert_eq!(controller.stats().conflicts_resolved, 1);

        controller.logical_time = 2;
        let fw2 = ForwardedWrite {
            origin_site_id: "site-b".to_string(),
            logical_time: 2,
            key: b"key2".to_vec(),
            value: b"value2".to_vec(),
        };
        controller.apply_remote_write(fw2);
        assert_eq!(controller.stats().conflicts_resolved, 2);
    }

    #[test]
    fn test_set_link_status_tracks_flaps() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        assert_eq!(controller.stats().link_flaps, 0);

        controller.set_link_status(LinkStatus::Down);
        assert_eq!(controller.stats().link_flaps, 0);
        assert_eq!(controller.link_status, LinkStatus::Down);

        controller.set_link_status(LinkStatus::Degraded);
        assert_eq!(controller.stats().link_flaps, 0);

        controller.set_link_status(LinkStatus::Up);
        assert_eq!(controller.stats().link_flaps, 1);
        assert_eq!(controller.link_status, LinkStatus::Up);

        controller.set_link_status(LinkStatus::Down);
        assert_eq!(controller.stats().link_flaps, 1);

        controller.set_link_status(LinkStatus::Up);
        assert_eq!(controller.stats().link_flaps, 2);
    }

    #[test]
    fn test_set_link_status_no_flap_when_already_up() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        controller.set_link_status(LinkStatus::Up);
        assert_eq!(controller.stats().link_flaps, 1);

        controller.set_link_status(LinkStatus::Up);
        assert_eq!(controller.stats().link_flaps, 1);
    }

    #[test]
    fn test_stats_returns_reference() {
        let controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        let stats = controller.stats();
        assert_eq!(stats.writes_forwarded, 0);
    }

    #[test]
    fn test_drain_pending_returns_all() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        controller.local_write(b"key1".to_vec(), b"value1".to_vec());
        controller.local_write(b"key2".to_vec(), b"value2".to_vec());
        controller.local_write(b"key3".to_vec(), b"value3".to_vec());

        let drained = controller.drain_pending();
        assert_eq!(drained.len(), 3);
        assert_eq!(drained[0].key, b"key1");
        assert_eq!(drained[1].key, b"key2");
        assert_eq!(drained[2].key, b"key3");
    }

    #[test]
    fn test_drain_pending_clears() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        controller.local_write(b"key".to_vec(), b"value".to_vec());
        controller.drain_pending();

        assert!(controller.pending_forwards.is_empty());

        let drained = controller.drain_pending();
        assert!(drained.is_empty());
    }

    #[test]
    fn test_apply_remote_write_time_update() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        controller.logical_time = 100;

        let fw = ForwardedWrite {
            origin_site_id: "site-b".to_string(),
            logical_time: 50,
            key: b"key".to_vec(),
            value: b"value".to_vec(),
        };
        controller.apply_remote_write(fw);
        assert_eq!(controller.logical_time, 100);
    }

    #[test]
    fn test_multiple_sites_conflict_resolution() {
        let mut controller_a = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        let mut controller_b =
            ActiveActiveController::new("site-b".to_string(), SiteRole::Secondary);

        controller_a.local_write(b"key".to_vec(), b"value_a".to_vec());
        controller_b.local_write(b"key".to_vec(), b"value_b".to_vec());

        let fw_a = controller_a.drain_pending()[0].clone();
        let fw_b = controller_b.drain_pending()[0].clone();

        let conflict_a = controller_a.apply_remote_write(fw_b);
        let conflict_b = controller_b.apply_remote_write(fw_a);

        assert!(conflict_a.is_some());
        assert!(conflict_b.is_some());

        assert_eq!(conflict_a.unwrap().winner, SiteRole::Primary);
        assert_eq!(conflict_b.unwrap().winner, SiteRole::Primary);
    }

    #[test]
    fn test_empty_pending_drain() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        let drained = controller.drain_pending();
        assert!(drained.is_empty());
    }

    #[test]
    fn test_controller_with_arc_mutex() {
        let controller = Arc::new(Mutex::new(ActiveActiveController::new(
            "site-a".to_string(),
            SiteRole::Primary,
        )));

        {
            let mut c = controller.lock().unwrap();
            c.local_write(b"key".to_vec(), b"value".to_vec());
        }

        {
            let c = controller.lock().unwrap();
            assert_eq!(c.stats().writes_forwarded, 1);
        }
    }

    #[test]
    fn test_serde_roundtrip_all_types() {
        let role = SiteRole::Primary;
        let role_json = serde_json::to_string(&role).unwrap();
        let role_back: SiteRole = serde_json::from_str(&role_json).unwrap();
        assert_eq!(role, role_back);

        let status = LinkStatus::Up;
        let status_json = serde_json::to_string(&status).unwrap();
        let status_back: LinkStatus = serde_json::from_str(&status_json).unwrap();
        assert_eq!(status, status_back);

        let stats = ActiveActiveStats {
            writes_forwarded: 10,
            conflicts_resolved: 2,
            link_flaps: 1,
        };
        let stats_json = serde_json::to_string(&stats).unwrap();
        let stats_back: ActiveActiveStats = serde_json::from_str(&stats_json).unwrap();
        assert_eq!(stats.writes_forwarded, stats_back.writes_forwarded);
    }
}
