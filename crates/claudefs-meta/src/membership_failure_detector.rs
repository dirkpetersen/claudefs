use crate::kvstore::KvStore;
use crate::types::MetaError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

fn get_current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// Node membership info for SWIM protocol (cross-site).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemberInfo {
    /// Node ID (site:node_index format, e.g., "A:0", "B:1")
    pub id: String,
    /// Metadata server address (ip:port)
    pub addr: String,
    /// Site identifier (A or B)
    pub site: char,
    /// Current state: alive, suspect, dead
    pub state: MemberState,
    /// Generation counter (updated on state change)
    pub generation: u64,
    /// Last heartbeat time (unix millis)
    pub last_heartbeat: u64,
}

/// State of a cluster member in the SWIM protocol.
#[derive(Clone, Debug, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemberState {
    /// Node is responsive
    Alive,
    /// Node is suspected dead (no heartbeat for T_suspect)
    Suspect,
    /// Node is confirmed dead (no heartbeat for T_dead)
    Dead,
}

/// Failure detector using SWIM protocol (cross-site).
pub struct MembershipFailureDetector {
    members: Arc<RwLock<HashMap<String, MemberInfo>>>,
    local_site: char,
    suspect_timeout_ms: u64,
    dead_timeout_ms: u64,
    kv: Arc<dyn KvStore>,
}

impl MembershipFailureDetector {
    /// Creates a new failure detector with the given timeouts.
    ///
    /// # Arguments
    /// * `local_site` - The local site identifier (A or B)
    /// * `suspect_timeout_ms` - Time in ms after which a node is marked suspect (default: 3000)
    /// * `dead_timeout_ms` - Time in ms after which a suspect node is marked dead (default: 30000)
    /// * `kv` - Key-value store for persisting member state
    pub fn new(
        local_site: char,
        suspect_timeout_ms: u64,
        dead_timeout_ms: u64,
        kv: Arc<dyn KvStore>,
    ) -> Self {
        Self {
            members: Arc::new(RwLock::new(HashMap::new())),
            local_site,
            suspect_timeout_ms,
            dead_timeout_ms,
            kv,
        }
    }

    /// Adds a new member to track.
    pub fn add_member(&self, member: MemberInfo) -> Result<(), MetaError> {
        let mut members = self
            .members
            .write()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        members.insert(member.id.clone(), member);
        Ok(())
    }

    /// Updates the last heartbeat time for a member.
    /// If the member was Suspect or Dead, transitions back to Alive.
    pub fn record_heartbeat(&self, member_id: &str) -> Result<(), MetaError> {
        let mut members = self
            .members
            .write()
            .map_err(|e| MetaError::KvError(e.to_string()))?;

        let member = members
            .get_mut(member_id)
            .ok_or_else(|| MetaError::NotFound(format!("member {} not found", member_id)))?;

        let now = get_current_time_ms();
        let old_state = member.state;

        member.last_heartbeat = now;

        if old_state != MemberState::Alive {
            member.state = MemberState::Alive;
            member.generation += 1;
            tracing::info!(
                "member {} state changed: {:?} -> {:?}, generation: {}",
                member_id,
                old_state,
                member.state,
                member.generation
            );
        }

        Ok(())
    }

    /// Checks all members' health at the given time.
    ///
    /// Members with no heartbeat for T_suspect transition to Suspect.
    /// Members in Suspect state for T_dead transition to Dead.
    /// Returns list of newly-detected dead members.
    /// State changes are persisted to KV for crash recovery.
    pub fn check_health(&self, now_ms: u64) -> Result<Vec<String>, MetaError> {
        let mut members = self
            .members
            .write()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        let mut dead_members = Vec::new();
        let mut to_persist: Vec<(String, MemberInfo)> = Vec::new();

        for (member_id, member) in members.iter_mut() {
            if member.site == self.local_site {
                continue;
            }

            let time_since_heartbeat = now_ms.saturating_sub(member.last_heartbeat);
            let old_state = member.state;

            if old_state == MemberState::Alive && time_since_heartbeat >= self.suspect_timeout_ms {
                member.state = MemberState::Suspect;
                member.generation += 1;
                tracing::info!(
                    "member {} transitioned to Suspect (no heartbeat for {}ms)",
                    member_id,
                    time_since_heartbeat
                );
                to_persist.push((member_id.clone(), member.clone()));
            } else if old_state == MemberState::Suspect
                && time_since_heartbeat >= self.dead_timeout_ms
            {
                member.state = MemberState::Dead;
                member.generation += 1;
                tracing::warn!(
                    "member {} transitioned to Dead (no heartbeat for {}ms)",
                    member_id,
                    time_since_heartbeat
                );
                dead_members.push(member_id.clone());
                to_persist.push((member_id.clone(), member.clone()));
            }
        }

        drop(members);

        for (member_id, member) in to_persist {
            let key = format!("fd:{}", member_id);
            let value =
                bincode::serialize(&member).map_err(|e| MetaError::KvError(e.to_string()))?;
            self.kv.put(key.into_bytes(), value)?;
        }

        for member_id in &dead_members {
            let _ = self.on_member_dead_callback(member_id);
        }

        Ok(dead_members)
    }

