use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ScraperError {
    #[error("HTTP error scraping {url}: {msg}")]
    Http { url: String, msg: String },
    #[error("Parse error for {url}: {msg}")]
    Parse { url: String, msg: String },
}

pub type MetricSample = HashMap<String, f64>;

#[derive(Debug, Clone)]
pub struct ScrapeResult {
    pub node_id: String,
    pub url: String,
    pub samples: MetricSample,
    pub scraped_at: u64,
    pub duration_ms: u64,
    pub success: bool,
    pub error: Option<String>,
}

impl ScrapeResult {
    pub fn failed(node_id: String, url: String, error: String) -> Self {
        Self {
            node_id,
            url,
            samples: HashMap::new(),
            scraped_at: 0,
            duration_ms: 0,
            success: false,
            error: Some(error),
        }
    }

    pub fn get_metric(&self, name: &str) -> Option<f64> {
        self.samples.get(name).copied()
    }
}

pub fn parse_prometheus_text(text: &str) -> MetricSample {
    let mut result = HashMap::new();
    
    for line in text.lines() {
        let line = line.trim();
        
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        
        if let Some(space_idx) = line.find(' ') {
            let metric_part = &line[..space_idx];
            let value_part = &line[space_idx + 1..];
            
            if let Some(value) = value_part.trim().parse::<f64>().ok() {
                let base_name = if let Some(brace_idx) = metric_part.find('{') {
                    &metric_part[..brace_idx]
                } else {
                    metric_part
                };
                
                *result.entry(base_name.to_string()).or_insert(0.0) += value;
            }
        }
    }
    
    result
}

pub struct NodeScraper {
    client: reqwest::Client,
    timeout: Duration,
}

impl NodeScraper {
    pub fn new(timeout_secs: u64) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .unwrap_or_default();
        
        Self {
            client,
            timeout: Duration::from_secs(timeout_secs),
        }
    }

    pub async fn scrape(&self, node_id: &str, url: &str) -> ScrapeResult {
        let start = std::time::Instant::now();
        let node_id_owned = node_id.to_string();
        let url_owned = url.to_string();
        
        match self.client.get(url).send().await {
            Ok(response) => {
                if !response.status().is_success() {
                    return ScrapeResult::failed(
                        node_id_owned,
                        url_owned,
                        format!("HTTP {}", response.status()),
                    );
                }
                
                match response.text().await {
                    Ok(text) => {
                        let samples = parse_prometheus_text(&text);
                        let duration_ms = start.elapsed().as_millis() as u64;
                        let scraped_at = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs();
                        
                        ScrapeResult {
                            node_id: node_id_owned,
                            url: url_owned,
                            samples,
                            scraped_at,
                            duration_ms,
                            success: true,
                            error: None,
                        }
                    }
                    Err(e) => ScrapeResult::failed(
                        node_id_owned,
                        url_owned,
                        format!("Read error: {}", e),
                    ),
                }
            }
            Err(e) => ScrapeResult::failed(
                node_id_owned,
                url_owned,
                format!("Connection error: {}", e),
            ),
        }
    }
}

pub struct ScraperPool {
    scrapers: HashMap<String, String>,
    results: Arc<RwLock<HashMap<String, ScrapeResult>>>,
    timeout_secs: u64,
}

impl ScraperPool {
    pub fn new(timeout_secs: u64) -> Self {
        Self {
            scrapers: HashMap::new(),
            results: Arc::new(RwLock::new(HashMap::new())),
            timeout_secs,
        }
    }

    pub fn add_node(&mut self, node_id: String, url: String) {
        self.scrapers.insert(node_id, url);
    }

    pub fn remove_node(&mut self, node_id: &str) {
        self.scrapers.remove(node_id);
    }

    pub fn node_count(&self) -> usize {
        self.scrapers.len()
    }

    pub async fn scrape_all(&self) -> Vec<ScrapeResult> {
        let scraper = NodeScraper::new(self.timeout_secs);
        
        let futures: Vec<_> = self.scrapers.iter()
            .map(|(node_id, url)| {
                let scraper = NodeScraper::new(self.timeout_secs);
                let node_id = node_id.clone();
                let url = url.clone();
                async move {
                    scraper.scrape(&node_id, &url).await
                }
            })
            .collect();
        
        let results = futures::future::join_all(futures).await;
        
        let mut results_map = self.results.write().await;
        for result in &results {
            results_map.insert(result.node_id.clone(), result.clone());
        }
        
        results
    }

    pub async fn latest_results(&self) -> HashMap<String, ScrapeResult> {
        self.results.read().await.clone()
    }

    pub async fn latest_result(&self, node_id: &str) -> Option<ScrapeResult> {
        self.results.read().await.get(node_id).cloned()
    }

