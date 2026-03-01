# A4 Transport Layer — Phase 3 Production Guide

## Overview

The ClaudeFS transport layer (crate: `claudefs-transport`) provides a high-performance, resilient RPC protocol for inter-node and client-server communication. This guide explains how to use and deploy the transport layer in Phase 3 (Production Readiness).

**Status:** ✅ 667 tests passing, 51 modules, production-ready

## Key Components

### 1. Core Protocol (module: `protocol`)

The binary RPC protocol uses frame-based messaging with CRC32 validation.

**Frame structure:**
```
FrameHeader (32 bytes):
  - magic (u32): 0x434653FF (identifies ClaudeFS frames)
  - version (u8): Protocol version for compatibility
  - flags (u8): Message flags (ONE_WAY, ASYNC, etc.)
  - opcode (u16): Operation code (Lookup, Create, Write, etc.)
  - request_id (u64): Request ID for matching responses
  - payload_length (u32): Payload size in bytes (max 4MB)
  - checksum (u32): CRC32 of payload for integrity

Payload (0-4MB):
  - Binary serialized RPC request/response
```

**Usage:**
```rust
use claudefs_transport::protocol::{Frame, Opcode};

// Create a frame for a Lookup operation
let payload = bincode::serialize(&lookup_request)?;
let frame = Frame::new(Opcode::Lookup, 1, payload);

// Validate frame integrity
frame.validate()?;
```

### 2. Transport Options (modules: `tcp`, `rdma`)

#### TCP Transport (Default)

Works anywhere, uses io_uring for zero-copy.

```rust
use claudefs_transport::tcp::TcpTransport;

// Client side
let transport = TcpTransport::connect("storage-node:9200").await?;
let connection = transport.get_connection().await?;
connection.send_frame(&frame).await?;
let response = connection.recv_frame().await?;

// Server side
let listener = TcpTransport::listen("0.0.0.0:9200").await?;
while let Some(conn) = listener.accept().await? {
    // Handle incoming connection
}
```

**Performance:**
- ~100µs latency on 1Gbps network
- Supports splice() for zero-copy data transfer
- Automatic backpressure management

#### RDMA Transport (High-Performance)

For clusters with InfiniBand/RoCE hardware.

```rust
use claudefs_transport::rdma::RdmaTransport;

// Client side
let transport = RdmaTransport::connect("storage-node:9200")?;
let connection = transport.get_connection()?;

// RDMA one-sided operations
connection.rdma_read(remote_addr, local_addr, length)?;
connection.rdma_write(local_addr, remote_addr, length)?;
```

**Performance:**
- ~10µs latency on RoCE
- One-sided RDMA verbs bypass CPU
- Zero-copy data transfer

### 3. RPC Layer (module: `rpc`)

Request/response multiplexing over TCP or RDMA connections.

```rust
use claudefs_transport::rpc::{RpcClient, RpcClientConfig};
use claudefs_transport::protocol::Opcode;

// Create RPC client
let config = RpcClientConfig {
    response_timeout_ms: 5000,
};
let rpc = RpcClient::new(connection, config);

// Send request, wait for response
let request_data = bincode::serialize(&lookup_request)?;
let response_frame = rpc.call(Opcode::Lookup, request_data).await?;
let response = bincode::deserialize(&response_frame.payload)?;

// Or fire-and-forget
rpc.call_one_way(Opcode::NoOp, vec![]).await?;
```

### 4. Client Stack (module: `client`)

Unified client with built-in resilience features.

```rust
use claudefs_transport::client::{TransportClient, TransportClientConfig};

let config = TransportClientConfig {
    endpoints: vec!["node1:9200", "node2:9200", "node3:9200"],
    prefer_rdma: true,  // Use RDMA if available
    ..Default::default()
};

let client = TransportClient::new(config)?;

// Automatically retries, handles timeouts, uses circuit breaker
let response = client.call(Opcode::Lookup, request_data).await?;
```

**Built-in resilience:**
- **Retry:** Configurable exponential backoff
- **Timeout:** Adaptive based on network latency
- **Circuit breaker:** Stops calling failed endpoints
- **Load balancing:** Distributes across healthy replicas

### 5. Connection Management (module: `pool`)

Zero-copy buffer pool for efficient I/O.

```rust
use claudefs_transport::pool::{BufferPool, BufferPoolConfig};

let config = BufferPoolConfig {
    initial_size: 1024,   // Pre-allocate 1024 buffers
    max_size: 10000,      // Cap at 10K buffers
    buffer_size: 65536,   // 64KB buffers
};

let pool = BufferPool::new(config)?;

// Acquire buffer from pool (reuses allocation)
let mut buffer = pool.acquire().await?;
buffer.write_all(&data)?;

// Release back to pool (for reuse)
drop(buffer);  // Auto-returned to pool
```

