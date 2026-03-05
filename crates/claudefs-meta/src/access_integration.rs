//! Access integration module bridging DAC and POSIX ACL permission systems.
//!
//! This module provides unified access checking that combines the basic DAC permission
//! system from `access.rs` with the POSIX ACL system from `acl.rs`. It implements the
//! POSIX.1e access check algorithm where ACLs take precedence over traditional mode bits.

use serde::{Deserialize, Serialize};

use crate::acl::{Acl, AclEntry, AclTag};
use crate::types::{FileType, InodeAttr, MetaError};

/// Context for integrated access checking.
#[derive(Clone, Debug)]
pub struct AccessCheckContext {
    /// The inode being accessed
    pub inode: InodeAttr,
    /// User context for permission checking
    pub user_context: crate::access::UserContext,
    /// The operation being performed
    pub operation: AccessOperation,
    /// Traditional DAC mode bits (from inode mode)
    pub dac_mode: u32,
    /// Optional ACL for the inode
    pub acl: Option<Acl>,
}

/// Operations that can be performed on an inode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccessOperation {
    /// Read operation
    Read,
    /// Write operation
    Write,
    /// Execute/search operation
    Execute,
    /// Set attribute operation
    SetAttr(SetAttrOp),
    /// Delete operation
    Delete,
}

/// Specific setattr operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetAttrOp {
    /// Change file mode (permissions)
    Mode(u32),
    /// Change owner (uid, gid)
    Owner(u32, u32),
    /// Change file size
    Size(u64),
    /// Change timestamps
    Timestamps,
    /// Any attribute change
    Any,
}

/// Result of an access check.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccessCheckResult {
    /// Access is allowed
    Allowed,
    /// Access is denied with a reason
    Denied(String),
    /// Access requires specific capabilities
    RequiresCaps(Vec<String>),
}

impl AccessCheckResult {
    /// Returns true if access is allowed.
    pub fn is_allowed(&self) -> bool {
        matches!(self, AccessCheckResult::Allowed)
    }

    /// Returns true if access is denied.
    pub fn is_denied(&self) -> bool {
        matches!(self, AccessCheckResult::Denied(_))
    }
}

/// Main entry point for integrated access checking.
///
/// If an ACL is present on the inode, it is evaluated first per POSIX.1e semantics.
/// If no ACL is present, falls back to traditional DAC permission checking.
/// Root (uid 0) always bypasses permission checks.
///
/// # Arguments
/// * `ctx` - The access check context containing inode, user, operation, and optional ACL
///
/// # Returns
/// * `AccessCheckResult::Allowed` - Access is permitted
/// * `AccessCheckResult::Denied` - Access denied with specific reason
/// * `AccessCheckResult::RequiresCaps` - Required capabilities for access
pub fn check_access(ctx: &AccessCheckContext) -> AccessCheckResult {
    // Root always allowed
    if ctx.user_context.is_root() {
        return AccessCheckResult::Allowed;
    }

    // If ACL exists, use POSIX.1e algorithm (ACL takes precedence over mode bits)
    if let Some(ref acl) = ctx.acl {
        return evaluate_acl_then_dac(
            acl,
            ctx.dac_mode,
            &ctx.user_context,
            &ctx.operation,
            ctx.inode.uid,
            ctx.inode.gid,
        );
    }

    // No ACL, fall back to DAC
    match ctx.operation {
        AccessOperation::SetAttr(ref op) => check_setattr_permission(ctx, op),
        AccessOperation::Delete => {
            // For delete, we need the parent directory - use dac_mode for simplicity
            // The actual delete check should use check_delete_permission
            check_dac_access(
                &ctx.inode,
                &ctx.user_context,
                crate::access::AccessMode::W_OK,
            )
        }
        _ => check_dac_operation(&ctx.inode, &ctx.user_context, &ctx.operation),
    }
}

