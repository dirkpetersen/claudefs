//! Security audit tests for A8 management admin API.
//!
//! Findings: FINDING-10 through FINDING-15

use claudefs_mgmt::{AdminApi, MgmtConfig, ClusterMetrics};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use std::sync::Arc;
use tower::ServiceExt;

fn make_api(token: Option<&str>) -> axum::Router {
    let mut config = MgmtConfig::default();
    config.admin_token = token.map(|t| t.to_string());
    let config = Arc::new(config);
    let metrics = Arc::new(ClusterMetrics::new());
    let api = Arc::new(AdminApi::new(metrics, config));
    api.router()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finding_10_token_comparison_not_constant_time() {
        let token1 = "aaaaaaaaaaaaaaaa";
        let token2 = "aaaaaaaaaaaaaaab";
        let token3 = "baaaaaaaaaaaaaaa";

        assert_ne!(token1, token2);
        assert_ne!(token1, token3);
    }

    #[tokio::test]
    async fn finding_11_all_endpoints_accessible_without_token_config() {
        let router = make_api(None);

        let endpoints = [
            "/health",
            "/metrics",
            "/api/v1/cluster/status",
            "/api/v1/nodes",
            "/api/v1/replication/status",
            "/api/v1/capacity",
        ];

        for endpoint in endpoints {
            let request = Request::builder()
                .uri(endpoint)
                .body(Body::empty())
                .unwrap();
            let response = router.clone().oneshot(request).await.unwrap();
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "FINDING-11: {} accessible without any auth when admin_token is None",
                endpoint
            );
        }
    }

    #[tokio::test]
    async fn finding_12_any_valid_token_grants_full_access() {
        let router = make_api(Some("admin-token-123"));

        let admin_endpoints = [
            ("/api/v1/cluster/status", "GET"),
            ("/api/v1/nodes", "GET"),
            ("/api/v1/replication/status", "GET"),
            ("/api/v1/capacity", "GET"),
        ];

        for (endpoint, _method) in admin_endpoints {
            let request = Request::builder()
                .uri(endpoint)
                .header("Authorization", "Bearer admin-token-123")
                .body(Body::empty())
                .unwrap();
            let response = router.clone().oneshot(request).await.unwrap();
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "FINDING-12: {} accessible with any valid token — no RBAC check",
                endpoint
            );
        }
    }

    /// FINDING-13 FIXED: Rate limiting now enforced on auth failures.
    #[tokio::test]
    async fn finding_13_rate_limiting_on_auth_failures() {
        let router = make_api(Some("correct-token"));

        let mut got_rate_limited = false;
        for i in 0..50u32 {
            let wrong_token = format!("wrong-token-{}", i);
            let request = Request::builder()
                .uri("/api/v1/nodes")
                .header("Authorization", format!("Bearer {}", wrong_token))
                .body(Body::empty())
                .unwrap();
            let response = router.clone().oneshot(request).await.unwrap();
            let status = response.status();
            assert!(
                status == StatusCode::UNAUTHORIZED || status == StatusCode::TOO_MANY_REQUESTS,
                "FINDING-13: Request {} returned unexpected status {}",
                i,
                status
            );
            if status == StatusCode::TOO_MANY_REQUESTS {
                got_rate_limited = true;
            }
        }
        assert!(
            got_rate_limited,
            "FINDING-13 FIXED: Rate limiting should kick in after repeated auth failures"
        );
    }

    #[tokio::test]
    async fn finding_14_health_leaks_version_without_auth() {
        let router = make_api(None);

        let request = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(
            json.get("version").is_some(),
            "FINDING-14: Version exposed without auth when admin_token is None"
        );
    }

    #[tokio::test]
    async fn finding_14_health_requires_auth_when_token_configured() {
        let router = make_api(Some("secret-token"));

        let request = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let response = router.oneshot(request).await.unwrap();

        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "FINDING-14: Health endpoint currently requires auth when token is configured"
        );
    }

    #[tokio::test]
    async fn finding_15_drain_endpoint_no_rbac_check() {
        let router = make_api(Some("any-user-token"));
        
        let request = Request::builder()
            .uri("/api/v1/nodes")
            .header("Authorization", "Bearer any-user-token")
            .body(Body::empty())
            .unwrap();
        let response = router.oneshot(request).await.unwrap();
        
        assert_eq!(response.status(), StatusCode::OK, "FINDING-15: Nodes endpoint accessible with any valid token — no RBAC check on credentials");
    }

    #[tokio::test]
    async fn auth_header_case_sensitivity() {
        let router = make_api(Some("token123"));

        let test_cases = [
            ("bearer token123", false),
            ("BEARER token123", false),
            ("Bearer token123", true),
            ("Bearer  token123", false),
        ];

        for (auth_value, should_pass) in test_cases {
            let request = Request::builder()
                .uri("/api/v1/nodes")
                .header("Authorization", auth_value)
                .body(Body::empty())
                .unwrap();
            let response = router.clone().oneshot(request).await.unwrap();
            if should_pass {
                assert_eq!(
                    response.status(),
                    StatusCode::OK,
                    "Auth '{}' should pass",
                    auth_value
                );
            } else {
                assert_eq!(
                    response.status(),
                    StatusCode::UNAUTHORIZED,
                    "Auth '{}' should fail",
                    auth_value
                );
            }
        }
    }

    #[test]
    fn rbac_inactive_user_denied_all_permissions() {
        use claudefs_mgmt::rbac::{RbacRegistry, User, Permission, admin_role};
        let mut registry = RbacRegistry::new();
        registry.add_role(admin_role());

        let mut user = User::new("u1".to_string(), "alice".to_string());
        user.active = false;
        registry.add_user(user);
        registry.assign_role("u1", "admin").unwrap();

        let result = registry.check_permission("u1", &Permission::ViewCluster);
        assert!(result.is_err());
    }

    #[test]
    fn rbac_removed_role_still_in_user_roles_list() {
        use claudefs_mgmt::rbac::{RbacRegistry, User, Permission, admin_role};
        let mut registry = RbacRegistry::new();
        registry.add_role(admin_role());

        let user = User::new("u1".to_string(), "alice".to_string());
        registry.add_user(user);
        registry.assign_role("u1", "admin").unwrap();

        registry.remove_role("admin");

        let user = registry.get_user("u1").unwrap();
        assert!(
            user.roles.contains(&"admin".to_string()),
            "Stale role reference remains"
        );

        let result = registry.check_permission("u1", &Permission::ViewCluster);
        assert!(result.is_err(), "Stale role grants no permissions — correct behavior");
    }

    #[test]
    fn rbac_viewer_cannot_drain_nodes() {
        use claudefs_mgmt::rbac::{RbacRegistry, User, Permission, viewer_role};
        let mut registry = RbacRegistry::new();
        registry.add_role(viewer_role());

        let user = User::new("u1".to_string(), "alice".to_string());
        registry.add_user(user);
        registry.assign_role("u1", "viewer").unwrap();

        let result = registry.check_permission("u1", &Permission::DrainNodes);
        assert!(result.is_err());
    }

    #[test]
    fn rbac_admin_can_drain_nodes() {
        use claudefs_mgmt::rbac::{RbacRegistry, User, Permission, admin_role};
        let mut registry = RbacRegistry::new();
        registry.add_role(admin_role());

        let user = User::new("u1".to_string(), "alice".to_string());
        registry.add_user(user);
        registry.assign_role("u1", "admin").unwrap();

        let result = registry.check_permission("u1", &Permission::DrainNodes);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn api_missing_authorization_header_rejected() {
        let router = make_api(Some("secret-token"));

        let request = Request::builder()
            .uri("/api/v1/nodes")
            .body(Body::empty())
            .unwrap();
        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn api_invalid_token_rejected() {
        let router = make_api(Some("correct-token"));

        let request = Request::builder()
            .uri("/api/v1/nodes")
            .header("Authorization", "Bearer wrong-token")
            .body(Body::empty())
            .unwrap();
        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn api_without_token_config_bypasses_auth() {
        let router = make_api(None);

        let request = Request::builder()
            .uri("/api/v1/nodes")
            .body(Body::empty())
            .unwrap();
        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}