//! UID/GID mapping between sites.
//!
//! When replicating between sites with different user databases,
//! UID/GID values must be translated between namespaces.

use std::collections::HashMap;

/// A mapping entry: source UID maps to destination UID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UidMapping {
    /// Source site ID.
    pub source_site_id: u64,
    /// UID in the source site's namespace.
    pub source_uid: u32,
    /// UID in the local namespace.
    pub dest_uid: u32,
}

/// A mapping entry for GID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GidMapping {
    /// Source site ID.
    pub source_site_id: u64,
    /// GID in the source site's namespace.
    pub source_gid: u32,
    /// GID in the local namespace.
    pub dest_gid: u32,
}

/// A UID/GID mapper that translates identity fields in journal payloads.
#[derive(Debug, Clone)]
pub struct UidMapper {
    uid_maps: HashMap<(u64, u32), u32>,
    gid_maps: HashMap<(u64, u32), u32>,
    passthrough: bool,
}

impl UidMapper {
    /// Create a passthrough mapper (no translation needed, same UID namespace).
    pub fn passthrough() -> Self {
        Self {
            uid_maps: HashMap::new(),
            gid_maps: HashMap::new(),
            passthrough: true,
        }
    }

    /// Create a mapper with explicit UID/GID mappings.
    pub fn new(uid_maps: Vec<UidMapping>, gid_maps: Vec<GidMapping>) -> Self {
        let mut uid_map = HashMap::new();
        for m in uid_maps {
            uid_map.insert((m.source_site_id, m.source_uid), m.dest_uid);
        }

        let mut gid_map = HashMap::new();
        for m in gid_maps {
            gid_map.insert((m.source_site_id, m.source_gid), m.dest_gid);
        }

        Self {
            uid_maps: uid_map,
            gid_maps: gid_map,
            passthrough: false,
        }
    }

    /// Translate a UID from the source site to the local namespace.
    pub fn translate_uid(&self, source_site_id: u64, uid: u32) -> u32 {
        if self.passthrough {
            return uid;
        }
        self.uid_maps
            .get(&(source_site_id, uid))
            .copied()
            .unwrap_or(uid)
    }

    /// Translate a GID from the source site to the local namespace.
    pub fn translate_gid(&self, source_site_id: u64, gid: u32) -> u32 {
        if self.passthrough {
            return gid;
        }
        self.gid_maps
            .get(&(source_site_id, gid))
            .copied()
            .unwrap_or(gid)
    }

    /// Add or update a UID mapping.
    pub fn add_uid_mapping(&mut self, mapping: UidMapping) {
        self.uid_maps.insert(
            (mapping.source_site_id, mapping.source_uid),
            mapping.dest_uid,
        );
        self.passthrough = false;
    }

    /// Add or update a GID mapping.
    pub fn add_gid_mapping(&mut self, mapping: GidMapping) {
        self.gid_maps.insert(
            (mapping.source_site_id, mapping.source_gid),
            mapping.dest_gid,
        );
        self.passthrough = false;
    }

    /// Remove a UID mapping for a site/uid pair.
    pub fn remove_uid_mapping(&mut self, source_site_id: u64, source_uid: u32) {
        self.uid_maps.remove(&(source_site_id, source_uid));
    }

    /// Remove a GID mapping for a site/gid pair.
    pub fn remove_gid_mapping(&mut self, source_site_id: u64, source_gid: u32) {
        self.gid_maps.remove(&(source_site_id, source_gid));
    }

    /// List all UID mappings.
    pub fn uid_mappings(&self) -> Vec<UidMapping> {
        self.uid_maps
            .iter()
            .map(|((site_id, source_uid), &dest_uid)| UidMapping {
                source_site_id: *site_id,
                source_uid: *source_uid,
                dest_uid,
            })
            .collect()
    }

    /// List all GID mappings.
    pub fn gid_mappings(&self) -> Vec<GidMapping> {
        self.gid_maps
            .iter()
            .map(|((site_id, source_gid), &dest_gid)| GidMapping {
                source_site_id: *site_id,
                source_gid: *source_gid,
                dest_gid,
            })
            .collect()
    }

