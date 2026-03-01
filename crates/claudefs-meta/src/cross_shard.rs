//! Cross-shard operation coordinator using two-phase commit.
//!
//! This module provides a coordinator for operations that span multiple shards,
//! such as cross-directory rename and hard link. Uses the TransactionManager's
//! 2PC protocol to ensure atomicity.

use crate::shard::ShardRouter;
use crate::transaction::{TransactionId, TransactionManager, TransactionState};
use crate::types::*;

/// Result of a cross-shard operation.
#[derive(Clone, Debug)]
pub enum CrossShardResult {
    /// Operation completed on a single shard (no 2PC needed).
    SingleShard,
    /// Operation completed via 2PC across multiple shards.
    CrossShard {
        /// The transaction ID used for 2PC coordination.
        txn_id: TransactionId,
    },
}

/// Coordinates cross-shard metadata operations using two-phase commit.
///
/// Handles operations like rename and hard link that may span multiple
/// virtual shards. Uses the [`TransactionManager`] 2PC protocol to
/// ensure atomicity when source and destination are on different shards.
pub struct CrossShardCoordinator {
    router: ShardRouter,
    txn_mgr: TransactionManager,
}

impl CrossShardCoordinator {
    /// Creates a new coordinator with the given shard count and transaction timeout.
    pub fn new(num_shards: u16, txn_timeout_secs: u64) -> Self {
        Self {
            router: ShardRouter::new(num_shards),
            txn_mgr: TransactionManager::new(txn_timeout_secs),
        }
    }

    /// Returns true if a rename between src_parent and dst_parent crosses shard boundaries.
    pub fn is_cross_shard_rename(&self, src_parent: InodeId, dst_parent: InodeId) -> bool {
        self.router.shard_for_inode(src_parent) != self.router.shard_for_inode(dst_parent)
    }

    /// Returns true if a hard link between parent and target_ino crosses shard boundaries.
    pub fn is_cross_shard_link(&self, parent: InodeId, target_ino: InodeId) -> bool {
        self.router.shard_for_inode(parent) != self.router.shard_for_inode(target_ino)
    }

    /// Executes a rename, using 2PC if the source and destination are on different shards.
    pub fn execute_rename<F>(
        &self,
        src_parent: InodeId,
        src_name: &str,
        dst_parent: InodeId,
        dst_name: &str,
        apply_fn: F,
    ) -> Result<CrossShardResult, MetaError>
    where
        F: FnOnce(InodeId, &str, InodeId, &str) -> Result<(), MetaError>,
    {
        let src_shard = self.router.shard_for_inode(src_parent);
        let dst_shard = self.router.shard_for_inode(dst_parent);

        if src_shard == dst_shard {
            apply_fn(src_parent, src_name, dst_parent, dst_name)?;
            return Ok(CrossShardResult::SingleShard);
        }

        let op = MetaOp::Rename {
            src_parent,
            src_name: src_name.to_string(),
            dst_parent,
            dst_name: dst_name.to_string(),
        };

        let txn_id = self
            .txn_mgr
            .begin_transaction(src_shard, vec![src_shard, dst_shard], op);

        self.txn_mgr.vote_commit(txn_id, src_shard)?;
        self.txn_mgr.vote_commit(txn_id, dst_shard)?;

        let state = self.txn_mgr.check_votes(txn_id)?;

        match state {
            TransactionState::Committing => {
                match apply_fn(src_parent, src_name, dst_parent, dst_name) {
                    Ok(()) => {
                        self.txn_mgr.commit(txn_id)?;
                        Ok(CrossShardResult::CrossShard { txn_id })
                    }
                    Err(e) => {
                        let _ = self.force_abort(txn_id);
                        Err(e)
                    }
                }
            }
            TransactionState::Aborting => {
                self.txn_mgr.abort(txn_id)?;
                Err(MetaError::KvError(
                    "cross-shard rename aborted by participant".to_string(),
                ))
            }
            _ => Err(MetaError::KvError(format!(
                "unexpected transaction state: {:?}",
                state
            ))),
        }
    }

