//! POSIX Access Control Lists (ACLs) for fine-grained permission control.
//!
//! This module implements POSIX extended ACLs on top of the basic `uid/gid/mode`
//! permission model. ACLs allow specifying permissions for specific users and
//! groups beyond the traditional owner/group/other model.

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

use crate::kvstore::{KvStore, MemoryKvStore};
use crate::types::{InodeId, MetaError};

const ACL_PREFIX: &[u8] = b"acl:";

/// ACL tag identifying the type of an ACL entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AclTag {
    /// Owning user (corresponds to file owner).
    UserObj,
    /// Named user by UID.
    User(u32),
    /// Owning group (corresponds to file group).
    GroupObj,
    /// Named group by GID.
    Group(u32),
    /// Mask entry (limits permissions for named users/groups and group owner).
    Mask,
    /// Other (users not matching any other entry).
    Other,
}

/// A single entry in an ACL.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AclEntry {
    /// Tag identifying the entry type.
    pub tag: AclTag,
    /// Permission bits: 4=read, 2=write, 1=execute.
    pub perms: u8,
}

/// A complete ACL for an inode.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Acl {
    /// The list of ACL entries.
    pub entries: Vec<AclEntry>,
}

/// Storage backend for ACLs.
pub struct AclStore {
    kv: Arc<Mutex<MemoryKvStore>>,
}

impl AclStore {
    /// Creates a new ACL store backed by the given KV store.
    pub fn new(kv: Arc<Mutex<MemoryKvStore>>) -> Self {
        Self { kv }
    }

    fn acl_key(ino: InodeId) -> Vec<u8> {
        let mut key = ACL_PREFIX.to_vec();
        key.extend_from_slice(&ino.as_u64().to_be_bytes());
        key
    }

    /// Stores an ACL for an inode (access ACL).
    ///
    /// # Errors
    ///
    /// Returns `MetaError::KvError` if the underlying store fails.
    pub fn set_acl(&self, ino: InodeId, acl: Acl) -> Result<(), MetaError> {
        let key = Self::acl_key(ino);
        let value = bincode::serialize(&acl)
            .map_err(|e| MetaError::KvError(format!("serialize: {}", e)))?;
        self.kv
            .lock()
            .map_err(|e| MetaError::KvError(e.to_string()))?
            .put(key, value)
    }

    /// Loads an ACL for an inode.
    ///
    /// Returns `Ok(None)` if no ACL is stored for the inode.
    pub fn get_acl(&self, ino: InodeId) -> Result<Option<Acl>, MetaError> {
        let key = Self::acl_key(ino);
        let kv = self
            .kv
            .lock()
            .map_err(|e| MetaError::KvError(e.to_string()))?;
        match kv.get(&key)? {
            Some(value) => {
                let acl: Acl = bincode::deserialize(&value)
                    .map_err(|e| MetaError::KvError(format!("deserialize: {}", e)))?;
                Ok(Some(acl))
            }
            None => Ok(None),
        }
    }

    /// Removes an ACL for an inode.
    pub fn remove_acl(&self, ino: InodeId) -> Result<(), MetaError> {
        let key = Self::acl_key(ino);
        self.kv
            .lock()
            .map_err(|e| MetaError::KvError(e.to_string()))?
            .delete(&key)
    }

    /// Checks if a user has the given permission on the inode.
    ///
    /// Uses the full POSIX ACL check algorithm:
    /// 1. If user is owner, use `UserObj` entry.
    /// 2. If user matches a named user entry, use that entry (masked).
    /// 3. If user is in owning group or a named group, use best matching group entry (masked).
    /// 4. Otherwise, use `Other` entry.
    pub fn check_permission(
        &self,
        ino: InodeId,
        attr: &crate::types::InodeAttr,
        uid: u32,
        gid: u32,
        supplementary_gids: &[u32],
        perms: u8,
    ) -> Result<bool, MetaError> {
        match self.get_acl(ino)? {
            Some(acl) => {
                let effective =
                    self.effective_perms(&acl, uid, gid, supplementary_gids, attr.uid, attr.gid);
                Ok((effective & perms) == perms)
            }
            None => {
                let mode_perms = basic_perms(attr.mode, uid, gid, supplementary_gids);
                Ok((mode_perms & perms) == perms)
            }
        }
    }

    /// Validates an ACL.
    ///
    /// A valid ACL must have `UserObj`, `GroupObj`, and `Other` entries.
    /// If there are named users or groups, a `Mask` entry is required.
    pub fn validate(&self, acl: &Acl) -> Result<(), MetaError> {
        let has_user_obj = acl.entries.iter().any(|e| e.tag == AclTag::UserObj);
        let has_group_obj = acl.entries.iter().any(|e| e.tag == AclTag::GroupObj);
        let has_other = acl.entries.iter().any(|e| e.tag == AclTag::Other);

        if !has_user_obj {
            return Err(MetaError::InvalidArgument("missing UserObj entry".into()));
        }
        if !has_group_obj {
            return Err(MetaError::InvalidArgument("missing GroupObj entry".into()));
        }
        if !has_other {
            return Err(MetaError::InvalidArgument("missing Other entry".into()));
        }

        let has_named_user = acl.entries.iter().any(|e| matches!(e.tag, AclTag::User(_)));
        let has_named_group = acl
            .entries
            .iter()
            .any(|e| matches!(e.tag, AclTag::Group(_)));
        let has_mask = acl.entries.iter().any(|e| e.tag == AclTag::Mask);

        if (has_named_user || has_named_group) && !has_mask {
            return Err(MetaError::InvalidArgument(
                "Mask required when named users or groups exist".into(),
            ));
        }

        Ok(())
    }

