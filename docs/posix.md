# POSIX Compatibility Testing

Validating a distributed POSIX file system requires proving two things: **metadata correctness** (do permissions, hard links, and directory renames behave exactly as POSIX specifies?) and **data integrity under concurrency** (if Node A and Node B write to the same file over RDMA, does it corrupt?).

## 1. Strict POSIX Semantics (Gold Standards)

These suites test absolute edge cases — symlink traversal limits, timestamp updates (`mtime`/`ctime`/`atime`), and exact `errno` return codes.

### pjdfstest

The most heavily used test suite for strict POSIX metadata compliance. Originally written for FreeBSD, ported to Linux/ZFS. Runs thousands of granular tests on syscalls: `chmod`, `chown`, `link`, `mkdir`, `rename`, `unlink`, etc.

If ClaudeFS fails pjdfstest, standard Linux utilities (`tar`, `rsync`, `git`) will likely break.

- https://github.com/pjd/pjdfstest

### LTP — Open POSIX Test Suite

The Linux Test Project's Open POSIX Test Suite systematically validates standard C library POSIX system calls. Provides exhaustive coverage for locking (`fcntl`), memory mapping (`mmap`), and threading overlaps on files.

- https://github.com/linux-test-project/ltp
- Tests under `testcases/open_posix_testsuite`

## 2. Linux File System Integration

Even with user-space or RDMA kernel bypass, mount via FUSE or kernel module and run standard Linux torture tests to ensure native-disk behavior.

### xfstests (fstests)

Originally developed for SGI's XFS, now the official regression test suite for Ext4, Btrfs, and virtually all major Linux file systems. Tests deep structural edge cases, fragmentation, and POSIX compliance under heavy I/O load.

Credibility in the storage community requires passing a standard xfstests run.

- https://git.kernel.org/pub/scm/fs/xfs/xfstests-dev.git

### fsx (File System eXerciser)

Originally developed by Apple. Tortures the file system with random sequences of `read`, `write`, `truncate`, `mmap`, and `fallocate` operations, strictly verifying data against an in-memory oracle.

Aggressively finds bugs in block allocation, hole-punching, and page-cache coherency.

- Bundled inside xfstests (`ltp/fsx.c`)
- Standalone: https://github.com/linux-test-project/ltp/blob/master/testcases/kernel/fs/fsx-mac/fsx.c

## 3. Distributed & Scale-Out Validation

Local POSIX tests are not enough. Must test distributed lock management, cache invalidation, and network concurrency.

### NFS Connectathon (cthon04)

Written by Sun Microsystems to test NFS compliance. Tests file/record locking, directory creation, and data consistency from multiple clients simultaneously. Ensures that when Client A creates a file, Client B on another node immediately sees it with correct POSIX metadata.

- https://github.com/linux-nfs/cthon04

### Jepsen

Industry standard for testing distributed systems under network partitions (split-brain). Tests whether POSIX locks (`flock`) safely release and whether the metadata cluster survives node failures without corruption.

RDMA connections will drop. Jepsen validates behavior when they do.

- https://jepsen.io
- https://github.com/jepsen-io/jepsen

### FIO with Verification

Jens Axboe's I/O benchmarking tool, but not just for speed. The `--verify` parameter writes cryptographic checksums to blocks and reads them back. Validates that the NVMe+RDMA data path isn't dropping packets or writing to wrong block offsets.

- https://github.com/axboe/fio
- Key flags: `--verify`, `--verify_fatal`

## 4. Crash Consistency

POSIX assumes atomic states. If power is pulled mid-operation, the file system must be exactly in the state before the operation or exactly after it.

### CrashMonkey / ALICE

Academic test suites that find crash-consistency bugs. Record all block I/O operations, simulate crashes at various points, then verify POSIX file system structure integrity on remount.

Crash consistency is the #1 source of data-loss bugs in new file systems.

- CrashMonkey: https://github.com/utsaslab/crashmonkey
- ALICE: https://github.com/utsaslab/ALICE

## CI/CD Testing Pipeline

Run in this order:

1. **Unit tests** — `pjdfstest` on a single node. Validate POSIX directory trees, symlinks, permissions are 100% correct.
2. **Concurrency tests** — `fsx` to ensure multiple threads truncating and writing locally do not corrupt data.
3. **Distributed tests** — Mount on 3-5 nodes via RDMA fabric. Run NFS Connectathon from all nodes simultaneously targeting the same directories.
4. **Stress/regression** — Run the "Quick" suite of `xfstests`.
5. **Partition tolerance** — Jepsen tests for split-brain scenarios and lock safety.
6. **Crash recovery** — CrashMonkey/ALICE for power-failure atomicity.
7. **Data integrity at scale** — FIO with `--verify` across all nodes at sustained throughput.
