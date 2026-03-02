//! S3 CORS configuration and preflight handling

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

/// CORS rule for an S3 bucket
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorsRule {
    /// Origins allowed to make requests (supports "*" wildcard)
    pub allowed_origins: Vec<String>,
    /// HTTP methods allowed (GET, PUT, POST, DELETE, HEAD, etc.)
    pub allowed_methods: Vec<String>,
    /// Headers allowed in requests (supports "*" wildcard)
    pub allowed_headers: Vec<String>,
    /// Headers exposed to the browser
    pub expose_headers: Vec<String>,
    /// How long browsers should cache preflight results (seconds)
    pub max_age_seconds: u32,
}

impl CorsRule {
    /// Creates a new empty CORS rule with default max_age of 3600 seconds.
    pub fn new() -> Self {
        Self {
            allowed_origins: Vec::new(),
            allowed_methods: Vec::new(),
            allowed_headers: Vec::new(),
            expose_headers: Vec::new(),
            max_age_seconds: 3600,
        }
    }

    /// Creates a permissive CORS rule that allows all origins, methods, and headers.
    pub fn allow_all() -> Self {
        Self {
            allowed_origins: vec!["*".to_string()],
            allowed_methods: vec![
                "GET".to_string(),
                "PUT".to_string(),
                "POST".to_string(),
                "DELETE".to_string(),
                "HEAD".to_string(),
            ],
            allowed_headers: vec!["*".to_string()],
            expose_headers: Vec::new(),
            max_age_seconds: 3600,
        }
    }

    /// Checks if the given origin is allowed by this rule.
    pub fn matches_origin(&self, origin: &str) -> bool {
        self.allowed_origins.iter().any(|o| o == "*" || o == origin)
    }

    /// Checks if the given HTTP method is allowed by this rule.
    pub fn allows_method(&self, method: &str) -> bool {
        let method_upper = method.to_uppercase();
        self.allowed_methods
            .iter()
            .any(|m| m.to_uppercase() == method_upper)
    }

    /// Checks if all given headers are allowed by this rule.
    pub fn allows_headers(&self, headers: &[&str]) -> bool {
        if self.allowed_headers.iter().any(|h| h == "*") {
            return true;
        }
        headers.iter().all(|h| {
            self.allowed_headers
                .iter()
                .any(|allowed| allowed.to_lowercase() == h.to_lowercase())
        })
    }

    /// Returns true if the rule has at least one origin and one method configured.
    pub fn is_valid(&self) -> bool {
        !self.allowed_origins.is_empty() && !self.allowed_methods.is_empty()
    }
}

impl Default for CorsRule {
    fn default() -> Self {
        Self::new()
    }
}

/// CORS configuration for a bucket
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorsConfig {
    /// CORS rules applied to this configuration
    pub rules: Vec<CorsRule>,
}

impl CorsConfig {
    /// Creates a new empty CORS configuration.
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Adds a CORS rule to this configuration.
    pub fn add_rule(&mut self, rule: CorsRule) {
        self.rules.push(rule);
    }

    /// Finds the first rule that matches the given origin and method.
    pub fn matching_rule(&self, origin: &str, method: &str) -> Option<&CorsRule> {
        self.rules
            .iter()
            .find(|r| r.matches_origin(origin) && r.allows_method(method))
    }

    /// Returns true if this configuration has no rules.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Returns the number of rules in this configuration.
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// CORS preflight request
#[derive(Debug, Clone)]
pub struct PreflightRequest {
    /// Origin header from the request
    pub origin: String,
    /// HTTP method being requested
    pub method: String,
    /// Headers being requested
    pub headers: Vec<String>,
}

/// CORS preflight response
#[derive(Debug, Clone)]
pub struct PreflightResponse {
    /// Allowed origin to include in response (None if denied)
    pub allowed_origin: Option<String>,
    /// Methods allowed for the request
    pub allowed_methods: Vec<String>,
    /// Headers allowed for the request
    pub allowed_headers: Vec<String>,
    /// Headers exposed to the browser
    pub expose_headers: Vec<String>,
    /// Cache duration for preflight response
    pub max_age: u32,
    /// Whether the preflight request is allowed
    pub allowed: bool,
}

/// Handles a CORS preflight request by checking against the configuration.
pub fn handle_preflight(config: &CorsConfig, req: &PreflightRequest) -> PreflightResponse {
    if let Some(rule) = config.matching_rule(&req.origin, &req.method) {
        let allowed_headers: Vec<String> = if rule.allowed_headers.iter().any(|h| h == "*") {
            req.headers.clone()
        } else {
            req.headers
                .iter()
                .filter(|h| {
                    rule.allowed_headers
                        .iter()
                        .any(|allowed| allowed.to_lowercase() == h.to_lowercase())
                })
                .cloned()
                .collect()
        };

        if allowed_headers.len() == req.headers.len()
            || rule.allowed_headers.iter().any(|h| h == "*")
        {
            return PreflightResponse {
                allowed_origin: Some(req.origin.clone()),
                allowed_methods: rule.allowed_methods.clone(),
                allowed_headers,
                expose_headers: rule.expose_headers.clone(),
                max_age: rule.max_age_seconds,
                allowed: true,
            };
        }
    }

    PreflightResponse {
        allowed_origin: None,
        allowed_methods: Vec::new(),
        allowed_headers: Vec::new(),
        expose_headers: Vec::new(),
        max_age: 0,
        allowed: false,
    }
}

/// Generates CORS response headers for a request based on the configuration.
pub fn cors_response_headers(
    config: &CorsConfig,
    origin: &str,
    method: &str,
) -> Vec<(String, String)> {
    let mut headers = Vec::new();

    if let Some(rule) = config.matching_rule(origin, method) {
        if rule.matches_origin(origin) {
            headers.push((
                "Access-Control-Allow-Origin".to_string(),
                origin.to_string(),
            ));
            if !rule.expose_headers.is_empty() {
                headers.push((
                    "Access-Control-Expose-Headers".to_string(),
                    rule.expose_headers.join(", "),
                ));
            }
        }
    }

    headers
}

/// Global CORS registry per bucket
pub struct CorsRegistry {
    configs: RwLock<HashMap<String, CorsConfig>>,
}

impl CorsRegistry {
    /// Creates a new empty CORS registry.
    pub fn new() -> Self {
        Self {
            configs: RwLock::new(HashMap::new()),
        }
    }