    /// Computes effective permissions for a user (applies mask).
    ///
    /// Returns the permission bits after applying the mask if present.
    pub fn effective_perms(
        &self,
        acl: &Acl,
        uid: u32,
        gid: u32,
        supplementary_gids: &[u32],
        owner_uid: u32,
        owner_gid: u32,
    ) -> u8 {
        let mask = acl
            .entries
            .iter()
            .find(|e| e.tag == AclTag::Mask)
            .map(|e| e.perms);

        for entry in &acl.entries {
            match &entry.tag {
                AclTag::UserObj => {
                    if uid == 0 || uid == owner_uid {
                        return entry.perms;
                    }
                }
                AclTag::User(named_uid) => {
                    if *named_uid == uid {
                        let perms = entry.perms;
                        return mask.map_or(perms, |m| perms & m);
                    }
                }
                _ => {}
            }
        }

        let all_gids: Vec<u32> = std::iter::once(gid)
            .chain(supplementary_gids.iter().copied())
            .collect();

        let mut best_group_perm: Option<u8> = None;
        for entry in &acl.entries {
            match &entry.tag {
                AclTag::GroupObj => {
                    if all_gids.contains(&owner_gid) || uid == 0 {
                        let perms = entry.perms;
                        let effective = mask.map_or(perms, |m| perms & m);
                        best_group_perm =
                            Some(best_group_perm.map_or(effective, |b| b | effective));
                    }
                }
                AclTag::Group(named_gid) => {
                    if all_gids.contains(named_gid) {
                        let perms = entry.perms;
                        let effective = mask.map_or(perms, |m| perms & m);
                        best_group_perm =
                            Some(best_group_perm.map_or(effective, |b| b | effective));
                    }
                }
                _ => {}
            }
        }

        if let Some(perms) = best_group_perm {
            return perms;
        }

        for entry in &acl.entries {
            if entry.tag == AclTag::Other {
                return entry.perms;
            }
        }

        0
    }
}

