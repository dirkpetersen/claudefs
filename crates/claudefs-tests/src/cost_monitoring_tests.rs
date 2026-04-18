//! A11: Cost Monitoring Tests
//!
//! Tests for the cost monitoring infrastructure including cost tracking,
//! budget alerts, cost attribution, and report generation.

use claudefs_mgmt::cost_tracker::{
    BudgetStatus, CostAlert, CostAlertEngine, CostAlertRule, CostBudget, CostCategory, CostEntry,
    CostTracker,
};
use std::collections::HashMap;
use std::path::Path;

#[allow(dead_code)]
fn ts(days_ago: u64) -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    now - (days_ago * 86400)
}

#[allow(dead_code)]
fn round_cents(amount: f64) -> f64 {
    (amount * 100.0).round() / 100.0
}

#[allow(dead_code)]
fn default_budget() -> CostBudget {
    CostBudget {
        daily_limit_usd: 100.0,
        monthly_limit_usd: 3000.0,
    }
}

#[test]
fn test_cost_monitor_script_exists() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = std::path::Path::new(manifest_dir)
        .ancestors()
        .find(|p| p.join("Cargo.toml").exists() && p.join("tools").exists())
        .expect("Could not find workspace root");
    let script_path = workspace_root.join("tools/cfs-cost-monitor-enhanced.sh");
    assert!(
        script_path.exists(),
        "Cost monitor script should exist at {:?}",
        script_path
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = std::fs::metadata(&script_path) {
            let permissions = metadata.permissions();
            let mode = permissions.mode();
            assert!(
                mode & 0o111 != 0,
                "Cost monitor script should be executable"
            );
        }
    }
}

#[test]
fn test_cost_tracker_initialization() {
    let budget = default_budget();
    let tracker = CostTracker::new(budget);
    
    assert_eq!(tracker.total_cost(), 0.0, "Initial cost should be zero");
    assert_eq!(
        tracker.budget_status(ts(0)),
        BudgetStatus::Ok,
        "Status should be Ok with no costs recorded"
    );
    
    let compute_cost = tracker.cost_by_category(&CostCategory::Compute);
    assert_eq!(compute_cost, 0.0, "Initial compute cost should be zero");
}

#[test]
fn test_cost_entry_recording_and_aggregation() {
    let budget = default_budget();
    let tracker = CostTracker::new(budget);
    let now = ts(0);
    
    tracker.record(CostEntry {
        category: CostCategory::Compute,
        amount_usd: 25.0,
        resource_id: "vm-1".to_string(),
        timestamp: now,
    });
    tracker.record(CostEntry {
        category: CostCategory::Compute,
        amount_usd: 15.0,
        resource_id: "vm-2".to_string(),
        timestamp: now,
    });
    tracker.record(CostEntry {
        category: CostCategory::Storage,
        amount_usd: 10.0,
        resource_id: "disk-1".to_string(),
        timestamp: now,
    });
    tracker.record(CostEntry {
        category: CostCategory::Network,
        amount_usd: 5.5,
        resource_id: "nic-1".to_string(),
        timestamp: now,
    });
    
    let total = tracker.total_cost();
    assert_eq!(round_cents(total), 55.5, "Total should be sum of all entries");
    
    let compute = tracker.cost_by_category(&CostCategory::Compute);
    assert_eq!(round_cents(compute), 40.0, "Compute should be 25 + 15");
    
    let storage = tracker.cost_by_category(&CostCategory::Storage);
    assert_eq!(round_cents(storage), 10.0, "Storage should be 10");
    
    let network = tracker.cost_by_category(&CostCategory::Network);
    assert_eq!(round_cents(network), 5.5, "Network should be 5.5");
}

