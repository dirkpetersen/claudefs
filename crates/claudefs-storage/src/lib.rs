#![warn(missing_docs)]

//! ClaudeFS storage subsystem: Local NVMe I/O via io_uring, FDP/ZNS placement, block allocator

pub mod allocator;
pub mod block;
pub mod device;
pub mod flush;
pub mod io_uring_bridge;
pub mod zns;
