use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum ReplayAction {
    WriteChunk {
        inode_id: u64,
        offset: u64,
        hash: [u8; 32],
        size: u32,
    },
    DeleteInode {
        inode_id: u64,
    },
    TruncateInode {
        inode_id: u64,
        new_size: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayConfig {
    pub max_entries_per_batch: usize,
    pub verify_hashes: bool,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        Self {
            max_entries_per_batch: 1000,
            verify_hashes: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ReplayStats {
    pub entries_replayed: u64,
    pub chunks_written: u64,
    pub inodes_deleted: u64,
    pub inodes_truncated: u64,
    pub errors: u64,
}

#[derive(Debug, Clone)]
pub struct InodeReplayState {
    pub inode_id: u64,
    pub chunks: Vec<(u64, [u8; 32])>,
    pub deleted: bool,
    pub final_size: Option<u64>,
}

#[derive(Debug)]
pub struct ReplayState {
    pub inode_states: HashMap<u64, InodeReplayState>,
}

impl Default for ReplayState {
    fn default() -> Self {
        Self {
            inode_states: HashMap::new(),
        }
    }
}

pub struct JournalReplayer {
    config: ReplayConfig,
}

impl JournalReplayer {
    pub fn new(config: ReplayConfig) -> Self {
        Self { config }
    }

    pub fn apply(&mut self, state: &mut ReplayState, action: ReplayAction) {
        match action {
            ReplayAction::WriteChunk {
                inode_id,
                offset,
                hash,
                size: _,
            } => {
                let inode_state = state
                    .inode_states
                    .entry(inode_id)
                    .or_insert(InodeReplayState {
                        inode_id,
                        chunks: Vec::new(),
                        deleted: false,
                        final_size: None,
                    });
                let already_exists = inode_state.chunks.iter().any(|(off, _)| *off == offset);
                if !already_exists {
                    inode_state.chunks.push((offset, hash));
                }
            }
            ReplayAction::DeleteInode { inode_id } => {
                if let Some(inode_state) = state.inode_states.get_mut(&inode_id) {
                    inode_state.deleted = true;
                }
            }
            ReplayAction::TruncateInode { inode_id, new_size } => {
                let inode_state = state
                    .inode_states
                    .entry(inode_id)
                    .or_insert(InodeReplayState {
                        inode_id,
                        chunks: Vec::new(),
                        deleted: false,
                        final_size: None,
                    });
                inode_state.final_size = Some(new_size);
            }
        }
    }

    pub fn replay_batch(
        &mut self,
        state: &mut ReplayState,
        actions: &[ReplayAction],
    ) -> ReplayStats {
        let mut stats = ReplayStats::default();
        stats.entries_replayed = actions.len() as u64;

        for action in actions {
            match action {
                ReplayAction::WriteChunk { .. } => {
                    stats.chunks_written += 1;
                }
                ReplayAction::DeleteInode { .. } => {
                    stats.inodes_deleted += 1;
                }
                ReplayAction::TruncateInode { .. } => {
                    stats.inodes_truncated += 1;
                }
            }
            self.apply(state, action.clone());
        }

        stats
    }

    pub fn finalize(&self, state: &ReplayState) -> Vec<InodeReplayState> {
        state
            .inode_states
            .values()
            .filter(|s| !s.deleted)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replayer_config_default() {
        let config = ReplayConfig::default();
        assert_eq!(config.max_entries_per_batch, 1000);
        assert_eq!(config.verify_hashes, true);
    }

    #[test]
    fn replay_stats_default() {
        let stats = ReplayStats::default();
        assert_eq!(stats.entries_replayed, 0);
        assert_eq!(stats.chunks_written, 0);
        assert_eq!(stats.inodes_deleted, 0);
        assert_eq!(stats.inodes_truncated, 0);
        assert_eq!(stats.errors, 0);
    }

    #[test]
    fn apply_write_chunk() {
        let mut replayer = JournalReplayer::new(ReplayConfig::default());
        let mut state = ReplayState::default();

        replayer.apply(
            &mut state,
            ReplayAction::WriteChunk {
                inode_id: 1,
                offset: 0,
                hash: [0xAB; 32],
                size: 4096,
            },
        );

        let inode_state = state.inode_states.get(&1).expect("inode should exist");
        assert_eq!(inode_state.chunks.len(), 1);
        assert_eq!(inode_state.chunks[0].0, 0);
        assert_eq!(inode_state.chunks[0].1, [0xAB; 32]);
    }

    #[test]
    fn apply_delete_inode() {
        let mut replayer = JournalReplayer::new(ReplayConfig::default());
        let mut state = ReplayState::default();

        state.inode_states.insert(
            1,
            InodeReplayState {
                inode_id: 1,
                chunks: vec![],
                deleted: false,
                final_size: None,
            },
        );

        replayer.apply(&mut state, ReplayAction::DeleteInode { inode_id: 1 });

        let inode_state = state.inode_states.get(&1).expect("inode should exist");
        assert_eq!(inode_state.deleted, true);
    }

    #[test]
    fn apply_truncate_inode() {
        let mut replayer = JournalReplayer::new(ReplayConfig::default());
        let mut state = ReplayState::default();

        replayer.apply(
            &mut state,
            ReplayAction::TruncateInode {
                inode_id: 1,
                new_size: 8192,
            },
        );

        let inode_state = state.inode_states.get(&1).expect("inode should exist");
        assert_eq!(inode_state.final_size, Some(8192));
    }

    #[test]
    fn replay_batch_empty() {
        let mut replayer = JournalReplayer::new(ReplayConfig::default());
        let mut state = ReplayState::default();

        let stats = replayer.replay_batch(&mut state, &[]);

        assert_eq!(stats.entries_replayed, 0);
        assert_eq!(stats.chunks_written, 0);
    }

    #[test]
    fn replay_batch_multiple_writes() {
        let mut replayer = JournalReplayer::new(ReplayConfig::default());
        let mut state = ReplayState::default();

        let actions = vec![
            ReplayAction::WriteChunk {
                inode_id: 1,
                offset: 0,
                hash: [1; 32],
                size: 4096,
            },
            ReplayAction::WriteChunk {
                inode_id: 1,
                offset: 4096,
                hash: [2; 32],
                size: 4096,
            },
            ReplayAction::WriteChunk {
                inode_id: 2,
                offset: 0,
                hash: [3; 32],
                size: 8192,
            },
        ];

        replayer.replay_batch(&mut state, &actions);

        let inode1 = state.inode_states.get(&1).expect("inode 1 should exist");
        assert_eq!(inode1.chunks.len(), 2);
        let inode2 = state.inode_states.get(&2).expect("inode 2 should exist");
        assert_eq!(inode2.chunks.len(), 1);
    }

    #[test]
    fn replay_batch_stats_chunks_written() {
        let mut replayer = JournalReplayer::new(ReplayConfig::default());
        let mut state = ReplayState::default();

        let actions = vec![
            ReplayAction::WriteChunk {
                inode_id: 1,
                offset: 0,
                hash: [1; 32],
                size: 4096,
            },
            ReplayAction::WriteChunk {
                inode_id: 1,
                offset: 4096,
                hash: [2; 32],
                size: 4096,
            },
        ];

        let stats = replayer.replay_batch(&mut state, &actions);

        assert_eq!(stats.chunks_written, 2);
    }

    #[test]
    fn replay_batch_stats_inodes_deleted() {
        let mut replayer = JournalReplayer::new(ReplayConfig::default());
        let mut state = ReplayState::default();

        let actions = vec![
            ReplayAction::DeleteInode { inode_id: 1 },
            ReplayAction::DeleteInode { inode_id: 2 },
        ];

        let stats = replayer.replay_batch(&mut state, &actions);

        assert_eq!(stats.inodes_deleted, 2);
    }

    #[test]
    fn replay_batch_stats_inodes_truncated() {
        let mut replayer = JournalReplayer::new(ReplayConfig::default());
        let mut state = ReplayState::default();

        let actions = vec![
            ReplayAction::TruncateInode {
                inode_id: 1,
                new_size: 4096,
            },
            ReplayAction::TruncateInode {
                inode_id: 2,
                new_size: 8192,
            },
        ];

        let stats = replayer.replay_batch(&mut state, &actions);

        assert_eq!(stats.inodes_truncated, 2);
    }

    #[test]
    fn finalize_excludes_deleted() {
        let mut replayer = JournalReplayer::new(ReplayConfig::default());
        let mut state = ReplayState::default();

        state.inode_states.insert(
            1,
            InodeReplayState {
                inode_id: 1,
                chunks: vec![],
                deleted: true,
                final_size: None,
            },
        );

        let result = replayer.finalize(&state);

        assert!(result.is_empty());
    }

    #[test]
    fn finalize_includes_alive() {
        let mut replayer = JournalReplayer::new(ReplayConfig::default());
        let mut state = ReplayState::default();

        state.inode_states.insert(
            1,
            InodeReplayState {
                inode_id: 1,
                chunks: vec![(0, [1; 32])],
                deleted: false,
                final_size: Some(4096),
            },
        );

        let result = replayer.finalize(&state);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].inode_id, 1);
    }

    #[test]
    fn inode_replay_state_chunks() {
        let state = InodeReplayState {
            inode_id: 42,
            chunks: vec![(0, [1; 32]), (4096, [2; 32])],
            deleted: false,
            final_size: Some(8192),
        };

        assert_eq!(state.chunks.len(), 2);
        assert_eq!(state.chunks[0].0, 0);
        assert_eq!(state.chunks[1].0, 4096);
    }

    #[test]
    fn replay_idempotent_hash() {
        let mut replayer = JournalReplayer::new(ReplayConfig::default());
        let mut state = ReplayState::default();

        replayer.apply(
            &mut state,
            ReplayAction::WriteChunk {
                inode_id: 1,
                offset: 0,
                hash: [0xAB; 32],
                size: 4096,
            },
        );

        replayer.apply(
            &mut state,
            ReplayAction::WriteChunk {
                inode_id: 1,
                offset: 0,
                hash: [0xAB; 32],
                size: 4096,
            },
        );

        let inode_state = state.inode_states.get(&1).expect("inode should exist");
        assert_eq!(inode_state.chunks.len(), 1);
    }
}
