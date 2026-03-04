# Fix compile errors in endpoint_registry.rs tests

The test code in `crates/claudefs-transport/src/endpoint_registry.rs` has compile errors:

```
error[E0308]: mismatched types
   --> crates/claudefs-transport/src/endpoint_registry.rs:751:64
    |
751 |             registry.register_gossip(node_id(i), vec![tcp_addr(8000 + i)]);
    |                                                       -------- ^^^^^^^^ expected `u16`, found `u64`

error[E0308]: mismatched types
   --> crates/claudefs-transport/src/endpoint_registry.rs:794:55
    |
794 |                 reg.register_static(id, vec![tcp_addr(8000 + i)], ProtocolPreference::TcpOnly);
    |                                              -------- ^^^^^^^^ expected `u16`, found `u64`

error[E0597]: `registry` does not live long enough
   --> crates/claudefs-transport/src/endpoint_registry.rs:768:19
    |
768 |           let reg = &registry;
    |                     ^^^^^^^^^ borrowed value does not live long enough
...
771 |           let handle = thread::spawn(move || {
    |  ______________________-
772 | |             for _ in 0..100 {
773 | |                 reg.resolve(&id_clone);
774 | |             }
775 | |         });
    | |__________- argument requires that `registry` is borrowed for `'static`
```

## Fixes needed

1. Cast `8000 + i` to u16: `tcp_addr((8000 + i) as u16)` instead of `tcp_addr(8000 + i)`

2. For the thread safety test, wrap registry in `Arc` so it can be shared across threads:
```rust
use std::sync::Arc;

#[test]
fn test_concurrent_register_and_resolve() {
    let registry = Arc::new(EndpointRegistry::new(EndpointRegistryConfig::default()));
    let id = node_id(1);

    registry.register_static(id, vec![tcp_addr(8080)], ProtocolPreference::TcpOnly);

    let reg = Arc::clone(&registry);
    let id_clone = id;

    let handle = thread::spawn(move || {
        for _ in 0..100 {
            reg.resolve(&id_clone);
        }
    });

    for _ in 0..100 {
        registry.register_static(id, vec![tcp_addr(8080)], ProtocolPreference::TcpOnly);
    }

    handle.join().unwrap();

    assert!(registry.resolve(&id).is_some());
}
```

Read the current file, fix all errors, and write the corrected file.
