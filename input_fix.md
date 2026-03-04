# Fix compile errors in endpoint_registry.rs

The file at `crates/claudefs-transport/src/endpoint_registry.rs` has compile errors:

```
error[E0277]: the `?` operator can only be used in a method that returns `Result` or `Option`
   --> crates/claudefs-transport/src/endpoint_registry.rs:263:47
    |
260 |     pub fn resolve_all(&self, node_id: &NodeId) -> Vec<TransportAddr> {
    |     ----------------------------------------------------------------- this function should return `Result` or `Option` to accept `?`
...
263 |         let entry = inner.entries.get(node_id)?;
    |                                               ^ cannot use the `?` operator in a method that returns `Vec<TransportAddr>`

error[E0502]: cannot borrow `inner` as mutable because it is also borrowed as immutable
   --> crates/claudefs-transport/src/endpoint_registry.rs:224:9
    |
205 |         let entry = inner.entries.get(node_id)?;
    |                     ----- immutable borrow occurs here
...
224 |         inner.stats.hits += 1;
    |         ^^^^^ mutable borrow occurs here
...
227 |         let filtered = match entry.preference {
    |                              ---------------- immutable borrow later used here
```

## Fixes needed

1. In `resolve_all`: Instead of using `?`, check if entry exists and return empty Vec if not found. Handle expiry check similarly.

2. In `resolve`: Clone the entry data (node_id, addrs, preference, expires_at) before modifying stats or other parts of inner. This avoids borrow conflicts.

Example pattern for resolve():
```rust
pub fn resolve(&self, node_id: &NodeId) -> Option<TransportAddr> {
    let mut inner = self.inner.write().unwrap();
    inner.stats.lookups += 1;

    // Clone the entry data to avoid borrow conflicts
    let entry = match inner.entries.get(node_id) {
        Some(e) => EndpointList {
            node_id: e.node_id,
            addrs: e.addrs.clone(),
            preference: e.preference,
            expires_at: e.expires_at,
        },
        None => {
            inner.stats.misses += 1;
            return None;
        }
    };
    
    // Now we can freely mutate inner since we own entry
    // ... rest of logic
}
```

Also fix the warnings about unnecessary `mut` on variables.

Read the current file, fix all errors and warnings, and write the corrected file.