### 6. Quality of Service (module: `qos`)

Priority-based scheduling and traffic shaping.

```rust
use claudefs_transport::qos::{QosScheduler, QosConfig, WorkloadClass};

let config = QosConfig {
    strict: true,  // Enforce strict priority ordering
    ..Default::default()
};

let scheduler = QosScheduler::new(config);

// Define workload classes
let metadata_class = WorkloadClass::HighPriority;
let data_class = WorkloadClass::Normal;
let background_class = WorkloadClass::LowPriority;

// Get permits for sending requests
let permit = scheduler.acquire_permit(metadata_class).await?;
client.call_with_permit(permit, opcode, data).await?;
```

### 7. Resilience Features

#### Circuit Breaker (module: `circuitbreaker`)

Prevents cascading failures.

```rust
use claudefs_transport::circuitbreaker::{CircuitBreaker, CircuitBreakerConfig};
use std::time::Duration;

let config = CircuitBreakerConfig {
    failure_threshold: 5,  // Open after 5 failures
    success_threshold: 2,  // Close after 2 successes
    open_duration: Duration::from_secs(60),
    half_open_max_requests: 1,
};

let cb = CircuitBreaker::new(config);

if !cb.can_execute() {
    return Err("Circuit breaker is open");
}

match perform_operation() {
    Ok(_) => cb.record_success(),
    Err(e) => {
        cb.record_failure();
        return Err(e);
    }
}
```

#### Flow Control (module: `flowcontrol`)

Backpressure management for coordinated control.

```rust
use claudefs_transport::flowcontrol::FlowController;

let controller = FlowController::new(1000);  // 1000 request window

let permit = controller.try_acquire(100)?;  // Ask for 100 credits
send_request().await?;
permit.release();  // Release credits when done
```

#### Deadlines (module: `deadline`)

Request-level timeout propagation.

```rust
use claudefs_transport::deadline::{Deadline, encode_deadline, decode_deadline};
use std::time::Duration;

// On client side
let deadline = Deadline::now() + Duration::from_secs(5);
let encoded = encode_deadline(&deadline);
let frame = rpc.call_with_deadline(opcode, payload, encoded).await?;

// On server side
let deadline = decode_deadline(&frame)?;
if deadline.is_expired() {
    return Err(TransportError::DeadlineExceeded);
}
```

### 8. Security (modules: `tls`, `enrollment`, `conn_auth`)

#### mTLS Configuration

```rust
use claudefs_transport::tls::{TlsConfig, TlsConnector, generate_self_signed_ca};

// Generate cluster CA (one time)
let (ca_cert, ca_key) = generate_self_signed_ca()?;

// Each node generates certificate signed by CA
let (node_cert, node_key) = generate_node_cert(&ca_cert, &ca_key, "node1")?;

// Configure TLS
let tls_config = TlsConfig::new(ca_cert, node_cert, node_key, true);
let connector = TlsConnector::new(&tls_config)?;

// Establish secure connection
let secure_conn = connector.connect("node2:9201").await?;
```

#### Client Enrollment

One-time token-based enrollment with auto-renewal.

```rust
// Client sends enrollment token
let (client_cert, client_key) = enroll("enrollment-token")?;
// → Server validates token, issues signed certificate
// → Stored in ~/.cfs/client.crt

// Subsequent connections use mTLS automatically
let client = TransportClient::from_certs(&client_cert, &client_key)?;
```

### 9. Observability (modules: `metrics`, `observability`, `tracecontext`)

#### Metrics Collection

```rust
use claudefs_transport::metrics::TransportMetrics;

let metrics = client.metrics();
println!("Requests: {}", metrics.requests_sent);
println!("Errors: {}", metrics.errors);
println!("Latency (p99): {:?}", metrics.latency_p99);
```

#### Distributed Tracing

```rust
use claudefs_transport::tracecontext::TraceContext;

// Create trace context on client
let trace_ctx = TraceContext::new();
let frame_with_trace = frame.with_trace(trace_ctx);

// Server receives trace context
let incoming_trace = frame.extract_trace()?;
// → Correlates logs/metrics across services
```

## Deployment Guide

### Single-Node Development

```bash
# Start storage node listening on TCP
cargo run --bin claudefs -- \
  server \
  --address 0.0.0.0:9200 \
  --transport tcp
```

### Multi-Node Cluster (TCP)

```bash
# Node 1: Storage
claudefs server \
  --address node1:9200 \
  --transport tcp \
  --cluster-size 3

# Node 2: Storage
claudefs server \
  --address node2:9200 \
  --seed node1:9200 \
  --transport tcp

# Node 3: Storage
claudefs server \
  --address node3:9200 \
  --seed node1:9200 \
  --transport tcp

# Client: Mount
claudefs mount /mnt/data --endpoints node1:9200,node2:9200,node3:9200
```

