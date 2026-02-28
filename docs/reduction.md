# Data Reduction: Deduplication, Compression, Encryption, Snapshots

In a high-performance scale-out system, these four features are a tightly coupled pipeline. Co-developing them from the start avoids the "Storage Efficiency Paradox" — where independently designed features actively fight each other under load.

## The Pipeline: Order of Operations

The ordering is non-negotiable. Encryption makes data look like random noise, destroying patterns that compression and deduplication depend on.

```
Write Path:  Data -> Deduplicate -> Compress -> Encrypt -> Store
Read Path:   Store -> Decrypt -> Decompress -> (dedupe is transparent) -> Data
```

1. **Deduplicate** — find identical blocks (exact-match CAS) and similar blocks (similarity + delta compression)
2. **Compress** — shrink unique blocks (skip already-deduplicated data to save CPU)
3. **Encrypt** — secure the shrunken, unique blocks with authenticated encryption

Reversing any step in this order (e.g., encrypting before compressing) reduces storage efficiency to near zero.

## Feature Dependencies

| Feature | Depends On | Why |
|---------|-----------|-----|
| **Deduplication** | Metadata engine | Exact-match via CAS fingerprints + similarity via Super-Features |
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

## Similarity-Based Deduplication (The VAST Approach)

Traditional exact-match block deduplication requires a 1:1 hash table mapping every block's fingerprint to its location. At scale, this table consumes massive RAM — a 1PB filesystem with 64KB chunks needs ~4 billion entries, requiring hundreds of GB of RAM just for the index.

VAST Data pioneered an alternative: **similarity-based resemblance detection with delta compression**. Instead of finding identical blocks, it finds *similar* blocks and stores only the differences. This achieves 3-4:1 data reduction with a fraction of the RAM overhead.

### How It Works: The Pipeline

In computer science literature, VAST's approach combines three well-established open algorithms:

**Step 1: Content-Defined Chunking (FastCDC)**

Slice incoming data into variable-length chunks (8KB–32KB) based on content boundaries, not fixed offsets. A rolling hash (Rabin fingerprint) identifies natural breakpoints in the data. This produces stable chunk boundaries even when data shifts — an insertion at byte 0 doesn't change every subsequent chunk boundary.