    /// Returns true if this is a passthrough mapper (no translations).
    pub fn is_passthrough(&self) -> bool {
        self.passthrough
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod passthrough_mapper {
        use super::*;

        #[test]
        fn test_passthrough_uid_unchanged() {
            let mapper = UidMapper::passthrough();
            assert_eq!(mapper.translate_uid(1, 1000), 1000);
        }

        #[test]
        fn test_passthrough_gid_unchanged() {
            let mapper = UidMapper::passthrough();
            assert_eq!(mapper.translate_gid(1, 1000), 1000);
        }

        #[test]
        fn test_passthrough_is_passthrough() {
            let mapper = UidMapper::passthrough();
            assert!(mapper.is_passthrough());
        }

        #[test]
        fn test_passthrough_large_uid() {
            let mapper = UidMapper::passthrough();
            assert_eq!(mapper.translate_uid(1, u32::MAX), u32::MAX);
        }

        #[test]
        fn test_passthrough_zero_uid() {
            let mapper = UidMapper::passthrough();
            assert_eq!(mapper.translate_uid(1, 0), 0);
        }
    }

    mod translate_known_uid {
        use super::*;

        #[test]
        fn test_translate_known_uid_basic() {
            let mapper = UidMapper::new(
                vec![UidMapping {
                    source_site_id: 1,
                    source_uid: 1000,
                    dest_uid: 2000,
                }],
                vec![],
            );
            assert_eq!(mapper.translate_uid(1, 1000), 2000);
        }

        #[test]
        fn test_translate_known_uid_different_site() {
            let mapper = UidMapper::new(
                vec![UidMapping {
                    source_site_id: 1,
                    source_uid: 1000,
                    dest_uid: 2000,
                }],
                vec![],
            );
            // Different site ID returns original
            assert_eq!(mapper.translate_uid(2, 1000), 1000);
        }

        #[test]
        fn test_translate_unknown_uid_returns_original() {
            let mapper = UidMapper::new(
                vec![UidMapping {
                    source_site_id: 1,
                    source_uid: 1000,
                    dest_uid: 2000,
                }],
                vec![],
            );
            // Unknown UID returns original
            assert_eq!(mapper.translate_uid(1, 9999), 9999);
        }
    }

    mod multiple_site_mappings {
        use super::*;

        #[test]
        fn test_different_sites_different_mappings() {
            let mapper = UidMapper::new(
                vec![
                    UidMapping {
                        source_site_id: 1,
                        source_uid: 100,
                        dest_uid: 1000,
                    },
                    UidMapping {
                        source_site_id: 2,
                        source_uid: 100,
                        dest_uid: 2000,
                    },
                ],
                vec![],
            );
            assert_eq!(mapper.translate_uid(1, 100), 1000);
            assert_eq!(mapper.translate_uid(2, 100), 2000);
        }

        #[test]
        fn test_three_sites_three_mappings() {
            let mapper = UidMapper::new(
                vec![
                    UidMapping {
                        source_site_id: 1,
                        source_uid: 50,
                        dest_uid: 150,
                    },
                    UidMapping {
                        source_site_id: 2,
                        source_uid: 50,
                        dest_uid: 250,
                    },
                    UidMapping {
                        source_site_id: 3,
                        source_uid: 50,
                        dest_uid: 350,
                    },
                ],
                vec![],
            );
            assert_eq!(mapper.translate_uid(1, 50), 150);
            assert_eq!(mapper.translate_uid(2, 50), 250);
            assert_eq!(mapper.translate_uid(3, 50), 350);
        }
    }

    mod add_remove_mappings {
        use super::*;

        #[test]
        fn test_add_uid_mapping() {
            let mut mapper = UidMapper::passthrough();
            mapper.add_uid_mapping(UidMapping {
                source_site_id: 1,
                source_uid: 1000,
                dest_uid: 2000,
            });
            assert_eq!(mapper.translate_uid(1, 1000), 2000);
        }

        #[test]
        fn test_add_gid_mapping() {
            let mut mapper = UidMapper::passthrough();
            mapper.add_gid_mapping(GidMapping {
                source_site_id: 1,
                source_gid: 1000,
                dest_gid: 2000,
            });
            assert_eq!(mapper.translate_gid(1, 1000), 2000);
        }

        #[test]
        fn test_remove_uid_mapping() {
            let mut mapper = UidMapper::new(
                vec![UidMapping {
                    source_site_id: 1,
                    source_uid: 1000,
                    dest_uid: 2000,
                }],
                vec![],
            );
            mapper.remove_uid_mapping(1, 1000);
            assert_eq!(mapper.translate_uid(1, 1000), 1000);
        }

        #[test]
        fn test_remove_gid_mapping() {
            let mut mapper = UidMapper::new(
                vec![],
                vec![GidMapping {
                    source_site_id: 1,
                    source_gid: 1000,
                    dest_gid: 2000,
                }],
            );
            mapper.remove_gid_mapping(1, 1000);
            assert_eq!(mapper.translate_gid(1, 1000), 1000);
        }

        #[test]
        fn test_remove_nonexistent_mapping() {
            let mut mapper = UidMapper::new(
                vec![UidMapping {
                    source_site_id: 1,
                    source_uid: 1000,
                    dest_uid: 2000,
                }],
                vec![],
            );
            // Should not panic
            mapper.remove_uid_mapping(1, 9999);
            // Original mapping should still work
            assert_eq!(mapper.translate_uid(1, 1000), 2000);
        }
    }

    mod gid_translation {
        use super::*;

        #[test]
        fn test_translate_known_gid() {
            let mapper = UidMapper::new(
                vec![],
                vec![GidMapping {
                    source_site_id: 1,
                    source_gid: 1000,
                    dest_gid: 2000,
                }],
            );
            assert_eq!(mapper.translate_gid(1, 1000), 2000);
        }

        #[test]
        fn test_translate_unknown_gid_returns_original() {
            let mapper = UidMapper::new(
                vec![],
                vec![GidMapping {
                    source_site_id: 1,
                    source_gid: 1000,
                    dest_gid: 2000,
                }],
            );
            assert_eq!(mapper.translate_gid(1, 9999), 9999);
        }

        #[test]
        fn test_gid_different_site_returns_original() {
            let mapper = UidMapper::new(
                vec![],
                vec![GidMapping {
                    source_site_id: 1,
                    source_gid: 1000,
                    dest_gid: 2000,
                }],
            );
            assert_eq!(mapper.translate_gid(2, 1000), 1000);
        }
    }

    mod mixed_translation {
        use super::*;

        #[test]
        fn test_uid_and_gid_translation() {
            let mapper = UidMapper::new(
                vec![UidMapping {
                    source_site_id: 1,
                    source_uid: 100,
                    dest_uid: 200,
                }],
                vec![GidMapping {
                    source_site_id: 1,
                    source_gid: 300,
                    dest_gid: 400,
                }],
            );
            assert_eq!(mapper.translate_uid(1, 100), 200);
            assert_eq!(mapper.translate_gid(1, 300), 400);
        }

        #[test]
        fn test_uid_gid_independent() {
            let mapper = UidMapper::new(
                vec![UidMapping {
                    source_site_id: 1,
                    source_uid: 100,
                    dest_uid: 200,
                }],
                vec![],
            );
            // GID translation should return original since no mapping
            assert_eq!(mapper.translate_gid(1, 100), 100);
            // UID translation should work
            assert_eq!(mapper.translate_uid(1, 100), 200);
        }
    }

    mod overwrite_mapping {
        use super::*;

        #[test]
        fn test_overwrite_existing_uid_mapping() {
            let mut mapper = UidMapper::new(
                vec![UidMapping {
                    source_site_id: 1,
                    source_uid: 1000,
                    dest_uid: 2000,
                }],
                vec![],
            );
            // Add new mapping with same source, different dest
            mapper.add_uid_mapping(UidMapping {
                source_site_id: 1,
                source_uid: 1000,
                dest_uid: 3000,
            });
            assert_eq!(mapper.translate_uid(1, 1000), 3000);
        }

        #[test]
        fn test_overwrite_via_new_method() {
            let mapper1 = UidMapper::new(
                vec![UidMapping {
                    source_site_id: 1,
                    source_uid: 1000,
                    dest_uid: 2000,
                }],
                vec![],
            );
            let mapper2 = UidMapper::new(
                vec![UidMapping {
                    source_site_id: 1,
                    source_uid: 1000,
                    dest_uid: 3000,
                }],
                vec![],
            );
            // Different mappers are independent
            assert_eq!(mapper1.translate_uid(1, 1000), 2000);
            assert_eq!(mapper2.translate_uid(1, 1000), 3000);
        }
    }

    mod list_mappings {
        use super::*;

        #[test]
        fn test_uid_mappings_list() {
            let mapper = UidMapper::new(
                vec![
                    UidMapping {
                        source_site_id: 1,
                        source_uid: 100,
                        dest_uid: 200,
                    },
                    UidMapping {
                        source_site_id: 2,
                        source_uid: 300,
                        dest_uid: 400,
                    },
                ],
                vec![],
            );
            let mappings = mapper.uid_mappings();
            assert_eq!(mappings.len(), 2);
        }

        #[test]
        fn test_gid_mappings_list() {
            let mapper = UidMapper::new(
                vec![],
                vec![
                    GidMapping {
                        source_site_id: 1,
                        source_gid: 100,
                        dest_gid: 200,
                    },
                    GidMapping {
                        source_site_id: 2,
                        source_gid: 300,
                        dest_gid: 400,
                    },
                ],
            );
            let mappings = mapper.gid_mappings();
            assert_eq!(mappings.len(), 2);
        }

        #[test]
        fn test_list_after_remove() {
            let mut mapper = UidMapper::new(
                vec![
                    UidMapping {
                        source_site_id: 1,
                        source_uid: 100,
                        dest_uid: 200,
                    },
                    UidMapping {
                        source_site_id: 1,
                        source_uid: 300,
                        dest_uid: 400,
                    },
                ],
                vec![],
            );
            mapper.remove_uid_mapping(1, 100);
            let mappings = mapper.uid_mappings();
            assert_eq!(mappings.len(), 1);
            assert_eq!(mappings[0].source_uid, 300);
        }

        #[test]
        fn test_empty_list() {
            let mapper = UidMapper::passthrough();
            assert!(mapper.uid_mappings().is_empty());
            assert!(mapper.gid_mappings().is_empty());
        }
    }

    mod is_passthrough {
        use super::*;

        #[test]
        fn test_passthrough_is_true() {
            let mapper = UidMapper::passthrough();
            assert!(mapper.is_passthrough());
        }

        #[test]
        fn test_with_mappings_is_false() {
            let mapper = UidMapper::new(
                vec![UidMapping {
                    source_site_id: 1,
                    source_uid: 100,
                    dest_uid: 200,
                }],
                vec![],
            );
            assert!(!mapper.is_passthrough());
        }

        #[test]
        fn test_after_add_mapping_becomes_false() {
            let mut mapper = UidMapper::passthrough();
            assert!(mapper.is_passthrough());
            mapper.add_uid_mapping(UidMapping {
                source_site_id: 1,
                source_uid: 100,
                dest_uid: 200,
            });
            assert!(!mapper.is_passthrough());
        }

        #[test]
        fn test_only_gid_mappings_is_not_passthrough() {
            let mapper = UidMapper::new(
                vec![],
                vec![GidMapping {
                    source_site_id: 1,
                    source_gid: 100,
                    dest_gid: 200,
                }],
            );
            assert!(!mapper.is_passthrough());
        }
    }
}
