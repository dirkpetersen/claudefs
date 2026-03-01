#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AclTag {
    UserObj,
    User(u32),
    GroupObj,
    Group(u32),
    Mask,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AclPerms {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

impl AclPerms {
    pub fn from_bits(bits: u8) -> Self {
        AclPerms {
            read: bits & 0x04 != 0,
            write: bits & 0x02 != 0,
            execute: bits & 0x01 != 0,
        }
    }

    pub fn to_bits(&self) -> u8 {
        (if self.read { 0x04 } else { 0 })
            | (if self.write { 0x02 } else { 0 })
            | (if self.execute { 0x01 } else { 0 })
    }

    pub fn all() -> Self {
        AclPerms {
            read: true,
            write: true,
            execute: true,
        }
    }

    pub fn none() -> Self {
        AclPerms {
            read: false,
            write: false,
            execute: false,
        }
    }

    pub fn read_only() -> Self {
        AclPerms {
            read: true,
            write: false,
            execute: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AclEntry {
    pub tag: AclTag,
    pub perms: AclPerms,
}

impl AclEntry {
    pub fn new(tag: AclTag, perms: AclPerms) -> Self {
        AclEntry { tag, perms }
    }

    pub fn is_named(&self) -> bool {
        matches!(self.tag, AclTag::User(_) | AclTag::Group(_))
    }
}

pub struct PosixAcl {
    entries: Vec<AclEntry>,
}

impl PosixAcl {
    pub fn new() -> Self {
        PosixAcl {
            entries: Vec::new(),
        }
    }

    pub fn add_entry(&mut self, entry: AclEntry) {
        self.entries.push(entry);
    }

    pub fn check_access(
        &self,
        uid: u32,
        file_uid: u32,
        gid: u32,
        file_gid: u32,
        req: AclPerms,
    ) -> bool {
        if uid == file_uid {
            if let Some(entry) = self.entries.iter().find(|e| e.tag == AclTag::UserObj) {
                let eff = self.effective_perms(entry.perms);
                return eff.read >= req.read
                    && eff.write >= req.write
                    && eff.execute >= req.execute;
            }
        }

        for entry in &self.entries {
            if let AclTag::User(uid_) = entry.tag {
                if uid == uid_ {
                    let eff = self.effective_perms(entry.perms);
                    return eff.read >= req.read
                        && eff.write >= req.write
                        && eff.execute >= req.execute;
                }
            }
        }

        let mut group_match = false;
        if gid == file_gid {
            if let Some(entry) = self.entries.iter().find(|e| e.tag == AclTag::GroupObj) {
                group_match = true;
                let eff = self.effective_perms(entry.perms);
                if !(eff.read >= req.read && eff.write >= req.write && eff.execute >= req.execute) {
                    return false;
                }
            }
        }

        for entry in &self.entries {
            if let AclTag::Group(gid_) = entry.tag {
                if gid == gid_ {
                    group_match = true;
                    let eff = self.effective_perms(entry.perms);
                    if !(eff.read >= req.read
                        && eff.write >= req.write
                        && eff.execute >= req.execute)
                    {
                        return false;
                    }
                }
            }
        }

        if group_match {
            return true;
        }

        if let Some(entry) = self.entries.iter().find(|e| e.tag == AclTag::Other) {
            let eff = self.effective_perms(entry.perms);
            return eff.read >= req.read && eff.write >= req.write && eff.execute >= req.execute;
        }

        false
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn has_mask(&self) -> bool {
        self.entries.iter().any(|e| e.tag == AclTag::Mask)
    }

    pub fn effective_perms(&self, entry_perms: AclPerms) -> AclPerms {
        if let Some(mask) = self.entries.iter().find(|e| e.tag == AclTag::Mask) {
            AclPerms {
                read: entry_perms.read && mask.perms.read,
                write: entry_perms.write && mask.perms.write,
                execute: entry_perms.execute && mask.perms.execute,
            }
        } else {
            entry_perms
        }
    }

    pub fn entries_for_tag(&self, tag: AclTag) -> Vec<&AclEntry> {
        self.entries.iter().filter(|e| e.tag == tag).collect()
    }
}

impl Default for PosixAcl {
    fn default() -> Self {
        Self::new()
    }
}

pub const XATTR_POSIX_ACL_ACCESS: &str = "system.posix_acl_access";
pub const XATTR_POSIX_ACL_DEFAULT: &str = "system.posix_acl_default";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_acl_perms_from_bits() {
        let perms = AclPerms::from_bits(0x07);
        assert!(perms.read);
        assert!(perms.write);
        assert!(perms.execute);

        let perms = AclPerms::from_bits(0x04);
        assert!(perms.read);
        assert!(!perms.write);
        assert!(!perms.execute);

        let perms = AclPerms::from_bits(0x00);
        assert!(!perms.read);
        assert!(!perms.write);
        assert!(!perms.execute);
    }

    #[test]
    fn test_acl_perms_to_bits() {
        let perms = AclPerms::all();
        assert_eq!(perms.to_bits(), 0x07);

        let perms = AclPerms::read_only();
        assert_eq!(perms.to_bits(), 0x04);

        let perms = AclPerms::none();
        assert_eq!(perms.to_bits(), 0x00);
    }

    #[test]
    fn test_acl_perms_roundtrip() {
        for bits in 0..=7 {
            let perms = AclPerms::from_bits(bits);
            assert_eq!(perms.to_bits(), bits);
        }
    }

    #[test]
    fn test_acl_perms_all() {
        let perms = AclPerms::all();
        assert!(perms.read);
        assert!(perms.write);
        assert!(perms.execute);
    }

    #[test]
    fn test_acl_perms_none() {
        let perms = AclPerms::none();
        assert!(!perms.read);
        assert!(!perms.write);
        assert!(!perms.execute);
    }

    #[test]
    fn test_acl_perms_read_only() {
        let perms = AclPerms::read_only();
        assert!(perms.read);
        assert!(!perms.write);
        assert!(!perms.execute);
    }

    #[test]
    fn test_acl_entry_is_named() {
        let entry = AclEntry::new(AclTag::UserObj, AclPerms::all());
        assert!(!entry.is_named());

        let entry = AclEntry::new(AclTag::User(1000), AclPerms::all());
        assert!(entry.is_named());

        let entry = AclEntry::new(AclTag::GroupObj, AclPerms::all());
        assert!(!entry.is_named());

        let entry = AclEntry::new(AclTag::Group(1000), AclPerms::all());
        assert!(entry.is_named());

        let entry = AclEntry::new(AclTag::Mask, AclPerms::all());
        assert!(!entry.is_named());

        let entry = AclEntry::new(AclTag::Other, AclPerms::all());
        assert!(!entry.is_named());
    }

    #[test]
    fn test_posix_acl_add_entry() {
        let mut acl = PosixAcl::new();
        acl.add_entry(AclEntry::new(AclTag::UserObj, AclPerms::all()));
        assert_eq!(acl.entry_count(), 1);
    }

    #[test]
    fn test_posix_acl_entry_count() {
        let mut acl = PosixAcl::new();
        assert_eq!(acl.entry_count(), 0);

        acl.add_entry(AclEntry::new(AclTag::UserObj, AclPerms::all()));
        acl.add_entry(AclEntry::new(AclTag::GroupObj, AclPerms::all()));
        acl.add_entry(AclEntry::new(AclTag::Other, AclPerms::all()));
        assert_eq!(acl.entry_count(), 3);
    }

    #[test]
    fn test_check_access_owner_allowed() {
        let mut acl = PosixAcl::new();
        acl.add_entry(AclEntry::new(AclTag::UserObj, AclPerms::all()));
        acl.add_entry(AclEntry::new(AclTag::GroupObj, AclPerms::none()));
        acl.add_entry(AclEntry::new(AclTag::Other, AclPerms::none()));

        assert!(acl.check_access(1000, 1000, 100, 100, AclPerms::read_only()));
    }

    #[test]
    fn test_check_access_other_denied() {
        let mut acl = PosixAcl::new();
        acl.add_entry(AclEntry::new(AclTag::UserObj, AclPerms::none()));
        acl.add_entry(AclEntry::new(AclTag::GroupObj, AclPerms::none()));
        acl.add_entry(AclEntry::new(AclTag::Other, AclPerms::none()));

        assert!(!acl.check_access(2000, 1000, 200, 100, AclPerms::read_only()));
    }

    #[test]
    fn test_check_access_named_user_allowed() {
        let mut acl = PosixAcl::new();
        acl.add_entry(AclEntry::new(AclTag::UserObj, AclPerms::none()));
        acl.add_entry(AclEntry::new(AclTag::User(2000), AclPerms::all()));
        acl.add_entry(AclEntry::new(AclTag::GroupObj, AclPerms::none()));
        acl.add_entry(AclEntry::new(AclTag::Other, AclPerms::none()));

        assert!(acl.check_access(2000, 1000, 100, 100, AclPerms::read_only()));
    }

    #[test]
    fn test_check_access_named_user_denied_without_match() {
        let mut acl = PosixAcl::new();
        acl.add_entry(AclEntry::new(AclTag::UserObj, AclPerms::none()));
        acl.add_entry(AclEntry::new(AclTag::User(3000), AclPerms::all()));
        acl.add_entry(AclEntry::new(AclTag::GroupObj, AclPerms::none()));
        acl.add_entry(AclEntry::new(AclTag::Other, AclPerms::none()));

        assert!(!acl.check_access(2000, 1000, 100, 100, AclPerms::read_only()));
    }

    #[test]
    fn test_mask_limits_named_group() {
        let mut acl = PosixAcl::new();
        acl.add_entry(AclEntry::new(AclTag::UserObj, AclPerms::all()));
        acl.add_entry(AclEntry::new(AclTag::Group(100), AclPerms::all()));
        acl.add_entry(AclEntry::new(AclTag::Mask, AclPerms::read_only()));
        acl.add_entry(AclEntry::new(AclTag::Other, AclPerms::none()));

        assert!(!acl.check_access(2000, 1000, 100, 100, AclPerms::all()));
    }

    #[test]
    fn test_has_mask() {
        let mut acl = PosixAcl::new();
        assert!(!acl.has_mask());

        acl.add_entry(AclEntry::new(AclTag::UserObj, AclPerms::all()));
        assert!(!acl.has_mask());

        acl.add_entry(AclEntry::new(AclTag::Mask, AclPerms::all()));
        assert!(acl.has_mask());
    }

    #[test]
    fn test_effective_perms_with_mask() {
        let mut acl = PosixAcl::new();
        acl.add_entry(AclEntry::new(AclTag::Mask, AclPerms::read_only()));

        let eff = acl.effective_perms(AclPerms::all());
        assert!(eff.read);
        assert!(!eff.write);
        assert!(!eff.execute);
    }

    #[test]
    fn test_effective_perms_without_mask() {
        let mut acl = PosixAcl::new();
        acl.add_entry(AclEntry::new(AclTag::UserObj, AclPerms::all()));

        let eff = acl.effective_perms(AclPerms::all());
        assert!(eff.read);
        assert!(eff.write);
        assert!(eff.execute);
    }

    #[test]
    fn test_entries_for_tag_filters_correctly() {
        let mut acl = PosixAcl::new();
        acl.add_entry(AclEntry::new(AclTag::UserObj, AclPerms::all()));
        acl.add_entry(AclEntry::new(AclTag::User(1000), AclPerms::read_only()));
        acl.add_entry(AclEntry::new(AclTag::User(2000), AclPerms::all()));
        acl.add_entry(AclEntry::new(AclTag::GroupObj, AclPerms::none()));

        let user_entries = acl.entries_for_tag(AclTag::UserObj);
        assert_eq!(user_entries.len(), 1);

        let named_users = acl.entries_for_tag(AclTag::User(1000));
        assert_eq!(named_users.len(), 1);

        let group_entries = acl.entries_for_tag(AclTag::GroupObj);
        assert_eq!(group_entries.len(), 1);
    }

    #[test]
    fn test_xattr_constants() {
        assert_eq!(XATTR_POSIX_ACL_ACCESS, "system.posix_acl_access");
        assert_eq!(XATTR_POSIX_ACL_DEFAULT, "system.posix_acl_default");
    }

    #[test]
    fn test_group_match() {
        let mut acl = PosixAcl::new();
        acl.add_entry(AclEntry::new(AclTag::UserObj, AclPerms::none()));
        acl.add_entry(AclEntry::new(AclTag::GroupObj, AclPerms::all()));
        acl.add_entry(AclEntry::new(AclTag::Other, AclPerms::none()));

        assert!(acl.check_access(2000, 1000, 100, 100, AclPerms::read_only()));
    }

    #[test]
    fn test_named_group_match() {
        let mut acl = PosixAcl::new();
        acl.add_entry(AclEntry::new(AclTag::UserObj, AclPerms::none()));
        acl.add_entry(AclEntry::new(AclTag::Group(100), AclPerms::all()));
        acl.add_entry(AclEntry::new(AclTag::Other, AclPerms::none()));

        assert!(acl.check_access(2000, 1000, 100, 200, AclPerms::read_only()));
    }
}
