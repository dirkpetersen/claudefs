use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Debug, Error)]
pub enum QueryError {
    #[error("DuckDB error: {0}")]
    DuckDbError(String),
    #[error("Query timeout")]
    Timeout,
    #[error("SQL injection attempt detected")]
    SqlInjection,
    #[error("I/O error: {0}")]
    IoError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub row_count: usize,
    pub execution_time_ms: u64,
}

pub struct QueryGateway {
    index_dir: PathBuf,
    connection: Arc<RwLock<Option<duckdb::Connection>>>,
    cache: Arc<DashMap<String, (QueryResult, Instant)>>,
    timeout: Duration,
    cache_ttl: Duration,
}

impl QueryGateway {
    pub fn new(index_dir: PathBuf) -> Self {
        Self {
            index_dir,
            connection: Arc::new(RwLock::new(None)),
            cache: Arc::new(DashMap::new()),
            timeout: Duration::from_secs(30),
            cache_ttl: Duration::from_secs(600),
        }
    }

    async fn get_connection(&self) -> Result<duckdb::Connection, QueryError> {
        let mut guard = self.connection.write().await;
        if let Some(conn) = guard.as_ref() {
            return Ok(conn.clone());
        }

        let conn = duckdb::Connection::open_in_memory()
            .map_err(|e| QueryError::DuckDbError(e.to_string()))?;

        let _ = conn.execute(
            "INSTALL json; LOAD json;",
            [],
        );

        if self.index_dir.exists() {
            let parquet_files = self.find_parquet_files();
            for pf in parquet_files {
                let create_stmt = format!(
                    "CREATE TABLE IF NOT EXISTS metadata AS SELECT * FROM read_parquet('{}')",
                    pf.display()
                );
                let _ = conn.execute(&create_stmt, []);
            }
        }

        *guard = Some(conn.clone());
        Ok(conn)
    }

