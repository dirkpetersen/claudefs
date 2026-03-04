# Fix claudefs-security test compilation errors

## Problem

The `claudefs-security` crate's tests are failing to compile with E0061 errors. The `AdminApi::new()` signature was changed in `claudefs-mgmt` to add a third parameter `index_dir: PathBuf`, but the security tests were not updated.

## Errors to fix

1. **E0061 (argument count mismatch)** in 5 locations:
   - `crates/claudefs-security/src/api_security_tests.rs:16` — `AdminApi::new(metrics, config)` should be `AdminApi::new(metrics, config, index_dir)`
   - `crates/claudefs-security/src/api_pentest_tests.rs:16` — same fix
   - `crates/claudefs-security/src/mgmt_pentest.rs:24` — same fix
   - `crates/claudefs-security/src/mgmt_pentest.rs:243` — same fix
   - `crates/claudefs-security/src/mgmt_pentest.rs:306` — same fix

2. **E0282 (type annotation needed)** in `crates/claudefs-security/src/phase2_audit.rs:84`:
   - `let mut p = None;` needs type annotation: `let mut p: Option<String> = None;` (or appropriate type)

3. **E0599 (method not found)** in `crates/claudefs-security/src/phase2_audit.rs:200`:
   - `c.value()` doesn't exist on `CertificateDer` — need to use the certificate parsing API directly from rustls_pki_types

## Files to fix

- `crates/claudefs-security/src/api_security_tests.rs` — add `index_dir` parameter
- `crates/claudefs-security/src/api_pentest_tests.rs` — add `index_dir` parameter
- `crates/claudefs-security/src/mgmt_pentest.rs` — add `index_dir` parameter (2 locations)
- `crates/claudefs-security/src/phase2_audit.rs` — fix type annotation and certificate parsing

## Approach

For all `AdminApi::new()` calls:
- Create a temporary directory using `std::env::temp_dir()` or `tempfile::tempdir()`
- Pass that directory as the third argument

For the type annotation issue:
- Examine the surrounding context to determine the correct type for `p`

For the certificate parsing issue:
- Use the proper rustls API for extracting certificate information, likely through `x509_parser` or direct rustls parsing

Run `cargo test -p claudefs-security --no-run` after making changes to verify compilation succeeds.
