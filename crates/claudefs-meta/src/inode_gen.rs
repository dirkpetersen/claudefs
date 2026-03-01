//! Inode generation numbers for NFS export consistency.
//!
//! Each inode has a generation number that changes when the inode number
//! is reused after deletion. NFS file handles include (ino, generation)
//! tuples, allowing stale handle detection when inodes are recycled.

use std::collections::HashMap;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::types::*;

/// A generation number for an inode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Generation(u64);

impl Generation {
    /// Creates a new generation number.
    pub fn new(gen: u64) -> Self {
        Self(gen)
    }

    /// Returns the generation as u64.
    pub fn as_u64(&self) -> u64 {
        self.0
    }

    /// Returns the next generation number.
    pub fn next(&self) -> Self {
        Self(self.0 + 1)
    }
}

impl Default for Generation {
    fn default() -> Self {
        Self(1)
    }
}

/// NFS file handle combining inode and generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NfsFileHandle {
    /// The inode number.
    pub ino: InodeId,
    /// The generation number at the time the handle was created.
    pub generation: Generation,
}

impl NfsFileHandle {
    /// Creates a new file handle.
    pub fn new(ino: InodeId, generation: Generation) -> Self {
        Self { ino, generation }
    }

    /// Serializes to bytes for NFS wire format.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(16);
        bytes.extend_from_slice(&self.ino.as_u64().to_le_bytes());
        bytes.extend_from_slice(&self.generation.as_u64().to_le_bytes());
        bytes
    }

    /// Deserializes from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 16 {
            return None;
        }
        let ino = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
        let gen = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
        Some(Self {
            ino: InodeId::new(ino),
            generation: Generation::new(gen),
        })
    }
}

/// Manages inode generation numbers.
///
/// Tracks the current generation for each inode number. When an inode
/// is deleted and the inode number is reused, the generation increments,
/// invalidating any NFS file handles that reference the old generation.
pub struct InodeGenManager {
    generations: RwLock<HashMap<InodeId, Generation>>,
}

impl InodeGenManager {
    /// Creates a new generation manager.
    pub fn new() -> Self {
        Self {
            generations: RwLock::new(HashMap::new()),
        }
    }

    /// Allocates a generation number for a new inode.
    /// If the inode number was previously used, increments the generation.
    pub fn allocate(&self, ino: InodeId) -> Generation {
        let mut gens = self.generations.write().unwrap();
        let gen = gens.entry(ino).and_modify(|g| *g = g.next()).or_default();
        *gen
    }

    /// Returns the current generation for an inode.
    pub fn get(&self, ino: &InodeId) -> Option<Generation> {
        let gens = self.generations.read().unwrap();
        gens.get(ino).copied()
    }

    /// Marks an inode as deleted, incrementing its generation.
    /// The next allocation of this inode number will get a higher generation.
    pub fn mark_deleted(&self, ino: &InodeId) {
        let mut gens = self.generations.write().unwrap();
        gens.entry(*ino)
            .and_modify(|g| *g = g.next())
            .or_insert_with(|| Generation::new(2));
    }

    /// Creates an NFS file handle for the given inode.
    pub fn make_handle(&self, ino: InodeId) -> NfsFileHandle {
        let gen = self.get(&ino).unwrap_or_default();
        NfsFileHandle::new(ino, gen)
    }

    /// Validates an NFS file handle against current generations.
    /// Returns true if the handle is still valid (generation matches).
    pub fn validate_handle(&self, handle: &NfsFileHandle) -> bool {
        match self.get(&handle.ino) {
            Some(current_gen) => current_gen == handle.generation,
            None => false,
        }
    }

    /// Returns the total number of tracked inodes.
    pub fn tracked_count(&self) -> usize {
        self.generations.read().unwrap().len()
    }

    /// Resets generation tracking (for testing or recovery).
    pub fn clear(&self) {
        self.generations.write().unwrap().clear();
    }

    /// Bulk loads generation data (for snapshot restore).
    pub fn load_generations(&self, data: Vec<(InodeId, Generation)>) {
        let mut gens = self.generations.write().unwrap();
        gens.clear();
        for (ino, gen) in data {
            gens.insert(ino, gen);
        }
    }

