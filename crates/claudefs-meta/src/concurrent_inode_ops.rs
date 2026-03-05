use crate::types::*;
use std::collections::HashMap;

/// Client ID for concurrent operations tracking.
pub type ClientId = u64;

#[derive(Clone, Debug)]
pub struct ConcurrentOpContext {
    pub inode_id: InodeId,
    pub operations: Vec<(ClientId, InodeOp)>,
    pub expected_final_state: InodeAttr,
    pub raft_order: Vec<(Term, LogIndex)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum InodeOp {
    Write { offset: u64, data: Vec<u8> },
    SetAttr { changes: AttrChanges },
    Chmod { mode: u32 },
    Chown { uid: u32, gid: u32 },
    Truncate { size: u64 },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AttrChanges {
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub size: Option<u64>,
    pub atime: Option<Timestamp>,
    pub mtime: Option<Timestamp>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LinearizabilityResult {
    Valid { raft_log_order: Vec<LogIndex> },
    Invalid { violation: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Violation {
    WriteSkew,
    LostUpdate,
    ReadAfterWrite,
    PhantomRead,
}

/// Verifies that concurrent operations appear linearizable.
pub fn verify_linearizability(ops: &[ConcurrentOpContext]) -> LinearizabilityResult {
    if ops.is_empty() {
        return LinearizabilityResult::Valid {
            raft_log_order: vec![],
        };
    }

    let mut all_operations: Vec<(Term, LogIndex, InodeOp)> = Vec::new();

    for ctx in ops {
        let raft_entries: Vec<_> = ctx.raft_order.iter().take(ctx.operations.len()).collect();

        for (i, (_, op)) in ctx.operations.iter().enumerate() {
            if let Some((term, log_idx)) = raft_entries.get(i) {
                all_operations.push((*term, *log_idx, op.clone()));
            }
        }
    }

    if all_operations.is_empty() {
        return LinearizabilityResult::Valid {
            raft_log_order: vec![],
        };
    }

    let sorted = serialize_operations(&all_operations);

    let mut attr = InodeAttr::new_file(ops[0].inode_id, 0, 0, 0o644, 1);

    for (_, op) in sorted.iter() {
        apply_operation(&mut attr, op);
    }

    let expected = &ops[0].expected_final_state;
    if attr.size != expected.size || attr.mode != expected.mode {
        return LinearizabilityResult::Invalid {
            violation: format!(
                "Final state mismatch: got size={}, mode={}, expected size={}, mode={}",
                attr.size, attr.mode, expected.size, expected.mode
            ),
        };
    }

    LinearizabilityResult::Valid {
        raft_log_order: sorted.iter().map(|(idx, _)| *idx).collect(),
    }
}

/// Simulates multiple concurrent writes to same inode.
pub fn simulate_concurrent_writes(ops: &[(ClientId, InodeOp)]) -> InodeAttr {
    let mut attr = InodeAttr::new_file(InodeId::new(1), 0, 0, 0o644, 1);

    let sorted_ops: Vec<_> = ops
        .iter()
        .enumerate()
        .map(|(i, (_, op))| (LogIndex::new(i as u64 + 1), op.clone()))
        .collect();

    for (_, op) in sorted_ops {
        apply_operation(&mut attr, &op);
    }

    attr
}

/// Verifies operations are consistently ordered by log index.
pub fn check_raft_order_consistency(ops: &[(LogIndex, InodeOp)]) -> bool {
    if ops.is_empty() {
        return true;
    }

    let mut indices: Vec<u64> = ops.iter().map(|(idx, _)| idx.as_u64()).collect();
    indices.sort();

    for i in 1..indices.len() {
        if indices[i] <= indices[i - 1] {
            return false;
        }
    }

    true
}

/// Detects write skew anomaly in concurrent operations.
pub fn detect_write_skew(ops: &[(ClientId, InodeOp)]) -> Option<Violation> {
    let mut write_ops: Vec<(&ClientId, u64, &Vec<u8>)> = Vec::new();

    for (client_id, op) in ops {
        if let InodeOp::Write { offset, data } = op {
            write_ops.push((client_id, *offset, data));
        }
    }

    if write_ops.len() < 2 {
        return None;
    }

    let mut offset_groups: HashMap<u64, Vec<&Vec<u8>>> = HashMap::new();
    for (_, offset, data) in &write_ops {
        offset_groups.entry(*offset).or_default().push(data);
    }

    for (_, group) in offset_groups {
        if group.len() >= 2 {
            return Some(Violation::WriteSkew);
        }
    }

    None
}

/// Applies a single operation to inode attributes.
pub fn apply_operation(attr: &mut InodeAttr, op: &InodeOp) {
    match op {
        InodeOp::Write { offset, data } => {
            let new_size = *offset + data.len() as u64;
            if new_size > attr.size {
                attr.size = new_size;
            }
            attr.blocks = (attr.size + 511) / 512;
            attr.mtime = Timestamp::now();
        }
        InodeOp::SetAttr { changes } => {
            merge_setattr_changes(attr, changes);
        }
        InodeOp::Chmod { mode } => {
            attr.mode = *mode;
            attr.ctime = Timestamp::now();
        }
        InodeOp::Chown { uid, gid } => {
            attr.uid = *uid;
            attr.gid = *gid;
            attr.ctime = Timestamp::now();
        }
        InodeOp::Truncate { size } => {
            attr.size = *size;
            attr.blocks = (attr.size + 511) / 512;
            attr.mtime = Timestamp::now();
            attr.ctime = Timestamp::now();
        }
    }
}

/// Merges SetAttr changes into inode attributes.
pub fn merge_setattr_changes(attr: &mut InodeAttr, changes: &AttrChanges) {
    if let Some(mode) = changes.mode {
        attr.mode = mode;
    }
    if let Some(uid) = changes.uid {
        attr.uid = uid;
    }
    if let Some(gid) = changes.gid {
        attr.gid = gid;
    }
    if let Some(size) = changes.size {
        attr.size = size;
        attr.blocks = (attr.size + 511) / 512;
    }
    if let Some(atime) = changes.atime {
        attr.atime = atime;
    }
    if let Some(mtime) = changes.mtime {
        attr.mtime = mtime;
    }
    attr.ctime = Timestamp::now();
}

/// Sorts operations by (Term, LogIndex) to determine Raft order.
pub fn serialize_operations(ops: &[(Term, LogIndex, InodeOp)]) -> Vec<(LogIndex, InodeOp)> {
    let mut sorted: Vec<_> = ops
        .iter()
        .map(|(term, log_idx, op)| ((term.as_u64(), log_idx.as_u64()), (*log_idx, op.clone())))
        .collect();

    sorted.sort_by_key(|(k, _)| *k);

    sorted.into_iter().map(|(_, v)| v).collect()
}

/// Computes final file size after all write operations.
pub fn compute_final_size(writes: &[(u64, &[u8])]) -> u64 {
    let mut max_end = 0u64;

    for (offset, data) in writes {
        let end = *offset + data.len() as u64;
        if end > max_end {
            max_end = end;
        }
    }

    max_end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_linearizability_empty() {
        let ctx = ConcurrentOpContext {
            inode_id: InodeId::new(1),
            operations: vec![],
            expected_final_state: InodeAttr::new_file(InodeId::new(1), 0, 0, 0o644, 1),
            raft_order: vec![],
        };
        let result = verify_linearizability(&[ctx]);
        assert!(matches!(result, LinearizabilityResult::Valid { .. }));
    }

    #[test]
    fn test_simulate_concurrent_writes() {
        let ops = vec![
            (1u64, InodeOp::Write {
                offset: 0,
                data: vec![0; 100]
            }),
            (2u64, InodeOp::Write {
                offset: 100,
                data: vec![0; 100]
            }),
        ];

        let attr = simulate_concurrent_writes(&ops);
        assert_eq!(attr.size, 200);
    }

    #[test]
    fn test_check_raft_order_consistency() {
        let ops = vec![
            (LogIndex::new(1), InodeOp::Truncate { size: 1 }),
            (LogIndex::new(2), InodeOp::Truncate { size: 2 }),
            (LogIndex::new(3), InodeOp::Truncate { size: 3 }),
        ];
        assert!(check_raft_order_consistency(&ops));
    }

    #[test]
    fn test_detect_write_skew_overlapping() {
        let ops = vec![
            (1u64, InodeOp::Write {
                offset: 10,
                data: vec![0; 20]
            }),
            (2u64, InodeOp::Write {
                offset: 10,
                data: vec![0; 20]
            }),
        ];

        let result = detect_write_skew(&ops);
        assert!(matches!(result, Some(Violation::WriteSkew)));
    }

    #[test]
    fn test_apply_operation_write() {
        let mut attr = InodeAttr::new_file(InodeId::new(1), 0, 0, 0o644, 1);
        apply_operation(&mut attr, &InodeOp::Write {
            offset: 0,
            data: vec![0; 512]
        });
        assert_eq!(attr.size, 512);
    }

    #[test]
    fn test_apply_operation_chmod() {
        let mut attr = InodeAttr::new_file(InodeId::new(1), 0, 0, 0o644, 1);
        apply_operation(&mut attr, &InodeOp::Chmod { mode: 0o755 });
        assert_eq!(attr.mode, 0o755);
    }

    #[test]
    fn test_apply_operation_chown() {
        let mut attr = InodeAttr::new_file(InodeId::new(1), 0, 0, 0o644, 1);
        apply_operation(&mut attr, &InodeOp::Chown { uid: 1000, gid: 1000 });
        assert_eq!(attr.uid, 1000);
        assert_eq!(attr.gid, 1000);
    }

    #[test]
    fn test_apply_operation_truncate() {
        let mut attr = InodeAttr::new_file(InodeId::new(1), 0, 0, 0o644, 1);
        attr.size = 1000;
        apply_operation(&mut attr, &InodeOp::Truncate { size: 500 });
        assert_eq!(attr.size, 500);
    }

    #[test]
    fn test_merge_setattr_changes() {
        let mut attr = InodeAttr::new_file(InodeId::new(1), 0, 0, 0o644, 1);
        let changes = AttrChanges {
            mode: Some(0o755),
            uid: Some(1000),
            gid: Some(1000),
            ..Default::default()
        };
        merge_setattr_changes(&mut attr, &changes);
        assert_eq!(attr.mode, 0o755);
        assert_eq!(attr.uid, 1000);
        assert_eq!(attr.gid, 1000);
    }

    #[test]
    fn test_serialize_operations() {
        let ops = vec![
            (Term::new(2), LogIndex::new(1), InodeOp::Truncate { size: 1 }),
            (Term::new(1), LogIndex::new(2), InodeOp::Truncate { size: 2 }),
        ];
        let result = serialize_operations(&ops);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_compute_final_size() {
        let d1 = vec![0u8; 100];
        let d2 = vec![0u8; 100];
        let writes = vec![
            (0u64, d1.as_slice()),
            (100u64, d2.as_slice()),
        ];
        let size = compute_final_size(&writes);
        assert_eq!(size, 200);
    }
}