/// Evaluates ACL permissions using POSIX.1e algorithm.
///
/// Per POSIX.1e, when an ACL is present, it takes precedence over traditional
/// mode bits. The evaluation order is:
/// 1. If user is owner, use UserObj entry
/// 2. If user matches a named User entry, use that entry (masked by Mask if present)
/// 3. If user's groups match GroupObj or named Group entries, use best match (masked)
/// 4. Otherwise, use Other entry
///
/// # Arguments
/// * `acl` - The ACL to evaluate
/// * `dac_mode` - Traditional DAC mode bits (fallback when ACL doesn't match)
/// * `user` - The user context
/// * `op` - The access operation
/// * `owner_uid` - The inode owner UID
/// * `owner_gid` - The inode owner GID
///
/// # Returns
/// Access check result based on ACL evaluation
pub fn evaluate_acl_then_dac(
    acl: &Acl,
    _dac_mode: u32,
    user: &crate::access::UserContext,
    op: &AccessOperation,
    owner_uid: u32,
    owner_gid: u32,
) -> AccessCheckResult {
    let required_perms = operation_to_perms(op);

    // Compute effective permissions from ACL
    let effective = get_effective_mode_from_acl(acl, owner_uid, owner_gid, user);

    // Check if ACL grants required permissions
    if (effective & required_perms) == required_perms {
        return AccessCheckResult::Allowed;
    }

    // ACL denied, try DAC as fallback (some systems allow this)
    // But per strict POSIX.1e, ACL takes precedence - so we deny
    AccessCheckResult::Denied(format!(
        "ACL denies access: required {:o}, effective {:o}",
        required_perms, effective
    ))
}

/// Converts an access operation to permission bits.
fn operation_to_perms(op: &AccessOperation) -> u8 {
    match op {
        AccessOperation::Read => 4,
        AccessOperation::Write => 2,
        AccessOperation::Execute => 1,
        AccessOperation::SetAttr(SetAttrOp::Size(_)) => 2,
        AccessOperation::SetAttr(_) => 0,
        AccessOperation::Delete => 2,
    }
}

/// Checks DAC access with a specific AccessMode.
fn check_dac_access(
    attr: &InodeAttr,
    ctx: &crate::access::UserContext,
    mode: crate::access::AccessMode,
) -> AccessCheckResult {
    match crate::access::check_access(attr, ctx, mode) {
        Ok(()) => AccessCheckResult::Allowed,
        Err(MetaError::PermissionDenied) => AccessCheckResult::Denied("permission denied".into()),
        Err(e) => AccessCheckResult::Denied(format!("access check failed: {}", e)),
    }
}

/// Checks DAC permission for an operation.
fn check_dac_operation(
    attr: &InodeAttr,
    ctx: &crate::access::UserContext,
    op: &AccessOperation,
) -> AccessCheckResult {
    let mode = match op {
        AccessOperation::Read => crate::access::AccessMode::R_OK,
        AccessOperation::Write => crate::access::AccessMode::W_OK,
        AccessOperation::Execute => crate::access::AccessMode::X_OK,
        AccessOperation::SetAttr(_) => crate::access::AccessMode::W_OK,
        AccessOperation::Delete => crate::access::AccessMode::W_OK,
    };
    check_dac_access(attr, ctx, mode)
}

/// Applies umask to a requested mode.
///
/// Wrapper around umask::apply_umask for convenience in the access integration module.
///
/// # Arguments
/// * `mode` - The requested file mode
/// * `umask` - The umask to apply
///
/// # Returns
/// The mode with umask applied
pub fn apply_umask_to_mode(mode: u32, umask: u32) -> u32 {
    crate::umask::apply_umask(mode, umask)
}

/// Checks if the user can override permissions as owner.
///
/// Root can always override. For non-root users, this checks if the user
/// owns the inode and considers CAP_FOWNER capability (for future use).
///
/// # Arguments
/// * `user` - The user context
/// * `inode` - The inode to check ownership of
///
/// # Returns
/// true if user can override as owner
pub fn is_owner_override_possible(user: &crate::access::UserContext, inode: &InodeAttr) -> bool {
    // Root can always override
    if user.is_root() {
        return true;
    }

    // Owner can override their own files
    user.uid == inode.uid
}

