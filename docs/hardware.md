# Hardware Reference

In 2026, NVMe mesh hardware is no longer passive — drives participate in data placement (FDP/ZNS), NICs offload entire storage stacks (DPUs), and memory pools span nodes (CXL). ClaudeFS's architecture is designed to exploit these capabilities.

## NVMe Drives

### Solidigm: The Density-Performance Leader

Solidigm dominates the scale-out NVMe space by focusing on density, data placement, and cost-per-TB rather than raw burst speed.

**D5-P5336 (61.44TB / 122.88TB)**

The flagship for read-heavy scale-out. In a ClaudeFS mesh, fewer drives per node means fewer network hops and simpler failure domains.

- 122TB in a single E3.S form factor — a 2U chassis holds ~2PB raw
- QLC NAND with CSAL (Cloud Storage Acceleration Layer) — an open-source host-based FTL that uses a small SLC/TLC cache to shape random writes into sequential stripes, achieving TLC-level performance at QLC prices
- CSAL is directly relevant to ClaudeFS: the Rust storage engine can integrate CSAL's write shaping with io_uring NVMe passthrough, letting the host control data placement rather than relying on the drive's internal FTL

**FDP (Flexible Data Placement)**

Solidigm is the primary driver of FDP, which is the practical alternative to ZNS for data placement control.

- FDP allows ClaudeFS to tag writes with placement hints (e.g., "metadata log," "snapshot data," "cold tier") — the drive places tagged data in separate physical NAND reclaim units
- Reduces Write Amplification Factor (WAF) to near 1.0 without ZNS's strict sequential-write rules
- Maintains the standard LBA block interface — no filesystem rewrite required
- ClaudeFS sends FDP directives via the `nvme-passthru` interface through io_uring

**FDP vs ZNS for ClaudeFS:**

| | FDP | ZNS |
|---|---|---|
| Write interface | Standard LBA (random OK) | Append-only zones (sequential required) |
| Data placement | Hint-based (advisory) | Zone-based (mandatory) |
| Firmware complexity | Standard FTL | Simplified FTL (host manages zones) |
| Filesystem impact | Minimal — add hint tags to writes | Major — requires log-structured design |
| WAF reduction | ~90% of ZNS benefit | Maximum (near 1.0) |
| Ecosystem support | Broad (standard block interface) | Narrow (requires ZNS-aware FS) |

**ClaudeFS decision:** Support both. FDP as the default (works with any modern NVMe), ZNS as an optional mode for metadata stores where absolute tail-latency control justifies the complexity. The io_uring NVMe passthrough path handles both — FDP via directive hints, ZNS via zone append commands.

### Other Drive Vendors

- **Samsung PM1743 (Gen5)** — highest raw IOPS for write-heavy workloads. Suitable as the write journal / SLC cache tier in front of high-density QLC. PCIe Gen5 x4 delivers 13 GB/s sequential read.
- **Kioxia CM7 series** — competitive Gen5 drives with strong ZNS support. Good alternative to Solidigm when ZNS is the primary placement strategy.
- **Western Digital Ultrastar DC SN860** — proven reliability for mixed workloads, strong enterprise support.

### Drive Tier Strategy

| Tier | Drive | Role in ClaudeFS |
|------|-------|-----------------|
| **Journal / Write cache** | Samsung PM1743 or Intel Optane (legacy) | SLC/TLC write absorption, metadata WAL, CSAL write shaper |
| **Flash layer (hot)** | Solidigm D5-P5336 (QLC) with FDP | Bulk data storage, read-heavy, maximum density |
| **S3 tier (cold)** | Object store (MinIO, AWS S3, etc.) | 64MB blobs, async tiering from flash |

## Compute: AMD EPYC

AMD EPYC 9005 series (Zen 5) is the preferred CPU for ClaudeFS storage nodes due to I/O density and hardware security offloads.

### Why AMD for Storage Nodes

- **128 PCIe Gen5 lanes per socket** — connect dozens of NVMe drives + 400GbE NICs directly to the CPU without PCIe switches. PCIe switches add latency and are a failure point.
- **384MB L3 cache (EPYC 9654)** — massive cache benefits the metadata server's fingerprint table lookups and inode resolution. The deduplication BLAKE3 hash table benefits enormously from L3 hit rates.
- **AVX-512** — hardware vectorization for LZ4/Zstd compression, BLAKE3 hashing, and AES-GCM encryption in the Rust data reduction pipeline. The `blake3` and `zstd` Rust crates auto-detect and use AVX-512 when available.
- **SEV-SNP (Secure Nested Paging)** — hardware-encrypted memory protects ClaudeFS metadata and data in-memory, even if the host is compromised. Relevant for multi-tenant and cloud deployments.

