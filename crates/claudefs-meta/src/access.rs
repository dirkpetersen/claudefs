//! POSIX permission checking
//!
//! Implements standard POSIX access checks for FUSE integration (A5), including
//! owner/group/other permission bits, sticky bit handling, and root bypass.

use crate::types::*;

/// POSIX access mode flags (matches libc values).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AccessMode(pub u32);

impl AccessMode {
    /// File existence test.
    pub const F_OK: AccessMode = AccessMode(0);
    /// Read permission.
    pub const R_OK: AccessMode = AccessMode(4);
    /// Write permission.
    pub const W_OK: AccessMode = AccessMode(2);
    /// Execute permission.
    pub const X_OK: AccessMode = AccessMode(1);
}

impl AccessMode {
    /// Checks if this mode includes read permission.
    pub fn has_read(&self) -> bool {
        self.0 & Self::R_OK.0 != 0
    }

    /// Checks if this mode includes write permission.
    pub fn has_write(&self) -> bool {
        self.0 & Self::W_OK.0 != 0
    }

    /// Checks if this mode includes execute permission.
    pub fn has_execute(&self) -> bool {
        self.0 & Self::X_OK.0 != 0
    }
}

/// User context for permission checking.
#[derive(Clone, Debug)]
pub struct UserContext {
    /// User ID
    pub uid: u32,
    /// Primary group ID
    pub gid: u32,
    /// Supplementary group IDs
    pub supplementary_gids: Vec<u32>,
}

impl UserContext {
    /// Creates a new UserContext.
    pub fn new(uid: u32, gid: u32, supplementary_gids: Vec<u32>) -> Self {
        Self {
            uid,
            gid,
            supplementary_gids,
        }
    }

    /// Returns the root (superuser) context.
    pub fn root() -> Self {
        Self {
            uid: 0,
            gid: 0,
            supplementary_gids: vec![],
        }
    }

    /// Checks if this user is root (uid 0).
    pub fn is_root(&self) -> bool {
        self.uid == 0
    }

    /// Checks if the user is in the given group.
    pub fn in_group(&self, gid: u32) -> bool {
        self.gid == gid || self.supplementary_gids.contains(&gid)
    }
}

/// Checks if the user has the requested access to the inode.
/// Implements standard POSIX: root bypasses all checks, owner bits, group bits, other bits.
pub fn check_access(
    attr: &InodeAttr,
    ctx: &UserContext,
    mode: AccessMode,
) -> Result<(), MetaError> {
    // Root bypasses all permission checks
    if ctx.is_root() {
        return Ok(());
    }

    // Extract permission bits
    let mode_bits = attr.mode & 0o777;

    // Owner check
    if attr.uid == ctx.uid {
        if mode.has_read() && mode_bits & 0o400 == 0 {
            return Err(MetaError::PermissionDenied);
        }
        if mode.has_write() && mode_bits & 0o200 == 0 {
            return Err(MetaError::PermissionDenied);
        }
        if mode.has_execute() && mode_bits & 0o100 == 0 {
            return Err(MetaError::PermissionDenied);
        }
        return Ok(());
    }

    // Group check
    if ctx.in_group(attr.gid) {
        if mode.has_read() && mode_bits & 0o040 == 0 {
            return Err(MetaError::PermissionDenied);
        }
        if mode.has_write() && mode_bits & 0o020 == 0 {
            return Err(MetaError::PermissionDenied);
        }
        if mode.has_execute() && mode_bits & 0o010 == 0 {
            return Err(MetaError::PermissionDenied);
        }
        return Ok(());
    }

    // Other check
    if mode.has_read() && mode_bits & 0o004 == 0 {
        return Err(MetaError::PermissionDenied);
    }
    if mode.has_write() && mode_bits & 0o002 == 0 {
        return Err(MetaError::PermissionDenied);
    }
    if mode.has_execute() && mode_bits & 0o001 == 0 {
        return Err(MetaError::PermissionDenied);
    }

    Ok(())
}

/// Checks sticky bit for unlink/rename in /tmp-like directories.
/// Only owner of the file, owner of the directory, or root can delete.
pub fn check_sticky_bit(
    parent_attr: &InodeAttr,
    child_attr: &InodeAttr,
    ctx: &UserContext,
) -> Result<(), MetaError> {
    // Root can always delete
    if ctx.is_root() {
        return Ok(());
    }

    // Check if sticky bit is set on parent directory (mode bit 01000)
    if parent_attr.mode & 0o1000 == 0 {
        return Ok(()); // No sticky bit, allow
    }

    // Allow if user owns the child
    if child_attr.uid == ctx.uid {
        return Ok(());
    }

    // Allow if user owns the parent directory
    if parent_attr.uid == ctx.uid {
        return Ok(());
    }

    Err(MetaError::PermissionDenied)
}

/// Checks write+execute on parent directory for creating entries.
pub fn can_create_in(parent_attr: &InodeAttr, ctx: &UserContext) -> Result<(), MetaError> {
    // Root can create anywhere
    if ctx.is_root() {
        return Ok(());
    }

    // Parent must be a directory
    if parent_attr.file_type != FileType::Directory {
        return Err(MetaError::NotADirectory(parent_attr.ino));
    }

    // Check write + execute on parent
    check_access(
        parent_attr,
        ctx,
        AccessMode(AccessMode::W_OK.0 | AccessMode::X_OK.0),
    )
}