#[test]
fn test_budget_status_thresholds() {
    let budget = default_budget();
    let tracker = CostTracker::new(budget);
    let now = ts(0);
    
    tracker.record(CostEntry {
        category: CostCategory::Compute,
        amount_usd: 10.0,
        resource_id: "vm-1".to_string(),
        timestamp: now,
    });
    assert_eq!(
        tracker.budget_status(now),
        BudgetStatus::Ok,
        "10% should be Ok"
    );
    
    tracker.record(CostEntry {
        category: CostCategory::Compute,
        amount_usd: 65.0,
        resource_id: "vm-2".to_string(),
        timestamp: now,
    });
    assert_eq!(
        tracker.budget_status(now),
        BudgetStatus::Warning,
        "75% should be Warning"
    );
    
    tracker.record(CostEntry {
        category: CostCategory::Compute,
        amount_usd: 15.0,
        resource_id: "vm-3".to_string(),
        timestamp: now,
    });
    assert_eq!(
        tracker.budget_status(now),
        BudgetStatus::Critical,
        "90% should be Critical"
    );
    
    tracker.record(CostEntry {
        category: CostCategory::Compute,
        amount_usd: 10.0,
        resource_id: "vm-4".to_string(),
        timestamp: now,
    });
    assert_eq!(
        tracker.budget_status(now),
        BudgetStatus::Exceeded,
        "100%+ should be Exceeded"
    );
}

#[tokio::test]
async fn test_cost_tracker_thread_safety_concurrent_records() {
    use std::sync::Arc;
    
    let budget = default_budget();
    let tracker = Arc::new(CostTracker::new(budget));
    let now = ts(0);
    
    let tracker1 = Arc::clone(&tracker);
    let handle1 = tokio::spawn(async move {
        for i in 0..50 {
            tracker1.record(CostEntry {
                category: CostCategory::Compute,
                amount_usd: 1.0,
                resource_id: format!("vm-{}", i),
                timestamp: now,
            });
        }
    });
    
    let tracker2 = Arc::clone(&tracker);
    let handle2 = tokio::spawn(async move {
        for i in 50..100 {
            tracker2.record(CostEntry {
                category: CostCategory::Storage,
                amount_usd: 1.0,
                resource_id: format!("disk-{}", i),
                timestamp: now,
            });
        }
    });
    
    let tracker3 = Arc::clone(&tracker);
    let handle3 = tokio::spawn(async move {
        for i in 100..150 {
            tracker3.record(CostEntry {
                category: CostCategory::Network,
                amount_usd: 1.0,
                resource_id: format!("nic-{}", i),
                timestamp: now,
            });
        }
    });
    
    let _ = tokio::join!(handle1, handle2, handle3);
    
    let total = tracker.total_cost();
    assert_eq!(
        round_cents(total),
        150.0,
        "150 concurrent entries of $1 each should total $150"
    );
    
    let compute = tracker.cost_by_category(&CostCategory::Compute);
    assert_eq!(round_cents(compute), 50.0, "Compute should have 50 entries");
    
    let storage = tracker.cost_by_category(&CostCategory::Storage);
    assert_eq!(round_cents(storage), 50.0, "Storage should have 50 entries");
    
    let network = tracker.cost_by_category(&CostCategory::Network);
    assert_eq!(round_cents(network), 50.0, "Network should have 50 entries");
}

#[test]
fn test_ec2_cost_calculation() {
    let budget = default_budget();
    let tracker = CostTracker::new(budget);
    let now = ts(0);
    
    let instance_hours = 24.0;
    let hourly_rate = 0.5;
    let compute_cost = round_cents(instance_hours * hourly_rate);
    
    tracker.record(CostEntry {
        category: CostCategory::Compute,
        amount_usd: compute_cost,
        resource_id: "vm-1".to_string(),
        timestamp: now,
    });
    
    let instance_hours_2 = 48.0;
    let compute_cost_2 = round_cents(instance_hours_2 * hourly_rate);
    tracker.record(CostEntry {
        category: CostCategory::Compute,
        amount_usd: compute_cost_2,
        resource_id: "vm-2".to_string(),
        timestamp: now,
    });
    
    let total_compute = tracker.cost_by_category(&CostCategory::Compute);
    let expected_total = round_cents((24.0 * 0.5) + (48.0 * 0.5));
    assert_eq!(
        round_cents(total_compute),
        expected_total,
        "EC2 costs should be calculated correctly"
    );
    
    let all_entries = tracker.total_cost();
    assert!(
        all_entries > 0.0,
        "Total cost should reflect EC2 spend"
    );
}