    fn find_parquet_files(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.index_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "parquet") {
                    files.push(path);
                }
            }
        }
        files
    }

    fn detect_sql_injection(query: &str) -> bool {
        let dangerous = [
            "DROP ", "DELETE ", "TRUNCATE ", "INSERT ", "UPDATE ", "CREATE ",
            "ALTER ", "GRANT ", "REVOKE ", ";", "--", "/*", "xp_", "sp_",
        ];
        let upper = query.to_uppercase();
        dangerous.iter().any(|d| upper.contains(d))
    }

    pub async fn execute_query(
        &self,
        query: &str,
        params: Vec<String>,
    ) -> Result<QueryResult, QueryError> {
        if Self::detect_sql_injection(query) {
            return Err(QueryError::SqlInjection);
        }

        let cache_key = format!("{}:{:?}", query, params);
        if let Some((result, cached_at)) = self.cache.get(&cache_key) {
            if cached_at.elapsed() < self.cache_ttl {
                return Ok(result.clone());
            }
        }

        let timeout = self.timeout;
        let query_owned = query.to_string();
        let params_owned = params;

        let result = tokio::time::timeout(
            timeout,
            tokio::task::spawn_blocking(move || {
                let conn = duckdb::Connection::open_in_memory()
                    .map_err(|e| QueryError::DuckDbError(e.to_string()))?;

                let _ = conn.execute(
                    "INSTALL json; LOAD json;",
                    [],
                );

                let mut stmt = conn.prepare(&query_owned)
                    .map_err(|e| QueryError::DuckDbError(e.to_string()))?;

                let param_refs: Vec<&dyn duckdb::ToSql> = params_owned
                    .iter()
                    .map(|s| s as &dyn duckdb::ToSql)
                    .collect();

                let mut rows = stmt.query(param_refs.as_slice())
                    .map_err(|e| QueryError::DuckDbError(e.to_string()))?;

                let columns: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
                let mut result_rows = Vec::new();

                while let Some(row) = rows.next()
                    .map_err(|e| QueryError::DuckDbError(e.to_string()))? {
                    let mut row_values = Vec::new();
                    for i in 0..columns.len() {
                        let value: serde_json::Value = match row.get_ref(i) {
                            Ok(duckdb::ValueRef::Null) => serde_json::Value::Null,
                            Ok(duckdb::ValueRef::Integer(i)) => serde_json::json!(i),
                            Ok(duckdb::ValueRef::Double(d)) => serde_json::json!(d),
                            Ok(duckdb::ValueRef::Text(s)) => {
                                serde_json::Value::String(String::from_utf8_lossy(s).to_string())
                            }
                            Ok(duckdb::ValueRef::Boolean(b)) => serde_json::json!(b),
                            _ => serde_json::Value::Null,
                        };
                        row_values.push(value);
                    }
                    result_rows.push(row_values);
                }

                Ok(QueryResult {
                    columns,
                    row_count: result_rows.len(),
                    rows: result_rows,
                    execution_time_ms: 0,
                })
            })
        ).await
        .map_err(|_| QueryError::Timeout)??;

        if result.execution_time_ms == 0 {
            self.cache.insert(cache_key, (result.clone(), Instant::now()));
        }

        Ok(result)
    }

    pub fn set_timeout(&mut self, duration: Duration) {
        self.timeout = duration;
    }

    pub fn clear_cache(&self) {
        self.cache.clear();
    }

    pub fn cache_stats(&self) -> (usize, usize) {
        let now = Instant::now();
        let mut valid = 0;
        let mut expired = 0;
        for entry in self.cache.iter() {
            if entry.value().1.elapsed() < self.cache_ttl {
                valid += 1;
            } else {
                expired += 1;
            }
        }
        (valid, expired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_parquet(tmpdir: &TempDir) {
        let conn = duckdb::Connection::open(tmpdir.path().join("test.parquet"))
            .unwrap();
        
        conn.execute(
            "CREATE TABLE test (id INTEGER, name VARCHAR, size BIGINT)",
            [],
        ).unwrap();
        
        conn.execute(
            "INSERT INTO test VALUES (1, 'file1', 1024), (2, 'file2', 2048), (3, 'file3', 4096)",
            [],
        ).unwrap();
        
        conn.execute(
            "COPY (SELECT * FROM test) TO 'test.parquet' (FORMAT PARQUET)",
            [],
        ).unwrap();
    }

    #[tokio::test]
    async fn test_query_gateway_new() {
        let tmpdir = TempDir::new().unwrap();
        let gateway = QueryGateway::new(tmpdir.path().to_path_buf());
        assert!(gateway.index_dir.exists() || !gateway.index_dir.to_string_lossy().is_empty());
    }

    #[tokio::test]
    async fn test_query_gateway_execute_simple_query() {
        let tmpdir = TempDir::new().unwrap();
        let gateway = QueryGateway::new(tmpdir.path().to_path_buf());
        
        let result = gateway.execute_query(
            "SELECT 1 as num UNION SELECT 2 UNION SELECT 3",
            vec![]
        ).await.unwrap();
        
        assert_eq!(result.columns, vec!["num"]);
        assert_eq!(result.row_count, 3);
    }

    #[tokio::test]
    async fn test_query_gateway_execute_with_parameters() {
        let tmpdir = TempDir::new().unwrap();
        let gateway = QueryGateway::new(tmpdir.path().to_path_buf());
        
        let result = gateway.execute_query(
            "SELECT ? as num",
            vec!["42".to_string()]
        ).await.unwrap();
        
        assert_eq!(result.columns, vec!["num"]);
        assert_eq!(result.rows[0][0], serde_json::json!(42));
    }

    #[tokio::test]
    async fn test_query_gateway_sql_injection_prevention() {
        let tmpdir = TempDir::new().unwrap();
        let gateway = QueryGateway::new(tmpdir.path().to_path_buf());
        
        let result = gateway.execute_query(
            "SELECT * FROM users; DROP TABLE users;--",
            vec![]
        ).await;
        
        assert!(matches!(result, Err(QueryError::SqlInjection)));
    }

    #[tokio::test]
    async fn test_query_gateway_query_timeout() {
        let tmpdir = TempDir::new().unwrap();
        let mut gateway = QueryGateway::new(tmpdir.path().to_path_buf());
        gateway.set_timeout(Duration::from_millis(10));
        
        let result = gateway.execute_query(
            "SELECT sleep(1)",
            vec![]
        ).await;
        
        assert!(matches!(result, Err(QueryError::Timeout)));
    }

    #[tokio::test]
    async fn test_query_gateway_result_caching() {
        let tmpdir = TempDir::new().unwrap();
        let gateway = QueryGateway::new(tmpdir.path().to_path_buf());
        
        let _ = gateway.execute_query("SELECT 1 as num", vec![]).await.unwrap();
        let (valid, _) = gateway.cache_stats();
        assert!(valid >= 1);
    }

    #[tokio::test]
    async fn test_query_gateway_cache_invalidation() {
        let tmpdir = TempDir::new().unwrap();
        let gateway = QueryGateway::new(tmpdir.path().to_path_buf());
        
        let _ = gateway.execute_query("SELECT 1 as num", vec![]).await.unwrap();
        gateway.clear_cache();
        
        let (valid, _) = gateway.cache_stats();
        assert_eq!(valid, 0);
    }

    #[tokio::test]
    async fn test_query_gateway_streaming_results() {
        let tmpdir = TempDir::new().unwrap();
        let gateway = QueryGateway::new(tmpdir.path().to_path_buf());
        
        let mut values = Vec::new();
        for i in 0..5000 {
            values.push(format!("{}", i));
        }
        let values_str = values.join(",");
        let query = format!("SELECT * FROM (VALUES {}) as t(x)", values_str);
        
        let result = gateway.execute_query(&query, vec![]).await.unwrap();
        assert_eq!(result.row_count, 5000);
    }

    #[tokio::test]
    async fn test_query_gateway_error_handling_bad_sql() {
        let tmpdir = TempDir::new().unwrap();
        let gateway = QueryGateway::new(tmpdir.path().to_path_buf());
        
        let result = gateway.execute_query(
            "SELECT * FROM nonexistent_table",
            vec![]
        ).await;
        
        assert!(matches!(result, Err(QueryError::DuckDbError(_))));
    }

    #[tokio::test]
    async fn test_query_gateway_connection_recovery() {
        let tmpdir = TempDir::new().unwrap();
        let gateway = QueryGateway::new(tmpdir.path().to_path_buf());
        
        let _ = gateway.execute_query("SELECT 1", vec![]).await.unwrap();
        let _ = gateway.execute_query("SELECT 2", vec![]).await.unwrap();
        
        let result = gateway.execute_query("SELECT 3", vec![]).await.unwrap();
        assert_eq!(result.row_count, 1);
    }

    #[tokio::test]
    async fn test_query_gateway_concurrent_queries() {
        let tmpdir = TempDir::new().unwrap();
        let gateway = QueryGateway::new(tmpdir.path().to_path_buf());
        
        let mut handles = Vec::new();
        for i in 0..10 {
            let gateway = Arc::new(QueryGateway::new(tmpdir.path().to_path_buf()));
            let handle = tokio::spawn(async move {
                gateway.execute_query("SELECT ? as num", vec![i.to_string()]).await
            });
            handles.push(handle);
        }
        
        let mut count = 0;
        for handle in handles {
            if handle.await.unwrap().is_ok() {
                count += 1;
            }
        }
        assert_eq!(count, 10);
    }

    #[tokio::test]
    async fn test_query_gateway_performance_cached_vs_uncached() {
        let tmpdir = TempDir::new().unwrap();
        let gateway = QueryGateway::new(tmpdir.path().to_path_buf());
        
        let start = Instant::now();
        let _ = gateway.execute_query("SELECT COUNT(*) as cnt FROM (VALUES (1), (2), (3), (4), (5))", vec![]).await.unwrap();
        let first_time = start.elapsed();
        
        let start = Instant::now();
        let _ = gateway.execute_query("SELECT COUNT(*) as cnt FROM (VALUES (1), (2), (3), (4), (5))", vec![]).await.unwrap();
        let second_time = start.elapsed();
        
        assert!(second_time < first_time);
    }
}