/// Extracts permission bits from a mode based on user/group context.
fn basic_perms(mode: u32, uid: u32, gid: u32, supplementary_gids: &[u32]) -> u8 {
    if uid == 0 {
        return 7;
    }

    let owner_uid = (mode >> 24) & 0xFF;
    let owner_gid = (mode >> 16) & 0xFF;
    let mode_bits = mode & 0o777;

    if owner_uid == uid {
        return ((mode_bits >> 6) & 0o7) as u8;
    }

    if owner_gid == gid || supplementary_gids.contains(&owner_gid) {
        return ((mode_bits >> 3) & 0o7) as u8;
    }

    (mode_bits & 0o7) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_store() -> AclStore {
        AclStore::new(Arc::new(Mutex::new(MemoryKvStore::new())))
    }

    fn make_acl(entries: Vec<(AclTag, u8)>) -> Acl {
        Acl {
            entries: entries
                .into_iter()
                .map(|(tag, perms)| AclEntry { tag, perms })
                .collect(),
        }
    }

    #[test]
    fn test_set_and_get_acl() {
        let store = make_test_store();
        let ino = InodeId::new(42);
        let acl = make_acl(vec![
            (AclTag::UserObj, 7),
            (AclTag::GroupObj, 5),
            (AclTag::Other, 0),
        ]);

        store.set_acl(ino, acl.clone()).unwrap();
        let retrieved = store.get_acl(ino).unwrap().unwrap();
        assert_eq!(retrieved.entries.len(), 3);
        assert_eq!(retrieved.entries[0].tag, AclTag::UserObj);
        assert_eq!(retrieved.entries[0].perms, 7);
    }

    #[test]
    fn test_remove_acl() {
        let store = make_test_store();
        let ino = InodeId::new(42);
        let acl = make_acl(vec![
            (AclTag::UserObj, 7),
            (AclTag::GroupObj, 5),
            (AclTag::Other, 0),
        ]);

        store.set_acl(ino, acl).unwrap();
        assert!(store.get_acl(ino).unwrap().is_some());

        store.remove_acl(ino).unwrap();
        assert!(store.get_acl(ino).unwrap().is_none());
    }

    #[test]
    fn test_check_permission_owner() {
        let store = make_test_store();
        let ino = InodeId::new(42);

        let acl = make_acl(vec![
            (AclTag::UserObj, 7),
            (AclTag::GroupObj, 0),
            (AclTag::Other, 0),
        ]);
        store.set_acl(ino, acl).unwrap();

        let attr = crate::types::InodeAttr::new_file(ino, 1000, 100, 0o644, 1);

        assert!(store
            .check_permission(ino, &attr, 1000, 200, &[], 4)
            .unwrap());
        assert!(store
            .check_permission(ino, &attr, 1000, 200, &[], 2)
            .unwrap());
        assert!(!store
            .check_permission(ino, &attr, 2000, 200, &[], 4)
            .unwrap());
    }

    #[test]
    fn test_check_permission_named_user() {
        let store = make_test_store();
        let ino = InodeId::new(42);

        let acl = make_acl(vec![
            (AclTag::UserObj, 0),
            (AclTag::User(2000), 7),
            (AclTag::GroupObj, 0),
            (AclTag::Mask, 6),
            (AclTag::Other, 0),
        ]);
        store.set_acl(ino, acl).unwrap();

        let attr = crate::types::InodeAttr::new_file(ino, 1000, 100, 0o600, 1);

        assert!(store
            .check_permission(ino, &attr, 2000, 200, &[], 4)
            .unwrap());
        assert!(!store
            .check_permission(ino, &attr, 2000, 200, &[], 1)
            .unwrap());
    }

    #[test]
    fn test_check_permission_group() {
        let store = make_test_store();
        let ino = InodeId::new(42);

        let acl = make_acl(vec![
            (AclTag::UserObj, 0),
            (AclTag::GroupObj, 5),
            (AclTag::Mask, 7),
            (AclTag::Other, 0),
        ]);
        store.set_acl(ino, acl).unwrap();

        let attr = crate::types::InodeAttr::new_file(ino, 1000, 100, 0o640, 1);

        assert!(store
            .check_permission(ino, &attr, 999, 100, &[], 4)
            .unwrap());
        assert!(store
            .check_permission(ino, &attr, 999, 100, &[], 1)
            .unwrap());
        assert!(!store
            .check_permission(ino, &attr, 999, 100, &[], 2)
            .unwrap());
    }

    #[test]
    fn test_check_permission_other() {
        let store = make_test_store();
        let ino = InodeId::new(42);

        let acl = make_acl(vec![
            (AclTag::UserObj, 0),
            (AclTag::GroupObj, 0),
            (AclTag::Other, 4),
        ]);
        store.set_acl(ino, acl).unwrap();

        let attr = crate::types::InodeAttr::new_file(ino, 1000, 100, 0o644, 1);

        assert!(store
            .check_permission(ino, &attr, 9999, 999, &[], 4)
            .unwrap());
        assert!(!store
            .check_permission(ino, &attr, 9999, 999, &[], 2)
            .unwrap());
    }

    #[test]
    fn test_validate_acl_missing_required_entry() {
        let store = make_test_store();

        let acl_no_user_obj = make_acl(vec![(AclTag::GroupObj, 5), (AclTag::Other, 0)]);
        assert!(store.validate(&acl_no_user_obj).is_err());

        let acl_no_group_obj = make_acl(vec![(AclTag::UserObj, 7), (AclTag::Other, 0)]);
        assert!(store.validate(&acl_no_group_obj).is_err());

        let acl_no_other = make_acl(vec![(AclTag::UserObj, 7), (AclTag::GroupObj, 5)]);
        assert!(store.validate(&acl_no_other).is_err());

        let acl_named_no_mask = make_acl(vec![
            (AclTag::UserObj, 7),
            (AclTag::User(1001), 6),
            (AclTag::GroupObj, 5),
            (AclTag::Other, 0),
        ]);
        assert!(store.validate(&acl_named_no_mask).is_err());

        let acl_valid = make_acl(vec![
            (AclTag::UserObj, 7),
            (AclTag::User(1001), 6),
            (AclTag::GroupObj, 5),
            (AclTag::Mask, 6),
            (AclTag::Other, 0),
        ]);
        assert!(store.validate(&acl_valid).is_ok());
    }

    #[test]
    fn test_acl_tag_serde() {
        let tags = vec![
            AclTag::UserObj,
            AclTag::User(1000),
            AclTag::GroupObj,
            AclTag::Group(100),
            AclTag::Mask,
            AclTag::Other,
        ];
        for tag in tags {
            let encoded = bincode::serialize(&tag).unwrap();
            let decoded: AclTag = bincode::deserialize(&encoded).unwrap();
            assert_eq!(tag, decoded);
        }
    }

    #[test]
    fn test_acl_entry_serde() {
        let entry = AclEntry {
            tag: AclTag::User(1000),
            perms: 5,
        };
        let encoded = bincode::serialize(&entry).unwrap();
        let decoded: AclEntry = bincode::deserialize(&encoded).unwrap();
        assert_eq!(entry, decoded);
    }

    #[test]
    fn test_acl_serde() {
        let acl = make_acl(vec![
            (AclTag::UserObj, 7),
            (AclTag::GroupObj, 5),
            (AclTag::Other, 0),
        ]);
        let encoded = bincode::serialize(&acl).unwrap();
        let decoded: Acl = bincode::deserialize(&encoded).unwrap();
        assert_eq!(acl, decoded);
    }
}