### Recommended SKUs

| SKU | Cores | Frequency | Use Case |
|-----|-------|-----------|----------|
| **EPYC 9654P** | 96 | 2.4 GHz base | Maximum parallelism — metadata servers, high-client-count nodes |
| **EPYC 9374F** | 32 | 4.05 GHz base | Maximum per-core performance — latency-sensitive RDMA paths |
| **EPYC 9454P** | 48 | 2.75 GHz base | Balanced — general-purpose storage nodes |

The 9374F is particularly interesting for metadata-heavy nodes where per-operation latency matters more than throughput. The 9654P suits data nodes serving many concurrent clients.

## Network: NICs and DPUs

The NIC is a co-processor in a modern NVMe mesh, not just a pipe. ClaudeFS needs RDMA (RoCE v2) for the performance transport and high-bandwidth TCP for the universal transport.

### NVIDIA ConnectX-7

The highest-performance traditional NIC for ClaudeFS's RDMA transport path.

- Up to 400 GbE / NDR InfiniBand
- Hardware-accelerated RoCE v2 with adaptive routing
- GPUDirect Storage — data moves from NVMe to GPU memory without CPU involvement (relevant for AI/ML workloads on ClaudeFS)
- Inline hardware encryption (IPsec/TLS) at line rate
- Best choice when the Rust host code handles compression/dedupe and the NIC handles pure transport

### NVIDIA BlueField-3 DPU

A full infrastructure-on-a-chip with onboard ARM cores and 16GB DDR5. For ClaudeFS, the DPU can offload entire subsystems:

- NVMe emulation via SNAP — present virtual NVMe namespaces to the host while the DPU manages the actual distributed storage
- Hardware AES-XTS encryption and compression at 400 Gb/s — offloads the data reduction pipeline from the host CPU
- pNFS and NFS gateway logic can run on the DPU's ARM cores, freeing the host EPYC entirely for metadata and data operations

**Architectural consideration:** If BlueField handles encryption/compression, the Rust code architecture changes — the host focuses on metadata, deduplication, and coordination while the DPU handles the data plane. This is an advanced deployment mode, not the default. The baseline assumes host-side processing in Rust.

### Broadcom Thor 2 (BCM57608)

The primary alternative for hyperscale environments prioritizing power efficiency and congestion control.

- 400 GbE PCIe 5.0 x16
- Best-in-class RoCE congestion control — critical during "incast" scenarios when many storage nodes respond to one client simultaneously (common in parallel reads across the ClaudeFS mesh)
- TruFlow engine offloads flow classification, keeping Rust threads focused on data logic
- Lower power than NVIDIA at similar throughput — relevant for dense deployments

### Intel E810 (Columbiaville)

The most cost-effective entry into RDMA storage, with the best in-kernel driver support.

- 100/200 GbE
- ADQ (Application Device Queues) — dedicate hardware NIC queues to ClaudeFS, isolating storage traffic from other workloads with zero jitter
- iWARP support — RDMA over standard TCP/IP, deployable on networks without lossless Ethernet (no PFC/ECN required). This is significant: many data centers cannot deploy RoCE due to network switch limitations. iWARP via E810 gives ClaudeFS RDMA performance on commodity networks.
- Best choice for budget-conscious or brownfield deployments

### NIC Selection Guide

| Scenario | Recommended NIC | Why |
|----------|----------------|-----|
| Greenfield HPC, maximum performance | ConnectX-7 (400GbE) | Highest RDMA throughput, GPUDirect |
| Offload everything to hardware | BlueField-3 DPU | ARM cores handle encryption, NFS, NVMe emulation |
| Hyperscale, power-sensitive | Broadcom Thor 2 | Best congestion control, lowest power |
| Enterprise, budget, brownfield | Intel E810 | iWARP on commodity networks, ADQ isolation |

## Chassis: Supermicro

Supermicro's Petascale line provides the physical density for NVMe-heavy ClaudeFS nodes.

### Key Innovations

- **E3.S / E1.S form factors (EDSFF)** — up to 32 hot-swap E3.S Gen5 drives in 2U. Shorter signal paths than U.2 cables reduce noise and maintain low latency at Gen5 speeds.
- **CXL 3.0 expansion slots (X14 series)** — dedicated CXL Type 3 slots for external memory pools. ClaudeFS can use CXL memory as a low-latency cache for the distributed deduplication fingerprint table, avoiding the NVMe-to-CPU round trip for metadata lookups.
- **Thermal management for QLC** — optimized airflow and liquid-cooling options prevent thermal throttling during sustained writes and background garbage collection on high-density QLC drives.

### Reference Node Configurations

#### High-Performance Node (~2PB raw, 2U)

