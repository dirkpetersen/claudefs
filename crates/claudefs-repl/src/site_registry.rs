//! Site registry for peer identity validation (addresses FINDING-06).
//!
//! Tracks known peer sites with their TLS fingerprints, enabling validation
//! that the `source_site_id` in an `EntryBatch` matches the authenticated TLS identity.

use std::collections::HashMap;
use thiserror::Error;

/// Record of a known site in the registry.
#[derive(Debug, Clone, PartialEq)]
pub struct SiteRecord {
    /// Unique site identifier.
    pub site_id: u64,
    /// Human-readable display name.
    pub display_name: String,
    /// SHA-256 fingerprint of the TLS certificate (if TLS is configured).
    pub tls_fingerprint: Option<[u8; 32]>,
    /// Network addresses for this site.
    pub addresses: Vec<String>,
    /// Timestamp when this site was added (microseconds since epoch).
    pub added_at_us: u64,
    /// Timestamp of last seen activity (microseconds since epoch).
    pub last_seen_us: u64,
}

impl SiteRecord {
    /// Create a new site record with the given site_id and display_name.
    pub fn new(site_id: u64, display_name: &str) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;
        Self {
            site_id,
            display_name: display_name.to_string(),
            tls_fingerprint: None,
            addresses: Vec::new(),
            added_at_us: now,
            last_seen_us: now,
        }
    }
}

/// Errors from site registry operations.
#[derive(Debug, Error, PartialEq)]
pub enum SiteRegistryError {
    /// Site is already registered.
    #[error("site {site_id} is already registered")]
    AlreadyRegistered {
        /// The site ID that is already registered.
        site_id: u64,
    },
    /// Site not found in registry.
    #[error("site {site_id} not found")]
    NotFound {
        /// The site ID that was not found.
        site_id: u64,
    },
    /// TLS fingerprint mismatch (potential MITM attack).
    #[error("TLS fingerprint mismatch for site {site_id}")]
    FingerprintMismatch {
        /// The site ID with the mismatch.
        site_id: u64,
    },
}

/// Registry for tracking known peer sites.
pub struct SiteRegistry {
    sites: HashMap<u64, SiteRecord>,
}

impl SiteRegistry {
    /// Create a new empty site registry.
    pub fn new() -> Self {
        Self {
            sites: HashMap::new(),
        }
    }

    /// Register a new site. Returns error if already registered.
    pub fn register(&mut self, record: SiteRecord) -> Result<(), SiteRegistryError> {
        let site_id = record.site_id;
        if self.sites.contains_key(&site_id) {
            return Err(SiteRegistryError::AlreadyRegistered { site_id });
        }
        self.sites.insert(site_id, record);
        Ok(())
    }

    /// Unregister a site by ID. Returns error if not found.
    pub fn unregister(&mut self, site_id: u64) -> Result<SiteRecord, SiteRegistryError> {
        self.sites
            .remove(&site_id)
            .ok_or(SiteRegistryError::NotFound { site_id })
    }

    /// Lookup a site by ID. Returns None if not found.
    pub fn lookup(&self, site_id: u64) -> Option<&SiteRecord> {
        self.sites.get(&site_id)
    }

    /// Verify that a source_site_id is known and optionally validate TLS fingerprint.
    pub fn verify_source_id(
        &self,
        site_id: u64,
        tls_fingerprint: Option<&[u8; 32]>,
    ) -> Result<(), SiteRegistryError> {
        let record = self
            .sites
            .get(&site_id)
            .ok_or(SiteRegistryError::NotFound { site_id })?;

        if let (Some(expected_fp), Some(given_fp)) =
            (record.tls_fingerprint.as_ref(), tls_fingerprint)
        {
            if expected_fp != given_fp {
                return Err(SiteRegistryError::FingerprintMismatch { site_id });
            }
        }

        Ok(())
    }

    /// Update the last_seen timestamp for a site.
    pub fn update_last_seen(
        &mut self,
        site_id: u64,
        timestamp_us: u64,
    ) -> Result<(), SiteRegistryError> {
        let record = self
            .sites
            .get_mut(&site_id)
            .ok_or(SiteRegistryError::NotFound { site_id })?;
        record.last_seen_us = timestamp_us;
        Ok(())
    }

    /// Get the number of registered sites.
    pub fn len(&self) -> usize {
        self.sites.len()
    }

    /// Returns true if the registry has no sites.
    pub fn is_empty(&self) -> bool {
        self.sites.is_empty()
    }

    /// Get an iterator over all sites.
    pub fn sites(&self) -> impl Iterator<Item = &SiteRecord> {
        self.sites.values()
    }
}

