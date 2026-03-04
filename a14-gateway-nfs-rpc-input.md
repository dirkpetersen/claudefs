# Task: Write gateway_nfs_rpc_security_tests.rs for claudefs-security crate

Write a comprehensive security test module for the `claudefs-gateway` crate focusing on NFS write tracking, RPC protocol validation, and S3 XML building security.

## File location
`crates/claudefs-security/src/gateway_nfs_rpc_security_tests.rs`

## Module structure
```rust
//! Gateway NFS write/RPC/S3 XML security tests.
//!
//! Part of A10 Phase 14: Gateway NFS & RPC security audit

#[cfg(test)]
mod tests {
    // imports and tests here
}
```

## Available types (verified from source)

```rust
use claudefs_gateway::nfs_write::{WriteStability, PendingWrite, WriteTracker};
use claudefs_gateway::rpc::{
    OpaqueAuth, RpcCall, RpcReply, TcpRecordMark,
    NFS_PROGRAM, MOUNT_PROGRAM, NFS_VERSION, MOUNT_VERSION,
    RPC_CALL, RPC_REPLY, AUTH_NONE, AUTH_SYS, AUTH_GSS,
    ACCEPT_SUCCESS, ACCEPT_PROG_UNAVAIL, ACCEPT_PROC_UNAVAIL, ACCEPT_GARBAGE_ARGS,
    NFS3_NULL, NFS3_GETATTR, NFS3_READ, NFS3_WRITE, NFS3_COMMIT,
};
use claudefs_gateway::s3_xml::{XmlBuilder, error_xml, copy_object_xml, create_multipart_upload_xml, complete_multipart_upload_xml};
```

**IMPORTANT**: Not all may be public. If any import fails, remove it and skip those tests.

## Existing tests to AVOID duplicating
- `gateway_auth_tests.rs`: token authentication, AUTH_SYS credentials
- `gateway_security_tests.rs`: S3 validation, pNFS layouts, NFS auth squashing
- `gateway_s3_security_tests.rs`: S3 bucket policy, presigned URLs, rate limiting, encryption, multipart
- `gateway_protocol_security_tests.rs`: NFS v4 sessions, ACLs, S3 encryption, object lock, versioning, CORS
- `gateway_infra_security_tests.rs`: TLS config, circuit breaker, S3 lifecycle, connection pool, quota

DO NOT duplicate these. Focus on NFS write tracking, RPC protocol, S3 XML building.

## Test categories (25 tests total)

### Category 1: NFS Write Tracking (5 tests)

1. **test_write_tracker_record_and_pending** — Create WriteTracker::new(verf=12345). Record write for file handle b"fh1" at offset 0, count 100, Unstable stability. Verify has_pending_writes(b"fh1") returns true. Verify pending_count(b"fh1") == 1.

2. **test_write_tracker_commit** — Create tracker. Record 3 writes for b"fh1". Verify total_pending() == 3. Call commit(b"fh1"). Verify pending_count(b"fh1") == 0. Verify commit returns the write verifier.

3. **test_write_tracker_stability_ordering** — Verify WriteStability ordering: Unstable < DataSync < FileSync. Document that higher stability = more durable. (FINDING: stability ordering allows choosing minimum durability guarantee).

4. **test_write_tracker_multiple_files** — Create tracker. Record writes for b"fh1" and b"fh2". Verify total_pending() includes both. Commit b"fh1" only. Verify b"fh2" still has pending writes.

5. **test_write_tracker_commit_all** — Create tracker. Record writes for 3 different files. Call commit_all(). Verify total_pending() == 0. Verify all files have no pending writes.

### Category 2: RPC Protocol Security (5 tests)

6. **test_rpc_opaque_auth_none** — Create OpaqueAuth::none(). Verify flavor == AUTH_NONE. Verify body is empty.

7. **test_rpc_reply_encode_success** — Call RpcReply::encode_success(xid=42, result=b"ok"). Verify returned bytes contain xid 42 and ACCEPT_SUCCESS.

8. **test_rpc_reply_encode_proc_unavail** — Call RpcReply::encode_proc_unavail(xid=1). Verify returned bytes contain xid 1 and ACCEPT_PROC_UNAVAIL code.