    /// Exports all generation data (for snapshot capture).
    pub fn export_generations(&self) -> Vec<(InodeId, Generation)> {
        let gens = self.generations.read().unwrap();
        gens.iter().map(|(k, v)| (*k, *v)).collect()
    }
}

impl Default for InodeGenManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allocate_new_inode() {
        let mgr = InodeGenManager::new();
        let gen = mgr.allocate(InodeId::new(100));
        assert_eq!(gen.as_u64(), 1);
    }

    #[test]
    fn test_allocate_reused_inode() {
        let mgr = InodeGenManager::new();
        let gen1 = mgr.allocate(InodeId::new(100));
        assert_eq!(gen1.as_u64(), 1);

        let gen2 = mgr.allocate(InodeId::new(100));
        assert_eq!(gen2.as_u64(), 2);
    }

    #[test]
    fn test_get_generation() {
        let mgr = InodeGenManager::new();
        mgr.allocate(InodeId::new(100));
        let gen = mgr.get(&InodeId::new(100));
        assert_eq!(gen, Some(Generation::new(1)));
    }

    #[test]
    fn test_get_unknown_inode() {
        let mgr = InodeGenManager::new();
        assert!(mgr.get(&InodeId::new(999)).is_none());
    }

    #[test]
    fn test_mark_deleted_increments_generation() {
        let mgr = InodeGenManager::new();
        mgr.allocate(InodeId::new(100));
        mgr.mark_deleted(&InodeId::new(100));

        let gen = mgr.get(&InodeId::new(100)).unwrap();
        assert_eq!(gen.as_u64(), 2);
    }

    #[test]
    fn test_make_handle() {
        let mgr = InodeGenManager::new();
        mgr.allocate(InodeId::new(100));
        let handle = mgr.make_handle(InodeId::new(100));
        assert_eq!(handle.ino, InodeId::new(100));
        assert_eq!(handle.generation.as_u64(), 1);
    }

    #[test]
    fn test_validate_handle_valid() {
        let mgr = InodeGenManager::new();
        mgr.allocate(InodeId::new(100));
        let handle = mgr.make_handle(InodeId::new(100));
        assert!(mgr.validate_handle(&handle));
    }

    #[test]
    fn test_validate_handle_stale() {
        let mgr = InodeGenManager::new();
        mgr.allocate(InodeId::new(100));
        let handle = mgr.make_handle(InodeId::new(100));

        mgr.mark_deleted(&InodeId::new(100));
        mgr.allocate(InodeId::new(100));

        assert!(!mgr.validate_handle(&handle));
    }

    #[test]
    fn test_file_handle_serialization() {
        let handle = NfsFileHandle::new(InodeId::new(42), Generation::new(7));
        let bytes = handle.to_bytes();
        let restored = NfsFileHandle::from_bytes(&bytes).unwrap();
        assert_eq!(handle, restored);
    }

    #[test]
    fn test_file_handle_from_short_bytes() {
        assert!(NfsFileHandle::from_bytes(&[0u8; 8]).is_none());
    }

    #[test]
    fn test_export_import_generations() {
        let mgr = InodeGenManager::new();
        mgr.allocate(InodeId::new(100));
        mgr.allocate(InodeId::new(200));

        let exported = mgr.export_generations();
        assert_eq!(exported.len(), 2);

        let mgr2 = InodeGenManager::new();
        mgr2.load_generations(exported);
        assert_eq!(mgr2.tracked_count(), 2);
        assert_eq!(mgr2.get(&InodeId::new(100)), Some(Generation::new(1)));
    }

    #[test]
    fn test_clear() {
        let mgr = InodeGenManager::new();
        mgr.allocate(InodeId::new(100));
        mgr.clear();
        assert_eq!(mgr.tracked_count(), 0);
    }

    #[test]
    fn test_generation_default() {
        let gen = Generation::default();
        assert_eq!(gen.as_u64(), 1);
    }

    #[test]
    fn test_generation_next() {
        let gen = Generation::new(5);
        assert_eq!(gen.next().as_u64(), 6);
    }
}