    /// Gets the current state of a member by ID.
    pub fn get_member(&self, member_id: &str) -> Result<Option<MemberInfo>, MetaError> {
        let members = self
            .members
            .read()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        Ok(members.get(member_id).cloned())
    }

    /// Returns all members currently in Alive state.
    /// Used for shard replica selection.
    pub fn list_alive_members(&self) -> Result<Vec<MemberInfo>, MetaError> {
        let members = self
            .members
            .read()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        Ok(members
            .values()
            .filter(|m| m.state == MemberState::Alive)
            .cloned()
            .collect())
    }

    /// Called when detector confirms a member is dead.
    /// Logs the dead member event.
    pub fn on_member_dead_callback(&self, member_id: &str) -> Result<(), MetaError> {
        tracing::warn!("member dead callback triggered for: {}", member_id);
        Ok(())
    }

    fn persist_member(&self, member_id: &str, member: &MemberInfo) -> Result<(), MetaError> {
        let key = format!("fd:{}", member_id);
        let value = bincode::serialize(member).map_err(|e| MetaError::KvError(e.to_string()))?;
        self.kv.put(key.into_bytes(), value)
    }

    /// Persists member states to KV store for crash recovery.
    /// Key prefix: "fd:" for failure detector.
    pub fn persist_state(&self) -> Result<(), MetaError> {
        let members = self
            .members
            .read()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        for (member_id, member) in members.iter() {
            self.persist_member(member_id, member)?;
        }
        Ok(())
    }

    /// Loads member states from KV store on startup.
    /// Returns the number of members loaded.
    pub fn load_state(&self) -> Result<usize, MetaError> {
        let pairs = self.kv.scan_prefix(b"fd:")?;
        let mut count = 0;

        for (key, value) in pairs {
            if let Ok(member_id) = std::str::from_utf8(&key[3..]) {
                if let Ok(member) = bincode::deserialize::<MemberInfo>(&value) {
                    let mut members = self
                        .members
                        .write()
                        .map_err(|e| MetaError::KvError(e.to_string()))?;
                    members.insert(member_id.to_string(), member);
                    count += 1;
                }
            }
        }

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kvstore::MemoryKvStore;

    fn setup_detector() -> (MembershipFailureDetector, Arc<MemoryKvStore>) {
        let kv = Arc::new(MemoryKvStore::new());
        let detector = MembershipFailureDetector::new('A', 3000, 30000, kv.clone());
        (detector, kv)
    }

    #[test]
    fn test_init_with_members() {
        let (detector, _kv) = setup_detector();

        let member = MemberInfo {
            id: "B:0".to_string(),
            addr: "192.168.1.10:8080".to_string(),
            site: 'B',
            state: MemberState::Alive,
            generation: 1,
            last_heartbeat: get_current_time_ms(),
        };

        detector.add_member(member).unwrap();

        let result = detector.get_member("B:0").unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "B:0");
    }

