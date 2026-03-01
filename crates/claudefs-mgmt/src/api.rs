use crate::config::MgmtConfig;
use crate::metrics::ClusterMetrics;
use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::ServiceExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub node_id: String,
    pub addr: String,
    pub status: NodeStatus,
    pub capacity_total: u64,
    pub capacity_used: u64,
    pub last_seen: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum NodeStatus {
    Healthy,
    Degraded,
    Offline,
    Draining,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterStatus {
    pub total_nodes: usize,
    pub healthy_nodes: usize,
    pub degraded_nodes: usize,
    pub offline_nodes: usize,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationStatus {
    pub lag_secs: f64,
    pub conflicts_total: u64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapacitySummary {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub usage_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrainResponse {
    pub node_id: String,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct NodeRegistry {
    nodes: HashMap<String, NodeInfo>,
}

impl NodeRegistry {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
        }
    }

    pub fn add_node(&mut self, info: NodeInfo) {
        self.nodes.insert(info.node_id.clone(), info);
    }

    pub fn get_node(&self, node_id: &str) -> Option<&NodeInfo> {
        self.nodes.get(node_id)
    }

    pub fn list_nodes(&self) -> Vec<&NodeInfo> {
        self.nodes.values().collect()
    }

    pub fn update_status(&mut self, node_id: &str, status: NodeStatus) {
        if let Some(node) = self.nodes.get_mut(node_id) {
            node.status = status;
        }
    }

    pub fn remove_node(&mut self, node_id: &str) {
        self.nodes.remove(node_id);
    }
}

impl Default for NodeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct AdminApi {
    metrics: Arc<ClusterMetrics>,
    config: Arc<MgmtConfig>,
    node_registry: Arc<RwLock<NodeRegistry>>,
}

impl AdminApi {
    pub fn new(metrics: Arc<ClusterMetrics>, config: Arc<MgmtConfig>) -> Self {
        Self {
            metrics,
            config,
            node_registry: Arc::new(RwLock::new(NodeRegistry::new())),
        }
    }

    pub fn router(self: Arc<Self>) -> Router {
        Router::new()
            .route("/health", get(health_handler))
            .route("/metrics", get(metrics_handler))
            .route("/api/v1/cluster/status", get(cluster_status_handler))
            .route("/api/v1/nodes", get(nodes_list_handler))
            .route("/api/v1/nodes/{node_id}/drain", post(node_drain_handler))
            .route("/api/v1/replication/status", get(replication_status_handler))
            .route("/api/v1/capacity", get(capacity_handler))
            .layer(axum::middleware::from_fn_with_state(
                self.clone(),
                auth_middleware,
            ))
            .with_state(self)
    }

    pub async fn serve(self) -> anyhow::Result<()> {
        let addr = self.config.bind_addr;
        let router = Arc::new(self).router();

        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!("Admin API listening on {}", addr);

        axum::serve(listener, router.into_make_service()).await?;
        Ok(())
    }
}

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": "0.1.0"
    }))
}

async fn metrics_handler(State(state): State<Arc<AdminApi>>) -> (StatusCode, String) {
    let output = state.metrics.render_prometheus();
    (StatusCode::OK, output)
}

async fn cluster_status_handler(State(state): State<Arc<AdminApi>>) -> Json<ClusterStatus> {
    let registry = state.node_registry.read().await;
    let nodes = registry.list_nodes();

    let total = nodes.len();
    let healthy = nodes.iter().filter(|n| n.status == NodeStatus::Healthy).count();
    let degraded = nodes.iter().filter(|n| n.status == NodeStatus::Degraded).count();
    let offline = nodes.iter().filter(|n| n.status == NodeStatus::Offline).count();

    let status = if offline > 0 {
        "degraded"
    } else if degraded > 0 {
        "degraded"
    } else {
        "healthy"
    }
    .to_string();

    Json(ClusterStatus {
        total_nodes: total,
        healthy_nodes: healthy,
        degraded_nodes: degraded,
        offline_nodes: offline,
        status,
    })
}

async fn nodes_list_handler(State(state): State<Arc<AdminApi>>) -> Json<Vec<NodeInfo>> {
    let registry = state.node_registry.read().await;
    Json(registry.list_nodes().into_iter().cloned().collect())
}