    pub async fn run_scrape_loop(self: Arc<Self>, interval_secs: u64) {
        loop {
            let _ = self.scrape_all().await;
            tokio::time::sleep(Duration::from_secs(interval_secs)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_prometheus_text_empty() {
        let result = parse_prometheus_text("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_prometheus_text_basic_counter() {
        let text = "http_requests_total 1234\n";
        let result = parse_prometheus_text(text);
        assert_eq!(result.get("http_requests_total"), Some(&1234.0));
    }

    #[test]
    fn test_parse_prometheus_text_gauge() {
        let text = "process_cpu_seconds_total 0.15\n";
        let result = parse_prometheus_text(text);
        assert_eq!(result.get("process_cpu_seconds_total"), Some(&0.15));
    }

    #[test]
    fn test_parse_prometheus_text_with_labels() {
        let text = r#"http_requests_total{method="GET"} 1234
http_requests_total{method="POST"} 5678"#;
        let result = parse_prometheus_text(text);
        assert_eq!(result.get("http_requests_total"), Some(&6912.0));
    }

    #[test]
    fn test_parse_prometheus_text_comments_ignored() {
        let text = "# HELP http_requests_total Total HTTP requests\n# TYPE http_requests_total counter\nhttp_requests_total 100\n";
        let result = parse_prometheus_text(text);
        assert_eq!(result.get("http_requests_total"), Some(&100.0));
    }

    #[test]
    fn test_parse_prometheus_text_blank_lines() {
        let text = "metric_a 1\n\nmetric_b 2\n\n";
        let result = parse_prometheus_text(text);
        assert_eq!(result.get("metric_a"), Some(&1.0));
        assert_eq!(result.get("metric_b"), Some(&2.0));
    }

    #[test]
    fn test_parse_prometheus_text_histogram() {
        let text = "http_request_duration_seconds_sum 123.45\nhttp_request_duration_seconds_count 100\nhttp_request_duration_seconds_bucket{le=\"0.1\"} 50\n";
        let result = parse_prometheus_text(text);
        assert_eq!(result.get("http_request_duration_seconds_sum"), Some(&123.45));
        assert_eq!(result.get("http_request_duration_seconds_count"), Some(&100.0));
    }

    #[test]
    fn test_parse_prometheus_text_multiple_metrics() {
        let text = "metric_a 1\nmetric_b 2\nmetric_c 3\n";
        let result = parse_prometheus_text(text);
        assert_eq!(result.get("metric_a"), Some(&1.0));
        assert_eq!(result.get("metric_b"), Some(&2.0));
        assert_eq!(result.get("metric_c"), Some(&3.0));
    }

    #[test]
    fn test_parse_prometheus_text_float_value() {
        let text = "metric 3.14159\n";
        let result = parse_prometheus_text(text);
        assert!((result.get("metric").unwrap() - 3.14159).abs() < 0.0001);
    }

    #[test]
    fn test_parse_prometheus_text_invalid_lines() {
        let text = "valid_metric 1\ninvalid line without value\nanother 2\n";
        let result = parse_prometheus_text(text);
        assert_eq!(result.get("valid_metric"), Some(&1.0));
        assert_eq!(result.get("another"), Some(&2.0));
    }

    #[test]
    fn test_scrape_result_failed_constructor() {
        let result = ScrapeResult::failed(
            "node1".to_string(),
            "http://node1:9400/metrics".to_string(),
            "Connection refused".to_string(),
        );
        
        assert_eq!(result.node_id, "node1");
        assert!(!result.success);
        assert!(result.error.is_some());
        assert!(result.samples.is_empty());
    }

    #[test]
    fn test_scrape_result_get_metric_existing() {
        let mut samples = MetricSample::new();
        samples.insert("cpu_usage".to_string(), 75.5);
        
        let result = ScrapeResult {
            node_id: "node1".to_string(),
            url: "http://node1:9400/metrics".to_string(),
            samples,
            scraped_at: 1234567890,
            duration_ms: 100,
            success: true,
            error: None,
        };
        
        assert_eq!(result.get_metric("cpu_usage"), Some(75.5));
    }

    #[test]
    fn test_scrape_result_get_metric_missing() {
        let samples = MetricSample::new();
        
        let result = ScrapeResult {
            node_id: "node1".to_string(),
            url: "http://node1:9400/metrics".to_string(),
            samples,
            scraped_at: 1234567890,
            duration_ms: 100,
            success: true,
            error: None,
        };
        
        assert_eq!(result.get_metric("nonexistent"), None);
    }

    #[test]
    fn test_scraper_pool_add_node() {
        let mut pool = ScraperPool::new(10);
        pool.add_node("node1".to_string(), "http://node1:9400/metrics".to_string());
        assert_eq!(pool.node_count(), 1);
    }

    #[test]
    fn test_scraper_pool_remove_node() {
        let mut pool = ScraperPool::new(10);
        pool.add_node("node1".to_string(), "http://node1:9400/metrics".to_string());
        pool.add_node("node2".to_string(), "http://node2:9400/metrics".to_string());
        
        pool.remove_node("node1");
        
        assert_eq!(pool.node_count(), 1);
    }

    #[tokio::test]
    async fn test_scraper_pool_latest_results_empty() {
        let pool = ScraperPool::new(10);
        let results = pool.latest_results().await;
        assert!(results.is_empty());
    }

    #[test]
    fn test_scraper_pool_multiple_nodes() {
        let mut pool = ScraperPool::new(10);
        pool.add_node("node1".to_string(), "http://node1:9400/metrics".to_string());
        pool.add_node("node2".to_string(), "http://node2:9400/metrics".to_string());
        pool.add_node("node3".to_string(), "http://node3:9400/metrics".to_string());
        
        assert_eq!(pool.node_count(), 3);
    }
}