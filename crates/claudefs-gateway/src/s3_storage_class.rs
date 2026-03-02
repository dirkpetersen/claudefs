//! S3 Storage Class Management

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// S3 storage classes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageClass {
    /// Hot storage - lowest latency, highest cost
    Standard,
    /// Infrequent access - lower cost, retrieval fees apply
    StandardIa,
    /// Single-AZ infrequent access - lowest cost for infrequent access
    OnezoneIa,
    /// Auto-tier based on access pattern
    IntelligentTiering,
    /// Archival storage - minutes to restore
    Glacier,
    /// Long-term archive - hours to restore
    GlacierDeepArchive,
    /// Legacy reduced redundancy - lower durability
    ReducedRedundancy,
    /// Ultra-low latency - highest cost
    Express,
}

impl StorageClass {
    /// Parse storage class from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "STANDARD" => Some(StorageClass::Standard),
            "STANDARD_IA" => Some(StorageClass::StandardIa),
            "ONEZONE_IA" => Some(StorageClass::OnezoneIa),
            "INTELLIGENT_TIERING" => Some(StorageClass::IntelligentTiering),
            "GLACIER" => Some(StorageClass::Glacier),
            "GLACIER_DEEP_ARCHIVE" => Some(StorageClass::GlacierDeepArchive),
            "REDUCED_REDUNDANCY" => Some(StorageClass::ReducedRedundancy),
            "EXPRESS_ONEZONE" | "EXPRESS" => Some(StorageClass::Express),
            _ => None,
        }
    }

    /// Returns the canonical string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            StorageClass::Standard => "STANDARD",
            StorageClass::StandardIa => "STANDARD_IA",
            StorageClass::OnezoneIa => "ONEZONE_IA",
            StorageClass::IntelligentTiering => "INTELLIGENT_TIERING",
            StorageClass::Glacier => "GLACIER",
            StorageClass::GlacierDeepArchive => "GLACIER_DEEP_ARCHIVE",
            StorageClass::ReducedRedundancy => "REDUCED_REDUNDANCY",
            StorageClass::Express => "EXPRESS_ONEZONE",
        }
    }

    /// Minimum storage duration in days (early deletion fees apply)
    pub fn min_storage_days(&self) -> u32 {
        match self {
            StorageClass::Standard => 0,
            StorageClass::StandardIa => 30,
            StorageClass::OnezoneIa => 30,
            StorageClass::IntelligentTiering => 0,
            StorageClass::Glacier => 90,
            StorageClass::GlacierDeepArchive => 180,
            StorageClass::ReducedRedundancy => 0,
            StorageClass::Express => 0,
        }
    }

    /// Whether this class supports real-time access (no restore needed)
    pub fn is_realtime(&self) -> bool {
        matches!(
            self,
            StorageClass::Standard
                | StorageClass::StandardIa
                | StorageClass::OnezoneIa
                | StorageClass::IntelligentTiering
                | StorageClass::ReducedRedundancy
                | StorageClass::Express
        )
    }

    /// Whether objects need restore before access
    pub fn requires_restore(&self) -> bool {
        matches!(
            self,
            StorageClass::Glacier | StorageClass::GlacierDeepArchive
        )
    }

    /// Relative cost tier (1=cheapest ... 10=most expensive)
    pub fn cost_tier(&self) -> u8 {
        match self {
            StorageClass::GlacierDeepArchive => 1,
            StorageClass::Glacier => 2,
            StorageClass::OnezoneIa => 3,
            StorageClass::StandardIa => 4,
            StorageClass::ReducedRedundancy => 5,
            StorageClass::IntelligentTiering => 6,
            StorageClass::Standard => 8,
            StorageClass::Express => 10,
        }
    }
}

impl std::fmt::Display for StorageClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Transition rule: move objects to cheaper class after N days
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageClassTransition {
    /// Days after creation before transition
    pub days: u32,
    /// Target storage class
    pub target_class: StorageClass,
}

