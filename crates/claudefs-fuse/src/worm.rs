//! WORM and Immutability Support
//!
//! WORM (Write Once Read Many) support for compliance use cases.

use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum ImmutabilityMode {
    None,
    AppendOnly,
    Immutable,
    WormRetention { retention_expires_at_secs: u64 },
    LegalHold { hold_id: String },
}

impl ImmutabilityMode {
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

    pub fn is_delete_blocked(&self, now_secs: u64) -> bool {
        self.is_write_blocked(now_secs)
    }

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

    pub fn is_truncate_blocked(&self, now_secs: u64) -> bool {
        self.is_write_blocked(now_secs)
    }

    pub fn is_append_allowed(&self, now_secs: u64) -> bool {
        match self {
            ImmutabilityMode::None => true,
            ImmutabilityMode::AppendOnly => true,
            ImmutabilityMode::Immutable => false,
            ImmutabilityMode::WormRetention { .. } => false,
            ImmutabilityMode::LegalHold { .. } => false,
        }
    }

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

#[derive(Debug, Clone)]
pub struct WormRecord {
    pub ino: u64,
    pub mode: ImmutabilityMode,
    pub set_at_secs: u64,
    pub set_by_uid: u32,
}

impl WormRecord {
    pub fn new(ino: u64, mode: ImmutabilityMode, now_secs: u64, uid: u32) -> Self {
        Self {
            ino,
            mode,
            set_at_secs: now_secs,
            set_by_uid: uid,
        }
    }

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

#[derive(Debug, Error, Clone)]
pub enum WormViolation {
    #[error("File is immutable")]
    Immutable,
    #[error("File is append-only")]
    AppendOnly,
    #[error("File under WORM retention until {0} secs")]
    RetentionActive(u64),
    #[error("File under legal hold: {0}")]
    LegalHold(String),
}

pub struct WormRegistry {
    records: HashMap<u64, WormRecord>,
    legal_holds: HashMap<String, Vec<u64>>,
    pre_hold_modes: HashMap<u64, ImmutabilityMode>,
}

impl WormRegistry {
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
            legal_holds: HashMap::new(),
            pre_hold_modes: HashMap::new(),
        }
    }

    pub fn set_mode(&mut self, ino: u64, mode: ImmutabilityMode, now_secs: u64, uid: u32) {
        self.records
            .insert(ino, WormRecord::new(ino, mode, now_secs, uid));
    }

    pub fn get(&self, ino: u64) -> Option<&WormRecord> {
        self.records.get(&ino)
    }

    pub fn clear(&mut self, ino: u64) -> Option<WormRecord> {
        self.records.remove(&ino)
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn check_write(&self, ino: u64, now_secs: u64) -> std::result::Result<(), WormViolation> {
        if let Some(record) = self.records.get(&ino) {
            record.check_write(now_secs)
        } else {
            Ok(())
        }
    }

    pub fn check_delete(&self, ino: u64, now_secs: u64) -> std::result::Result<(), WormViolation> {
        if let Some(record) = self.records.get(&ino) {
            record.check_delete(now_secs)
        } else {
            Ok(())
        }
    }

    pub fn check_rename(&self, ino: u64, now_secs: u64) -> std::result::Result<(), WormViolation> {
        if let Some(record) = self.records.get(&ino) {
            record.check_rename(now_secs)
        } else {
            Ok(())
        }
    }

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

    pub fn place_legal_hold(&mut self, hold_id: &str, inos: Vec<u64>, now_secs: u64, uid: u32) {
        for ino in &inos {
            if let Some(record) = self.records.get(ino) {
                self.pre_hold_modes.insert(*ino, record.mode.clone());
            } else {
                self.pre_hold_modes.insert(*ino, ImmutabilityMode::None);
            }
        }

        let mut hold_inos = inos.clone();
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

    pub fn legal_hold_count(&self) -> usize {
        self.legal_holds.values().map(|v| v.len()).sum()
    }

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