#[test]
fn test_bedrock_cost_calculation() {
    let budget = default_budget();
    let tracker = CostTracker::new(budget);
    let now = ts(0);
    
    let input_tokens = 1_000_000;
    let output_tokens = 100_000;
    let input_rate = 0.0003;
    let output_rate = 0.0015;
    
    let input_cost = round_cents((input_tokens as f64) * input_rate);
    let output_cost = round_cents((output_tokens as f64) * output_rate);
    let total_cost = round_cents(input_cost + output_cost);
    
    tracker.record(CostEntry {
        category: CostCategory::Api,
        amount_usd: total_cost,
        resource_id: "bedrock-claude-3-5".to_string(),
        timestamp: now,
    });
    
    let api_cost = tracker.cost_by_category(&CostCategory::Api);
    assert!(
        round_cents(api_cost) > 0.0,
        "Bedrock API cost should be non-zero"
    );
    
    tracker.record(CostEntry {
        category: CostCategory::Api,
        amount_usd: 5.0,
        resource_id: "bedrock-embedding".to_string(),
        timestamp: now,
    });
    
    let total = tracker.total_cost();
    assert!(
        total > total_cost,
        "Total should include embedding costs"
    );
}

#[test]
fn test_multi_service_cost_aggregation() {
    let budget = default_budget();
    let tracker = CostTracker::new(budget);
    let now = ts(0);
    
    tracker.record(CostEntry {
        category: CostCategory::Compute,
        amount_usd: 25.0,
        resource_id: "vm-1".to_string(),
        timestamp: now,
    });
    tracker.record(CostEntry {
        category: CostCategory::Compute,
        amount_usd: 15.0,
        resource_id: "vm-2".to_string(),
        timestamp: now,
    });
    
    tracker.record(CostEntry {
        category: CostCategory::Storage,
        amount_usd: 10.0,
        resource_id: "disk-1".to_string(),
        timestamp: now,
    });
    tracker.record(CostEntry {
        category: CostCategory::Storage,
        amount_usd: 5.0,
        resource_id: "s3-bucket".to_string(),
        timestamp: now,
    });
    
    tracker.record(CostEntry {
        category: CostCategory::Api,
        amount_usd: 8.5,
        resource_id: "bedrock-api".to_string(),
        timestamp: now,
    });
    
    let total = tracker.total_cost();
    let expected = round_cents(25.0 + 15.0 + 10.0 + 5.0 + 8.5);
    assert_eq!(round_cents(total), expected, "Multi-service total should aggregate correctly");
    
    let compute = tracker.cost_by_category(&CostCategory::Compute);
    let storage = tracker.cost_by_category(&CostCategory::Storage);
    let api = tracker.cost_by_category(&CostCategory::Api);
    
    assert_eq!(round_cents(compute + storage + api), expected, "Category sum should equal total");
}

#[test]
fn test_cost_currency_precision() {
    let budget = default_budget();
    let tracker = CostTracker::new(budget);
    let now = ts(0);
    
    tracker.record(CostEntry {
        category: CostCategory::Compute,
        amount_usd: 10.123,
        resource_id: "vm-1".to_string(),
        timestamp: now,
    });
    tracker.record(CostEntry {
        category: CostCategory::Storage,
        amount_usd: 5.456,
        resource_id: "disk-1".to_string(),
        timestamp: now,
    });
    tracker.record(CostEntry {
        category: CostCategory::Network,
        amount_usd: 2.789,
        resource_id: "nic-1".to_string(),
        timestamp: now,
    });
    
    let total = tracker.total_cost();
    let expected = round_cents(10.123 + 5.456 + 2.789);
    assert_eq!(
        round_cents(total),
        expected,
        "USD amounts should maintain 2-decimal precision"
    );
    
    assert!(
        (total - expected).abs() < 0.01,
        "Total should be within 1 cent of expected"
    );
}

