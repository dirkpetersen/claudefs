# GPU Acceleration (Optional)

GPU acceleration in ClaudeFS is strictly optional. Every operation — deduplication, compression, encryption, metadata search — has a CPU-only implementation that runs on any hardware. When a GPU is present, ClaudeFS offloads specific workloads to it for significant speedup. The system auto-detects GPU availability at startup.

## Design Principle: CPU-First, GPU-Accelerated

This is not two separate codebases. It is one Rust implementation with a trait-based abstraction:

```
trait DataReductionEngine {
    fn chunk(&self, data: &[u8]) -> Vec<Chunk>;
    fn hash(&self, chunks: &[Chunk]) -> Vec<Fingerprint>;
    fn compress(&self, data: &[u8]) -> Vec<u8>;
    fn decompress(&self, data: &[u8]) -> Vec<u8>;
}

// Two implementations, same interface
struct CpuEngine { /* AVX-512, BLAKE3, LZ4/Zstd */ }
struct GpuEngine { /* CUDA kernels, nvCOMP, GPU hashing */ }
```

The storage engine calls through the trait. At startup, if a CUDA-capable GPU is detected and the `claudefs-gpu` feature flag is enabled, the GPU engine is used. Otherwise, the CPU engine runs. Both produce identical output — same chunk boundaries, same hashes, same compressed format. A node can switch between CPU and GPU mode across restarts without data migration.

### Why Not Two Codebases

Gemini suggests the GPU fundamentally changes the architecture. It doesn't — and shouldn't:

- **Correctness comes from one path** — if the CPU and GPU implementations produce different chunking or hashing results, you get data corruption when a GPU node fails and a CPU node takes over. One trait, one test suite, two backends.
- **AI can write both backends** — the trait interface is the contract. Claude writes the CPU implementation, then writes the GPU implementation against the same tests. The borrow checker ensures both backends handle buffer ownership identically.
- **Build-time feature flag** — `cargo build --features gpu` pulls in CUDA dependencies. The default build has zero GPU dependencies, keeping the binary portable and the dependency tree small.

## What a GPU Accelerates

### 1. Hardware Decompression (Blackwell)

NVIDIA Blackwell GPUs (RTX Pro 6000, B200) include a **fixed-function hardware decompression engine** — not CUDA cores, dedicated silicon:

- Decompresses LZ4, Snappy, and Deflate at up to 800 GB/s
- Does not consume CUDA cores — runs in parallel with other GPU work
- Relevant for the read path: data stored compressed on NVMe is decompressed in-flight as it moves from disk to network, with the CPU never touching the compressed blocks

**Impact on ClaudeFS:** On the read path, GPUDirect Storage can DMA compressed data from NVMe directly into GPU memory, decompress in hardware, and serve it out the RDMA NIC — all without CPU involvement. This is the most compelling GPU use case because it frees the CPU entirely for metadata and coordination.

On Ada-generation GPUs (RTX 6000 Ada, 48GB), decompression runs on CUDA cores instead of dedicated hardware — still faster than CPU for large blocks, but consumes SM resources.

### 2. Parallel Hashing for Deduplication

Deduplication fingerprinting (BLAKE3, SHA-256) is embarrassingly parallel. A GPU with thousands of cores computes hashes 10-100x faster than a 96-core CPU:

- **Content-Defined Chunking** — the rolling hash (FastCDC/Rabin) scans incoming data for chunk boundaries. GPU-parallel Gear Hash implementations process the entire data stream in one pass.
- **Fingerprint computation** — compute BLAKE3 hashes for thousands of chunks simultaneously
- **Similarity feature extraction** — MinHash Super-Feature generation is parallel per-chunk, ideal for GPU

**The math:** With 96 GB VRAM (RTX Pro 6000 Blackwell), the GPU can buffer ~80-90 GB of incoming write data and compute all dedup hashes in a single parallel pass before flushing to NVMe. The entire similarity index for ~900 million blocks fits in VRAM, making lookups sub-microsecond.

### 3. GPU-Accelerated Compression

NVIDIA's `nvCOMP` library provides GPU-accelerated LZ4, Snappy, Zstd, and Deflate:

- Compression throughput: 50-100 GB/s on Blackwell (vs ~4 GB/s per CPU core for LZ4)
- The GPU can compress an entire 64MB S3 blob in one parallel pass
- For the write path: incoming data → GPU chunking + hashing + compression → write compressed blocks to NVMe

On CPU-only nodes, LZ4 at ~4 GB/s per core across 96 cores still provides ~100+ GB/s aggregate — sufficient for most workloads. The GPU advantage is most significant on nodes with fewer CPU cores or during burst writes.

### 4. Encryption Offload

AES-256-GCM runs efficiently on both CPU (AES-NI) and GPU (CUDA):

- GPU encryption is faster for bulk data but adds a PCIe transfer if the data isn't already in VRAM
- If the GPU is already handling decompression or compression (data is in VRAM), encryption as the next pipeline stage is nearly free
- On CPU-only nodes, AES-NI provides ~10 GB/s per core — sufficient for inline encryption

