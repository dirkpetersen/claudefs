//! Security audit tests for A6 replication conduit authentication.
//!
//! Findings: FINDING-05 through FINDING-09
//! These tests verify security properties (and document known weaknesses)
//! in the cross-site replication conduit.

use claudefs_repl::conduit::{Conduit, ConduitConfig, ConduitTlsConfig, EntryBatch};
use claudefs_repl::journal::{JournalEntry, OpKind};

fn make_entry(seq: u64, inode: u64) -> JournalEntry {
    JournalEntry::new(seq, 0, 1, 1000 + seq, inode, OpKind::Write, vec![1, 2, 3])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finding_05_default_conduit_config_has_no_tls() {
        let config = ConduitConfig::default();
        assert!(
            config.tls.is_none(),
            "FINDING-05: Default conduit has no TLS — violates D7 mTLS requirement"
        );
    }

    #[test]
    fn finding_05_conduit_pair_operates_without_tls() {
        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);
        assert!(config_a.tls.is_none());
        assert!(config_b.tls.is_none());
        let (_a, _b) = Conduit::new_pair(config_a, config_b);
    }

    #[tokio::test]
    async fn finding_06_spoofed_site_id_accepted() {
        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);
        let (conduit_a, conduit_b) = Conduit::new_pair(config_a, config_b);

        let entries = vec![make_entry(1, 100)];
        let spoofed_batch = EntryBatch::new(999, entries, 1);

        conduit_a.send_batch(spoofed_batch).await.unwrap();
        let received = conduit_b.recv_batch().await.unwrap();
        assert_eq!(
            received.source_site_id, 999,
            "FINDING-06: Spoofed site_id accepted without validation"
        );
    }

    #[tokio::test]
    async fn finding_06_batch_from_wrong_site_not_rejected() {
        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);
        let (conduit_a, conduit_b) = Conduit::new_pair(config_a, config_b);

        for site_id in [0u64, 3, u64::MAX] {
            let entries = vec![make_entry(1, 100)];
            let batch = EntryBatch::new(site_id, entries, 1);
            conduit_a.send_batch(batch).await.unwrap();
            let received = conduit_b.recv_batch().await.unwrap();
            assert_eq!(received.source_site_id, site_id);
        }
    }

    #[test]
    fn finding_07_entry_batch_has_no_integrity_field() {
        let entries = vec![make_entry(1, 100)];
        let batch = EntryBatch::new(1, entries, 1);
        assert_eq!(batch.source_site_id, 1);
        assert_eq!(batch.batch_seq, 1);
        let mut tampered = batch.clone();
        tampered.source_site_id = 999;
        assert_ne!(batch, tampered);
    }

    #[test]
    fn finding_08_tls_key_material_stored_as_plain_vec() {
        let key_data = vec![0x42u8; 32];
        let tls = ConduitTlsConfig::new(b"cert".to_vec(), key_data.clone(), b"ca".to_vec());
        assert_eq!(
            tls.key_pem, key_data,
            "FINDING-08: Private key stored as plain Vec<u8>"
        );
    }

    #[tokio::test]
    async fn finding_09_no_rate_limiting_on_conduit() {
        let config_a = ConduitConfig {
            max_batch_size: 10000,
            ..ConduitConfig::new(1, 2)
        };
        let config_b = ConduitConfig {
            max_batch_size: 10000,
            ..ConduitConfig::new(2, 1)
        };
        let (conduit_a, conduit_b) = Conduit::new_pair(config_a, config_b);

        for i in 0..100u64 {
            let entries = vec![make_entry(i, 100)];
            let batch = EntryBatch::new(1, entries, i);
            conduit_a.send_batch(batch).await.unwrap();
        }

        let stats = conduit_a.stats();
        assert_eq!(
            stats.batches_sent, 100,
            "FINDING-09: All 100 batches accepted — no rate limiting"
        );

        for _ in 0..100 {
            let _ = conduit_b.recv_batch().await;
        }
    }

    #[test]
    fn conduit_tls_config_accepts_invalid_pem() {
        let tls = ConduitTlsConfig::new(
            b"not-a-valid-cert".to_vec(),
            b"not-a-valid-key".to_vec(),
            b"not-a-valid-ca".to_vec(),
        );
        assert_eq!(tls.cert_pem, b"not-a-valid-cert");
    }

    #[test]
    fn conduit_tls_config_accepts_empty_key() {
        let tls = ConduitTlsConfig::new(vec![], vec![], vec![]);
        assert!(tls.cert_pem.is_empty());
        assert!(tls.key_pem.is_empty());
        assert!(tls.ca_pem.is_empty());
    }

    #[tokio::test]
    async fn conduit_send_after_peer_shutdown() {
        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);
        let (conduit_a, conduit_b) = Conduit::new_pair(config_a, config_b);

        conduit_b.shutdown().await;
        let entries = vec![make_entry(1, 100)];
        let batch = EntryBatch::new(1, entries, 1);
        let _ = conduit_a.send_batch(batch).await;
    }

    #[test]
    fn conduit_config_clone_preserves_tls() {
        let tls = ConduitTlsConfig::new(b"cert".to_vec(), b"key".to_vec(), b"ca".to_vec());
        let mut config = ConduitConfig::new(1, 2);
        config.tls = Some(tls);
        let cloned = config.clone();
        assert!(cloned.tls.is_some());
        assert_eq!(cloned.tls.unwrap().cert_pem, b"cert");
    }

    #[tokio::test]
    async fn conduit_bidirectional_batch_flow() {
        let config_a = ConduitConfig::new(1, 2);
        let config_b = ConduitConfig::new(2, 1);
        let (conduit_a, conduit_b) = Conduit::new_pair(config_a, config_b);

        let entries_a = vec![make_entry(1, 100)];
        let batch_a = EntryBatch::new(1, entries_a, 1);
        conduit_a.send_batch(batch_a).await.unwrap();

        let entries_b = vec![make_entry(10, 200)];
        let batch_b = EntryBatch::new(2, entries_b, 1);
        conduit_b.send_batch(batch_b).await.unwrap();

        let recv_a = conduit_b.recv_batch().await.unwrap();
        let recv_b = conduit_a.recv_batch().await.unwrap();

        assert_eq!(recv_a.source_site_id, 1);
        assert_eq!(recv_b.source_site_id, 2);
    }

    #[test]
    fn entry_batch_equality_checks_all_fields() {
        let entries = vec![make_entry(1, 100)];
        let batch1 = EntryBatch::new(1, entries.clone(), 1);
        let batch2 = EntryBatch::new(1, entries, 1);
        assert_eq!(batch1, batch2);

        let batch3 = EntryBatch::new(2, vec![make_entry(1, 100)], 1);
        assert_ne!(batch1, batch3);
    }
}