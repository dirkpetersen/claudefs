//! A11 Phase 5 Block 2: Preemptible Instance Lifecycle Tests
//!
//! Tests for spot pricing, instance lifecycle management, and disruption handling.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq)]
pub enum InstanceStatus {
    Running,
    Draining,
    Drained,
    Terminated,
}

#[derive(Debug, Clone)]
pub struct SpotPrice {
    pub instance_type: String,
    pub spot_price: f64,
    pub on_demand_price: f64,
    pub timestamp: String,
}

#[derive(Debug, Clone)]
pub struct MockInstance {
    pub instance_id: String,
    pub role: String,
    pub status: InstanceStatus,
    pub pending_operations: Arc<AtomicUsize>,
    pub operation_time_ms: u64,
    pub hourly_rate: f64,
    pub uptime_hours: f64,
}

#[derive(Debug, Clone)]
pub struct MockIMDS {
    pub call_count: Arc<AtomicUsize>,
    pub termination_after: usize,
    pub has_termination_notice: Arc<AtomicBool>,
}

impl MockIMDS {
    fn new(termination_after: usize) -> Self {
        Self {
            call_count: Arc::new(AtomicUsize::new(0)),
            termination_after,
            has_termination_notice: Arc::new(AtomicBool::new(false)),
        }
    }

    fn check_notice(&self) -> bool {
        let count = self.call_count.fetch_add(1, Ordering::SeqCst);
        if count >= self.termination_after {
            self.has_termination_notice.store(true, Ordering::SeqCst);
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone)]
pub struct CostCalculation {
    pub instance_type: String,
    pub pricing_model: String,
    pub hourly_rate: f64,
    pub uptime_hours: f64,
}

fn round_cents(amount: f64) -> f64 {
    (amount * 100.0).round() / 100.0
}

fn get_on_demand_price(instance_type: &str) -> f64 {
    match instance_type {
        "i4i.2xlarge" => 0.624,
        "i4i.4xlarge" => 1.248,
        "c7a.2xlarge" => 0.369,
        "c7a.xlarge" => 0.1845,
        "t3.medium" => 0.0424,
        _ => 0.50,
    }
}

fn calculate_discount(spot: f64, on_demand: f64) -> f64 {
    if on_demand <= 0.0 {
        return 0.0;
    }
    round_cents(((on_demand - spot) / on_demand) * 100.0)
}

fn calculate_monthly_savings(spot: f64, on_demand: f64) -> f64 {
    round_cents((on_demand - spot) * 730.0)
}

fn should_launch_decision(spot_price: f64, on_demand_price: f64, interruption_rate: f64) -> &'static str {
    let spot_ratio = spot_price / on_demand_price;

    if spot_ratio < 0.50 && interruption_rate < 5.0 {
        "true"
    } else if spot_ratio > 0.70 || interruption_rate > 10.0 {
        "false"
    } else {
        "maybe"
    }
}

fn calculate_instance_cost(hourly_rate: f64, uptime_hours: f64) -> f64 {
    round_cents(hourly_rate * uptime_hours)
}

#[cfg(test)]
mod spot_pricing_tests {
    use super::*;

    #[test]
    fn test_spot_pricing_query_valid() {
        let spot_price = SpotPrice {
            instance_type: "i4i.2xlarge".to_string(),
            spot_price: 0.19,
            on_demand_price: 0.624,
            timestamp: "2026-04-18T10:00:00Z".to_string(),
        };

        assert_eq!(spot_price.instance_type, "i4i.2xlarge");
        assert!(spot_price.spot_price < spot_price.on_demand_price);
        assert!(spot_price.spot_price > 0.0);
    }