| Component | Specification |
|-----------|--------------|
| CPU | 2x AMD EPYC 9654P (96C each) |
| Memory | 768GB DDR5-4800 + CXL expansion |
| Drives (data) | 16x Solidigm D5-P5336 122TB (E3.S) |
| Drives (journal) | 2x Samsung PM1743 3.84TB (Gen5) |
| NIC | 2x NVIDIA ConnectX-7 400GbE |
| Chassis | Supermicro Petascale 2U E3.S |

#### Budget Node (~200TB raw, 1U)

| Component | Specification |
|-----------|--------------|
| CPU | 1x AMD EPYC 9454P (48C) |
| Memory | 256GB DDR5-4800 |
| Drives (data) | 8x Solidigm D5-P5336 30.72TB (E3.S) |
| Drives (journal) | 1x Samsung PM1743 1.92TB |
| NIC | 2x Intel E810 100GbE |
| Chassis | Supermicro 1U E3.S |

#### Development / Test Node

| Component | Specification |
|-----------|--------------|
| CPU | 1x AMD EPYC 9374F (32C, high frequency) |
| Memory | 128GB DDR5 |
| Drives | 4x consumer NVMe (any Gen4/Gen5) |
| NIC | 1x Intel E810 100GbE or RoCE-capable NIC |
| Chassis | Any 1U with M.2/U.2 slots |

## Why No GPU in Storage Nodes

A common question: should each storage node include an NVIDIA RTX 6000 or Blackwell GPU to accelerate deduplication, compression, and encryption? The answer is no — the math doesn't support it.

### The CPU Already Saturates NVMe

A 96-core AMD EPYC 9654 with AVX-512 provides aggregate throughput that far exceeds what the storage bus can feed:

| Operation | Per-Core Throughput | 96-Core Aggregate | NVMe Ceiling (16 drives) |
|-----------|-------------------|-------------------|--------------------------|
| BLAKE3 hashing | ~8 GB/s | ~768 GB/s | ~100-200 GB/s |
| LZ4 compression | ~4 GB/s | ~384 GB/s | ~100-200 GB/s |
| AES-256-GCM encryption | ~10 GB/s | ~960 GB/s | ~100-200 GB/s |
| FastCDC chunking | ~2-4 GB/s | ~192-384 GB/s | ~100-200 GB/s |
| Zstd compression | ~1 GB/s | ~96 GB/s | ~100-200 GB/s |

Even Zstd — the slowest operation — provides 96 GB/s aggregate, close to or above the NVMe throughput ceiling. For every other operation, the CPU has 3-8x more throughput than the storage can deliver. A GPU solving a non-existent bottleneck adds cost without benefit.

NVIDIA Blackwell's 800 GB/s hardware decompression engine is impressive, but it decompresses faster than the drives can feed data — the bottleneck is NVMe, not the CPU.

### The Cost of Per-Node GPUs

| Cost Factor | Impact |
|------------|--------|
| Hardware | $5,000-$8,000 per node (RTX 6000 Ada / RTX Pro 6000 Blackwell) |
| Power | 300-600W TDP per GPU — significant cooling and electricity cost |
| PCIe lanes | x16 lanes consumed that could serve another NVMe drive or 400GbE NIC |
| Complexity | CUDA dependency in codebase, more `unsafe` Rust, less mature ecosystem |
| 20-node cluster | $100,000-$160,000 in GPUs accelerating operations that aren't bottlenecked |

### What About GPUDirect Storage?

GPUDirect Storage (GDS) is valuable — but it's a **client-side** optimization, not a storage server feature. GDS allows AI/ML compute nodes to DMA training data directly from the network into GPU VRAM, bypassing client CPU memory. This works with ClaudeFS's RDMA transport: the *client's* GPU registers VRAM as an RDMA-accessible memory region, and the storage server sends data via standard RDMA writes. No GPU is needed on the storage server.

### Decision

ClaudeFS storage nodes do not include GPUs. The RDMA transport is GPUDirect Storage compatible for client-side GPU clusters (AI/ML training). If a future workload demonstrates a genuine CPU bottleneck in the data reduction pipeline, GPU acceleration can be added behind a trait-based abstraction with a `--features gpu` build flag — but the current hardware math shows no such bottleneck exists.

## Related Research

See [docs/literature.md](literature.md) for the full paper catalog. Key references for hardware:

- **Section 6** — FDP vs ZNS comparative study (supports "FDP default, ZNS optional")
- **Section 7** — BaM/GIDS GPU-to-storage communication (GPUDirect Storage compatibility)
- **Section 9** — Linux Storage Stack Diagram 6.20 (io_uring, NVMe-oF, FDP kernel interfaces)
