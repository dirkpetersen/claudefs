//! WORM and Immutability Support
//!
//! WORM (Write Once Read Many) support for compliance use cases.
//!
//! This module implements immutability modes for files, including:
//! - Append-only mode where data can only be appended
//! - Immutable mode where no modifications are allowed
//! - Time-based retention with expiration
//! - Legal holds for litigation or compliance requirements

use std::collections::HashMap;
use thiserror::Error;

/// Immutability mode for a file or directory.
///
/// Controls what operations are permitted on an inode based on
/// compliance and retention requirements.
#[derive(Debug, Clone, PartialEq)]
pub enum ImmutabilityMode {
    /// No restrictions; normal read-write behavior.
    None,
    /// Append-only; data can be appended but not overwritten or truncated.
    AppendOnly,
    /// Fully immutable; no modifications, deletes, or renames permitted.
    Immutable,
    /// WORM retention with time-based expiration.
    ///
    /// The file is protected until `retention_expires_at_secs` (Unix timestamp).
    /// After expiration, restrictions are lifted.
    WormRetention {
        /// Unix timestamp (seconds) when retention expires.
        retention_expires_at_secs: u64,
    },
    /// Legal hold for litigation or compliance.
    ///
    /// Files under legal hold cannot be modified or deleted until the hold is lifted.
    LegalHold {
        /// Unique identifier for the legal hold.
        hold_id: String,
    },
}

impl ImmutabilityMode {
    /// Returns `true` if write operations are blocked at the given time.
    ///
    /// For `WormRetention`, writes are blocked until the retention expires.
    /// For `LegalHold`, `Immutable`, and `AppendOnly`, writes are always blocked.
    pub fn is_write_blocked(&self, now_secs: u64) -> bool {
        match self {
            ImmutabilityMode::None => false,
            ImmutabilityMode::AppendOnly => true,
            ImmutabilityMode::Immutable => true,
            ImmutabilityMode::WormRetention {
                retention_expires_at_secs,
            } => now_secs < *retention_expires_at_secs,
            ImmutabilityMode::LegalHold { .. } => true,
        }
    }

    /// Returns `true` if delete operations are blocked at the given time.
    ///
    /// Delete blocking follows the same rules as write blocking.
    pub fn is_delete_blocked(&self, now_secs: u64) -> bool {
        self.is_write_blocked(now_secs)
    }

    /// Returns `true` if rename operations are blocked at the given time.
    ///
    /// Renames are blocked for all non-`None` modes when retention is active.
    pub fn is_rename_blocked(&self, now_secs: u64) -> bool {
        match self {
            ImmutabilityMode::None => false,
            ImmutabilityMode::AppendOnly => true,
            ImmutabilityMode::Immutable => true,
            ImmutabilityMode::WormRetention {
                retention_expires_at_secs,
            } => now_secs < *retention_expires_at_secs,
            ImmutabilityMode::LegalHold { .. } => true,
        }
    }

    /// Returns `true` if truncate operations are blocked at the given time.
    ///
    /// Truncate blocking follows the same rules as write blocking.
    pub fn is_truncate_blocked(&self, now_secs: u64) -> bool {
        self.is_write_blocked(now_secs)
    }

    /// Returns `true` if append operations are allowed at the given time.
    ///
    /// Only `None` and `AppendOnly` modes permit appends.
    pub fn is_append_allowed(&self, _now_secs: u64) -> bool {
        match self {
            ImmutabilityMode::None => true,
            ImmutabilityMode::AppendOnly => true,
            ImmutabilityMode::Immutable => false,
            ImmutabilityMode::WormRetention { .. } => false,
            ImmutabilityMode::LegalHold { .. } => false,
        }
    }

    /// Returns the remaining retention time in seconds, if applicable.
    ///
    /// Returns `Some(seconds)` for `WormRetention` mode, `None` otherwise.
    /// Uses saturating subtraction to return 0 if already expired.
    pub fn retention_remaining_secs(&self, now_secs: u64) -> Option<u64> {
        match self {
            ImmutabilityMode::WormRetention {
                retention_expires_at_secs,
            } => {
                let remaining = retention_expires_at_secs.saturating_sub(now_secs);
                Some(remaining)
            }
            _ => None,
        }
    }
}

/// Record tracking WORM state for a single inode.
#[derive(Debug, Clone)]
pub struct WormRecord {
    /// Inode number this record applies to.
    pub ino: u64,
    /// The immutability mode in effect.
    pub mode: ImmutabilityMode,
    /// Unix timestamp (seconds) when this mode was set.
    pub set_at_secs: u64,
    /// UID of the user who set this mode.
    pub set_by_uid: u32,
}