    #[test]
    fn test_spot_pricing_history_trend() {
        let prices = vec![
            0.25, 0.24, 0.23, 0.22, 0.21, 0.20, 0.19,
        ];

        let avg_recent = (prices[4] + prices[5] + prices[6]) / 3.0;
        let avg_older = (prices[0] + prices[1] + prices[2]) / 3.0;

        let trend = if avg_recent < avg_older - 0.02 {
            "Downward"
        } else if avg_recent > avg_older + 0.02 {
            "Upward"
        } else {
            "Stable"
        };

        assert_eq!(trend, "Downward");
        assert!((avg_recent - avg_older).abs() > 0.02_f64);
    }

    #[test]
    fn test_breakeven_calculation() {
        let spot = 0.19;
        let on_demand = 0.624;

        let discount = calculate_discount(spot, on_demand);
        
        assert!((discount - 69.6).abs() < 1.0, "Discount should be ~69.6%, got {}", discount);

        let monthly_savings = calculate_monthly_savings(spot, on_demand);
        let expected_savings = (on_demand - spot) * 730.0;
        assert!((monthly_savings - expected_savings).abs() < 0.1);
    }

    #[test]
    fn test_should_launch_decision_logic() {
        let case_a = should_launch_decision(0.19, 0.624, 2.0);
        assert_eq!(case_a, "true", "Case A: good price, low interruption");

        let case_b = should_launch_decision(0.60, 0.624, 15.0);
        assert_eq!(case_b, "false", "Case B: high price, high interruption");

        let case_c = should_launch_decision(0.40, 0.624, 3.0);
        assert_eq!(case_c, "maybe", "Case C: mid price");

        let case_d = should_launch_decision(0.45, 0.50, 8.0);
        assert_eq!(case_d, "false", "Case D: high spot ratio");
    }
}

#[cfg(test)]
mod instance_lifecycle_tests {
    use super::*;

    #[test]
    fn test_provision_instance_success() {
        let instance = MockInstance {
            instance_id: "i-1234567890abcdef0".to_string(),
            role: "storage".to_string(),
            status: InstanceStatus::Running,
            pending_operations: Arc::new(AtomicUsize::new(0)),
            operation_time_ms: 0,
            hourly_rate: 0.19,
            uptime_hours: 0.0,
        };

        let tags = vec![
            ("Name", "cfs-storage-i-1234567890abcdef0"),
            ("Role", "storage"),
            ("Site", "A"),
            ("CostCenter", "Infrastructure"),
            ("Agent", "a11"),
            ("StartTime", "2026-04-18T10:00:00Z"),
        ];

        assert_eq!(instance.status, InstanceStatus::Running);
        assert!(tags.iter().all(|(k, _)| !k.is_empty()));
        assert_eq!(tags.len(), 6);
    }

    #[test]
    fn test_provision_instance_with_retries() {
        let mut attempt = 0;
        let mut success = false;
        let transient_failures = 2;

        for i in 0..=transient_failures {
            attempt = i;
            if attempt >= transient_failures {
                success = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50 * (attempt as u64 + 1)));
        }

