use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use crate::query_gateway::{QueryGateway, QueryResult};

#[derive(Debug, Clone)]
pub struct AppState {
    pub gateway: Arc<RwLock<QueryGateway>>,
}

impl AppState {
    pub fn new(gateway: QueryGateway) -> Self {
        Self {
            gateway: Arc::new(RwLock::new(gateway)),
        }
    }

    pub fn with_gateway(gateway: Arc<RwLock<QueryGateway>>) -> Self {
        Self { gateway }
    }
}

unsafe impl Send for AppState {}
unsafe impl Sync for AppState {}

#[derive(Debug, Serialize, Deserialize)]
pub struct TopUser {
    pub owner_name: String,
    pub total_size_bytes: u64,
    pub file_count: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TopDir {
    pub path: String,
    pub total_size_bytes: u64,
    pub file_count: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StaleFile {
    pub path: String,
    pub filename: String,
    pub mtime: i64,
    pub size_bytes: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileTypeStats {
    pub file_type: String,
    pub count: u64,
    pub total_size: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReductionReport {
    pub path: String,
    pub total_logical_bytes: u64,
    pub total_stored_bytes: u64,
    pub reduction_ratio: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClusterHealth {
    pub status: String,
    pub nodes_healthy: u32,
    pub nodes_total: u32,
    pub replication_lag_ms: u64,
    pub last_check: i64,
}

#[derive(Debug, Deserialize)]
pub struct CustomQueryRequest {
    pub query: String,
    pub params: Option<Vec<String>>,
}

#[derive(Debug)]
pub enum ApiError {
    QueryError(String),
    Timeout,
    Internal(String),
    NotFound(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            ApiError::QueryError(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
            ApiError::Timeout => (StatusCode::REQUEST_TIMEOUT, "Query timeout").into_response(),
            ApiError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response(),
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg).into_response(),
        }
    }
}

impl From<crate::query_gateway::QueryError> for ApiError {
    fn from(err: crate::query_gateway::QueryError) -> Self {
        match err {
            crate::query_gateway::QueryError::Timeout => ApiError::Timeout,
            crate::query_gateway::QueryError::SqlInjection => {
                ApiError::QueryError("SQL injection detected".to_string())
            }
            other => ApiError::QueryError(other.to_string()),
        }
    }
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        .route("/query", post(query_handler))
        .route("/query/custom", post(custom_query_handler))
        .route("/analytics/top-users", get(top_users_handler))
        .route("/analytics/top-dirs", get(top_dirs_handler))
        .route("/analytics/stale-files", get(stale_files_handler))
        .route("/analytics/file-types", get(file_types_handler))
        .route("/analytics/reduction", get(reduction_handler))
        .with_state(state)
}

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "timestamp": chrono::Utc::now().timestamp()
    }))
}

async fn metrics_handler(State(state): State<AppState>) -> Result<String, ApiError> {
    let gateway = state.gateway.read().await;
    let (valid, expired) = gateway.cache_stats();
    let mut output = String::new();
    
    output.push_str("# HELP claudefs_query_cache_entries Cache entries\n");
    output.push_str("# TYPE claudefs_query_cache_entries gauge\n");
    output.push_str(&format!(
        "claudefs_query_cache_entries_valid {}\n",
        valid
    ));
    output.push_str(&format!(
        "claudefs_query_cache_entries_expired {}\n",
        expired
    ));
    
    Ok(output)
}

async fn query_handler(
    State(state): State<AppState>,
    Json(req): Json<CustomQueryRequest>,
) -> Result<Json<QueryResult>, ApiError> {
    let gateway = state.gateway.read().await;
    let params = req.params.unwrap_or_default();
    let result = gateway
        .execute_query(&req.query, params)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(result))
}

