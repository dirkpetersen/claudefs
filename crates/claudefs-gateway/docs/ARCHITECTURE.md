# ClaudeFS Gateway (A7) Architecture

## Overview

The ClaudeFS Protocol Gateway (A7) provides multi-protocol access to the distributed storage system. It translates client protocols (NFSv3, NFSv4, pNFS, S3, SMB3) into internal operations and coordinates with the metadata service (A2) and transport layer (A4).

## System Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                         Client Access                               │
├─────────────────┬─────────────────┬─────────────┬──────────────────┤
│    NFSv3/v4     │      pNFS       │     S3      │      SMB3        │
│   (port 2049)   │   (layout ops)  │  (port 9000)│   (port 445)     │
└────────┬────────┴────────┬────────┴──────┬───────┴────────┬────────┘
         │                 │               │                │
         ▼                 ▼               ▼                ▼
┌─────────────────────────────────────────────────────────────────────┐
│                     Gateway Protocol Layer                          │
├─────────────┬─────────────┬─────────────┬─────────────┬────────────┤
│ nfs.rs      │  pnfs.rs    │   s3.rs     │   smb.rs    │ auth.rs    │
│ mount.rs    │ pnfs_flex   │ s3_router   │ smb_multi   │ token_auth │
│ nfs_cache   │             │ s3_multipart│             │ nfs_acl    │
└──────┬──────┴──────┬──────┴──────┬──────┴──────┬──────┴─────┬──────┘
       │             │             │             │            │
       ▼             │             ▼             │            ▼
┌─────────────────────────────────────────────────────────────────────┐
│                   Gateway Resilience Layer                          │
├────────────────────┬────────────────────┬───────────────────────────┤
│  gateway_conn_pool │ gateway_circuit_   │      quota.rs             │
│                    │    breaker         │                           │
└─────────┬──────────┴────────┬───────────┴───────────┬─────────────┘
          │                   │                       │
          ▼                   ▼                       ▼
┌─────────────────────────────────────────────────────────────────────┐
│                   Backend Communication                             │
├─────────────────────────────┬───────────────────────────────────────┤
│      A2: Metadata Service  │        A4: Transport Layer            │
│  - inode operations        │  - RDMA (libfabric)                   │
│  - file handles            │  - TCP (io_uring zero-copy)           │
│  - attributes              │  - Connection pooling                 │
└─────────────────────────────┴───────────────────────────────────────┘
```

## Integration with Other Subsystems

### A2: Metadata Service Integration

The gateway interfaces with A2 for all metadata operations:

| Operation | Gateway Module | A2 Interface |
|-----------|----------------|--------------|
| File lookup | `nfs.rs` | Inode lookup by name |
| Get attributes | `nfs.rs` | Inode stat |
| Create file | `nfs.rs` | Inode create |
| Remove file | `nfs.rs` | Inode unlink |
| Read directory | `nfs.rs` | Directory read |
| Set attributes | `nfs.rs` | Inode setattr |

**File Handle Architecture:**
- NFS file handles contain an inode number encoded via `FileHandle3::from_inode()`
- Gateway translates client file handles to internal inodes before querying A2
- Handles returned to clients are stable across gateway restarts (inode-based)

**Authentication Flow:**
1. Client presents credentials (UID/GID for NFS, access key for S3)
2. `auth.rs` validates and maps to internal identity
3. All A2 operations include the authenticated context

### A4: Transport Layer Integration

The gateway uses A4 for backend communication:

| Transport Type | Use Case | Configuration |
|----------------|----------|---------------|
| RDMA | High-performance clusters | `libfabric` provider auto-detection |
| TCP | General purpose | `io_uring` zero-copy sockets |

**Connection Pool (`gateway_conn_pool.rs`):**
```rust
// Default configuration
pub struct ConnPoolConfig {
    min_per_node: 2,           // Minimum connections per backend
    max_per_node: 10,          // Maximum connections per backend
    max_idle_ms: 300_000,      // 5 minutes idle timeout
    connect_timeout_ms: 5000,  // 5 second connect timeout
    health_check_interval_ms: 30_000,  // 30 second health checks
}
```

**Circuit Breaker (`gateway_circuit_breaker.rs`):**
```rust
pub struct CircuitBreakerConfig {
    failure_threshold: 5,      // Failures before opening circuit
    success_threshold: 2,      // Successes to close from half-open
    open_duration_ms: 30_000,  // Time before trying recovery
    timeout_ms: 5_000,         // Operation timeout
}
```

## Multi-Protocol Architecture

### Protocol Selection Logic

The gateway determines which protocol to use based on:

1. **Port binding**: Each protocol listens on a distinct port
   - NFSv3: 2049 (TCP)
   - MOUNT: 20048 (TCP)
   - S3: 9000 (HTTP)
   - SMB3: 445 (TCP)

2. **Request parsing**: HTTP requests are S3, binary RPC is NFS/MOUNT

3. **Fallback strategies**:
   - NFSv4 falls back to NFSv3 if unsupported operation
   - pNFS falls back to regular NFS if layout unavailable
   - SMB2/3 negotiate to SMB1 if needed

### NFSv3 Protocol Flow

```
Client              Gateway                   A2 (Metadata)
   │                   │                          │
   │──MOUNT /export───>│                          │
   │<──File Handle────│                          │
   │                  │                          │
   │──LOOKUP "file"──>│──lookup("/", "file")────>│
   │<─File Handle────│<─inode #1234─────────────│
   │                  │                          │
   │──GETATTR────────>│──getattr(1234)──────────>│
   │<─attributes─────│<─size, mtime, etc.───────│
   │                  │                          │
   │──READ───────────>│──read(1234, 0, 64KB)────>│
   │<─64KB data──────│<─data────────────────────│