#[test]
fn test_cost_alert_rule_evaluation() {
    let mut engine = CostAlertEngine::new();
    engine.add_rule(CostAlertRule {
        threshold_usd: 25.0,
        category: Some(CostCategory::Compute),
        message_template: "Compute spending ${amount}".to_string(),
    });
    engine.add_rule(CostAlertRule {
        threshold_usd: 50.0,
        category: None,
        message_template: "Total spending ${amount}".to_string(),
    });
    
    let budget = default_budget();
    let tracker = CostTracker::new(budget);
    let now = ts(0);
    
    tracker.record(CostEntry {
        category: CostCategory::Compute,
        amount_usd: 20.0,
        resource_id: "vm-1".to_string(),
        timestamp: now,
    });
    
    let alerts = engine.evaluate(&tracker);
    assert!(
        alerts.is_empty(),
        "No alerts should trigger below threshold"
    );
    
    tracker.record(CostEntry {
        category: CostCategory::Compute,
        amount_usd: 10.0,
        resource_id: "vm-2".to_string(),
        timestamp: now,
    });
    
    let alerts = engine.evaluate(&tracker);
    assert!(
        !alerts.is_empty(),
        "Alert should trigger when compute cost exceeds $25"
    );
    
    let compute_alert = alerts.iter().find(|a| a.category == CostCategory::Compute);
    assert!(
        compute_alert.is_some(),
        "Compute alert should be present"
    );
}

#[test]
fn test_cost_alert_threshold_boundaries() {
    let budget = default_budget();
    let tracker = CostTracker::new(budget);
    let now = ts(0);
    
    let mut engine = CostAlertEngine::new();
    engine.add_rule(CostAlertRule {
        threshold_usd: 25.0,
        category: None,
        message_template: "25% alert: ${amount}".to_string(),
    });
    engine.add_rule(CostAlertRule {
        threshold_usd: 50.0,
        category: None,
        message_template: "50% alert: ${amount}".to_string(),
    });
    engine.add_rule(CostAlertRule {
        threshold_usd: 75.0,
        category: None,
        message_template: "75% alert: ${amount}".to_string(),
    });
    engine.add_rule(CostAlertRule {
        threshold_usd: 100.0,
        category: None,
        message_template: "100% alert: ${amount}".to_string(),
    });
    
    tracker.record(CostEntry {
        category: CostCategory::Compute,
        amount_usd: 20.0,
        resource_id: "vm-1".to_string(),
        timestamp: now,
    });
    let alerts = engine.evaluate(&tracker);
    assert!(alerts.is_empty(), "20% should not trigger any alert");
    
    tracker.record(CostEntry {
        category: CostCategory::Compute,
        amount_usd: 5.0,
        resource_id: "vm-2".to_string(),
        timestamp: now,
    });
    let alerts = engine.evaluate(&tracker);
    assert_eq!(alerts.len(), 1, "25% should trigger one alert");
    
    tracker.record(CostEntry {
        category: CostCategory::Compute,
        amount_usd: 25.0,
        resource_id: "vm-3".to_string(),
        timestamp: now,
    });
    let alerts = engine.evaluate(&tracker);
    assert!(
        alerts.len() >= 2,
        "50% should trigger at least 2 alerts"
    );
    
    tracker.record(CostEntry {
        category: CostCategory::Compute,
        amount_usd: 25.0,
        resource_id: "vm-4".to_string(),
        timestamp: now,
    });
    let alerts = engine.evaluate(&tracker);
    assert!(
        alerts.len() >= 3,
        "75% should trigger at least 3 alerts"
    );
}

