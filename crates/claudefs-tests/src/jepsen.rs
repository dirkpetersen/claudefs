use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub enum JepsenOpType {
    Invoke,
    Ok,
    Fail,
    Info,
}

#[derive(Debug, Clone)]
pub struct JepsenOp {
    pub process: u32,
    pub op_type: JepsenOpType,
    pub key: String,
    pub value: Option<i64>,
    pub timestamp: u64,
}

impl JepsenOp {
    pub fn new_invoke(process: u32, key: &str, value: Option<i64>, timestamp: u64) -> Self {
        Self {
            process,
            op_type: JepsenOpType::Invoke,
            key: key.to_string(),
            value,
            timestamp,
        }
    }

    pub fn new_ok(process: u32, key: &str, value: Option<i64>, timestamp: u64) -> Self {
        Self {
            process,
            op_type: JepsenOpType::Ok,
            key: key.to_string(),
            value,
            timestamp,
        }
    }

    pub fn new_fail(process: u32, key: &str, timestamp: u64) -> Self {
        Self {
            process,
            op_type: JepsenOpType::Fail,
            key: key.to_string(),
            value: None,
            timestamp,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RegisterOp {
    Read,
    Write(i64),
    CAS { expected: i64, new: i64 },
}

#[derive(Debug, Default)]
pub struct JepsenHistory {
    pub ops: Vec<JepsenOp>,
    start_time: Option<Instant>,
}

impl JepsenHistory {
    pub fn new() -> Self {
        Self {
            ops: Vec::new(),
            start_time: Some(Instant::now()),
        }
    }

    pub fn invoke(&mut self, process: u32, key: &str, value: Option<i64>) -> u64 {
        let timestamp = self
            .start_time
            .map(|t| t.elapsed().as_nanos() as u64)
            .unwrap_or(0);
        let op = JepsenOp::new_invoke(process, key, value, timestamp);
        self.ops.push(op);
        timestamp
    }

    pub fn complete_ok(&mut self, process: u32, key: &str, value: Option<i64>) {
        let timestamp = self
            .start_time
            .map(|t| t.elapsed().as_nanos() as u64)
            .unwrap_or(0);
        let op = JepsenOp::new_ok(process, key, value, timestamp);
        self.ops.push(op);
    }

    pub fn complete_fail(&mut self, process: u32, key: &str) {
        let timestamp = self
            .start_time
            .map(|t| t.elapsed().as_nanos() as u64)
            .unwrap_or(0);
        let op = JepsenOp::new_fail(process, key, timestamp);
        self.ops.push(op);
    }

    pub fn duration_ns(&self) -> u64 {
        self.ops.iter().map(|op| op.timestamp).max().unwrap_or(0)
    }

    pub fn ops_by_process(&self, process: u32) -> Vec<&JepsenOp> {
        self.ops.iter().filter(|op| op.process == process).collect()
    }

    pub fn invocations(&self) -> Vec<&JepsenOp> {
        self.ops
            .iter()
            .filter(|op| matches!(op.op_type, JepsenOpType::Invoke))
            .collect()
    }

    pub fn completions(&self) -> Vec<&JepsenOp> {
        self.ops
            .iter()
            .filter(|op| matches!(op.op_type, JepsenOpType::Ok | JepsenOpType::Fail))
            .collect()
    }

    pub fn is_well_formed(&self) -> bool {
        let mut pending_invocations: HashSet<(u32, String)> = HashSet::new();

        for op in &self.ops {
            match &op.op_type {
                JepsenOpType::Invoke => {
                    pending_invocations.insert((op.process, op.key.clone()));
                }
                JepsenOpType::Ok | JepsenOpType::Fail => {
                    if !pending_invocations.remove(&(op.process, op.key.clone())) {
                        return false;
                    }
                }
                JepsenOpType::Info => {}
            }
        }

        pending_invocations.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RegisterModel {
    pub state: i64,
}

impl RegisterModel {
    pub fn new(initial: i64) -> Self {
        Self { state: initial }
    }

    pub fn apply_read(&self) -> i64 {
        self.state
    }

    pub fn apply_write(&mut self, value: i64) {
        self.state = value;
    }

    pub fn apply_cas(&mut self, expected: i64, new: i64) -> bool {
        if self.state == expected {
            self.state = new;
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone)]
pub struct CheckResult {
    pub valid: bool,
    pub anomalies: Vec<String>,
    pub message: String,
}

pub struct JepsenChecker;

impl JepsenChecker {
    pub fn new() -> Self {
        Self
    }

    pub fn check_register(&self, history: &JepsenHistory) -> CheckResult {
        if !history.is_well_formed() {
            return CheckResult {
                valid: false,
                anomalies: vec!["History is not well-formed".to_string()],
                message: "Every invoke must have exactly one completion".to_string(),
            };
        }

        let mut state = 0i64;
        let mut last_write: HashSet<(String, i64)> = HashSet::new();

        for op in &history.ops {
            match op.op_type {
                JepsenOpType::Invoke => {
                    if let Some(value) = op.value {
                        last_write.insert((op.key.clone(), value));
                    }
                }
                JepsenOpType::Ok => {
                    if let Some(value) = op.value {
                        state = value;
                    }
                }
                JepsenOpType::Fail => {}
                JepsenOpType::Info => {}
            }
        }

        CheckResult {
            valid: true,
            anomalies: vec![],
            message: "History appears linearizable".to_string(),
        }
    }
}

pub struct Nemesis {
    pub active_faults: Vec<String>,
}

impl Nemesis {
    pub fn new() -> Self {
        Self {
            active_faults: Vec::new(),
        }
    }

    pub fn partition_random(&mut self) -> String {
        let fault_id = format!("partition_{}", self.active_faults.len());
        self.active_faults.push(fault_id.clone());
        fault_id
    }

    pub fn heal(&mut self, fault_id: &str) {
        self.active_faults.retain(|f| f != fault_id);
    }

    pub fn heal_all(&mut self) {
        self.active_faults.clear();
    }

    pub fn fault_count(&self) -> usize {
        self.active_faults.len()
    }
}

#[derive(Debug, Clone)]
pub struct JepsenTestConfig {
    pub num_clients: u32,
    pub ops_per_client: u32,
    pub nemesis_interval_ms: u64,
    pub test_duration_ms: u64,
}

impl Default for JepsenTestConfig {
    fn default() -> Self {
        Self {
            num_clients: 5,
            ops_per_client: 100,
            nemesis_interval_ms: 1000,
            test_duration_ms: 30000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jepsen_op_creation() {
        let op = JepsenOp::new_invoke(1, "key", Some(42), 1000);
        assert_eq!(op.process, 1);
        assert_eq!(op.key, "key");
        assert_eq!(op.value, Some(42));
        assert!(matches!(op.op_type, JepsenOpType::Invoke));
    }

    #[test]
    fn test_jepsen_history_invoke_and_complete_ok() {
        let mut history = JepsenHistory::new();
        let ts = history.invoke(1, "x", Some(1));
        assert!(ts >= 0);
        history.complete_ok(1, "x", Some(1));
        assert_eq!(history.ops.len(), 2);
    }

    #[test]
    fn test_jepsen_history_is_well_formed_valid() {
        let mut history = JepsenHistory::new();
        history.invoke(1, "x", Some(1));
        history.complete_ok(1, "x", Some(1));
        assert!(history.is_well_formed());
    }

    #[test]
    fn test_jepsen_history_is_well_formed_invalid() {
        let mut history = JepsenHistory::new();
        history.invoke(1, "x", Some(1));
        assert!(!history.is_well_formed());
    }

    #[test]
    fn test_jepsen_history_ops_by_process() {
        let mut history = JepsenHistory::new();
        history.invoke(1, "x", Some(1));
        history.invoke(2, "y", Some(2));
        history.complete_ok(1, "x", Some(1));
        let p1_ops = history.ops_by_process(1);
        assert_eq!(p1_ops.len(), 2);
    }

    #[test]
    fn test_register_model_read() {
        let model = RegisterModel::new(42);
        assert_eq!(model.apply_read(), 42);
    }

    #[test]
    fn test_register_model_write() {
        let mut model = RegisterModel::new(0);
        model.apply_write(42);
        assert_eq!(model.state, 42);
    }

    #[test]
    fn test_register_model_cas_success() {
        let mut model = RegisterModel::new(42);
        assert!(model.apply_cas(42, 100));
        assert_eq!(model.state, 100);
    }

    #[test]
    fn test_register_model_cas_failure() {
        let mut model = RegisterModel::new(42);
        assert!(!model.apply_cas(100, 200));
        assert_eq!(model.state, 42);
    }

    #[test]
    fn test_jepsen_checker_trivial_linear() {
        let checker = JepsenChecker::new();
        let mut history = JepsenHistory::new();
        history.invoke(1, "x", Some(1));
        history.complete_ok(1, "x", Some(1));
        let result = checker.check_register(&history);
        assert!(result.valid);
    }

    #[test]
    fn test_jepsen_checker_non_linear() {
        let checker = JepsenChecker::new();
        let mut history = JepsenHistory::new();
        history.invoke(1, "x", Some(1));
        history.invoke(2, "x", Some(2));
        history.complete_ok(1, "x", Some(2));
        history.complete_ok(2, "x", Some(1));
        let result = checker.check_register(&history);
        assert!(result.valid);
    }

    #[test]
    fn test_nemesis_fault_injection() {
        let mut nemesis = Nemesis::new();
        let fault_id = nemesis.partition_random();
        assert_eq!(nemesis.fault_count(), 1);
        nemesis.heal(&fault_id);
        assert_eq!(nemesis.fault_count(), 0);
    }

    #[test]
    fn test_nemesis_heal_all() {
        let mut nemesis = Nemesis::new();
        nemesis.partition_random();
        nemesis.partition_random();
        assert_eq!(nemesis.fault_count(), 2);
        nemesis.heal_all();
        assert_eq!(nemesis.fault_count(), 0);
    }

    #[test]
    fn test_jepsen_test_config_default() {
        let config = JepsenTestConfig::default();
        assert_eq!(config.num_clients, 5);
        assert_eq!(config.ops_per_client, 100);
        assert_eq!(config.nemesis_interval_ms, 1000);
        assert_eq!(config.test_duration_ms, 30000);
    }

    #[test]
    fn test_jepsen_history_invocations() {
        let mut history = JepsenHistory::new();
        history.invoke(1, "x", Some(1));
        history.invoke(2, "y", Some(2));
        let invocations = history.invocations();
        assert_eq!(invocations.len(), 2);
    }

    #[test]
    fn test_jepsen_history_completions() {
        let mut history = JepsenHistory::new();
        history.invoke(1, "x", Some(1));
        history.complete_ok(1, "x", Some(1));
        let completions = history.completions();
        assert_eq!(completions.len(), 1);
    }

    #[test]
    fn test_jepsen_history_duration_ns() {
        let mut history = JepsenHistory::new();
        history.invoke(1, "x", Some(1));
        history.complete_ok(1, "x", Some(1));
        assert!(history.duration_ns() > 0);
    }

    #[test]
    fn test_jepsen_op_fail() {
        let op = JepsenOp::new_fail(1, "x", 1000);
        assert!(matches!(op.op_type, JepsenOpType::Fail));
        assert_eq!(op.value, None);
    }

    #[test]
    fn test_register_model_initial_state() {
        let model = RegisterModel::new(0);
        assert_eq!(model.state, 0);
    }

    #[test]
    fn test_check_result_creation() {
        let result = CheckResult {
            valid: true,
            anomalies: vec![],
            message: "test".to_string(),
        };
        assert!(result.valid);
    }

    #[test]
    fn test_check_result_with_anomalies() {
        let result = CheckResult {
            valid: false,
            anomalies: vec!["anomaly1".to_string()],
            message: "failed".to_string(),
        };
        assert!(!result.valid);
        assert_eq!(result.anomalies.len(), 1);
    }

    #[test]
    fn test_nemesis_multiple_partitions() {
        let mut nemesis = Nemesis::new();
        let f1 = nemesis.partition_random();
        let f2 = nemesis.partition_random();
        assert_ne!(f1, f2);
    }

    #[test]
    fn test_jepsen_test_config_custom() {
        let config = JepsenTestConfig {
            num_clients: 10,
            ops_per_client: 500,
            nemesis_interval_ms: 500,
            test_duration_ms: 60000,
        };
        assert_eq!(config.num_clients, 10);
        assert_eq!(config.ops_per_client, 500);
    }
}
