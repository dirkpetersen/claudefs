//! POSIX umask and file mode calculation for the metadata service.

use crate::types::MetaError;

/// Maximum valid umask value (all permissions masked).
pub const MAX_UMASK: u32 = 0o777;

/// POSIX file mode bits.
pub mod mode {
    /// Set-user-ID bit.
    pub const SETUID: u32 = 0o4000;
    /// Set-group-ID bit.
    pub const SETGID: u32 = 0o2000;
    /// Sticky bit.
    pub const STICKY: u32 = 0o1000;
    /// Owner read/write/execute bits.
    pub const OWNER_RWX: u32 = 0o700;
    /// Group read/write/execute bits.
    pub const GROUP_RWX: u32 = 0o070;
    /// Other read/write/execute bits.
    pub const OTHER_RWX: u32 = 0o007;
    /// Mask for permission bits (without type bits).
    pub const PERM_MASK: u32 = 0o7777;
    /// Regular file type bits.
    pub const REG: u32 = 0o100000;
    /// Directory type bits.
    pub const DIR: u32 = 0o040000;
    /// Symlink type bits.
    pub const LNK: u32 = 0o120000;
    /// Type mask (S_IFMT).
    pub const TYPE_MASK: u32 = 0o170000;
}

/// Applies a umask to a requested mode.
///
/// Strips the umask bits from the permission bits. Does NOT modify the file type bits.
pub fn apply_umask(mode: u32, umask: u32) -> u32 {
    let perms = mode & mode::PERM_MASK;
    let type_bits = mode & mode::TYPE_MASK;
    let masked = perms & !umask;
    type_bits | masked
}

/// Calculates the default creation mode for a new file.
///
/// Files are created with the requested mode, then masked by umask.
/// Returns the final mode with the REG type bits set.
pub fn file_create_mode(requested_mode: u32, umask: u32) -> u32 {
    let perms = requested_mode & !umask;
    mode::REG | perms
}

/// Calculates the default creation mode for a new directory.
///
/// Directories are created with the requested mode, then masked by umask.
/// Returns the final mode with the DIR type bits set.
pub fn dir_create_mode(requested_mode: u32, umask: u32) -> u32 {
    let perms = requested_mode & !umask;
    mode::DIR | perms
}

/// Validates a umask value.
///
/// A valid umask is in the range 0o000 - 0o777.
/// Returns `Err(MetaError::InvalidArgument)` if out of range.
pub fn validate_umask(umask: u32) -> Result<(), MetaError> {
    if umask > MAX_UMASK {
        Err(MetaError::InvalidArgument(format!(
            "umask {} exceeds maximum valid value {}",
            umask, MAX_UMASK
        )))
    } else {
        Ok(())
    }
}

/// Checks if a mode has the setuid bit set.
pub fn is_setuid(mode: u32) -> bool {
    (mode & mode::SETUID) != 0
}

/// Checks if a mode has the setgid bit set.
pub fn is_setgid(mode: u32) -> bool {
    (mode & mode::SETGID) != 0
}

/// Checks if a mode has the sticky bit set.
pub fn is_sticky(mode: u32) -> bool {
    (mode & mode::STICKY) != 0
}

/// Returns the permission bits of a mode (strips type bits, keeps 0o7777).
pub fn perm_bits(mode: u32) -> u32 {
    mode & mode::PERM_MASK
}

/// Returns only the file type bits (strips permission bits).
pub fn type_bits(mode: u32) -> u32 {
    mode & mode::TYPE_MASK
}

/// Formats a mode as a Unix mode string (e.g., "-rwxr-xr-x" or "drwxr-xr-x").
pub fn format_mode(mode: u32) -> String {
    let type_char = match type_bits(mode) {
        mode::DIR => 'd',
        mode::LNK => 'l',
        _ => '-',
    };

    let owner_r = if (mode & 0o400) != 0 { 'r' } else { '-' };
    let owner_w = if (mode & 0o200) != 0 { 'w' } else { '-' };
    let owner_x = if (mode & 0o100) != 0 { 'x' } else { '-' };
    let group_r = if (mode & 0o040) != 0 { 'r' } else { '-' };
    let group_w = if (mode & 0o020) != 0 { 'w' } else { '-' };
    let group_x = if (mode & 0o010) != 0 { 'x' } else { '-' };
    let other_r = if (mode & 0o004) != 0 { 'r' } else { '-' };
    let other_w = if (mode & 0o002) != 0 { 'w' } else { '-' };
    let other_x = if (mode & 0o001) != 0 { 'x' } else { '-' };

    format!(
        "{}{}{}{}{}{}{}{}{}{}",
        type_char, owner_r, owner_w, owner_x, group_r, group_w, group_x, other_r, other_w, other_x
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_umask_standard() {
        assert_eq!(apply_umask(0o644, 0o022), 0o644 & !0o022);
    }

    #[test]
    fn test_apply_umask_no_mask() {
        assert_eq!(apply_umask(0o755, 0o000), 0o755);
    }

    #[test]
    fn test_apply_umask_full_mask() {
        assert_eq!(apply_umask(0o777, 0o777), 0o000);
    }

    #[test]
    fn test_apply_umask_preserves_type() {
        let reg_mode = mode::REG | 0o644;
        let result = apply_umask(reg_mode, 0o022);
        assert_eq!(type_bits(result), mode::REG);
        assert_eq!(perm_bits(result), 0o644 & !0o022);

        let dir_mode = mode::DIR | 0o755;
        let result = apply_umask(dir_mode, 0o022);
        assert_eq!(type_bits(result), mode::DIR);
        assert_eq!(perm_bits(result), 0o755 & !0o022);
    }

    #[test]
    fn test_file_create_mode() {
        let result = file_create_mode(0o666, 0o022);
        assert_eq!(type_bits(result), mode::REG);
        assert_eq!(perm_bits(result), 0o644);
    }

    #[test]
    fn test_dir_create_mode() {
        let result = dir_create_mode(0o777, 0o022);
        assert_eq!(type_bits(result), mode::DIR);
        assert_eq!(perm_bits(result), 0o755);
    }

    #[test]
    fn test_validate_umask_valid() {
        assert!(validate_umask(0o000).is_ok());
        assert!(validate_umask(0o022).is_ok());
        assert!(validate_umask(0o777).is_ok());
    }

    #[test]
    fn test_validate_umask_invalid() {
        assert!(validate_umask(0o1000).is_err());
        assert!(validate_umask(0o1000)
            .unwrap_err()
            .to_string()
            .contains("invalid"));
    }

    #[test]
    fn test_is_setuid() {
        assert!(is_setuid(0o4755));
        assert!(!is_setuid(0o755));
    }

    #[test]
    fn test_is_setgid() {
        assert!(is_setgid(0o2755));
        assert!(!is_setgid(0o755));
    }

    #[test]
    fn test_is_sticky() {
        assert!(is_sticky(0o1777));
        assert!(!is_sticky(0o755));
    }

    #[test]
    fn test_perm_bits() {
        let mode = mode::DIR | 0o755;
        assert_eq!(perm_bits(mode), 0o755);
    }

    #[test]
    fn test_type_bits() {
        assert_eq!(type_bits(mode::DIR | 0o755), mode::DIR);
        assert_eq!(type_bits(mode::REG | 0o644), mode::REG);
    }

    #[test]
    fn test_format_mode_regular_file() {
        assert_eq!(format_mode(0o100644), "-rw-r--r--");
    }

    #[test]
    fn test_format_mode_directory() {
        assert_eq!(format_mode(0o040755), "drwxr-xr-x");
    }
}
