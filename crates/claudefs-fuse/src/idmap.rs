//! ID mapping for FUSE mounts.
//!
//! Provides UID/GID translation between host and local namespaces,
//! supporting multiple mapping strategies including identity, squash,
//! range shifting, and explicit table-based mappings.

use crate::error::Result;
use std::collections::HashMap;

/// Mapping mode for UID/GID translation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdMapMode {
    /// No translation; host IDs pass through unchanged.
    Identity,
    /// All IDs map to a single "nobody" user/group.
    Squash {
        /// UID to map all users to.
        nobody_uid: u32,
        /// GID to map all groups to.
        nobody_gid: u32,
    },
    /// Linear shift of a contiguous ID range.
    RangeShift {
        /// Start of the host ID range.
        host_base: u32,
        /// Start of the local ID range.
        local_base: u32,
        /// Number of IDs in the range.
        count: u32,
    },
    /// Explicit host-to-local mapping table.
    Table,
}

/// Single entry in an ID mapping table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdMapEntry {
    /// ID on the host system.
    pub host_id: u32,
    /// Corresponding ID in the local namespace.
    pub local_id: u32,
}

/// UID/GID mapper for FUSE namespace translation.
pub struct IdMapper {
    mode: IdMapMode,
    uid_table: HashMap<u32, u32>,
    gid_table: HashMap<u32, u32>,
}

impl IdMapper {
    /// Creates a new mapper with the specified mode.
    pub fn new(mode: IdMapMode) -> Self {
        Self {
            mode,
            uid_table: HashMap::new(),
            gid_table: HashMap::new(),
        }
    }

    /// Adds a UID mapping entry (Table mode only).
    ///
    /// Returns an error if not in Table mode, the table is full,
    /// or the host_id already exists.
    pub fn add_uid_entry(&mut self, entry: IdMapEntry) -> Result<()> {
        if self.mode != IdMapMode::Table {
            return Err(crate::error::FuseError::InvalidArgument {
                msg: "add_uid_entry only supported in Table mode".to_string(),
            });
        }

        if self.uid_table.len() >= 65_535 {
            return Err(crate::error::FuseError::InvalidArgument {
                msg: "max UID entries exceeded (65535)".to_string(),
            });
        }

        if self.uid_table.contains_key(&entry.host_id) {
            return Err(crate::error::FuseError::AlreadyExists {
                name: format!("duplicate host_id {} in UID table", entry.host_id),
            });
        }

        self.uid_table.insert(entry.host_id, entry.local_id);
        Ok(())
    }

    /// Adds a GID mapping entry (Table mode only).
    ///
    /// Returns an error if not in Table mode, the table is full,
    /// or the host_id already exists.
    pub fn add_gid_entry(&mut self, entry: IdMapEntry) -> Result<()> {
        if self.mode != IdMapMode::Table {
            return Err(crate::error::FuseError::InvalidArgument {
                msg: "add_gid_entry only supported in Table mode".to_string(),
            });
        }

        if self.gid_table.len() >= 65_535 {
            return Err(crate::error::FuseError::InvalidArgument {
                msg: "max GID entries exceeded (65535)".to_string(),
            });
        }

        if self.gid_table.contains_key(&entry.host_id) {
            return Err(crate::error::FuseError::AlreadyExists {
                name: format!("duplicate host_id {} in GID table", entry.host_id),
            });
        }

        self.gid_table.insert(entry.host_id, entry.local_id);
        Ok(())
    }

    /// Maps a host UID to a local UID.
    ///
    /// Root (UID 0) is always preserved except in Squash mode.
    pub fn map_uid(&self, host_uid: u32) -> u32 {
        match &self.mode {
            IdMapMode::Identity => {
                if host_uid == 0 {
                    return 0;
                }
                host_uid
            }
            IdMapMode::Squash { nobody_uid, .. } => *nobody_uid,
            IdMapMode::RangeShift {
                host_base,
                local_base,
                count,
            } => {
                if host_uid == 0 {
                    return 0;
                }
                if host_uid >= *host_base && host_uid < host_base + count {
                    local_base + (host_uid - host_base)
                } else {
                    host_uid
                }
            }
            IdMapMode::Table => self.uid_table.get(&host_uid).copied().unwrap_or(host_uid),
        }
    }