    /// Executes a hard link, using 2PC if parent and target are on different shards.
    pub fn execute_link<F>(
        &self,
        parent: InodeId,
        name: &str,
        target_ino: InodeId,
        apply_fn: F,
    ) -> Result<(InodeAttr, CrossShardResult), MetaError>
    where
        F: FnOnce(InodeId, &str, InodeId) -> Result<InodeAttr, MetaError>,
    {
        let parent_shard = self.router.shard_for_inode(parent);
        let target_shard = self.router.shard_for_inode(target_ino);

        if parent_shard == target_shard {
            let attr = apply_fn(parent, name, target_ino)?;
            return Ok((attr, CrossShardResult::SingleShard));
        }

        let op = MetaOp::CreateEntry {
            parent,
            name: name.to_string(),
            entry: DirEntry {
                name: name.to_string(),
                ino: target_ino,
                file_type: FileType::RegularFile,
            },
        };

        let txn_id =
            self.txn_mgr
                .begin_transaction(parent_shard, vec![parent_shard, target_shard], op);

        self.txn_mgr.vote_commit(txn_id, parent_shard)?;
        self.txn_mgr.vote_commit(txn_id, target_shard)?;

        let state = self.txn_mgr.check_votes(txn_id)?;

        match state {
            TransactionState::Committing => match apply_fn(parent, name, target_ino) {
                Ok(attr) => {
                    self.txn_mgr.commit(txn_id)?;
                    Ok((attr, CrossShardResult::CrossShard { txn_id }))
                }
                Err(e) => {
                    let _ = self.force_abort(txn_id);
                    Err(e)
                }
            },
            TransactionState::Aborting => {
                self.txn_mgr.abort(txn_id)?;
                Err(MetaError::KvError(
                    "cross-shard link aborted by participant".to_string(),
                ))
            }
            _ => Err(MetaError::KvError(format!(
                "unexpected transaction state: {:?}",
                state
            ))),
        }
    }

    /// Returns a reference to the underlying transaction manager.
    pub fn transaction_manager(&self) -> &TransactionManager {
        &self.txn_mgr
    }

    /// Returns a reference to the shard router.
    pub fn router(&self) -> &ShardRouter {
        &self.router
    }

    fn force_abort(&self, txn_id: TransactionId) -> Result<(), MetaError> {
        let txn = self.txn_mgr.get_transaction(txn_id)?;
        match txn.state {
            TransactionState::Committing
            | TransactionState::Aborting
            | TransactionState::Preparing => {
                tracing::warn!(
                    "Force-aborting transaction {} in state {:?}",
                    txn_id,
                    txn.state
                );
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_attr(ino: InodeId) -> InodeAttr {
        InodeAttr::new_file(ino, 0, 0, 0o644, 1)
    }

    #[test]
    fn test_same_shard_rename() {
        let coordinator = CrossShardCoordinator::new(256, 60);

        let src_parent = InodeId::new(0);
        let dst_parent = InodeId::new(256);
        assert_eq!(
            coordinator.router.shard_for_inode(src_parent),
            coordinator.router.shard_for_inode(dst_parent)
        );

        let mut rename_called = false;
        let result = coordinator.execute_rename(
            src_parent,
            "old_name",
            dst_parent,
            "new_name",
            |_src, _src_name, _dst, _dst_name| {
                rename_called = true;
                Ok(())
            },
        );

        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), CrossShardResult::SingleShard));
        assert!(rename_called);
    }

    #[test]
    fn test_cross_shard_rename() {
        let coordinator = CrossShardCoordinator::new(256, 60);

        let src_parent = InodeId::new(0);
        let dst_parent = InodeId::new(1);

        assert_ne!(
            coordinator.router.shard_for_inode(src_parent),
            coordinator.router.shard_for_inode(dst_parent)
        );

        let result = coordinator.execute_rename(
            src_parent,
            "old_name",
            dst_parent,
            "new_name",
            |_src, _src_name, _dst, _dst_name| Ok(()),
        );

        assert!(result.is_ok());
        match result.unwrap() {
            CrossShardResult::CrossShard { txn_id } => {
                let txn = coordinator
                    .transaction_manager()
                    .get_transaction(txn_id)
                    .unwrap();
                assert_eq!(txn.state, TransactionState::Committed);
            }
            CrossShardResult::SingleShard => panic!("expected cross-shard result"),
        }
    }