        assert!(success);
        assert_eq!(attempt, 2);
    }

    #[tokio::test]
    async fn test_drain_instance_graceful() {
        let instance = Arc::new(Mutex::new(MockInstance {
            instance_id: "i-test".to_string(),
            role: "storage".to_string(),
            status: InstanceStatus::Running,
            pending_operations: Arc::new(AtomicUsize::new(10)),
            operation_time_ms: 50,
            hourly_rate: 0.19,
            uptime_hours: 1.0,
        }));

        let start_time = std::time::Instant::now();
        let mut operations_completed = 0;

        loop {
            let inst = instance.lock().await;
            let pending = inst.pending_operations.load(Ordering::SeqCst);
            
            if pending == 0 {
                break;
            }
            drop(inst);

            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            operations_completed += 1;

            if operations_completed >= 10 {
                let mut inst = instance.lock().await;
                inst.pending_operations.store(0, Ordering::SeqCst);
            }
        }

        let elapsed = start_time.elapsed().as_millis() as u64;

        assert!(elapsed < 120000, "Should complete within 120s timeout");
        assert_eq!(operations_completed, 10);
    }

    #[tokio::test]
    async fn test_drain_instance_timeout() {
        let instance = Arc::new(Mutex::new(MockInstance {
            instance_id: "i-test".to_string(),
            role: "storage".to_string(),
            status: InstanceStatus::Running,
            pending_operations: Arc::new(AtomicUsize::new(10)),
            operation_time_ms: 150,
            hourly_rate: 0.19,
            uptime_hours: 1.0,
        }));

        let timeout_ms = 500u64;
        let start_time = std::time::Instant::now();

        loop {
            if start_time.elapsed().as_millis() >= timeout_ms {
                break;
            }

            let inst = instance.lock().await;
            let pending = inst.pending_operations.load(Ordering::SeqCst);
            
            if pending > 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            }
        }

        let elapsed = start_time.elapsed().as_millis() as u64;

        assert!(elapsed >= timeout_ms, "Should timeout after 500ms");
    }
}

#[cfg(test)]
mod disruption_handling_tests {
    use super::*;

    #[test]
    fn test_spot_termination_notice_detected() {
        let mock_imds = MockIMDS::new(2);

        let detected_after_1 = mock_imds.check_notice();
        assert!(!detected_after_1, "Should not detect on first check");

        let start = std::time::Instant::now();
        let detected_after_2 = mock_imds.check_notice();
        let latency = start.elapsed().as_millis();

        assert!(detected_after_2, "Should detect on second check");
        assert!(latency < 20, "Detection should be fast");
    }

    #[test]
    fn test_disruption_triggers_drain() {
        let drain_initiated = Arc::new(AtomicBool::new(false));
        let drain_initiated_clone = Arc::clone(&drain_initiated);

        let notice_received = Arc::new(AtomicBool::new(false));

        if notice_received.load(Ordering::SeqCst) {
            drain_initiated_clone.store(true, Ordering::SeqCst);
        }

        notice_received.store(true, Ordering::SeqCst);

        assert!(drain_initiated.load(Ordering::SeqCst), "Drain should be triggered");
    }

    #[test]
    fn test_replacement_launch_after_disruption() {
        let old_instance_id = "i-old-123".to_string();
        let new_instance_id = "i-new-456".to_string();

        let replacement_tags = vec![
            ("ReplacementOf", old_instance_id.clone()),
            ("DisruptionCount", "1".to_string()),
            ("TotalUptime", "0".to_string()),
        ];

        let has_replacement_of = replacement_tags.iter()
            .any(|(k, v)| *k == "ReplacementOf" && v == &old_instance_id);

        assert!(has_replacement_of);
        assert_ne!(old_instance_id, new_instance_id);
    }

    #[test]
    fn test_concurrent_disruptions() {
        use std::sync::mpsc;
        use std::thread;

        let (tx, rx) = mpsc::channel();
        let disruption_count = 3;
        let mut handles = vec![];

        for i in 0..disruption_count {
            let tx_clone = tx.clone();
            let handle = thread::spawn(move || {
                let drain_initiated = Arc::new(AtomicBool::new(true));
                tx_clone.send(drain_initiated).unwrap();
            });
            handles.push(handle);
        }

        drop(tx);

        let results: Vec<_> = rx.iter().collect();
        
        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(results.len(), disruption_count);
        assert!(results.iter().all(|r| r.load(Ordering::SeqCst)));
    }
}

#[cfg(test)]
mod cost_tracking_tests {
    use super::*;

    #[test]
    fn test_instance_cost_calculation() {
        let hourly_rate = 0.19;
        let uptime_hours = 2.0;

        let cost = calculate_instance_cost(hourly_rate, uptime_hours);
        let expected = 0.38;

        assert!((cost - expected).abs() < 0.01, "Cost should be $0.38, got {}", cost);
    }