    /// Sets the CORS configuration for a bucket.
    pub fn set_config(&self, bucket: &str, config: CorsConfig) {
        self.configs
            .write()
            .unwrap()
            .insert(bucket.to_string(), config);
    }

    /// Gets the CORS configuration for a bucket, if one exists.
    pub fn get_config(&self, bucket: &str) -> Option<CorsConfig> {
        self.configs.read().unwrap().get(bucket).cloned()
    }

    /// Removes the CORS configuration for a bucket. Returns true if config existed.
    pub fn remove_config(&self, bucket: &str) -> bool {
        self.configs.write().unwrap().remove(bucket).is_some()
    }

    /// Handles a preflight request for a specific bucket.
    pub fn handle_preflight(&self, bucket: &str, req: &PreflightRequest) -> PreflightResponse {
        if let Some(config) = self.get_config(bucket) {
            handle_preflight(&config, req)
        } else {
            PreflightResponse {
                allowed_origin: None,
                allowed_methods: Vec::new(),
                allowed_headers: Vec::new(),
                expose_headers: Vec::new(),
                max_age: 0,
                allowed: false,
            }
        }
    }
}

impl Default for CorsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cors_rule_new() {
        let rule = CorsRule::new();
        assert!(rule.allowed_origins.is_empty());
        assert_eq!(rule.max_age_seconds, 3600);
    }

    #[test]
    fn test_cors_rule_allow_all() {
        let rule = CorsRule::allow_all();
        assert!(rule.allowed_origins.contains(&"*".to_string()));
        assert!(rule.allowed_methods.contains(&"GET".to_string()));
    }

    #[test]
    fn test_cors_rule_matches_origin_exact() {
        let rule = CorsRule {
            allowed_origins: vec!["https://example.com".to_string()],
            ..Default::default()
        };
        assert!(rule.matches_origin("https://example.com"));
        assert!(!rule.matches_origin("https://evil.com"));
    }

    #[test]
    fn test_cors_rule_matches_origin_wildcard() {
        let rule = CorsRule {
            allowed_origins: vec!["*".to_string()],
            ..Default::default()
        };
        assert!(rule.matches_origin("https://example.com"));
        assert!(rule.matches_origin("https://anything.com"));
    }

    #[test]
    fn test_cors_rule_allows_method() {
        let rule = CorsRule {
            allowed_methods: vec!["GET".to_string(), "PUT".to_string()],
            ..Default::default()
        };
        assert!(rule.allows_method("GET"));
        assert!(rule.allows_method("get"));
        assert!(!rule.allows_method("POST"));
    }

    #[test]
    fn test_cors_rule_allows_headers_exact() {
        let rule = CorsRule {
            allowed_headers: vec!["content-type".to_string(), "authorization".to_string()],
            ..Default::default()
        };
        assert!(rule.allows_headers(&["content-type"]));
        assert!(rule.allows_headers(&["content-type", "authorization"]));
        assert!(!rule.allows_headers(&["x-custom"]));
    }

    #[test]
    fn test_cors_rule_allows_headers_wildcard() {
        let rule = CorsRule {
            allowed_headers: vec!["*".to_string()],
            ..Default::default()
        };
        assert!(rule.allows_headers(&["any-header"]));
    }