    #[test]
    fn test_record_heartbeat_updates_timestamp() {
        let (detector, _kv) = setup_detector();

        let before = get_current_time_ms();

        let member = MemberInfo {
            id: "B:0".to_string(),
            addr: "192.168.1.10:8080".to_string(),
            site: 'B',
            state: MemberState::Alive,
            generation: 1,
            last_heartbeat: before - 10000,
        };

        detector.add_member(member).unwrap();
        detector.record_heartbeat("B:0").unwrap();

        let after = get_current_time_ms();
        let result = detector.get_member("B:0").unwrap().unwrap();

        assert!(result.last_heartbeat >= before);
        assert!(result.last_heartbeat <= after);
    }

    #[test]
    fn test_suspect_after_timeout() {
        let (detector, _kv) = setup_detector();

        let old_time = get_current_time_ms() - 5000;

        let member = MemberInfo {
            id: "B:0".to_string(),
            addr: "192.168.1.10:8080".to_string(),
            site: 'B',
            state: MemberState::Alive,
            generation: 1,
            last_heartbeat: old_time,
        };

        detector.add_member(member).unwrap();

        let now = get_current_time_ms();
        let dead = detector.check_health(now).unwrap();

        assert!(dead.is_empty());

        let result = detector.get_member("B:0").unwrap().unwrap();
        assert_eq!(result.state, MemberState::Suspect);
    }

    #[test]
    fn test_dead_after_suspect_timeout() {
        let (detector, _kv) = setup_detector();

        let old_time = get_current_time_ms() - 35000;

        let member = MemberInfo {
            id: "B:0".to_string(),
            addr: "192.168.1.10:8080".to_string(),
            site: 'B',
            state: MemberState::Suspect,
            generation: 2,
            last_heartbeat: old_time,
        };

        detector.add_member(member).unwrap();

        let now = get_current_time_ms();
        let dead = detector.check_health(now).unwrap();

        assert_eq!(dead.len(), 1);
        assert_eq!(dead[0], "B:0");

        let result = detector.get_member("B:0").unwrap().unwrap();
        assert_eq!(result.state, MemberState::Dead);
    }

    #[test]
    fn test_recovery_to_alive_on_heartbeat() {
        let (detector, _kv) = setup_detector();

        let member = MemberInfo {
            id: "B:0".to_string(),
            addr: "192.168.1.10:8080".to_string(),
            site: 'B',
            state: MemberState::Suspect,
            generation: 2,
            last_heartbeat: get_current_time_ms() - 10000,
        };

        detector.add_member(member).unwrap();
        detector.record_heartbeat("B:0").unwrap();

        let result = detector.get_member("B:0").unwrap().unwrap();
        assert_eq!(result.state, MemberState::Alive);
        assert!(result.generation > 2);
    }

    #[test]
    fn test_list_alive_members_filters_dead() {
        let (detector, _kv) = setup_detector();

        let now = get_current_time_ms();

        detector
            .add_member(MemberInfo {
                id: "B:0".to_string(),
                addr: "192.168.1.10:8080".to_string(),
                site: 'B',
                state: MemberState::Alive,
                generation: 1,
                last_heartbeat: now,
            })
            .unwrap();

        detector
            .add_member(MemberInfo {
                id: "B:1".to_string(),
                addr: "192.168.1.11:8080".to_string(),
                site: 'B',
                state: MemberState::Dead,
                generation: 3,
                last_heartbeat: now - 40000,
            })
            .unwrap();

        let alive = detector.list_alive_members().unwrap();

        assert_eq!(alive.len(), 1);
        assert_eq!(alive[0].id, "B:0");
    }

    #[test]
    fn test_persist_member_state_to_kv() {
        let (detector, kv) = setup_detector();

        let member = MemberInfo {
            id: "B:0".to_string(),
            addr: "192.168.1.10:8080".to_string(),
            site: 'B',
            state: MemberState::Alive,
            generation: 1,
            last_heartbeat: get_current_time_ms(),
        };

        detector.add_member(member).unwrap();
        detector.persist_state().unwrap();

        let result = kv.get(b"fd:B:0").unwrap();
        assert!(result.is_some());

        let restored: MemberInfo = bincode::deserialize(&result.unwrap()).unwrap();
        assert_eq!(restored.id, "B:0");
    }