9. **test_rpc_reply_encode_auth_error** — Call RpcReply::encode_auth_error(xid=5, stat=1). Verify bytes contain auth error encoding.

10. **test_rpc_constants_valid** — Verify NFS_PROGRAM == 100003. Verify MOUNT_PROGRAM == 100005. Verify NFS_VERSION == 3. Verify NFS3_NULL == 0. Verify NFS3_WRITE == 7. Verify NFS3_COMMIT == 21. (FINDING: constants match RFC 1813).

### Category 3: TCP Record Mark (5 tests)

11. **test_tcp_record_mark_encode** — Create data b"hello". Encode with TcpRecordMark::encode(). Verify result starts with 4-byte header. Verify header encodes is_last=true and length=5.

12. **test_tcp_record_mark_decode** — Encode known data. Take first 4 bytes. Decode with TcpRecordMark::decode(). Verify (is_last, length) matches expected values.

13. **test_tcp_record_mark_roundtrip** — Encode data. Extract header. Decode header. Verify decoded length matches original data length. (FINDING: record mark framing prevents message confusion).

14. **test_tcp_record_mark_empty** — Encode empty data b"". Decode header. Verify length == 0 and is_last == true. Document behavior for empty records.

15. **test_tcp_record_mark_max_fragment** — Create data of size 0x7FFFFFFF (max fragment, just check decode behavior). Construct header bytes manually with max length. Decode. Verify correct length returned.

### Category 4: S3 XML Builder Security (5 tests)

16. **test_xml_builder_basic** — Create XmlBuilder. Call header(). Open tag "Root". Add elem "Name" with value "test". Close "Root". Finish. Verify output contains XML declaration, proper tags, and value.

17. **test_xml_builder_escaping** — Create builder. Add elem with value containing `<>&"'`. Verify these are escaped in output (`&lt;`, `&gt;`, `&amp;`, `&quot;`, `&apos;`). (FINDING: XML escaping prevents injection).

18. **test_xml_error_response** — Call error_xml("NoSuchBucket", "The bucket does not exist", "/mybucket", "req-123"). Verify output contains all 4 values properly escaped and wrapped in Error tags.

19. **test_xml_multipart_upload** — Call create_multipart_upload_xml("mybucket", "mykey", "upload-123"). Verify output contains bucket, key, and upload ID. Verify well-formed XML.

20. **test_xml_copy_object** — Call copy_object_xml("etag-abc", "2026-01-01T00:00:00Z"). Verify output contains ETag and LastModified elements.

### Category 5: NFS Write Edge Cases & Integration (5 tests)

21. **test_write_tracker_verf_consistency** — Create tracker with verf=999. Verify write_verf() == 999. Record and commit writes. Verify write_verf() still returns 999 (verifier unchanged). (FINDING: verifier stability for NFS client crash recovery).

22. **test_write_tracker_remove_file** — Create tracker. Record writes for b"fh1". Call remove_file(b"fh1"). Verify has_pending_writes(b"fh1") returns false. Verify total_pending() decreased.

23. **test_write_tracker_pending_writes_list** — Create tracker. Record 3 writes for b"fh1" at offsets 0, 100, 200. Call pending_writes(b"fh1"). Verify returns 3 PendingWrite entries with correct offsets.

24. **test_xml_builder_elem_types** — Create builder. Open "Stats". Add elem_u64("Size", 12345). Add elem_u32("Count", 42). Add elem_bool("Enabled", true). Add elem_opt("Optional", None). Close "Stats". Verify output has correct values and no Optional tag.

25. **test_xml_builder_default** — Create XmlBuilder::default(). Verify it works same as XmlBuilder::new(). Build simple XML. Verify non-empty output.

## Implementation notes
- Use `fn make_xxx()` helper functions
- Mark findings with `// FINDING-GW-NFS-XX: description`
- If a type is not public, skip that test and add an alternative
- DO NOT use any async code — all tests are synchronous
- Use `assert!`, `assert_eq!`, `matches!`
- For WriteTracker: WriteTracker::new(verf)
- For TcpRecordMark: TcpRecordMark::encode(data) and TcpRecordMark::decode(header_bytes)
- For XmlBuilder: XmlBuilder::new() with method chaining

## Output format
Output ONLY the complete Rust source file. No explanations, no markdown fences.