- Paper: *"FastCDC: A Fast and Efficient Content-Defined Chunking Approach for Data Deduplication"* (USENIX ATC '16)
- Rust implementation: the `fastcdc` crate provides a production-quality implementation

**Step 2: Feature Extraction and Similarity Hashing**

Instead of computing a single hash per chunk (which only finds exact matches), extract multiple small "features" from each chunk using Locality-Sensitive Hashing (LSH) or MinHash:

- Divide each chunk into sub-regions
- Compute Rabin fingerprints for each sub-region
- Select the minimum (or maximum) fingerprints as the chunk's "Super-Features"
- Two chunks sharing 3 out of 4 Super-Features are flagged as "similar"

The key insight: the similarity index stores only these compact Super-Features, not full block hashes. This index is **orders of magnitude smaller** than a traditional 1:1 hash table.

- Paper: *"Finesse: Fine-Grained Feature Locality based Fast Resemblance Detection for Post-Deduplication Delta Compression"* (FAST '14) — the closest academic blueprint to VAST's architecture
- Mathematical foundations: MinHash and LSH date back decades and are unpatentable mathematical concepts

**Step 3: Delta Compression**

When a similar (but not identical) block is found, fetch the reference block and compute a byte-level delta — storing only the differences:

- A 64KB chunk that differs from its reference by 200 bytes stores as a ~200 byte delta + a pointer to the reference
- This is far more space-efficient than storing the full chunk, even after LZ4/Zstd compression

- Paper: *"Edelta: A Word-Enlarging Based Fast Delta Compression Approach"* (FAST '20) — modern delta compression designed for NVMe throughput
- Rust implementation: `xdelta3` bindings, or Zstd's dictionary compression mode (which approximates delta compression using a reference block as the dictionary)

### RAM Savings vs Traditional Dedupe

| Approach | Index Size (1PB, 64KB chunks) | Reduction Ratio | CPU Cost |
|----------|-------------------------------|-----------------|----------|
| **Exact-match block dedupe** | ~128 GB RAM (32-byte hash per chunk) | 2-3:1 (exact matches only) | Low (one hash per write) |
| **Similarity + delta compression** | ~8-16 GB RAM (compact Super-Features) | 3-4:1 (similar + identical) | Moderate (feature extraction + delta) |

The similarity approach uses 8-10x less RAM for the index while achieving higher reduction ratios — it catches data that is *almost* the same, not just exactly the same. This is common in real workloads: edited documents, recompiled binaries, VM images with small diffs, scientific datasets with shared headers.

### Can AI Code This in Rust?

Yes, and this is a strong case for Rust + AI development. The pipeline decomposes into clean, independent stages:

1. **FastCDC chunking** — well-defined algorithm, existing Rust crate, straightforward to integrate
2. **Feature extraction (MinHash/LSH)** — pure math, no complex state. AI excels at implementing hash functions. The Rust compiler ensures the concurrent feature extraction across io_uring threads is race-free.
3. **Similarity index** — a distributed hash map of Super-Features. This is the hardest part — it must be consistent across nodes, low-latency for lookups, and crash-safe. The Rust ownership model helps here: the index is either owned by one thread or shared via `Arc<RwLock>`, and the compiler enforces this.
4. **Delta compression** — Zstd dictionary mode gives ~90% of custom delta compression performance with zero custom code. Zstd is SIMD-accelerated and the Rust `zstd` crate is production-grade.

The borrow checker is particularly valuable here: the pipeline passes ownership of data buffers between stages (chunk -> extract features -> compress -> encrypt), and Rust guarantees no stage accidentally holds a reference to a buffer that another stage is modifying.

### Performance on Modern Hardware (2026)

On AMD EPYC 9654 with AVX-512:

| Stage | Throughput (per core) | Parallelism | Hardware Acceleration |
|-------|----------------------|-------------|----------------------|
| FastCDC chunking | ~2-4 GB/s | Embarrassingly parallel | SIMD for rolling hash |
| BLAKE3 fingerprint | ~8 GB/s | Embarrassingly parallel | AVX-512 native |
| MinHash features | ~3-5 GB/s | Embarrassingly parallel | SIMD for hash computation |
| Similarity lookup | ~10M lookups/s | Distributed across nodes | L3 cache (384MB on EPYC 9654) |
| Zstd delta compress | ~1-2 GB/s | Per-chunk parallel | AVX-512, dictionary mode |

On a 96-core EPYC 9654, the entire similarity pipeline can process **~50-100 GB/s aggregate** when all cores are utilized — well above the NVMe throughput ceiling of even a fully loaded node. The pipeline is CPU-bound only on the delta compression stage, which can be offloaded to io_uring worker threads or Intel QAT hardware.

The critical factor is the **similarity index lookup latency**. If the Super-Feature index fits in L3 cache (384MB on EPYC 9654 = ~24 million entries = ~1.5PB of data at 64KB chunks), lookups are sub-microsecond. Beyond L3, lookups hit DRAM (~100ns) or CXL memory (~300ns). This is why the compact Super-Feature index matters — it determines how much data a single node can deduplicate at NVMe speed.

### ClaudeFS Implementation Strategy

ClaudeFS implements similarity deduplication as a **two-tier approach**:

**Tier 1: Exact-match CAS (inline, always on)**
- BLAKE3 hash per chunk → exact match lookup in distributed metadata
- If the exact block exists, skip write entirely (reference counting increment)
- This catches the easy wins (identical blocks) at minimal CPU cost

**Tier 2: Similarity + delta compression (async, background)**
- After exact-match dedupe, extract Super-Features from remaining unique chunks
- Background threads scan for similar blocks in the feature index
- When similarity is found, compute delta and replace the full chunk with reference + delta
- Runs during idle periods or as part of the S3 tiering pipeline (compress with Zstd dictionary mode using the reference block as dictionary)

This two-tier approach keeps the hot write path simple (just BLAKE3 + lookup) while capturing the higher 3-4:1 reduction ratios in the background. The async tier runs on the same io_uring event loop, using worker threads for CPU-heavy delta compression.

### Legal Notes for Open-Source Implementation

The individual algorithms are unpatentable mathematical concepts freely usable in open source:
- FastCDC, Rabin fingerprints, MinHash, LSH — published academic algorithms
- BLAKE3, Zstd, LZ4 — open-source libraries with permissive licenses
- Delta compression — decades-old technique (xdelta, bsdiff)

VAST Data holds patents on their *specific combination and pipeline* (US10860548B2, US10656844B2, US11281387B2), particularly how they use SCM/NVRAM as an async staging buffer and their specific Super-Feature construction method. ClaudeFS's implementation differs in several fundamental ways:

- No SCM/NVRAM dependency — uses FDP-tagged NVMe + SLC journal instead
- Two-tier approach (exact inline + similarity async) vs VAST's single similarity pipeline
- Different feature construction (standard MinHash vs VAST's proprietary similarity hash)
- Different storage backend (distributed CAS with per-node metadata vs VAST's shared SCM state)
- Different erasure coding approach (per-segment EC vs VAST's massive global stripes)

The key principle: implement using published open algorithms (FastCDC, MinHash, Zstd dictionary), not by replicating VAST's patented step-by-step pipeline.

### Foundational Papers

See [docs/literature.md](literature.md) section 5 for full descriptions. Key papers: Finesse (FAST '14), FastCDC (USENIX ATC '16), Edelta (FAST '20), SiLo (USENIX ATC '11).

VAST patents US10860548B2, US10656844B2, US11281387B2 are useful for understanding the problem space (read the claims section to understand what to avoid).

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

See [docs/literature.md](literature.md) for the full paper catalog. Key references for data reduction:

- **Section 5** — Finesse, FastCDC, SiLo, Edelta, Speed-Dedup, ACM Dedup Survey (similarity deduplication and delta compression)
- **Section 6** — Z-LFS, ZNSage, FDP vs ZNS (flash management and data placement)
- **Section 8** — Linux Storage Stack Diagram 6.20, NFS LOCALIO, Rust Beyond-Refs (infrastructure)
