use crate::analytics::MetadataRecord;
use claudefs_meta::journal::MetadataJournal;
use claudefs_meta::journal_tailer::{JournalTailer, TailerConfig, TailerCursor};
use claudefs_meta::types::{InodeId, MetaOp};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

pub struct MetadataConsumer {
    tailer: Arc<RwLock<JournalTailer>>,
    inode_cache: Arc<RwLock<HashMap<InodeId, MetadataRecord>>>,
}

impl MetadataConsumer {
    pub async fn new(journal: Arc<MetadataJournal>) -> anyhow::Result<Self> {
        let config = TailerConfig {
            consumer_id: "mgmt-analytics".to_string(),
            batch_size: 1000,
            enable_compaction: false,
        };

        let tailer = JournalTailer::new(journal, config);

        Ok(Self {
            tailer: Arc::new(RwLock::new(tailer)),
            inode_cache: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub async fn poll_batch(&self) -> anyhow::Result<Vec<MetadataRecord>> {
        let mut tailer = self.tailer.write().await;
        
        let batch = tailer.poll_batch().map_err(|e| anyhow::anyhow!("Journal error: {}", e))?;

        match batch {
            None => Ok(Vec::new()),
            Some(batch) => {
                let mut records = Vec::with_capacity(batch.entries.len());
                let mut cache = self.inode_cache.write().await;

                for entry in batch.entries {
                    match &entry.op {
                        MetaOp::CreateInode { attr } => {
                            let record = MetadataRecord {
                                inode: attr.ino.as_u64(),
                                path: format!("/{}", attr.ino.as_u64()),
                                filename: attr.symlink_target.clone().unwrap_or_default(),
                                parent_path: "/".to_string(),
                                owner_uid: attr.uid,
                                owner_name: format!("uid_{}", attr.uid),
                                group_gid: attr.gid,
                                group_name: format!("gid_{}", attr.gid),
                                size_bytes: attr.size,
                                blocks_stored: (attr.size + 4095) / 4096,
                                mtime: attr.mtime.secs as i64,
                                ctime: attr.ctime.secs as i64,
                                file_type: match attr.file_type {
                                    claudefs_meta::types::FileType::RegularFile => "file",
                                    claudefs_meta::types::FileType::Directory => "dir",
                                    claudefs_meta::types::FileType::Symlink => "symlink",
                                    claudefs_meta::types::FileType::BlockDevice => "block",
                                    claudefs_meta::types::FileType::CharDevice => "char",
                                    claudefs_meta::types::FileType::Fifo => "fifo",
                                    claudefs_meta::types::FileType::Socket => "socket",
                                }.to_string(),
                                is_replicated: false,
                            };
                            cache.insert(attr.ino, record.clone());
                            records.push(record);
                        }
                        MetaOp::SetAttr { ino, attr } => {
                            if let Some(record) = cache.get_mut(ino) {
                                record.size_bytes = attr.size;
                                record.owner_uid = attr.uid;
                                record.mtime = attr.mtime.secs as i64;
                                record.ctime = attr.ctime.secs as i64;
                                record.blocks_stored = (attr.size + 4095) / 4096;
                                records.push(record.clone());
                            }
                        }
                        MetaOp::DeleteInode { ino } => {
                            cache.remove(ino);
                        }
                        MetaOp::CreateEntry { parent, name: _, entry: _ } => {
                            if let Some(parent_record) = cache.get_mut(parent) {
                                parent_record.mtime = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap()
                                    .as_secs() as i64;
                                records.push(parent_record.clone());
                            }
                        }
                        MetaOp::DeleteEntry { parent, name: _ } => {
                            if let Some(parent_record) = cache.get_mut(parent) {
                                parent_record.mtime = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap()
                                    .as_secs() as i64;
                                records.push(parent_record.clone());
                            }
                        }
                        MetaOp::Rename { src_parent: _, src_name: _, dst_parent, dst_name: _ } => {
                            if let Some(record) = cache.get_mut(dst_parent) {
                                record.mtime = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap()
                                    .as_secs() as i64;
                                records.push(record.clone());
                            }
                        }
                        MetaOp::Link { parent, name, ino } => {
                            let target_record = cache.get(ino).cloned();
                            if let Some(parent_record) = cache.get_mut(parent) {
                                parent_record.mtime = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap()
                                    .as_secs() as i64;
                            }
                            if let Some(target) = target_record {
                                let mut updated = target;
                                updated.parent_path = format!("/{}", parent.as_u64());
                                updated.filename = name.to_string();
                                records.push(updated);
                            }
                        }
                        _ => {
                            debug!("Ignoring journal op: {:?}", entry.op);
                        }
                    }
                }

                Ok(records)
            }
        }
    }

    pub async fn cursor(&self) -> anyhow::Result<TailerCursor> {
        let tailer = self.tailer.read().await;
        Ok(tailer.cursor().clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_consumer_empty_journal() {
        let journal = Arc::new(MetadataJournal::new(1, 10000));
        let consumer = MetadataConsumer::new(journal).await.unwrap();
        
        let records = consumer.poll_batch().await.unwrap();
        assert!(records.is_empty());
    }

    #[tokio::test]
    async fn test_consumer_tracks_inode_cache() {
        use claudefs_meta::types::InodeAttr;

        let journal = Arc::new(MetadataJournal::new(1, 10000));
        
        let attr = InodeAttr::new_file(InodeId::new(1), 1000, 1000, 0o644, 1);
        journal.append(MetaOp::CreateInode { attr }, claudefs_meta::types::LogIndex::new(1)).unwrap();
        
        let consumer = MetadataConsumer::new(journal).await.unwrap();
        
        let records = consumer.poll_batch().await.unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].inode, 1);
        
        let cursor = consumer.cursor().await.unwrap();
        assert_eq!(cursor.last_consumed, 1);
    }

    #[tokio::test]
    async fn test_consumer_setattr_updates_cache() {
        use claudefs_meta::types::InodeAttr;

        let journal = Arc::new(MetadataJournal::new(1, 10000));
        
        let attr = InodeAttr::new_file(InodeId::new(1), 1000, 1000, 0o644, 1);
        journal.append(MetaOp::CreateInode { attr }, claudefs_meta::types::LogIndex::new(1)).unwrap();
        
        let consumer = MetadataConsumer::new(journal.clone()).await.unwrap();
        consumer.poll_batch().await.unwrap();
        
        let mut new_attr = InodeAttr::new_file(InodeId::new(1), 1000, 1000, 0o644, 1);
        new_attr.size = 8192;
        journal.append(MetaOp::SetAttr { ino: InodeId::new(1), attr: new_attr }, claudefs_meta::types::LogIndex::new(2)).unwrap();
        
        let records = consumer.poll_batch().await.unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].size_bytes, 8192);
    }

    #[tokio::test]
    async fn test_consumer_delete_removes_from_cache() {
        use claudefs_meta::types::InodeAttr;

        let journal = Arc::new(MetadataJournal::new(1, 10000));
        
        let attr = InodeAttr::new_file(InodeId::new(1), 1000, 1000, 0o644, 1);
        journal.append(MetaOp::CreateInode { attr }, claudefs_meta::types::LogIndex::new(1)).unwrap();
        
        let consumer = MetadataConsumer::new(journal.clone()).await.unwrap();
        consumer.poll_batch().await.unwrap();
        
        journal.append(MetaOp::DeleteInode { ino: InodeId::new(1) }, claudefs_meta::types::LogIndex::new(2)).unwrap();
        
        let _ = consumer.poll_batch().await.unwrap();
        
        let cache = consumer.inode_cache.read().await;
        assert!(cache.is_empty());
    }
}