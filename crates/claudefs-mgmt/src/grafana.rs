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

pub fn all_dashboards() -> Vec<Value> {
    vec![
        generate_cluster_overview_dashboard(),
        generate_top_users_dashboard(),
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
    fn test_all_dashboards_returns_2() {
        let dashboards = all_dashboards();
        assert_eq!(dashboards.len(), 2);
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
}
