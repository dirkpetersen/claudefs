use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;
use tokio::task;

#[derive(Error, Debug)]
pub enum AnalyticsError {
    #[error("DuckDB error: {0}")]
    DuckDbError(String),
    #[error("No Parquet files found in index directory")]
    ParquetNotFound,
    #[error("Query failed: {0}")]
    QueryFailed(String),
    #[error("Failed to parse result: {0}")]
    ParseError(String),
}

impl From<duckdb::Error> for AnalyticsError {
    fn from(err: duckdb::Error) -> Self {
        AnalyticsError::DuckDbError(err.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataRecord {
    pub inode: u64,
    pub path: String,
    pub filename: String,
    pub parent_path: String,
    pub owner_uid: u32,
    pub owner_name: String,
    pub group_gid: u32,
    pub group_name: String,
    pub size_bytes: u64,
    pub blocks_stored: u64,
    pub mtime: i64,
    pub ctime: i64,
    pub file_type: String,
    pub is_replicated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserStorageUsage {
    pub owner_name: String,
    pub total_size_bytes: u64,
    pub file_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirStorageUsage {
    pub path: String,
    pub total_size_bytes: u64,
    pub file_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReductionStats {
    pub path: String,
    pub total_logical_bytes: u64,
    pub total_stored_bytes: u64,
    pub reduction_ratio: f64,
}

pub struct AnalyticsEngine {
    index_dir: PathBuf,
}

impl AnalyticsEngine {
    pub fn new(index_dir: PathBuf) -> Self {
        Self { index_dir }
    }

    pub async fn query(&self, sql: &str) -> anyhow::Result<Vec<HashMap<String, serde_json::Value>>> {
        use std::path::Path;
        
        let index_dir = self.index_dir.clone();
        let sql = sql.to_string();
        
        task::spawn_blocking(move || {
            fn find_parquet_files(dir: &Path) -> Vec<PathBuf> {
                let mut files = Vec::new();
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            files.extend(find_parquet_files(&path));
                        } else if let Some(name) = path.file_name() {
                            let name_str = name.to_string_lossy();
                            if name_str.starts_with("metadata") && name_str.ends_with(".parquet") {
                                files.push(path);
                            }
                        }
                    }
                }
                files
            }
            
            let parquet_files = find_parquet_files(&index_dir);

            if parquet_files.is_empty() {
                return Ok(Vec::new());
            }

            let conn = duckdb::Connection::open_in_memory()?;

            let paths: Vec<String> = parquet_files.iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect();
            let paths_str = paths.iter()
                .map(|p| format!("'{}'", p.replace("'", "''")))
                .collect::<Vec<_>>()
                .join(",");

            let create_sql = format!(
                "CREATE TABLE metadata AS SELECT * FROM read_parquet([{}])",
                paths_str
            );
            conn.execute_batch(&create_sql)?;

            let mut stmt = conn.prepare(&sql)?;
            let column_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();

            let rows = stmt.query_map([], |row| {
                let mut map = HashMap::new();
                for (i, col_name) in column_names.iter().enumerate() {
                    let value = Self::duckdb_value_to_json(&row, i);
                    map.insert(col_name.clone(), value);
                }
                Ok(map)
            })?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row?);
            }

            Ok(results)
        })
        .await
        .map_err(|e| anyhow::anyhow!("Join error: {}", e))?
    }

    pub async fn top_users(&self, limit: usize) -> anyhow::Result<Vec<UserStorageUsage>> {
        let sql = format!(
            "SELECT owner_name, SUM(size_bytes) as total_size_bytes, COUNT(*) as file_count
             FROM metadata
             GROUP BY owner_name
             ORDER BY total_size_bytes DESC
             LIMIT {}", limit);

        let results = self.query(&sql).await?;

        let mut users = Vec::new();
        for row in results {
            users.push(UserStorageUsage {
                owner_name: row.get("owner_name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                total_size_bytes: row.get("total_size_bytes").and_then(|v| v.as_u64()).unwrap_or(0),
                file_count: row.get("file_count").and_then(|v| v.as_u64()).unwrap_or(0),
            });
        }
        Ok(users)
    }

    pub async fn top_dirs(&self, depth: usize, limit: usize) -> anyhow::Result<Vec<DirStorageUsage>> {
        let sql = format!(
            "SELECT parent_path as path, SUM(size_bytes) as total_size_bytes, COUNT(*) as file_count
             FROM metadata
             GROUP BY parent_path
             ORDER BY total_size_bytes DESC
             LIMIT {}", limit);

        let results = self.query(&sql).await?;

        let mut dirs = Vec::new();
        for row in results {
            let path = row.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let extracted = Self::extract_path_at_depth(&path, depth);
            if let Some(final_path) = extracted {
                if let Some(existing) = dirs.iter_mut().find(|d: &&mut DirStorageUsage| d.path == final_path) {
                    existing.total_size_bytes += row.get("total_size_bytes").and_then(|v| v.as_u64()).unwrap_or(0);
                    existing.file_count += row.get("file_count").and_then(|v| v.as_u64()).unwrap_or(0);
                } else {
                    dirs.push(DirStorageUsage {
                        path: final_path,
                        total_size_bytes: row.get("total_size_bytes").and_then(|v| v.as_u64()).unwrap_or(0),
                        file_count: row.get("file_count").and_then(|v| v.as_u64()).unwrap_or(0),
                    });
                }
            }
        }

        dirs.sort_by(|a, b| b.total_size_bytes.cmp(&a.total_size_bytes));
        dirs.truncate(limit);
        Ok(dirs)
    }

    fn extract_path_at_depth(path: &str, depth: usize) -> Option<String> {
        if depth == 0 {
            return Some("/".to_string());
        }
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if depth > parts.len() {
            return None;
        }
        let prefix = parts[..depth].join("/");
        if prefix.is_empty() {
            Some("/".to_string())
        } else {
            Some(format!("/{}", prefix))
        }
    }

    pub async fn reduction_report(&self, limit: usize) -> anyhow::Result<Vec<ReductionStats>> {
        let sql = format!(
            "SELECT
                parent_path as path,
                SUM(size_bytes) as total_logical_bytes,
                SUM(blocks_stored * 4096) as total_stored_bytes
             FROM metadata
             GROUP BY parent_path
             ORDER BY total_logical_bytes DESC
             LIMIT {}", limit);

        let results = self.query(&sql).await?;

        let mut stats = Vec::new();
        for row in results {
            let logical = row.get("total_logical_bytes").and_then(|v| v.as_u64()).unwrap_or(0);
            let stored = row.get("total_stored_bytes").and_then(|v| v.as_u64()).unwrap_or(1);
            let ratio = if stored > 0 { logical as f64 / stored as f64 } else { 0.0 };
            stats.push(ReductionStats {
                path: row.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                total_logical_bytes: logical,
                total_stored_bytes: stored,
                reduction_ratio: ratio,
            });
        }
        Ok(stats)
    }

    #[allow(dead_code)]
    pub async fn reduction_stats(&self) -> anyhow::Result<Vec<ReductionStats>> {
        let sql = "SELECT
            path,
            SUM(size_bytes) as total_logical_bytes,
            SUM(blocks_stored * 4096) as total_stored_bytes
           FROM metadata
           GROUP BY path";

        let results = self.query(sql).await?;

        let mut stats = Vec::new();
        for row in results {
            let logical = row.get("total_logical_bytes").and_then(|v| v.as_u64()).unwrap_or(0);
            let stored = row.get("total_stored_bytes").and_then(|v| v.as_u64()).unwrap_or(1);
            let ratio = if stored > 0 { logical as f64 / stored as f64 } else { 0.0 };
            stats.push(ReductionStats {
                path: row.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                total_logical_bytes: logical,
                total_stored_bytes: stored,
                reduction_ratio: ratio,
            });
        }
        Ok(stats)
    }

    pub async fn find_files(&self, pattern: &str, limit: usize) -> anyhow::Result<Vec<MetadataRecord>> {
        let glob_pattern = Self::pattern_to_sql_glob(pattern);

        let sql = format!(
            "SELECT * FROM metadata WHERE filename LIKE '{}' LIMIT {}",
            glob_pattern, limit
        );

        let results = self.query(&sql).await?;

        let mut records = Vec::new();
        for row in results {
            records.push(MetadataRecord {
                inode: row.get("inode").and_then(|v| v.as_u64()).unwrap_or(0),
                path: row.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                filename: row.get("filename").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                parent_path: row.get("parent_path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                owner_uid: row.get("owner_uid").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                owner_name: row.get("owner_name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                group_gid: row.get("group_gid").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                group_name: row.get("group_name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                size_bytes: row.get("size_bytes").and_then(|v| v.as_u64()).unwrap_or(0),
                blocks_stored: row.get("blocks_stored").and_then(|v| v.as_u64()).unwrap_or(0),
                mtime: row.get("mtime").and_then(|v| v.as_i64()).unwrap_or(0),
                ctime: row.get("ctime").and_then(|v| v.as_i64()).unwrap_or(0),
                file_type: row.get("file_type").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                is_replicated: row.get("is_replicated").and_then(|v| v.as_bool()).unwrap_or(false),
            });
        }
        Ok(records)
    }

    pub async fn stale_files(&self, days: u64, limit: usize) -> anyhow::Result<Vec<MetadataRecord>> {
        let cutoff = chrono::Utc::now().timestamp() - (days as i64 * 86400);

        let sql = format!(
            "SELECT * FROM metadata WHERE mtime < {} ORDER BY mtime ASC LIMIT {}",
            cutoff, limit
        );

        let results = self.query(&sql).await?;

        let mut records = Vec::new();
        for row in results {
            records.push(MetadataRecord {
                inode: row.get("inode").and_then(|v| v.as_u64()).unwrap_or(0),
                path: row.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                filename: row.get("filename").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                parent_path: row.get("parent_path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                owner_uid: row.get("owner_uid").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                owner_name: row.get("owner_name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                group_gid: row.get("group_gid").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                group_name: row.get("group_name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                size_bytes: row.get("size_bytes").and_then(|v| v.as_u64()).unwrap_or(0),
                blocks_stored: row.get("blocks_stored").and_then(|v| v.as_u64()).unwrap_or(0),
                mtime: row.get("mtime").and_then(|v| v.as_i64()).unwrap_or(0),
                ctime: row.get("ctime").and_then(|v| v.as_i64()).unwrap_or(0),
                file_type: row.get("file_type").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                is_replicated: row.get("is_replicated").and_then(|v| v.as_bool()).unwrap_or(false),
            });
        }
        Ok(records)
    }

    fn duckdb_value_to_json(row: &duckdb::Row, idx: usize) -> serde_json::Value {
        if let Ok(v) = row.get::<_, Option<i64>>(idx) {
            return serde_json::json!(v);
        }
        if let Ok(v) = row.get::<_, Option<f64>>(idx) {
            return serde_json::json!(v);
        }
        if let Ok(v) = row.get::<_, Option<String>>(idx) {
            return serde_json::json!(v);
        }
        if let Ok(v) = row.get::<_, Option<bool>>(idx) {
            return serde_json::json!(v);
        }
        serde_json::Value::Null
    }

    fn pattern_to_sql_glob(pattern: &str) -> String {
        let mut result = String::new();
        let mut in_percent = false;

        for ch in pattern.chars() {
            match ch {
                '*' => {
                    if !in_percent {
                        result.push('%');
                        in_percent = true;
                    }
                }
                '?' => {
                    result.push('_');
                    in_percent = false;
                }
                '.' => {
                    result.push_str("\\.");
                    in_percent = false;
                }
                _ => {
                    result.push(ch);
                    in_percent = false;
                }
            }
        }

        if result.is_empty() {
            result.push('%');
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_record_serialization() {
        let record = MetadataRecord {
            inode: 12345,
            path: "/home/user/file.txt".to_string(),
            filename: "file.txt".to_string(),
            parent_path: "/home/user".to_string(),
            owner_uid: 1000,
            owner_name: "user".to_string(),
            group_gid: 1000,
            group_name: "users".to_string(),
            size_bytes: 4096,
            blocks_stored: 1,
            mtime: 1234567890,
            ctime: 1234567890,
            file_type: "regular".to_string(),
            is_replicated: true,
        };

        let json = serde_json::to_string(&record).unwrap();
        let decoded: MetadataRecord = serde_json::from_str(&json).unwrap();

        assert_eq!(record.inode, decoded.inode);
        assert_eq!(record.path, decoded.path);
        assert_eq!(record.filename, decoded.filename);
        assert_eq!(record.owner_name, decoded.owner_name);
        assert_eq!(record.is_replicated, decoded.is_replicated);
    }

    #[test]
    fn test_metadata_record_deserialization() {
        let json = r#"{
            "inode": 12345,
            "path": "/home/user/file.txt",
            "filename": "file.txt",
            "parent_path": "/home/user",
            "owner_uid": 1000,
            "owner_name": "user",
            "group_gid": 1000,
            "group_name": "users",
            "size_bytes": 4096,
            "blocks_stored": 1,
            "mtime": 1234567890,
            "ctime": 1234567890,
            "file_type": "regular",
            "is_replicated": true
        }"#;

        let record: MetadataRecord = serde_json::from_str(json).unwrap();
        assert_eq!(record.inode, 12345);
        assert_eq!(record.filename, "file.txt");
    }

    #[tokio::test]
    async fn test_top_users_empty_index() {
        let engine = AnalyticsEngine::new(PathBuf::from("/tmp/test"));
        let result = engine.top_users(10).await.unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_pattern_to_sql_glob_asterisk() {
        let result = AnalyticsEngine::pattern_to_sql_glob("*.txt");
        assert_eq!(result, "%\\.txt");
    }

    #[test]
    fn test_pattern_to_sql_glob_question_mark() {
        let result = AnalyticsEngine::pattern_to_sql_glob("file?.txt");
        assert_eq!(result, "file_\\.txt");
    }

    #[test]
    fn test_pattern_to_sql_glob_complex() {
        let result = AnalyticsEngine::pattern_to_sql_glob("backup_*.tar.gz");
        assert_eq!(result, "backup_%\\.tar\\.gz");
    }

    #[test]
    fn test_pattern_to_sql_glob_empty() {
        let result = AnalyticsEngine::pattern_to_sql_glob("");
        assert_eq!(result, "%");
    }

    #[test]
    fn test_extract_path_at_depth_zero() {
        let result = AnalyticsEngine::extract_path_at_depth("/home/user/file.txt", 0);
        assert_eq!(result, Some("/".to_string()));
    }

    #[test]
    fn test_extract_path_at_depth_one() {
        let result = AnalyticsEngine::extract_path_at_depth("/home/user/file.txt", 1);
        assert_eq!(result, Some("/home".to_string()));
    }

    #[test]
    fn test_extract_path_at_depth_two() {
        let result = AnalyticsEngine::extract_path_at_depth("/home/user/file.txt", 2);
        assert_eq!(result, Some("/home/user".to_string()));
    }

    #[test]
    fn test_extract_path_at_depth_exceeds() {
        let result = AnalyticsEngine::extract_path_at_depth("/home/user", 5);
        assert_eq!(result, None);
    }

    #[test]
    fn test_user_storage_usage_round_trip() {
        let usage = UserStorageUsage {
            owner_name: "alice".to_string(),
            total_size_bytes: 1_000_000_000,
            file_count: 500,
        };

        let json = serde_json::to_string(&usage).unwrap();
        let decoded: UserStorageUsage = serde_json::from_str(&json).unwrap();

        assert_eq!(usage.owner_name, decoded.owner_name);
        assert_eq!(usage.total_size_bytes, decoded.total_size_bytes);
        assert_eq!(usage.file_count, decoded.file_count);
    }

    #[test]
    fn test_dir_storage_usage_round_trip() {
        let usage = DirStorageUsage {
            path: "/home/data".to_string(),
            total_size_bytes: 10_000_000_000,
            file_count: 1000,
        };

        let json = serde_json::to_string(&usage).unwrap();
        let decoded: DirStorageUsage = serde_json::from_str(&json).unwrap();

        assert_eq!(usage.path, decoded.path);
        assert_eq!(usage.total_size_bytes, decoded.total_size_bytes);
    }

    #[test]
    fn test_reduction_stats_calculation() {
        let stats = ReductionStats {
            path: "/data".to_string(),
            total_logical_bytes: 1000,
            total_stored_bytes: 400,
            reduction_ratio: 2.5,
        };

        let expected_ratio = stats.total_logical_bytes as f64 / stats.total_stored_bytes as f64;
        assert!((stats.reduction_ratio - expected_ratio).abs() < 0.001);
    }

    #[test]
    fn test_reduction_stats_zero_stored_bytes() {
        let stats = ReductionStats {
            path: "/data".to_string(),
            total_logical_bytes: 1000,
            total_stored_bytes: 0,
            reduction_ratio: 0.0,
        };

        let computed_ratio = if stats.total_stored_bytes > 0 {
            stats.total_logical_bytes as f64 / stats.total_stored_bytes as f64
        } else {
            0.0
        };

        assert_eq!(computed_ratio, 0.0);
    }

    #[test]
    fn test_analytics_engine_new() {
        let engine = AnalyticsEngine::new(PathBuf::from("/custom/path"));
        assert_eq!(engine.index_dir, PathBuf::from("/custom/path"));
    }
}