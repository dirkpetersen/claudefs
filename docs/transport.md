# Transport Layer Design

Building a high-performance transport layer in Rust for Linux 6.20+ exploits `io_uring`, zero-copy primitives, and RDMA. Since ClaudeFS servers run on modern kernels but must support older clients, the transport is a multi-modal strategy.

## Transport Options

### 1. pNFS (Parallel NFS, NFSv4.1+)

pNFS separates the Metadata Server (MDS) from the Data Servers (DS). The client gets a "layout" from the MDS and then talks directly to storage nodes via the Data Server protocol.

**Pros:**
- Native client support on almost every Linux distro since 2012 — no custom kernel module required
- The layout mechanism gives clients direct-to-storage-node data access, bypassing the metadata server on the data path
- Implementing a pNFS-compatible server in Rust is feasible using `nfs-v4` crates or a user-space RPC framework

**Cons:**
- Complexity — the pNFS state machine (leases, recalls, layouts) is significantly harder to implement than a custom protocol
- Legacy NFSv3 clients won't understand layouts and will treat the MDS as a bottleneck proxy, routing all data through it unless a fallback proxy mode is implemented

### 2. BeeGFS Protocol

BeeGFS uses a thin TCP/RDMA-based protocol that is striping-aware from the start, popular in HPC.

**Pros:**
- Simplicity — the protocol is essentially "Command + Offset + Length + Data," much easier to implement in Rust than the full NFSv4.2 spec
- Native parallelism — the BeeGFS client kernel module handles parallel streams and multi-pathing across storage nodes well
- A "BeeGFS-compatible" storage server in Rust could understand existing client requests but use an io_uring backend

**Cons:**
- Client maintenance — tethered to the BeeGFS kernel module. If the module breaks on a future kernel or distro, the server is useless to that client
- License concerns — while the client is GPL, a clean-room server implementation must avoid protocol patent issues for commercial use

### 3. Rust-Native Transport (The 2026 Approach)

Maximize the 6.20+ kernel by building a transport that leverages `io_uring` for both disk and network.

**Architecture: io_uring + Zero-Copy**

In Rust, use `tokio-uring` or `glommio` crates for a thread-per-core architecture:

- **Zero-copy data path:** Use `splice()` or `sendfile()` via io_uring to move data directly from NVMe to network socket without touching user-space memory
- **RDMA/RoCE:** Use `ibverbs` Rust bindings. RDMA allows clients to pull data from server memory without the server's CPU involvement — one-sided READ/WRITE verbs
- **io_uring for everything:** Disk I/O, network sends, and network receives all go through the same io_uring submission ring, enabling batching of heterogeneous operations

## Comparison

| Transport | Complexity | Client Ease | Max Performance |
|-----------|-----------|-------------|-----------------|
| **pNFS (v4.1+)** | High | Best (built-in) | Very High |
| **BeeGFS Protocol** | Low | Moderate (module required) | Extreme (HPC optimized) |
| **Custom Rust/RDMA** | Moderate | Hard (requires SDK) | Absolute Maximum |

## ClaudeFS Transport Strategy

ClaudeFS uses a layered approach — a high-performance internal protocol with compatibility frontends:

### Core: Custom RPC over io_uring

The internal cluster protocol between ClaudeFS nodes (server-to-server and performance-client-to-server) uses a custom binary RPC built on io_uring shared buffers. This is where maximum performance lives:

- Thread-per-core architecture via `tokio-uring` or `glommio`
- Zero-copy disk-to-network via `splice`/`sendfile` through io_uring
- RDMA one-sided verbs via `libfabric` when RDMA hardware is available
- Automatic fallback to TCP with io_uring zero-copy send when RDMA is not available

### Legacy Frontend: NFS Gateway

For clients that cannot run the ClaudeFS FUSE daemon (older distros, appliances, non-Linux):

- A Rust NFS server (NFS-Ganesha-style) translates NFSv3/v4 requests into the internal protocol
- Runs as a gateway process on dedicated nodes or co-located on ClaudeFS servers
- NFSv3 clients get full access but with single-server bandwidth (no parallelism)

### Modern Frontend: pNFS Layouts

For modern Linux clients (NFSv4.1+) that don't want to install the FUSE client:

- ClaudeFS metadata servers issue pNFS layouts pointing clients directly at data servers
- Clients get parallel direct-to-node data access using the standard kernel NFS client
- No custom kernel module or FUSE daemon required — just a standard NFS mount
- This is the best option for "works everywhere, good performance, zero install"

### FUSE Client (Primary)

For clients that install ClaudeFS:

- Speaks the core RPC protocol directly — no NFS translation overhead
- FUSE passthrough for native-speed local data I/O
- RDMA or TCP depending on available hardware
- Full feature set including relaxed POSIX flags, client-side caching, direct S3 tiering hints
