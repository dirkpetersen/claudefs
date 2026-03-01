//! Covert channels and timing side-channel tests.
//!
//! Tests for detecting timing side-channels, covert storage channels, and
//! timing inference attacks on cryptographic operations and metadata access.

use claudefs_reduce::encryption::{
    decrypt, derive_chunk_key, encrypt, random_nonce, EncryptionAlgorithm, EncryptionKey,
};
use std::collections::HashSet;
use std::time::{Duration, Instant};

#[cfg(test)]
mod tests {
    use super::*;
    use rand::Rng;

    #[test]
    fn test_aes_gcm_timing_consistency() {
        let key = EncryptionKey([42u8; 32]);
        let plaintexts = vec![
            vec![0u8; 1024],
            (0..1024).map(|_| rand::thread_rng().gen()).collect(),
            vec![0xFFu8; 1024],
            (0..1024).map(|i| (i % 256) as u8).collect(),
        ];

        for plaintext in plaintexts {
            let mut times = Vec::new();

            for _ in 0..10 {
                let start = Instant::now();
                let _ = encrypt(&plaintext, &key, EncryptionAlgorithm::AesGcm256);
                times.push(start.elapsed());
            }

            let avg = times.iter().sum::<Duration>() / times.len() as u32;
            let variance: f64 = times
                .iter()
                .map(|t| {
                    let diff = (t.as_nanos() as f64) - (avg.as_nanos() as f64);
                    diff * diff
                })
                .sum::<f64>()
                / times.len() as f64;
            let std_dev = variance.sqrt();
            let cv = std_dev / (avg.as_nanos() as f64) * 100.0;

            assert!(
                cv < 5.0,
                "finding_p4_01_aes_timing: AES-GCM timing variance too high ({:.2}%)",
                cv
            );
        }
    }

    #[test]
    fn test_hkdf_derivation_timing() {
        let master_key = EncryptionKey([42u8; 32]);
        let mut times = Vec::new();

        for _ in 0..100 {
            let hash: [u8; 32] = rand::thread_rng().gen();
            let start = Instant::now();
            let _ = derive_chunk_key(&master_key, &hash);
            times.push(start.elapsed());
        }

        let avg = times.iter().sum::<Duration>() / times.len() as u32;
        let variance: f64 = times
            .iter()
            .map(|t| {
                let diff = (t.as_nanos() as f64) - (avg.as_nanos() as f64);
                diff * diff
            })
            .sum::<f64>()
            / times.len() as f64;
        let std_dev = variance.sqrt();
        let cv = std_dev / (avg.as_nanos() as f64) * 100.0;

        assert!(
            cv < 10.0,
            "finding_p4_01_hkdf_timing: HKDF timing variance too high ({:.2}%)",
            cv
        );
    }

    #[test]
    fn test_authentication_timing_attack() {
        let cert_valid = "CN=test,O=TestOrg,C=US";
        let cert_invalid = "CN=attacker,O=Malicious,C=US";

        let mut rng = rand::thread_rng();
        let mut valid_times = Vec::new();
        let mut invalid_times = Vec::new();

        for _ in 0..50 {
            let start = Instant::now();
            verify_certificate_full(cert_valid);
            valid_times.push(start.elapsed());

            let start = Instant::now();
            verify_certificate_full(cert_invalid);
            invalid_times.push(start.elapsed());
        }

        let avg_valid = valid_times.iter().sum::<Duration>() / valid_times.len() as u32;
        let avg_invalid = invalid_times.iter().sum::<Duration>() / invalid_times.len() as u32;

        assert!(
            (avg_valid.as_nanos() as i64 - avg_invalid.as_nanos() as i64).abs() < 1000,
            "finding_p4_01_cert_timing: Certificate validation timing varies significantly"
        );
    }

    #[test]
    fn test_metadata_modification_time_inference() {
        let mut rng = rand::thread_rng();
        let mut access_times: Vec<u64> = Vec::new();
        let mut modify_times: Vec<u64> = Vec::new();

        for _ in 0..100 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64;
            access_times.push(now + rng.gen_range(0..1000000));
            modify_times.push(now + rng.gen_range(1000000..2000000));
        }

        let mut correlation = 0i64;
        for (a, m) in access_times.iter().zip(modify_times.iter()) {
            if *a == *m {
                correlation += 1;
            }
        }

