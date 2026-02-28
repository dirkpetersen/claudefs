# Language Choice: Rust vs C++26

ClaudeFS is written in Rust. This document explains why, including an honest assessment of where Rust creates friction and where C++ would have advantages.

## The Core Argument: Compiler-Enforced Safety vs Discipline-Based Safety

C++26 has made massive strides — Safety Profiles, `std::expected`, `std::span`, contracts. But these are **opt-in**. Rust's memory safety, aliasing rules, and thread-safety (`Send`/`Sync`) are **enforced by the compiler**. In a distributed storage system where bugs manifest as silent data corruption under load, this distinction is existential.

The practical difference: when AI generates 5,000 lines of C++ that compile cleanly, you still need to audit for use-after-move, dangling references in lambdas, and data races that only trigger at high IOPS. When AI generates 5,000 lines of Rust that compile cleanly, those classes of bugs are eliminated by construction.

## AI-Assisted Development: The Compiler Feedback Loop

Both Claude and Gemini are highly capable in both languages. But the development loop differs fundamentally:

**Rust:** AI writes code -> compiler rejects it with a specific error -> AI fixes the logic -> compiler accepts -> the code is memory-safe and data-race-free. The compiler acts as a real-time correctness checker on the AI's output.

**C++:** AI writes code -> compiler accepts it -> code may contain a subtle race condition or lifetime bug -> bug surfaces under load in production -> debugging session begins.

This matters enormously for a project like ClaudeFS where:
- Concurrent access to shared metadata structures is the norm
- RDMA one-sided verbs write directly into memory buffers without CPU involvement
- Crash consistency requires precise control over write ordering
- The system runs for months without restart

### Where AI Struggles with Rust

Honesty requires acknowledging the friction:

- **Complex lifetimes in async code** — AI (both Claude and Gemini) sometimes gets stuck in borrow-checker loops with `async` traits, self-referential structs, and complex lifetime annotations. The fix is usually to simplify the architecture (use `Arc` instead of references, restructure ownership) rather than to fight the borrow checker.
- **Verbosity** — Rust's explicit error handling (`Result<T, E>` everywhere) and type definitions produce more lines of code than equivalent C++. AI-generated Rust files are typically 20-30% larger.
- **`unsafe` is unavoidable** — ClaudeFS will need `unsafe` blocks for raw NVMe passthrough via io_uring, RDMA buffer registration, and parts of the FUSE interface. The borrow checker cannot help inside `unsafe`. The discipline here is to minimize and isolate `unsafe` behind safe abstractions.

### Where AI Excels with Rust

- **Fearless concurrency** — asking AI to "make this single-threaded storage loop multi-threaded" produces code that, if it compiles, is guaranteed race-free. In C++, the same refactor is a source of Heisenbugs.
- **Refactoring safety** — large-scale restructuring (changing ownership patterns, splitting modules) is safe if it compiles. The compiler catches every broken invariant.
- **The error message loop** — Rust's compiler errors are specific and actionable. Pasting them back to the AI almost always produces a correct fix. C++ template errors are notoriously unhelpful.

## Ecosystem Comparison for ClaudeFS

| Component | Rust | C++ |
|-----------|------|-----|
| FUSE v3 | `fuser` crate — mature, actively maintained | `libfuse3` — C library, requires manual bindings |
| io_uring | `io-uring` crate, `tokio-uring`, `glommio` | `liburing` — C library, C++ wrappers vary in quality |
| RDMA / libfabric | `rdma-sys`, bindgen wrappers — functional but thin | Native C API, C++ wrappers available |
| S3 | `aws-sdk-rust` — official AWS SDK | `aws-sdk-cpp` — official but heavy |
| Async runtime | Tokio — production-grade, massive ecosystem | No standard; Boost.Asio, custom event loops |
| Serialization | `serde` + `bincode`/`prost` — fast, ergonomic | protobuf, flatbuffers — fast, more boilerplate |
| Build system | Cargo — single tool, reproducible, fast | CMake/Meson — powerful but fragmented |
| Package management | crates.io — centralized, versioned | Conan/vcpkg — improving but still painful |

Key observation: the Rust ecosystem for storage systems (io_uring, FUSE, async runtimes) has matured significantly. The gap that existed in 2022 is largely closed. Cargo alone is a major productivity advantage over CMake for a project of this scale.

## The `unsafe` Budget

ClaudeFS will need `unsafe` for:
- **io_uring shared buffers** — registered memory regions for zero-copy I/O
- **RDMA buffer registration** — pinned memory for one-sided verbs
- **FUSE low-level interface** — raw inode operations
- **NVMe passthrough commands** — raw command construction for `IORING_OP_URING_CMD`

The strategy: wrap each `unsafe` boundary in a safe abstraction, audit these modules rigorously, and use `miri` and `cargo-careful` for unsafe code testing. The rest of the codebase (metadata logic, replication, S3 tiering, protocol handling) stays in safe Rust.

## C++ Advantages We Accept Losing

- **Micro-optimization tribal knowledge** — 40 years of C++ public code means AI can suggest highly specific SIMD intrinsics, bit-manipulation hacks, and cache-line tricks. Rust equivalents exist but are less documented.
- **C++26 reflection** (`^^` and `[: :]`) — native compile-time reflection for serialization codegen. Rust's `derive` macros cover much of this but less flexibly.
- **"Break the rules" flexibility** — when you need an unsafe NVMe performance trick, C++ lets you do it inline. Rust forces you to wrap it in `unsafe` and justify it.
- **Faster dirty prototyping** — C++ is less "picky." You can get a working (if buggy) prototype faster because the compiler doesn't enforce ownership rules.

For ClaudeFS, none of these outweigh the safety guarantees. A storage system that silently corrupts data under load is worse than one that took longer to write.

## The Verdict

Rust is the right choice for ClaudeFS because:

1. **Storage systems die on Heisenbugs** — bugs that only appear under specific timing, load, or failure conditions. AI is prone to creating these in C++ because it doesn't reason about thread interleaving. In Rust, data races don't compile.
2. **The compiler is a force multiplier for AI** — the borrow checker catches the AI's mistakes before they reach runtime. This is especially valuable when multiple developers (human and AI) contribute code.
3. **The ecosystem is ready** — `fuser`, `tokio-uring`, `libfabric` bindings, `serde`, Cargo. The tooling gap that historically favored C++ for systems work has closed.
4. **`unsafe` is contained** — the parts of ClaudeFS that need `unsafe` (io_uring, RDMA, FUSE FFI) are well-bounded. The vast majority of the codebase — metadata, replication, protocol, S3 tiering — benefits from full safety guarantees.