    #[test]
    fn test_cors_rule_is_valid() {
        let rule = CorsRule {
            allowed_origins: vec!["*".to_string()],
            allowed_methods: vec!["GET".to_string()],
            ..Default::default()
        };
        assert!(rule.is_valid());
    }

    #[test]
    fn test_cors_rule_is_valid_invalid() {
        let rule = CorsRule::new();
        assert!(!rule.is_valid());
    }

    #[test]
    fn test_cors_config_new() {
        let config = CorsConfig::new();
        assert!(config.is_empty());
        assert_eq!(config.rule_count(), 0);
    }

    #[test]
    fn test_cors_config_add_rule() {
        let mut config = CorsConfig::new();
        config.add_rule(CorsRule::allow_all());
        assert_eq!(config.rule_count(), 1);
    }

    #[test]
    fn test_cors_config_matching_rule() {
        let mut config = CorsConfig::new();
        let mut rule = CorsRule::allow_all();
        rule.allowed_origins = vec!["https://example.com".to_string()];
        config.add_rule(rule);

        assert!(config.matching_rule("https://example.com", "GET").is_some());
        assert!(config.matching_rule("https://evil.com", "GET").is_none());
    }

    #[test]
    fn test_handle_preflight_allowed() {
        let mut config = CorsConfig::new();
        config.add_rule(CorsRule::allow_all());

        let req = PreflightRequest {
            origin: "https://example.com".to_string(),
            method: "GET".to_string(),
            headers: vec!["content-type".to_string()],
        };

        let resp = handle_preflight(&config, &req);
        assert!(resp.allowed);
        assert_eq!(resp.allowed_origin, Some("https://example.com".to_string()));
    }

    #[test]
    fn test_handle_preflight_denied_no_config() {
        let config = CorsConfig::new();

        let req = PreflightRequest {
            origin: "https://example.com".to_string(),
            method: "GET".to_string(),
            headers: vec![],
        };

        let resp = handle_preflight(&config, &req);
        assert!(!resp.allowed);
    }

    #[test]
    fn test_handle_preflight_denied_no_matching_rule() {
        let mut config = CorsConfig::new();
        let mut rule = CorsRule::new();
        rule.allowed_origins = vec!["https://allowed.com".to_string()];
        rule.allowed_methods = vec!["PUT".to_string()];
        config.add_rule(rule);

        let req = PreflightRequest {
            origin: "https://example.com".to_string(),
            method: "GET".to_string(),
            headers: vec![],
        };

        let resp = handle_preflight(&config, &req);
        assert!(!resp.allowed);
    }

    #[test]
    fn test_cors_response_headers() {
        let mut config = CorsConfig::new();
        let mut rule = CorsRule::allow_all();
        rule.expose_headers = vec!["x-custom".to_string()];
        config.add_rule(rule);

        let headers = cors_response_headers(&config, "https://example.com", "GET");
        assert!(headers
            .iter()
            .any(|(k, v)| k == "Access-Control-Allow-Origin" && v == "https://example.com"));
    }

    #[test]
    fn test_cors_response_headers_no_match() {
        let config = CorsConfig::new();

        let headers = cors_response_headers(&config, "https://example.com", "GET");
        assert!(headers.is_empty());
    }

    #[test]
    fn test_cors_registry_set_get_config() {
        let registry = CorsRegistry::new();
        let mut config = CorsConfig::new();
        config.add_rule(CorsRule::allow_all());

        registry.set_config("mybucket", config.clone());
        assert_eq!(registry.get_config("mybucket"), Some(config));
    }

    #[test]
    fn test_cors_registry_get_config_none() {
        let registry = CorsRegistry::new();
        assert_eq!(registry.get_config("nonexistent"), None);
    }

    #[test]
    fn test_cors_registry_remove_config() {
        let registry = CorsRegistry::new();
        registry.set_config("mybucket", CorsConfig::new());

        assert!(registry.remove_config("mybucket"));
        assert_eq!(registry.get_config("mybucket"), None);
    }

    #[test]
    fn test_cors_registry_remove_config_not_found() {
        let registry = CorsRegistry::new();
        assert!(!registry.remove_config("nonexistent"));
    }

    #[test]
    fn test_cors_registry_handle_preflight() {
        let registry = CorsRegistry::new();
        let mut config = CorsConfig::new();
        config.add_rule(CorsRule::allow_all());
        registry.set_config("mybucket", config);

        let req = PreflightRequest {
            origin: "https://example.com".to_string(),
            method: "GET".to_string(),
            headers: vec![],
        };

        let resp = registry.handle_preflight("mybucket", &req);
        assert!(resp.allowed);
    }

    #[test]
    fn test_cors_registry_handle_preflight_no_bucket() {
        let registry = CorsRegistry::new();

        let req = PreflightRequest {
            origin: "https://example.com".to_string(),
            method: "GET".to_string(),
            headers: vec![],
        };

        let resp = registry.handle_preflight("nonexistent", &req);
        assert!(!resp.allowed);
    }
}
