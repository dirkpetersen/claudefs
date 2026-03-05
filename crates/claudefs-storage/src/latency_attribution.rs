use std::collections::HashMap;
use thiserror::Error;
use tracing::debug;

#[derive(Debug, Clone)]
pub struct LatencyStage {
    pub name: String,
    pub start_ns: u64,
    pub duration_ns: u64,
}

#[derive(Debug, Clone)]
pub struct OpLatencyTrace {
    pub op_id: u64,
    pub client_id: u32,
    pub op_type: String,
    pub stages: Vec<LatencyStage>,
    pub total_ns: u64,
    pub timestamp_ms: u64,
}

#[derive(Debug, Error)]
pub enum LatencyError {
    #[error("Operation not found: op_id {0}")]
    OpNotFound(u64),
    #[error("Operation already started: op_id {0}")]
    OpAlreadyStarted(u64),
    #[error("Invalid operation state: {0}")]
    InvalidState(String),
}

pub type LatencyResult<T> = Result<T, LatencyError>;

pub struct LatencyAttributor {
    active_ops: HashMap<u64, OpLatencyTrace>,
    stage_samples: HashMap<String, Vec<u64>>,
    percentiles: HashMap<String, (u64, u64, u64)>,
}

impl LatencyAttributor {
    pub fn new() -> Self {
        Self {
            active_ops: HashMap::new(),
            stage_samples: HashMap::new(),
            percentiles: HashMap::new(),
        }
    }

    pub fn start_op(&mut self, op_id: u64, client_id: u32, op_type: &str) -> LatencyResult<()> {
        if self.active_ops.contains_key(&op_id) {
            return Err(LatencyError::OpAlreadyStarted(op_id));
        }

        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let trace = OpLatencyTrace {
            op_id,
            client_id,
            op_type: op_type.to_string(),
            stages: Vec::new(),
            total_ns: 0,
            timestamp_ms,
        };

        self.active_ops.insert(op_id, trace);
        debug!(
            "Started operation tracking: op_id={}, client_id={}, op_type={}",
            op_id, client_id, op_type
        );
        Ok(())
    }

    pub fn record_stage(
        &mut self,
        op_id: u64,
        stage_name: &str,
        duration_ns: u64,
    ) -> LatencyResult<()> {
        let trace = self
            .active_ops
            .get_mut(&op_id)
            .ok_or(LatencyError::OpNotFound(op_id))?;

        let stage = LatencyStage {
            name: stage_name.to_string(),
            start_ns: trace.total_ns,
            duration_ns,
        };

        trace.total_ns += duration_ns;
        trace.stages.push(stage);

        let key = format!("{}:{}", trace.op_type, stage_name);
        self.stage_samples
            .entry(key)
            .or_insert_with(Vec::new)
            .push(duration_ns);

        debug!(
            "Recorded stage for op_id={}: {}={}ns",
            op_id, stage_name, duration_ns
        );
        Ok(())
    }

    pub fn finish_op(&mut self, op_id: u64) -> LatencyResult<OpLatencyTrace> {
        let trace = self
            .active_ops
            .remove(&op_id)
            .ok_or(LatencyError::OpNotFound(op_id))?;

        debug!(
            "Finished operation: op_id={}, total_ns={}",
            op_id, trace.total_ns
        );
        Ok(trace)
    }

    pub fn stage_percentile(&self, op_type: &str, stage: &str, percentile: u8) -> Option<u64> {
        let key = format!("{}:{}", op_type, stage);
        let samples = self.stage_samples.get(&key)?;
        if samples.is_empty() {
            return None;
        }

        let mut sorted = samples.clone();
        sorted.sort();

        let idx = match percentile {
            50 => sorted.len() * 50 / 100,
            95 => sorted.len() * 95 / 100,
            99 => sorted.len() * 99 / 100,
            p => sorted.len() * p as usize / 100,
        };

        let idx = idx.min(sorted.len() - 1);
        Some(sorted[idx])
    }

    pub fn sample_count(&self, key: &str) -> usize {
        self.stage_samples.get(key).map(|v| v.len()).unwrap_or(0)
    }

