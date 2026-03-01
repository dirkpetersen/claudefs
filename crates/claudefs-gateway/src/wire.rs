//! Wire protocol validation utilities

/// Validate an NFS file handle (must be 1-64 bytes)
pub fn validate_nfs_fh(data: &[u8]) -> crate::error::Result<()> {
    if data.is_empty() {
        return Err(crate::error::GatewayError::ProtocolError {
            reason: "file handle cannot be empty".to_string(),
        });
    }
    if data.len() > 64 {
        return Err(crate::error::GatewayError::ProtocolError {
            reason: "file handle exceeds 64 bytes".to_string(),
        });
    }
    Ok(())
}

/// Validate an NFS filename component (1-255 bytes, no null bytes, no '/')
pub fn validate_nfs_filename(name: &str) -> crate::error::Result<()> {
    if name.is_empty() {
        return Err(crate::error::GatewayError::ProtocolError {
            reason: "filename cannot be empty".to_string(),
        });
    }
    if name.len() > 255 {
        return Err(crate::error::GatewayError::ProtocolError {
            reason: "filename exceeds 255 bytes".to_string(),
        });
    }
    if name.contains('\0') {
        return Err(crate::error::GatewayError::ProtocolError {
            reason: "filename cannot contain null bytes".to_string(),
        });
    }
    if name.contains('/') {
        return Err(crate::error::GatewayError::ProtocolError {
            reason: "filename cannot contain '/'".to_string(),
        });
    }
    Ok(())
}

/// Validate an NFS path (must start with '/', no null bytes, max 1024 bytes)
pub fn validate_nfs_path(path: &str) -> crate::error::Result<()> {
    if !path.starts_with('/') {
        return Err(crate::error::GatewayError::ProtocolError {
            reason: "path must start with '/'".to_string(),
        });
    }
    if path.len() > 1024 {
        return Err(crate::error::GatewayError::ProtocolError {
            reason: "path exceeds 1024 bytes".to_string(),
        });
    }
    if path.contains('\0') {
        return Err(crate::error::GatewayError::ProtocolError {
            reason: "path cannot contain null bytes".to_string(),
        });
    }
    Ok(())
}

/// Validate an NFS read/write count (1 to 1MB)
pub fn validate_nfs_count(count: u32) -> crate::error::Result<()> {
    if count == 0 || count > 1_048_576 {
        return Err(crate::error::GatewayError::ProtocolError {
            reason: "count must be between 1 and 1MB".to_string(),
        });
    }
    Ok(())
}

/// Validate an S3 key (1-1024 bytes, valid UTF-8, no leading slash)
pub fn validate_s3_key(key: &str) -> crate::error::Result<()> {
    if key.is_empty() {
        return Err(crate::error::GatewayError::ProtocolError {
            reason: "S3 key cannot be empty".to_string(),
        });
    }
    if key.len() > 1024 {
        return Err(crate::error::GatewayError::ProtocolError {
            reason: "S3 key exceeds 1024 bytes".to_string(),
        });
    }
    if !key.is_ascii() {
        return Err(crate::error::GatewayError::ProtocolError {
            reason: "S3 key must be ASCII".to_string(),
        });
    }
    if key.starts_with('/') {
        return Err(crate::error::GatewayError::ProtocolError {
            reason: "S3 key cannot start with '/'".to_string(),
        });
    }
    Ok(())
}

/// Validate an S3 object size (0 to 5TB)
pub fn validate_s3_size(size: u64) -> crate::error::Result<()> {
    const MAX_SIZE: u64 = 5_497_558_138_880u64; // 5TB
    if size > MAX_SIZE {
        return Err(crate::error::GatewayError::ProtocolError {
            reason: "S3 object size exceeds 5TB".to_string(),
        });
    }
    Ok(())
}

/// Validate S3 multipart part number (1-10000)
pub fn validate_part_number(n: u32) -> crate::error::Result<()> {
    if n == 0 || n > 10000 {
        return Err(crate::error::GatewayError::ProtocolError {
            reason: "part number must be between 1 and 10000".to_string(),
        });
    }
    Ok(())
}

/// Validate an S3 upload ID (non-empty string)
pub fn validate_upload_id(id: &str) -> crate::error::Result<()> {
    if id.is_empty() {
        return Err(crate::error::GatewayError::ProtocolError {
            reason: "upload ID cannot be empty".to_string(),
        });
    }
    Ok(())
}