async fn node_drain_handler(
    State(state): State<Arc<AdminApi>>,
    Path(node_id): Path<String>,
) -> Json<DrainResponse> {
    let mut registry = state.node_registry.write().await;

    if registry.get_node(&node_id).is_some() {
        registry.update_status(&node_id, NodeStatus::Draining);
        Json(DrainResponse {
            node_id,
            status: "draining".to_string(),
            message: "Node drain initiated".to_string(),
        })
    } else {
        Json(DrainResponse {
            node_id,
            status: "error".to_string(),
            message: "Node not found".to_string(),
        })
    }
}

async fn replication_status_handler(State(state): State<Arc<AdminApi>>) -> Json<ReplicationStatus> {
    let lag = state.metrics.replication_lag_secs.get();
    let conflicts = state.metrics.replication_conflicts_total.get();

    let status = if lag > 60.0 {
        "lagging"
    } else {
        "ok"
    }
    .to_string();

    Json(ReplicationStatus {
        lag_secs: lag,
        conflicts_total: conflicts,
        status,
    })
}

async fn capacity_handler(State(state): State<Arc<AdminApi>>) -> Json<CapacitySummary> {
    let total = state.metrics.capacity_total_bytes.get() as u64;
    let used = state.metrics.capacity_used_bytes.get() as u64;
    let available = state.metrics.capacity_available_bytes.get() as u64;

    let usage_percent = if total > 0 {
        (used as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    Json(CapacitySummary {
        total_bytes: total,
        used_bytes: used,
        available_bytes: available,
        usage_percent,
    })
}

async fn auth_middleware(
    State(state): State<Arc<AdminApi>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if state.config.admin_token.is_none() {
        return next.run(request).await;
    }

    let token = state.config.admin_token.as_ref().unwrap();

    match request.headers().get("Authorization") {
        Some(auth_header) => {
            let auth_str = auth_header.to_str().unwrap_or("");
            if auth_str.starts_with("Bearer ") {
                let provided_token = &auth_str[7..];
                if provided_token == token {
                    return next.run(request).await;
                }
            }
            (
                StatusCode::UNAUTHORIZED,
                [(
                    header::WWW_AUTHENTICATE,
                    r#"Bearer realm="claudefs-mgmt""#,
                )],
                "Unauthorized",
            )
                .into_response()
        }
        None => (
            StatusCode::UNAUTHORIZED,
            [(
                header::WWW_AUTHENTICATE,
                r#"Bearer realm="claudefs-mgmt""#,
            )],
            "Unauthorized",
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_health_endpoint() {
        let config = Arc::new(MgmtConfig::default());
        let metrics = Arc::new(ClusterMetrics::new());
        let api = Arc::new(AdminApi::new(metrics, config));
        let router = api.router();

        let request = Request::builder().uri("/health").body(Body::empty()).unwrap();
        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
        assert_eq!(json["version"], "0.1.0");
    }

    #[tokio::test]
    async fn test_metrics_endpoint() {
        let config = Arc::new(MgmtConfig::default());
        let metrics = Arc::new(ClusterMetrics::new());
        metrics.iops_read.add(100);
        let api = Arc::new(AdminApi::new(metrics, config));
        let router = api.router();

        let request = Request::builder().uri("/metrics").body(Body::empty()).unwrap();
        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let output = String::from_utf8_lossy(&body);
        assert!(output.contains("claudefs_iops_read_total"));
    }

    #[tokio::test]
    async fn test_cluster_status_endpoint() {
        let config = Arc::new(MgmtConfig::default());
        let metrics = Arc::new(ClusterMetrics::new());
        let api = Arc::new(AdminApi::new(metrics, config));
        let router = api.router();

        let request = Request::builder()
            .uri("/api/v1/cluster/status")
            .body(Body::empty())
            .unwrap();
        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json.get("total_nodes").is_some());
        assert!(json.get("status").is_some());
    }

    #[tokio::test]
    async fn test_nodes_list_endpoint() {
        let config = Arc::new(MgmtConfig::default());
        let metrics = Arc::new(ClusterMetrics::new());
        let api = Arc::new(AdminApi::new(metrics, config));
        let router = api.router();

        let request = Request::builder()
            .uri("/api/v1/nodes")
            .body(Body::empty())
            .unwrap();
        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(json.is_empty());
    }

    #[tokio::test]
    async fn test_unauthorized_request_rejected_with_token() {
        let mut config = MgmtConfig::default();
        config.admin_token = Some("secret-token".to_string());

        let config = Arc::new(config);
        let metrics = Arc::new(ClusterMetrics::new());
        let api = Arc::new(AdminApi::new(metrics, config));
        let router = api.router();

        let request = Request::builder()
            .uri("/api/v1/nodes")
            .body(Body::empty())
            .unwrap();
        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_authorized_request_accepted_with_token() {
        let mut config = MgmtConfig::default();
        config.admin_token = Some("secret-token".to_string());

        let config = Arc::new(config);
        let metrics = Arc::new(ClusterMetrics::new());
        let api = Arc::new(AdminApi::new(metrics, config));
        let router = api.router();

        let request = Request::builder()
            .uri("/api/v1/nodes")
            .header("Authorization", "Bearer secret-token")
            .body(Body::empty())
            .unwrap();
        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }



    #[tokio::test]
    async fn test_replication_status_endpoint() {
        let config = Arc::new(MgmtConfig::default());
        let metrics = Arc::new(ClusterMetrics::new());
        metrics.replication_lag_secs.set(5.0);
        let api = Arc::new(AdminApi::new(metrics, config));
        let router = api.router();

        let request = Request::builder()
            .uri("/api/v1/replication/status")
            .body(Body::empty())
            .unwrap();
        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json.get("lag_secs").is_some());
    }

    #[tokio::test]
    async fn test_capacity_endpoint() {
        let config = Arc::new(MgmtConfig::default());
        let metrics = Arc::new(ClusterMetrics::new());
        metrics.capacity_total_bytes.set(1000000000.0);
        metrics.capacity_used_bytes.set(500000000.0);
        let api = Arc::new(AdminApi::new(metrics, config));
        let router = api.router();

        let request = Request::builder()
            .uri("/api/v1/capacity")
            .body(Body::empty())
            .unwrap();
        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["total_bytes"], 1_000_000_000_u64);
        assert_eq!(json["used_bytes"], 500_000_000_u64);
    }

    #[tokio::test]
    async fn test_health_endpoint_returns_200() {
        let config = Arc::new(MgmtConfig::default());
        let metrics = Arc::new(ClusterMetrics::new());
        let api = Arc::new(AdminApi::new(metrics, config));
        let router = api.router();

        let request = Request::builder().uri("/health").body(Body::empty()).unwrap();
        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_invalid_token_rejected() {
        let mut config = MgmtConfig::default();
        config.admin_token = Some("secret-token".to_string());

        let config = Arc::new(config);
        let metrics = Arc::new(ClusterMetrics::new());
        let api = Arc::new(AdminApi::new(metrics, config));
        let router = api.router();

        let request = Request::builder()
            .uri("/api/v1/nodes")
            .header("Authorization", "Bearer wrong-token")
            .body(Body::empty())
            .unwrap();
        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_invalid_auth_scheme_rejected() {
        let mut config = MgmtConfig::default();
        config.admin_token = Some("secret-token".to_string());

        let config = Arc::new(config);
        let metrics = Arc::new(ClusterMetrics::new());
        let api = Arc::new(AdminApi::new(metrics, config));
        let router = api.router();

        let request = Request::builder()
            .uri("/api/v1/nodes")
            .header("Authorization", "Basic dXNlcjpwYXNz")
            .body(Body::empty())
            .unwrap();
        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_metrics_format_correctness() {
        let config = Arc::new(MgmtConfig::default());
        let metrics = Arc::new(ClusterMetrics::new());
        metrics.nodes_total.set(5.0);
        metrics.nodes_healthy.set(4.0);
        let api = Arc::new(AdminApi::new(metrics, config));
        let router = api.router();

        let request = Request::builder().uri("/metrics").body(Body::empty()).unwrap();
        let response = router.oneshot(request).await.unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let output = String::from_utf8_lossy(&body);

        assert!(output.contains("# TYPE claudefs_nodes_total gauge"));
        assert!(output.contains("claudefs_nodes_total 5"));
    }
}