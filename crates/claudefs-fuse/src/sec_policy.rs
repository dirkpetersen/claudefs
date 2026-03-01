use crate::{FuseError, Result};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    SysAdmin,
    DacReadSearch,
    DacOverride,
    Chown,
    FOwner,
    FSetId,
    Kill,
    SetGid,
    SetUid,
    SetPCap,
    NetAdmin,
    SysChroot,
    Mknod,
    Lease,
    AuditWrite,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitySet {
    caps: Vec<Capability>,
}

impl CapabilitySet {
    pub fn new() -> Self {
        Self { caps: Vec::new() }
    }

    pub fn fuse_minimal() -> Self {
        let mut caps = Self::new();
        caps.add(Capability::SysAdmin);
        caps
    }

    pub fn contains(&self, cap: &Capability) -> bool {
        self.caps.contains(cap)
    }

    pub fn add(&mut self, cap: Capability) {
        if !self.caps.contains(&cap) {
            self.caps.push(cap);
        }
    }

    pub fn remove(&mut self, cap: Capability) -> bool {
        if let Some(pos) = self.caps.iter().position(|c| c == &cap) {
            self.caps.remove(pos);
            true
        } else {
            false
        }
    }

    pub fn len(&self) -> usize {
        self.caps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.caps.is_empty()
    }
}

impl Default for CapabilitySet {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SeccompMode {
    #[default]
    Disabled,
    Log,
    Enforce,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyscallPolicy {
    mode: SeccompMode,
    allowed: Vec<String>,
    blocked: Vec<String>,
}

impl SyscallPolicy {
    pub fn new() -> Self {
        Self {
            mode: SeccompMode::Disabled,
            allowed: Vec::new(),
            blocked: Vec::new(),
        }
    }