impl Default for SiteRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64
    }

    #[test]
    fn test_register_new_site() {
        let mut registry = SiteRegistry::new();
        let record = SiteRecord::new(1, "site-a");
        let result = registry.register(record);
        assert!(result.is_ok());
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_register_duplicate_site() {
        let mut registry = SiteRegistry::new();
        let record = SiteRecord::new(1, "site-a");
        registry.register(record).unwrap();
        let record2 = SiteRecord::new(1, "site-b");
        let result = registry.register(record2);
        assert!(matches!(
            result,
            Err(SiteRegistryError::AlreadyRegistered { site_id: 1 })
        ));
    }

    #[test]
    fn test_lookup_existing_site() {
        let mut registry = SiteRegistry::new();
        let record = SiteRecord::new(1, "site-a");
        registry.register(record).unwrap();

        let found = registry.lookup(1);
        assert!(found.is_some());
        assert_eq!(found.unwrap().display_name, "site-a");
    }

    #[test]
    fn test_lookup_nonexistent_site() {
        let registry = SiteRegistry::new();
        let found = registry.lookup(999);
        assert!(found.is_none());
    }

    #[test]
    fn test_unregister_existing_site() {
        let mut registry = SiteRegistry::new();
        let record = SiteRecord::new(1, "site-a");
        registry.register(record).unwrap();

        let result = registry.unregister(1);
        assert!(result.is_ok());
        assert!(registry.is_empty());
    }

    #[test]
    fn test_unregister_nonexistent_site() {
        let mut registry = SiteRegistry::new();
        let result = registry.unregister(999);
        assert!(matches!(
            result,
            Err(SiteRegistryError::NotFound { site_id: 999 })
        ));
    }

    #[test]
    fn test_verify_source_id_known_site() {
        let mut registry = SiteRegistry::new();
        let record = SiteRecord::new(1, "site-a");
        registry.register(record).unwrap();

        let result = registry.verify_source_id(1, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_source_id_unknown_site() {
        let registry = SiteRegistry::new();
        let result = registry.verify_source_id(999, None);
        assert!(matches!(
            result,
            Err(SiteRegistryError::NotFound { site_id: 999 })
        ));
    }

    #[test]
    fn test_verify_source_id_with_fingerprint_match() {
        let mut registry = SiteRegistry::new();
        let mut record = SiteRecord::new(1, "site-a");
        let fp: [u8; 32] = [0xAB; 32];
        record.tls_fingerprint = Some(fp);
        registry.register(record).unwrap();

        let result = registry.verify_source_id(1, Some(&[0xAB; 32]));
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_source_id_with_fingerprint_mismatch() {
        let mut registry = SiteRegistry::new();
        let mut record = SiteRecord::new(1, "site-a");
        let fp: [u8; 32] = [0xAB; 32];
        record.tls_fingerprint = Some(fp);
        registry.register(record).unwrap();

        let result = registry.verify_source_id(1, Some(&[0xCD; 32]));
        assert!(matches!(
            result,
            Err(SiteRegistryError::FingerprintMismatch { site_id: 1 })
        ));
    }

    #[test]
    fn test_update_last_seen() {
        let mut registry = SiteRegistry::new();
        let record = SiteRecord::new(1, "site-a");
        registry.register(record).unwrap();

        let ts = now() + 1000000;
        let result = registry.update_last_seen(1, ts);
        assert!(result.is_ok());

        let found = registry.lookup(1).unwrap();
        assert_eq!(found.last_seen_us, ts);
    }

    #[test]
    fn test_update_last_seen_nonexistent() {
        let mut registry = SiteRegistry::new();
        let result = registry.update_last_seen(999, now());
        assert!(matches!(
            result,
            Err(SiteRegistryError::NotFound { site_id: 999 })
        ));
    }

    #[test]
    fn test_sites_iterator() {
        let mut registry = SiteRegistry::new();
        registry.register(SiteRecord::new(1, "site-a")).unwrap();
        registry.register(SiteRecord::new(2, "site-b")).unwrap();

        let mut ids: Vec<u64> = registry.sites().map(|s| s.site_id).collect();
        ids.sort();
        assert_eq!(ids, vec![1, 2]);
    }

    #[test]
    fn test_reregister_after_unregister() {
        let mut registry = SiteRegistry::new();
        registry.register(SiteRecord::new(1, "site-a")).unwrap();
        registry.unregister(1).unwrap();

        let result = registry.register(SiteRecord::new(1, "site-a"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_site_record_new() {
        let record = SiteRecord::new(42, "test-site");
        assert_eq!(record.site_id, 42);
        assert_eq!(record.display_name, "test-site");
        assert!(record.tls_fingerprint.is_none());
        assert!(record.addresses.is_empty());
    }

    #[test]
    fn test_site_record_clone() {
        let record = SiteRecord::new(1, "site-a");
        let cloned = record.clone();
        assert_eq!(record, cloned);
    }

    #[test]
    fn test_error_display() {
        let err = SiteRegistryError::AlreadyRegistered { site_id: 123 };
        assert_eq!(format!("{}", err), "site 123 is already registered");

        let err = SiteRegistryError::NotFound { site_id: 456 };
        assert_eq!(format!("{}", err), "site 456 not found");

        let err = SiteRegistryError::FingerprintMismatch { site_id: 789 };
        assert_eq!(format!("{}", err), "TLS fingerprint mismatch for site 789");
    }

    #[test]
    fn test_len_and_is_empty() {
        let registry = SiteRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);

        let mut registry = SiteRegistry::new();
        registry.register(SiteRecord::new(1, "site-a")).unwrap();
        assert!(!registry.is_empty());
        assert_eq!(registry.len(), 1);
    }
}