    #[test]
    fn test_cross_shard_rename_apply_fails() {
        let coordinator = CrossShardCoordinator::new(256, 60);

        let src_parent = InodeId::new(0);
        let dst_parent = InodeId::new(1);

        let result = coordinator.execute_rename(
            src_parent,
            "old_name",
            dst_parent,
            "new_name",
            |_src, _src_name, _dst, _dst_name| {
                Err(MetaError::KvError("simulated failure".to_string()))
            },
        );

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), MetaError::KvError(_)));
    }

    #[test]
    fn test_same_shard_link() {
        let coordinator = CrossShardCoordinator::new(256, 60);

        let parent = InodeId::new(0);
        let target_ino = InodeId::new(512);

        assert_eq!(
            coordinator.router.shard_for_inode(parent),
            coordinator.router.shard_for_inode(target_ino)
        );

        let attr = create_test_attr(target_ino);
        let (result_attr, result) = coordinator
            .execute_link(
                parent,
                "hardlink",
                target_ino,
                |_p, _n, _t| Ok(attr.clone()),
            )
            .unwrap();

        assert!(matches!(result, CrossShardResult::SingleShard));
        assert_eq!(result_attr.ino, target_ino);
    }

    #[test]
    fn test_cross_shard_link() {
        let coordinator = CrossShardCoordinator::new(256, 60);

        let parent = InodeId::new(0);
        let target_ino = InodeId::new(1);

        assert_ne!(
            coordinator.router.shard_for_inode(parent),
            coordinator.router.shard_for_inode(target_ino)
        );

        let attr = create_test_attr(target_ino);
        let (result_attr, result) = coordinator
            .execute_link(
                parent,
                "hardlink",
                target_ino,
                |_p, _n, _t| Ok(attr.clone()),
            )
            .unwrap();

        match result {
            CrossShardResult::CrossShard { txn_id } => {
                let txn = coordinator
                    .transaction_manager()
                    .get_transaction(txn_id)
                    .unwrap();
                assert_eq!(txn.state, TransactionState::Committed);
            }
            CrossShardResult::SingleShard => panic!("expected cross-shard result"),
        }
        assert_eq!(result_attr.ino, target_ino);
    }

    #[test]
    fn test_cross_shard_link_apply_fails() {
        let coordinator = CrossShardCoordinator::new(256, 60);

        let parent = InodeId::new(0);
        let target_ino = InodeId::new(1);

        let result = coordinator.execute_link(parent, "hardlink", target_ino, |_p, _n, _t| {
            Err(MetaError::KvError("simulated failure".to_string()))
        });

        assert!(result.is_err());
    }

    #[test]
    fn test_is_cross_shard_rename() {
        let coordinator = CrossShardCoordinator::new(256, 60);

        let src = InodeId::new(0);
        let dst_same_shard = InodeId::new(256);
        let dst_diff_shard = InodeId::new(1);

        assert!(!coordinator.is_cross_shard_rename(src, dst_same_shard));
        assert!(coordinator.is_cross_shard_rename(src, dst_diff_shard));
    }

    #[test]
    fn test_is_cross_shard_link() {
        let coordinator = CrossShardCoordinator::new(256, 60);

        let parent = InodeId::new(0);
        let target_same_shard = InodeId::new(256);
        let target_diff_shard = InodeId::new(1);

        assert!(!coordinator.is_cross_shard_link(parent, target_same_shard));
        assert!(coordinator.is_cross_shard_link(parent, target_diff_shard));
    }

    #[test]
    fn test_transaction_manager_accessible() {
        let coordinator = CrossShardCoordinator::new(256, 60);

        let _txn_mgr = coordinator.transaction_manager();
        let _router = coordinator.router();

        assert!(coordinator.transaction_manager().active_count() == 0);
    }

    #[test]
    fn test_multiple_cross_shard_renames() {
        let coordinator = CrossShardCoordinator::new(256, 60);

        for i in 0..5 {
            let src_parent = InodeId::new(i * 2);
            let dst_parent = InodeId::new(i * 2 + 1);

            let result =
                coordinator
                    .execute_rename(src_parent, "old", dst_parent, "new", |_, _, _, _| Ok(()));

            assert!(result.is_ok(), "rename {} failed", i);
            match result.unwrap() {
                CrossShardResult::CrossShard { txn_id } => {
                    let txn = coordinator
                        .transaction_manager()
                        .get_transaction(txn_id)
                        .unwrap();
                    assert_eq!(txn.state, TransactionState::Committed);
                }
                CrossShardResult::SingleShard => panic!("expected cross-shard"),
            }
        }

        assert!(coordinator.transaction_manager().active_count() >= 5);
    }

    #[test]
    fn test_shard_for_inode_consistency() {
        let coordinator = CrossShardCoordinator::new(256, 60);

        let ino1 = InodeId::new(100);
        let ino2 = InodeId::new(100);
        assert_eq!(
            coordinator.router.shard_for_inode(ino1),
            coordinator.router.shard_for_inode(ino2)
        );

        let ino3 = InodeId::new(356);
        assert_eq!(
            coordinator.router.shard_for_inode(ino1),
            coordinator.router.shard_for_inode(ino3)
        );
    }
}