async fn custom_query_handler(
    State(state): State<AppState>,
    Json(req): Json<CustomQueryRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let gateway = state.gateway.read().await;
    let params = req.params.unwrap_or_default();
    let result = gateway
        .execute_query(&req.query, params)
        .await
        .map_err(ApiError::from)?;

    let rows: Vec<serde_json::Value> = result
        .rows
        .into_iter()
        .map(|row| {
            serde_json::Value::Object(
                result
                    .columns
                    .iter()
                    .zip(row.into_iter())
                    .map(|(k, v)| (k.clone(), v))
                    .collect(),
            )
        })
        .collect();

    Ok(Json(serde_json::json!({
        "columns": result.columns,
        "rows": rows,
        "row_count": result.row_count
    })))
}

async fn top_users_handler(
    State(state): State<AppState>,
    Query(params): Query<TopUsersParams>,
) -> Result<Json<Vec<TopUser>>, ApiError> {
    let limit = params.limit.unwrap_or(20);
    let gateway = state.gateway.read().await;
    
    let sql = format!(
        "SELECT owner_name, SUM(size_bytes) as total_size_bytes, COUNT(*) as file_count
         FROM metadata
         GROUP BY owner_name
         ORDER BY total_size_bytes DESC
         LIMIT {}",
        limit
    );
    
    let result = gateway
        .execute_query(&sql, vec![])
        .await
        .map_err(ApiError::from)?;
    
    let users: Vec<TopUser> = result
        .rows
        .iter()
        .map(|row| TopUser {
            owner_name: row.get(0).and_then(|v| v.as_str()).unwrap_or("").to_string(),
            total_size_bytes: row.get(1).and_then(|v| v.as_u64()).unwrap_or(0),
            file_count: row.get(2).and_then(|v| v.as_u64()).unwrap_or(0),
        })
        .collect();
    
    Ok(Json(users))
}

async fn top_dirs_handler(
    State(state): State<AppState>,
    Query(params): Query<TopDirsParams>,
) -> Result<Json<Vec<TopDir>>, ApiError> {
    let limit = params.limit.unwrap_or(20);
    let gateway = state.gateway.read().await;
    
    let sql = format!(
        "SELECT parent_path as path, SUM(size_bytes) as total_size_bytes, COUNT(*) as file_count
         FROM metadata
         GROUP BY parent_path
         ORDER BY total_size_bytes DESC
         LIMIT {}",
        limit
    );
    
    let result = gateway
        .execute_query(&sql, vec![])
        .await
        .map_err(ApiError::from)?;
    
    let dirs: Vec<TopDir> = result
        .rows
        .iter()
        .map(|row| TopDir {
            path: row.get(0).and_then(|v| v.as_str()).unwrap_or("").to_string(),
            total_size_bytes: row.get(1).and_then(|v| v.as_u64()).unwrap_or(0),
            file_count: row.get(2).and_then(|v| v.as_u64()).unwrap_or(0),
        })
        .collect();
    
    Ok(Json(dirs))
}

async fn stale_files_handler(
    State(state): State<AppState>,
    Query(params): Query<StaleFilesParams>,
) -> Result<Json<Vec<StaleFile>>, ApiError> {
    let days = params.days.unwrap_or(30);
    let limit = params.limit.unwrap_or(100);
    let gateway = state.gateway.read().await;
    
    let cutoff = chrono::Utc::now().timestamp() - (days as i64 * 86400);
    let sql = format!(
        "SELECT path, filename, mtime, size_bytes FROM metadata
         WHERE mtime < {}
         ORDER BY mtime ASC
         LIMIT {}",
        cutoff, limit
    );
    
    let result = gateway
        .execute_query(&sql, vec![])
        .await
        .map_err(ApiError::from)?;
    
    let files: Vec<StaleFile> = result
        .rows
        .iter()
        .map(|row| StaleFile {
            path: row.get(0).and_then(|v| v.as_str()).unwrap_or("").to_string(),
            filename: row.get(1).and_then(|v| v.as_str()).unwrap_or("").to_string(),
            mtime: row.get(2).and_then(|v| v.as_i64()).unwrap_or(0),
            size_bytes: row.get(3).and_then(|v| v.as_u64()).unwrap_or(0),
        })
        .collect();
    
    Ok(Json(files))
}