### 5. Metadata Query Acceleration (Experimental)

With 96 GB VRAM, the GPU can host the hot metadata index:

- ~1 billion metadata records at ~100 bytes each fit entirely in VRAM
- DuckDB/Arrow GPU kernels could execute analytical queries (top users, largest directories) as GPU operations
- This is speculative and depends on the maturity of GPU-accelerated SQL engines in 2026

### 6. GPUDirect Storage (GDS)

The most impactful integration is not compute but **data movement**:

- GDS creates a direct DMA path between NVMe drives and GPU memory, bypassing CPU system memory entirely
- Data moves: NVMe → (PCIe) → GPU VRAM → (RDMA NIC) → remote client GPU
- Reduces latency by up to 3.8x and increases bandwidth by 2-8x vs CPU-bounced transfers
- Critical for AI/ML workloads: training data feeds directly from the ClaudeFS mesh into GPU VRAM without CPU copies

GDS works with ClaudeFS's RDMA transport (libfabric) — the GPU registers VRAM as RDMA-accessible memory regions.

## GPU Hardware Options

| GPU | VRAM | Decompression | PCIe | Best For |
|-----|------|---------------|------|----------|
| **RTX Pro 6000 Blackwell** | 96 GB GDDR7 | Hardware engine (800 GB/s) | Gen5 x16 | Full pipeline offload, large dedup tables |
| **RTX 6000 Ada** | 48 GB GDDR6 ECC | CUDA-based | Gen4 x16 | Dedup hashing, compression, GDS |
| **NVIDIA B200** | 192 GB HBM3e | Hardware engine | Gen5 x16 | Data center AI + storage convergence |
| **No GPU** | — | — | — | CPU-only mode, full functionality |

The RTX Pro 6000 Blackwell (96 GB) is the sweet spot for storage nodes: enough VRAM to hold the dedup index for a multi-PB namespace, hardware decompression engine, and PCIe Gen5 for full 400GbE line rate with GDS.

The RTX 6000 Ada (48 GB) is a cost-effective option when dedup acceleration is the primary goal and hardware decompression isn't needed.

## The CPU-Only Baseline

Without a GPU, ClaudeFS uses:

| Operation | CPU Implementation | Performance (96-core EPYC 9654) |
|-----------|-------------------|--------------------------------|
| FastCDC chunking | SIMD rolling hash | ~2-4 GB/s per core |
| BLAKE3 hashing | AVX-512 native | ~8 GB/s per core |
| LZ4 compression | SIMD-accelerated | ~4 GB/s per core |
| Zstd compression | AVX-512 | ~1 GB/s per core |
| AES-256-GCM | AES-NI | ~10 GB/s per core |
| Similarity features | SIMD MinHash | ~3-5 GB/s per core |

Across 96 cores, aggregate throughput for the full pipeline is ~50-100 GB/s — well above the NVMe throughput ceiling of even a fully loaded node. **The CPU-only path is not slow.** The GPU accelerates it further and frees CPU cores for metadata, networking, and coordination.

## Parallel Development with AI

The trait-based design enables genuinely parallel development:

1. **Define the trait interface and test suite first** — property-based tests that verify: given the same input, CPU and GPU backends produce byte-identical output
2. **AI writes the CPU backend** — uses existing Rust crates (`blake3`, `lz4_flex`, `zstd`, `fastcdc`). This is the well-trodden path with mature libraries.
3. **AI writes the GPU backend in parallel** — CUDA kernels via `cudarc` or `rust-cuda` crates, `nvCOMP` bindings for compression. This is more experimental and may require more iteration.
4. **Both backends pass the same test suite** — if they diverge, the tests catch it immediately

The CPU backend ships first (it's simpler, all dependencies are mature). The GPU backend ships as an optional feature when ready. No blocking dependency between the two.

### Risks of GPU Development

- **CUDA Rust ecosystem maturity** — `cudarc` and `rust-cuda` are functional but less mature than the CPU crate ecosystem. Expect more `unsafe` code and fewer examples.
- **nvCOMP licensing** — verify that nvCOMP's license is compatible with MIT for distribution
- **Testing on CI** — GPU tests require CUDA-capable hardware in CI. Use feature flags to separate GPU tests from the main test suite.
- **Fallback correctness** — the most critical test: a mixed cluster where some nodes have GPUs and others don't must produce identical results. The trait abstraction enforces this, but integration testing across heterogeneous nodes is essential.

## Rust Crates

| Purpose | CPU Crate | GPU Crate/Binding |
|---------|-----------|-------------------|
| Hashing | `blake3` (AVX-512) | Custom CUDA kernel or `cudarc` |
| Chunking | `fastcdc` | Custom CUDA Gear Hash |
| Compression | `lz4_flex`, `zstd` | `nvcomp` bindings |
| Encryption | `aes-gcm` (AES-NI) | CUDA AES kernel |
| Data movement | io_uring | GPUDirect Storage (`cufile`) |
| GPU runtime | — | `cudarc` or `rust-cuda` |