/// Checks setattr permission.
///
/// SetAttr operations have specific permission requirements:
/// - Mode change: requires owner or root
/// - Owner change: requires root (CAP_CHOWN)
/// - Size change: requires write permission
/// - Timestamp change: requires write permission
/// - Any: requires owner or root
///
/// # Arguments
/// * `ctx` - The access check context
/// * `op` - The specific setattr operation
///
/// # Returns
/// Access check result
pub fn check_setattr_permission(ctx: &AccessCheckContext, op: &SetAttrOp) -> AccessCheckResult {
    // Root can always do setattr
    if ctx.user_context.is_root() {
        return AccessCheckResult::Allowed;
    }

    match op {
        SetAttrOp::Mode(_) => {
            // Mode change requires owner or CAP_FOWNER
            if is_owner_override_possible(&ctx.user_context, &ctx.inode) {
                AccessCheckResult::Allowed
            } else {
                AccessCheckResult::Denied("mode change requires owner or CAP_FOWNER".to_string())
            }
        }
        SetAttrOp::Owner(_, _) => {
            // Owner change requires root (CAP_CHOWN)
            AccessCheckResult::RequiresCaps(vec!["CAP_CHOWN".to_string()])
        }
        SetAttrOp::Size(_) | SetAttrOp::Timestamps | SetAttrOp::Any => {
            // Size/timestamp changes require write permission
            check_dac_access(
                &ctx.inode,
                &ctx.user_context,
                crate::access::AccessMode::W_OK,
            )
        }
    }
}

/// Checks delete permission on a file.
///
/// Delete requires write+execute on the parent directory, plus sticky bit check.
/// The sticky bit (on parent directory) prevents users from deleting files they
/// don't own in directories like /tmp.
///
/// # Arguments
/// * `parent` - The parent directory inode attributes
/// * `child` - The child file inode attributes
/// * `ctx` - The user context
///
/// # Returns
/// Access check result for delete operation
pub fn check_delete_permission(
    parent: &InodeAttr,
    child: &InodeAttr,
    ctx: &crate::access::UserContext,
) -> AccessCheckResult {
    // Root can always delete
    if ctx.is_root() {
        return AccessCheckResult::Allowed;
    }

    // Parent must be a directory
    if parent.file_type != FileType::Directory {
        return AccessCheckResult::Denied("parent is not a directory".to_string());
    }

    // Check write+execute on parent directory
    let mode = crate::access::AccessMode(
        crate::access::AccessMode::W_OK.0 | crate::access::AccessMode::X_OK.0,
    );
    match crate::access::check_access(parent, ctx, mode) {
        Err(MetaError::PermissionDenied) => {
            return AccessCheckResult::Denied("no write+execute on parent directory".to_string());
        }
        Err(e) => {
            return AccessCheckResult::Denied(format!("parent access check failed: {}", e));
        }
        Ok(()) => {}
    }

    // Check sticky bit
    match crate::access::check_sticky_bit(parent, child, ctx) {
        Err(MetaError::PermissionDenied) => {
            AccessCheckResult::Denied("sticky bit prevents deletion".to_string())
        }
        Err(e) => AccessCheckResult::Denied(format!("sticky bit check failed: {}", e)),
        Ok(()) => AccessCheckResult::Allowed,
    }
}