async fn file_types_handler(
    State(state): State<AppState>,
) -> Result<Json<Vec<FileTypeStats>>, ApiError> {
    let gateway = state.gateway.read().await;
    
    let sql = "SELECT file_type, COUNT(*) as count, SUM(size_bytes) as total_size
               FROM metadata
               GROUP BY file_type
               ORDER BY total_size DESC";
    
    let result = gateway
        .execute_query(sql, vec![])
        .await
        .map_err(ApiError::from)?;
    
    let stats: Vec<FileTypeStats> = result
        .rows
        .iter()
        .map(|row| FileTypeStats {
            file_type: row.get(0).and_then(|v| v.as_str()).unwrap_or("unknown").to_string(),
            count: row.get(1).and_then(|v| v.as_u64()).unwrap_or(0),
            total_size: row.get(2).and_then(|v| v.as_u64()).unwrap_or(0),
        })
        .collect();
    
    Ok(Json(stats))
}

async fn reduction_handler(
    State(state): State<AppState>,
    Query(params): Query<ReductionReportParams>,
) -> Result<Json<Vec<ReductionReport>>, ApiError> {
    let limit = params.limit.unwrap_or(20);
    let gateway = state.gateway.read().await;
    
    let sql = format!(
        "SELECT parent_path as path,
                SUM(size_bytes) as total_logical_bytes,
                SUM(blocks_stored * 4096) as total_stored_bytes
         FROM metadata
         GROUP BY parent_path
         ORDER BY total_logical_bytes DESC
         LIMIT {}",
        limit
    );
    
    let result = gateway
        .execute_query(&sql, vec![])
        .await
        .map_err(ApiError::from)?;
    
    let reports: Vec<ReductionReport> = result
        .rows
        .iter()
        .map(|row| {
            let logical = row.get(1).and_then(|v| v.as_u64()).unwrap_or(0);
            let stored = row.get(2).and_then(|v| v.as_u64()).unwrap_or(1);
            let ratio = if stored > 0 { logical as f64 / stored as f64 } else { 0.0 };
            
            ReductionReport {
                path: row.get(0).and_then(|v| v.as_str()).unwrap_or("").to_string(),
                total_logical_bytes: logical,
                total_stored_bytes: stored,
                reduction_ratio: ratio,
            }
        })
        .collect();
    
    Ok(Json(reports))
}

#[derive(Debug, Deserialize)]
struct TopUsersParams {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct TopDirsParams {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct StaleFilesParams {
    days: Option<u64>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ReductionReportParams {
    limit: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_app_state_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AppState>();
    }

    #[tokio::test]
    async fn test_create_router() {
        let gateway = QueryGateway::new(PathBuf::from("/tmp/test-index"));
        let state = AppState::new(gateway);
        let _router = create_router(state);
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let gateway = QueryGateway::new(PathBuf::from("/tmp/test-index"));
        let state = AppState::new(gateway);
        let router = create_router(state);

        let req = axum::http::Request::builder()
            .uri("/health")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = router.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_query_endpoint() {
        let gateway = QueryGateway::new(PathBuf::from("/tmp/test-index"));
        let state = AppState::new(gateway);
        let router = create_router(state);

        let req = axum::http::Request::builder()
            .uri("/query")
            .method("POST")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(r#"{"query": "SELECT 1 as num}"#))
            .unwrap();
        let response = router.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_top_users_endpoint() {
        let gateway = QueryGateway::new(PathBuf::from("/tmp/test-index"));
        let state = AppState::new(gateway);
        let router = create_router(state);

        let req = axum::http::Request::builder()
            .uri("/analytics/top-users?limit=10")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = router.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_file_types_endpoint() {
        let gateway = QueryGateway::new(PathBuf::from("/tmp/test-index"));
        let state = AppState::new(gateway);
        let router = create_router(state);

        let req = axum::http::Request::builder()
            .uri("/analytics/file-types")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = router.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_api_error_response() {
        let error = ApiError::Timeout;
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    }

    #[tokio::test]
    async fn test_api_error_from_query_error() {
        let query_err = crate::query_gateway::QueryError::Timeout;
        let api_err = ApiError::from(query_err);
        match api_err {
            ApiError::Timeout => {}
            _ => panic!("Expected Timeout"),
        }
    }
}