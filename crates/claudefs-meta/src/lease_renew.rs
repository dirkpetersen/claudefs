//! Automatic lease renewal for metadata cache consistency.
//!
//! Extends the LeaseManager with configurable auto-renewal policies.
//! Leases approaching expiry are automatically renewed if the client
//! is still active, preventing unnecessary cache invalidation.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::types::*;

/// Configuration for automatic lease renewal.
#[derive(Clone, Debug)]
pub struct LeaseRenewConfig {
    /// Fraction of lease duration at which to trigger renewal (0.0-1.0).
    /// Default: 0.8 (renew at 80% of lease duration).
    pub renew_threshold: f64,
    /// Maximum number of consecutive auto-renewals before requiring explicit client contact.
    pub max_auto_renewals: u32,
    /// Grace period after lease expiry during which the lease is still considered valid.
    pub grace_period: Duration,
    /// Whether auto-renewal is enabled.
    pub enabled: bool,
}

impl Default for LeaseRenewConfig {
    fn default() -> Self {
        Self {
            renew_threshold: 0.8,
            max_auto_renewals: 10,
            grace_period: Duration::from_secs(5),
            enabled: true,
        }
    }
}

/// Tracks renewal state for a single lease.
#[derive(Clone, Debug)]
pub struct LeaseRenewState {
    /// The lease ID being tracked.
    pub lease_id: u64,
    /// Inode the lease is for.
    pub ino: InodeId,
    /// Client holding the lease.
    pub client: NodeId,
    /// When the lease was originally granted.
    pub granted_at: Instant,
    /// When the lease expires.
    pub expires_at: Instant,
    /// Number of auto-renewals performed.
    pub renewal_count: u32,
    /// When the last renewal was performed.
    pub last_renewed: Instant,
    /// Lease duration for calculating renewal threshold.
    pub lease_duration: Duration,
}

impl LeaseRenewState {
    /// Creates a new renewal state.
    pub fn new(lease_id: u64, ino: InodeId, client: NodeId, lease_duration: Duration) -> Self {
        let now = Instant::now();
        Self {
            lease_id,
            ino,
            client,
            granted_at: now,
            expires_at: now + lease_duration,
            renewal_count: 0,
            last_renewed: now,
            lease_duration,
        }
    }

    /// Checks if the lease needs renewal based on the threshold.
    pub fn needs_renewal(&self, threshold: f64) -> bool {
        let total = self.lease_duration.as_secs_f64();
        let remaining = self
            .expires_at
            .saturating_duration_since(Instant::now())
            .as_secs_f64();
        let elapsed_fraction = 1.0 - (remaining / total);
        elapsed_fraction >= threshold
    }

    /// Records a renewal.
    pub fn record_renewal(&mut self) {
        let now = Instant::now();
        self.expires_at = now + self.lease_duration;
        self.last_renewed = now;
        self.renewal_count += 1;
    }

    /// Checks if the lease is expired (past grace period).
    pub fn is_expired(&self, grace: Duration) -> bool {
        Instant::now() > self.expires_at + grace
    }

    /// Checks if auto-renewal limit is reached.
    pub fn at_renewal_limit(&self, max: u32) -> bool {
        self.renewal_count >= max
    }

    /// Returns time until expiry.
    pub fn time_to_expiry(&self) -> Duration {
        self.expires_at.saturating_duration_since(Instant::now())
    }
}

/// Actions returned by the renewal manager.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenewalAction {
    /// Renew the lease (call LeaseManager::renew).
    Renew(u64),
    /// Notify client that lease is expiring (client should re-confirm).
    NotifyClient {
        /// The lease ID that needs client attention.
        lease_id: u64,
        /// The client to notify.
        client: NodeId,
    },
    /// Expire the lease (client unresponsive, max renewals reached).
    Expire(u64),
}

/// Manages automatic lease renewal policies.
pub struct LeaseRenewManager {
    config: LeaseRenewConfig,
    tracked: HashMap<u64, LeaseRenewState>,
    active_clients: HashSet<NodeId>,
}