```

### pNFS Layout Server

The gateway acts as a pNFS layout server, providing clients with direct access to storage nodes:

1. Client requests layout for a file
2. Gateway queries A2 for file extents and maps to data servers
3. Gateway returns layout with data server addresses
4. Client reads/writes directly to data servers (A4 transport)

### S3 API

S3 requests are translated to internal operations:

| S3 Operation | Internal Operation |
|--------------|-------------------|
| GET Object | read(file, offset, length) |
| PUT Object | write(file, 0, data) |
| HEAD Object | getattr(file) |
| List Objects | readdir(dir) |
| Delete Object | unlink(file) |
| Create Multipart | create + track parts |
| Upload Part | write to offset |

### SMB3 Protocol

SMB3 uses a custom VFS plugin that maps SMB operations to the internal API:

- SMB2_CREATE for file open/create
- SMB2_READ/SMB2_WRITE for I/O
- SMB2_SET_INFO for attributes
- SMB3 multi-channel support via `smb_multichannel.rs`

## Error Handling and Retry Policies

### Retry Strategy by Protocol

| Protocol | Retry Behavior |
|----------|----------------|
| NFSv3 | Client retries on NFS3ERR_JUKEBOX; gateway does not retry |
| NFSv4 | Stateful server handles retry; delegation recall on conflict |
| pNFS | Layout revocation on LAYOUTRETURN; client retries |
| S3 | SDK handles retries with exponential backoff |
| SMB3 | Client redirects on tree connect failure |

### Circuit Breaker Integration

The circuit breaker (`gateway_circuit_breaker.rs`) provides fault tolerance:

1. **Closed state**: Normal operation; failures are counted
2. **Open state**: Requests are rejected immediately; no load on failing backend
3. **Half-open state**: Limited requests allowed to test recovery

Circuit breakers are per-backend-node:
```rust
let mut registry = CircuitBreakerRegistry::new();
let cb = registry.get_or_create("backend-1", CircuitBreakerConfig::default());

cb.call(|| {
    // Operation that might fail
    backend.request()
}).map_err(|e| match e {
    CircuitBreakerError::CircuitOpen { .. } => "backend unavailable",
    _ => "operation failed",
})?;
```

### Quota Enforcement

Quota violations are enforced before operations proceed (`quota.rs`):

```rust
let violation = quota_manager.record_write(subject, bytes);
match violation {
    QuotaViolation::HardLimitExceeded => return Error::QuotaExceeded,
    QuotaViolation::SoftLimitExceeded => log::warn!("soft quota exceeded"),
    QuotaViolation::None => proceed(),
}
```

## Module Inventory

| Module | Purpose |
|--------|---------|
| `server.rs` | RPC dispatcher for NFSv3/MOUNT protocols |
| `nfs.rs` | NFSv3 protocol handlers |
| `nfs_cache.rs` | Server-side attribute cache |
| `mount.rs` | MOUNT protocol for export discovery |
| `pnfs.rs` | pNFS layout server |
| `pnfs_flex.rs` | Flexible layout support |
| `s3.rs` | S3 API endpoint |
| `s3_router.rs` | S3 request routing |
| `s3_multipart.rs` | Multipart upload state machine |
| `smb.rs` | SMB protocol handlers |
| `smb_multichannel.rs` | SMB multi-channel support |
| `gateway_conn_pool.rs` | Backend connection pooling |
| `gateway_circuit_breaker.rs` | Fault tolerance |
| `gateway_metrics.rs` | Prometheus metrics |
| `health.rs` | Health check framework |
| `quota.rs` | Quota enforcement |
| `auth.rs` | Authentication/authorization |
| `config.rs` | Configuration structures |

## Design Rationale

### Why Connection Pooling?

Without connection pooling, each client request would establish a new connection to backend nodes, causing:
- High latency from TCP handshake overhead
- CPU overhead from TLS handshakes
- Connection table exhaustion on busy gateways

The gateway maintains a pool of pre-established connections, reusing them across requests.

### Why Circuit Breakers?

In distributed systems, failures are inevitable. When a backend node becomes unavailable:
- Without circuit breakers: Gateway continues sending requests, causing timeout cascades
- With circuit breakers: Failed requests are rejected immediately, allowing backend to recover

### Why Multi-Protocol?

Different workloads have different requirements:
- NFS: Legacy application compatibility, standard filesystem semantics
- pNFS: High-performance HPC workloads requiring direct storage access
- S3: Cloud-native applications, data lake access
- SMB: Windows application compatibility

## See Also

- [Integration Guide](INTEGRATION_GUIDE.md) - Configuration and deployment
- [Performance Tuning](PERFORMANCE_TUNING.md) - Production optimization
- [Operations Runbook](OPERATIONS_RUNBOOK.md) - Day-to-day operations
- [Protocol Notes](PROTOCOL_NOTES.md) - Protocol-specific details