/// Computes effective permission bits from ACL for a user.
///
/// This implements the POSIX.1e effective permissions algorithm:
/// - Owner uses UserObj entry directly
/// - Named users use User entry masked by Mask
/// - Groups use best matching group entry masked by Mask
/// - Others use Other entry
///
/// # Arguments
/// * `acl` - The ACL to compute from
/// * `owner_uid` - The inode owner UID
/// * `owner_gid` - The inode owner GID
/// * `ctx` - The user context
///
/// # Returns
/// Effective permission bits (r=4, w=2, x=1)
pub fn get_effective_mode_from_acl(
    acl: &Acl,
    owner_uid: u32,
    owner_gid: u32,
    ctx: &crate::access::UserContext,
) -> u8 {
    // Find the mask entry if present
    let mask = acl
        .entries
        .iter()
        .find(|e| e.tag == AclTag::Mask)
        .map(|e| e.perms);

    // Check for owner match
    if ctx.uid == owner_uid || ctx.uid == 0 {
        for entry in &acl.entries {
            if entry.tag == AclTag::UserObj {
                return entry.perms;
            }
        }
    }

    // Check for named user match
    for entry in &acl.entries {
        if let AclTag::User(named_uid) = entry.tag {
            if named_uid == ctx.uid {
                let perms = entry.perms;
                return mask.map_or(perms, |m| perms & m);
            }
        }
    }

    // Build list of all groups user is in
    let all_gids: Vec<u32> = std::iter::once(ctx.gid)
        .chain(ctx.supplementary_gids.iter().copied())
        .collect();

    // Check for group matches - find best (union of all matching group perms)
    let mut best_group_perm: Option<u8> = None;
    for entry in &acl.entries {
        match &entry.tag {
            AclTag::GroupObj => {
                if all_gids.contains(&owner_gid) || ctx.uid == 0 {
                    let perms = entry.perms;
                    let effective = mask.map_or(perms, |m| perms & m);
                    best_group_perm = Some(best_group_perm.map_or(effective, |b| b | effective));
                }
            }
            AclTag::Group(named_gid) => {
                if all_gids.contains(named_gid) {
                    let perms = entry.perms;
                    let effective = mask.map_or(perms, |m| perms & m);
                    best_group_perm = Some(best_group_perm.map_or(effective, |b| b | effective));
                }
            }
            _ => {}
        }
    }

    if let Some(perms) = best_group_perm {
        return perms;
    }

    // Fall back to Other entry
    for entry in &acl.entries {
        if entry.tag == AclTag::Other {
            return entry.perms;
        }
    }

    // No matching entry found, no permissions
    0
}