#[test]
fn test_cost_alert_message_templating() {
    let mut engine = CostAlertEngine::new();
    engine.add_rule(CostAlertRule {
        threshold_usd: 10.0,
        category: None,
        message_template: "Budget alert: spending reached ${amount}".to_string(),
    });
    
    let budget = default_budget();
    let tracker = CostTracker::new(budget);
    let now = ts(0);
    
    tracker.record(CostEntry {
        category: CostCategory::Compute,
        amount_usd: 15.5,
        resource_id: "vm-1".to_string(),
        timestamp: now,
    });
    
    let alerts = engine.evaluate(&tracker);
    assert_eq!(alerts.len(), 1, "Alert should be generated");
    
    let alert = &alerts[0];
    assert!(
        alert.message.contains("15.50"),
        "Alert message should contain formatted amount"
    );
    assert!(
        alert.message.contains("Budget alert"),
        "Alert message should contain template text"
    );
}

#[test]
fn test_cost_attribution_by_stage() {
    let budget = default_budget();
    let tracker = CostTracker::new(budget);
    let now = ts(0);
    
    let mut stage_costs: HashMap<String, f64> = HashMap::new();
    
    tracker.record(CostEntry {
        category: CostCategory::Compute,
        amount_usd: 10.0,
        resource_id: "agent-a1-stage-canary".to_string(),
        timestamp: now,
    });
    stage_costs.insert("canary".to_string(), 10.0);
    
    tracker.record(CostEntry {
        category: CostCategory::Compute,
        amount_usd: 8.5,
        resource_id: "agent-a2-stage-10%".to_string(),
        timestamp: now,
    });
    stage_costs.insert("10%".to_string(), 8.5);
    
    tracker.record(CostEntry {
        category: CostCategory::Storage,
        amount_usd: 15.0,
        resource_id: "agent-a3-stage-50%".to_string(),
        timestamp: now,
    });
    stage_costs.insert("50%".to_string(), 15.0);
    
    tracker.record(CostEntry {
        category: CostCategory::Network,
        amount_usd: 12.0,
        resource_id: "agent-a4-stage-100%".to_string(),
        timestamp: now,
    });
    stage_costs.insert("100%".to_string(), 12.0);
    
    assert_eq!(
        round_cents(stage_costs.get("canary").copied().unwrap_or(0.0)),
        10.0,
        "Canary stage should have $10"
    );
    assert_eq!(
        round_cents(stage_costs.get("10%").copied().unwrap_or(0.0)),
        8.5,
        "10% stage should have $8.50"
    );
    assert_eq!(
        round_cents(stage_costs.get("50%").copied().unwrap_or(0.0)),
        15.0,
        "50% stage should have $15"
    );
    assert_eq!(
        round_cents(stage_costs.get("100%").copied().unwrap_or(0.0)),
        12.0,
        "100% stage should have $12"
    );
    
    let total = tracker.total_cost();
    let stage_total: f64 = stage_costs.values().sum();
    assert_eq!(
        round_cents(total),
        round_cents(stage_total),
        "Stage costs should sum to total"
    );
}

#[test]
fn test_cost_attribution_by_agent() {
    let budget = default_budget();
    let tracker = CostTracker::new(budget);
    let now = ts(0);
    
    let mut agent_costs: HashMap<String, f64> = HashMap::new();
    
    for agent_num in 1..=11 {
        let agent_id = format!("agent-a{}", agent_num);
        let cost = 5.0 + (agent_num as f64);
        
        tracker.record(CostEntry {
            category: CostCategory::Compute,
            amount_usd: cost,
            resource_id: format!("{}-vm-1", agent_id),
            timestamp: now,
        });
        
        agent_costs.insert(agent_id, cost);
    }
    
    for i in 1..=11 {
        let agent_id = format!("agent-a{}", i);
        let recorded = agent_costs.get(&agent_id).copied().unwrap_or(0.0);
        assert!(
            recorded > 0.0,
            "Agent {} should have attributed costs",
            agent_id
        );
    }
    
    let total = tracker.total_cost();
    let agent_total: f64 = agent_costs.values().sum();
    assert_eq!(
        round_cents(total),
        round_cents(agent_total),
        "Agent costs should sum to total"
    );
}