/// Parse an NFS mode bits value, return Unix permission string (e.g., "rwxr-xr--")
pub fn format_mode(mode: u32) -> String {
    let mut result = String::with_capacity(10);

    let user = (mode >> 6) & 0o7;
    let group = (mode >> 3) & 0o7;
    let other = mode & 0o7;

    result.push(if user & 0o4 != 0 { 'r' } else { '-' });
    result.push(if user & 0o2 != 0 { 'w' } else { '-' });
    result.push(if user & 0o1 != 0 { 'x' } else { '-' });

    result.push(if group & 0o4 != 0 { 'r' } else { '-' });
    result.push(if group & 0o2 != 0 { 'w' } else { '-' });
    result.push(if group & 0o1 != 0 { 'x' } else { '-' });

    result.push(if other & 0o4 != 0 { 'r' } else { '-' });
    result.push(if other & 0o2 != 0 { 'w' } else { '-' });
    result.push(if other & 0o1 != 0 { 'x' } else { '-' });

    result
}

/// Parse mode string like "755" (octal) or "rwxr-xr-x" to u32
pub fn parse_mode(s: &str) -> crate::error::Result<u32> {
    if s.len() == 3 && s.chars().all(|c| c.is_ascii_digit()) {
        let val =
            u32::from_str_radix(s, 8).map_err(|_| crate::error::GatewayError::ProtocolError {
                reason: "invalid octal mode".to_string(),
            })?;
        Ok(val)
    } else {
        let mut mode: u32 = 0;
        let chars: Vec<char> = s.chars().collect();

        if chars.len() != 9 {
            return Err(crate::error::GatewayError::ProtocolError {
                reason: "mode must be 9 characters (e.g., rwxr-xr-x) or 3 digits (e.g., 755)"
                    .to_string(),
            });
        }

        for (i, c) in chars.iter().enumerate() {
            let group = i / 3;
            let _pos_in_group = i % 3;
            let base = 8 - group * 3;
            match c {
                'r' => mode |= 1 << (base),
                'w' => mode |= 1 << (base - 1),
                'x' => mode |= 1 << (base - 2),
                '-' => {}
                _ => {
                    return Err(crate::error::GatewayError::ProtocolError {
                        reason: format!("invalid mode character: {}", c),
                    })
                }
            }
        }
        Ok(mode)
    }
}

/// Compute a simple ETag from data (format: "<hex-hash>")
pub fn compute_etag(data: &[u8]) -> String {
    let sum: u64 = data.iter().map(|&b| b as u64).sum();
    let hash = sum
        .wrapping_mul(0x9e3779b97f4a7c15)
        .wrapping_add(data.len() as u64);
    format!("{:032x}", hash)
}

/// Validate an ISO 8601 date string (basic check: has digits, dashes, T, colons, Z)
pub fn is_valid_iso8601(s: &str) -> bool {
    if s.len() < 20 {
        return false;
    }

    if !s
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        return false;
    }

    if !s.contains('-') || !s.contains('T') || !s.ends_with('Z') {
        return false;
    }

    true
}

