# AI Inference Acceleration: KV Cache on the Storage Mesh

ClaudeFS can serve as a high-speed KV cache tier for LLM inference workloads. This is a **client-side** capability — the GPUs live in compute/inference nodes, not in ClaudeFS storage nodes. The storage mesh provides the fast, shared NVMe fabric that inference frameworks like NVIDIA Dynamo use as extended memory.

## The Problem: The KV Cache Memory Wall

Large language models maintain a key-value (KV) cache during inference — the accumulated attention state from all previous tokens in a conversation. As AI moves from single prompts to agentic workflows (long-lived, multi-turn reasoning), this cache grows rapidly:

- A 70B parameter model with a 128K context window can require 40-80 GB of KV cache per session
- Multiple concurrent sessions on a single GPU exhaust VRAM quickly
- When VRAM fills, standard systems either evict the cache (forcing expensive recomputation) or refuse new requests

The KV cache is too large for GPU VRAM but too latency-sensitive for standard storage. It needs a tier between DRAM and disk — which is exactly what an NVMe mesh over RDMA provides.

## WEKA's NIXL Plugin and NVIDIA Dynamo

NVIDIA Dynamo is an open-source inference framework that disaggregates the prefill (input processing) and decode (output generation) phases of LLM inference.

**WEKA's contribution:** WEKA open-sourced a NIXL (NVIDIA Inference Transfer Library) plugin that allows Dynamo to stream KV cache blocks directly from WEKA's NVMe mesh at near-memory speeds via GPUDirect Storage.

**The results (validated at SC25):**
- Up to 4.2x more tokens per GPU by sustaining high KV cache hit rates and avoiding recomputation
- Time to First Token (TTFT) improvements up to 41x for long context windows that exceed DRAM
- 270 GB/s read throughput across 8 GPUs in Dynamo testing

The mechanism: instead of discarding old KV cache entries when VRAM fills, they are offloaded to the NVMe mesh via RDMA. When the agent needs that context again, it is pulled back into VRAM at 200+ GB/s — fast enough that the model barely notices the eviction.

## ClaudeFS as a KV Cache Tier

ClaudeFS's architecture is well-suited for this workload:

- **RDMA transport** — the same `libfabric` one-sided verbs used for file I/O can serve KV cache blocks at NVMe-native latency. No protocol translation.
- **GPUDirect Storage compatible** — inference GPUs register VRAM as RDMA targets. ClaudeFS storage nodes write KV blocks directly into GPU VRAM without CPU copies on either side. No GPU needed in the storage server.
- **Distributed NVMe pool** — KV cache entries are spread across the mesh. Multiple inference nodes can share cached context (e.g., system prompts, shared document embeddings) without each caching independently.
- **Flash-native latency** — io_uring NVMe passthrough provides sub-10 microsecond reads, vs hundreds of microseconds for standard filesystem reads. This keeps TTFT low.

### What ClaudeFS Needs to Add

A NIXL-compatible plugin for NVIDIA Dynamo, similar to what WEKA open-sourced. The plugin would:

1. **Register ClaudeFS RDMA endpoints** with the Dynamo scheduler
2. **Expose a KV block store API** — put/get operations for fixed-size KV cache blocks, keyed by (session_id, layer, token_range)
3. **Use the existing RDMA transport** — one-sided RDMA WRITE for eviction (GPU → storage), one-sided RDMA READ for reload (storage → GPU)
4. **Leverage the flash layer** — KV blocks are stored on the NVMe flash tier, not tiered to S3 (latency-sensitive)
5. **TTL-based eviction** — KV cache blocks are ephemeral. Automatic cleanup when sessions end or TTL expires. No deduplication or compression needed for this workload — the data is effectively random (attention weights).

This is a relatively thin integration layer on top of the existing RDMA transport and flash layer — not a new storage engine.

## Where the GPUs Live

**Correction to Gemini:** Gemini repeatedly suggests putting GPUs in storage nodes. This is wrong for ClaudeFS, as demonstrated in [docs/hardware.md](hardware.md):

- **Inference/compute GPUs (H100, B200, GB200)** — live in dedicated compute nodes. Run the actual LLM models. Use GPUDirect Storage to pull data from ClaudeFS over RDMA.
- **Storage nodes** — no GPUs. The CPU (EPYC 9654) already saturates NVMe throughput for all data reduction operations. Adding a GPU to a storage node wastes $5K-$8K and 300-600W per node on a non-bottleneck.

The "converged node" model (GPU + NVMe in every node) that Gemini and WEKA advocate makes sense when the same node runs both inference and storage. ClaudeFS's architecture disaggregates these roles: storage nodes are optimized for capacity, throughput, and reliability; compute nodes are optimized for GPU density. They scale independently.

```
Inference Nodes (H100/B200)          ClaudeFS Storage Nodes (no GPU)
┌─────────────────────┐              ┌─────────────────────┐
│  GPU VRAM           │   RDMA       │  NVMe Flash Layer   │
│  ┌───────────────┐  │◄────────────►│  ┌───────────────┐  │
│  │ KV Cache      │  │  GPUDirect   │  │ KV Blocks     │  │
│  │ (hot)         │  │  Storage     │  │ (warm)        │  │
│  └───────────────┘  │              │  └───────────────┘  │
│  LLM Model Weights  │              │  Training Data      │
│  Inference Engine    │              │  Checkpoints        │
└─────────────────────┘              └─────────────────────┘
```

## Related Research

- **NVIDIA Dynamo** — open-source disaggregated inference framework. Separates prefill and decode phases for independent scaling.
- **WEKA NIXL plugin** — open-source NIXL integration for NVIDIA Dynamo. Reference implementation for storage-backed KV cache.
- **NVIDIA Dynamo 0.4 Technical Blog** — details the disaggregated serving model and KV cache offloading.
- See [docs/literature.md](literature.md) Section 7 (BaM/GIDS) for GPU-to-storage communication research.
- See [docs/hardware.md](hardware.md) "Why No GPU in Storage Nodes" for the CPU vs GPU throughput analysis.
