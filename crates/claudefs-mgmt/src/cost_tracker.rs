use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CostCategory {
    Compute,
    Storage,
    Network,
    Api,
    Other,
}

#[derive(Debug, Clone)]
pub struct CostEntry {
    pub category: CostCategory,
    pub amount_usd: f64,
    pub resource_id: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone)]
pub struct CostBudget {
    pub daily_limit_usd: f64,
    pub monthly_limit_usd: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetStatus {
    Ok,
    Warning,
    Critical,
    Exceeded,
}

#[derive(Error, Debug)]
pub enum CostTrackerError {
    #[error("Budget not configured")]
    NoBudget,
    #[error("Invalid timestamp")]
    InvalidTimestamp,
    #[error("Lock error")]
    LockError,
}

pub struct CostTracker {
    entries: Arc<Mutex<Vec<CostEntry>>>,
    budget: CostBudget,
}

impl CostTracker {
    pub fn new(budget: CostBudget) -> Self {
        Self {
            entries: Arc::new(Mutex::new(Vec::new())),
            budget,
        }
    }

    pub fn record(&self, entry: CostEntry) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.push(entry);
        }
    }

    pub fn total_cost(&self) -> f64 {
        self.entries
            .lock()
            .map(|entries| entries.iter().map(|e| e.amount_usd).sum())
            .unwrap_or(0.0)
    }

    pub fn cost_by_category(&self, category: &CostCategory) -> f64 {
        self.entries
            .lock()
            .map(|entries| {
                entries
                    .iter()
                    .filter(|e| &e.category == category)
                    .map(|e| e.amount_usd)
                    .sum()
            })
            .unwrap_or(0.0)
    }

    pub fn daily_total(&self, day_start_ts: u64) -> f64 {
        self.entries
            .lock()
            .map(|entries| {
                entries
                    .iter()
                    .filter(|e| e.timestamp >= day_start_ts)
                    .map(|e| e.amount_usd)
                    .sum()
            })
            .unwrap_or(0.0)
    }

    pub fn budget_status(&self, day_start_ts: u64) -> BudgetStatus {
        let daily = self.daily_total(day_start_ts);
        let limit = self.budget.daily_limit_usd;

        if limit <= 0.0 {
            return BudgetStatus::Ok;
        }

        let percentage = daily / limit;

        if percentage >= 1.0 {
            BudgetStatus::Exceeded
        } else if percentage >= 0.9 {
            BudgetStatus::Critical
        } else if percentage >= 0.75 {
            BudgetStatus::Warning
        } else {
            BudgetStatus::Ok
        }
    }

    pub fn top_resources(&self, n: usize) -> Vec<(String, f64)> {
        let entries = match self.entries.lock() {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };

        let mut totals: HashMap<String, f64> = HashMap::new();
        for entry in entries.iter() {
            *totals.entry(entry.resource_id.clone()).or_insert(0.0) += entry.amount_usd;
        }

        let mut sorted: Vec<(String, f64)> = totals.into_iter().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        sorted.into_iter().take(n).collect()
    }
}

#[derive(Debug, Clone)]
pub struct CostAlert {
    pub category: CostCategory,
    pub amount_usd: f64,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct CostAlertRule {
    pub threshold_usd: f64,
    pub category: Option<CostCategory>,
    pub message_template: String,
}

pub struct CostAlertEngine {
    pub rules: Vec<CostAlertRule>,
}

impl CostAlertEngine {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    pub fn add_rule(&mut self, rule: CostAlertRule) {
        self.rules.push(rule);
    }