impl LeaseRenewManager {
    /// Creates a new renewal manager.
    pub fn new(config: LeaseRenewConfig) -> Self {
        Self {
            config,
            tracked: HashMap::new(),
            active_clients: HashSet::new(),
        }
    }

    /// Registers a lease for automatic renewal tracking.
    pub fn track_lease(
        &mut self,
        lease_id: u64,
        ino: InodeId,
        client: NodeId,
        lease_duration: Duration,
    ) {
        let state = LeaseRenewState::new(lease_id, ino, client, lease_duration);
        self.tracked.insert(lease_id, state);
    }

    /// Removes a lease from tracking (e.g., lease explicitly revoked).
    pub fn untrack_lease(&mut self, lease_id: u64) {
        self.tracked.remove(&lease_id);
    }

    /// Marks a client as active (e.g., recent heartbeat or operation).
    pub fn client_active(&mut self, client: NodeId) {
        self.active_clients.insert(client);
    }

    /// Marks a client as inactive.
    pub fn client_inactive(&mut self, client: &NodeId) {
        self.active_clients.remove(client);
    }

    /// Checks all tracked leases and returns actions needed.
    pub fn check_renewals(&mut self) -> Vec<RenewalAction> {
        if !self.config.enabled {
            return Vec::new();
        }

        let mut actions = Vec::new();

        let lease_ids: Vec<u64> = self.tracked.keys().copied().collect();

        for lease_id in lease_ids {
            let state = match self.tracked.get(&lease_id) {
                Some(s) => s.clone(),
                None => continue,
            };

            if state.is_expired(self.config.grace_period) {
                actions.push(RenewalAction::Expire(lease_id));
                continue;
            }

            if !state.needs_renewal(self.config.renew_threshold) {
                continue;
            }

            if state.at_renewal_limit(self.config.max_auto_renewals) {
                actions.push(RenewalAction::NotifyClient {
                    lease_id,
                    client: state.client,
                });
                continue;
            }

            if self.active_clients.contains(&state.client) {
                actions.push(RenewalAction::Renew(lease_id));
            } else {
                actions.push(RenewalAction::NotifyClient {
                    lease_id,
                    client: state.client,
                });
            }
        }

        actions
    }

    /// Applies a renewal action (updates internal state).
    pub fn apply_renewal(&mut self, lease_id: u64) {
        if let Some(state) = self.tracked.get_mut(&lease_id) {
            state.record_renewal();
            tracing::debug!(
                lease_id,
                renewal_count = state.renewal_count,
                "auto-renewed lease"
            );
        }
    }

    /// Removes expired leases from tracking.
    pub fn cleanup_expired(&mut self) -> Vec<u64> {
        let expired: Vec<u64> = self
            .tracked
            .iter()
            .filter(|(_, s)| s.is_expired(self.config.grace_period))
            .map(|(id, _)| *id)
            .collect();

        for id in &expired {
            self.tracked.remove(id);
        }

        expired
    }

    /// Returns the number of tracked leases.
    pub fn tracked_count(&self) -> usize {
        self.tracked.len()
    }

    /// Returns the number of active clients.
    pub fn active_client_count(&self) -> usize {
        self.active_clients.len()
    }