    /// Maps a host GID to a local GID.
    ///
    /// Root (GID 0) is always preserved except in Squash mode.
    pub fn map_gid(&self, host_gid: u32) -> u32 {
        match &self.mode {
            IdMapMode::Identity => {
                if host_gid == 0 {
                    return 0;
                }
                host_gid
            }
            IdMapMode::Squash { nobody_gid, .. } => *nobody_gid,
            IdMapMode::RangeShift {
                host_base,
                local_base,
                count,
            } => {
                if host_gid == 0 {
                    return 0;
                }
                if host_gid >= *host_base && host_gid < host_base + count {
                    local_base + (host_gid - host_base)
                } else {
                    host_gid
                }
            }
            IdMapMode::Table => self.gid_table.get(&host_gid).copied().unwrap_or(host_gid),
        }
    }

    /// Reverse-maps a local UID back to the host UID (Table mode only).
    ///
    /// Returns `None` if not in Table mode or no mapping exists.
    pub fn reverse_map_uid(&self, local_uid: u32) -> Option<u32> {
        if !matches!(self.mode, IdMapMode::Table) {
            return None;
        }
        self.uid_table
            .iter()
            .find(|(_, &local)| local == local_uid)
            .map(|(&host, _)| host)
    }

    /// Reverse-maps a local GID back to the host GID (Table mode only).
    ///
    /// Returns `None` if not in Table mode or no mapping exists.
    pub fn reverse_map_gid(&self, local_gid: u32) -> Option<u32> {
        if !matches!(self.mode, IdMapMode::Table) {
            return None;
        }
        self.gid_table
            .iter()
            .find(|(_, &local)| local == local_gid)
            .map(|(&host, _)| host)
    }

    /// Returns the number of UID mapping entries.
    pub fn uid_entry_count(&self) -> usize {
        self.uid_table.len()
    }

    /// Returns the number of GID mapping entries.
    pub fn gid_entry_count(&self) -> usize {
        self.gid_table.len()
    }

    /// Returns a reference to the current mapping mode.
    pub fn mode(&self) -> &IdMapMode {
        &self.mode
    }
}

/// Statistics for ID mapping operations.
#[derive(Debug, Clone, Copy, Default)]
pub struct IdMapStats {
    /// Total UID lookup attempts.
    pub uid_lookups: u64,
    /// Total GID lookup attempts.
    pub gid_lookups: u64,
    /// UID lookups that found a mapping.
    pub uid_hits: u64,
    /// GID lookups that found a mapping.
    pub gid_hits: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity_passthrough() {
        let mapper = IdMapper::new(IdMapMode::Identity);
        assert_eq!(mapper.map_uid(100), 100);
        assert_eq!(mapper.map_gid(200), 200);
    }

    #[test]
    fn test_identity_root_preservation() {
        let mapper = IdMapper::new(IdMapMode::Identity);
        assert_eq!(mapper.map_uid(0), 0);
        assert_eq!(mapper.map_gid(0), 0);
    }

    #[test]
    fn test_squash_maps_all_uids() {
        let mapper = IdMapper::new(IdMapMode::Squash {
            nobody_uid: 65534,
            nobody_gid: 65534,
        });
        assert_eq!(mapper.map_uid(0), 65534);
        assert_eq!(mapper.map_uid(100), 65534);
        assert_eq!(mapper.map_uid(1000), 65534);
    }

    #[test]
    fn test_squash_maps_all_gids() {
        let mapper = IdMapper::new(IdMapMode::Squash {
            nobody_uid: 65534,
            nobody_gid: 65534,
        });
        assert_eq!(mapper.map_gid(0), 65534);
        assert_eq!(mapper.map_gid(100), 65534);
    }

    #[test]
    fn test_rangeshift_in_range() {
        let mapper = IdMapper::new(IdMapMode::RangeShift {
            host_base: 1000,
            local_base: 2000,
            count: 100,
        });
        assert_eq!(mapper.map_uid(1000), 2000);
        assert_eq!(mapper.map_uid(1050), 2050);
        assert_eq!(mapper.map_uid(1099), 2099);
    }

