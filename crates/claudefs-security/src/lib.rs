// FILE: lib.rs
#![warn(missing_docs)]

//! ClaudeFS security audit crate: fuzzing harnesses, crypto property tests,
//! transport validation, and audit tooling.
//!
//! This crate is owned by A10 (Security Audit Agent) and provides:
//! - Protocol frame fuzzing (malformed frames, oversized payloads, invalid opcodes)
//! - Message deserialization fuzzing (unbounded strings, OOM vectors, type confusion)
//! - Cryptographic security property tests (nonce uniqueness, key isolation, timing)
//! - Transport validation tests (frame boundaries, checksum bypass, flag abuse)
//! - Audit report types for tracking findings

pub mod audit;
#[cfg(test)]
pub mod api_security_tests;
#[cfg(test)]
pub mod api_pentest_tests;
#[cfg(test)]
pub mod conduit_auth_tests;
#[cfg(test)]
pub mod crypto_tests;
pub mod fuzz_message;
pub mod fuzz_protocol;
#[cfg(test)]
pub mod gateway_auth_tests;
#[cfg(test)]
pub mod gateway_nfs_rpc_security_tests;
#[cfg(test)]
pub mod transport_tests;
#[cfg(test)]
pub mod unsafe_review_tests;
#[cfg(test)]
pub mod unsafe_audit;
#[cfg(test)]
pub mod crypto_audit;
#[cfg(test)]
pub mod crypto_zeroize_audit;
#[cfg(test)]
pub mod mgmt_pentest;
#[cfg(test)]
pub mod fuzz_fuse;
#[cfg(test)]
pub mod dep_audit;
#[cfg(test)]
pub mod dos_resilience;
#[cfg(test)]
pub mod supply_chain;
#[cfg(test)]
pub mod operational_security;
#[cfg(test)]
pub mod advanced_fuzzing;
#[cfg(test)]
pub mod phase2_audit;
#[cfg(test)]
pub mod meta_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod meta_access_xattr_security_tests;
#[cfg(test)]
pub mod gateway_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod gateway_copy_referral_security_tests;
#[cfg(test)]
pub mod fuse_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod fuse_barrier_policy_security_tests;
#[cfg(test)]
pub mod fuse_ext_security_tests;
#[cfg(test)]
pub mod storage_encryption_tests;
#[cfg(test)]
pub mod mgmt_rbac_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod storage_deep_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod storage_deep_security_tests_v2;
#[cfg(test)]
pub mod gateway_s3_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod gateway_s3_ver_multi_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod gateway_s3_notif_repl_class_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod meta_deep_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod transport_deep_security_tests;
#[cfg(test)]
pub mod transport_conn_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod transport_pipeline_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod repl_deep_security_tests_v2;
#[cfg(test)]
#[allow(missing_docs)]
pub mod fuse_deep_security_tests;
#[cfg(test)]
pub mod gateway_protocol_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod gateway_export_mount_portmap_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod meta_consensus_security_tests;
#[cfg(test)]
pub mod mgmt_extended_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod storage_erasure_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod gateway_infra_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod fuse_cache_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod repl_infra_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod storage_qos_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod meta_fsck_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod storage_device_ext_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod gateway_deleg_cache_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod reduce_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod reduce_deep_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod reduce_extended_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod repl_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod repl_failover_bootstrap_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod repl_phase2_security_tests;
    #[cfg(test)]
    #[allow(missing_docs)]
    pub mod repl_health_security_tests;
    #[cfg(test)]
    #[allow(missing_docs)]
    pub mod repl_qos_gc_security_tests;
    #[cfg(test)]
    #[allow(missing_docs)]
    pub mod repl_splitbrain_tls_security_tests;
    #[cfg(test)]
    #[allow(missing_docs)]
    pub mod transport_security_tests;
    #[cfg(test)]
    #[allow(missing_docs)]
    pub mod fuse_prefetch_health_security_tests;
    #[cfg(test)]
    #[allow(missing_docs)]
    pub mod gateway_wire_audit_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod meta_fingerprint_negcache_watch_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod gateway_metrics_health_stats_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod meta_transaction_lease_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod meta_worm_security_tests;
#[cfg(test)]
#[allow(missing_docs)]
pub mod meta_conflict_security_tests;
    #[cfg(test)]
    #[allow(missing_docs)]
    pub mod gateway_pnfs_s3router_security_tests;
    #[cfg(test)]
    #[allow(missing_docs)]
    pub mod gateway_perf_config_security_tests;