    #[test]
    fn test_recover_member_state_from_kv() {
        let (_detector, _kv) = setup_detector();

        {
            let kv = Arc::new(MemoryKvStore::new());
            let member = MemberInfo {
                id: "B:0".to_string(),
                addr: "192.168.1.10:8080".to_string(),
                site: 'B',
                state: MemberState::Alive,
                generation: 1,
                last_heartbeat: 12345,
            };
            let key = b"fd:B:0".to_vec();
            let value = bincode::serialize(&member).unwrap();
            kv.put(key, value).unwrap();

            let detector2 = MembershipFailureDetector::new('A', 3000, 30000, kv.clone());
            let count = detector2.load_state().unwrap();
            assert_eq!(count, 1);

            let result = detector2.get_member("B:0").unwrap().unwrap();
            assert_eq!(result.last_heartbeat, 12345);
        }
    }

    #[test]
    fn test_on_member_dead_callback_triggers_log() {
        let (detector, _kv) = setup_detector();

        let member = MemberInfo {
            id: "B:0".to_string(),
            addr: "192.168.1.10:8080".to_string(),
            site: 'B',
            state: MemberState::Suspect,
            generation: 2,
            last_heartbeat: get_current_time_ms() - 35000,
        };

        detector.add_member(member).unwrap();

        let now = get_current_time_ms();
        let _dead = detector.check_health(now).unwrap();
    }

    #[test]
    fn test_concurrent_heartbeats_race_free() {
        let kv = Arc::new(MemoryKvStore::new());
        let detector = Arc::new(MembershipFailureDetector::new('A', 3000, 30000, kv));

        let member = MemberInfo {
            id: "B:0".to_string(),
            addr: "192.168.1.10:8080".to_string(),
            site: 'B',
            state: MemberState::Alive,
            generation: 1,
            last_heartbeat: get_current_time_ms(),
        };

        detector.add_member(member).unwrap();

        let mut handles = vec![];
        for _ in 0..10 {
            let d = detector.clone();
            handles.push(std::thread::spawn(move || {
                for _ in 0..100 {
                    let _ = d.record_heartbeat("B:0");
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        let result = detector.get_member("B:0").unwrap().unwrap();
        assert_eq!(result.state, MemberState::Alive);
    }

    #[test]
    fn test_local_site_excluded_from_dead_check() {
        let (detector, _kv) = setup_detector();

        let old_time = get_current_time_ms() - 40000;

        detector
            .add_member(MemberInfo {
                id: "A:0".to_string(),
                addr: "192.168.1.10:8080".to_string(),
                site: 'A',
                state: MemberState::Alive,
                generation: 1,
                last_heartbeat: old_time,
            })
            .unwrap();

        detector
            .add_member(MemberInfo {
                id: "B:0".to_string(),
                addr: "192.168.1.11:8080".to_string(),
                site: 'B',
                state: MemberState::Suspect,
                generation: 2,
                last_heartbeat: old_time,
            })
            .unwrap();

        let now = get_current_time_ms();
        let dead = detector.check_health(now).unwrap();

        assert_eq!(dead.len(), 1);
        assert_eq!(dead[0], "B:0");

        let local_member = detector.get_member("A:0").unwrap().unwrap();
        assert_eq!(local_member.state, MemberState::Alive);
    }

    #[test]
    fn test_generation_increments_on_state_change() {
        let (detector, _kv) = setup_detector();

        let member = MemberInfo {
            id: "B:0".to_string(),
            addr: "192.168.1.10:8080".to_string(),
            site: 'B',
            state: MemberState::Alive,
            generation: 1,
            last_heartbeat: get_current_time_ms(),
        };

        detector.add_member(member).unwrap();

        let old_time = get_current_time_ms() - 5000;
        {
            let mut members = detector.members.write().unwrap();
            let m = members.get_mut("B:0").unwrap();
            m.last_heartbeat = old_time;
        }

        let _ = detector.check_health(get_current_time_ms()).unwrap();

        let result = detector.get_member("B:0").unwrap().unwrap();
        assert_eq!(result.generation, 2);

        let old_time2 = get_current_time_ms() - 35000;
        {
            let mut members = detector.members.write().unwrap();
            let m = members.get_mut("B:0").unwrap();
            m.last_heartbeat = old_time2;
        }

        let _ = detector.check_health(get_current_time_ms()).unwrap();

        let result = detector.get_member("B:0").unwrap().unwrap();
        assert_eq!(result.generation, 3);
    }
}
