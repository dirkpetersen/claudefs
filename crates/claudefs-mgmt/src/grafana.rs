use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrafanaPanel {
    pub id: u32,
    pub title: String,
    pub panel_type: PanelType,
    pub targets: Vec<PrometheusTarget>,
    pub grid_pos: GridPos,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PanelType {
    Timeseries,
    Gauge,
    Stat,
    Table,
    Heatmap,
    BarChart,
    PieChart,
    Timeline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrometheusTarget {
    pub expr: String,
    pub legend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridPos {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

pub fn generate_cluster_overview_dashboard() -> Value {
    json!({
        "title": "ClaudeFS Cluster Overview",
        "uid": "claudefs-cluster-overview",
        "schemaVersion": 38,
        "version": 1,
        "refresh": "15s",
        "time": {
            "from": "now-6h",
            "to": "now"
        },
        "panels": [
            {
                "id": 1,
                "title": "Cluster IOPS",
                "type": "timeseries",
                "gridPos": {
                    "x": 0,
                    "y": 0,
                    "w": 12,
                    "h": 8
                },
                "targets": [
                    {
                        "expr": "rate(claudefs_ops_read_total[5m])",
                        "legend": "Read IOPS"
                    },
                    {
                        "expr": "rate(claudefs_ops_write_total[5m])",
                        "legend": "Write IOPS"
                    }
                ]
            },
            {
                "id": 2,
                "title": "Bandwidth",
                "type": "timeseries",
                "gridPos": {
                    "x": 12,
                    "y": 0,
                    "w": 12,
                    "h": 8
                },
                "targets": [
                    {
                        "expr": "rate(claudefs_bytes_read_total[5m])",
                        "legend": "Read Bytes/s"
                    },
                    {
                        "expr": "rate(claudefs_bytes_written_total[5m])",
                        "legend": "Write Bytes/s"
                    }
                ]
            },
            {
                "id": 3,
                "title": "Capacity Utilization",
                "type": "gauge",
                "gridPos": {
                    "x": 0,
                    "y": 8,
                    "w": 6,
                    "h": 8
                },
                "targets": [
                    {
                        "expr": "claudefs_capacity_used_bytes / claudefs_capacity_total_bytes * 100",
                        "legend": "Used %"
                    }
                ],
                "fieldConfig": {
                    "defaults": {
                        "max": 100,
                        "min": 0,
                        "thresholds": {
                            "mode": "absolute",
                            "steps": [
                                {"color": "green", "value": null},
                                {"color": "yellow", "value": 70},
                                {"color": "red", "value": 90}
                            ]
                        }
                    }
                }
            },
            {
                "id": 4,
                "title": "Node Health",
                "type": "stat",
                "gridPos": {
                    "x": 6,
                    "y": 8,
                    "w": 6,
                    "h": 8
                },
                "targets": [
                    {
                        "expr": "claudefs_nodes_healthy",
                        "legend": "Healthy"
                    },
                    {
                        "expr": "claudefs_nodes_total",
                        "legend": "Total"
                    }
                ]
            },
            {
                "id": 5,
                "title": "Replication Lag",
                "type": "timeseries",
                "gridPos": {
                    "x": 12,
                    "y": 8,
                    "w": 12,
                    "h": 8
                },
                "targets": [
                    {
                        "expr": "claudefs_replication_lag_secs",
                        "legend": "Lag (s)"
                    }
                ],
                "fieldConfig": {
                    "defaults": {
                        "thresholds": {
                            "mode": "absolute",
                            "steps": [
                                {"color": "green", "value": null},
                                {"color": "red", "value": 60}
                            ]
                        }
                    }
                }
            },
            {
                "id": 6,
                "title": "Dedupe Ratio",
                "type": "stat",
                "gridPos": {
                    "x": 0,
                    "y": 16,
                    "w": 6,
                    "h": 6
                },
                "targets": [
                    {
                        "expr": "claudefs_dedupe_ratio",
                        "legend": "Ratio"
                    }
                ]
            },
            {
                "id": 7,
                "title": "Compression Ratio",
                "type": "stat",
                "gridPos": {
                    "x": 6,
                    "y": 16,
                    "w": 6,
                    "h": 6
                },
                "targets": [
                    {
                        "expr": "claudefs_compression_ratio",
                        "legend": "Ratio"
                    }
                ]
            },
            {
                "id": 8,
                "title": "Write Latency p99",
                "type": "timeseries",
                "gridPos": {
                    "x": 12,
                    "y": 16,
                    "w": 12,
                    "h": 6
                },
                "targets": [
                    {
                        "expr": "claudefs_latency_write_us_p99 / 1000",
                        "legend": "Latency (ms)"
                    }
                ]
            }
        ]
    })
}

pub fn generate_top_users_dashboard() -> Value {
    json!({
        "title": "ClaudeFS Top Users",
        "uid": "claudefs-top-users",
        "schemaVersion": 38,
        "version": 1,
        "refresh": "1m",
        "time": {
            "from": "now-24h",
            "to": "now"
        },
        "panels": [
            {
                "id": 1,
                "title": "Top Users by Storage",
                "type": "table",
                "gridPos": {
                    "x": 0,
                    "y": 0,
                    "w": 12,
                    "h": 10
                },
                "targets": [
                    {
                        "expr": "topk(20, claudefs_user_bytes_used)",
                        "legend": "User"
                    }
                ]
            },
            {
                "id": 2,
                "title": "Top Users Comparison",
                "type": "barchart",
                "gridPos": {
                    "x": 12,
                    "y": 0,
                    "w": 12,
                    "h": 10
                },
                "targets": [
                    {
                        "expr": "topk(20, claudefs_user_bytes_used)",
                        "legend": "{{user}}"
                    }
                ],
                "options": {
                    "orientation": "horizontal",
                    "showValue": "auto",
                    "groupBy": [],
                    "values": false
                }
            },
            {
                "id": 3,
                "title": "Top Groups by Storage",
                "type": "table",
                "gridPos": {
                    "x": 0,
                    "y": 10,
                    "w": 12,
                    "h": 10
                },
                "targets": [
                    {
                        "expr": "topk(20, claudefs_group_bytes_used)",
                        "legend": "Group"
                    }
                ]
            },
            {
                "id": 4,
                "title": "User File Count",
                "type": "table",
                "gridPos": {
                    "x": 12,
                    "y": 10,
                    "w": 12,
                    "h": 10
                },
                "targets": [
                    {
                        "expr": "topk(20, claudefs_user_files_total)",
                        "legend": "User"
                    }
                ]
            }
        ]
    })
}

pub fn generate_health_overview_dashboard() -> Value {
    json!({
        "title": "ClaudeFS Health Overview",
        "uid": "claudefs-health-overview-phase4b4",
        "schemaVersion": 38,
        "version": 1,
        "refresh": "30s",
        "time": {"from": "now-24h", "to": "now"},
        "panels": [
            {"id": 1, "title": "Cluster Health Score", "type": "gauge", "gridPos": {"x": 0, "y": 0, "w": 6, "h": 6}, "targets": [{"expr": "claudefs_cluster_health_score", "legend": "Health %"}], "fieldConfig": {"defaults": {"max": 100, "min": 0, "unit": "percent", "thresholds": {"mode": "absolute", "steps": [{"color": "red", "value": null}, {"color": "yellow", "value": 50}, {"color": "green", "value": 80}]}}}},
            {"id": 2, "title": "Node Status Grid", "type": "table", "gridPos": {"x": 6, "y": 0, "w": 18, "h": 10}, "targets": [{"expr": "claudefs_node_status", "legend": "Status"}]},
            {"id": 3, "title": "Recovery Actions", "type": "table", "gridPos": {"x": 0, "y": 10, "w": 12, "h": 8}, "targets": [{"expr": "claudefs_recovery_actions_total", "legend": "Actions"}]},
            {"id": 4, "title": "Backup Status", "type": "stat", "gridPos": {"x": 12, "y": 10, "w": 12, "h": 8}, "targets": [{"expr": "claudefs_backup_last_timestamp_seconds", "legend": "Last Backup"}, {"expr": "claudefs_backup_next_scheduled_timestamp_seconds", "legend": "Next Backup"}, {"expr": "claudefs_backup_success_rate", "legend": "Success Rate %"}]}
        ]
    })
}

pub fn generate_performance_trends_dashboard() -> Value {
    json!({
        "title": "ClaudeFS Performance Trends",
        "uid": "claudefs-performance-trends-phase4b4",
        "schemaVersion": 38,
        "version": 1,
        "refresh": "15s",
        "time": {"from": "now-1h", "to": "now"},
        "panels": [
            {"id": 1, "title": "IOPS Over Time", "type": "timeseries", "gridPos": {"x": 0, "y": 0, "w": 12, "h": 8}, "targets": [{"expr": "rate(claudefs_ops_read_total[5m])", "legend": "Read IOPS"}, {"expr": "rate(claudefs_ops_write_total[5m])", "legend": "Write IOPS"}]},
            {"id": 2, "title": "Latency Percentiles", "type": "timeseries", "gridPos": {"x": 12, "y": 0, "w": 12, "h": 8}, "targets": [{"expr": "claudefs_latency_p50_ms", "legend": "p50"}, {"expr": "claudefs_latency_p95_ms", "legend": "p95"}, {"expr": "claudefs_latency_p99_ms", "legend": "p99"}]},
            {"id": 3, "title": "Throughput (Bytes/s)", "type": "timeseries", "gridPos": {"x": 0, "y": 8, "w": 12, "h": 8}, "targets": [{"expr": "rate(claudefs_bytes_read_total[5m])", "legend": "Read"}, {"expr": "rate(claudefs_bytes_written_total[5m])", "legend": "Write"}]},
            {"id": 4, "title": "Cache Hit Rate", "type": "gauge", "gridPos": {"x": 12, "y": 8, "w": 6, "h": 8}, "targets": [{"expr": "claudefs_cache_hit_ratio * 100", "legend": "Hit Rate %"}], "fieldConfig": {"defaults": {"max": 100, "min": 0, "unit": "percent", "thresholds": {"mode": "absolute", "steps": [{"color": "red", "value": null}, {"color": "yellow", "value": 50}, {"color": "green", "value": 80}]}}}},
            {"id": 5, "title": "Query Latency", "type": "timeseries", "gridPos": {"x": 18, "y": 8, "w": 6, "h": 8}, "targets": [{"expr": "claudefs_metadata_query_latency_ms", "legend": "Metadata"}, {"expr": "claudefs_data_query_latency_ms", "legend": "Data"}]}
        ]
    })
}

pub fn generate_capacity_planning_dashboard() -> Value {
    json!({
        "title": "ClaudeFS Capacity Planning",
        "uid": "claudefs-capacity-planning-phase4b4",
        "schemaVersion": 38,
        "version": 1,
        "refresh": "60s",
        "time": {"from": "now-30d", "to": "now"},
        "panels": [
            {"id": 1, "title": "Storage Utilization", "type": "gauge", "gridPos": {"x": 0, "y": 0, "w": 6, "h": 8}, "targets": [{"expr": "claudefs_storage_utilization_percent", "legend": "Utilization %"}], "fieldConfig": {"defaults": {"max": 100, "min": 0, "unit": "percent", "thresholds": {"mode": "absolute", "steps": [{"color": "green", "value": null}, {"color": "yellow", "value": 70}, {"color": "red", "value": 85}]}}}},
            {"id": 2, "title": "Flash vs S3 Distribution", "type": "piechart", "gridPos": {"x": 6, "y": 0, "w": 9, "h": 8}, "targets": [{"expr": "claudefs_flash_used_bytes", "legend": "Flash"}, {"expr": "claudefs_s3_used_bytes", "legend": "S3"}], "options": {"pieType": "pie", "displayLabels": ["percent"], "legend": {"displayMode": "list", "placement": "right"}}},
            {"id": 3, "title": "Capacity Forecast", "type": "timeseries", "gridPos": {"x": 15, "y": 0, "w": 9, "h": 8}, "targets": [{"expr": "claudefs_storage_utilized_bytes", "legend": "Current"}]},
            {"id": 4, "title": "Ingest Rate", "type": "stat", "gridPos": {"x": 0, "y": 8, "w": 6, "h": 6}, "targets": [{"expr": "rate(claudefs_bytes_ingested_total[5m])", "legend": "Ingest"}]},
            {"id": 5, "title": "Eviction Rate", "type": "stat", "gridPos": {"x": 6, "y": 8, "w": 6, "h": 6}, "targets": [{"expr": "rate(claudefs_bytes_evicted_to_s3_total[5m])", "legend": "Eviction"}]},
            {"id": 6, "title": "Days Until Full", "type": "stat", "gridPos": {"x": 12, "y": 8, "w": 6, "h": 6}, "targets": [{"expr": "(100 - claudefs_storage_utilization_percent) / (rate(claudefs_bytes_ingested_total[5m]) * 86400 / claudefs_storage_total_bytes)", "legend": "Days"}], "fieldConfig": {"defaults": {"thresholds": {"mode": "absolute", "steps": [{"color": "red", "value": null}, {"color": "yellow", "value": 7}, {"color": "green", "value": 14}]}}}},
            {"id": 7, "title": "Tiering Effectiveness", "type": "barchart", "gridPos": {"x": 0, "y": 14, "w": 24, "h": 8}, "targets": [{"expr": "claudefs_s3_cache_hits_total", "legend": "S3 Hits"}, {"expr": "claudefs_flash_hits_total", "legend": "Flash Hits"}], "options": {"orientation": "auto", "showValue": "auto", "groupBy": [], "values": false}}
        ]
    })
}

pub fn generate_alerts_dashboard() -> Value {
    json!({
        "title": "ClaudeFS Alerts & Issues",
        "uid": "claudefs-alerts-issues-phase4b4",
        "schemaVersion": 38,
        "version": 1,
        "refresh": "10s",
        "time": {"from": "now-7d", "to": "now"},
        "panels": [
            {"id": 1, "title": "Active Alerts", "type": "table", "gridPos": {"x": 0, "y": 0, "w": 24, "h": 8}, "targets": [{"expr": "claudefs_active_alerts", "legend": "Alerts"}]},
            {"id": 2, "title": "Alert Timeline", "type": "barchart", "gridPos": {"x": 0, "y": 8, "w": 12, "h": 8}, "targets": [{"expr": "claudefs_alerts_total", "legend": "Alerts"}], "options": {"orientation": "horizontal", "showValue": "auto", "groupBy": [], "values": false}},
            {"id": 3, "title": "Alert Types Distribution", "type": "piechart", "gridPos": {"x": 12, "y": 8, "w": 12, "h": 8}, "targets": [{"expr": "claudefs_alerts_infrastructure_total", "legend": "Infrastructure"}, {"expr": "claudefs_alerts_performance_total", "legend": "Performance"}, {"expr": "claudefs_alerts_capacity_total", "legend": "Capacity"}, {"expr": "claudefs_alerts_cost_total", "legend": "Cost"}, {"expr": "claudefs_alerts_recovery_total", "legend": "Recovery"}], "options": {"pieType": "pie", "displayLabels": ["percent"], "legend": {"displayMode": "list", "placement": "right"}}},
            {"id": 4, "title": "Recovery Correlation", "type": "table", "gridPos": {"x": 0, "y": 16, "w": 24, "h": 8}, "targets": [{"expr": "claudefs_recovery_correlation", "legend": "Correlation"}]}
        ]
    })
}

pub fn all_dashboards() -> Vec<Value> {
    vec![
        generate_cluster_overview_dashboard(),
        generate_top_users_dashboard(),
        generate_health_overview_dashboard(),
        generate_performance_trends_dashboard(),
        generate_capacity_planning_dashboard(),
        generate_alerts_dashboard(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_cluster_overview_dashboard_title() {
        let dashboard = generate_cluster_overview_dashboard();
        assert_eq!(dashboard["title"], "ClaudeFS Cluster Overview");
    }

    #[test]
    fn test_cluster_overview_uid() {
        let dashboard = generate_cluster_overview_dashboard();
        assert_eq!(dashboard["uid"], "claudefs-cluster-overview");
    }

    #[test]
    fn test_cluster_overview_has_panels() {
        let dashboard = generate_cluster_overview_dashboard();
        assert!(dashboard.get("panels").is_some());
        let panels = dashboard["panels"].as_array().unwrap();
        assert!(panels.len() >= 4);
    }

    #[test]
    fn test_cluster_overview_panel_count() {
        let dashboard = generate_cluster_overview_dashboard();
        let panels = dashboard["panels"].as_array().unwrap();
        assert!(panels.len() >= 4);
    }

    #[test]
    fn test_generate_top_users_dashboard_title() {
        let dashboard = generate_top_users_dashboard();
        assert_eq!(dashboard["title"], "ClaudeFS Top Users");
    }

    #[test]
    fn test_top_users_uid() {
        let dashboard = generate_top_users_dashboard();
        assert_eq!(dashboard["uid"], "claudefs-top-users");
    }

    #[test]
    fn test_all_dashboards_returns_6() {
        let dashboards = all_dashboards();
        assert_eq!(dashboards.len(), 6);
    }

    #[test]
    fn test_dashboard_has_required_fields() {
        let dashboard = generate_cluster_overview_dashboard();

        assert!(dashboard.get("title").is_some());
        assert!(dashboard.get("uid").is_some());
        assert!(dashboard.get("panels").is_some());
        assert!(dashboard.get("schemaVersion").is_some());
    }

    #[test]
    fn test_panel_json_serialization() {
        let panel = GrafanaPanel {
            id: 1,
            title: "Test Panel".to_string(),
            panel_type: PanelType::Timeseries,
            targets: vec![PrometheusTarget {
                expr: "metric".to_string(),
                legend: "legend".to_string(),
            }],
            grid_pos: GridPos {
                x: 0,
                y: 0,
                w: 12,
                h: 8,
            },
        };

        let json = serde_json::to_string(&panel).unwrap();
        let parsed: GrafanaPanel = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.id, 1);
        assert_eq!(parsed.panel_type, PanelType::Timeseries);
    }

    #[test]
    fn test_prometheus_target_json_serialization() {
        let target = PrometheusTarget {
            expr: "rate(test_metric[5m])".to_string(),
            legend: "Test Metric".to_string(),
        };

        let json = serde_json::to_string(&target).unwrap();
        let parsed: PrometheusTarget = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.expr, "rate(test_metric[5m])");
        assert_eq!(parsed.legend, "Test Metric");
    }

    #[test]
    fn test_top_users_has_panels() {
        let dashboard = generate_top_users_dashboard();
        assert!(dashboard.get("panels").is_some());
    }

    #[test]
    fn test_health_dashboard_valid_json() {
        let dashboard = generate_health_overview_dashboard();
        assert!(dashboard.get("title").is_some());
        assert!(dashboard.get("uid").is_some());
        assert!(dashboard.get("panels").is_some());
        assert!(dashboard.get("schemaVersion").is_some());
    }

    #[test]
    fn test_health_dashboard_panel_count() {
        let dashboard = generate_health_overview_dashboard();
        let panels = dashboard["panels"].as_array().unwrap();
        assert_eq!(panels.len(), 4);
    }

    #[test]
    fn test_performance_dashboard_valid_json() {
        let dashboard = generate_performance_trends_dashboard();
        assert!(dashboard.get("title").is_some());
        assert!(dashboard.get("uid").is_some());
        assert!(dashboard.get("panels").is_some());
        assert!(dashboard.get("schemaVersion").is_some());
    }

    #[test]
    fn test_performance_dashboard_panel_count() {
        let dashboard = generate_performance_trends_dashboard();
        let panels = dashboard["panels"].as_array().unwrap();
        assert_eq!(panels.len(), 5);
    }

    #[test]
    fn test_capacity_dashboard_valid_json() {
        let dashboard = generate_capacity_planning_dashboard();
        assert!(dashboard.get("title").is_some());
        assert!(dashboard.get("uid").is_some());
        assert!(dashboard.get("panels").is_some());
        assert!(dashboard.get("schemaVersion").is_some());
    }

    #[test]
    fn test_capacity_dashboard_panel_count() {
        let dashboard = generate_capacity_planning_dashboard();
        let panels = dashboard["panels"].as_array().unwrap();
        assert_eq!(panels.len(), 7);
    }

    #[test]
    fn test_alerts_dashboard_valid_json() {
        let dashboard = generate_alerts_dashboard();
        assert!(dashboard.get("title").is_some());
        assert!(dashboard.get("uid").is_some());
        assert!(dashboard.get("panels").is_some());
        assert!(dashboard.get("schemaVersion").is_some());
    }

    #[test]
    fn test_alerts_dashboard_panel_count() {
        let dashboard = generate_alerts_dashboard();
        let panels = dashboard["panels"].as_array().unwrap();
        assert_eq!(panels.len(), 4);
    }
}
