//! NFS ACL types for NFSv3 POSIX ACL extension and NFSv4

use serde::{Deserialize, Serialize};

/// ACL entry type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AclTag {
    /// Owner of the file
    UserObj,
    /// Named user
    User,
    /// Owning group
    GroupObj,
    /// Named group
    Group,
    /// Mask entry (limits group/named entries)
    Mask,
    /// Others
    Other,
}

/// ACL permission bits
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AclPerms {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

impl AclPerms {
    pub fn new(read: bool, write: bool, execute: bool) -> Self {
        Self {
            read,
            write,
            execute,
        }
    }

    pub fn rwx() -> Self {
        Self {
            read: true,
            write: true,
            execute: true,
        }
    }

    pub fn rw() -> Self {
        Self {
            read: true,
            write: true,
            execute: false,
        }
    }

    pub fn rx() -> Self {
        Self {
            read: true,
            write: false,
            execute: true,
        }
    }

    pub fn r_only() -> Self {
        Self {
            read: true,
            write: false,
            execute: false,
        }
    }

    pub fn none() -> Self {
        Self {
            read: false,
            write: false,
            execute: false,
        }
    }

    pub fn to_bits(&self) -> u8 {
        let mut bits = 0u8;
        if self.read {
            bits |= 4;
        }
        if self.write {
            bits |= 2;
        }
        if self.execute {
            bits |= 1;
        }
        bits
    }

    pub fn from_bits(bits: u8) -> Self {
        Self {
            read: (bits & 4) != 0,
            write: (bits & 2) != 0,
            execute: (bits & 1) != 0,
        }
    }
}

/// A single ACL entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AclEntry {
    pub tag: AclTag,
    pub qualifier: Option<u32>,
    pub perms: AclPerms,
}

impl AclEntry {
    pub fn new_user_obj(perms: AclPerms) -> Self {
        Self {
            tag: AclTag::UserObj,
            qualifier: None,
            perms,
        }
    }

    pub fn new_user(uid: u32, perms: AclPerms) -> Self {
        Self {
            tag: AclTag::User,
            qualifier: Some(uid),
            perms,
        }
    }

    pub fn new_group_obj(perms: AclPerms) -> Self {
        Self {
            tag: AclTag::GroupObj,
            qualifier: None,
            perms,
        }
    }

    pub fn new_group(gid: u32, perms: AclPerms) -> Self {
        Self {
            tag: AclTag::Group,
            qualifier: Some(gid),
            perms,
        }
    }

    pub fn new_mask(perms: AclPerms) -> Self {
        Self {
            tag: AclTag::Mask,
            qualifier: None,
            perms,
        }
    }

    pub fn new_other(perms: AclPerms) -> Self {
        Self {
            tag: AclTag::Other,
            qualifier: None,
            perms,
        }
    }

    pub fn applies_to(&self, uid: u32, gid: u32) -> bool {
        match self.tag {
            AclTag::UserObj => true,
            AclTag::User => self.qualifier == Some(uid),
            AclTag::GroupObj => true,
            AclTag::Group => self.qualifier == Some(gid),
            AclTag::Mask => false,
            AclTag::Other => false,
        }
    }
}

/// A POSIX ACL (set of entries)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PosixAcl {
    pub entries: Vec<AclEntry>,
}

impl PosixAcl {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn add(&mut self, entry: AclEntry) {
        self.entries.push(entry);
    }

    pub fn remove_tag(&mut self, tag: AclTag) -> usize {
        let initial_len = self.entries.len();
        self.entries.retain(|e| e.tag != tag);
        initial_len - self.entries.len()
    }

    pub fn check_access(&self, uid: u32, gid: u32) -> (bool, bool, bool) {
        let mut can_read = false;
        let mut can_write = false;
        let mut can_execute = false;

        for entry in &self.entries {
            if entry.applies_to(uid, gid) || matches!(entry.tag, AclTag::Other) {
                if entry.perms.read {
                    can_read = true;
                }
                if entry.perms.write {
                    can_write = true;
                }
                if entry.perms.execute {
                    can_execute = true;
                }
            }
            if matches!(entry.tag, AclTag::Mask) {
                for e in &self.entries {
                    if matches!(e.tag, AclTag::Group | AclTag::User) {
                        if e.perms.read {
                            can_read = true;
                        }
                        if e.perms.write {
                            can_write = true;
                        }
                        if e.perms.execute {
                            can_execute = true;
                        }
                    }
                }
            }
        }

        (can_read, can_write, can_execute)
    }

