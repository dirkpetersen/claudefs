//! Cross-site active-active failover state machine.
//!
//! Implements detection and recovery from site failures in a two-site
//! active-active replication setup.

use crate::site_registry::SiteId;
use serde::{Deserialize, Serialize};
use tracing::{error, warn};

/// Represents the current state of the failover controller.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FailoverState {
    /// Both sites are operational.
    Normal,
    /// One site has failed or has high replication lag.
    Degraded {
        /// The ID of the failed or degraded site.
        failed_site: SiteId,
    },
    /// Failover has occurred, primary is now the specified site.
    Failover {
        /// The new primary site after failover.
        primary: SiteId,
        /// The standby site.
        standby: SiteId,
    },
    /// A failed site is being recovered.
    Recovery {
        /// The ID of the site being recovered.
        recovering_site: SiteId,
    },
    /// Both sites are down or unreachable (split brain).
    SplitBrain,
}

/// Events that can trigger state transitions in the failover controller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FailoverEvent {
    /// A site has gone down.
    SiteDown {
        /// The ID of the site that went down.
        site_id: SiteId,
        /// Timestamp when the failure was detected.
        detected_at_ns: u64,
    },
    /// A site has come back up.
    SiteUp {
        /// The ID of the site that came back up.
        site_id: SiteId,
        /// Timestamp when the recovery was detected.
        detected_at_ns: u64,
    },
    /// Replication lag exceeded threshold.
    ReplicationLagHigh {
        /// The ID of the site with high lag.
        site_id: SiteId,
        /// The lag in nanoseconds.
        lag_ns: u64,
    },
    /// Manual failover was triggered.
    ManualFailover {
        /// The target site to promote as primary.
        target_primary: SiteId,
    },
    /// Recovery of a site has completed.
    RecoveryComplete {
        /// The ID of the recovered site.
        site_id: SiteId,
    },
}

/// Statistics about failover operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FailoverStats {
    /// Total number of state transitions.
    pub state_transitions: u64,
    /// Total number of failover events.
    pub failover_count: u64,
    /// Total number of recovery events.
    pub recovery_count: u64,
    /// Total number of split brain events.
    pub split_brain_count: u64,
}

/// Controller for managing site failover state machine.
pub struct FailoverController {
    state: FailoverState,
    stats: FailoverStats,
    site_a: SiteId,
    site_b: SiteId,
}

impl FailoverController {
    /// Create a new failover controller with the given sites.
    pub fn new(site_a: SiteId, site_b: SiteId) -> Self {
        Self {
            state: FailoverState::Normal,
            stats: FailoverStats::default(),
            site_a,
            site_b,
        }
    }

    /// Process an event and return the new state.
    pub fn process_event(&mut self, event: FailoverEvent) -> FailoverState {
        self.stats.state_transitions += 1;

        let new_state = match (&self.state, &event) {
            // Normal + SiteDown -> Degraded
            (FailoverState::Normal, FailoverEvent::SiteDown { site_id, .. }) => {
                self.stats.failover_count += 1;
                warn!("Site {:?} down, transitioning to Degraded", site_id);
                FailoverState::Degraded {
                    failed_site: *site_id,
                }
            }
            // Normal + ReplicationLagHigh -> Degraded
            (FailoverState::Normal, FailoverEvent::ReplicationLagHigh { site_id, .. }) => {
                warn!(
                    "Replication lag high for site {:?}, transitioning to Degraded",
                    site_id
                );
                FailoverState::Degraded {
                    failed_site: *site_id,
                }
            }
            // Degraded + SiteDown (different site) -> SplitBrain
            (
                FailoverState::Degraded {
                    failed_site: failed,
                },
                FailoverEvent::SiteDown {
                    site_id: new_down, ..
                },
            ) if *new_down != *failed => {
                self.stats.split_brain_count += 1;
                error!(
                    "Second site {:?} down in Degraded state, split brain!",
                    new_down
                );
                FailoverState::SplitBrain
            }
            // Degraded + SiteUp (same as failed) -> Normal
            (FailoverState::Degraded { failed_site }, FailoverEvent::SiteUp { site_id, .. })
                if *site_id == *failed_site =>
            {
                self.stats.recovery_count += 1;
                FailoverState::Normal
            }
            // Degraded + ManualFailover -> Failover
            (FailoverState::Degraded { .. }, FailoverEvent::ManualFailover { target_primary }) => {
                let standby = if *target_primary == self.site_a {
                    self.site_b
                } else {
                    self.site_a
                };
                FailoverState::Failover {
                    primary: *target_primary,
                    standby,
                }
            }
            // Failover + RecoveryComplete -> Recovery
            (FailoverState::Failover { .. }, FailoverEvent::RecoveryComplete { site_id }) => {
                FailoverState::Recovery {
                    recovering_site: *site_id,
                }
            }
            // Recovery + SiteUp -> Normal
            (FailoverState::Recovery { .. }, FailoverEvent::SiteUp { .. }) => {
                self.stats.recovery_count += 1;
                FailoverState::Normal
            }
            // Any other combination: no state change
            _ => self.state.clone(),
        };

        self.state = new_state;
        self.state.clone()
    }