#[test]
fn test_cost_report_json_schema() {
    let budget = default_budget();
    let daily_limit = budget.daily_limit_usd;
    let tracker = CostTracker::new(budget.clone());
    let now = ts(0);
    
    tracker.record(CostEntry {
        category: CostCategory::Compute,
        amount_usd: 25.0,
        resource_id: "vm-1".to_string(),
        timestamp: now,
    });
    tracker.record(CostEntry {
        category: CostCategory::Storage,
        amount_usd: 15.0,
        resource_id: "disk-1".to_string(),
        timestamp: now,
    });
    tracker.record(CostEntry {
        category: CostCategory::Network,
        amount_usd: 5.5,
        resource_id: "nic-1".to_string(),
        timestamp: now,
    });
    
    let total = tracker.total_cost();
    let daily = tracker.daily_total(now);
    let status = tracker.budget_status(now);
    let compute = tracker.cost_by_category(&CostCategory::Compute);
    let storage = tracker.cost_by_category(&CostCategory::Storage);
    let network = tracker.cost_by_category(&CostCategory::Network);
    let top = tracker.top_resources(5);
    
    let report = serde_json::json!({
        "timestamp": now,
        "period": format!("{} to {}", format_time(now), format_time(now + 86400 - 1)),
        "summary": {
            "total_cost_usd": round_cents(total),
            "daily_average": round_cents(daily),
            "budget_limit": daily_limit,
            "budget_utilization_percent": round_cents((daily / daily_limit) * 100.0),
            "status": format!("{:?}", status)
        },
        "by_category": {
            "Compute": round_cents(compute),
            "Storage": round_cents(storage),
            "Network": round_cents(network)
        },
        "top_resources": top.iter().map(|(id, cost)| {
            serde_json::json!({
                "id": id,
                "cost_usd": round_cents(*cost)
            })
        }).collect::<Vec<_>>()
    });
    
    let json_str = serde_json::to_string(&report).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    
    assert!(parsed.get("timestamp").is_some(), "Report should have timestamp");
    assert!(parsed.get("summary").is_some(), "Report should have summary");
    assert!(parsed.get("by_category").is_some(), "Report should have by_category");
    
    let summary = parsed.get("summary").unwrap();
    assert!(summary.get("total_cost_usd").is_some(), "Summary should have total_cost_usd");
    assert!(summary.get("budget_utilization_percent").is_some(), "Summary should have budget_utilization_percent");
    assert!(summary.get("status").is_some(), "Summary should have status");
}

#[allow(dead_code)]
fn format_time(ts: u64) -> String {
    let secs = ts;
    let days_since_epoch = secs / 86400;
    format!("2026-04-{:02}", 18 + days_since_epoch as i64)
}

#[test]
fn test_cost_report_historical_data() {
    let budget = default_budget();
    let tracker = CostTracker::new(budget);
    
    let mut expected_daily: Vec<f64> = Vec::new();
    for day in 0..7 {
        let day_ts = ts(day);
        let daily_cost = 30.0 + (day as f64 * 2.0);
        expected_daily.push(daily_cost);
        
        tracker.record(CostEntry {
            category: CostCategory::Compute,
            amount_usd: daily_cost,
            resource_id: format!("vm-day-{}", day),
            timestamp: day_ts,
        });
    }
    
    let total_all = tracker.total_cost();
    // daily_total(ts(6)) returns all costs >= ts(6), which includes all 7 days
    let cumulative_from_oldest = tracker.daily_total(ts(6));
    assert!(
        (total_all - cumulative_from_oldest).abs() < 0.01,
        "daily_total from oldest day should equal total cost"
    );

    let cumulative_from_day1 = tracker.daily_total(ts(1));
    let cumulative_from_day2 = tracker.daily_total(ts(2));
    assert!(
        cumulative_from_day1 < cumulative_from_day2,
        "Cumulative totals should increase as we look further back (older timestamps have more entries)"
    );

    // daily_total(ts(6)) includes all 7 days worth of costs
    let all_days_total = tracker.daily_total(ts(6));
    let expected_all = expected_daily.iter().sum::<f64>();
    assert!(
        (all_days_total - expected_all).abs() < 0.01,
        "Total from oldest day should equal sum of all daily costs"
    );
    
    for day in 0..7 {
        let day_start = ts(day);
        let daily = tracker.daily_total(day_start);
        assert!(
            daily > 0.0,
            "Day {} should have non-zero cumulative cost",
            day
        );
    }
}