    pub fn is_valid(&self) -> bool {
        let has_user_obj = self.entries.iter().any(|e| e.tag == AclTag::UserObj);
        let has_group_obj = self.entries.iter().any(|e| e.tag == AclTag::GroupObj);
        let has_other = self.entries.iter().any(|e| e.tag == AclTag::Other);
        let has_mask = self.entries.iter().any(|e| e.tag == AclTag::Mask);

        let has_named = self
            .entries
            .iter()
            .any(|e| matches!(e.tag, AclTag::User | AclTag::Group));

        if has_named && !has_mask {
            return false;
        }

        has_user_obj && has_group_obj && has_other
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn by_tag(&self, tag: AclTag) -> Vec<&AclEntry> {
        self.entries.iter().filter(|e| e.tag == tag).collect()
    }

    pub fn to_mode_bits(&self) -> u32 {
        let user_perms = self
            .entries
            .iter()
            .find(|e| e.tag == AclTag::UserObj)
            .map(|e| e.perms.to_bits())
            .unwrap_or(0);

        let group_perms = self
            .entries
            .iter()
            .find(|e| e.tag == AclTag::Mask)
            .or_else(|| self.entries.iter().find(|e| e.tag == AclTag::GroupObj))
            .map(|e| e.perms.to_bits())
            .unwrap_or(0);

        let other_perms = self
            .entries
            .iter()
            .find(|e| e.tag == AclTag::Other)
            .map(|e| e.perms.to_bits())
            .unwrap_or(0);

        ((user_perms as u32) << 6) | ((group_perms as u32) << 3) | (other_perms as u32)
    }
}

impl Default for PosixAcl {
    fn default() -> Self {
        Self::new()
    }
}

/// NFSv4 ACE type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Nfs4AceType {
    Allow,
    Deny,
    Audit,
    Alarm,
}

/// NFSv4 ACE flags
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Nfs4AceFlags {
    pub file_inherit: bool,
    pub dir_inherit: bool,
    pub no_propagate: bool,
    pub inherit_only: bool,
    pub successful_access: bool,
    pub failed_access: bool,
    pub group: bool,
}

impl Nfs4AceFlags {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn file_inherit_only() -> Self {
        Self {
            file_inherit: true,
            inherit_only: true,
            ..Default::default()
        }
    }
}

/// NFSv4 access mask bits
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Nfs4AccessMask {
    pub read_data: bool,
    pub write_data: bool,
    pub execute: bool,
    pub append_data: bool,
    pub read_named_attrs: bool,
    pub write_named_attrs: bool,
    pub delete: bool,
    pub read_acl: bool,
    pub write_acl: bool,
    pub read_attributes: bool,
    pub write_attributes: bool,
}

impl Nfs4AccessMask {
    pub fn read_only() -> Self {
        Self {
            read_data: true,
            read_named_attrs: true,
            read_attributes: true,
            read_acl: true,
            ..Default::default()
        }
    }

    pub fn read_write() -> Self {
        Self {
            read_data: true,
            write_data: true,
            append_data: true,
            read_named_attrs: true,
            write_named_attrs: true,
            read_attributes: true,
            write_attributes: true,
            read_acl: true,
            ..Default::default()
        }
    }

    pub fn full_control() -> Self {
        Self {
            read_data: true,
            write_data: true,
            execute: true,
            append_data: true,
            read_named_attrs: true,
            write_named_attrs: true,
            delete: true,
            read_acl: true,
            write_acl: true,
            read_attributes: true,
            write_attributes: true,
        }
    }

    pub fn to_u32(&self) -> u32 {
        let mut bits = 0u32;
        if self.read_data {
            bits |= 1 << 0;
        }
        if self.write_data {
            bits |= 1 << 1;
        }
        if self.execute {
            bits |= 1 << 2;
        }
        if self.append_data {
            bits |= 1 << 3;
        }
        if self.read_named_attrs {
            bits |= 1 << 4;
        }
        if self.write_named_attrs {
            bits |= 1 << 5;
        }
        if self.delete {
            bits |= 1 << 6;
        }
        if self.read_acl {
            bits |= 1 << 7;
        }
        if self.write_acl {
            bits |= 1 << 8;
        }
        if self.read_attributes {
            bits |= 1 << 9;
        }
        if self.write_attributes {
            bits |= 1 << 10;
        }
        bits
    }