    /// Get a reference to the current state.
    pub fn state(&self) -> &FailoverState {
        &self.state
    }

    /// Get a reference to the statistics.
    pub fn stats(&self) -> &FailoverStats {
        &self.stats
    }

    /// Check if the system is in a degraded state.
    pub fn is_degraded(&self) -> bool {
        matches!(
            self.state,
            FailoverState::Degraded { .. }
                | FailoverState::Failover { .. }
                | FailoverState::Recovery { .. }
                | FailoverState::SplitBrain
        )
    }

    /// Get the current primary site based on state.
    pub fn primary_site(&self) -> SiteId {
        match &self.state {
            FailoverState::Normal
            | FailoverState::Degraded { .. }
            | FailoverState::Recovery { .. } => self.site_a,
            FailoverState::Failover { primary, .. } => *primary,
            FailoverState::SplitBrain => self.site_a,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::site_registry::SiteId;

    fn site(id: u64) -> SiteId {
        SiteId(id)
    }

    #[test]
    fn new_starts_normal() {
        let controller = FailoverController::new(site(1), site(2));
        assert!(matches!(controller.state(), FailoverState::Normal));
    }

    #[test]
    fn site_down_transitions_to_degraded() {
        let mut controller = FailoverController::new(site(1), site(2));
        let new_state = controller.process_event(FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 1000,
        });
        assert!(matches!(
            new_state,
            FailoverState::Degraded { failed_site } if failed_site == site(1)
        ));
    }

    #[test]
    fn site_up_recovers_to_normal() {
        let mut controller = FailoverController::new(site(1), site(2));
        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 1000,
        });
        let new_state = controller.process_event(FailoverEvent::SiteUp {
            site_id: site(1),
            detected_at_ns: 2000,
        });
        assert!(matches!(new_state, FailoverState::Normal));
    }

    #[test]
    fn replication_lag_transitions_to_degraded() {
        let mut controller = FailoverController::new(site(1), site(2));
        let new_state = controller.process_event(FailoverEvent::ReplicationLagHigh {
            site_id: site(1),
            lag_ns: 5000000000,
        });
        assert!(matches!(
            new_state,
            FailoverState::Degraded { failed_site } if failed_site == site(1)
        ));
    }

    #[test]
    fn manual_failover_transitions() {
        let mut controller = FailoverController::new(site(1), site(2));
        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 1000,
        });
        let new_state = controller.process_event(FailoverEvent::ManualFailover {
            target_primary: site(2),
        });
        assert!(matches!(
            new_state,
            FailoverState::Failover { primary, standby } if primary == site(2) && standby == site(1)
        ));
    }

    #[test]
    fn recovery_complete_transitions() {
        let mut controller = FailoverController::new(site(1), site(2));
        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 1000,
        });
        controller.process_event(FailoverEvent::ManualFailover {
            target_primary: site(2),
        });
        let new_state =
            controller.process_event(FailoverEvent::RecoveryComplete { site_id: site(1) });
        assert!(matches!(
            new_state,
            FailoverState::Recovery { recovering_site } if recovering_site == site(1)
        ));
    }

    #[test]
    fn second_site_down_causes_split_brain() {
        let mut controller = FailoverController::new(site(1), site(2));
        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 1000,
        });
        let new_state = controller.process_event(FailoverEvent::SiteDown {
            site_id: site(2),
            detected_at_ns: 2000,
        });
        assert!(matches!(new_state, FailoverState::SplitBrain));
    }

    #[test]
    fn is_degraded_in_normal_false() {
        let controller = FailoverController::new(site(1), site(2));
        assert!(!controller.is_degraded());
    }

    #[test]
    fn is_degraded_in_degraded_true() {
        let mut controller = FailoverController::new(site(1), site(2));
        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 1000,
        });
        assert!(controller.is_degraded());
    }

    #[test]
    fn is_degraded_in_failover_true() {
        let mut controller = FailoverController::new(site(1), site(2));
        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 1000,
        });
        controller.process_event(FailoverEvent::ManualFailover {
            target_primary: site(2),
        });
        assert!(controller.is_degraded());
    }

    #[test]
    fn is_degraded_in_split_brain_true() {
        let mut controller = FailoverController::new(site(1), site(2));
        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 1000,
        });
        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(2),
            detected_at_ns: 2000,
        });
        assert!(controller.is_degraded());
    }

    #[test]
    fn stats_failover_count() {
        let mut controller = FailoverController::new(site(1), site(2));
        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 1000,
        });
        assert_eq!(controller.stats().failover_count, 1);
    }

    #[test]
    fn stats_recovery_count() {
        let mut controller = FailoverController::new(site(1), site(2));
        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 1000,
        });
        controller.process_event(FailoverEvent::SiteUp {
            site_id: site(1),
            detected_at_ns: 2000,
        });
        assert_eq!(controller.stats().recovery_count, 1);
    }

    #[test]
    fn stats_split_brain_count() {
        let mut controller = FailoverController::new(site(1), site(2));
        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 1000,
        });
        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(2),
            detected_at_ns: 2000,
        });
        assert_eq!(controller.stats().split_brain_count, 1);
    }

    #[test]
    fn stats_state_transitions_counted() {
        let mut controller = FailoverController::new(site(1), site(2));
        assert_eq!(controller.stats().state_transitions, 0);
        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 1000,
        });
        assert_eq!(controller.stats().state_transitions, 1);
    }

    #[test]
    fn primary_site_normal() {
        let controller = FailoverController::new(site(1), site(2));
        assert_eq!(controller.primary_site(), site(1));
    }

    #[test]
    fn primary_site_failover() {
        let mut controller = FailoverController::new(site(1), site(2));
        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 1000,
        });
        controller.process_event(FailoverEvent::ManualFailover {
            target_primary: site(2),
        });
        assert_eq!(controller.primary_site(), site(2));
    }

    #[test]
    fn state_getter() {
        let mut controller = FailoverController::new(site(1), site(2));
        assert!(matches!(controller.state(), FailoverState::Normal));
        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 1000,
        });
        assert!(matches!(controller.state(), FailoverState::Degraded { .. }));
    }

    #[test]
    fn stats_getter() {
        let controller = FailoverController::new(site(1), site(2));
        let stats = controller.stats();
        assert_eq!(stats.state_transitions, 0);
        assert_eq!(stats.failover_count, 0);
        assert_eq!(stats.recovery_count, 0);
        assert_eq!(stats.split_brain_count, 0);
    }

    #[test]
    fn failover_event_serialize() {
        let event = FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 1000,
        };
        let serialized = bincode::serialize(&event).unwrap();
        assert!(!serialized.is_empty());
    }

    #[test]
    fn failover_state_serialize() {
        let state = FailoverState::Normal;
        let serialized = bincode::serialize(&state).unwrap();
        assert!(!serialized.is_empty());

        let degraded = FailoverState::Degraded {
            failed_site: site(1),
        };
        let serialized = bincode::serialize(&degraded).unwrap();
        assert!(!serialized.is_empty());
    }

    #[test]
    fn multiple_transitions_stats() {
        let mut controller = FailoverController::new(site(1), site(2));
        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 1000,
        });
        controller.process_event(FailoverEvent::SiteUp {
            site_id: site(1),
            detected_at_ns: 2000,
        });
        controller.process_event(FailoverEvent::SiteDown {
            site_id: site(1),
            detected_at_ns: 3000,
        });
        controller.process_event(FailoverEvent::ManualFailover {
            target_primary: site(2),
        });

        assert_eq!(controller.stats().state_transitions, 4);
        assert_eq!(controller.stats().failover_count, 2);
        assert_eq!(controller.stats().recovery_count, 1);
    }
}
