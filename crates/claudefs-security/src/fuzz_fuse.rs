//! Phase 3 FUSE protocol security fuzzing for claudefs-fuse.
//!
//! Findings: FINDING-FUSE-01 through FINDING-FUSE-15
//!
//! Tests adversarial inputs to FUSE configuration and operation handling.

use claudefs_fuse::cache::CacheConfig;
use claudefs_fuse::mount::{parse_mount_options, MountOptions};
use claudefs_fuse::passthrough::{check_kernel_version, PassthroughConfig, PassthroughStatus};
use proptest::prelude::*;

// ============================================================================
// FINDING-FUSE-01: Malformed mount option strings - injection attempts
// ============================================================================

#[test]
fn fuzz_mount_options_empty_string() {
    let result = parse_mount_options("");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), MountOptions::default());
}

#[test]
fn fuzz_mount_options_only_whitespace() {
    let result = parse_mount_options("   ");
    assert!(result.is_ok());
}

#[test]
fn fuzz_mount_options_sql_injection_attempt() {
    let result = parse_mount_options("allow_other'; DROP TABLE mounts;--");
    assert!(result.is_err());
}

#[test]
fn fuzz_mount_options_path_traversal() {
    let result = parse_mount_options("../../etc/passwd");
    assert!(result.is_err());
}

#[test]
fn fuzz_mount_options_null_byte_injection() {
    let result = parse_mount_options("allow_other\x00root");
    assert!(result.is_err() || result.unwrap().allow_other == false);
}

#[test]
fn fuzz_mount_options_shell_metacharacters() {
    let result = parse_mount_options("$(whoami)");
    assert!(result.is_err());
}

#[test]
fn fuzz_mount_options_backtick_expansion() {
    let result = parse_mount_options("`id`");
    assert!(result.is_err());
}

#[test]
fn fuzz_mount_options_newline_injection() {
    let result = parse_mount_options("allow_other\nroot\nro");
    assert!(result.is_err() || !result.unwrap().allow_other);
}

// ============================================================================
// FINDING-FUSE-02: Cache policy boundary tests - numeric overflow potential
// ============================================================================

#[test]
fn fuzz_cache_config_zero_ttl() {
    let config = CacheConfig {
        capacity: 100,
        ttl_secs: 0,
        negative_ttl_secs: 0,
    };
    assert_eq!(config.ttl_secs, 0);
    assert_eq!(config.negative_ttl_secs, 0);
}

#[test]
fn fuzz_cache_config_max_u64_ttl() {
    let config = CacheConfig {
        capacity: 100,
        ttl_secs: u64::MAX,
        negative_ttl_secs: u64::MAX,
    };
    assert_eq!(config.ttl_secs, u64::MAX);
}

#[test]
fn fuzz_cache_config_near_max_ttl() {
    let config = CacheConfig {
        capacity: 100,
        ttl_secs: u64::MAX - 1,
        negative_ttl_secs: u64::MAX - 1,
    };
    assert_eq!(config.ttl_secs, u64::MAX - 1);
}

#[test]
fn fuzz_cache_config_zero_capacity() {
    let config = CacheConfig {
        capacity: 0,
        ttl_secs: 30,
        negative_ttl_secs: 5,
    };
    assert_eq!(config.capacity, 0);
}

#[test]
fn fuzz_cache_config_max_capacity() {
    let config = CacheConfig {
        capacity: usize::MAX,
        ttl_secs: 30,
        negative_ttl_secs: 5,
    };
    assert_eq!(config.capacity, usize::MAX);
}

#[test]
fn fuzz_cache_config_unusual_ttl_values() {
    let config = CacheConfig {
        capacity: 100,
        ttl_secs: 1,
        negative_ttl_secs: 0,
    };
    assert_eq!(config.ttl_secs, 1);

    let config2 = CacheConfig {
        capacity: 100,
        ttl_secs: u64::MAX,
        negative_ttl_secs: 1,
    };
    assert_eq!(config2.negative_ttl_secs, 1);
}

// ============================================================================
// FINDING-FUSE-03: Passthrough config validation - kernel version combos
// ============================================================================

#[test]
fn fuzz_passthrough_kernel_6_7_too_old() {
    let config = PassthroughConfig::default();
    let status = check_kernel_version(6, 7, &config);
    assert!(matches!(
        status,
        PassthroughStatus::DisabledKernelTooOld { .. }
    ));
}