    pub fn from_u32(v: u32) -> Self {
        Self {
            read_data: (v & (1 << 0)) != 0,
            write_data: (v & (1 << 1)) != 0,
            execute: (v & (1 << 2)) != 0,
            append_data: (v & (1 << 3)) != 0,
            read_named_attrs: (v & (1 << 4)) != 0,
            write_named_attrs: (v & (1 << 5)) != 0,
            delete: (v & (1 << 6)) != 0,
            read_acl: (v & (1 << 7)) != 0,
            write_acl: (v & (1 << 8)) != 0,
            read_attributes: (v & (1 << 9)) != 0,
            write_attributes: (v & (1 << 10)) != 0,
        }
    }
}

/// NFSv4 ACE (Access Control Entry)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Nfs4Ace {
    pub ace_type: Nfs4AceType,
    pub flags: Nfs4AceFlags,
    pub access_mask: Nfs4AccessMask,
    pub who: String,
}

impl Nfs4Ace {
    pub fn allow_owner(mask: Nfs4AccessMask) -> Self {
        Self {
            ace_type: Nfs4AceType::Allow,
            flags: Nfs4AceFlags::none(),
            access_mask: mask,
            who: "OWNER@".to_string(),
        }
    }

    pub fn allow_everyone(mask: Nfs4AccessMask) -> Self {
        Self {
            ace_type: Nfs4AceType::Allow,
            flags: Nfs4AceFlags::none(),
            access_mask: mask,
            who: "EVERYONE@".to_string(),
        }
    }