    pub fn fuse_allowlist() -> Self {
        let allowed = [
            "read",
            "write",
            "open",
            "close",
            "stat",
            "fstat",
            "lstat",
            "poll",
            "lseek",
            "mmap",
            "mprotect",
            "munmap",
            "brk",
            "rt_sigaction",
            "rt_sigprocmask",
            "ioctl",
            "pread64",
            "pwrite64",
            "readv",
            "writev",
            "access",
            "pipe",
            "select",
            "sched_yield",
            "mremap",
            "msync",
            "mincore",
            "madvise",
            "shmget",
            "shmat",
            "shmctl",
            "dup",
            "dup2",
            "nanosleep",
            "getitimer",
            "alarm",
            "setitimer",
            "getpid",
            "sendfile",
            "socket",
            "connect",
            "accept",
            "sendto",
            "recvfrom",
            "sendmsg",
            "recvmsg",
            "shutdown",
            "bind",
            "listen",
            "getsockname",
            "getpeername",
            "socketpair",
            "setsockopt",
            "getsockopt",
            "clone",
            "fork",
            "vfork",
            "execve",
            "exit",
            "wait4",
            "kill",
            "uname",
            "semget",
            "semop",
            "semctl",
            "shmdt",
            "msgget",
            "msgsnd",
            "msgrcv",
            "msgctl",
            "fcntl",
            "flock",
            "fsync",
            "fdatasync",
            "truncate",
            "ftruncate",
            "getcwd",
            "chdir",
            "fchdir",
            "rename",
            "mkdir",
            "rmdir",
            "creat",
            "link",
            "unlink",
            "symlink",
            "readlink",
            "chmod",
            "fchmod",
            "chown",
            "fchown",
            "lchown",
            "umask",
            "gettimeofday",
            "getrlimit",
            "getrusage",
            "sysinfo",
            "times",
            "ptrace",
            "getuid",
            "syslog",
            "getgid",
            "setuid",
            "setgid",
            "geteuid",
            "getegid",
            "setpgid",
            "getppid",
            "getpgrp",
            "setsid",
            "setreuid",
            "setregid",
            "getgroups",
            "setgroups",
            "setresuid",
            "getresuid",
            "setresgid",
            "getresgid",
            "getpgid",
            "setfsuid",
            "setfsgid",
            "getsid",
            "capget",
            "capset",
            "rt_sigpending",
            "rt_sigtimedwait",
            "rt_sigqueueinfo",
            "rt_sigsuspend",
            "sigaltstack",
            "utime",
            "mknod",
            "statfs",
            "fstatfs",
            "sysfs",
            "getpriority",
            "setpriority",
            "sched_setparam",
            "sched_getparam",
            "sched_setscheduler",
            "sched_getscheduler",
            "sched_get_priority_max",
            "sched_get_priority_min",
            "sched_rr_get_interval",
            "mlock",
            "munlock",
            "mlockall",
            "munlockall",
            "vhangup",
            "modify_ldt",
            "pivot_root",
            "prctl",
            "arch_prctl",
            "adjtimex",
            "setrlimit",
            "chroot",
            "sync",
            "acct",
            "settimeofday",
            "mount",
            "umount2",
            "getdents",
            "getdents64",
            "restart_syscall",
            "tgkill",
            "utimes",
            "futex",
            "set_thread_area",
            "io_setup",
            "io_destroy",
            "io_getevents",
            "io_submit",
            "io_cancel",
            "get_thread_area",
            "lookup_dcookie",
            "epoll_create",
            "epoll_ctl_old",
            "epoll_wait_old",
            "remap_file_pages",
            "set_tid_address",
            "timer_create",
            "timer_settime",
            "timer_gettime",
            "timer_getoverrun",
            "timer_delete",
            "clock_settime",
            "clock_gettime",
            "clock_getres",
            "clock_nanosleep",
            "exit_group",
            "epoll_wait",
            "epoll_ctl",
            "utimes",
            "vserver",
            "mbind",
            "set_mempolicy",
            "get_mempolicy",
            "mq_open",
            "mq_unlink",
            "mq_timedsend",
            "mq_timedreceive",
            "mq_notify",
            "mq_getsetattr",
            "kexec_load",
            "waitid",
            "add_key",
            "request_key",
            "keyctl",
            "ioprio_set",
            "ioprio_get",
            "inotify_init",
            "inotify_add_watch",
            "inotify_rm_watch",
            "migrate_pages",
            "openat",
            "mkdirat",
            "mknodat",
            "fchownat",
            "futimesat",
            "newfstatat",
            "unlinkat",
            "renameat",
            "linkat",
            "symlinkat",
            "readlinkat",
            "fchmodat",
            "faccessat",
            "pselect6",
            "ppoll",
            "unshare",
            "set_robust_list",
            "get_robust_list",
            "splice",
            "tee",
            "sync_file_range",
            "vmsplice",
            "move_pages",
            "utimensat",
            "epoll_pwait",
            "signalfd",
            "timerfd_create",
            "eventfd",
            "fallocate",
            "timerfd_settime",
            "timerfd_gettime",
            "accept4",
            "signalfd4",
            "eventfd2",
            "epoll_create1",
            "dup3",
            "pipe2",
            "inotify_init1",
            "preadv",
            "pwritev",
            "rt_tgsigqueueinfo",
            "perf_event_open",
            "recvmmsg",
            "fanotify_init",
            "fanotify_mark",
            "prlimit64",
            "name_to_handle_at",
            "open_by_handle_at",
            "clock_adjtime",
            "syncfs",
            "sendmmsg",
            "setns",
            "getcpu",
            "process_vm_readv",
            "process_vm_writev",
            "kcmp",
            "finit_module",
            "sched_setattr",
            "sched_getattr",
            "renameat2",
            "seccomp",
            "getrandom",
            "memfd_create",
            "kexec_file_load",
            "bpf",
            "execveat",
            "userfaultfd",
            "membarrier",
            "mlock2",
            "copy_file_range",
            "preadv2",
            "pwritev2",
            "pkey_mprotect",
            "pkey_alloc",
            "pkey_free",
            "statx",
            "io_pgetevents",
            "rseq",
            "pidfd_send_signal",
            "io_uring_setup",
            "io_uring_enter",
            "io_uring_register",
            "open_tree",
            "move_mount",
            "fsopen",
            "fsconfig",
            "fsmount",
            "fspick",
            "pidfd_open",
            "clone3",
            "close_range",
            "openat2",
            "pidfd_getfd",
            "faccessat2",
            "process_madvise",
            "epoll_pwait2",
            "mount_setattr",
            "quotactl_fd",
            "landlock_create_ruleset",
            "landlock_add_rule",
            "landlock_restrict_self",
            "memfd_secret",
            "process_mrelease",
            "futex_waitv",
            "set_mempolicy_home_node",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        Self {
            mode: SeccompMode::Enforce,
            allowed,
            blocked: Vec::new(),
        }
    }

    pub fn is_allowed(&self, syscall: &str) -> bool {
        if self.allowed.is_empty() {
            !self.blocked.contains(&syscall.to_string())
        } else {
            self.allowed.contains(&syscall.to_string())
                && !self.blocked.contains(&syscall.to_string())
        }
    }

    pub fn is_blocked(&self, syscall: &str) -> bool {
        if self.blocked.is_empty() {
            false
        } else {
            self.blocked.contains(&syscall.to_string())
        }
    }

    pub fn mode(&self) -> SeccompMode {
        self.mode
    }

    pub fn with_mode(mut self, mode: SeccompMode) -> Self {
        self.mode = mode;
        self
    }
}

impl Default for SyscallPolicy {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountNamespace {
    ns_id: u64,
    pid: u32,
    created_at_secs: u64,
}

impl MountNamespace {
    pub fn new(ns_id: u64, pid: u32) -> Self {
        let created_at_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            ns_id,
            pid,
            created_at_secs,
        }
    }