impl StorageClassTransition {
    /// Creates a new transition rule
    pub fn new(days: u32, target_class: StorageClass) -> Self {
        Self { days, target_class }
    }
}

/// Per-object storage class state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectStorageState {
    /// Current storage class
    pub current_class: StorageClass,
    /// Scheduled transition date
    pub transition_date: Option<std::time::SystemTime>,
    /// Restore expiry time
    pub restore_expiry: Option<std::time::SystemTime>,
    /// Whether restore is in progress
    pub is_restoring: bool,
}

impl ObjectStorageState {
    /// Creates a new storage state
    pub fn new(class: StorageClass) -> Self {
        Self {
            current_class: class,
            transition_date: None,
            restore_expiry: None,
            is_restoring: false,
        }
    }

    /// Whether the object needs to be restored before access
    pub fn needs_restore(&self) -> bool {
        self.current_class.requires_restore() && !self.is_restored()
    }

    /// Whether the object is currently accessible (restored and not expired)
    pub fn is_restored(&self) -> bool {
        if let Some(expiry) = self.restore_expiry {
            // Check if restore has not expired
            std::time::SystemTime::now() < expiry && !self.is_restoring
        } else {
            false
        }
    }

    /// Start a restore operation
    pub fn start_restore(&mut self, duration: std::time::Duration) {
        self.is_restoring = true;
        let expiry = std::time::SystemTime::now() + duration;
        self.restore_expiry = Some(expiry);
    }

    /// Complete a restore operation
    pub fn complete_restore(&mut self) {
        self.is_restoring = false;
    }
}

/// Compute which transition applies given object age in days
/// Returns the target StorageClass if a transition should fire, else None
pub fn evaluate_transitions(
    current_class: &StorageClass,
    age_days: u32,
    transitions: &[StorageClassTransition],
) -> Option<StorageClass> {
    // Only transition from Standard to other classes
    if *current_class != StorageClass::Standard
        && *current_class != StorageClass::IntelligentTiering
    {
        return None;
    }

    // Find the first transition that applies (lowest days >= age_days)
    let mut best: Option<&StorageClassTransition> = None;

    for t in transitions {
        if t.days <= age_days {
            if best.is_none() || t.days > best.unwrap().days {
                best = Some(t);
            }
        }
    }

    best.map(|t| t.target_class)
}

/// Restore request for archived objects
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreRequest {
    /// Object key to restore
    pub object_key: String,
    /// Restore tier (Expedited/Standard/Bulk)
    pub tier: RestoreTier,
    /// How many days to keep the restored copy
    pub days: u32,
}

impl RestoreRequest {
    /// Creates a new restore request
    pub fn new(object_key: String, tier: RestoreTier, days: u32) -> Self {
        Self {
            object_key,
            tier,
            days,
        }
    }
}

/// Restore tier options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RestoreTier {
    /// 1-5 minutes restore (Glacier only)
    Expedited,
    /// 3-5 hours restore
    Standard,
    /// 5-12 hours restore, cheapest
    Bulk,
}

impl RestoreTier {
    /// Returns the tier name
    pub fn as_str(&self) -> &'static str {
        match self {
            RestoreTier::Expedited => "Expedited",
            RestoreTier::Standard => "Standard",
            RestoreTier::Bulk => "Bulk",
        }
    }

    /// Returns the expected restore duration
    pub fn restore_duration(&self) -> std::time::Duration {
        match self {
            RestoreTier::Expedited => std::time::Duration::from_secs(5 * 60), // 5 minutes
            RestoreTier::Standard => std::time::Duration::from_secs(5 * 60 * 60), // 5 hours
            RestoreTier::Bulk => std::time::Duration::from_secs(12 * 60 * 60), // 12 hours
        }
    }
}

/// Storage class errors
#[derive(Debug, Error)]
pub enum StorageClassError {
    #[error("Invalid storage class: {0}")]
    InvalidClass(String),

