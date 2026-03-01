#![warn(missing_docs)]

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReadWriteMode {
    #[default]
    ReadWrite,
    ReadOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CacheMode {
    #[default]
    None,
    Relaxed,
    Strict,
}

#[derive(Debug, Clone, Default)]
pub struct MountOptions {
    pub source: PathBuf,
    pub target: PathBuf,
    pub read_only: ReadWriteMode,
    pub allow_other: bool,
    pub default_permissions: bool,
    pub cache_mode: CacheMode,
    pub max_background: u32,
    pub congestion_threshold: u32,
    pub direct_io: bool,
    pub kernel_cache: bool,
    pub auto_unmount: bool,
    pub fd: Option<i32>,
}

impl Default for MountOptions {
    fn default() -> Self {
        Self {
            source: PathBuf::from("."),
            target: PathBuf::from("/mnt/fuse"),
            read_only: ReadWriteMode::ReadWrite,
            allow_other: false,
            default_permissions: false,
            cache_mode: CacheMode::None,
            max_background: 16,
            congestion_threshold: 12,
            direct_io: false,
            kernel_cache: true,
            auto_unmount: true,
            fd: None,
        }
    }
}

impl MountOptions {
    pub fn new(source: PathBuf, target: PathBuf) -> Self {
        Self {
            source,
            target,
            ..Default::default()
        }
    }

    pub fn to_fuse_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        args.push(self.source.to_string_lossy().to_string());
        args.push(self.target.to_string_lossy().to_string());

        if self.allow_other {
            args.push("-o".to_string());
            args.push("allow_other".to_string());
        }

        if self.default_permissions {
            args.push("-o".to_string());
            args.push("default_permissions".to_string());
        }

        if self.direct_io {
            args.push("-o".to_string());
            args.push("direct_io".to_string());
        }

        match self.read_only {
            ReadWriteMode::ReadOnly => {
                args.push("-r".to_string());
            }
            _ => {}
        }

        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mount_options_default() {
        let opts = MountOptions::default();
        assert_eq!(opts.read_only, ReadWriteMode::ReadWrite);
        assert_eq!(opts.cache_mode, CacheMode::None);
        assert_eq!(opts.max_background, 16);
    }

    #[test]
    fn test_mount_options_new() {
        let source = PathBuf::from("/data");
        let target = PathBuf::from("/mnt/test");
        let opts = MountOptions::new(source.clone(), target.clone());
        assert_eq!(opts.source, source);
        assert_eq!(opts.target, target);
    }

    #[test]
    fn test_to_fuse_args_basic() {
        let opts = MountOptions::new(PathBuf::from("src"), PathBuf::from("tgt"));
        let args = opts.to_fuse_args();
        assert!(args.contains(&"src".to_string()));
        assert!(args.contains(&"tgt".to_string()));
    }

    #[test]
    fn test_to_fuse_args_read_only() {
        let mut opts = MountOptions::default();
        opts.read_only = ReadWriteMode::ReadOnly;
        let args = opts.to_fuse_args();
        assert!(args.contains(&"-r".to_string()));
    }

    #[test]
    fn test_to_fuse_args_allow_other() {
        let mut opts = MountOptions::default();
        opts.allow_other = true;
        let args = opts.to_fuse_args();
        assert!(args.contains(&"-o".to_string()));
        assert!(args.contains(&"allow_other".to_string()));
    }

    #[test]
    fn test_read_write_mode_default() {
        assert_eq!(ReadWriteMode::default(), ReadWriteMode::ReadWrite);
    }

    #[test]
    fn test_cache_mode_default() {
        assert_eq!(CacheMode::default(), CacheMode::None);
    }
}