    pub fn deny_everyone(mask: Nfs4AccessMask) -> Self {
        Self {
            ace_type: Nfs4AceType::Deny,
            flags: Nfs4AceFlags::none(),
            access_mask: mask,
            who: "EVERYONE@".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_acl_perms_new() {
        let perms = AclPerms::new(true, false, true);
        assert!(perms.read);
        assert!(!perms.write);
        assert!(perms.execute);
    }

    #[test]
    fn test_acl_perms_rwx() {
        let perms = AclPerms::rwx();
        assert!(perms.read);
        assert!(perms.write);
        assert!(perms.execute);
    }

    #[test]
    fn test_acl_perms_rw() {
        let perms = AclPerms::rw();
        assert!(perms.read);
        assert!(perms.write);
        assert!(!perms.execute);
    }

    #[test]
    fn test_acl_perms_rx() {
        let perms = AclPerms::rx();
        assert!(perms.read);
        assert!(!perms.write);
        assert!(perms.execute);
    }

    #[test]
    fn test_acl_perms_r_only() {
        let perms = AclPerms::r_only();
        assert!(perms.read);
        assert!(!perms.write);
        assert!(!perms.execute);
    }

    #[test]
    fn test_acl_perms_none() {
        let perms = AclPerms::none();
        assert!(!perms.read);
        assert!(!perms.write);
        assert!(!perms.execute);
    }

    #[test]
    fn test_acl_perms_to_bits() {
        assert_eq!(AclPerms::rwx().to_bits(), 7);
        assert_eq!(AclPerms::rw().to_bits(), 6);
        assert_eq!(AclPerms::rx().to_bits(), 5);
        assert_eq!(AclPerms::r_only().to_bits(), 4);
        assert_eq!(AclPerms::none().to_bits(), 0);
    }

    #[test]
    fn test_acl_perms_from_bits() {
        assert_eq!(AclPerms::from_bits(7), AclPerms::rwx());
        assert_eq!(AclPerms::from_bits(6), AclPerms::rw());
        assert_eq!(AclPerms::from_bits(5), AclPerms::rx());
        assert_eq!(AclPerms::from_bits(4), AclPerms::r_only());
        assert_eq!(AclPerms::from_bits(0), AclPerms::none());
    }

    #[test]
    fn test_acl_entry_user_obj() {
        let entry = AclEntry::new_user_obj(AclPerms::rwx());
        assert_eq!(entry.tag, AclTag::UserObj);
        assert_eq!(entry.qualifier, None);
        assert_eq!(entry.perms, AclPerms::rwx());
    }

    #[test]
    fn test_acl_entry_user() {
        let entry = AclEntry::new_user(1000, AclPerms::rw());
        assert_eq!(entry.tag, AclTag::User);
        assert_eq!(entry.qualifier, Some(1000));
    }

    #[test]
    fn test_acl_entry_group_obj() {
        let entry = AclEntry::new_group_obj(AclPerms::rx());
        assert_eq!(entry.tag, AclTag::GroupObj);
        assert_eq!(entry.qualifier, None);
    }

    #[test]
    fn test_acl_entry_group() {
        let entry = AclEntry::new_group(1000, AclPerms::r_only());
        assert_eq!(entry.tag, AclTag::Group);
        assert_eq!(entry.qualifier, Some(1000));
    }

    #[test]
    fn test_acl_entry_mask() {
        let entry = AclEntry::new_mask(AclPerms::rw());
        assert_eq!(entry.tag, AclTag::Mask);
    }

    #[test]
    fn test_acl_entry_other() {
        let entry = AclEntry::new_other(AclPerms::r_only());
        assert_eq!(entry.tag, AclTag::Other);
    }

    #[test]
    fn test_acl_entry_applies_to_user_obj() {
        let entry = AclEntry::new_user_obj(AclPerms::rwx());
        assert!(entry.applies_to(1000, 500));
    }

    #[test]
    fn test_acl_entry_applies_to_user() {
        let entry = AclEntry::new_user(1000, AclPerms::rwx());
        assert!(entry.applies_to(1000, 500));
        assert!(!entry.applies_to(2000, 500));
    }

    #[test]
    fn test_acl_entry_applies_to_group_obj() {
        let entry = AclEntry::new_group_obj(AclPerms::rwx());
        assert!(entry.applies_to(1000, 500));
    }

    #[test]
    fn test_acl_entry_applies_to_group() {
        let entry = AclEntry::new_group(500, AclPerms::rwx());
        assert!(entry.applies_to(1000, 500));
        assert!(!entry.applies_to(1000, 600));
    }

    #[test]
    fn test_acl_entry_applies_to_mask() {
        let entry = AclEntry::new_mask(AclPerms::rwx());
        assert!(!entry.applies_to(1000, 500));
    }

    #[test]
    fn test_posix_acl_new() {
        let acl = PosixAcl::new();
        assert!(acl.is_empty());
    }

    #[test]
    fn test_posix_acl_add() {
        let mut acl = PosixAcl::new();
        acl.add(AclEntry::new_user_obj(AclPerms::rwx()));
        assert_eq!(acl.len(), 1);
    }

    #[test]
    fn test_posix_acl_remove_tag() {
        let mut acl = PosixAcl::new();
        acl.add(AclEntry::new_user_obj(AclPerms::rwx()));
        acl.add(AclEntry::new_group_obj(AclPerms::rw()));
        acl.add(AclEntry::new_other(AclPerms::r_only()));

        let removed = acl.remove_tag(AclTag::GroupObj);
        assert_eq!(removed, 1);
        assert_eq!(acl.len(), 2);
    }

    #[test]
    fn test_posix_acl_check_access() {
        let mut acl = PosixAcl::new();
        acl.add(AclEntry::new_user_obj(AclPerms::rwx()));
        acl.add(AclEntry::new_group_obj(AclPerms::rx()));
        acl.add(AclEntry::new_other(AclPerms::r_only()));

        let (can_read, can_write, can_execute) = acl.check_access(1000, 500);
        assert!(can_read);
        assert!(can_write);
        assert!(can_execute);
    }

    #[test]
    fn test_posix_acl_is_valid() {
        let mut acl = PosixAcl::new();
        acl.add(AclEntry::new_user_obj(AclPerms::rwx()));
        acl.add(AclEntry::new_group_obj(AclPerms::rx()));
        acl.add(AclEntry::new_other(AclPerms::r_only()));
        assert!(acl.is_valid());
    }

    #[test]
    fn test_posix_acl_is_valid_with_named() {
        let mut acl = PosixAcl::new();
        acl.add(AclEntry::new_user_obj(AclPerms::rwx()));
        acl.add(AclEntry::new_group_obj(AclPerms::rx()));
        acl.add(AclEntry::new_user(1000, AclPerms::rw()));
        acl.add(AclEntry::new_mask(AclPerms::rw()));
        acl.add(AclEntry::new_other(AclPerms::r_only()));
        assert!(acl.is_valid());
    }

    #[test]
    fn test_posix_acl_is_valid_missing_mask() {
        let mut acl = PosixAcl::new();
        acl.add(AclEntry::new_user_obj(AclPerms::rwx()));
        acl.add(AclEntry::new_group_obj(AclPerms::rx()));
        acl.add(AclEntry::new_user(1000, AclPerms::rw()));
        acl.add(AclEntry::new_other(AclPerms::r_only()));
        assert!(!acl.is_valid());
    }

    #[test]
    fn test_posix_acl_by_tag() {
        let mut acl = PosixAcl::new();
        acl.add(AclEntry::new_user_obj(AclPerms::rwx()));
        acl.add(AclEntry::new_user(1000, AclPerms::rw()));
        acl.add(AclEntry::new_user(2000, AclPerms::r_only()));

        let users = acl.by_tag(AclTag::User);
        assert_eq!(users.len(), 2);
    }

    #[test]
    fn test_posix_acl_to_mode_bits() {
        let mut acl = PosixAcl::new();
        acl.add(AclEntry::new_user_obj(AclPerms::rwx()));
        acl.add(AclEntry::new_group_obj(AclPerms::rx()));
        acl.add(AclEntry::new_other(AclPerms::r_only()));

        assert_eq!(acl.to_mode_bits(), 0o754);
    }

    #[test]
    fn test_posix_acl_to_mode_bits_with_mask() {
        let mut acl = PosixAcl::new();
        acl.add(AclEntry::new_user_obj(AclPerms::rwx()));
        acl.add(AclEntry::new_mask(AclPerms::rwx()));
        acl.add(AclEntry::new_other(AclPerms::r_only()));

        // User: rwx = 7 -> 7*64 = 448
        // Mask: rwx = 7 -> 7*8 = 56
        // Other: r = 4 -> 4
        // Total = 508 = 0o774
        assert_eq!(acl.to_mode_bits(), 0o774);
    }

    #[test]
    fn test_nfs4_access_mask_read_only() {
        let mask = Nfs4AccessMask::read_only();
        assert!(mask.read_data);
        assert!(mask.read_named_attrs);
        assert!(mask.read_attributes);
        assert!(mask.read_acl);
        assert!(!mask.write_data);
    }

    #[test]
    fn test_nfs4_access_mask_read_write() {
        let mask = Nfs4AccessMask::read_write();
        assert!(mask.read_data);
        assert!(mask.write_data);
        assert!(mask.append_data);
        assert!(!mask.delete);
    }

    #[test]
    fn test_nfs4_access_mask_full_control() {
        let mask = Nfs4AccessMask::full_control();
        assert!(mask.read_data);
        assert!(mask.write_data);
        assert!(mask.execute);
        assert!(mask.delete);
        assert!(mask.read_acl);
        assert!(mask.write_acl);
    }

    #[test]
    fn test_nfs4_access_mask_to_u32() {
        let mask = Nfs4AccessMask::full_control();
        let bits = mask.to_u32();
        assert!(bits & 1 != 0);
        assert!(bits & (1 << 1) != 0);
    }

    #[test]
    fn test_nfs4_access_mask_from_u32() {
        let mask = Nfs4AccessMask::from_u32(0x7FF);
        assert!(mask.read_data);
        assert!(mask.write_data);
        assert!(mask.execute);
        assert!(mask.read_acl);
    }

    #[test]
    fn test_nfs4_ace_allow_owner() {
        let ace = Nfs4Ace::allow_owner(Nfs4AccessMask::read_only());
        assert_eq!(ace.ace_type, Nfs4AceType::Allow);
        assert_eq!(ace.who, "OWNER@");
    }

    #[test]
    fn test_nfs4_ace_allow_everyone() {
        let ace = Nfs4Ace::allow_everyone(Nfs4AccessMask::read_write());
        assert_eq!(ace.ace_type, Nfs4AceType::Allow);
        assert_eq!(ace.who, "EVERYONE@");
    }

    #[test]
    fn test_nfs4_ace_deny_everyone() {
        let ace = Nfs4Ace::deny_everyone(Nfs4AccessMask::full_control());
        assert_eq!(ace.ace_type, Nfs4AceType::Deny);
        assert_eq!(ace.who, "EVERYONE@");
    }
}
