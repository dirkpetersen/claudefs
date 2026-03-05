pub struct TieringMetrics {
    records: Vec<(String, u64, u64)>,
}

impl TieringMetrics {
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
        }
    }

    pub fn record(&mut self, op: &str, bytes: u64, latency_ns: u64) {
        self.records.push((op.to_string(), bytes, latency_ns));
    }

    pub fn avg_latency(&self, op: &str) -> f64 {
        let matching: Vec<_> = self.records.iter().filter(|(o, _, _)| o == op).collect();
        if matching.is_empty() {
            0.0
        } else {
            matching.iter().map(|(_, _, l)| *l as f64).sum::<f64>() / matching.len() as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_record() {
        let mut m = TieringMetrics::new();
        m.record("evict", 100, 50);
    }
    #[test]
    fn test_avg() {
        let mut m = TieringMetrics::new();
        m.record("evict", 100, 100);
        assert_eq!(m.avg_latency("evict"), 100.0);
    }
}
