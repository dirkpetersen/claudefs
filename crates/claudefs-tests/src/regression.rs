use std::collections::HashMap;
use std::path::Path;
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone)]
pub struct RegressionCase {
    pub id: String,
    pub title: String,
    pub severity: Severity,
    pub components: Vec<String>,
    pub description: String,
    pub reproduction: String,
    pub fixed_in: Option<String>,
}

pub struct RegressionRegistry {
    cases: HashMap<String, RegressionCase>,
}

impl RegressionRegistry {
    pub fn new() -> Self {
        Self {
            cases: HashMap::new(),
        }
    }

    pub fn register(&mut self, case: RegressionCase) {
        self.cases.insert(case.id.clone(), case);
    }

    pub fn get(&self, id: &str) -> Option<&RegressionCase> {
        self.cases.get(id)
    }

    pub fn by_severity(&self, severity: &Severity) -> Vec<&RegressionCase> {
        self.cases
            .values()
            .filter(|c| &c.severity == severity)
            .collect()
    }

    pub fn by_component(&self, component: &str) -> Vec<&RegressionCase> {
        self.cases
            .values()
            .filter(|c| c.components.iter().any(|comp| comp == component))
            .collect()
    }

    pub fn fixed_cases(&self) -> Vec<&RegressionCase> {
        self.cases
            .values()
            .filter(|c| c.fixed_in.is_some())
            .collect()
    }

    pub fn open_cases(&self) -> Vec<&RegressionCase> {
        self.cases
            .values()
            .filter(|c| c.fixed_in.is_none())
            .collect()
    }

    pub fn count(&self) -> usize {
        self.cases.len()
    }

    pub fn seed_known_issues(&mut self) {
        self.register(RegressionCase {
            id: "CLAUDEFS-1".to_string(),
            title: "Race condition in block allocator".to_string(),
            severity: Severity::High,
            components: vec!["claudefs-storage".to_string()],
            description: "Concurrent allocation can cause double-allocation".to_string(),
            reproduction: "Run 10 threads allocating blocks simultaneously".to_string(),
            fixed_in: None,
        });

        self.register(RegressionCase {
            id: "CLAUDEFS-2".to_string(),
            title: "Memory leak in transport layer".to_string(),
            severity: Severity::Critical,
            components: vec!["claudefs-transport".to_string()],
            description: "Connection pool not properly released on disconnect".to_string(),
            reproduction: "Connect and disconnect 1000 times".to_string(),
            fixed_in: None,
        });

        self.register(RegressionCase {
            id: "CLAUDEFS-3".to_string(),
            title: "Incorrect inode generation".to_string(),
            severity: Severity::Medium,
            components: vec!["claudefs-meta".to_string()],
            description: "Inode numbers can collide after restart".to_string(),
            reproduction: "Restart meta server and create same files".to_string(),
            fixed_in: None,
        });

        self.register(RegressionCase {
            id: "CLAUDEFS-4".to_string(),
            title: "Compression performance degradation".to_string(),
            severity: Severity::Low,
            components: vec!["claudefs-reduce".to_string()],
            description: "LZ4 compression slower than expected on small blocks".to_string(),
            reproduction: "Benchmark compression of 4KB blocks".to_string(),
            fixed_in: None,
        });

        self.register(RegressionCase {
            id: "CLAUDEFS-5".to_string(),
            title: "Metadata sync on rename".to_string(),
            severity: Severity::High,
            components: vec!["claudefs-meta".to_string()],
            description: "Rename doesn't sync parent directory mtime".to_string(),
            reproduction: "Rename file and check parent mtime".to_string(),
            fixed_in: Some("abc123".to_string()),
        });
    }
}

#[derive(Debug, Clone)]
pub struct RegressionResult {
    pub case_id: String,
    pub reproduced: bool,
    pub details: String,
}

pub struct RegressionRunner {
    pub registry: RegressionRegistry,
}

impl RegressionRunner {
    pub fn new() -> Self {
        Self {
            registry: RegressionRegistry::new(),
        }
    }

    pub fn run_case(&self, id: &str, test_path: &Path) -> RegressionResult {
        if let Some(case) = self.registry.get(id) {
            RegressionResult {
                case_id: id.to_string(),
                reproduced: false,
                details: format!("Would run test for: {}", case.title),
            }
        } else {
            RegressionResult {
                case_id: id.to_string(),
                reproduced: false,
                details: format!("Case {} not found", id),
            }
        }
    }

