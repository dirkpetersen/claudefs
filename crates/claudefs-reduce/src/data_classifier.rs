//! Content-aware data classification for optimal compression algorithm selection.
//!
//! The classifier detects data type from content, enabling the pipeline to:
//! - Skip compression for already-compressed data (video, JPEG, ZIP, etc.)
//! - Use fast LZ4 for hot data, Zstd for cold data going to S3
//! - Use delta compression for version-controlled text files

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Classification of data content type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataClass {
    /// Plain text (ASCII/UTF-8).
    Text,
    /// Generic binary data.
    Binary,
    /// Already compressed media (JPEG, PNG, MP4, ZIP, etc.).
    CompressedMedia,
    /// Executable binary (ELF, PE).
    Executable,
    /// Structured data (JSON, XML, CSV, Parquet).
    StructuredData,
    /// Unknown or insufficient data.
    Unknown,
}

/// Hint for which compression algorithm to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionHint {
    /// Skip compression (already compressed or encrypted).
    SkipCompression,
    /// Use LZ4 (fast, for hot data).
    UseLz4,
    /// Use Zstd (better ratio, for cold data/S3).
    UseZstd,
    /// Use delta compression (for similar data).
    UseDelta,
}

/// Result of data classification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassificationResult {
    /// Detected data class.
    pub class: DataClass,
    /// Confidence level (0.0 to 1.0).
    pub confidence: f64,
    /// Suggested compression algorithm.
    pub compression_hint: CompressionHint,
}

/// Stateless data classifier.
pub struct DataClassifier;

impl DataClassifier {
    /// Classify data based on its content.
    ///
    /// Inspects the first 512 bytes (or less) for magic bytes and entropy.
    pub fn classify(data: &[u8]) -> ClassificationResult {
        if data.is_empty() {
            return ClassificationResult {
                class: DataClass::Unknown,
                confidence: 0.0,
                compression_hint: CompressionHint::UseLz4,
            };
        }

        let inspect_len = data.len().min(512);
        let inspect_data = &data[..inspect_len];

        if let Some(result) = Self::check_magic_bytes(inspect_data) {
            return result;
        }

        if let Some(result) = Self::check_structured_data(data) {
            return result;
        }

        let entropy = Self::entropy(inspect_data);

        if entropy > 7.5 {
            return ClassificationResult {
                class: DataClass::CompressedMedia,
                confidence: 0.8,
                compression_hint: CompressionHint::SkipCompression,
            };
        }

        if entropy < 2.5 && Self::is_printable_ascii(inspect_data) {
            return ClassificationResult {
                class: DataClass::Text,
                confidence: 0.9,
                compression_hint: CompressionHint::UseZstd,
            };
        }

        ClassificationResult {
            class: DataClass::Binary,
            confidence: 0.7,
            compression_hint: CompressionHint::UseLz4,
        }
    }

    /// Compute Shannon entropy of data (0.0 = all same byte, 8.0 = max random).
    pub fn entropy(data: &[u8]) -> f64 {
        if data.is_empty() {
            return 0.0;
        }

        let mut counts: HashMap<u8, usize> = HashMap::new();
        for &byte in data {
            *counts.entry(byte).or_insert(0) += 1;
        }

        let total = data.len() as f64;
        let mut entropy = 0.0;

        for &count in counts.values() {
            if count > 0 {
                let p = count as f64 / total;
                entropy -= p * p.log2();
            }
        }

        entropy
    }

    /// Get the compression hint for a data class.
    pub fn compression_hint_for(class: DataClass) -> CompressionHint {
        match class {
            DataClass::Text => CompressionHint::UseZstd,
            DataClass::Binary => CompressionHint::UseLz4,
            DataClass::CompressedMedia => CompressionHint::SkipCompression,
            DataClass::Executable => CompressionHint::UseLz4,
            DataClass::StructuredData => CompressionHint::UseZstd,
            DataClass::Unknown => CompressionHint::UseLz4,
        }
    }

    fn check_magic_bytes(data: &[u8]) -> Option<ClassificationResult> {
        if data.len() < 3 {
            return None;
        }

        if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
            return Some(ClassificationResult {
                class: DataClass::CompressedMedia,
                confidence: 0.95,
                compression_hint: CompressionHint::SkipCompression,
            });
        }

