use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct KernelVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl KernelVersion {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        KernelVersion {
            major,
            minor,
            patch,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() < 2 || parts.len() > 3 {
            return None;
        }

        let major = parts[0].parse::<u32>().ok()?;
        let minor = parts[1].parse::<u32>().ok()?;
        let patch = if parts.len() == 3 {
            parts[2].parse::<u32>().ok()?
        } else {
            0
        };

        Some(KernelVersion {
            major,
            minor,
            patch,
        })
    }

    pub fn at_least(&self, other: &KernelVersion) -> bool {
        self >= other
    }
}

impl fmt::Display for KernelVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

pub const KERNEL_FUSE_PASSTHROUGH: KernelVersion = KernelVersion {
    major: 6,
    minor: 8,
    patch: 0,
};
pub const KERNEL_ATOMIC_WRITES: KernelVersion = KernelVersion {
    major: 6,
    minor: 11,
    patch: 0,
};
pub const KERNEL_DYNAMIC_IORING: KernelVersion = KernelVersion {
    major: 6,
    minor: 20,
    patch: 0,
};

#[derive(Debug, Clone, PartialEq)]
pub enum PassthroughMode {
    Full,
    Partial,
    None,
}

#[derive(Debug, Clone)]
pub struct NegotiatedCapabilities {
    pub passthrough_mode: PassthroughMode,
    pub atomic_writes: bool,
    pub dynamic_ioring: bool,
    pub writeback_cache: bool,
    pub async_read: bool,
}

impl NegotiatedCapabilities {
    pub fn for_kernel(version: &KernelVersion) -> Self {
        let passthrough_mode = if version.at_least(&KERNEL_FUSE_PASSTHROUGH) {
            PassthroughMode::Full
        } else if version.at_least(&KernelVersion::new(5, 14, 0)) {
            PassthroughMode::Partial
        } else {
            PassthroughMode::None
        };

        NegotiatedCapabilities {
            passthrough_mode,
            atomic_writes: version.at_least(&KERNEL_ATOMIC_WRITES),
            dynamic_ioring: version.at_least(&KERNEL_DYNAMIC_IORING),
            writeback_cache: true,
            async_read: true,
        }
    }

    pub fn best_mode(&self) -> &PassthroughMode {
        &self.passthrough_mode
    }

    pub fn supports_passthrough(&self) -> bool {
        !matches!(self.passthrough_mode, PassthroughMode::None)
    }
}

pub struct CapabilityNegotiator {
    detected_version: Option<KernelVersion>,
    capabilities: Option<NegotiatedCapabilities>,
    negotiated: bool,
}

impl CapabilityNegotiator {
    pub fn new() -> Self {
        CapabilityNegotiator {
            detected_version: None,
            capabilities: None,
            negotiated: false,
        }
    }

    pub fn negotiate(&mut self, kernel_version: KernelVersion) -> &NegotiatedCapabilities {
        self.detected_version = Some(kernel_version.clone());
        self.capabilities = Some(NegotiatedCapabilities::for_kernel(&kernel_version));
        self.negotiated = true;
        self.capabilities.as_ref().unwrap()
    }

    pub fn capabilities(&self) -> &NegotiatedCapabilities {
        self.capabilities
            .as_ref()
            .expect("negotiate() must be called first")
    }

    pub fn is_negotiated(&self) -> bool {
        self.negotiated
    }

    pub fn kernel_version(&self) -> Option<&KernelVersion> {
        self.detected_version.as_ref()
    }
}