    pub fn reset(&mut self) {
        self.active_ops.clear();
        self.stage_samples.clear();
        self.percentiles.clear();
        debug!("LatencyAttributor reset");
    }

    pub fn update_percentiles(&mut self, key: &str) {
        if let Some(samples) = self.stage_samples.get(key) {
            if samples.is_empty() {
                return;
            }

            let mut sorted = samples.clone();
            sorted.sort();

            let p50 = sorted[sorted.len() * 50 / 100];
            let p95 = sorted[sorted.len() * 95 / 100];
            let p99 = sorted[sorted.len() * 99 / 100];

            self.percentiles.insert(key.to_string(), (p50, p95, p99));
        }
    }

    pub fn get_percentiles(&self, key: &str) -> Option<&(u64, u64, u64)> {
        self.percentiles.get(key)
    }
}

impl Default for LatencyAttributor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_attributor() -> LatencyAttributor {
        LatencyAttributor::new()
    }

    #[test]
    fn test_start_op() {
        let mut attr = create_attributor();
        assert!(attr.start_op(1, 100, "write").is_ok());
    }

    #[test]
    fn test_start_op_duplicate() {
        let mut attr = create_attributor();
        attr.start_op(1, 100, "write").unwrap();
        assert!(matches!(
            attr.start_op(1, 100, "write"),
            Err(LatencyError::OpAlreadyStarted(1))
        ));
    }

    #[test]
    fn test_record_stage() {
        let mut attr = create_attributor();
        attr.start_op(1, 100, "write").unwrap();
        attr.record_stage(1, "submission", 1000).unwrap();
        attr.record_stage(1, "device_io", 5000).unwrap();
        attr.record_stage(1, "completion", 500).unwrap();

        let trace = attr.finish_op(1).unwrap();
        assert_eq!(trace.stages.len(), 3);
        assert_eq!(trace.total_ns, 6500);
        assert_eq!(trace.stages[0].name, "submission");
        assert_eq!(trace.stages[1].name, "device_io");
        assert_eq!(trace.stages[2].name, "completion");
    }

    #[test]
    fn test_record_stage_nonexistent_op() {
        let mut attr = create_attributor();
        assert!(matches!(
            attr.record_stage(999, "submission", 1000),
            Err(LatencyError::OpNotFound(999))
        ));
    }

    #[test]
    fn test_finish_op() {
        let mut attr = create_attributor();
        attr.start_op(1, 100, "read").unwrap();
        attr.record_stage(1, "submission", 500).unwrap();
        attr.record_stage(1, "device_io", 3000).unwrap();

        let trace = attr.finish_op(1).unwrap();
        assert_eq!(trace.op_id, 1);
        assert_eq!(trace.client_id, 100);
        assert_eq!(trace.op_type, "read");
        assert_eq!(trace.total_ns, 3500);
    }

    #[test]
    fn test_finish_nonexistent_op() {
        let mut attr = create_attributor();
        assert!(matches!(
            attr.finish_op(999),
            Err(LatencyError::OpNotFound(999))
        ));
    }

    #[test]
    fn test_multiple_concurrent_operations() {
        let mut attr = create_attributor();

        attr.start_op(1, 100, "write").unwrap();
        attr.start_op(2, 101, "read").unwrap();
        attr.start_op(3, 102, "write").unwrap();

        attr.record_stage(1, "device_io", 1000).unwrap();
        attr.record_stage(2, "device_io", 2000).unwrap();
        attr.record_stage(3, "device_io", 1500).unwrap();

        let trace1 = attr.finish_op(1).unwrap();
        let trace2 = attr.finish_op(2).unwrap();
        let trace3 = attr.finish_op(3).unwrap();

        assert_eq!(trace1.op_id, 1);
        assert_eq!(trace2.op_id, 2);
        assert_eq!(trace3.op_id, 3);
    }

    #[test]
    fn test_stage_ordering() {
        let mut attr = create_attributor();
        attr.start_op(1, 100, "write").unwrap();
        attr.record_stage(1, "submission", 1000).unwrap();
        attr.record_stage(1, "device_io", 5000).unwrap();
        attr.record_stage(1, "completion", 500).unwrap();

        let trace = attr.finish_op(1).unwrap();

        assert!(trace.stages[0].start_ns < trace.stages[1].start_ns);
        assert!(trace.stages[1].start_ns < trace.stages[2].start_ns);
    }

    #[test]
    fn test_reset_clears_all() {
        let mut attr = create_attributor();
        attr.start_op(1, 100, "write").unwrap();
        attr.record_stage(1, "submission", 1000).unwrap();

        attr.reset();

        assert!(matches!(
            attr.record_stage(1, "device_io", 1000),
            Err(LatencyError::OpNotFound(1))
        ));
    }

    #[test]
    fn test_stage_percentile_p50() {
        let mut attr = create_attributor();

        for i in 1..=100 {
            attr.start_op(i, 100, "read").unwrap();
            attr.record_stage(i, "device_io", i as u64 * 1000).unwrap();
            attr.finish_op(i).unwrap();
        }

        attr.update_percentiles("read:device_io");

        let p50 = attr.stage_percentile("read", "device_io", 50);
        assert!(p50.is_some());
        assert!(p50.unwrap() >= 40000 && p50.unwrap() <= 60000);
    }

    #[test]
    fn test_stage_percentile_p95() {
        let mut attr = create_attributor();

        for i in 1..=100 {
            attr.start_op(i, 100, "write").unwrap();
            attr.record_stage(i, "device_io", i as u64 * 1000).unwrap();
            attr.finish_op(i).unwrap();
        }

        attr.update_percentiles("write:device_io");

        let p95 = attr.stage_percentile("write", "device_io", 95);
        assert!(p95.is_some());
        assert!(p95.unwrap() >= 90000);
    }

    #[test]
    fn test_stage_percentile_p99() {
        let mut attr = create_attributor();

        for i in 1..=100 {
            attr.start_op(i, 100, "read").unwrap();
            attr.record_stage(i, "completion", i as u64 * 100).unwrap();
            attr.finish_op(i).unwrap();
        }

        attr.update_percentiles("read:completion");

        let p99 = attr.stage_percentile("read", "completion", 99);
        assert!(p99.is_some());
    }

    #[test]
    fn test_stage_percentile_nonexistent() {
        let attr = create_attributor();
        assert!(attr.stage_percentile("nonexistent", "stage", 50).is_none());
    }

    #[test]
    fn test_sample_count() {
        let mut attr = create_attributor();

        assert_eq!(attr.sample_count("write:device_io"), 0);

        for i in 1..=5 {
            attr.start_op(i, 100, "write").unwrap();
            attr.record_stage(i, "device_io", 1000).unwrap();
            attr.finish_op(i).unwrap();
        }

        assert_eq!(attr.sample_count("write:device_io"), 5);
    }

    #[test]
    fn test_large_operation_sequences() {
        let mut attr = create_attributor();

        for i in 1..=1000 {
            attr.start_op(i, 100, "write").unwrap();
            attr.record_stage(i, "submission", 100).unwrap();
            attr.record_stage(i, "device_io", 1000).unwrap();
            attr.record_stage(i, "completion", 100).unwrap();
            attr.finish_op(i).unwrap();
        }

        assert_eq!(attr.sample_count("write:submission"), 1000);
        assert_eq!(attr.sample_count("write:device_io"), 1000);
        assert_eq!(attr.sample_count("write:completion"), 1000);
    }

    #[test]
    fn test_multiple_stage_types() {
        let mut attr = create_attributor();

        attr.start_op(1, 100, "write").unwrap();
        attr.record_stage(1, "submission", 500).unwrap();
        attr.record_stage(1, "device_io", 3000).unwrap();
        attr.record_stage(1, "completion", 200).unwrap();
        attr.finish_op(1).unwrap();

        attr.start_op(2, 101, "read").unwrap();
        attr.record_stage(2, "submission", 300).unwrap();
        attr.record_stage(2, "device_io", 2000).unwrap();
        attr.record_stage(2, "completion", 150).unwrap();
        attr.finish_op(2).unwrap();

        assert_eq!(attr.sample_count("write:submission"), 1);
        assert_eq!(attr.sample_count("write:device_io"), 1);
        assert_eq!(attr.sample_count("write:completion"), 1);
        assert_eq!(attr.sample_count("read:submission"), 1);
        assert_eq!(attr.sample_count("read:device_io"), 1);
        assert_eq!(attr.sample_count("read:completion"), 1);
    }

    #[test]
    fn test_latency_trace_fields() {
        let mut attr = create_attributor();
        attr.start_op(42, 200, "trim").unwrap();
        attr.record_stage(42, "submission", 100).unwrap();

        let trace = attr.finish_op(42).unwrap();

        assert_eq!(trace.op_id, 42);
        assert_eq!(trace.client_id, 200);
        assert_eq!(trace.op_type, "trim");
        assert!(trace.timestamp_ms > 0);
    }

    #[test]
    fn test_percentiles_p50_p95_p99() {
        let mut attr = create_attributor();

        for i in 1..=100 {
            attr.start_op(i, 100, "write").unwrap();
            attr.record_stage(i, "device_io", i as u64 * 1000).unwrap();
            attr.finish_op(i).unwrap();
        }

        attr.update_percentiles("write:device_io");

        let percs = attr.get_percentiles("write:device_io");
        assert!(percs.is_some());

        let (p50, p95, p99) = percs.unwrap();
        assert!(p50 <= p95);
        assert!(p95 <= p99);
    }

    #[test]
    fn test_concurrent_different_op_types() {
        let mut attr = create_attributor();

        attr.start_op(1, 100, "write").unwrap();
        attr.start_op(2, 101, "read").unwrap();
        attr.start_op(3, 102, "trim").unwrap();
        attr.start_op(4, 103, "write").unwrap();

        attr.record_stage(1, "device_io", 1000).unwrap();
        attr.record_stage(2, "device_io", 800).unwrap();
        attr.record_stage(3, "device_io", 200).unwrap();
        attr.record_stage(4, "device_io", 1200).unwrap();

        let write_samples = attr.sample_count("write:device_io");
        let read_samples = attr.sample_count("read:device_io");
        let trim_samples = attr.sample_count("trim:device_io");

        assert_eq!(write_samples, 2);
        assert_eq!(read_samples, 1);
        assert_eq!(trim_samples, 1);
    }

    #[test]
    fn test_stage_accumulation() {
        let mut attr = create_attributor();
        attr.start_op(1, 100, "write").unwrap();
        attr.record_stage(1, "device_io", 1000).unwrap();

        let trace = attr.finish_op(1).unwrap();
        assert_eq!(trace.stages.len(), 1);

        attr.start_op(2, 100, "write").unwrap();
        attr.record_stage(2, "device_io", 2000).unwrap();

        let trace = attr.finish_op(2).unwrap();
        assert_eq!(trace.stages.len(), 1);
    }

    #[test]
    fn test_empty_stage_samples() {
        let attr = create_attributor();
        assert_eq!(attr.sample_count("nonexistent"), 0);
    }

    #[test]
    fn test_finish_op_removes_from_active() {
        let mut attr = create_attributor();
        attr.start_op(1, 100, "write").unwrap();
        attr.finish_op(1).unwrap();

        assert!(matches!(
            attr.finish_op(1),
            Err(LatencyError::OpNotFound(1))
        ));
    }

    #[test]
    fn test_stage_duration_tracking() {
        let mut attr = create_attributor();
        attr.start_op(1, 100, "write").unwrap();
        attr.record_stage(1, "submission", 1000).unwrap();
        attr.record_stage(1, "device_io", 5000).unwrap();

        let trace = attr.finish_op(1).unwrap();

        assert_eq!(trace.stages[0].duration_ns, 1000);
        assert_eq!(trace.stages[1].duration_ns, 5000);
    }

    #[test]
    fn test_total_ns_calculation() {
        let mut attr = create_attributor();
        attr.start_op(1, 100, "write").unwrap();
        attr.record_stage(1, "submission", 100).unwrap();
        attr.record_stage(1, "device_io", 200).unwrap();
        attr.record_stage(1, "completion", 300).unwrap();

        let trace = attr.finish_op(1).unwrap();
        assert_eq!(trace.total_ns, 600);
    }

    #[test]
    fn test_different_client_ids() {
        let mut attr = create_attributor();

        for i in 1..=10 {
            attr.start_op(i, i as u32, "write").unwrap();
            attr.record_stage(i, "device_io", 1000).unwrap();
            attr.finish_op(i).unwrap();
        }

        for i in 1..=10 {
            let trace = attr.active_ops.get(&i);
            assert!(trace.is_none());
        }
    }

    #[test]
    fn test_mixed_percentile_queries() {
        let mut attr = create_attributor();

        for i in 1..=50 {
            attr.start_op(i, 100, "write").unwrap();
            attr.record_stage(i, "device_io", i as u64 * 100).unwrap();
            attr.finish_op(i).unwrap();
        }

        attr.update_percentiles("write:device_io");

        let p50 = attr.stage_percentile("write", "device_io", 50);
        let p95 = attr.stage_percentile("write", "device_io", 95);
        let p99 = attr.stage_percentile("write", "device_io", 99);

        assert!(p50.is_some());
        assert!(p95.is_some());
        assert!(p99.is_some());
    }

    #[test]
    fn test_reset_preserves_nothing() {
        let mut attr = create_attributor();

        for i in 1..=5 {
            attr.start_op(i, 100, "write").unwrap();
            attr.record_stage(i, "device_io", 1000).unwrap();
            attr.finish_op(i).unwrap();
        }

        attr.reset();

        assert_eq!(attr.sample_count("write:device_io"), 0);
        assert!(attr.get_percentiles("write:device_io").is_none());
    }

    #[test]
    fn test_op_type_string_stability() {
        let mut attr = create_attributor();
        attr.start_op(1, 100, "write").unwrap();
        attr.record_stage(1, "device_io", 1000).unwrap();

        let trace = attr.finish_op(1).unwrap();
        assert_eq!(trace.op_type, "write");

        let trace = attr.finish_op(1);
        assert!(trace.is_err());
    }

    #[test]
    fn test_stage_names_static() {
        let mut attr = create_attributor();
        attr.start_op(1, 100, "write").unwrap();
        attr.record_stage(1, "submission", 100).unwrap();
        attr.record_stage(1, "device_io", 200).unwrap();
        attr.record_stage(1, "completion", 300).unwrap();

        let trace = attr.finish_op(1).unwrap();

        for stage in &trace.stages {
            match stage.name.as_str() {
                "submission" | "device_io" | "completion" => {}
                _ => panic!("Unexpected stage name"),
            }
        }
    }

    #[test]
    fn test_many_small_stages() {
        let mut attr = create_attributor();
        attr.start_op(1, 100, "batch").unwrap();

        for i in 0..100 {
            attr.record_stage(1, "sub_op", 10).unwrap();
        }

        let trace = attr.finish_op(1).unwrap();
        assert_eq!(trace.stages.len(), 100);
    }

    #[test]
    fn test_stage_percentile_single_sample() {
        let mut attr = create_attributor();

        attr.start_op(1, 100, "write").unwrap();
        attr.record_stage(1, "device_io", 5000).unwrap();
        attr.finish_op(1).unwrap();

        attr.update_percentiles("write:device_io");

        let p50 = attr.stage_percentile("write", "device_io", 50);
        let p95 = attr.stage_percentile("write", "device_io", 95);
        let p99 = attr.stage_percentile("write", "device_io", 99);

        assert_eq!(p50, Some(5000));
        assert_eq!(p95, Some(5000));
        assert_eq!(p99, Some(5000));
    }

    #[test]
    fn test_sequential_same_op_type() {
        let mut attr = create_attributor();

        for i in 1..=10 {
            attr.start_op(i, 100, "write").unwrap();
            attr.record_stage(i, "device_io", i as u64 * 100).unwrap();
            attr.finish_op(i).unwrap();
        }

        let p50 = attr.stage_percentile("write", "device_io", 50);
        assert!(p50.is_some());
    }
}