        if data.len() >= 4 && data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
            return Some(ClassificationResult {
                class: DataClass::CompressedMedia,
                confidence: 0.95,
                compression_hint: CompressionHint::SkipCompression,
            });
        }

        if data.len() >= 4
            && (data.starts_with(&[0x50, 0x4B, 0x03, 0x04])
                || data.starts_with(&[0x50, 0x4B, 0x05, 0x06]))
        {
            return Some(ClassificationResult {
                class: DataClass::CompressedMedia,
                confidence: 0.95,
                compression_hint: CompressionHint::SkipCompression,
            });
        }

        if data.len() >= 4 && data.starts_with(&[0x7F, 0x45, 0x4C, 0x46]) {
            return Some(ClassificationResult {
                class: DataClass::Executable,
                confidence: 0.95,
                compression_hint: CompressionHint::UseLz4,
            });
        }

        if data.len() >= 2 && data.starts_with(&[0x4D, 0x5A]) {
            return Some(ClassificationResult {
                class: DataClass::Executable,
                confidence: 0.90,
                compression_hint: CompressionHint::UseLz4,
            });
        }

        None
    }

    fn check_structured_data(data: &[u8]) -> Option<ClassificationResult> {
        let first_non_ws = data.iter().position(|&b| !b.is_ascii_whitespace())?;

        let first_char = data[first_non_ws];

        if first_char == b'{' || first_char == b'[' {
            return Some(ClassificationResult {
                class: DataClass::StructuredData,
                confidence: 0.85,
                compression_hint: CompressionHint::UseZstd,
            });
        }

        if first_char == b'<' {
            return Some(ClassificationResult {
                class: DataClass::StructuredData,
                confidence: 0.85,
                compression_hint: CompressionHint::UseZstd,
            });
        }

        None
    }

    /// Check if data is mostly printable ASCII.
    ///
    /// Returns true if >= 80% of bytes are printable ASCII (0x20-0x7E, 0x09, 0x0A, 0x0D).
    pub fn is_printable_ascii(data: &[u8]) -> bool {
        if data.is_empty() {
            return true;
        }

        let printable_count = data
            .iter()
            .filter(|&&b| (0x20..=0x7E).contains(&b) || b == 0x09 || b == 0x0A || b == 0x0D)
            .count();

        (printable_count as f64 / data.len() as f64) >= 0.8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_jpeg_magic_bytes() {
        let data = [0xFFu8, 0xD8, 0xFF, 0x00, 0x00, 0x00];
        let result = DataClassifier::classify(&data);
        assert_eq!(result.class, DataClass::CompressedMedia);
        assert_eq!(result.compression_hint, CompressionHint::SkipCompression);
    }

    #[test]
    fn test_classify_png_magic_bytes() {
        let data = [0x89u8, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        let result = DataClassifier::classify(&data);
        assert_eq!(result.class, DataClass::CompressedMedia);
        assert_eq!(result.compression_hint, CompressionHint::SkipCompression);
    }

    #[test]
    fn test_classify_zip_magic_bytes() {
        let data = [0x50u8, 0x4B, 0x03, 0x04, 0x00, 0x00];
        let result = DataClassifier::classify(&data);
        assert_eq!(result.class, DataClass::CompressedMedia);
        assert_eq!(result.compression_hint, CompressionHint::SkipCompression);
    }

    #[test]
    fn test_classify_zip_empty_magic_bytes() {
        let data = [0x50u8, 0x4B, 0x05, 0x06, 0x00, 0x00];
        let result = DataClassifier::classify(&data);
        assert_eq!(result.class, DataClass::CompressedMedia);
        assert_eq!(result.compression_hint, CompressionHint::SkipCompression);
    }

    #[test]
    fn test_classify_elf_binary() {
        let data = [0x7Fu8, 0x45, 0x4C, 0x46, 0x02, 0x01, 0x01, 0x00];
        let result = DataClassifier::classify(&data);
        assert_eq!(result.class, DataClass::Executable);
        assert_eq!(result.compression_hint, CompressionHint::UseLz4);
    }

    #[test]
    fn test_classify_pe_binary() {
        let data = [0x4Du8, 0x5A, 0x90, 0x00, 0x03, 0x00];
        let result = DataClassifier::classify(&data);
        assert_eq!(result.class, DataClass::Executable);
        assert_eq!(result.compression_hint, CompressionHint::UseLz4);
    }

    #[test]
    fn test_classify_json_text() {
        let data = br#"{"key": "value"}"#;
        let result = DataClassifier::classify(data);
        assert_eq!(result.class, DataClass::StructuredData);
        assert_eq!(result.compression_hint, CompressionHint::UseZstd);
    }

    #[test]
    fn test_classify_json_array() {
        let data = br#"[1, 2, 3]"#;
        let result = DataClassifier::classify(data);
        assert_eq!(result.class, DataClass::StructuredData);
        assert_eq!(result.compression_hint, CompressionHint::UseZstd);
    }

    #[test]
    fn test_classify_xml_text() {
        let data = br#"<?xml version="1.0"?><root/>"#;
        let result = DataClassifier::classify(data);
        assert_eq!(result.class, DataClass::StructuredData);
        assert_eq!(result.compression_hint, CompressionHint::UseZstd);
    }

    #[test]
    fn test_classify_html_text() {
        let data = br#"<!DOCTYPE html><html><body></body></html>"#;
        let result = DataClassifier::classify(data);
        assert_eq!(result.class, DataClass::StructuredData);
        assert_eq!(result.compression_hint, CompressionHint::UseZstd);
    }

    #[test]
    fn test_classify_plain_text() {
        let data = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let result = DataClassifier::classify(data);
        assert_eq!(result.class, DataClass::Text);
        assert_eq!(result.compression_hint, CompressionHint::UseZstd);
    }

    #[test]
    fn test_classify_high_entropy() {
        let mut data = [0u8; 512];
        for (i, byte) in data.iter_mut().enumerate() {
            *byte = (i % 256) as u8;
        }

        let entropy = DataClassifier::entropy(&data);
        assert!(entropy > 7.5);

        let result = DataClassifier::classify(&data);
        assert_eq!(result.class, DataClass::CompressedMedia);
        assert_eq!(result.compression_hint, CompressionHint::SkipCompression);
    }

    #[test]
    fn test_classify_empty_data() {
        let result = DataClassifier::classify(&[]);
        assert_eq!(result.class, DataClass::Unknown);
    }

    #[test]
    fn test_entropy_zero_byte_array() {
        let data = [0u8; 512];
        let entropy = DataClassifier::entropy(&data);
        assert!((entropy - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_entropy_uniform_distribution() {
        let mut data = [0u8; 256];
        for i in 0..256 {
            data[i] = i as u8;
        }

        let entropy = DataClassifier::entropy(&data);
        assert!((entropy - 8.0).abs() < 0.01);
    }

    #[test]
    fn test_entropy_binary_data() {
        let data: Vec<u8> = (0..128).flat_map(|i| vec![i, i, i, i]).collect();
        let entropy = DataClassifier::entropy(&data);
        assert!(entropy > 0.0 && entropy < 8.0);
    }

    #[test]
    fn test_compression_hint_for_all_classes() {
        assert_eq!(
            DataClassifier::compression_hint_for(DataClass::Text),
            CompressionHint::UseZstd
        );
        assert_eq!(
            DataClassifier::compression_hint_for(DataClass::Binary),
            CompressionHint::UseLz4
        );
        assert_eq!(
            DataClassifier::compression_hint_for(DataClass::CompressedMedia),
            CompressionHint::SkipCompression
        );
        assert_eq!(
            DataClassifier::compression_hint_for(DataClass::Executable),
            CompressionHint::UseLz4
        );
        assert_eq!(
            DataClassifier::compression_hint_for(DataClass::StructuredData),
            CompressionHint::UseZstd
        );
        assert_eq!(
            DataClassifier::compression_hint_for(DataClass::Unknown),
            CompressionHint::UseLz4
        );
    }

    #[test]
    fn test_is_printable_ascii_true() {
        let data = b"Hello, world! This is printable ASCII text.\n\tNewline and tab.";
        assert!(DataClassifier::is_printable_ascii(data));
    }

    #[test]
    fn test_is_printable_ascii_false() {
        let data = [0x00, 0x01, 0x02, 0x03, 0xFF, 0xFE, 0xFD];
        assert!(!DataClassifier::is_printable_ascii(&data));
    }

    #[test]
    fn test_is_printable_ascii_mixed() {
        let data = b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0A\x0B\x0C\x0D\x0E\x0F\x10Hello";
        assert!(!DataClassifier::is_printable_ascii(data));
    }

    #[test]
    fn test_classify_small_data_8_bytes() {
        let data = [0x7Fu8, 0x45, 0x4C, 0x46, 0x00, 0x00, 0x00, 0x00];
        let result = DataClassifier::classify(&data);
        assert_eq!(result.class, DataClass::Executable);
    }

    #[test]
    fn test_entropy_empty() {
        let entropy = DataClassifier::entropy(&[]);
        assert!((entropy - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_entropy_single_byte() {
        let entropy = DataClassifier::entropy(&[0x42]);
        assert!((entropy - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_is_printable_ascii_empty() {
        assert!(DataClassifier::is_printable_ascii(&[]));
    }

    #[test]
    fn test_classify_json_whitespace_prefix() {
        let data = b"   \n\t  {\"key\": \"value\"}";
        let result = DataClassifier::classify(data);
        assert_eq!(result.class, DataClass::StructuredData);
    }

    #[test]
    fn test_classify_xml_whitespace_prefix() {
        let data = b"   \n  <?xml version=\"1.0\"?>";
        let result = DataClassifier::classify(data);
        assert_eq!(result.class, DataClass::StructuredData);
    }
}