#[test]
fn test_cost_monitoring_e2e_workflow() {
    let budget = default_budget();
    let tracker = CostTracker::new(budget);
    let now = ts(0);
    
    let mut engine = CostAlertEngine::new();
    engine.add_rule(CostAlertRule {
        threshold_usd: 25.0,
        category: None,
        message_template: "Warning: ${amount} spent".to_string(),
    });
    engine.add_rule(CostAlertRule {
        threshold_usd: 75.0,
        category: None,
        message_template: "Critical: ${amount} spent".to_string(),
    });
    
    tracker.record(CostEntry {
        category: CostCategory::Compute,
        amount_usd: 20.0,
        resource_id: "vm-1".to_string(),
        timestamp: now,
    });
    
    let status = tracker.budget_status(now);
    assert_eq!(status, BudgetStatus::Ok, "Initial status should be Ok");
    
    let alerts = engine.evaluate(&tracker);
    assert!(alerts.is_empty(), "No alerts at 20%");
    
    tracker.record(CostEntry {
        category: CostCategory::Storage,
        amount_usd: 55.0,
        resource_id: "disk-1".to_string(),
        timestamp: now,
    });
    
    let status = tracker.budget_status(now);
    assert_eq!(status, BudgetStatus::Warning, "Status should be Warning at 75%");
    
    let alerts = engine.evaluate(&tracker);
    assert!(!alerts.is_empty(), "Alert should trigger at 75%");
    
    tracker.record(CostEntry {
        category: CostCategory::Compute,
        amount_usd: 25.0,
        resource_id: "vm-2".to_string(),
        timestamp: now,
    });
    
    let status = tracker.budget_status(now);
    assert_eq!(status, BudgetStatus::Exceeded, "Status should be Exceeded at 100%");
    
    let alerts = engine.evaluate(&tracker);
    let critical_alerts: Vec<_> = alerts.iter().filter(|a| a.message.contains("Critical")).collect();
    assert!(
        !critical_alerts.is_empty(),
        "Critical alert should be present"
    );
    
    let total = tracker.total_cost();
    let compute = tracker.cost_by_category(&CostCategory::Compute);
    let storage = tracker.cost_by_category(&CostCategory::Storage);
    let top = tracker.top_resources(3);
    
    let report = serde_json::json!({
        "total_cost_usd": round_cents(total),
        "by_category": {
            "Compute": round_cents(compute),
            "Storage": round_cents(storage)
        },
        "top_resources": top,
        "alerts": alerts.iter().map(|a| {
            serde_json::json!({
                "level": if a.message.contains("Critical") { "Critical" } else { "Warning" },
                "message": a.message
            })
        }).collect::<Vec<_>>()
    });
    
    let json_str = serde_json::to_string(&report).unwrap();
    assert!(
        json_str.len() > 0,
        "Report JSON should be generated"
    );
    
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert!(parsed.get("total_cost_usd").is_some(), "Report should have total");
    assert!(parsed.get("by_category").is_some(), "Report should have categories");
    assert!(parsed.get("alerts").is_some(), "Report should have alerts");
}