    #[test]
    fn test_rangeshift_out_of_range() {
        let mapper = IdMapper::new(IdMapMode::RangeShift {
            host_base: 1000,
            local_base: 2000,
            count: 100,
        });
        assert_eq!(mapper.map_uid(999), 999);
        assert_eq!(mapper.map_uid(1100), 1100);
        assert_eq!(mapper.map_uid(0), 0);
    }

    #[test]
    fn test_rangeshift_root_preservation() {
        let mapper = IdMapper::new(IdMapMode::RangeShift {
            host_base: 1000,
            local_base: 2000,
            count: 100,
        });
        assert_eq!(mapper.map_uid(0), 0);
        assert_eq!(mapper.map_gid(0), 0);
    }

    #[test]
    fn test_table_mode_hit() {
        let mut mapper = IdMapper::new(IdMapMode::Table);
        mapper
            .add_uid_entry(IdMapEntry {
                host_id: 1000,
                local_id: 2000,
            })
            .unwrap();
        mapper
            .add_gid_entry(IdMapEntry {
                host_id: 500,
                local_id: 600,
            })
            .unwrap();
        assert_eq!(mapper.map_uid(1000), 2000);
        assert_eq!(mapper.map_gid(500), 600);
    }

    #[test]
    fn test_table_mode_miss() {
        let mut mapper = IdMapper::new(IdMapMode::Table);
        mapper
            .add_uid_entry(IdMapEntry {
                host_id: 1000,
                local_id: 2000,
            })
            .unwrap();
        assert_eq!(mapper.map_uid(999), 999);
    }

    #[test]
    fn test_reverse_map_uid() {
        let mut mapper = IdMapper::new(IdMapMode::Table);
        mapper
            .add_uid_entry(IdMapEntry {
                host_id: 1000,
                local_id: 2000,
            })
            .unwrap();
        assert_eq!(mapper.reverse_map_uid(2000), Some(1000));
        assert_eq!(mapper.reverse_map_uid(999), None);
    }

    #[test]
    fn test_reverse_map_gid() {
        let mut mapper = IdMapper::new(IdMapMode::Table);
        mapper
            .add_gid_entry(IdMapEntry {
                host_id: 500,
                local_id: 600,
            })
            .unwrap();
        assert_eq!(mapper.reverse_map_gid(600), Some(500));
        assert_eq!(mapper.reverse_map_gid(999), None);
    }

    #[test]
    fn test_add_entry_duplicate() {
        let mut mapper = IdMapper::new(IdMapMode::Table);
        mapper
            .add_uid_entry(IdMapEntry {
                host_id: 1000,
                local_id: 2000,
            })
            .unwrap();
        let result = mapper.add_uid_entry(IdMapEntry {
            host_id: 1000,
            local_id: 3000,
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_max_entries_limit() {
        let mut mapper = IdMapper::new(IdMapMode::Table);
        for i in 0..65536 {
            let result = mapper.add_uid_entry(IdMapEntry {
                host_id: i,
                local_id: i,
            });
            if i < 65535 {
                assert!(result.is_ok());
            } else {
                assert!(result.is_err());
            }
        }
    }

    #[test]
    fn test_gid_mirrors_uid() {
        let mut mapper = IdMapper::new(IdMapMode::Table);
        mapper
            .add_uid_entry(IdMapEntry {
                host_id: 100,
                local_id: 200,
            })
            .unwrap();
        mapper
            .add_gid_entry(IdMapEntry {
                host_id: 300,
                local_id: 400,
            })
            .unwrap();

        assert_eq!(mapper.uid_entry_count(), 1);
        assert_eq!(mapper.gid_entry_count(), 1);

        assert_eq!(mapper.map_uid(100), 200);
        assert_eq!(mapper.map_gid(300), 400);
    }

    #[test]
    fn test_table_mode_not_allowed_for_uid_entry() {
        let mut mapper = IdMapper::new(IdMapMode::Identity);
        let result = mapper.add_uid_entry(IdMapEntry {
            host_id: 100,
            local_id: 200,
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_rangeshift_gid() {
        let mapper = IdMapper::new(IdMapMode::RangeShift {
            host_base: 1000,
            local_base: 2000,
            count: 100,
        });
        assert_eq!(mapper.map_gid(1050), 2050);
        assert_eq!(mapper.map_gid(999), 999);
    }
}
