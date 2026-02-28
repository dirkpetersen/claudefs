# Data Reduction: Deduplication, Compression, Encryption, Snapshots

In a high-performance scale-out system, these four features are a tightly coupled pipeline. Co-developing them from the start avoids the "Storage Efficiency Paradox" — where independently designed features actively fight each other under load.

## The Pipeline: Order of Operations

The ordering is non-negotiable. Encryption makes data look like random noise, destroying patterns that compression and deduplication depend on.

```
Write Path:  Data -> Deduplicate -> Compress -> Encrypt -> Store
Read Path:   Store -> Decrypt -> Decompress -> (dedupe is transparent) -> Data
```

1. **Deduplicate** — find identical blocks via content fingerprinting
2. **Compress** — shrink unique blocks (skip already-deduplicated data to save CPU)
3. **Encrypt** — secure the shrunken, unique blocks with authenticated encryption

Reversing any step in this order (e.g., encrypting before compressing) reduces storage efficiency to near zero.

## Feature Dependencies

| Feature | Depends On | Why |
|---------|-----------|-----|
| **Deduplication** | Metadata engine | Needs a global fingerprint table (SHA-256/BLAKE3) to find matches |
| **Compression** | Deduplication | Only compress unique data to save CPU cycles |
| **Encryption** | Compression | Encrypted data cannot be compressed effectively |
| **Snapshots** | Deduplication | Snapshots are a saved set of block pointers into the CAS store |

## Deduplication

### Content-Addressable Storage (CAS)

ClaudeFS stores data blocks addressed by their content hash. Two identical blocks (from any file, any node, any site) share the same physical storage. This model makes snapshots essentially free — a snapshot is a new pointer tree referencing existing content-addressed blocks.

### Chunking Strategy

- **Rolling hash (Rabin/FastCDC)** for variable-length chunking — adapts to data boundaries, producing stable fingerprints even when data shifts (insertions/deletions)
- Target chunk size: 64KB–256KB range. Too small = metadata explosion. Too large = poor dedupe ratio.
- The 64MB S3 blob size is a separate concern — blobs are containers packed with many deduplicated, compressed, encrypted chunks

### Fingerprinting

- **BLAKE3** preferred over SHA-256 — faster (SIMD-accelerated), cryptographically secure, and available as a well-maintained Rust crate (`blake3`)
- Fingerprints are stored in the distributed metadata alongside inode/block mappings
- Consistent hashing distributes the fingerprint table across metadata servers (InfiniFS pattern)

### The Garbage Collection Problem

With CAS, deleting a file or snapshot doesn't necessarily free blocks — other files/snapshots may reference the same content. Two approaches:

- **Reference counting** — each block tracks how many pointers reference it. Deletion is O(1) but reference counts must be kept consistent across distributed metadata under failures.
- **Mark-and-sweep** — periodically walk all pointer trees and mark reachable blocks. Slower but simpler to keep consistent.

**ClaudeFS approach:** Reference counting in the metadata engine with periodic mark-and-sweep as a consistency check. Reference count updates are part of the same atomic metadata transaction as file operations (leveraging NVMe atomic writes from kernel 6.11+).

## Compression

### Throughput Amplification

On NVMe + RDMA hardware, compression isn't just about saving space — it increases effective throughput. At 2:1 compression, sending 1GB of data uses only 500MB of network and PCIe bandwidth. This matters for:

- **RDMA transfers** — compressed-before-send doubles effective fabric bandwidth
- **S3 tiering** — 64MB logical blobs compress to ~32MB actual object store writes, halving cloud storage costs and upload time
- **Flash layer capacity** — the same NVMe pool stores 2x more data

### Algorithm Choice

| Algorithm | Ratio | Speed | Use Case |
|-----------|-------|-------|----------|
| **LZ4** | ~2:1 | ~4 GB/s | Hot data path — inline compression on NVMe writes |
| **Zstd** | ~3:1 | ~1 GB/s | Warm/cold data — S3 tiering, background recompression |
| **Zstd (dictionary)** | ~4:1+ | ~800 MB/s | Small files with shared structure (logs, configs) |

- Rust crates: `lz4_flex` (pure Rust, fast), `zstd` (bindings to libzstd)
- Compression can be offloaded to io_uring worker threads or Intel QAT hardware without blocking the main I/O path

### Variable-Length Packing

After deduplication and compression, chunks are variable-length. Don't force them into fixed-size blocks. Instead, pack compressed chunks into larger **segments** (e.g., 2MB) aligned to ZNS zone append boundaries. This maximizes NVMe sequential write throughput and avoids internal fragmentation.

## Encryption

### Authenticated Encryption (AEAD)

Use AEAD (Authenticated Encryption with Associated Data) to provide encryption and integrity verification in one pass:

- **AES-256-GCM** — hardware-accelerated on all modern x86 (AES-NI). Preferred when CPU supports it.
- **ChaCha20-Poly1305** — constant-time, fast on CPUs without AES-NI. Fallback for ARM or older hardware.

AEAD provides bit-rot detection as a side effect — if a block is silently corrupted on disk or during transfer, decryption fails with an authentication error. This supplements ClaudeFS's end-to-end checksum strategy.

### Key Management

- **Per-file keys** derived from a master key via HKDF — allows per-file access revocation without re-encrypting the entire filesystem
- Unique IV/nonce per chunk — prevents identical plaintext chunks from producing identical ciphertext (which would leak information even with deduplication)
- Master keys stored externally (KMS, HSM, or user-provided passphrase via PBKDF2/Argon2)

### Interaction with Deduplication

Encryption after deduplication means the dedupe engine sees plaintext fingerprints. This is a deliberate tradeoff:

- **Pro:** Full deduplication effectiveness — identical data is detected regardless of encryption
- **Con:** The fingerprint table (BLAKE3 hashes) is sensitive metadata that must be protected. Metadata at rest is encrypted separately with the cluster key.
- If per-user encryption with zero-knowledge is required (user A's data invisible to user B even at the storage layer), deduplication is limited to within each user's key domain

## Snapshots

### Copy-on-Write (CoW)

With CAS, snapshots are a pointer tree saved at a point in time. Creating a snapshot is O(1) — just freeze the current root pointer. No data is copied.

- **Immutable snapshots** — the log-structured design (see [docs/kernel.md](docs/kernel.md), ZNS section) means every write creates a new entry. Snapshots are a pointer to the log head at a given time.
- **Writable clones** — fork the pointer tree. New writes go to new blocks; shared blocks remain shared via reference counting.
- **Snapshot deletion** — decrement reference counts on all blocks pointed to by the snapshot. Blocks with zero references are reclaimed by GC.

### Cross-Site Snapshots

Snapshots interact with cross-site replication:

- A consistent snapshot can be replicated as a set of content-addressed blocks — only blocks not already present at the remote site need to be transferred (incremental replication via CAS)
- This makes cross-site replication bandwidth-efficient: deduplicated, compressed, encrypted blocks are transferred once

## Inline vs Post-Processing

| Strategy | Pros | Cons |
|----------|------|------|
| **Inline** | No junk data on flash; immediate space savings; consistent latency | Higher write latency; more CPU on the hot path |
| **Post-processing** | Maximum burst write speed; simpler write path | Flash fills with unreduced data; background GC causes latency spikes |

**ClaudeFS approach:** Inline for the common path with a post-processing fallback.

- **Inline deduplication** — fingerprint lookup on every write. If the block already exists, skip the write entirely. This is cheap (one hash + one metadata lookup) and prevents the most waste.
- **Inline compression** — LZ4 on the hot path. Fast enough to not bottleneck NVMe writes. Zstd applied during background S3 tiering.
- **Inline encryption** — AES-GCM with hardware acceleration. Negligible overhead on modern CPUs.
- **Post-processing recompression** — background thread upgrades LZ4-compressed blocks to Zstd before S3 tiering, maximizing cloud storage efficiency without slowing the write path.

## Related Literature

- **Z-LFS** (USENIX ATC 2025) — ZNS-tailored log-structured filesystem. Solves small-zone ZNS constraints with speculative log stream management. Up to 33x performance gains over standard LFS on ZNS. Direct blueprint for ClaudeFS's flash layer data placement.
- **Speed-Dedup** (MDPI 2024) — Scale-out deduplication framework that avoids WAL performance hits. Focuses on reducing I/O amplification — critical when combining dedupe with flash.
- **Distributed Data Deduplication for Big Data: A Survey** (ACM Computing Surveys 2025) — Comprehensive survey on routing data between clients and nodes to maximize dedupe ratios.
- **ZNSage** (2025) — Hardware-agnostic swap and metadata placement on ZNS, relevant to ClaudeFS's metadata store design.
- **Linux Storage Stack Diagram 6.20** (Thomas-Krenn / Werner Fischer) — Canonical visualization of how io_uring, NVMe-oF, and VFS interact in modern kernels.
- **NFS LOCALIO** (kernel 6.12+) — Bypasses the network stack when NFS client and server are on the same host. Relevant for containerized ClaudeFS deployments where the NFS gateway is co-located.

### Rust Ecosystem

- **Thread-per-core (TPC) architecture** — `glommio` and `monoio` crates use io_uring to avoid Tokio's thread pool overhead, essential for microsecond-level latency. See technical blogs on TPC patterns in Rust.
- **Beyond-Refs / Field Projections** (Rust Blog 2026) — Latest research on `Pin` and memory-mapped buffer management in Rust, relevant to zero-copy I/O paths.