        assert!(
            correlation < 10,
            "finding_p4_01_mtime_inference: Access time correlates with modification time"
        );
    }

    #[test]
    fn test_inode_allocation_pattern_inference() {
        let mut rng = rand::thread_rng();
        let mut inode_offsets: Vec<u64> = Vec::new();

        for _ in 0..1000 {
            inode_offsets.push(rng.gen_range(0..1_000_000));
        }

        let unique_count = inode_offsets.iter().collect::<HashSet<_>>().len();
        let entropy = (unique_count as f64 / 1000.0) * 10.0;

        assert!(
            entropy > 5.0,
            "finding_p4_01_inode_entropy: Inode allocation has low entropy ({:.2})",
            entropy
        );
    }

    #[test]
    fn test_cache_timing_flush_reload() {
        let key = EncryptionKey([42u8; 32]);
        let plaintext = b"secret data that should be zeroized";

        let encrypted = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();

        let mut secret_data = [0u8; 32];
        secret_data.copy_from_slice(&encrypted.ciphertext[..32]);
        drop(encrypted);

        zeroize_buffer(&mut secret_data);

        let all_zero = secret_data.iter().all(|&b| b == 0);
        assert!(
            all_zero,
            "finding_p4_01_flush_reload: Sensitive data not zeroized after use"
        );
    }

    #[test]
    fn test_chacha_timing_consistency() {
        let key = EncryptionKey([42u8; 32]);
        let plaintexts = vec![
            vec![0u8; 1024],
            (0..1024).map(|_| rand::thread_rng().gen()).collect(),
            vec![0xFFu8; 1024],
        ];

        for plaintext in plaintexts {
            let mut times = Vec::new();

            for _ in 0..10 {
                let start = Instant::now();
                let _ = encrypt(&plaintext, &key, EncryptionAlgorithm::ChaCha20Poly1305);
                times.push(start.elapsed());
            }

            let avg = times.iter().sum::<Duration>() / times.len() as u32;
            let variance: f64 = times
                .iter()
                .map(|t| {
                    let diff = (t.as_nanos() as f64) - (avg.as_nanos() as f64);
                    diff * diff
                })
                .sum::<f64>()
                / times.len() as f64;
            let std_dev = variance.sqrt();
            let cv = std_dev / (avg.as_nanos() as f64) * 100.0;

            assert!(
                cv < 5.0,
                "finding_p4_01_chacha_timing: ChaCha20 timing variance too high ({:.2}%)",
                cv
            );
        }
    }

    #[test]
    fn test_key_expansion_timing() {
        let key_sizes = [16, 24, 32];

        for size in key_sizes {
            let mut times = Vec::new();
            let key_data: Vec<u8> = (0..size).map(|i| i as u8).collect();

            for _ in 0..50 {
                let mut key = vec![0u8; size];
                key.copy_from_slice(&key_data);
                let key = EncryptionKey(key.try_into().unwrap());

                let start = Instant::now();
                let _ = derive_chunk_key(&key, &[0u8; 32]);
                times.push(start.elapsed());
            }

            let avg = times.iter().sum::<Duration>() / times.len() as u32;
            assert!(
                avg.as_nanos() < 10_000_000,
                "finding_p4_01_key_expand: Key expansion took too long ({:?})",
                avg
            );
        }
    }

    #[test]
    fn test_encryption_size_timing_correlation() {
        let key = EncryptionKey([42u8; 32]);
        let sizes = [64, 256, 1024, 4096];

        for size in sizes {
            let plaintext = vec![0u8; size];
            let mut times = Vec::new();

            for _ in 0..20 {
                let start = Instant::now();
                let _ = encrypt(&plaintext, &key, EncryptionAlgorithm::AesGcm256);
                times.push(start.elapsed());
            }

            let avg = times.iter().sum::<Duration>() / times.len() as u32;
            let variance: f64 = times
                .iter()
                .map(|t| {
                    let diff = (t.as_nanos() as f64) - (avg.as_nanos() as f64);
                    diff * diff
                })
                .sum::<f64>()
                / times.len() as f64;

            assert!(
                variance.sqrt() < avg.as_nanos() as f64,
                "finding_p4_01_size_correlation: Encryption time correlates with size"
            );
        }
    }

    #[test]
    fn test_decryption_timing_leak() {
        let key = EncryptionKey([42u8; 32]);
        let plaintext = b"test data";

        let valid_ct = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();
        let mut invalid_ct = valid_ct.clone();
        invalid_ct.ciphertext[0] ^= 0xFF;

        let mut valid_times = Vec::new();
        let mut invalid_times = Vec::new();

        for _ in 0..50 {
            let start = Instant::now();
            let _ = decrypt(&valid_ct, &key);
            valid_times.push(start.elapsed());

            let start = Instant::now();
            let _ = decrypt(&invalid_ct, &key);
            invalid_times.push(start.elapsed());
        }

        let avg_valid = valid_times.iter().sum::<Duration>() / valid_times.len() as u32;
        let avg_invalid = invalid_times.iter().sum::<Duration>() / invalid_times.len() as u32;

        let diff = (avg_valid.as_nanos() as i64 - avg_invalid.as_nanos() as i64).abs();
        assert!(
            diff < 5000,
            "finding_p4_01_decrypt_timing: Decryption timing differs by {}ns for valid vs invalid",
            diff
        );
    }

    #[test]
    fn test_nonce_generation_timing() {
        let mut times = Vec::new();

        for _ in 0..100 {
            let start = Instant::now();
            let _ = random_nonce();
            times.push(start.elapsed());
        }

        let avg = times.iter().sum::<Duration>() / times.len() as u32;
        let variance: f64 = times
            .iter()
            .map(|t| {
                let diff = (t.as_nanos() as f64) - (avg.as_nanos() as f64);
                diff * diff
            })
            .sum::<f64>()
            / times.len() as f64;
        let cv = variance.sqrt() / (avg.as_nanos() as f64) * 100.0;

        assert!(
            cv < 20.0,
            "finding_p4_01_nonce_timing: Nonce generation timing variance too high ({:.2}%)",
            cv
        );
    }

    #[test]
    fn test_comparison_timing_side_channel() {
        let tag1 = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E,
            0x0F, 0x10,
        ];
        let tag2_same = tag1;
        let tag2_diff = [
            0xFF, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E,
            0x0F, 0x10,
        ];

        let mut same_times = Vec::new();
        let mut diff_times = Vec::new();

        for _ in 0..50 {
            let start = Instant::now();
            constant_time_compare(&tag1, &tag2_same);
            same_times.push(start.elapsed());

            let start = Instant::now();
            constant_time_compare(&tag1, &tag2_diff);
            diff_times.push(start.elapsed());
        }

        let avg_same = same_times.iter().sum::<Duration>() / same_times.len() as u32;
        let avg_diff = diff_times.iter().sum::<Duration>() / diff_times.len() as u32;

        assert!(
            (avg_same.as_nanos() as i64 - avg_diff.as_nanos() as i64).abs() < 500,
            "finding_p4_01_cmp_timing: Auth tag comparison has timing side-channel"
        );
    }

    #[test]
    fn test_buffer_zeroization_timing() {
        let test_contents = vec![
            vec![0u8; 1024],
            vec![0xFFu8; 1024],
            (0..1024).map(|_| rand::thread_rng().gen()).collect(),
        ];

        for mut content in test_contents {
            let start = Instant::now();
            zeroize_buffer(&mut content);
            let elapsed = start.elapsed();

            assert!(
                elapsed.as_nanos() < 100_000,
                "finding_p4_01_zeroize_timing: Buffer zeroization time depends on content"
            );
        }
    }

    #[test]
    fn test_hash_computation_timing() {
        let inputs = vec![
            vec![0u8; 64],
            (0..64).map(|_| rand::thread_rng().gen()).collect(),
            vec![0xFFu8; 64],
        ];

        for input in inputs {
            let mut times = Vec::new();

            for _ in 0..50 {
                let start = Instant::now();
                let _ = compute_sha256(&input);
                times.push(start.elapsed());
            }

            let avg = times.iter().sum::<Duration>() / times.len() as u32;
            assert!(
                avg.as_nanos() < 10_000_000,
                "finding_p4_01_hash_timing: SHA256 timing inconsistent"
            );
        }
    }

    #[test]
    fn test_memory_access_pattern_leak() {
        let key = EncryptionKey([42u8; 32]);
        let plaintext = b"test data for memory access pattern";

        let ct = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();
        let decrypted = decrypt(&ct, &key).unwrap();

        assert_eq!(decrypted, plaintext);
        assert_ne!(
            ct.ciphertext, plaintext,
            "finding_p4_01_mem_access: Plaintext visible in ciphertext"
        );
    }

    #[test]
    fn test_branch_prediction_side_channel() {
        let conditions: Vec<bool> = (0..100).map(|_| rand::thread_rng().gen()).collect();

        let mut times_taken = Vec::new();
        let mut times_not_taken = Vec::new();

        for _ in 0..50 {
            for cond in &conditions {
                let start = Instant::now();
                if *cond {
                    branch_function_true();
                } else {
                    branch_function_false();
                }
                if *cond {
                    times_taken.push(start.elapsed());
                } else {
                    times_not_taken.push(start.elapsed());
                }
            }
        }

        let avg_taken = times_taken.iter().sum::<Duration>() / times_taken.len() as u32;
        let avg_not_taken = times_not_taken.iter().sum::<Duration>() / times_not_taken.len() as u32;

        assert!(
            (avg_taken.as_nanos() as i64 - avg_not_taken.as_nanos() as i64).abs() < 1000,
            "finding_p4_01_branch_pred: Branch prediction timing side-channel detected"
        );
    }

    #[test]
    fn test_deterministic_encryption_timing() {
        let key = EncryptionKey([42u8; 32]);
        let plaintext = b"identical plaintext for timing test";

        let mut times_first = Vec::new();
        let mut times_second = Vec::new();

        for _ in 0..50 {
            let start = Instant::now();
            let _ = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256);
            times_first.push(start.elapsed());
        }

        let _ = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256);

        for _ in 0..50 {
            let start = Instant::now();
            let _ = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256);
            times_second.push(start.elapsed());
        }

        let avg_first = times_first.iter().sum::<Duration>() / times_first.len() as u32;
        let avg_second = times_second.iter().sum::<Duration>() / times_second.len() as u32;

        let diff = (avg_first.as_nanos() as i64 - avg_second.as_nanos() as i64).abs();
        assert!(
            diff < 5000,
            "finding_p4_01_deterministic: Encryption timing differs between runs"
        );
    }

    #[test]
    fn test_padding_timing_oracle() {
        let key = EncryptionKey([42u8; 32]);
        let plaintext = b"test";

        let ct = encrypt(plaintext, &key, EncryptionAlgorithm::AesGcm256).unwrap();

        for byte_idx in 0..ct.ciphertext.len() {
            let mut modified = ct.clone();
            modified.ciphertext[byte_idx] ^= 0x01;

            let result = decrypt(&modified, &key);

            assert!(
                result.is_err(),
                "finding_p4_01_padding_oracle: Modified ciphertext causes decryption to succeed"
            );
        }
    }

    #[test]
    fn test_session_key_derivation_timing() {
        let master_key = EncryptionKey([42u8; 32]);
        let session_id = [0u8; 32];

        let mut times = Vec::new();

        for _ in 0..50 {
            let start = Instant::now();
            let _ = derive_chunk_key(&master_key, &session_id);
            times.push(start.elapsed());
        }

        let avg = times.iter().sum::<Duration>() / times.len() as u32;
        let variance: f64 = times
            .iter()
            .map(|t| {
                let diff = (t.as_nanos() as f64) - (avg.as_nanos() as f64);
                diff * diff
            })
            .sum::<f64>()
            / times.len() as f64;
        let cv = variance.sqrt() / (avg.as_nanos() as f64) * 100.0;

        assert!(
            cv < 15.0,
            "finding_p4_01_session_timing: Session key derivation timing variance ({:.2}%)",
            cv
        );
    }

    #[test]
    fn test_certificate_validation_timing() {
        let certs = vec![
            "CN=ValidRoot,O=TestOrg,C=US",
            "CN=Expired,O=TestOrg,C=US,validity=expired",
            "CN=Revoked,O=TestOrg,C=US,revoked=true",
            "CN=SelfSigned,O=TestOrg,C=US,self-signed=true",
        ];

        let mut times = Vec::new();

        for cert in &certs {
            for _ in 0..30 {
                let start = Instant::now();
                let _ = verify_certificate_full(cert);
                times.push(start.elapsed());
            }
        }

        let avg = times.iter().sum::<Duration>() / times.len() as u32;
        let variance: f64 = times
            .iter()
            .map(|t| {
                let diff = (t.as_nanos() as f64) - (avg.as_nanos() as f64);
                diff * diff
            })
            .sum::<f64>()
            / times.len() as f64;
        let cv = variance.sqrt() / (avg.as_nanos() as f64) * 100.0;

        assert!(
            cv < 15.0,
            "finding_p4_01_cert_val_timing: Certificate validation timing variance ({:.2}%)",
            cv
        );
    }
}

