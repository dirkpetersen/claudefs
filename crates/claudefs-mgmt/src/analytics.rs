use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

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

    pub fn query(&self, sql: &str) -> anyhow::Result<Vec<HashMap<String, serde_json::Value>>> {
        tracing::debug!("Query executed (stub): {}", sql);

        // TODO: Implement with DuckDB
        // This would use the duckdb crate to query Parquet files:
        //
        // use duckdb::Connection;
        // let conn = Connection::open(&self.duckdb_path)?;
        //
        // let sql = sql.replace(
        //     "metadata",
        //     &format!("read_parquet('{}/**/*.parquet')", self.index_dir.display())
        // );
        //
        // let mut stmt = conn.prepare(&sql)?;
        // let rows = stmt.query_map([], |row| {
        //     // convert row to HashMap
        // })?;
        // // collect results
        //
        Ok(Vec::new())
    }

    pub fn top_users(&self, limit: usize) -> anyhow::Result<Vec<UserStorageUsage>> {
        tracing::debug!("top_users called with limit {}", limit);
        // TODO: Implement with DuckDB
        // SELECT owner_name, SUM(size_bytes) as total_size_bytes, COUNT(*) as file_count
        // FROM read_parquet('{index_dir}/**/*.parquet')
        // GROUP BY owner_name
        // ORDER BY total_size_bytes DESC
        // LIMIT {limit}
        Ok(Vec::new())
    }

    pub fn top_dirs(&self, depth: usize, limit: usize) -> anyhow::Result<Vec<DirStorageUsage>> {
        tracing::debug!("top_dirs called with depth {} limit {}", depth, limit);
        // TODO: Implement with DuckDB
        // SELECT parent_path as path, SUM(size_bytes) as total_size_bytes, COUNT(*) as file_count
        // FROM read_parquet('{index_dir}/**/*.parquet')
        // WHERE array_length(string_to_array(path, '/')) = {depth + 1}
        // GROUP BY parent_path
        // ORDER BY total_size_bytes DESC
        // LIMIT {limit}
        Ok(Vec::new())
    }

    pub fn find_files(&self, pattern: &str, limit: usize) -> anyhow::Result<Vec<MetadataRecord>> {
        tracing::debug!("find_files called with pattern {} limit {}", pattern, limit);

        let glob_pattern = self.pattern_to_sql_glob(pattern);
        tracing::debug!("Converted to SQL pattern: {}", glob_pattern);

        // TODO: Implement with DuckDB
        // SELECT * FROM read_parquet('{index_dir}/**/*.parquet')
        // WHERE filename LIKE '{glob_pattern}'
        // LIMIT {limit}
        Ok(Vec::new())
    }

    pub fn stale_files(&self, days: u64, limit: usize) -> anyhow::Result<Vec<MetadataRecord>> {
        tracing::debug!("stale_files called with days {} limit {}", days, limit);

        let cutoff = chrono::Utc::now().timestamp() - (days as i64 * 86400);

        // TODO: Implement with DuckDB
        // SELECT * FROM read_parquet('{index_dir}/**/*.parquet')
        // WHERE mtime < {cutoff}
        // ORDER BY mtime ASC
        // LIMIT {limit}
        let _ = cutoff;
        Ok(Vec::new())
    }

    pub fn reduction_report(&self, limit: usize) -> anyhow::Result<Vec<ReductionStats>> {
        tracing::debug!("reduction_report called with limit {}", limit);

        // TODO: Implement with DuckDB
        // SELECT
        //   parent_path as path,
        //   SUM(size_bytes) as total_logical_bytes,
        //   SUM(blocks_stored * 4096) as total_stored_bytes,
        //   SUM(size_bytes) / NULLIF(SUM(blocks_stored * 4096), 0) as reduction_ratio
        // FROM read_parquet('{index_dir}/**/*.parquet')
        // GROUP BY parent_path
        // ORDER BY total_logical_bytes DESC
        // LIMIT {limit}
        Ok(Vec::new())
    }

    fn pattern_to_sql_glob(&self, pattern: &str) -> String {
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

    #[test]
    fn test_top_users_empty_index() {
        let engine = AnalyticsEngine::new(PathBuf::from("/tmp/test"));
        let result = engine.top_users(10).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_pattern_to_sql_glob_asterisk() {
        let engine = AnalyticsEngine::new(PathBuf::from("/tmp"));
        let result = engine.pattern_to_sql_glob("*.txt");
        assert_eq!(result, "%\\.txt");
    }

    #[test]
    fn test_pattern_to_sql_glob_question_mark() {
        let engine = AnalyticsEngine::new(PathBuf::from("/tmp"));
        let result = engine.pattern_to_sql_glob("file?.txt");
        assert_eq!(result, "file_\\.txt");
    }

    #[test]
    fn test_pattern_to_sql_glob_complex() {
        let engine = AnalyticsEngine::new(PathBuf::from("/tmp"));
        let result = engine.pattern_to_sql_glob("backup_*.tar.gz");
        assert_eq!(result, "backup_%\\.tar\\.gz");
    }

    #[test]
    fn test_pattern_to_sql_glob_empty() {
        let engine = AnalyticsEngine::new(PathBuf::from("/tmp"));
        let result = engine.pattern_to_sql_glob("");
        assert_eq!(result, "%");
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