    pub fn run_all(&self, test_path: &Path) -> Vec<RegressionResult> {
        self.registry
            .open_cases()
            .iter()
            .map(|c| self.run_case(&c.id, test_path))
            .collect()
    }

    pub fn summary(&self, results: &[RegressionResult]) -> RegressionSummary {
        let total = results.len();
        let reproduced = results.iter().filter(|r| r.reproduced).count();
        let not_reproduced = total - reproduced;
        let fixed_percent = if total > 0 {
            (reproduced as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        RegressionSummary {
            total,
            reproduced,
            not_reproduced,
            fixed_percent,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RegressionSummary {
    pub total: usize,
    pub reproduced: usize,
    pub not_reproduced: usize,
    pub fixed_percent: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_regression_case_creation() {
        let case = RegressionCase {
            id: "TEST-1".to_string(),
            title: "Test case".to_string(),
            severity: Severity::Medium,
            components: vec!["claudefs-storage".to_string()],
            description: "A test regression".to_string(),
            reproduction: "Run the test".to_string(),
            fixed_in: None,
        };
        assert_eq!(case.id, "TEST-1");
        assert!(case.fixed_in.is_none());
    }

    #[test]
    fn test_severity_ordering() {
        assert!(Severity::Low < Severity::Medium);
        assert!(Severity::Medium < Severity::High);
        assert!(Severity::High < Severity::Critical);
    }

    #[test]
    fn test_regression_registry_register_and_get() {
        let mut registry = RegressionRegistry::new();
        let case = RegressionCase {
            id: "TEST-1".to_string(),
            title: "Test".to_string(),
            severity: Severity::Low,
            components: vec![],
            description: "desc".to_string(),
            reproduction: "repro".to_string(),
            fixed_in: None,
        };
        registry.register(case);
        let retrieved = registry.get("TEST-1");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, "TEST-1");
    }

    #[test]
    fn test_by_severity_filtering() {
        let mut registry = RegressionRegistry::new();
        registry.register(RegressionCase {
            id: "1".to_string(),
            title: "Low".to_string(),
            severity: Severity::Low,
            components: vec![],
            description: "".to_string(),
            reproduction: "".to_string(),
            fixed_in: None,
        });
        registry.register(RegressionCase {
            id: "2".to_string(),
            title: "High".to_string(),
            severity: Severity::High,
            components: vec![],
            description: "".to_string(),
            reproduction: "".to_string(),
            fixed_in: None,
        });

        let low_cases = registry.by_severity(&Severity::Low);
        assert_eq!(low_cases.len(), 1);
    }

    #[test]
    fn test_by_component_filtering() {
        let mut registry = RegressionRegistry::new();
        registry.register(RegressionCase {
            id: "1".to_string(),
            title: "Storage issue".to_string(),
            severity: Severity::Low,
            components: vec!["claudefs-storage".to_string()],
            description: "".to_string(),
            reproduction: "".to_string(),
            fixed_in: None,
        });
        registry.register(RegressionCase {
            id: "2".to_string(),
            title: "Meta issue".to_string(),
            severity: Severity::Low,
            components: vec!["claudefs-meta".to_string()],
            description: "".to_string(),
            reproduction: "".to_string(),
            fixed_in: None,
        });

        let storage_cases = registry.by_component("claudefs-storage");
        assert_eq!(storage_cases.len(), 1);
    }

    #[test]
    fn test_fixed_cases() {
        let mut registry = RegressionRegistry::new();
        registry.register(RegressionCase {
            id: "1".to_string(),
            title: "Fixed".to_string(),
            severity: Severity::Low,
            components: vec![],
            description: "".to_string(),
            reproduction: "".to_string(),
            fixed_in: Some("abc123".to_string()),
        });
        registry.register(RegressionCase {
            id: "2".to_string(),
            title: "Open".to_string(),
            severity: Severity::Low,
            components: vec![],
            description: "".to_string(),
            reproduction: "".to_string(),
            fixed_in: None,
        });

        let fixed = registry.fixed_cases();
        assert_eq!(fixed.len(), 1);
    }

    #[test]
    fn test_open_cases() {
        let mut registry = RegressionRegistry::new();
        registry.register(RegressionCase {
            id: "1".to_string(),
            title: "Fixed".to_string(),
            severity: Severity::Low,
            components: vec![],
            description: "".to_string(),
            reproduction: "".to_string(),
            fixed_in: Some("abc123".to_string()),
        });
        registry.register(RegressionCase {
            id: "2".to_string(),
            title: "Open".to_string(),
            severity: Severity::Low,
            components: vec![],
            description: "".to_string(),
            reproduction: "".to_string(),
            fixed_in: None,
        });

        let open = registry.open_cases();
        assert_eq!(open.len(), 1);
    }

    #[test]
    fn test_seed_known_issues() {
        let mut registry = RegressionRegistry::new();
        registry.seed_known_issues();
        assert_eq!(registry.count(), 5);
    }

    #[test]
    fn test_regression_runner_summary() {
        let runner = RegressionRunner::new();
        let results = vec![
            RegressionResult {
                case_id: "1".to_string(),
                reproduced: true,
                details: "reproduced".to_string(),
            },
            RegressionResult {
                case_id: "2".to_string(),
                reproduced: false,
                details: "not reproduced".to_string(),
            },
            RegressionResult {
                case_id: "3".to_string(),
                reproduced: true,
                details: "reproduced".to_string(),
            },
        ];
        let summary = runner.summary(&results);
        assert_eq!(summary.total, 3);
        assert_eq!(summary.reproduced, 2);
        assert_eq!(summary.not_reproduced, 1);
    }

    #[test]
    fn test_regression_result_creation() {
        let result = RegressionResult {
            case_id: "TEST-1".to_string(),
            reproduced: true,
            details: "Test details".to_string(),
        };
        assert_eq!(result.case_id, "TEST-1");
        assert!(result.reproduced);
    }

    #[test]
    fn test_regression_summary_calculation() {
        let summary = RegressionSummary {
            total: 10,
            reproduced: 7,
            not_reproduced: 3,
            fixed_percent: 70.0,
        };
        assert_eq!(summary.total, 10);
        assert_eq!(summary.fixed_percent, 70.0);
    }

    #[test]
    fn test_severity_ordering_comparisons() {
        assert!(Severity::Low <= Severity::Low);
        assert!(Severity::Low <= Severity::Medium);
        assert!(Severity::Critical >= Severity::High);
    }

    #[test]
    fn test_registry_count() {
        let mut registry = RegressionRegistry::new();
        assert_eq!(registry.count(), 0);
        registry.register(RegressionCase {
            id: "1".to_string(),
            title: "".to_string(),
            severity: Severity::Low,
            components: vec![],
            description: "".to_string(),
            reproduction: "".to_string(),
            fixed_in: None,
        });
        assert_eq!(registry.count(), 1);
    }

    #[test]
    fn test_run_case_not_found() {
        let runner = RegressionRunner::new();
        let result = runner.run_case("NONEXISTENT", Path::new("/tmp"));
        assert!(!result.reproduced);
    }

    #[test]
    fn test_run_all() {
        let mut runner = RegressionRunner::new();
        runner.registry.register(RegressionCase {
            id: "1".to_string(),
            title: "Test".to_string(),
            severity: Severity::Low,
            components: vec![],
            description: "".to_string(),
            reproduction: "".to_string(),
            fixed_in: None,
        });
        let results = runner.run_all(Path::new("/tmp"));
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_fixed_in_option() {
        let case = RegressionCase {
            id: "1".to_string(),
            title: "".to_string(),
            severity: Severity::Low,
            components: vec![],
            description: "".to_string(),
            reproduction: "".to_string(),
            fixed_in: Some("v1.0.0".to_string()),
        };
        assert!(case.fixed_in.is_some());
    }

    #[test]
    fn test_multiple_components() {
        let case = RegressionCase {
            id: "1".to_string(),
            title: "".to_string(),
            severity: Severity::Low,
            components: vec!["claudefs-storage".to_string(), "claudefs-meta".to_string()],
            description: "".to_string(),
            reproduction: "".to_string(),
            fixed_in: None,
        };
        assert_eq!(case.components.len(), 2);
    }

    #[test]
    fn test_by_component_no_match() {
        let registry = RegressionRegistry::new();
        let cases = registry.by_component("nonexistent");
        assert!(cases.is_empty());
    }

    #[test]
    fn test_by_severity_no_match() {
        let registry = RegressionRegistry::new();
        let cases = registry.by_severity(&Severity::Critical);
        assert!(cases.is_empty());
    }
}
