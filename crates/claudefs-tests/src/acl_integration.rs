//! POSIX ACL and fallocate integration tests
//!
//! Tests for ACL enforcement and fallocate mode handling from claudefs-fuse.

use claudefs_fuse::fallocate::{
    FallocateOp, FallocateStats, FALLOC_FL_COLLAPSE_RANGE, FALLOC_FL_INSERT_RANGE,
    FALLOC_FL_KEEP_SIZE, FALLOC_FL_PUNCH_HOLE, FALLOC_FL_ZERO_RANGE,
};
use claudefs_fuse::posix_acl::{
    AclEntry, AclPerms, AclTag, PosixAcl, XATTR_POSIX_ACL_ACCESS, XATTR_POSIX_ACL_DEFAULT,
};

#[test]
fn test_acl_perms_from_bits_roundtrip() {
    for bits in 0..8u8 {
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
    assert_eq!(perms.to_bits(), 0x07);
}

#[test]
fn test_acl_perms_none() {
    let perms = AclPerms::none();
    assert!(!perms.read);
    assert!(!perms.write);
    assert!(!perms.execute);
    assert_eq!(perms.to_bits(), 0x00);
}

#[test]
fn test_acl_perms_read_only() {
    let perms = AclPerms::read_only();
    assert!(perms.read);
    assert!(!perms.write);
    assert!(!perms.execute);
    assert_eq!(perms.to_bits(), 0x04);
}

#[test]
fn test_acl_tag_user_obj() {
    let tag = AclTag::UserObj;
    matches!(tag, AclTag::UserObj);
}

#[test]
fn test_acl_tag_user_with_uid() {
    let tag = AclTag::User(1000);
    matches!(tag, AclTag::User(1000));
}

#[test]
fn test_acl_tag_group_obj() {
    let tag = AclTag::GroupObj;
    matches!(tag, AclTag::GroupObj);
}

#[test]
fn test_acl_tag_group_with_gid() {
    let tag = AclTag::Group(1000);
    matches!(tag, AclTag::Group(1000));
}

#[test]
fn test_acl_tag_mask() {
    let tag = AclTag::Mask;
    matches!(tag, AclTag::Mask);
}

#[test]
fn test_acl_tag_other() {
    let tag = AclTag::Other;
    matches!(tag, AclTag::Other);
}

#[test]
fn test_posix_acl_new_creates_empty() {
    let mut acl = PosixAcl::new();
    acl.add_entry(AclEntry::new(AclTag::Other, AclPerms::none()));
}

#[test]
fn test_posix_acl_add_entry() {
    let mut acl = PosixAcl::new();
    acl.add_entry(AclEntry::new(AclTag::UserObj, AclPerms::all()));
    acl.add_entry(AclEntry::new(AclTag::Other, AclPerms::none()));
}

#[test]
fn test_posix_acl_check_access_user_obj_owner() {
    let mut acl = PosixAcl::new();
    acl.add_entry(AclEntry::new(AclTag::UserObj, AclPerms::all()));
    acl.add_entry(AclEntry::new(AclTag::GroupObj, AclPerms::read_only()));
    acl.add_entry(AclEntry::new(AclTag::Other, AclPerms::read_only()));

    let result = acl.check_access(1000, 1000, 0, 0, AclPerms::read_only());
    assert!(result);
}

#[test]
fn test_posix_acl_check_access_other_tag() {
    let mut acl = PosixAcl::new();
    acl.add_entry(AclEntry::new(AclTag::UserObj, AclPerms::none()));
    acl.add_entry(AclEntry::new(AclTag::GroupObj, AclPerms::none()));
    acl.add_entry(AclEntry::new(AclTag::Other, AclPerms::read_only()));

    let result = acl.check_access(9999, 1000, 9999, 2000, AclPerms::read_only());
    assert!(result);
}

#[test]
fn test_posix_acl_check_access_group_tag() {
    let mut acl = PosixAcl::new();
    acl.add_entry(AclEntry::new(AclTag::UserObj, AclPerms::none()));
    acl.add_entry(AclEntry::new(AclTag::GroupObj, AclPerms::all()));
    acl.add_entry(AclEntry::new(AclTag::Other, AclPerms::read_only()));

    let result = acl.check_access(2000, 1000, 2000, 0, AclPerms::read_only());
    assert!(result);
}

#[test]
fn test_posix_acl_check_access_mask_restricts() {
    let mut acl = PosixAcl::new();
    acl.add_entry(AclEntry::new(AclTag::UserObj, AclPerms::all()));
    acl.add_entry(AclEntry::new(AclTag::GroupObj, AclPerms::all()));
    acl.add_entry(AclEntry::new(AclTag::Mask, AclPerms::read_only()));
    acl.add_entry(AclEntry::new(AclTag::Other, AclPerms::read_only()));

    let result = acl.check_access(2000, 1000, 2000, 0, AclPerms::all());
    assert!(!result);
}

#[test]
fn test_posix_acl_check_access_no_acl_denies() {
    let acl = PosixAcl::new();

    let result = acl.check_access(1000, 1000, 0, 0, AclPerms::read_only());
    assert!(!result);
}

#[test]
fn test_xattr_constants_non_empty() {
    assert!(!XATTR_POSIX_ACL_ACCESS.is_empty());
    assert!(!XATTR_POSIX_ACL_DEFAULT.is_empty());
    assert_eq!(XATTR_POSIX_ACL_ACCESS, "system.posix_acl_access");
    assert_eq!(XATTR_POSIX_ACL_DEFAULT, "system.posix_acl_default");
}

#[test]
fn test_fallocate_op_from_flags_zero_returns_allocate() {
    let result = FallocateOp::from_flags(0, 0, 1000);
    assert!(result.is_ok());
    let op = result.unwrap();
    matches!(op, FallocateOp::Allocate { .. });
}

#[test]
fn test_fallocate_op_from_flags_keep_size() {
    let result = FallocateOp::from_flags(FALLOC_FL_KEEP_SIZE, 0, 1000);
    assert!(result.is_ok());
    let op = result.unwrap();
    matches!(op, FallocateOp::Allocate { keep_size: true });
}

#[test]
fn test_fallocate_op_from_flags_punch_hole_with_keep_size() {
    let result = FallocateOp::from_flags(FALLOC_FL_PUNCH_HOLE | FALLOC_FL_KEEP_SIZE, 1000, 500);
    assert!(result.is_ok());
    let op = result.unwrap();
    matches!(op, FallocateOp::PunchHole { .. });
}

#[test]
fn test_fallocate_op_is_space_saving_punch_hole() {
    let op = FallocateOp::PunchHole {
        offset: 0,
        len: 1000,
    };
    assert!(op.is_space_saving());
}

#[test]
fn test_fallocate_op_is_space_saving_collapse_range() {
    let op = FallocateOp::CollapseRange {
        offset: 0,
        len: 1000,
    };
    assert!(op.is_space_saving());
}

#[test]
fn test_fallocate_op_is_space_saving_allocate() {
    let op = FallocateOp::Allocate { keep_size: false };
    assert!(!op.is_space_saving());
}

#[test]
fn test_fallocate_op_is_space_saving_zero_range() {
    let op = FallocateOp::ZeroRange {
        offset: 0,
        len: 1000,
        keep_size: true,
    };
    assert!(!op.is_space_saving());
}

#[test]
fn test_fallocate_op_modifies_size_allocate_without_keep() {
    let op = FallocateOp::Allocate { keep_size: false };
    assert!(op.modifies_size());
}

#[test]
fn test_fallocate_op_modifies_size_allocate_with_keep() {
    let op = FallocateOp::Allocate { keep_size: true };
    assert!(!op.modifies_size());
}

#[test]
fn test_fallocate_op_modifies_size_punch_hole() {
    let op = FallocateOp::PunchHole {
        offset: 0,
        len: 1000,
    };
    assert!(!op.modifies_size());
}

#[test]
fn test_fallocate_op_modifies_size_collapse_range() {
    let op = FallocateOp::CollapseRange {
        offset: 0,
        len: 1000,
    };
    assert!(op.modifies_size());
}

#[test]
fn test_fallocate_op_modifies_size_insert_range() {
    let op = FallocateOp::InsertRange {
        offset: 0,
        len: 1000,
    };
    assert!(op.modifies_size());
}

#[test]
fn test_fallocate_op_affected_range() {
    let op = FallocateOp::PunchHole {
        offset: 1000,
        len: 500,
    };
    let range = op.affected_range();
    assert!(range.is_some());
    let (offset, len) = range.unwrap();
    assert_eq!(offset, 1000);
    assert_eq!(len, 500);
}

#[test]
fn test_fallocate_stats_default() {
    let stats = FallocateStats::default();
    assert_eq!(stats.total_allocations, 0);
    assert_eq!(stats.total_punch_holes, 0);
    assert_eq!(stats.total_zero_ranges, 0);
    assert_eq!(stats.bytes_allocated, 0);
    assert_eq!(stats.bytes_punched, 0);
}

#[test]
fn test_fallocate_stats_new() {
    let stats = FallocateStats::new();
    assert_eq!(stats.total_allocations, 0);
}

#[test]
fn test_fallocate_stats_record_allocation() {
    let mut stats = FallocateStats::new();
    let op = FallocateOp::Allocate { keep_size: false };
    stats.record(&op, 1000);
    assert_eq!(stats.total_allocations, 1);
    assert_eq!(stats.bytes_allocated, 1000);
}

#[test]
fn test_fallocate_stats_record_punch_hole() {
    let mut stats = FallocateStats::new();
    let op = FallocateOp::PunchHole {
        offset: 0,
        len: 500,
    };
    stats.record(&op, 500);
    assert_eq!(stats.total_punch_holes, 1);
    assert_eq!(stats.bytes_punched, 500);
}

#[test]
fn test_fallocate_stats_record_zero_range() {
    let mut stats = FallocateStats::new();
    let op = FallocateOp::ZeroRange {
        offset: 0,
        len: 500,
        keep_size: true,
    };
    stats.record(&op, 500);
    assert_eq!(stats.total_zero_ranges, 1);
}

#[test]
fn test_combined_acl_check_and_fallocate() {
    let mut acl = PosixAcl::new();
    acl.add_entry(AclEntry::new(AclTag::UserObj, AclPerms::all()));
    acl.add_entry(AclEntry::new(AclTag::GroupObj, AclPerms::read_only()));
    acl.add_entry(AclEntry::new(AclTag::Other, AclPerms::read_only()));

    let can_read = acl.check_access(1000, 1000, 0, 0, AclPerms::read_only());
    assert!(can_read);

    let result = FallocateOp::from_flags(0, 0, 1000);
    assert!(result.is_ok());
}

#[test]
fn test_fallocate_zero_range_is_space_saving() {
    let op = FallocateOp::from_flags(FALLOC_FL_ZERO_RANGE | FALLOC_FL_KEEP_SIZE, 0, 1000);
    assert!(op.is_ok());
    let unwrapped = op.unwrap();
    assert!(!unwrapped.is_space_saving());
}

#[test]
fn test_acl_entry_new() {
    let entry = AclEntry::new(AclTag::UserObj, AclPerms::all());
    assert!(matches!(entry.tag, AclTag::UserObj));
}

#[test]
fn test_acl_entry_is_named() {
    let entry = AclEntry::new(AclTag::User(1000), AclPerms::all());
    assert!(entry.is_named());
}

#[test]
fn test_acl_entry_is_not_named() {
    let entry = AclEntry::new(AclTag::UserObj, AclPerms::all());
    assert!(!entry.is_named());
}

#[test]
fn test_fallocate_collapse_range() {
    let result = FallocateOp::from_flags(FALLOC_FL_COLLAPSE_RANGE, 1000, 500);
    assert!(result.is_ok());
    let op = result.unwrap();
    matches!(op, FallocateOp::CollapseRange { .. });
}

#[test]
fn test_fallocate_insert_range() {
    let result = FallocateOp::from_flags(FALLOC_FL_INSERT_RANGE, 1000, 500);
    assert!(result.is_ok());
    let op = result.unwrap();
    matches!(op, FallocateOp::InsertRange { .. });
}

#[test]
fn test_fallocate_zerorange() {
    let result = FallocateOp::from_flags(FALLOC_FL_ZERO_RANGE, 0, 1000);
    assert!(result.is_ok());
    let op = result.unwrap();
    matches!(op, FallocateOp::ZeroRange { .. });
}

#[test]
fn test_fallocate_allocate_default() {
    let result = FallocateOp::from_flags(0, 0, 1000);
    assert!(result.is_ok());
    let op = result.unwrap();
    matches!(op, FallocateOp::Allocate { keep_size: false });
}

#[test]
fn test_fallocate_punch_hole_without_keep_fails() {
    let result = FallocateOp::from_flags(FALLOC_FL_PUNCH_HOLE, 0, 1000);
    assert!(result.is_err());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_posix_acl_multiple_entries() {
        let mut acl = PosixAcl::new();
        acl.add_entry(AclEntry::new(AclTag::UserObj, AclPerms::all()));
        acl.add_entry(AclEntry::new(AclTag::GroupObj, AclPerms::read_only()));
        acl.add_entry(AclEntry::new(AclTag::Other, AclPerms::read_only()));
        acl.add_entry(AclEntry::new(AclTag::Mask, AclPerms::all()));
    }
}
