# Protocol-Specific Implementation Notes

## NFSv3 (RFC 1813)

### Supported Operations

| Operation | Code | Support | Notes |
|-----------|------|---------|-------|
| NULL | 0 | Full | No-op |
| GETATTR | 1 | Full | Returns file attributes |
| SETATTR | 2 | Partial | truncate, set times |
| LOOKUP | 3 | Full | Directory lookup |
| ACCESS | 4 | Partial | Check permissions |
| READLINK | 5 | None | symlink read |
| READ | 6 | Full | File read |
| WRITE | 7 | Full | File write (unstable only) |
| CREATE | 8 | Partial | create with attributes |
| MKDIR | 9 | Full | Create directory |
| SYMLINK | 10 | None | Create symlink |
| MKNOD | 11 | None | Special files |
| REMOVE | 12 | Full | Delete file |
| RMDIR | 13 | Full | Delete directory |
| RENAME | 14 | Partial | Basic rename |
| LINK | 15 | None | Hard links |
| READDIR | 16 | Full | Directory listing |
| READDIRPLUS | 17 | Full | With attributes |
| FSSTAT | 18 | Partial | Filesystem stats |
| FSINFO | 19 | Full | Server capabilities |
| PATHCONF | 20 | Full | POSIX info |
| COMMIT | 21 | Full | Sync to stable storage |

### File Handle Format

File handles are inode-based for stability:
```rust
// Internal format (32 bytes)
struct FileHandle3 {
    fsid: u64,           // Filesystem ID
    inode: u64,          // Inode number
    generation: u32,     // Inode generation
    verification: u32,   // Checksum
}
```

### Write Modes

| Mode | Stable | Description |
|------|--------|-------------|
| 0 | UNSTABLE | Server may delay commit |
| 1 | DATA_SYNC | Sync to disk, metadata async |
| 2 | FILE_SYNC | All sync |

Current implementation uses UNSTABLE (0) for performance.

### Known Limitations

- No support for ACLs (use NFSv4)
- No support for security labels
- WRITE mode UNSTABLE only (no true FILE_SYNC)

## NFSv4 Session Management

### Session Structure

```rust
struct Nfsv4Session {
    session_id: u64,
    channel: u32,
    state: SessionState,
    seqid: u32,
}
```

### Session Recovery

When client reconnects:
1. Client sends EXCHANGE_ID with previous session ID
2. Server returns correct state (OK, NEED_RECOVERY, SEQUENCE)
3. Client performs SEQUENCE to recover slot positions

### Delegation

The gateway supports NFSv4.1/4.2 delegations for read caching:

```rust
// Delegation types
enum DelegationType {
    None,
    Read,       // Client can cache reads
    Write,      // Client can cache reads/writes
}

// Delegation recall on conflict
// 1. Another client opens for write
// 2. Server sends CB_RECALL
// 3. Client must flush and return delegation
```

### pNFS Integration

NFSv4.1+ supports parallel NFS (pNFS):

```rust
// Layout types supported
enum LayoutType {
    LAYOUT4_NFSV4_1_FILES,  // Object/stripe across data servers
    LAYOUT4_FLEX_FILES,     // Flexible storage container
}
```

## pNFS Layout Server

### Layout Operations

| Operation | Description |
|-----------|-------------|
| GETDEVICEINFO | Get data server addresses |
| LAYOUTGET | Get file layout |
| LAYOUTCOMMIT | Commit layout changes |
| LAYOUTRETURN | Return unused layout |

### Flexible Files Layout (pnfs_flex.rs)

ClaudeFS uses Flexible Files layout for pNFS:

```rust
// Layout structure
struct FlexFilesLayout {
    devices: Vec<DataServerAddr>,
    stripe_width: u32,
    object_id: u64,
}
```

### Layout Recovery

When data server fails:
1. Client detects I/O error
2. Client requests new layout via LAYOUTGET
3. Server returns layout without failed server
4. Client retries on new data servers

## S3 API Compliance

### Supported Operations

| Operation | Status | Notes |
|-----------|--------|-------|
| GET Object | Full | Read object data |
| HEAD Object | Full | Get metadata |
| PUT Object | Full | Create/replace object |
| DELETE Object | Full | Delete object |
| List Objects v1 | Full | Basic listing |
| List Objects v2 | Full | With continuation |
| POST Object | Full | Anonymous upload |
| Copy Object | Full | Server-side copy |
| Initiate Multipart | Full | Start upload |
| Upload Part | Full | Upload chunk |
| Complete Multipart | Full | Finalize upload |
| Abort Multipart | Full | Cancel upload |
| List Multipart | Full | List parts |
| Create Bucket | Full | Create container |
| Delete Bucket | Full | Remove bucket |
| Get Bucket Policy | Full | Access policy |
| Put Bucket Policy | Full | Set access policy |
| Get Bucket Versioning | Full | Versioning status |
| Put Bucket Versioning | Full | Enable/disable |
| Get Object ACL | Full | Access list |
| Put Object ACL | Full | Set ACL |