    pub fn age_secs(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        now.saturating_sub(self.created_at_secs)
    }

    pub fn ns_id(&self) -> u64 {
        self.ns_id
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityProfile {
    capabilities: CapabilitySet,
    syscall_policy: SyscallPolicy,
    mount_ns: Option<MountNamespace>,
    enforce_no_new_privs: bool,
}

impl SecurityProfile {
    pub fn default() -> Self {
        Self {
            capabilities: CapabilitySet::new(),
            syscall_policy: SyscallPolicy::new(),
            mount_ns: None,
            enforce_no_new_privs: false,
        }
    }

    pub fn hardened() -> Self {
        Self {
            capabilities: CapabilitySet::fuse_minimal(),
            syscall_policy: SyscallPolicy::fuse_allowlist(),
            mount_ns: None,
            enforce_no_new_privs: true,
        }
    }

    pub fn with_capabilities(caps: CapabilitySet) -> Self {
        Self {
            capabilities: caps,
            syscall_policy: SyscallPolicy::new(),
            mount_ns: None,
            enforce_no_new_privs: false,
        }
    }

    pub fn with_syscall_policy(policy: SyscallPolicy) -> Self {
        Self {
            capabilities: CapabilitySet::new(),
            syscall_policy: policy,
            mount_ns: None,
            enforce_no_new_privs: false,
        }
    }

    pub fn is_syscall_permitted(&self, syscall: &str) -> bool {
        self.syscall_policy.is_allowed(syscall)
    }

    pub fn required_capabilities(&self) -> &CapabilitySet {
        &self.capabilities
    }

    pub fn with_mount_namespace(mut self, ns: MountNamespace) -> Self {
        self.mount_ns = Some(ns);
        self
    }

    pub fn with_no_new_privs(mut self, enabled: bool) -> Self {
        self.enforce_no_new_privs = enabled;
        self
    }

    pub fn mount_ns(&self) -> Option<&MountNamespace> {
        self.mount_ns.as_ref()
    }

    pub fn enforce_no_new_privs(&self) -> bool {
        self.enforce_no_new_privs
    }
}

impl Default for SecurityProfile {
    fn default() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone, Error, Serialize, Deserialize)]
pub enum ViolationType {
    #[error("Unauthorized syscall: {0}")]
    UnauthorizedSyscall(String),
    #[error("Capability escalation attempt: {0}")]
    CapabilityEscalation(String),
    #[error("New privileges attempt: {0}")]
    NewPrivilegesAttempt(String),
    #[error("Unauthorized mount: {0}")]
    UnauthorizedMount(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyViolation {
    violation_type: ViolationType,
    details: String,
    timestamp: SystemTime,
}

impl PolicyViolation {
    pub fn new(vtype: ViolationType, details: &str) -> Self {
        Self {
            violation_type: vtype,
            details: details.to_string(),
            timestamp: SystemTime::now(),
        }
    }

    pub fn violation_type(&self) -> &ViolationType {
        &self.violation_type
    }

    pub fn details(&self) -> &str {
        &self.details
    }

    pub fn timestamp(&self) -> SystemTime {
        self.timestamp
    }
}

#[derive(Debug, Clone)]
pub struct PolicyEnforcer {
    profile: SecurityProfile,
    violations: Vec<PolicyViolation>,
    max_violations: usize,
}

impl PolicyEnforcer {
    pub fn new(profile: SecurityProfile) -> Self {
        Self {
            profile,
            violations: Vec::new(),
            max_violations: 100,
        }
    }

    pub fn with_max_violations(mut self, max: usize) -> Self {
        self.max_violations = max;
        self
    }

    pub fn check_syscall(&mut self, syscall: &str) -> Result<()> {
        if self.is_over_limit() {
            warn!("Policy enforcement limit reached, rejecting syscall checks");
            return Err(FuseError::NotSupported {
                op: "policy limit reached".to_string(),
            });
        }

        if !self.profile.is_syscall_permitted(syscall) {
            debug!("Blocking syscall: {}", syscall);
            self.record_violation(
                ViolationType::UnauthorizedSyscall(syscall.to_string()),
                &format!("Syscall {} is not permitted by policy", syscall),
            );
            return Err(FuseError::PermissionDenied {
                ino: 0,
                op: format!("syscall: {}", syscall),
            });
        }
        Ok(())
    }

    pub fn record_violation(&mut self, vtype: ViolationType, details: &str) {
        let violation = PolicyViolation::new(vtype, details);
        info!("Policy violation recorded: {}", violation.details());
        self.violations.push(violation);

        if self.violations.len() > self.max_violations {
            self.violations.remove(0);
        }
    }

    pub fn violation_count(&self) -> usize {
        self.violations.len()
    }

    pub fn recent_violations(&self, n: usize) -> &[PolicyViolation] {
        let start = self.violations.len().saturating_sub(n);
        &self.violations[start..]
    }

    pub fn is_over_limit(&self) -> bool {
        self.violation_count() >= self.max_violations
    }

    pub fn clear_violations(&mut self) {
        self.violations.clear();
    }

    pub fn profile(&self) -> &SecurityProfile {
        &self.profile
    }

    pub fn max_violations(&self) -> usize {
        self.max_violations
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_set_new() {
        let caps = CapabilitySet::new();
        assert!(caps.is_empty());
        assert_eq!(caps.len(), 0);
    }

    #[test]
    fn test_capability_set_fuse_minimal() {
        let caps = CapabilitySet::fuse_minimal();
        assert!(!caps.is_empty());
        assert_eq!(caps.len(), 1);
        assert!(caps.contains(&Capability::SysAdmin));
    }

    #[test]
    fn test_capability_set_add() {
        let mut caps = CapabilitySet::new();
        caps.add(Capability::SysAdmin);
        assert_eq!(caps.len(), 1);
        caps.add(Capability::SysAdmin);
        assert_eq!(caps.len(), 1);
    }

    #[test]
    fn test_capability_set_remove() {
        let mut caps = CapabilitySet::new();
        caps.add(Capability::SysAdmin);
        caps.add(Capability::NetAdmin);
        assert!(caps.remove(Capability::SysAdmin));
        assert_eq!(caps.len(), 1);
        assert!(!caps.remove(Capability::SysAdmin));
    }

    #[test]
    fn test_capability_set_contains() {
        let mut caps = CapabilitySet::new();
        caps.add(Capability::DacOverride);
        assert!(caps.contains(&Capability::DacOverride));
        assert!(!caps.contains(&Capability::SysAdmin));
    }

    #[test]
    fn test_seccomp_mode_default() {
        let mode = SeccompMode::default();
        assert_eq!(mode, SeccompMode::Disabled);
    }

    #[test]
    fn test_syscall_policy_fuse_allowlist() {
        let policy = SyscallPolicy::fuse_allowlist();
        assert!(policy.is_allowed("read"));
        assert!(policy.is_allowed("write"));
        assert!(policy.is_allowed("open"));
        assert!(policy.is_allowed("io_uring_enter"));
        assert!(policy.is_allowed("clone3"));
    }

    #[test]
    fn test_syscall_policy_is_blocked() {
        let mut policy = SyscallPolicy::new();
        policy = policy.with_mode(SeccompMode::Enforce);
        assert!(!policy.is_blocked("read"));
    }

    #[test]
    fn test_syscall_policy_custom_blocked() {
        let policy = SyscallPolicy {
            mode: SeccompMode::Enforce,
            allowed: vec!["read".to_string(), "write".to_string()],
            blocked: vec!["execve".to_string()],
        };
        assert!(policy.is_allowed("read"));
        assert!(policy.is_blocked("execve"));
    }

    #[test]
    fn test_mount_namespace_new() {
        let ns = MountNamespace::new(12345, 67890);
        assert_eq!(ns.ns_id(), 12345);
        assert_eq!(ns.pid(), 67890);
    }

    #[test]
    fn test_mount_namespace_age() {
        let ns = MountNamespace::new(1, 1);
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(ns.age_secs() >= 0);
    }

    #[test]
    fn test_security_profile_default() {
        let profile = SecurityProfile::default();
        assert!(profile.required_capabilities().is_empty());
        assert!(!profile.enforce_no_new_privs());
    }

    #[test]
    fn test_security_profile_hardened() {
        let profile = SecurityProfile::hardened();
        assert!(profile
            .required_capabilities()
            .contains(&Capability::SysAdmin));
        assert!(profile.enforce_no_new_privs());
    }

    #[test]
    fn test_security_profile_with_capabilities() {
        let mut caps = CapabilitySet::new();
        caps.add(Capability::SysAdmin);
        caps.add(Capability::NetAdmin);
        let profile = SecurityProfile::with_capabilities(caps);
        assert_eq!(profile.required_capabilities().len(), 2);
    }

    #[test]
    fn test_security_profile_is_syscall_permitted() {
        let profile = SecurityProfile::hardened();
        assert!(profile.is_syscall_permitted("read"));
        assert!(profile.is_syscall_permitted("write"));
    }

    #[test]
    fn test_policy_violation_new() {
        let violation = PolicyViolation::new(
            ViolationType::UnauthorizedSyscall("execve".to_string()),
            "Attempted to execute arbitrary code",
        );
        assert!(matches!(
            violation.violation_type(),
            ViolationType::UnauthorizedSyscall(_)
        ));
    }

    #[test]
    fn test_policy_enforcer_new() {
        let profile = SecurityProfile::default();
        let enforcer = PolicyEnforcer::new(profile);
        assert_eq!(enforcer.violation_count(), 0);
        assert!(!enforcer.is_over_limit());
    }

    #[test]
    fn test_policy_enforcer_check_syscall_allowed() {
        let profile = SecurityProfile::hardened();
        let mut enforcer = PolicyEnforcer::new(profile);
        assert!(enforcer.check_syscall("read").is_ok());
    }

    #[test]
    fn test_policy_enforcer_check_syscall_blocked() {
        let profile = SecurityProfile::hardened();
        let mut enforcer = PolicyEnforcer::new(profile);
        let result = enforcer.check_syscall("nonexistent_syscall_xyz");
        assert!(result.is_err());
    }

    #[test]
    fn test_policy_enforcer_record_violation() {
        let profile = SecurityProfile::default();
        let mut enforcer = PolicyEnforcer::new(profile);
        enforcer.record_violation(
            ViolationType::CapabilityEscalation("CAP_SYS_ADMIN".to_string()),
            "Tried to gain admin capabilities",
        );
        assert_eq!(enforcer.violation_count(), 1);
    }

    #[test]
    fn test_policy_enforcer_recent_violations() {
        let profile = SecurityProfile::default();
        let mut enforcer = PolicyEnforcer::new(profile);
        for i in 0..5 {
            enforcer.record_violation(
                ViolationType::UnauthorizedSyscall("test".to_string()),
                &format!("Violation {}", i),
            );
        }
        let recent = enforcer.recent_violations(3);
        assert_eq!(recent.len(), 3);
    }

    #[test]
    fn test_policy_enforcer_is_over_limit() {
        let profile = SecurityProfile::default();
        let mut enforcer = PolicyEnforcer::new(profile).with_max_violations(3);
        for i in 0..5 {
            enforcer.record_violation(
                ViolationType::UnauthorizedSyscall("test".to_string()),
                &format!("Violation {}", i),
            );
        }
        assert!(enforcer.is_over_limit());
    }

    #[test]
    fn test_policy_enforcer_clear_violations() {
        let profile = SecurityProfile::default();
        let mut enforcer = PolicyEnforcer::new(profile);
        enforcer.record_violation(
            ViolationType::UnauthorizedMount("/evil".to_string()),
            "Unauthorized mount attempt",
        );
        assert_eq!(enforcer.violation_count(), 1);
        enforcer.clear_violations();
        assert_eq!(enforcer.violation_count(), 0);
    }

    #[test]
    fn test_security_profile_with_mount_namespace() {
        let ns = MountNamespace::new(123, 456);
        let profile = SecurityProfile::default().with_mount_namespace(ns);
        assert!(profile.mount_ns().is_some());
    }
}
