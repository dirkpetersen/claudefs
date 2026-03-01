# A8: Fix notify_filter.rs Compilation Errors

## Problems

1. **Conflicting Default implementations:** Line 62 derives Default, line 70 implements Default manually
   - Rust allows ONE OR THE OTHER, not both
   - Fix: Remove the #[derive(Default)] from line 62 struct, keep manual impl

2. **AtomicU64 doesn't implement Clone:** NotifyFilterStats has #[derive(Clone)] but contains AtomicU64
   - AtomicU64 is explicitly not Clone-safe for good reasons
   - Fix: Remove #[derive(Clone)] from NotifyFilterStats, implement manually or use interior mutability pattern

## Solution

### NotifyFilter (lines 62-79)
- Change line 62 from `#[derive(Debug, Clone, Default)]` to `#[derive(Debug, Clone)]`
- Keep the manual `impl Default for NotifyFilter` on lines 70-79

### NotifyFilterStats (lines 21-60)
- Option A: Remove `Clone` derive and implement manually
  ```rust
  impl Clone for NotifyFilterStats {
      fn clone(&self) -> Self {
          Self {
              matched_count: AtomicU64::new(self.matched_count.load(Ordering::SeqCst)),
              suppressed_count: AtomicU64::new(self.suppressed_count.load(Ordering::SeqCst)),
              throttled_count: AtomicU64::new(self.throttled_count.load(Ordering::SeqCst)),
              total_checked: AtomicU64::new(self.total_checked.load(Ordering::SeqCst)),
          }
      }
  }
  ```

- Option B (Recommended): Wrap in Arc for shared ownership
  ```rust
  use std::sync::Arc;
  pub struct NotifyFilterStats(Arc<NotifyFilterStatsInner>);

  struct NotifyFilterStatsInner {
      matched_count: AtomicU64,
      // ... etc
  }
  ```

Recommend Option A for simplicity. Just implement Clone explicitly, cloning the atomic values at the time of clone.

## Output
Fixed crates/claudefs-fuse/src/notify_filter.rs with zero errors.