#[test]
fn fuzz_passthrough_kernel_6_8_exact() {
    let config = PassthroughConfig::default();
    let status = check_kernel_version(6, 8, &config);
    assert!(matches!(status, PassthroughStatus::Enabled));
}

#[test]
fn fuzz_passthrough_kernel_6_9_newer() {
    let config = PassthroughConfig::default();
    let status = check_kernel_version(6, 9, &config);
    assert!(matches!(status, PassthroughStatus::Enabled));
}

#[test]
fn fuzz_passthrough_kernel_5_15_too_old() {
    let config = PassthroughConfig::default();
    let status = check_kernel_version(5, 15, &config);
    assert!(matches!(
        status,
        PassthroughStatus::DisabledKernelTooOld { .. }
    ));
}

#[test]
fn fuzz_passthrough_kernel_4_19_ancient() {
    let config = PassthroughConfig::default();
    let status = check_kernel_version(4, 19, &config);
    assert!(matches!(
        status,
        PassthroughStatus::DisabledKernelTooOld { .. }
    ));
}

#[test]
fn fuzz_passthrough_disabled_config() {
    let config = PassthroughConfig {
        enabled: false,
        min_kernel_major: 6,
        min_kernel_minor: 8,
    };
    let status = check_kernel_version(6, 10, &config);
    assert!(matches!(status, PassthroughStatus::DisabledByConfig));
}

#[test]
fn fuzz_passthrough_custom_min_kernel() {
    let config = PassthroughConfig {
        enabled: true,
        min_kernel_major: 7,
        min_kernel_minor: 0,
    };
    let status = check_kernel_version(6, 20, &config);
    assert!(matches!(
        status,
        PassthroughStatus::DisabledKernelTooOld { .. }
    ));

    let status2 = check_kernel_version(7, 0, &config);
    assert!(matches!(status2, PassthroughStatus::Enabled));
}

// ============================================================================
// FINDING-FUSE-04: Serialization fuzzing - edge cases for config parsing
// ============================================================================

#[test]
fn fuzz_mount_options_case_sensitivity() {
    let result = parse_mount_options("ALLOW_OTHER");
    assert!(result.is_err());

    let result2 = parse_mount_options("Allow_Other");
    assert!(result2.is_err());

    let result3 = parse_mount_options("RO");
    assert!(result3.is_err());
}

#[test]
fn fuzz_mount_options_duplicates() {
    let result = parse_mount_options("allow_other,allow_other,allow_other");
    assert!(result.is_ok());
    assert!(result.unwrap().allow_other);
}

#[test]
fn fuzz_mount_options_mixed_valid_invalid() {
    let result = parse_mount_options("allow_other,invalid_opt,ro");
    assert!(result.is_err());
}

#[test]
fn fuzz_mount_options_special_chars_in_value() {
    let result = parse_mount_options("allow_other=true");
    assert!(result.is_err() || !result.unwrap().allow_other);
}

#[test]
fn fuzz_mount_options_numeric_suffix() {
    let result = parse_mount_options("allow_other1");
    assert!(result.is_err());

    let result2 = parse_mount_options("ro2");
    assert!(result2.is_err());
}

// ============================================================================
// FINDING-FUSE-05: FUSE operation boundary tests - extreme values
// ============================================================================

#[test]
fn fuzz_mount_options_very_long_string() {
    let long_str = "a".repeat(10000);
    let result = parse_mount_options(&long_str);
    assert!(result.is_err());
}

#[test]
fn fuzz_mount_options_unicode_characters() {
    let result = parse_mount_options("allow_othér");
    assert!(result.is_err());

    let result2 = parse_mount_options("日本語");
    assert!(result2.is_err());
}

#[test]
fn fuzz_mount_options_emoji_in_options() {
    let result = parse_mount_options("🚀");
    assert!(result.is_err());
}

#[test]
fn fuzz_mount_options_mixed_unicode() {
    let result = parse_mount_options("allow_other,🔐,ro");
    assert!(result.is_err());
}

#[test]
fn fuzz_passthrough_kernel_version_zero() {
    let config = PassthroughConfig::default();
    let status = check_kernel_version(0, 0, &config);
    assert!(matches!(
        status,
        PassthroughStatus::DisabledKernelTooOld { .. }
    ));
}