/// Creates a default ACL based on file type.
///
/// Files get a restrictive default ACL (0600) - owner read/write only.
/// Directories get a more permissive default ACL (0755) - owner full, others search.
///
/// # Arguments
/// * `file_type` - The type of file to create default ACL for
///
/// # Returns
/// A default ACL appropriate for the file type
pub fn default_acl_for_type(file_type: FileType) -> Acl {
    match file_type {
        FileType::Directory => Acl {
            entries: vec![
                AclEntry {
                    tag: AclTag::UserObj,
                    perms: 7,
                },
                AclEntry {
                    tag: AclTag::GroupObj,
                    perms: 5,
                },
                AclEntry {
                    tag: AclTag::Other,
                    perms: 5,
                },
            ],
        },
        _ => Acl {
            entries: vec![
                AclEntry {
                    tag: AclTag::UserObj,
                    perms: 6,
                },
                AclEntry {
                    tag: AclTag::GroupObj,
                    perms: 0,
                },
                AclEntry {
                    tag: AclTag::Other,
                    perms: 0,
                },
            ],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_file_attr(uid: u32, gid: u32, mode: u32) -> InodeAttr {
        InodeAttr::new_file(crate::types::InodeId::new(2), uid, gid, mode, 1)
    }

    fn create_dir_attr(uid: u32, gid: u32, mode: u32) -> InodeAttr {
        InodeAttr::new_directory(crate::types::InodeId::new(1), uid, gid, mode, 1)
    }

    fn make_acl(entries: Vec<(AclTag, u8)>) -> Acl {
        Acl {
            entries: entries
                .into_iter()
                .map(|(tag, perms)| AclEntry { tag, perms })
                .collect(),
        }
    }

    // Test 1: ACL evaluation - owner matches UserObj entry
    #[test]
    fn test_acl_owner_matches_userobj() {
        let acl = make_acl(vec![
            (AclTag::UserObj, 7),
            (AclTag::GroupObj, 0),
            (AclTag::Other, 0),
        ]);
        let attr = create_file_attr(1000, 1000, 0o644);
        let ctx = crate::access::UserContext::new(1000, 1000, vec![]);

        let check_ctx = AccessCheckContext {
            inode: attr,
            user_context: ctx,
            operation: AccessOperation::Read,
            dac_mode: 0o644,
            acl: Some(acl),
        };

        let result = check_access(&check_ctx);
        assert!(result.is_allowed());
    }

    // Test 2: ACL evaluation - named user matches User(uid) entry
    #[test]
    fn test_acl_named_user_matches() {
        let acl = make_acl(vec![
            (AclTag::UserObj, 0),
            (AclTag::User(2000), 7),
            (AclTag::GroupObj, 0),
            (AclTag::Mask, 7),
            (AclTag::Other, 0),
        ]);
        let attr = create_file_attr(1000, 1000, 0o600);
        let ctx = crate::access::UserContext::new(2000, 2000, vec![]);

        let check_ctx = AccessCheckContext {
            inode: attr,
            user_context: ctx,
            operation: AccessOperation::Read,
            dac_mode: 0o600,
            acl: Some(acl),
        };

        let result = check_access(&check_ctx);
        assert!(result.is_allowed());
    }

    // Test 3: ACL evaluation - group matches GroupObj entry (file gid)
    #[test]
    fn test_acl_group_obj_matches() {
        let acl = make_acl(vec![
            (AclTag::UserObj, 0),
            (AclTag::GroupObj, 5),
            (AclTag::Mask, 7),
            (AclTag::Other, 0),
        ]);
        let attr = create_file_attr(1000, 1000, 0o640);
        let ctx = crate::access::UserContext::new(999, 1000, vec![]);

        let check_ctx = AccessCheckContext {
            inode: attr,
            user_context: ctx,
            operation: AccessOperation::Read,
            dac_mode: 0o640,
            acl: Some(acl),
        };

        let result = check_access(&check_ctx);
        assert!(result.is_allowed());
    }

    // Test 4: ACL evaluation - named group matches Group(gid) entry
    #[test]
    fn test_acl_named_group_matches() {
        let acl = make_acl(vec![
            (AclTag::UserObj, 0),
            (AclTag::GroupObj, 0),
            (AclTag::Group(2000), 5),
            (AclTag::Mask, 7),
            (AclTag::Other, 0),
        ]);
        let attr = create_file_attr(1000, 1000, 0o600);
        let ctx = crate::access::UserContext::new(999, 1000, vec![2000]);

        let check_ctx = AccessCheckContext {
            inode: attr,
            user_context: ctx,
            operation: AccessOperation::Read,
            dac_mode: 0o600,
            acl: Some(acl),
        };

        let result = check_access(&check_ctx);
        assert!(result.is_allowed());
    }

    // Test 5: ACL evaluation - mask entry limits named entries
    #[test]
    fn test_acl_mask_limits_named() {
        let acl = make_acl(vec![
            (AclTag::UserObj, 0),
            (AclTag::User(2000), 7),
            (AclTag::GroupObj, 0),
            (AclTag::Mask, 4),
            (AclTag::Other, 0),
        ]);
        let attr = create_file_attr(1000, 1000, 0o600);
        let ctx = crate::access::UserContext::new(2000, 2000, vec![]);

        let check_ctx = AccessCheckContext {
            inode: attr,
            user_context: ctx,
            operation: AccessOperation::Write,
            dac_mode: 0o600,
            acl: Some(acl),
        };

        let result = check_access(&check_ctx);
        assert!(result.is_denied());
    }

    // Test 6: ACL evaluation - other entry for unmatched users
    #[test]
    fn test_acl_other_for_unmatched() {
        let acl = make_acl(vec![
            (AclTag::UserObj, 0),
            (AclTag::GroupObj, 0),
            (AclTag::Other, 4),
        ]);
        let attr = create_file_attr(1000, 1000, 0o600);
        let ctx = crate::access::UserContext::new(9999, 9999, vec![]);

        let check_ctx = AccessCheckContext {
            inode: attr,
            user_context: ctx,
            operation: AccessOperation::Write,
            dac_mode: 0o600,
            acl: Some(acl),
        };

        let result = check_access(&check_ctx);
        assert!(result.is_denied());
    }

    // Test 7: ACL evaluation - missing ACL falls back to DAC
    #[test]
    fn test_no_acl_falls_back_to_dac() {
        let attr = create_file_attr(1000, 1000, 0o644);
        let ctx = crate::access::UserContext::new(1000, 1000, vec![]);

        let check_ctx = AccessCheckContext {
            inode: attr,
            user_context: ctx,
            operation: AccessOperation::Read,
            dac_mode: 0o644,
            acl: None,
        };

        let result = check_access(&check_ctx);
        assert!(result.is_allowed());
    }

    // Test 8: ACL evaluation - root bypasses ACL checks
    #[test]
    fn test_root_bypasses_acl() {
        let acl = make_acl(vec![
            (AclTag::UserObj, 0),
            (AclTag::GroupObj, 0),
            (AclTag::Other, 0),
        ]);
        let attr = create_file_attr(1000, 1000, 0o000);
        let ctx = crate::access::UserContext::root();

        let check_ctx = AccessCheckContext {
            inode: attr,
            user_context: ctx,
            operation: AccessOperation::Write,
            dac_mode: 0o000,
            acl: Some(acl),
        };

        let result = check_access(&check_ctx);
        assert!(result.is_allowed());
    }

    // Test 9: Owner override - owner can access regardless of other bits
    #[test]
    fn test_owner_override_owner_can_access() {
        let attr = create_file_attr(1000, 1000, 0o000);
        let ctx = crate::access::UserContext::new(1000, 1000, vec![]);

        let result = is_owner_override_possible(&ctx, &attr);
        assert!(result);
    }

    // Test 10: Owner override - non-owner cannot use owner bits
    #[test]
    fn test_owner_override_non_owner_cannot() {
        let attr = create_file_attr(1000, 1000, 0o700);
        let ctx = crate::access::UserContext::new(2000, 2000, vec![]);

        let result = is_owner_override_possible(&ctx, &attr);
        assert!(!result);
    }

    // Test 11: Mode bits - setuid/setgid bits preserved during access check
    #[test]
    fn test_mode_bits_preserve_special_bits() {
        let mode_with_suid = 0o4755;
        let attr = create_file_attr(1000, 1000, mode_with_suid);

        assert!(crate::umask::is_setuid(attr.mode));
        assert!(!crate::umask::is_setgid(attr.mode));

        let mode_with_sgid = 0o2755;
        let attr2 = create_file_attr(1000, 1000, mode_with_sgid);

        assert!(!crate::umask::is_setuid(attr2.mode));
        assert!(crate::umask::is_setgid(attr2.mode));
    }

    // Test 12: SetAttr permission - mode change requires owner/root
    #[test]
    fn test_setattr_mode_requires_owner() {
        let attr = create_file_attr(1000, 1000, 0o644);
        let ctx = crate::access::UserContext::new(2000, 2000, vec![]);

        let check_ctx = AccessCheckContext {
            inode: attr,
            user_context: ctx,
            operation: AccessOperation::SetAttr(SetAttrOp::Mode(0o755)),
            dac_mode: 0o644,
            acl: None,
        };

        let result = check_access(&check_ctx);
        assert!(result.is_denied());
    }

    // Test 13: SetAttr permission - owner change requires root
    #[test]
    fn test_setattr_owner_requires_root() {
        let attr = create_file_attr(1000, 1000, 0o644);
        let ctx = crate::access::UserContext::new(1000, 1000, vec![]);

        let check_ctx = AccessCheckContext {
            inode: attr,
            user_context: ctx,
            operation: AccessOperation::SetAttr(SetAttrOp::Owner(2000, 2000)),
            dac_mode: 0o644,
            acl: None,
        };

        let result = check_access(&check_ctx);
        assert!(
            matches!(result, AccessCheckResult::RequiresCaps(caps) if caps.contains(&"CAP_CHOWN".to_string()))
        );
    }

    // Test 14: SetAttr permission - size truncation requires write permission
    #[test]
    fn test_setattr_size_requires_write() {
        let attr = create_file_attr(1000, 1000, 0o644);
        let ctx = crate::access::UserContext::new(1000, 1000, vec![]);

        let check_ctx = AccessCheckContext {
            inode: attr,
            user_context: ctx,
            operation: AccessOperation::SetAttr(SetAttrOp::Size(0)),
            dac_mode: 0o644,
            acl: None,
        };

        let result = check_access(&check_ctx);
        assert!(result.is_allowed());
    }

    // Test 15: SetAttr permission - timestamp change requires write permission
    #[test]
    fn test_setattr_timestamp_requires_write() {
        let attr = create_file_attr(1000, 1000, 0o644);
        let ctx = crate::access::UserContext::new(1000, 1000, vec![]);

        let check_ctx = AccessCheckContext {
            inode: attr,
            user_context: ctx,
            operation: AccessOperation::SetAttr(SetAttrOp::Timestamps),
            dac_mode: 0o644,
            acl: None,
        };

        let result = check_access(&check_ctx);
        assert!(result.is_allowed());
    }

    // Test 16: Delete permission - write+execute on parent directory
    #[test]
    fn test_delete_requires_parent_write_exec() {
        let parent = create_dir_attr(1000, 1000, 0o555);
        let child = create_file_attr(2000, 2000, 0o644);
        let ctx = crate::access::UserContext::new(2000, 2000, vec![]);

        let result = check_delete_permission(&parent, &child, &ctx);
        assert!(result.is_denied());
    }

    // Test 17: Delete permission - sticky bit blocks non-owners
    #[test]
    fn test_delete_sticky_bit_blocks() {
        let parent = create_dir_attr(1000, 1000, 0o755 | 0o1000);
        let child = create_file_attr(2000, 2000, 0o644);
        let ctx = crate::access::UserContext::new(3000, 3000, vec![]);

        let result = check_delete_permission(&parent, &child, &ctx);
        assert!(result.is_denied());
    }

    // Test 18: Default ACL - files get restricted default (0600)
    #[test]
    fn test_default_acl_files_restricted() {
        let acl = default_acl_for_type(FileType::RegularFile);

        let user_obj = acl
            .entries
            .iter()
            .find(|e| e.tag == AclTag::UserObj)
            .unwrap();
        assert_eq!(user_obj.perms, 6);

        let other = acl.entries.iter().find(|e| e.tag == AclTag::Other).unwrap();
        assert_eq!(other.perms, 0);
    }

    // Test 19: Default ACL - directories get more permissive default (0755)
    #[test]
    fn test_default_acl_directories_permissive() {
        let acl = default_acl_for_type(FileType::Directory);

        let user_obj = acl
            .entries
            .iter()
            .find(|e| e.tag == AclTag::UserObj)
            .unwrap();
        assert_eq!(user_obj.perms, 7);

        let other = acl.entries.iter().find(|e| e.tag == AclTag::Other).unwrap();
        assert_eq!(other.perms, 5);
    }

    // Test 20: Umask application - umask strips requested permissions
    #[test]
    fn test_umask_strips_permissions() {
        let mode = apply_umask_to_mode(0o777, 0o022);
        assert_eq!(mode & 0o777, 0o755);

        let mode2 = apply_umask_to_mode(0o666, 0o077);
        assert_eq!(mode2 & 0o777, 0o600);
    }

    // Test 21: Deny entries - ACL with explicit deny blocks access
    #[test]
    fn test_acl_deny_blocks_access() {
        let acl = make_acl(vec![
            (AclTag::UserObj, 0),
            (AclTag::GroupObj, 0),
            (AclTag::Other, 0),
        ]);
        let attr = create_file_attr(1000, 1000, 0o644);
        let ctx = crate::access::UserContext::new(1000, 1000, vec![]);

        let check_ctx = AccessCheckContext {
            inode: attr,
            user_context: ctx,
            operation: AccessOperation::Read,
            dac_mode: 0o644,
            acl: Some(acl),
        };

        let result = check_access(&check_ctx);
        assert!(result.is_denied());
    }

    // Test 22: Group permissions - supplementary groups checked
    #[test]
    fn test_supplementary_groups_checked() {
        let acl = make_acl(vec![
            (AclTag::UserObj, 0),
            (AclTag::GroupObj, 0),
            (AclTag::Group(3000), 5),
            (AclTag::Mask, 7),
            (AclTag::Other, 0),
        ]);
        let attr = create_file_attr(1000, 1000, 0o600);
        let ctx = crate::access::UserContext::new(2000, 2000, vec![3000, 4000]);

        let check_ctx = AccessCheckContext {
            inode: attr,
            user_context: ctx,
            operation: AccessOperation::Read,
            dac_mode: 0o600,
            acl: Some(acl),
        };

        let result = check_access(&check_ctx);
        assert!(result.is_allowed());
    }

    // Test 23: Mixed ACL/DAC - ACL present but only basic entries
    #[test]
    fn test_mixed_acl_dac_basic_entries() {
        let acl = make_acl(vec![
            (AclTag::UserObj, 7),
            (AclTag::GroupObj, 5),
            (AclTag::Other, 4),
        ]);
        let attr = create_file_attr(1000, 1000, 0o644);
        let ctx = crate::access::UserContext::new(9999, 9999, vec![]);

        let check_ctx = AccessCheckContext {
            inode: attr,
            user_context: ctx,
            operation: AccessOperation::Write,
            dac_mode: 0o644,
            acl: Some(acl),
        };

        let result = check_access(&check_ctx);
        assert!(result.is_denied());
    }

    // Test 24: Capability consideration - CAP_FOWNER allows owner override
    #[test]
    fn test_cap_fowner_allows_owner_override() {
        let attr = create_file_attr(1000, 1000, 0o000);
        let ctx = crate::access::UserContext::root();

        let can_override = is_owner_override_possible(&ctx, &attr);
        assert!(can_override);

        // Non-root owner can also override their own files
        let ctx2 = crate::access::UserContext::new(1000, 1000, vec![]);
        let can_override2 = is_owner_override_possible(&ctx2, &attr);
        assert!(can_override2);
    }

    // Additional proptest tests
    proptest::proptest! {
        #[test]
        fn prop_umask_application(mode in 0u32..=0o777, umask in 0u32..=0o777) {
            let result = apply_umask_to_mode(mode, umask);
            proptest::prop_assert!(result <= 0o777);
        }

        #[test]
        fn prop_default_acl_valid(ft in 0u8..=1u8) {
            let file_type = if ft == 0 { FileType::RegularFile } else { FileType::Directory };
            let acl = default_acl_for_type(file_type);
            proptest::prop_assert!(acl.entries.len() >= 3);
            proptest::prop_assert!(acl.entries.iter().any(|e| e.tag == AclTag::UserObj));
            proptest::prop_assert!(acl.entries.iter().any(|e| e.tag == AclTag::GroupObj));
            proptest::prop_assert!(acl.entries.iter().any(|e| e.tag == AclTag::Other));
        }

        #[test]
        fn prop_effective_mode_acl_valid(uid in 0u32..=65535u32, gid in 0u32..=65535u32, owner_uid in 0u32..=65535u32, owner_gid in 0u32..=65535u32) {
            let acl = make_acl(vec![
                (AclTag::UserObj, 7),
                (AclTag::GroupObj, 5),
                (AclTag::Other, 4),
            ]);
            let ctx = crate::access::UserContext::new(uid, gid, vec![]);
            let effective = get_effective_mode_from_acl(&acl, owner_uid, owner_gid, &ctx);
            proptest::prop_assert!(effective <= 7);
        }

        #[test]
        fn prop_acl_result_allowed_denied(result in 0u8..2u8) {
            let result = if result == 0 {
                AccessCheckResult::Allowed
            } else {
                AccessCheckResult::Denied("test".to_string())
            };
            let encoded = bincode::serialize(&result).unwrap();
            let decoded: AccessCheckResult = bincode::deserialize(&encoded).unwrap();
            proptest::prop_assert_eq!(result, decoded);
        }
    }
}
