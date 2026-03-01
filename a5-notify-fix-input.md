Fix the file /home/cfs/claudefs/crates/claudefs-fuse/src/notify_filter.rs.

The struct `NotifyFilter` is missing - it was accidentally removed. You need to add the struct definition BEFORE line 62 (before `impl NotifyFilter {`).

The struct to insert is:

```rust
#[derive(Debug, Clone, Default)]
pub struct NotifyFilter {
    pub filter_type: FilterType,
    pub action: FilterAction,
    pub pattern: Option<String>,
    pub enabled: bool,
}

```

Insert this block between line 60 (closing `}` of `NotifyFilterStats` impl) and line 62 (start of `impl NotifyFilter {`).

The file currently looks like this at lines 59-63:
```
    }
}

impl NotifyFilter {
    pub fn new(filter_type: FilterType) -> Self {
```

After the fix it should look like:
```
    }
}

#[derive(Debug, Clone, Default)]
pub struct NotifyFilter {
    pub filter_type: FilterType,
    pub action: FilterAction,
    pub pattern: Option<String>,
    pub enabled: bool,
}

impl NotifyFilter {
    pub fn new(filter_type: FilterType) -> Self {
```

Read the file, insert the struct definition in the right place, and write the updated file back.