    pub fn evaluate(&self, tracker: &CostTracker) -> Vec<CostAlert> {
        let mut alerts = Vec::new();

        for rule in &self.rules {
            let amount = match &rule.category {
                Some(cat) => tracker.cost_by_category(cat),
                None => tracker.total_cost(),
            };

            if amount >= rule.threshold_usd {
                let message = rule
                    .message_template
                    .replace("{amount}", &format!("{:.2}", amount));
                alerts.push(CostAlert {
                    category: rule.category.unwrap_or(CostCategory::Other),
                    amount_usd: amount,
                    message,
                });
            }
        }

        alerts
    }
}

impl Default for CostAlertEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(days_ago: u64) -> u64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        now - (days_ago * 86400)
    }

    #[test]
    fn test_cost_category_variants() {
        let _ = CostCategory::Compute;
        let _ = CostCategory::Storage;
        let _ = CostCategory::Network;
        let _ = CostCategory::Api;
        let _ = CostCategory::Other;
    }

    #[test]
    fn test_cost_entry_creation() {
        let entry = CostEntry {
            category: CostCategory::Compute,
            amount_usd: 10.5,
            resource_id: "vm-1".to_string(),
            timestamp: 1234567890,
        };
        assert_eq!(entry.amount_usd, 10.5);
        assert_eq!(entry.resource_id, "vm-1");
    }

    #[test]
    fn test_cost_budget_creation() {
        let budget = CostBudget {
            daily_limit_usd: 100.0,
            monthly_limit_usd: 3000.0,
        };
        assert_eq!(budget.daily_limit_usd, 100.0);
        assert_eq!(budget.monthly_limit_usd, 3000.0);
    }

    #[test]
    fn test_budget_status_variants() {
        let _ = BudgetStatus::Ok;
        let _ = BudgetStatus::Warning;
        let _ = BudgetStatus::Critical;
        let _ = BudgetStatus::Exceeded;
    }

    #[test]
    fn test_cost_tracker_new() {
        let budget = CostBudget {
            daily_limit_usd: 100.0,
            monthly_limit_usd: 3000.0,
        };
        let tracker = CostTracker::new(budget);
        assert_eq!(tracker.total_cost(), 0.0);
    }

    #[test]
    fn test_cost_tracker_record() {
        let budget = CostBudget {
            daily_limit_usd: 100.0,
            monthly_limit_usd: 3000.0,
        };
        let tracker = CostTracker::new(budget);

        tracker.record(CostEntry {
            category: CostCategory::Compute,
            amount_usd: 10.0,
            resource_id: "vm-1".to_string(),
            timestamp: ts(0),
        });

        assert_eq!(tracker.total_cost(), 10.0);
    }

    #[test]
    fn test_cost_tracker_multiple_entries() {
        let budget = CostBudget {
            daily_limit_usd: 100.0,
            monthly_limit_usd: 3000.0,
        };
        let tracker = CostTracker::new(budget);

        tracker.record(CostEntry {
            category: CostCategory::Compute,
            amount_usd: 10.0,
            resource_id: "vm-1".to_string(),
            timestamp: ts(0),
        });
        tracker.record(CostEntry {
            category: CostCategory::Storage,
            amount_usd: 5.0,
            resource_id: "disk-1".to_string(),
            timestamp: ts(0),
        });

        assert_eq!(tracker.total_cost(), 15.0);
    }

    #[test]
    fn test_cost_by_category() {
        let budget = CostBudget {
            daily_limit_usd: 100.0,
            monthly_limit_usd: 3000.0,
        };
        let tracker = CostTracker::new(budget);

        tracker.record(CostEntry {
            category: CostCategory::Compute,
            amount_usd: 10.0,
            resource_id: "vm-1".to_string(),
            timestamp: ts(0),
        });
        tracker.record(CostEntry {
            category: CostCategory::Compute,
            amount_usd: 5.0,
            resource_id: "vm-2".to_string(),
            timestamp: ts(0),
        });
        tracker.record(CostEntry {
            category: CostCategory::Storage,
            amount_usd: 3.0,
            resource_id: "disk-1".to_string(),
            timestamp: ts(0),
        });

        assert_eq!(tracker.cost_by_category(&CostCategory::Compute), 15.0);
        assert_eq!(tracker.cost_by_category(&CostCategory::Storage), 3.0);
        assert_eq!(tracker.cost_by_category(&CostCategory::Network), 0.0);
    }

    #[test]
    fn test_daily_total() {
        let budget = CostBudget {
            daily_limit_usd: 100.0,
            monthly_limit_usd: 3000.0,
        };
        let tracker = CostTracker::new(budget);

        let today = ts(0);
        let yesterday = ts(1);

        tracker.record(CostEntry {
            category: CostCategory::Compute,
            amount_usd: 10.0,
            resource_id: "vm-1".to_string(),
            timestamp: today,
        });
        tracker.record(CostEntry {
            category: CostCategory::Storage,
            amount_usd: 5.0,
            resource_id: "disk-1".to_string(),
            timestamp: yesterday,
        });

        assert_eq!(tracker.daily_total(today), 10.0);
    }

    #[test]
    fn test_budget_status_ok() {
        let budget = CostBudget {
            daily_limit_usd: 100.0,
            monthly_limit_usd: 3000.0,
        };
        let tracker = CostTracker::new(budget);

        tracker.record(CostEntry {
            category: CostCategory::Compute,
            amount_usd: 10.0,
            resource_id: "vm-1".to_string(),
            timestamp: ts(0),
        });

        assert_eq!(tracker.budget_status(ts(0)), BudgetStatus::Ok);
    }

    #[test]
    fn test_budget_status_warning() {
        let budget = CostBudget {
            daily_limit_usd: 100.0,
            monthly_limit_usd: 3000.0,
        };
        let tracker = CostTracker::new(budget);

        tracker.record(CostEntry {
            category: CostCategory::Compute,
            amount_usd: 80.0,
            resource_id: "vm-1".to_string(),
            timestamp: ts(0),
        });

        assert_eq!(tracker.budget_status(ts(0)), BudgetStatus::Warning);
    }

    #[test]
    fn test_budget_status_critical() {
        let budget = CostBudget {
            daily_limit_usd: 100.0,
            monthly_limit_usd: 3000.0,
        };
        let tracker = CostTracker::new(budget);

        tracker.record(CostEntry {
            category: CostCategory::Compute,
            amount_usd: 95.0,
            resource_id: "vm-1".to_string(),
            timestamp: ts(0),
        });

        assert_eq!(tracker.budget_status(ts(0)), BudgetStatus::Critical);
    }

    #[test]
    fn test_budget_status_exceeded() {
        let budget = CostBudget {
            daily_limit_usd: 100.0,
            monthly_limit_usd: 3000.0,
        };
        let tracker = CostTracker::new(budget);

        tracker.record(CostEntry {
            category: CostCategory::Compute,
            amount_usd: 110.0,
            resource_id: "vm-1".to_string(),
            timestamp: ts(0),
        });

        assert_eq!(tracker.budget_status(ts(0)), BudgetStatus::Exceeded);
    }

    #[test]
    fn test_top_resources() {
        let budget = CostBudget {
            daily_limit_usd: 100.0,
            monthly_limit_usd: 3000.0,
        };
        let tracker = CostTracker::new(budget);

        tracker.record(CostEntry {
            category: CostCategory::Compute,
            amount_usd: 10.0,
            resource_id: "vm-1".to_string(),
            timestamp: ts(0),
        });
        tracker.record(CostEntry {
            category: CostCategory::Compute,
            amount_usd: 20.0,
            resource_id: "vm-2".to_string(),
            timestamp: ts(0),
        });
        tracker.record(CostEntry {
            category: CostCategory::Storage,
            amount_usd: 15.0,
            resource_id: "vm-1".to_string(),
            timestamp: ts(0),
        });

        let top = tracker.top_resources(2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].0, "vm-1");
        assert_eq!(top[0].1, 25.0);
        assert_eq!(top[1].0, "vm-2");
        assert_eq!(top[1].1, 20.0);
    }

    #[test]
    fn test_top_resources_limit() {
        let budget = CostBudget {
            daily_limit_usd: 100.0,
            monthly_limit_usd: 3000.0,
        };
        let tracker = CostTracker::new(budget);

        for i in 0..5 {
            tracker.record(CostEntry {
                category: CostCategory::Compute,
                amount_usd: 10.0 + i as f64,
                resource_id: format!("vm-{}", i),
                timestamp: ts(0),
            });
        }

        let top = tracker.top_resources(2);
        assert_eq!(top.len(), 2);
    }

    #[test]
    fn test_top_resources_empty() {
        let budget = CostBudget {
            daily_limit_usd: 100.0,
            monthly_limit_usd: 3000.0,
        };
        let tracker = CostTracker::new(budget);

        let top = tracker.top_resources(5);
        assert!(top.is_empty());
    }

    #[test]
    fn test_cost_alert_creation() {
        let alert = CostAlert {
            category: CostCategory::Compute,
            amount_usd: 100.0,
            message: "Test alert".to_string(),
        };
        assert_eq!(alert.amount_usd, 100.0);
    }

    #[test]
    fn test_cost_alert_rule_creation() {
        let rule = CostAlertRule {
            threshold_usd: 50.0,
            category: Some(CostCategory::Compute),
            message_template: "Compute cost exceeded {amount}".to_string(),
        };
        assert_eq!(rule.threshold_usd, 50.0);
        assert!(rule.category.is_some());
    }

    #[test]
    fn test_cost_alert_engine_new() {
        let engine = CostAlertEngine::new();
        assert!(engine.rules.is_empty());
    }

    #[test]
    fn test_cost_alert_engine_add_rule() {
        let mut engine = CostAlertEngine::new();
        let rule = CostAlertRule {
            threshold_usd: 50.0,
            category: Some(CostCategory::Compute),
            message_template: "Test".to_string(),
        };
        engine.add_rule(rule);
        assert_eq!(engine.rules.len(), 1);
    }

    #[test]
    fn test_cost_alert_engine_evaluate_no_alerts() {
        let mut engine = CostAlertEngine::new();
        engine.add_rule(CostAlertRule {
            threshold_usd: 100.0,
            category: Some(CostCategory::Compute),
            message_template: "Alert {amount}".to_string(),
        });

        let budget = CostBudget {
            daily_limit_usd: 100.0,
            monthly_limit_usd: 3000.0,
        };
        let tracker = CostTracker::new(budget);

        let alerts = engine.evaluate(&tracker);
        assert!(alerts.is_empty());
    }

    #[test]
    fn test_cost_alert_engine_evaluate_triggered() {
        let mut engine = CostAlertEngine::new();
        engine.add_rule(CostAlertRule {
            threshold_usd: 50.0,
            category: Some(CostCategory::Compute),
            message_template: "Compute cost: ${amount}".to_string(),
        });

        let budget = CostBudget {
            daily_limit_usd: 100.0,
            monthly_limit_usd: 3000.0,
        };
        let tracker = CostTracker::new(budget);

        tracker.record(CostEntry {
            category: CostCategory::Compute,
            amount_usd: 75.0,
            resource_id: "vm-1".to_string(),
            timestamp: ts(0),
        });

        let alerts = engine.evaluate(&tracker);
        assert_eq!(alerts.len(), 1);
        assert!(alerts[0].message.contains("75.00"));
    }

    #[test]
    fn test_cost_alert_engine_evaluate_all_categories() {
        let mut engine = CostAlertEngine::new();
        engine.add_rule(CostAlertRule {
            threshold_usd: 50.0,
            category: None,
            message_template: "Total cost: ${amount}".to_string(),
        });

        let budget = CostBudget {
            daily_limit_usd: 100.0,
            monthly_limit_usd: 3000.0,
        };
        let tracker = CostTracker::new(budget);

        tracker.record(CostEntry {
            category: CostCategory::Compute,
            amount_usd: 30.0,
            resource_id: "vm-1".to_string(),
            timestamp: ts(0),
        });
        tracker.record(CostEntry {
            category: CostCategory::Storage,
            amount_usd: 30.0,
            resource_id: "disk-1".to_string(),
            timestamp: ts(0),
        });

        let alerts = engine.evaluate(&tracker);
        assert_eq!(alerts.len(), 1);
    }

    #[test]
    fn test_budget_status_zero_limit() {
        let budget = CostBudget {
            daily_limit_usd: 0.0,
            monthly_limit_usd: 0.0,
        };
        let tracker = CostTracker::new(budget);

        tracker.record(CostEntry {
            category: CostCategory::Compute,
            amount_usd: 100.0,
            resource_id: "vm-1".to_string(),
            timestamp: ts(0),
        });

        assert_eq!(tracker.budget_status(ts(0)), BudgetStatus::Ok);
    }

    #[test]
    fn test_budget_status_negative_limit() {
        let budget = CostBudget {
            daily_limit_usd: -10.0,
            monthly_limit_usd: -300.0,
        };
        let tracker = CostTracker::new(budget);

        tracker.record(CostEntry {
            category: CostCategory::Compute,
            amount_usd: 100.0,
            resource_id: "vm-1".to_string(),
            timestamp: ts(0),
        });

        assert_eq!(tracker.budget_status(ts(0)), BudgetStatus::Ok);
    }

    #[test]
    fn test_cost_tracker_thread_safety() {
        use std::thread;

        let budget = CostBudget {
            daily_limit_usd: 100.0,
            monthly_limit_usd: 3000.0,
        };
        let tracker = Arc::new(CostTracker::new(budget));
        let tracker_clone = Arc::clone(&tracker);

        let handle = thread::spawn(move || {
            for _ in 0..100 {
                tracker_clone.record(CostEntry {
                    category: CostCategory::Compute,
                    amount_usd: 1.0,
                    resource_id: "vm-1".to_string(),
                    timestamp: ts(0),
                });
            }
        });

        for _ in 0..100 {
            tracker.record(CostEntry {
                category: CostCategory::Compute,
                amount_usd: 1.0,
                resource_id: "vm-2".to_string(),
                timestamp: ts(0),
            });
        }

        handle.join().unwrap();

        assert_eq!(tracker.total_cost(), 200.0);
    }
}