/// Checks write+execute on parent, plus sticky bit for deletion.
pub fn can_delete_from(
    parent_attr: &InodeAttr,
    child_attr: &InodeAttr,
    ctx: &UserContext,
) -> Result<(), MetaError> {
    // Root can delete anywhere
    if ctx.is_root() {
        return Ok(());
    }

    // Parent must be a directory
    if parent_attr.file_type != FileType::Directory {
        return Err(MetaError::NotADirectory(parent_attr.ino));
    }

    // Check write + execute on parent
    check_access(
        parent_attr,
        ctx,
        AccessMode(AccessMode::W_OK.0 | AccessMode::X_OK.0),
    )?;

    // Check sticky bit
    check_sticky_bit(parent_attr, child_attr, ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_dir_attr(uid: u32, gid: u32, mode: u32) -> InodeAttr {
        InodeAttr {
            ino: InodeId::new(1),
            file_type: FileType::Directory,
            mode: 0o040000 | mode,
            nlink: 2,
            uid,
            gid,
            size: 4096,
            blocks: 8,
            atime: Timestamp::now(),
            mtime: Timestamp::now(),
            ctime: Timestamp::now(),
            crtime: Timestamp::now(),
            content_hash: None,
            repl_state: ReplicationState::Local,
            vector_clock: VectorClock::new(1, 0),
            generation: 0,
            symlink_target: None,
        }
    }

    fn create_file_attr(uid: u32, gid: u32, mode: u32) -> InodeAttr {
        InodeAttr {
            ino: InodeId::new(2),
            file_type: FileType::RegularFile,
            mode: 0o100000 | mode,
            nlink: 1,
            uid,
            gid,
            size: 100,
            blocks: 1,
            atime: Timestamp::now(),
            mtime: Timestamp::now(),
            ctime: Timestamp::now(),
            crtime: Timestamp::now(),
            content_hash: None,
            repl_state: ReplicationState::Local,
            vector_clock: VectorClock::new(1, 0),
            generation: 0,
            symlink_target: None,
        }
    }

    #[test]
    fn test_root_bypasses_checks() {
        let attr = create_file_attr(1000, 1000, 0o000);
        let ctx = UserContext::root();

        // Root can do anything regardless of permissions
        assert!(check_access(&attr, &ctx, AccessMode::R_OK).is_ok());
        assert!(check_access(&attr, &ctx, AccessMode::W_OK).is_ok());
        assert!(check_access(&attr, &ctx, AccessMode::X_OK).is_ok());
    }

    #[test]
    fn test_owner_read() {
        let attr = create_file_attr(1000, 1000, 0o644);
        let ctx = UserContext::new(1000, 1000, vec![]);

        assert!(check_access(&attr, &ctx, AccessMode::R_OK).is_ok());
    }

    #[test]
    fn test_owner_write() {
        let attr = create_file_attr(1000, 1000, 0o644);
        let ctx = UserContext::new(1000, 1000, vec![]);

        assert!(check_access(&attr, &ctx, AccessMode::W_OK).is_ok());
    }

    #[test]
    fn test_owner_execute() {
        let attr = create_file_attr(1000, 1000, 0o755);
        let ctx = UserContext::new(1000, 1000, vec![]);

        assert!(check_access(&attr, &ctx, AccessMode::X_OK).is_ok());
    }

    #[test]
    fn test_group_read() {
        let attr = create_file_attr(1000, 1000, 0o640);
        let ctx = UserContext::new(2000, 1000, vec![]);

        assert!(check_access(&attr, &ctx, AccessMode::R_OK).is_ok());
    }

    #[test]
    fn test_other_read() {
        let attr = create_file_attr(1000, 1000, 0o644);
        let ctx = UserContext::new(2000, 2000, vec![]);

        assert!(check_access(&attr, &ctx, AccessMode::R_OK).is_ok());
    }

    #[test]
    fn test_no_permission() {
        let attr = create_file_attr(1000, 1000, 0o600);
        let ctx = UserContext::new(2000, 2000, vec![]);

        assert!(matches!(
            check_access(&attr, &ctx, AccessMode::R_OK),
            Err(MetaError::PermissionDenied)
        ));
    }

    #[test]
    fn test_sticky_bit_owner_can_delete() {
        let parent = create_dir_attr(1000, 1000, 0o755 | 0o1000); // sticky bit set
        let child = create_file_attr(2000, 2000, 0o644);
        let ctx = UserContext::new(2000, 2000, vec![]);

        // Owner of child can delete
        assert!(check_sticky_bit(&parent, &child, &ctx).is_ok());
    }

    #[test]
    fn test_sticky_bit_non_owner_cannot_delete() {
        let parent = create_dir_attr(1000, 1000, 0o755 | 0o1000); // sticky bit set
        let child = create_file_attr(2000, 2000, 0o644);
        let ctx = UserContext::new(3000, 3000, vec![]);

        // Non-owner cannot delete
        assert!(matches!(
            check_sticky_bit(&parent, &child, &ctx),
            Err(MetaError::PermissionDenied)
        ));
    }

    #[test]
    fn test_can_create_in_directory() {
        let parent = create_dir_attr(1000, 1000, 0o755);
        let ctx = UserContext::new(1000, 1000, vec![]);

        assert!(can_create_in(&parent, &ctx).is_ok());
    }

    #[test]
    fn test_cannot_create_in_file() {
        let not_dir = create_file_attr(1000, 1000, 0o755);
        let ctx = UserContext::new(1000, 1000, vec![]);

        assert!(matches!(
            can_create_in(&not_dir, &ctx),
            Err(MetaError::NotADirectory(_))
        ));
    }
}