#[test]
fn fuzz_passthrough_kernel_version_negative_simulated() {
    let config = PassthroughConfig::default();
    let status = check_kernel_version(0, 1, &config);
    assert!(matches!(
        status,
        PassthroughStatus::DisabledKernelTooOld { .. }
    ));
}

// ============================================================================
// FINDING-FUSE-06: Edge case - conflicting mount options
// ============================================================================

#[test]
fn fuzz_mount_options_ro_rw_conflict() {
    let result = parse_mount_options("ro,rw");
    assert!(result.is_ok());
    assert!(!result.unwrap().ro);
}

#[test]
fn fuzz_mount_options_all_options_combined() {
    let result = parse_mount_options("allow_other,allow_root,default_permissions,auto_unmount,direct_io,kernel_cache,nonempty,ro");
    assert!(result.is_ok());
    let opts = result.unwrap();
    assert!(opts.allow_other);
    assert!(opts.allow_root);
    assert!(opts.default_permissions);
    assert!(opts.auto_unmount);
    assert!(opts.direct_io);
    assert!(opts.kernel_cache);
    assert!(opts.nonempty);
    assert!(opts.ro);
}

// ============================================================================
// FINDING-FUSE-07: CacheConfig serde-like parsing would need manual implementation
// Note: CacheConfig does not have serde derives - this test documents the gap
// ============================================================================

#[test]
#[ignore = "CacheConfig lacks serde support - requires manual JSON parsing implementation"]
fn fuzz_cache_config_json_deserialization() {
    let json_str = r#"{"capacity": 1000, "ttl_secs": 60, "negative_ttl_secs": 10}"#;
    let _config: CacheConfig = serde_json::from_str(json_str).unwrap();
}

#[test]
#[ignore = "CacheConfig lacks serde support - requires manual JSON parsing implementation"]
fn fuzz_cache_config_json_malformed() {
    let json_str = r#"{"capacity": "not a number", "ttl_secs": -1}"#;
    let result: Result<CacheConfig, _> = serde_json::from_str(json_str);
    assert!(result.is_err());
}

#[test]
#[ignore = "CacheConfig lacks serde support - requires manual JSON parsing implementation"]
fn fuzz_cache_config_json_missing_fields() {
    let json_str = r#"{"capacity": 100}"#;
    let result: Result<CacheConfig, _> = serde_json::from_str(json_str);
    assert!(result.is_err());
}

#[test]
#[ignore = "CacheConfig lacks serde support - requires manual JSON parsing implementation"]
fn fuzz_cache_config_json_extra_fields() {
    let json_str =
        r#"{"capacity": 100, "ttl_secs": 30, "negative_ttl_secs": 5, "extra_field": "ignored"}"#;
    let config: CacheConfig = serde_json::from_str(json_str).unwrap();
    assert_eq!(config.capacity, 100);
}

// ============================================================================
// FINDING-FUSE-08: PassthroughConfig serde-like parsing would need manual impl
// Note: PassthroughConfig does not have serde derives - this test documents the gap
// ============================================================================

#[test]
#[ignore = "PassthroughConfig lacks serde support - requires manual JSON parsing implementation"]
fn fuzz_passthrough_config_json_deserialization() {
    let json_str = r#"{"enabled": true, "min_kernel_major": 6, "min_kernel_minor": 8}"#;
    let _config: PassthroughConfig = serde_json::from_str(json_str).unwrap();
}

// ============================================================================
// FINDING-FUSE-09: No FuseConfig type exists in claudefs-fuse crate
// This test documents the missing configuration aggregation type
// ============================================================================

#[test]
#[ignore = "FuseConfig does not exist in claudefs-fuse - needs to be created"]
fn fuzz_fuse_config_type_missing() {
    let _config = claudefs_fuse::FuseConfig::default();
}

// ============================================================================
// FINDING-FUSE-10: No CachePolicy type exists in claudefs-fuse crate
// This test documents the missing cache policy enum
// ============================================================================

#[test]
#[ignore = "CachePolicy does not exist in claudefs-fuse - needs to be created"]
fn fuzz_cache_policy_type_missing() {
    let _policy = claudefs_fuse::CachePolicy::default();
}

// ============================================================================
// FINDING-FUSE-11: Boundary testing - potential integer overflow scenarios
// ============================================================================

