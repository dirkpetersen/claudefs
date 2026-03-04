//! RPC types for quota enforcement

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

#[async_trait]
pub trait QuotaStorage: Send + Sync {
    async fn persist_quota(&self, subject: &str, bytes: u64) -> Result<(), QuotaError>;
    async fn read_ledger(&self, subject: &str) -> Result<Option<QuotaLedger>, QuotaError>;
    async fn add_to_ledger(&self, subject: &str, bytes: u64) -> Result<(), QuotaError>;
    async fn subtract_from_ledger(&self, subject: &str, bytes: u64) -> Result<(), QuotaError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaLedger {
    pub subject: String,
    pub bytes_used: u64,
    pub last_updated: i64,
}

#[derive(Error, Debug)]
pub enum QuotaError {
    #[error("Quota exceeded for {subject}: used {used}/{limit} bytes")]
    ExceededLimit { subject: String, used: u64, limit: u64 },

    #[error("Quota storage error: {0}")]
    StorageError(String),

    #[error("Invalid quota: {0}")]
    Invalid(String),
}

pub struct MockA2QuotaStorage {
    ledgers: Arc<tokio::sync::RwLock<std::collections::HashMap<String, QuotaLedger>>>,
    limits: Arc<std::collections::HashMap<String, u64>>,
}

impl MockA2QuotaStorage {
    pub fn new() -> Self {
        Self {
            ledgers: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            limits: Arc::new(std::collections::HashMap::new()),
        }
    }
}

impl Default for MockA2QuotaStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl QuotaStorage for MockA2QuotaStorage {
    async fn persist_quota(&self, _subject: &str, _bytes: u64) -> Result<(), QuotaError> {
        Ok(())
    }

    async fn read_ledger(&self, subject: &str) -> Result<Option<QuotaLedger>, QuotaError> {
        Ok(self.ledgers.read().await.get(subject).cloned())
    }

    async fn add_to_ledger(&self, subject: &str, bytes: u64) -> Result<(), QuotaError> {
        let mut ledgers = self.ledgers.write().await;
        let entry = ledgers.entry(subject.to_string()).or_insert(QuotaLedger {
            subject: subject.to_string(),
            bytes_used: 0,
            last_updated: chrono::Utc::now().timestamp(),
        });
        entry.bytes_used += bytes;
        entry.last_updated = chrono::Utc::now().timestamp();
        Ok(())
    }

    async fn subtract_from_ledger(&self, subject: &str, bytes: u64) -> Result<(), QuotaError> {
        let mut ledgers = self.ledgers.write().await;
        if let Some(entry) = ledgers.get_mut(subject) {
            entry.bytes_used = entry.bytes_used.saturating_sub(bytes);
            entry.last_updated = chrono::Utc::now().timestamp();
        }
        Ok(())
    }
}