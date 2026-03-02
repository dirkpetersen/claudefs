# ClaudeFS Protocol Gateway (A7)

ClaudeFS subsystem providing multi-protocol access: NFSv3/v4, pNFS layouts, S3 API endpoint, and SMB3 support.

## Quick Start

```bash
# Build the gateway
cargo build --package claudefs-gateway

# Start NFS server
./target/debug/cfs-gateway start nfs --export /export

# Start S3 API (in another terminal)
./target/debug/cfs-gateway start s3 --bind 0.0.0.0:9000

# Check health
curl http://localhost:8080/health
```

## Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                        Client Access                                │
├─────────────┬─────────────┬─────────────┬──────────────────────────┤
│  NFSv3/v4   │    pNFS     │     S3      │         SMB3             │
│  port 2049  │  (layouts)  │  port 9000  │       port 445           │
└──────┬──────┴──────┬──────┴──────┬──────┴────────────┬─────────────┘
       │             │             │                    │
       ▼             ▼             ▼                    ▼
┌─────────────────────────────────────────────────────────────────────┐
│                    Gateway Protocol Layer                           │
├──────────────────┬─────────────────┬───────────────────────────────┤
│  gateway_conn_pool │ gateway_circuit_breaker │ quota.rs          │
│    (connections)   │    (resilience)      │ (limits)            │
└──────────────────┴─────────────────┴───────────────────────────────┘
       │                            │
       ▼                            ▼
┌─────────────────┐      ┌─────────────────────┐
│  A2 Metadata    │      │  A4 Transport       │
│  Service        │      │  (RDMA/TCP)         │
└─────────────────┘      └─────────────────────┘
```

## Protocols Supported

| Protocol | Port | Description |
|----------|------|-------------|
| NFSv3 | 2049 | Standard POSIX file access |
| MOUNT | 20048 | NFS export discovery |
| NFSv4.1/4.2 | 2049 | Stateful NFS with sessions |
| pNFS | 2049 | Parallel direct-to-storage |
| S3 | 9000 | AWS S3-compatible API |
| SMB3 | 445 | Windows file sharing |

## Module Inventory

| Module | Lines | Purpose |
|--------|-------|---------|
| `lib.rs` | 53 | Public API exports |
| `server.rs` | 781 | RPC dispatcher, NFS/MOUNT handling |
| `nfs.rs` | ~2000 | NFSv3 protocol implementation |
| `nfs_cache.rs` | 338 | Server-side attribute cache |
| `mount.rs` | ~500 | MOUNT protocol handler |
| `pnfs.rs` | ~500 | pNFS layout server |
| `pnfs_flex.rs` | ~300 | Flexible files layout |
| `s3.rs` | ~1500 | S3 API implementation |
| `s3_router.rs` | ~400 | S3 request routing |
| `s3_multipart.rs` | 595 | Multipart upload state machine |
| `smb.rs` | ~800 | SMB protocol implementation |
| `smb_multichannel.rs` | 812 | Multi-channel SMB |
| `gateway_conn_pool.rs` | 696 | Backend connection pooling |
| `gateway_circuit_breaker.rs` | 679 | Fault tolerance |
| `gateway_metrics.rs` | 878 | Prometheus metrics |
| `health.rs` | 484 | Health check framework |
| `quota.rs` | 473 | Quota enforcement |
| `auth.rs` | ~300 | Authentication/authorization |
| `config.rs` | 441 | Configuration structures |

## Test Coverage

| Category | Tests |
|----------|-------|
| Connection Pool | 28 tests |
| Circuit Breaker | 31 tests |
| Health Check | 30 tests |
| Quota | 29 tests |
| S3 Multipart | 27 tests |
| SMB Multi-channel | 29 tests |
| Attr Cache | 26 tests |
| Metrics | 30 tests |
| **Total** | **1032 tests** |

Run tests:
```bash
cargo test --package claudefs-gateway
```

## Configuration

### Basic Configuration File

```yaml
# gateway.yaml
nfs:
  bind:
    addr: "0.0.0.0"
    port: 2049
  mount_bind:
    addr: "0.0.0.0"
    port: 20048
  exports:
    - path: "/export"
      read_only: false
      root_squash: true

s3:
  bind:
    addr: "0.0.0.0"
    port: 9000
  region: "us-west-2"
  enable_versioning: false

metadata_servers:
  - address: "meta-1.claudefs.local:7001"
  - address: "meta-2.claudefs.local:7001"

connection_pool:
  min_per_node: 4
  max_per_node: 32

circuit_breaker:
  failure_threshold: 5
  open_duration_ms: 30000
```

### CLI Usage

```bash
# Start all protocols
cfs-gateway start --config gateway.yaml

# Start specific protocol
cfs-gateway start nfs --export /data
cfs-gateway start s3 --bind :9000

# Validate config
cfs-gateway validate-config --config gateway.yaml

# Show metrics
cfs-gateway metrics
```

## Documentation

- [Architecture](docs/ARCHITECTURE.md) - System design and integration
- [Integration Guide](docs/INTEGRATION_GUIDE.md) - Configuration and deployment
- [Performance Tuning](docs/PERFORMANCE_TUNING.md) - Production optimization
- [Operations Runbook](docs/OPERATIONS_RUNBOOK.md) - Day-to-day operations
- [Protocol Notes](docs/PROTOCOL_NOTES.md) - Protocol-specific details

## Performance

Expected throughput on modern hardware:

| Protocol | 10 Gbps | 25 Gbps | 100 Gbps RDMA |
|----------|---------|---------|---------------|
| NFSv3 | 800 MB/s | 2 GB/s | - |
| pNFS | 900 MB/s | 2.2 GB/s | 8 GB/s |
| S3 | 700 MB/s | 1.8 GB/s | - |
| SMB3 | 750 MB/s | 1.9 GB/s | 6 GB/s |

## Known Limitations

### Phase 3 (Current)

- NFSv4 delegations not fully implemented
- SMB signing performance needs optimization
- pNFS flex files requires more testing
- Quota grace period not enforced at protocol level

### Roadmap

- Full NFSv4.2 support with delegations
- Active-active gateway HA
- Rate limiting per-client
- WebDAV protocol support

## Dependencies

- **A2 (claudefs-meta)**: Metadata service for inode operations
- **A4 (claudefs-transport)**: Backend transport (RDMA/TCP)

## License

MIT - See workspace LICENSE file.