    #[error("Transition not allowed from {0} to {1}")]
    TransitionNotAllowed(StorageClass, StorageClass),

    #[error("Object requires restore before access")]
    RestoreRequired,

    #[error("Restore already in progress")]
    RestoreInProgress,

    #[error("Invalid transition: {0}")]
    InvalidTransition(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_class_from_str() {
        assert_eq!(
            StorageClass::from_str("STANDARD"),
            Some(StorageClass::Standard)
        );
        assert_eq!(
            StorageClass::from_str("STANDARD_IA"),
            Some(StorageClass::StandardIa)
        );
        assert_eq!(
            StorageClass::from_str("GLACIER"),
            Some(StorageClass::Glacier)
        );
        assert_eq!(StorageClass::from_str("INVALID"), None);
        assert_eq!(
            StorageClass::from_str("EXPRESS"),
            Some(StorageClass::Express)
        );
        assert_eq!(
            StorageClass::from_str("EXPRESS_ONEZONE"),
            Some(StorageClass::Express)
        );
    }

    #[test]
    fn test_storage_class_as_str() {
        assert_eq!(StorageClass::Standard.as_str(), "STANDARD");
        assert_eq!(StorageClass::StandardIa.as_str(), "STANDARD_IA");
        assert_eq!(StorageClass::Glacier.as_str(), "GLACIER");
        assert_eq!(
            StorageClass::GlacierDeepArchive.as_str(),
            "GLACIER_DEEP_ARCHIVE"
        );
    }

    #[test]
    fn test_is_realtime() {
        assert!(StorageClass::Standard.is_realtime());
        assert!(StorageClass::Express.is_realtime());
        assert!(!StorageClass::Glacier.is_realtime());
        assert!(!StorageClass::GlacierDeepArchive.is_realtime());
    }

    #[test]
    fn test_requires_restore() {
        assert!(!StorageClass::Standard.requires_restore());
        assert!(!StorageClass::IntelligentTiering.requires_restore());
        assert!(StorageClass::Glacier.requires_restore());
        assert!(StorageClass::GlacierDeepArchive.requires_restore());
    }

    #[test]
    fn test_min_storage_days() {
        assert_eq!(StorageClass::Standard.min_storage_days(), 0);
        assert_eq!(StorageClass::StandardIa.min_storage_days(), 30);
        assert_eq!(StorageClass::Glacier.min_storage_days(), 90);
        assert_eq!(StorageClass::GlacierDeepArchive.min_storage_days(), 180);
    }

    #[test]
    fn test_cost_tier_ordering() {
        assert!(StorageClass::GlacierDeepArchive.cost_tier() < StorageClass::Standard.cost_tier());
        assert!(StorageClass::Standard.cost_tier() < StorageClass::Express.cost_tier());
        assert!(StorageClass::OnezoneIa.cost_tier() < StorageClass::StandardIa.cost_tier());
    }

    #[test]
    fn test_evaluate_transitions_single_rule() {
        let transitions = vec![StorageClassTransition::new(30, StorageClass::StandardIa)];

        assert_eq!(
            evaluate_transitions(&StorageClass::Standard, 29, &transitions),
            None
        );
        assert_eq!(
            evaluate_transitions(&StorageClass::Standard, 30, &transitions),
            Some(StorageClass::StandardIa)
        );
        assert_eq!(
            evaluate_transitions(&StorageClass::Standard, 100, &transitions),
            Some(StorageClass::StandardIa)
        );
    }

    #[test]
    fn test_evaluate_transitions_multiple_rules() {
        let transitions = vec![
            StorageClassTransition::new(30, StorageClass::StandardIa),
            StorageClassTransition::new(90, StorageClass::Glacier),
            StorageClassTransition::new(180, StorageClass::GlacierDeepArchive),
        ];

        // Age 29: no transition
        assert_eq!(
            evaluate_transitions(&StorageClass::Standard, 29, &transitions),
            None
        );

        // Age 30: first rule applies
        assert_eq!(
            evaluate_transitions(&StorageClass::Standard, 30, &transitions),
            Some(StorageClass::StandardIa)
        );

        // Age 100: second rule applies (closest below 100)
        assert_eq!(
            evaluate_transitions(&StorageClass::Standard, 100, &transitions),
            Some(StorageClass::Glacier)
        );

        // Age 200: third rule applies
        assert_eq!(
            evaluate_transitions(&StorageClass::Standard, 200, &transitions),
            Some(StorageClass::GlacierDeepArchive)
        );
    }

    #[test]
    fn test_no_transition_when_age_insufficient() {
        let transitions = vec![StorageClassTransition::new(60, StorageClass::StandardIa)];

        assert_eq!(
            evaluate_transitions(&StorageClass::Standard, 59, &transitions),
            None
        );
        assert_eq!(
            evaluate_transitions(&StorageClass::Standard, 0, &transitions),
            None
        );
    }

    #[test]
    fn test_no_transition_from_non_standard() {
        let transitions = vec![StorageClassTransition::new(30, StorageClass::StandardIa)];

        assert_eq!(
            evaluate_transitions(&StorageClass::Glacier, 100, &transitions),
            None
        );
        assert_eq!(
            evaluate_transitions(&StorageClass::StandardIa, 100, &transitions),
            None
        );
    }

    #[test]
    fn test_object_storage_state_needs_restore() {
        let mut state = ObjectStorageState::new(StorageClass::Glacier);
        assert!(state.needs_restore());

        state.start_restore(std::time::Duration::from_secs(3600));
        assert!(state.needs_restore()); // still restoring

        state.complete_restore();
        assert!(!state.needs_restore()); // now restored
    }

    #[test]
    fn test_object_storage_state_non_glacier() {
        let state = ObjectStorageState::new(StorageClass::Standard);
        assert!(!state.needs_restore());
        assert!(state.is_restored());
    }

    #[test]
    fn test_restore_tier_duration() {
        assert_eq!(RestoreTier::Expedited.restore_duration().as_secs(), 5 * 60);
        assert_eq!(
            RestoreTier::Standard.restore_duration().as_secs(),
            5 * 60 * 60
        );
        assert_eq!(RestoreTier::Bulk.restore_duration().as_secs(), 12 * 60 * 60);
    }

    #[test]
    fn test_restore_tier_as_str() {
        assert_eq!(RestoreTier::Expedited.as_str(), "Expedited");
        assert_eq!(RestoreTier::Standard.as_str(), "Standard");
        assert_eq!(RestoreTier::Bulk.as_str(), "Bulk");
    }

    #[test]
    fn test_restore_request() {
        let req = RestoreRequest::new("my-object".to_string(), RestoreTier::Standard, 10);
        assert_eq!(req.object_key, "my-object");
        assert!(matches!(req.tier, RestoreTier::Standard));
        assert_eq!(req.days, 10);
    }

    #[test]
    fn test_transition_display() {
        let class = StorageClass::Glacier;
        assert_eq!(format!("{}", class), "GLACIER");
    }

    #[test]
    fn test_intelligent_tiering_transitions() {
        let transitions = vec![StorageClassTransition::new(30, StorageClass::Glacier)];

        // IntelligentTiering can also transition
        assert_eq!(
            evaluate_transitions(&StorageClass::IntelligentTiering, 30, &transitions),
            Some(StorageClass::Glacier)
        );
    }

    #[test]
    fn test_storage_class_equality() {
        let a = StorageClass::Standard;
        let b = StorageClass::from_str("STANDARD").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn test_empty_transitions() {
        let transitions: Vec<StorageClassTransition> = vec![];

        assert_eq!(
            evaluate_transitions(&StorageClass::Standard, 100, &transitions),
            None
        );
    }
}