impl WormRecord {
    /// Creates a new WORM record for the given inode.
    pub fn new(ino: u64, mode: ImmutabilityMode, now_secs: u64, uid: u32) -> Self {
        Self {
            ino,
            mode,
            set_at_secs: now_secs,
            set_by_uid: uid,
        }
    }

    /// Checks if a write operation is permitted.
    ///
    /// Returns `Err(WormViolation)` if the operation would violate immutability.
    pub fn check_write(&self, now_secs: u64) -> std::result::Result<(), WormViolation> {
        if self.mode.is_write_blocked(now_secs) {
            match &self.mode {
                ImmutabilityMode::Immutable => Err(WormViolation::Immutable),
                ImmutabilityMode::AppendOnly => Err(WormViolation::AppendOnly),
                ImmutabilityMode::WormRetention {
                    retention_expires_at_secs,
                } => Err(WormViolation::RetentionActive(*retention_expires_at_secs)),
                ImmutabilityMode::LegalHold { hold_id } => {
                    Err(WormViolation::LegalHold(hold_id.clone()))
                }
                ImmutabilityMode::None => Ok(()),
            }
        } else {
            Ok(())
        }
    }

    /// Checks if a delete operation is permitted.
    ///
    /// Returns `Err(WormViolation)` if the operation would violate immutability.
    pub fn check_delete(&self, now_secs: u64) -> std::result::Result<(), WormViolation> {
        if self.mode.is_delete_blocked(now_secs) {
            match &self.mode {
                ImmutabilityMode::Immutable => Err(WormViolation::Immutable),
                ImmutabilityMode::AppendOnly => Err(WormViolation::AppendOnly),
                ImmutabilityMode::WormRetention {
                    retention_expires_at_secs,
                } => Err(WormViolation::RetentionActive(*retention_expires_at_secs)),
                ImmutabilityMode::LegalHold { hold_id } => {
                    Err(WormViolation::LegalHold(hold_id.clone()))
                }
                ImmutabilityMode::None => Ok(()),
            }
        } else {
            Ok(())
        }
    }

    /// Checks if a rename operation is permitted.
    ///
    /// Returns `Err(WormViolation)` if the operation would violate immutability.
    pub fn check_rename(&self, now_secs: u64) -> std::result::Result<(), WormViolation> {
        if self.mode.is_rename_blocked(now_secs) {
            match &self.mode {
                ImmutabilityMode::Immutable => Err(WormViolation::Immutable),
                ImmutabilityMode::AppendOnly => Err(WormViolation::AppendOnly),
                ImmutabilityMode::WormRetention {
                    retention_expires_at_secs,
                } => Err(WormViolation::RetentionActive(*retention_expires_at_secs)),
                ImmutabilityMode::LegalHold { hold_id } => {
                    Err(WormViolation::LegalHold(hold_id.clone()))
                }
                ImmutabilityMode::None => Ok(()),
            }
        } else {
            Ok(())
        }
    }

    /// Checks if a truncate operation is permitted.
    ///
    /// Returns `Err(WormViolation)` if the operation would violate immutability.
    pub fn check_truncate(&self, now_secs: u64) -> std::result::Result<(), WormViolation> {
        if self.mode.is_truncate_blocked(now_secs) {
            match &self.mode {
                ImmutabilityMode::Immutable => Err(WormViolation::Immutable),
                ImmutabilityMode::AppendOnly => Err(WormViolation::AppendOnly),
                ImmutabilityMode::WormRetention {
                    retention_expires_at_secs,
                } => Err(WormViolation::RetentionActive(*retention_expires_at_secs)),
                ImmutabilityMode::LegalHold { hold_id } => {
                    Err(WormViolation::LegalHold(hold_id.clone()))
                }
                ImmutabilityMode::None => Ok(()),
            }
        } else {
            Ok(())
        }
    }
}

/// Error returned when an operation violates WORM constraints.
#[derive(Debug, Error, Clone)]
pub enum WormViolation {
    /// The file is fully immutable.
    #[error("File is immutable")]
    Immutable,
    /// The file is append-only; writes and truncates are blocked.
    #[error("File is append-only")]
    AppendOnly,
    /// The file is under active WORM retention until the given timestamp.
    #[error("File under WORM retention until {0} secs")]
    RetentionActive(u64),
    /// The file is under a legal hold with the given identifier.
    #[error("File under legal hold: {0}")]
    LegalHold(String),
}