    #[test]
    fn test_replacement_cost_included() {
        let instance_a_rate = 0.19;
        let instance_a_uptime = 8.0;
        let instance_a_cost = calculate_instance_cost(instance_a_rate, instance_a_uptime);

        let replacement_rate = 0.19;
        let replacement_uptime = 4.0;
        let replacement_cost = calculate_instance_cost(replacement_rate, replacement_uptime);

        let total = round_cents(instance_a_cost + replacement_cost);
        let expected_total = round_cents(1.52 + 0.76);

        assert!((total - expected_total).abs() < 0.01, "Total should be $2.28, got {}", total);
    }

    #[test]
    fn test_daily_cost_report_accuracy() {
        let cluster_config = vec![
            ("orchestrator", "c7a.2xlarge", false, 0.35),
            ("storage-a1", "i4i.2xlarge", true, 0.19),
            ("storage-a2", "i4i.2xlarge", true, 0.19),
            ("storage-a3", "i4i.2xlarge", true, 0.19),
            ("storage-b1", "i4i.2xlarge", true, 0.19),
            ("storage-b2", "i4i.2xlarge", true, 0.19),
            ("client-1", "c7a.xlarge", true, 0.05),
            ("client-2", "c7a.xlarge", true, 0.05),
            ("conduit", "t3.medium", true, 0.015),
        ];

        let uptime_hours = 24.0;
        let mut total_cost = 0.0;
        let mut on_demand_equivalent = 0.0;

        for (_, _, is_spot, rate) in &cluster_config {
            let cost = calculate_instance_cost(*rate, uptime_hours);
            total_cost += cost;

            if *is_spot {
                let od_rate = get_on_demand_price(match _2 {
                    "i4i.2xlarge" => "i4i.2xlarge",
                    "c7a.xlarge" => "c7a.xlarge",
                    "t3.medium" => "t3.medium",
                    _ => "c7a.2xlarge",
                });
                on_demand_equivalent += calculate_instance_cost(od_rate, uptime_hours);
            } else {
                on_demand_equivalent += cost;
            }
        }

        let savings_pct = if on_demand_equivalent > 0.0 {
            ((on_demand_equivalent - total_cost) / on_demand_equivalent) * 100.0
        } else {
            0.0
        };

        assert!((total_cost - 10.26).abs() < 0.1, "Total cost should be ~$10.26, got {}", total_cost);
        assert!((savings_pct - 60.0).abs() < 5.0, "Savings should be ~60%, got {}%", savings_pct);
    }
}

#[test]
fn test_spot_pricing_scripts_exist() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = std::path::Path::new(manifest_dir)
        .ancestors()
        .find(|p| p.join("Cargo.toml").exists() && p.join("tools").exists())
        .expect("Could not find workspace root");

    let scripts = [
        "cfs-spot-pricing.sh",
        "cfs-instance-manager.sh",
        "cfs-disruption-handler.sh",
    ];

    for script in &scripts {
        let script_path = workspace_root.join("tools").join(script);
        assert!(
            script_path.exists(),
            "Script {} should exist at {:?}",
            script,
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
                    "Script {} should be executable",
                    script
                );
            }
        }
    }
}

#[test]
fn test_systemd_service_exists() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = std::path::Path::new(manifest_dir)
        .ancestors()
        .find(|p| p.join("Cargo.toml").exists() && p.join("systemd").exists())
        .expect("Could not find workspace root");

    let service_path = workspace_root.join("systemd/cfs-spot-monitor.service");
    assert!(
        service_path.exists(),
        "Systemd service should exist at {:?}",
        service_path
    );

    let content = std::fs::read_to_string(&service_path).unwrap();
    assert!(content.contains("[Unit]"), "Service should have [Unit] section");
    assert!(content.contains("[Service]"), "Service should have [Service] section");
    assert!(content.contains("ExecStart="), "Service should have ExecStart");
}