    /// Returns all leases for a specific client.
    pub fn leases_for_client(&self, client: &NodeId) -> Vec<u64> {
        self.tracked
            .iter()
            .filter(|(_, s)| s.client == *client)
            .map(|(id, _)| *id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manager() -> LeaseRenewManager {
        LeaseRenewManager::new(LeaseRenewConfig::default())
    }

    #[test]
    fn test_track_lease() {
        let mut mgr = make_manager();
        mgr.track_lease(
            1,
            InodeId::new(100),
            NodeId::new(1),
            Duration::from_secs(30),
        );
        assert_eq!(mgr.tracked_count(), 1);
    }

    #[test]
    fn test_untrack_lease() {
        let mut mgr = make_manager();
        mgr.track_lease(
            1,
            InodeId::new(100),
            NodeId::new(1),
            Duration::from_secs(30),
        );
        mgr.untrack_lease(1);
        assert_eq!(mgr.tracked_count(), 0);
    }

    #[test]
    fn test_fresh_lease_no_renewal() {
        let mut mgr = make_manager();
        mgr.track_lease(
            1,
            InodeId::new(100),
            NodeId::new(1),
            Duration::from_secs(30),
        );
        mgr.client_active(NodeId::new(1));

        let actions = mgr.check_renewals();
        assert!(actions.is_empty(), "fresh lease should not need renewal");
    }

    #[test]
    fn test_needs_renewal_threshold() {
        let state = LeaseRenewState::new(
            1,
            InodeId::new(100),
            NodeId::new(1),
            Duration::from_millis(10),
        );
        // With a 10ms lease, it should need renewal very quickly
        std::thread::sleep(Duration::from_millis(9));
        assert!(state.needs_renewal(0.8));
    }

    #[test]
    fn test_renewal_count_tracking() {
        let mut state = LeaseRenewState::new(
            1,
            InodeId::new(100),
            NodeId::new(1),
            Duration::from_secs(30),
        );
        assert_eq!(state.renewal_count, 0);
        state.record_renewal();
        assert_eq!(state.renewal_count, 1);
        state.record_renewal();
        assert_eq!(state.renewal_count, 2);
    }

    #[test]
    fn test_at_renewal_limit() {
        let mut state = LeaseRenewState::new(
            1,
            InodeId::new(100),
            NodeId::new(1),
            Duration::from_secs(30),
        );
        for _ in 0..10 {
            state.record_renewal();
        }
        assert!(state.at_renewal_limit(10));
    }

    #[test]
    fn test_active_client_tracking() {
        let mut mgr = make_manager();
        mgr.client_active(NodeId::new(1));
        mgr.client_active(NodeId::new(2));
        assert_eq!(mgr.active_client_count(), 2);

        mgr.client_inactive(&NodeId::new(1));
        assert_eq!(mgr.active_client_count(), 1);
    }

    #[test]
    fn test_leases_for_client() {
        let mut mgr = make_manager();
        mgr.track_lease(
            1,
            InodeId::new(100),
            NodeId::new(1),
            Duration::from_secs(30),
        );
        mgr.track_lease(
            2,
            InodeId::new(200),
            NodeId::new(1),
            Duration::from_secs(30),
        );
        mgr.track_lease(
            3,
            InodeId::new(300),
            NodeId::new(2),
            Duration::from_secs(30),
        );

        let client1_leases = mgr.leases_for_client(&NodeId::new(1));
        assert_eq!(client1_leases.len(), 2);
    }

    #[test]
    fn test_disabled_returns_no_actions() {
        let mut mgr = LeaseRenewManager::new(LeaseRenewConfig {
            enabled: false,
            ..Default::default()
        });
        mgr.track_lease(
            1,
            InodeId::new(100),
            NodeId::new(1),
            Duration::from_millis(1),
        );
        std::thread::sleep(Duration::from_millis(5));
        let actions = mgr.check_renewals();
        assert!(actions.is_empty());
    }

    #[test]
    fn test_apply_renewal_updates_state() {
        let mut mgr = make_manager();
        mgr.track_lease(
            1,
            InodeId::new(100),
            NodeId::new(1),
            Duration::from_secs(30),
        );
        mgr.apply_renewal(1);

        let state = &mgr.tracked[&1];
        assert_eq!(state.renewal_count, 1);
    }

    #[test]
    fn test_time_to_expiry() {
        let state = LeaseRenewState::new(
            1,
            InodeId::new(100),
            NodeId::new(1),
            Duration::from_secs(30),
        );
        let ttl = state.time_to_expiry();
        // Should be close to 30 seconds
        assert!(ttl.as_secs() >= 29);
    }
}