/// Registry tracking WORM state for all inodes.
///
/// Maintains records for inodes with immutability constraints,
/// including support for legal holds across multiple files.
pub struct WormRegistry {
    records: HashMap<u64, WormRecord>,
    legal_holds: HashMap<String, Vec<u64>>,
    pre_hold_modes: HashMap<u64, ImmutabilityMode>,
}

impl Default for WormRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl WormRegistry {
    /// Creates an empty WORM registry.
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
            legal_holds: HashMap::new(),
            pre_hold_modes: HashMap::new(),
        }
    }

    /// Sets or updates the immutability mode for an inode.
    pub fn set_mode(&mut self, ino: u64, mode: ImmutabilityMode, now_secs: u64, uid: u32) {
        self.records
            .insert(ino, WormRecord::new(ino, mode, now_secs, uid));
    }

    /// Returns the WORM record for the given inode, if any.
    pub fn get(&self, ino: u64) -> Option<&WormRecord> {
        self.records.get(&ino)
    }

    /// Removes the WORM record for the given inode.
    ///
    /// Returns the removed record, if any.
    pub fn clear(&mut self, ino: u64) -> Option<WormRecord> {
        self.records.remove(&ino)
    }

    /// Returns the number of inodes with WORM records.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Returns `true` if there are no WORM records.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Checks if a write operation is permitted for the given inode.
    ///
    /// Returns `Ok(())` if no record exists or the operation is permitted.
    pub fn check_write(&self, ino: u64, now_secs: u64) -> std::result::Result<(), WormViolation> {
        if let Some(record) = self.records.get(&ino) {
            record.check_write(now_secs)
        } else {
            Ok(())
        }
    }

    /// Checks if a delete operation is permitted for the given inode.
    ///
    /// Returns `Ok(())` if no record exists or the operation is permitted.
    pub fn check_delete(&self, ino: u64, now_secs: u64) -> std::result::Result<(), WormViolation> {
        if let Some(record) = self.records.get(&ino) {
            record.check_delete(now_secs)
        } else {
            Ok(())
        }
    }

    /// Checks if a rename operation is permitted for the given inode.
    ///
    /// Returns `Ok(())` if no record exists or the operation is permitted.
    pub fn check_rename(&self, ino: u64, now_secs: u64) -> std::result::Result<(), WormViolation> {
        if let Some(record) = self.records.get(&ino) {
            record.check_rename(now_secs)
        } else {
            Ok(())
        }
    }

    /// Checks if a truncate operation is permitted for the given inode.
    ///
    /// Returns `Ok(())` if no record exists or the operation is permitted.
    pub fn check_truncate(
        &self,
        ino: u64,
        now_secs: u64,
    ) -> std::result::Result<(), WormViolation> {
        if let Some(record) = self.records.get(&ino) {
            record.check_truncate(now_secs)
        } else {
            Ok(())
        }
    }

    /// Places a legal hold on multiple inodes.
    ///
    /// Saves the previous mode for each inode so it can be restored
    /// when the hold is lifted.
    pub fn place_legal_hold(&mut self, hold_id: &str, inos: Vec<u64>, now_secs: u64, uid: u32) {
        for ino in &inos {
            if let Some(record) = self.records.get(ino) {
                self.pre_hold_modes.insert(*ino, record.mode.clone());
            } else {
                self.pre_hold_modes.insert(*ino, ImmutabilityMode::None);
            }
        }

        let hold_inos = inos.clone();
        for ino in inos {
            self.records.insert(
                ino,
                WormRecord::new(
                    ino,
                    ImmutabilityMode::LegalHold {
                        hold_id: hold_id.to_string(),
                    },
                    now_secs,
                    uid,
                ),
            );
        }
        self.legal_holds.insert(hold_id.to_string(), hold_inos);
    }

    /// Lifts a legal hold and restores previous immutability modes.
    ///
    /// Returns the list of inodes that were under this hold.
    pub fn lift_legal_hold(&mut self, hold_id: &str) -> Vec<u64> {
        if let Some(inos) = self.legal_holds.remove(hold_id) {
            for ino in &inos {
                if let Some(pre_mode) = self.pre_hold_modes.remove(ino) {
                    if let Some(record) = self.records.get_mut(ino) {
                        record.mode = pre_mode;
                    }
                } else {
                    self.records.remove(ino);
                }
            }
            inos
        } else {
            Vec::new()
        }
    }

    /// Returns the total number of inodes under legal holds.
    pub fn legal_hold_count(&self) -> usize {
        self.legal_holds.values().map(|v| v.len()).sum()
    }

    /// Returns inodes with expired WORM retention.
    ///
    /// Useful for background cleanup to remove expired retention records.
    pub fn expired_retention(&self, now_secs: u64) -> Vec<u64> {
        self.records
            .iter()
            .filter(|(_, record)| {
                if let ImmutabilityMode::WormRetention {
                    retention_expires_at_secs: expiry,
                } = &record.mode
                {
                    now_secs >= *expiry
                } else {
                    false
                }
            })
            .map(|(&ino, _)| ino)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_none_allows_operations() {
        let mode = ImmutabilityMode::None;
        assert!(!mode.is_write_blocked(1000));
        assert!(!mode.is_delete_blocked(1000));
        assert!(!mode.is_rename_blocked(1000));
        assert!(!mode.is_truncate_blocked(1000));
        assert!(mode.is_append_allowed(1000));
    }

    #[test]
    fn test_immutable_blocks_operations() {
        let mode = ImmutabilityMode::Immutable;
        assert!(mode.is_write_blocked(1000));
        assert!(mode.is_delete_blocked(1000));
        assert!(mode.is_rename_blocked(1000));
        assert!(mode.is_truncate_blocked(1000));
        assert!(!mode.is_append_allowed(1000));
    }

    #[test]
    fn test_append_only_blocks() {
        let mode = ImmutabilityMode::AppendOnly;
        assert!(mode.is_write_blocked(1000));
        assert!(mode.is_delete_blocked(1000));
        assert!(mode.is_rename_blocked(1000));
        assert!(mode.is_truncate_blocked(1000));
        assert!(mode.is_append_allowed(1000));
    }

    #[test]
    fn test_worm_retention_active() {
        let mode = ImmutabilityMode::WormRetention {
            retention_expires_at_secs: 5000,
        };
        assert!(mode.is_write_blocked(1000));
        assert!(mode.is_write_blocked(4999));
        assert!(!mode.is_write_blocked(5000));
    }

    #[test]
    fn test_worm_retention_blocks_delete() {
        let mode = ImmutabilityMode::WormRetention {
            retention_expires_at_secs: 5000,
        };
        assert!(mode.is_delete_blocked(4000));
        assert!(!mode.is_delete_blocked(5000));
    }

    #[test]
    fn test_legal_hold_blocks() {
        let mode = ImmutabilityMode::LegalHold {
            hold_id: "hold123".to_string(),
        };
        assert!(mode.is_write_blocked(0));
        assert!(mode.is_delete_blocked(0));
        assert!(mode.is_rename_blocked(0));
    }

    #[test]
    fn test_retention_remaining_positive() {
        let mode = ImmutabilityMode::WormRetention {
            retention_expires_at_secs: 5000,
        };
        let remaining = mode.retention_remaining_secs(1000);
        assert_eq!(remaining, Some(4000));
    }

    #[test]
    fn test_retention_remaining_zero() {
        let mode = ImmutabilityMode::WormRetention {
            retention_expires_at_secs: 5000,
        };
        let remaining = mode.retention_remaining_secs(5000);
        assert_eq!(remaining, Some(0));
    }

    #[test]
    fn test_retention_remaining_negative() {
        let mode = ImmutabilityMode::WormRetention {
            retention_expires_at_secs: 3000,
        };
        let remaining = mode.retention_remaining_secs(5000);
        assert_eq!(remaining, Some(0));
    }

    #[test]
    fn test_retention_remaining_none_for_non_retention() {
        let mode = ImmutabilityMode::Immutable;
        assert_eq!(mode.retention_remaining_secs(1000), None);
    }

    #[test]
    fn test_worm_record_check_write() {
        let record = WormRecord::new(1, ImmutabilityMode::Immutable, 1000, 0);
        let result = record.check_write(1500);
        assert!(matches!(result, Err(WormViolation::Immutable)));
    }

    #[test]
    fn test_worm_record_check_write_allows() {
        let record = WormRecord::new(1, ImmutabilityMode::None, 1000, 0);
        let result = record.check_write(1500);
        assert!(result.is_ok());
    }

    #[test]
    fn test_worm_record_check_delete() {
        let record = WormRecord::new(1, ImmutabilityMode::Immutable, 1000, 0);
        let result = record.check_delete(1500);
        assert!(matches!(result, Err(WormViolation::Immutable)));
    }

    #[test]
    fn test_worm_record_check_truncate() {
        let record = WormRecord::new(1, ImmutabilityMode::AppendOnly, 1000, 0);
        let result = record.check_truncate(1500);
        assert!(matches!(result, Err(WormViolation::AppendOnly)));
    }

    #[test]
    fn test_worm_registry_set_get() {
        let mut registry = WormRegistry::new();
        registry.set_mode(1, ImmutabilityMode::Immutable, 1000, 100);

        let record = registry.get(1).unwrap();
        assert!(matches!(record.mode, ImmutabilityMode::Immutable));
    }

    #[test]
    fn test_worm_registry_clear() {
        let mut registry = WormRegistry::new();
        registry.set_mode(1, ImmutabilityMode::Immutable, 1000, 100);

        let removed = registry.clear(1);
        assert!(removed.is_some());
        assert!(registry.get(1).is_none());
    }

    #[test]
    fn test_check_on_unregistered_ino() {
        let registry = WormRegistry::new();
        let result = registry.check_write(999, 1000);
        assert!(result.is_ok());
    }

    #[test]
    fn test_place_legal_hold() {
        let mut registry = WormRegistry::new();
        registry.set_mode(1, ImmutabilityMode::None, 1000, 100);
        registry.set_mode(2, ImmutabilityMode::Immutable, 1000, 100);

        registry.place_legal_hold("hold1", vec![1, 2], 2000, 100);

        assert_eq!(registry.legal_hold_count(), 2);

        let r1 = registry.get(1).unwrap();
        assert!(matches!(r1.mode, ImmutabilityMode::LegalHold { .. }));
    }

    #[test]
    fn test_lift_legal_hold() {
        let mut registry = WormRegistry::new();
        registry.set_mode(1, ImmutabilityMode::None, 1000, 100);

        registry.place_legal_hold("hold1", vec![1], 2000, 100);
        let lifted = registry.lift_legal_hold("hold1");

        assert_eq!(lifted.len(), 1);

        let r1 = registry.get(1).unwrap();
        assert!(matches!(r1.mode, ImmutabilityMode::None));
    }

    #[test]
    fn test_lift_legal_hold_restores_pre_hold_mode() {
        let mut registry = WormRegistry::new();
        registry.set_mode(1, ImmutabilityMode::Immutable, 1000, 100);

        registry.place_legal_hold("hold1", vec![1], 2000, 100);
        registry.lift_legal_hold("hold1");

        let r1 = registry.get(1).unwrap();
        assert!(matches!(r1.mode, ImmutabilityMode::Immutable));
    }

    #[test]
    fn test_expired_retention() {
        let mut registry = WormRegistry::new();
        registry.set_mode(
            1,
            ImmutabilityMode::WormRetention {
                retention_expires_at_secs: 5000,
            },
            1000,
            100,
        );
        registry.set_mode(
            2,
            ImmutabilityMode::WormRetention {
                retention_expires_at_secs: 10000,
            },
            1000,
            100,
        );
        registry.set_mode(3, ImmutabilityMode::Immutable, 1000, 100);

        let expired = registry.expired_retention(8000);
        assert!(expired.contains(&1));
        assert!(!expired.contains(&2));
        assert!(!expired.contains(&3));
    }

    #[test]
    fn test_worm_registry_len() {
        let mut registry = WormRegistry::new();
        assert_eq!(registry.len(), 0);

        registry.set_mode(1, ImmutabilityMode::Immutable, 1000, 100);
        assert_eq!(registry.len(), 1);

        registry.set_mode(2, ImmutabilityMode::AppendOnly, 1000, 100);
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn test_check_write_retention_violation() {
        let mut registry = WormRegistry::new();
        registry.set_mode(
            1,
            ImmutabilityMode::WormRetention {
                retention_expires_at_secs: 5000,
            },
            1000,
            100,
        );

        let result = registry.check_write(1, 3000);
        assert!(matches!(result, Err(WormViolation::RetentionActive(5000))));
    }

    #[test]
    fn test_check_delete_legal_hold() {
        let mut registry = WormRegistry::new();
        registry.set_mode(
            1,
            ImmutabilityMode::LegalHold {
                hold_id: "LIT-001".to_string(),
            },
            1000,
            100,
        );

        let result = registry.check_delete(1, 10000);
        assert!(matches!(result, Err(WormViolation::LegalHold(h)) if h == "LIT-001"));
    }

    #[test]
    fn test_append_allowed_mode() {
        let mut registry = WormRegistry::new();
        registry.set_mode(1, ImmutabilityMode::AppendOnly, 1000, 100);

        let mode = registry.get(1).unwrap().mode.clone();
        assert!(mode.is_append_allowed(2000));
    }
}