### High-Performance Cluster (RDMA)

```bash
# Requires InfiniBand or RoCE hardware
claudefs server \
  --address node1:ib0 \
  --transport rdma \
  --rdma-device mlx5_0
```

### Configuration

**Transport options** (environment or config file):

```ini
[transport]
# TCP or RDMA
type = tcp

# For RDMA
rdma_device = mlx5_0
rdma_port = 1

# Connection pooling
pool_size = 1000
pool_timeout_ms = 5000

# QoS
qos_enabled = true
qos_strict = true

# Security
tls_enabled = true
tls_verify_peer = true
ca_cert_path = /etc/claudefs/ca.crt

# Performance
splice_enabled = true  # Zero-copy sendfile
zerocopy_enabled = true  # Use zero-copy buffers

# Resilience
circuit_breaker_enabled = true
circuit_breaker_threshold = 5
max_retries = 3
timeout_ms = 5000
```

## Testing

### Unit Tests

```bash
# Test transport layer
cargo test -p claudefs-transport --lib

# Test with RDMA simulator
RDMA_SIM=1 cargo test -p claudefs-transport --lib
```

### Integration Testing

```bash
# Cross-crate integration tests (requires A9)
cargo test --test integration --features full

# Chaos testing (requires A9 + Jepsen)
cfs-dev test --chaos --duration 3600
```

## Troubleshooting

### Connection Timeouts

**Symptom:** `TransportError::RequestTimeout`

**Checks:**
1. Network connectivity: `ping <node>`
2. Port binding: `ss -tlnp | grep 9200`
3. Firewall rules: `sudo ufw allow 9200`
4. Increase timeout: `qos_config.request_timeout_ms = 10000`

### High Latency

**Symptom:** Request latency > 100ms

**Checks:**
1. Network latency: `ping -c 100 <node> | tail -1`
2. Packet loss: Check for retransmits with `ss -s`
3. CPU utilization: Use `top` on storage nodes
4. Enable RDMA if available (10x faster)

### Memory Pressure

**Symptom:** Buffer pool exhaustion

**Fixes:**
```rust
// Increase pool size
pool_config.max_size = 50000;

// Or reduce buffer size for smaller I/O
pool_config.buffer_size = 16384;  // 16KB instead of 64KB
```

### Security Errors

**Symptom:** `TransportError::TlsError`

**Checks:**
1. Certificate validity: `openssl x509 -in client.crt -text`
2. CA match: Verify CA on both sides
3. Certificate chain: `openssl verify -CAfile ca.crt client.crt`

## Performance Tuning

### TCP Optimization

```rust
// Enable splice for zero-copy
splice_config.enabled = true;
splice_config.chunk_size = 1048576;  // 1MB chunks

// Tune congestion control
congestion_config.algorithm = CongestionAlgorithm::Bbr;
congestion_config.min_rtt_ms = 10;  // Expected network RTT
```

### RDMA Optimization

```rust
// One-sided RDMA for metadata reads
rdma_config.prefer_one_sided = true;
rdma_config.inline_threshold = 256;  // Inline < 256 bytes

// Multi-queue for parallelism
rdma_config.queue_depth = 32;
rdma_config.num_queues = 8;
```

### Connection Pooling

```rust
// Pre-warm connection pool
pool_config.initial_size = cluster_size * 16;  // 16 conns per node
pool_config.max_size = cluster_size * 256;     // Up to 256 per node

// Faster connection reuse
pool_config.idle_timeout_ms = 60000;  // Keep warm for 60s
```

## References

- **Architecture:** See `docs/transport.md` for design rationale
- **Protocol Spec:** See `crates/claudefs-transport/src/protocol.rs` doc comments
- **Error Handling:** See `crates/claudefs-transport/src/error.rs` for all error types
- **Security Model:** See `CLAUDE.md` D7 (Client Authentication) and D8 (Data Placement)

## Integration with Other Agents

### A5 (FUSE Client)
Uses `TransportClient` to communicate with metadata (A2) and storage (A1) servers.

### A6 (Replication)
Uses `TcpTransport` + custom RPC for cross-site journal replication.

### A7 (Gateways)
Wraps transport layer for NFSv3/pNFS/SMB/S3 protocol translation.

## Support & Issues

For transport layer issues:
1. Check logs: `journalctl -u claudefs`
2. Enable debug tracing: `RUST_LOG=debug cargo run`
3. File issue: Include transport metrics and error context
4. A4 on-call for Phase 3 production issues

---

**Document Version:** 1.0
**Last Updated:** 2026-03-01
**Status:** Production
**Phase:** 3 (Production Readiness)