impl Default for CapabilityNegotiator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kernel_version_new_and_display() {
        let v = KernelVersion::new(6, 8, 0);
        assert_eq!(v.major, 6);
        assert_eq!(v.minor, 8);
        assert_eq!(v.patch, 0);
        assert_eq!(format!("{}", v), "6.8.0");
    }

    #[test]
    fn test_kernel_version_parse_three_parts() {
        let v = KernelVersion::parse("6.8.0").unwrap();
        assert_eq!(v.major, 6);
        assert_eq!(v.minor, 8);
        assert_eq!(v.patch, 0);
    }

    #[test]
    fn test_kernel_version_parse_two_parts() {
        let v = KernelVersion::parse("6.8").unwrap();
        assert_eq!(v.major, 6);
        assert_eq!(v.minor, 8);
        assert_eq!(v.patch, 0);
    }

    #[test]
    fn test_kernel_version_parse_invalid_returns_none() {
        assert!(KernelVersion::parse("invalid").is_none());
        assert!(KernelVersion::parse("6").is_none());
        assert!(KernelVersion::parse("6.8.0.1").is_none());
    }

    #[test]
    fn test_kernel_version_ordering() {
        let v1 = KernelVersion::new(6, 8, 0);
        let v2 = KernelVersion::new(6, 9, 0);
        let v3 = KernelVersion::new(7, 0, 0);
        assert!(v1 < v2);
        assert!(v2 < v3);
        assert!(v1 < v3);
    }

    #[test]
    fn test_kernel_version_at_least_self() {
        let v = KernelVersion::new(6, 8, 0);
        assert!(v.at_least(&v));
    }

    #[test]
    fn test_capabilities_for_kernel_6_8_full_passthrough() {
        let caps = NegotiatedCapabilities::for_kernel(&KernelVersion::new(6, 8, 0));
        assert_eq!(caps.passthrough_mode, PassthroughMode::Full);
    }

    #[test]
    fn test_capabilities_for_kernel_5_14_partial_passthrough() {
        let caps = NegotiatedCapabilities::for_kernel(&KernelVersion::new(5, 14, 0));
        assert_eq!(caps.passthrough_mode, PassthroughMode::Partial);
    }

    #[test]
    fn test_capabilities_for_kernel_5_10_no_passthrough() {
        let caps = NegotiatedCapabilities::for_kernel(&KernelVersion::new(5, 10, 0));
        assert_eq!(caps.passthrough_mode, PassthroughMode::None);
        assert!(!caps.supports_passthrough());
    }

    #[test]
    fn test_capabilities_atomic_writes_at_6_11() {
        let caps_before = NegotiatedCapabilities::for_kernel(&KernelVersion::new(6, 10, 99));
        let caps_after = NegotiatedCapabilities::for_kernel(&KernelVersion::new(6, 11, 0));
        assert!(!caps_before.atomic_writes);
        assert!(caps_after.atomic_writes);
    }

    #[test]
    fn test_capabilities_dynamic_ioring_at_6_20() {
        let caps_before = NegotiatedCapabilities::for_kernel(&KernelVersion::new(6, 19, 99));
        let caps_after = NegotiatedCapabilities::for_kernel(&KernelVersion::new(6, 20, 0));
        assert!(!caps_before.dynamic_ioring);
        assert!(caps_after.dynamic_ioring);
    }

    #[test]
    fn test_negotiator_new_is_not_negotiated() {
        let negotiator = CapabilityNegotiator::new();
        assert!(!negotiator.is_negotiated());
    }

    #[test]
    fn test_negotiator_negotiate_sets_capabilities() {
        let mut negotiator = CapabilityNegotiator::new();
        let caps = negotiator.negotiate(KernelVersion::new(6, 8, 0));
        assert_eq!(caps.passthrough_mode, PassthroughMode::Full);
    }

    #[test]
    fn test_negotiator_is_negotiated_after_negotiate() {
        let mut negotiator = CapabilityNegotiator::new();
        negotiator.negotiate(KernelVersion::new(6, 8, 0));
        assert!(negotiator.is_negotiated());
    }

    #[test]
    fn test_negotiator_kernel_version_accessible_after_negotiate() {
        let mut negotiator = CapabilityNegotiator::new();
        negotiator.negotiate(KernelVersion::new(6, 8, 1));
        let v = negotiator.kernel_version().unwrap();
        assert_eq!(v.patch, 1);
    }
}