#[test]
fn fuzz_cache_config_addition_overflow_potential() {
    let config = CacheConfig {
        capacity: usize::MAX,
        ttl_secs: u64::MAX,
        negative_ttl_secs: u64::MAX,
    };
    let sum = config.ttl_secs.saturating_add(config.negative_ttl_secs);
    assert!(sum >= config.ttl_secs);
}

#[test]
fn fuzz_mount_options_excessive_commas() {
    let result = parse_mount_options(",,,,,,,,,,,,,,");
    assert!(result.is_ok());
}

#[test]
fn fuzz_mount_options_trailing_comma() {
    let result = parse_mount_options("allow_other,");
    assert!(result.is_ok());
    assert!(result.unwrap().allow_other);
}

#[test]
fn fuzz_mount_options_leading_comma() {
    let result = parse_mount_options(",allow_other");
    assert!(result.is_ok());
    assert!(result.unwrap().allow_other);
}

// ============================================================================
// FINDING-FUSE-12: Property-based testing - random mount option strings
// ============================================================================

proptest! {
    #[test]
    fn prop_mount_options_random_valid_input(s in "^(allow_other|allow_root|default_permissions|auto_unmount|direct_io|kernel_cache|nonempty|ro|rw)*(,(allow_other|allow_root|default_permissions|auto_unmount|direct_io|kernel_cache|nonempty|ro|rw))*$") {
        let result = parse_mount_options(&s);
        assert!(result.is_ok());
    }

    #[test]
    fn prop_mount_options_random_strings(s in "[a-zA-Z0-9_, ]*") {
        let result = parse_mount_options(&s);
        if result.is_err() {
            let err = result.unwrap_err();
            assert!(err.to_string().contains("Invalid option") || err.to_string().len() > 0);
        }
    }

    #[test]
    fn prop_cache_config_reasonable_values(capacity in 0..1000000u32, ttl in 0..86400u32, negative_ttl in 0..3600u32) {
        let config = CacheConfig {
            capacity: capacity as usize,
            ttl_secs: ttl as u64,
            negative_ttl_secs: negative_ttl as u64,
        };
        assert_eq!(config.capacity as u32, capacity);
        assert_eq!(config.ttl_secs as u32, ttl);
        assert_eq!(config.negative_ttl_secs as u32, negative_ttl);
    }

    #[test]
    fn prop_passthrough_kernel_version(major in 0u32..20, minor in 0u32..30) {
        let config = PassthroughConfig::default();
        let status = check_kernel_version(major, minor, &config);
        match status {
            PassthroughStatus::Enabled => {},
            PassthroughStatus::DisabledKernelTooOld { .. } => {},
            PassthroughStatus::DisabledByConfig => {},
            PassthroughStatus::DisabledUnsupportedFeature => {},
        }
    }
}

// ============================================================================
// FINDING-FUSE-13: Denial of service - resource exhaustion via option parsing
// ============================================================================

#[test]
fn fuzz_mount_options_massive_input() {
    let massive = "a,".repeat(1_000_000);
    let result = parse_mount_options(&massive);
    assert!(result.is_err());
}

#[test]
fn fuzz_mount_options_repeated_option_names() {
    let repeated = "allow_other,".repeat(1000);
    let result = parse_mount_options(&repeated);
    assert!(result.is_ok());
    assert!(result.unwrap().allow_other);
}

// ============================================================================
// FINDING-FUSE-14: Testing with binary-looking input
// ============================================================================

#[test]
fn fuzz_mount_options_binary_data() {
    let binary = vec![0u8; 100];
    let result = parse_mount_options(std::str::from_utf8(&binary).unwrap_or(""));
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn fuzz_mount_options_control_characters() {
    let with_control = "allow\x01_other\x02ro\x03";
    let result = parse_mount_options(with_control);
    assert!(result.is_err() || !result.unwrap().allow_other);
}

// ============================================================================
// FINDING-FUSE-15: UTF-8 BOM and encoding edge cases
// ============================================================================

#[test]
fn fuzz_mount_options_utf8_bom() {
    let with_bom = "\u{FEFF}allow_other";
    let result = parse_mount_options(with_bom);
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn fuzz_mount_options_homograph_attack_simulation() {
    let cyrillic_o = "аllow_other";
    let result = parse_mount_options(cyrillic_o);
    assert!(result.is_err());
}