### Limitations

- **Presigned URLs**: Expiration limited to 7 days max
- **Select Object Content**: Not supported
- **Inventory**: Not supported
- **Batch Operations**: Not supported
- **Replication**: Not supported (use A6)
- **Object Lock**: Limited (WORM not enforced at protocol level)

### Multipart Upload

S3 multipart upload flow:
```
1. CreateMultipartUpload → upload_id
2. UploadPart (repeat, 1-10000 parts)
3. CompleteMultipartUpload → combines parts
```

Part requirements:
- Minimum 5MB (except last part)
- Maximum 10000 parts
- Part ETag stored for completion

### Bucket Policies

Supported policy conditions:
```json
{
  "Condition": {
    "IpAddress": {"aws:SourceIp": "10.0.0.0/8"},
    "Bool": {"aws:SecureTransport": "true"}
  }
}
```

### Storage Classes

| Class | Description | Use Case |
|-------|-------------|----------|
| STANDARD | Default | Frequent access |
| STANDARD_IA | Infrequent | < 30 day retention |
| GLACIER | Archive | > 90 day retention |

## SMB3 Protocol

### SMB3 Negotiation

```
Client                    Gateway
   │                         │
   │──NEGOTIATE─────────────>│
   │<─PROTOCOL VERSION ──────│ (SMB 3.1.1 max)
   │                         │
   │──SESSION SETUP─────────>│
   │<─AUTH CHALLENGE────────│
   │──SESSION KEY───────────>│
   │<─SESSION SUCCESS───────│
   │                         │
   │──TREE CONNECT──────────>│
   │<─TREE ID────────────────│
```

### Authentication

Supported authentication:
- NTLM (legacy)
- Kerberos (recommended)
- SPNEGO

### Multi-Channel (smb_multichannel.rs)

SMB3.0+ supports multiple connections per session:

**Requirements:**
- Multiple network interfaces (or NIC with RSS)
- Compatible signing (all channels must use same key)

**Configuration:**
```yaml
smb:
  multichannel:
    enabled: true
    max_channels: 4
    min_channels: 1
```

**Channel Selection:**
- Round-robin across interfaces
- Weighted by link speed
- Prefer RDMA-capable interfaces

### Multi-Protocol Issues

#### File Locking

NFS and SMB use different locking mechanisms:
- NFS: POSIX flock via NLM
- SMB: SMB2 LOCK

**Conflict Resolution:**
- A7 tracks locks internally per protocol
- Cross-protocol conflicts handled via A2 metadata
- Not recommended for simultaneous NFS+SMB access

#### Case Sensitivity

| Protocol | Behavior |
|----------|----------|
| NFS | Case-sensitive |
| SMB | Case-insensitive |

**Resolution:**
- Gateway normalizes paths to lowercase for SMB
- NFS clients see original case
- Not recommended for simultaneous access

#### Unicode Normalization

SMB requires UTF-16LE with specific normalization:
- File names normalized to NFC
- Gateway handles normalization transparently

## Performance Characteristics

### NFS Latency Breakdown

```
Client         Gateway        A2 (Meta)      A4 (Storage)
  │              │               │               │
  │──READ───────>│               │               │
  │              │──GETATTR─────>│               │
  │              │<─metadata────│               │
  │              │              │               │
  │              │              │──READ────────>│
  │              │              │<─data─────────│
  │<─data───────│               │               │
  
典型延迟:
- 客户端→网关: 0.1ms (LAN)
- 网关→A2 GETATTR: 0.5-2ms
- A2→A4 数据读取: 0.2-1ms
- 总计: 1-4ms
```

### S3 Latency Expectations

```
GET Object (1KB):   1-3ms
GET Object (1MB):   3-10ms
PUT Object (1KB):   2-4ms  
PUT Object (1MB):   10-30ms
List Objects (100): 10-50ms
```

### SMB Latency Expectations

```
SMBDirect (RDMA):
- READ:  0.2-0.5ms
- WRITE: 0.2-0.5ms

TCP:
- READ:  0.5-2ms
- WRITE: 0.5-2ms
```

## References

- [RFC 1813](https://tools.ietf.org/html/rfc1813) - NFSv3
- [RFC 5661](https://tools.ietf.org/html/rfc5661) - NFSv4.1
- [MS-SMB2](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-smb2/) - SMB2/3
- [AWS S3 API](https://docs.aws.amazon.com/AmazonS3/latest/API/Welcome.html) - S3 API

## See Also

- [Architecture](ARCHITECTURE.md) - System design
- [Integration Guide](INTEGRATION_GUIDE.md) - Configuration
- [Performance Tuning](PERFORMANCE_TUNING.md) - Optimization
- [Operations Runbook](OPERATIONS_RUNBOOK.md) - Management