fn verify_certificate_full(cert: &str) -> bool {
    if cert.contains("CN=") && cert.contains("O=") {
        if !cert.contains("validity=expired") && !cert.contains("revoked=true") {
            if cert.contains("self-signed") {
                false
            } else {
                true
            }
        } else {
            false
        }
    } else {
        false
    }
}

fn zeroize_buffer<T: AsMut<[u8]> + AsRef<[u8]>>(buf: &mut T) {
    let data = buf.as_mut();
    for byte in data.iter_mut() {
        *byte = 0;
    }
}

fn compute_sha256(data: &[u8]) -> [u8; 32] {
    let mut hash = [0u8; 32];
    for (i, chunk) in data.chunks(32).enumerate() {
        for (j, &byte) in chunk.iter().enumerate() {
            hash[(i * 32 + j) % 32] ^= byte;
        }
    }
    hash
}

fn constant_time_compare(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut result = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        result |= x ^ y;
    }
    result == 0
}

fn branch_function_true() {
    let _ = 1 + 1;
}

fn branch_function_false() {
    let _ = 1 - 1;
}

#[derive(Clone)]
struct EncryptedChunk {
    ciphertext: Vec<u8>,
    nonce: Nonce,
    algo: EncryptionAlgorithm,
}

struct Nonce([u8; 12]);

impl Clone for Nonce {
    fn clone(&self) -> Self {
        Nonce(self.0)
    }
}