/// Generate a correlation/request ID as hex string
pub fn generate_request_id(seed: u64) -> String {
    let id = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    format!("{:032x}", id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_nfs_fh_valid() {
        assert!(validate_nfs_fh(&[1, 2, 3]).is_ok());
        assert!(validate_nfs_fh(&[0u8; 64]).is_ok());
    }

    #[test]
    fn test_validate_nfs_fh_too_short() {
        let result = validate_nfs_fh(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_nfs_fh_too_long() {
        let result = validate_nfs_fh(&[0u8; 65]);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_nfs_filename_valid() {
        assert!(validate_nfs_filename("file.txt").is_ok());
        assert!(validate_nfs_filename("a").is_ok());
    }

    #[test]
    fn test_validate_nfs_filename_empty() {
        assert!(validate_nfs_filename("").is_err());
    }

    #[test]
    fn test_validate_nfs_filename_too_long() {
        assert!(validate_nfs_filename(&"a".repeat(256)).is_err());
    }

    #[test]
    fn test_validate_nfs_filename_contains_slash() {
        assert!(validate_nfs_filename("a/b").is_err());
    }

    #[test]
    fn test_validate_nfs_filename_contains_null() {
        assert!(validate_nfs_filename("a\0b").is_err());
    }

    #[test]
    fn test_validate_nfs_path_valid() {
        assert!(validate_nfs_path("/export/file").is_ok());
        assert!(validate_nfs_path("/").is_ok());
    }

    #[test]
    fn test_validate_nfs_path_no_leading_slash() {
        assert!(validate_nfs_path("export").is_err());
    }

    #[test]
    fn test_validate_nfs_path_too_long() {
        assert!(validate_nfs_path(&"/".repeat(1025)).is_err());
    }

    #[test]
    fn test_validate_nfs_path_contains_null() {
        assert!(validate_nfs_path("/a\0b").is_err());
    }

    #[test]
    fn test_validate_nfs_count_valid() {
        assert!(validate_nfs_count(1).is_ok());
        assert!(validate_nfs_count(4096).is_ok());
        assert!(validate_nfs_count(1_048_576).is_ok());
    }

    #[test]
    fn test_validate_nfs_count_zero() {
        assert!(validate_nfs_count(0).is_err());
    }

    #[test]
    fn test_validate_nfs_count_too_large() {
        assert!(validate_nfs_count(1_048_577).is_err());
    }

    #[test]
    fn test_validate_s3_key_valid() {
        assert!(validate_s3_key("file.txt").is_ok());
        assert!(validate_s3_key("folder/file.txt").is_ok());
    }

    #[test]
    fn test_validate_s3_key_empty() {
        assert!(validate_s3_key("").is_err());
    }

    #[test]
    fn test_validate_s3_key_leading_slash() {
        assert!(validate_s3_key("/file.txt").is_err());
    }

    #[test]
    fn test_validate_s3_key_non_ascii() {
        assert!(validate_s3_key("文件.txt").is_err());
    }

    #[test]
    fn test_validate_s3_size_valid() {
        assert!(validate_s3_size(0).is_ok());
        assert!(validate_s3_size(1000).is_ok());
    }

    #[test]
    fn test_validate_s3_size_too_large() {
        assert!(validate_s3_size(6_000_000_000_000u64).is_err());
    }

    #[test]
    fn test_validate_part_number_valid() {
        assert!(validate_part_number(1).is_ok());
        assert!(validate_part_number(5000).is_ok());
        assert!(validate_part_number(10000).is_ok());
    }

    #[test]
    fn test_validate_part_number_invalid() {
        assert!(validate_part_number(0).is_err());
        assert!(validate_part_number(10001).is_err());
    }

    #[test]
    fn test_validate_upload_id_valid() {
        assert!(validate_upload_id("abc123").is_ok());
    }

    #[test]
    fn test_validate_upload_id_empty() {
        assert!(validate_upload_id("").is_err());
    }

    #[test]
    fn test_format_mode_755() {
        assert_eq!(format_mode(0o755), "rwxr-xr-x");
    }

    #[test]
    fn test_format_mode_644() {
        assert_eq!(format_mode(0o644), "rw-r--r--");
    }

    #[test]
    fn test_format_mode_000() {
        assert_eq!(format_mode(0o000), "---------");
    }

    #[test]
    fn test_format_mode_777() {
        assert_eq!(format_mode(0o777), "rwxrwxrwx");
    }

    #[test]
    fn test_parse_mode_octal() {
        assert_eq!(parse_mode("755").unwrap(), 0o755);
        assert_eq!(parse_mode("644").unwrap(), 0o644);
    }

    #[test]
    fn test_parse_mode_string() {
        assert_eq!(parse_mode("rwxr-xr-x").unwrap(), 0o755);
        assert_eq!(parse_mode("rw-r--r--").unwrap(), 0o644);
        assert_eq!(parse_mode("---------").unwrap(), 0o000);
    }

    #[test]
    fn test_parse_mode_invalid() {
        assert!(parse_mode("xyz").is_err());
    }

    #[test]
    fn test_compute_etag() {
        let etag = compute_etag(b"hello");
        assert_eq!(etag.len(), 32);
    }

    #[test]
    fn test_compute_etag_empty() {
        let etag = compute_etag(b"");
        assert_eq!(etag.len(), 32);
    }

    #[test]
    fn test_is_valid_iso8601_valid() {
        assert!(is_valid_iso8601("2024-01-15T10:30:00Z"));
    }

    #[test]
    fn test_is_valid_iso8601_invalid() {
        assert!(!is_valid_iso8601("not-a-date"));
        assert!(!is_valid_iso8601("2024/01/15"));
    }

    #[test]
    fn test_generate_request_id() {
        let id = generate_request_id(12345);
        assert_eq!(id.len(), 32);
    }

    #[test]
    fn test_generate_request_id_different() {
        let id1 = generate_request_id(1);
        let id2 = generate_request_id(2);
        assert_ne!(id1, id2);
    }
}
