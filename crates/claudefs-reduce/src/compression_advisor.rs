//! Compression algorithm advisor based on observed ratios.

use crate::compression::CompressionAlgorithm;

#[derive(Debug, Clone, Default)]
pub struct AlgoMetrics {
    pub samples: u64,
    pub total_ratio: f64,
    pub total_latency_us: u64,
}

impl AlgoMetrics {
    pub fn avg_ratio(&self) -> f64 {
        if self.samples == 0 {
            1.0
        } else {
            self.total_ratio / self.samples as f64
        }
    }

    pub fn avg_latency_us(&self) -> u64 {
        if self.samples == 0 {
            0
        } else {
            self.total_latency_us / self.samples
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompressionAdvice {
    Use(CompressionAlgorithm),
    Skip,
}

pub struct CompressionAdvisor {
    lz4_metrics: AlgoMetrics,
    zstd_metrics: AlgoMetrics,
    min_ratio: f64,
}

impl CompressionAdvisor {
    pub fn new(min_ratio: f64) -> Self {
        Self {
            lz4_metrics: AlgoMetrics::default(),
            zstd_metrics: AlgoMetrics::default(),
            min_ratio,
        }
    }

    pub fn record(
        &mut self,
        algo: CompressionAlgorithm,
        original_bytes: u64,
        compressed_bytes: u64,
        latency_us: u64,
    ) {
        let ratio = if compressed_bytes == 0 {
            1.0
        } else {
            original_bytes as f64 / compressed_bytes as f64
        };
        match algo {
            CompressionAlgorithm::Lz4 => {
                self.lz4_metrics.samples += 1;
                self.lz4_metrics.total_ratio += ratio;
                self.lz4_metrics.total_latency_us += latency_us;
            }
            CompressionAlgorithm::Zstd { .. } => {
                self.zstd_metrics.samples += 1;
                self.zstd_metrics.total_ratio += ratio;
                self.zstd_metrics.total_latency_us += latency_us;
            }
            CompressionAlgorithm::None => {}
        }
    }

    pub fn advise_inline(&self) -> CompressionAdvice {
        if self.lz4_metrics.avg_ratio() >= self.min_ratio {
            CompressionAdvice::Use(CompressionAlgorithm::Lz4)
        } else {
            CompressionAdvice::Skip
        }
    }

    pub fn advise_background(&self) -> CompressionAdvice {
        if self.zstd_metrics.avg_ratio() >= self.min_ratio {
            CompressionAdvice::Use(CompressionAlgorithm::Zstd { level: 3 })
        } else {
            CompressionAdvice::Skip
        }
    }

    pub fn best_algorithm(&self) -> Option<CompressionAlgorithm> {
        let lz4_r = self.lz4_metrics.avg_ratio();
        let zstd_r = self.zstd_metrics.avg_ratio();
        if self.lz4_metrics.samples == 0 && self.zstd_metrics.samples == 0 {
            return None;
        }
        if zstd_r > lz4_r {
            Some(CompressionAlgorithm::Zstd { level: 3 })
        } else {
            Some(CompressionAlgorithm::Lz4)
        }
    }

    pub fn lz4_metrics(&self) -> &AlgoMetrics {
        &self.lz4_metrics
    }
    pub fn zstd_metrics(&self) -> &AlgoMetrics {
        &self.zstd_metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn algo_metrics_default() {
        let metrics = AlgoMetrics::default();
        assert_eq!(metrics.samples, 0);
        assert_eq!(metrics.avg_ratio(), 1.0);
        assert_eq!(metrics.avg_latency_us(), 0);
    }

    #[test]
    fn algo_metrics_avg_ratio() {
        let mut metrics = AlgoMetrics::default();
        metrics.total_ratio = 6.0;
        metrics.samples = 2;
        assert_eq!(metrics.avg_ratio(), 3.0);
    }

    #[test]
    fn algo_metrics_avg_latency() {
        let mut metrics = AlgoMetrics::default();
        metrics.total_latency_us = 200;
        metrics.samples = 2;
        assert_eq!(metrics.avg_latency_us(), 100);
    }

    #[test]
    fn record_lz4_updates_metrics() {
        let mut advisor = CompressionAdvisor::new(1.1);
        advisor.record(CompressionAlgorithm::Lz4, 1000, 500, 100);
        assert_eq!(advisor.lz4_metrics().samples, 1);
    }

    #[test]
    fn record_zstd_updates_metrics() {
        let mut advisor = CompressionAdvisor::new(1.1);
        advisor.record(CompressionAlgorithm::Zstd { level: 3 }, 1000, 400, 200);
        assert_eq!(advisor.zstd_metrics().samples, 1);
    }

    #[test]
    fn record_none_no_op() {
        let mut advisor = CompressionAdvisor::new(1.1);
        advisor.record(CompressionAlgorithm::None, 1000, 500, 100);
        assert_eq!(advisor.lz4_metrics().samples, 0);
        assert_eq!(advisor.zstd_metrics().samples, 0);
    }

    #[test]
    fn record_ratio_computed_correctly() {
        let mut advisor = CompressionAdvisor::new(1.0);
        advisor.record(CompressionAlgorithm::Lz4, 1000, 500, 0);
        assert_eq!(advisor.lz4_metrics().avg_ratio(), 2.0);
    }

    #[test]
    fn advise_inline_skip_no_samples() {
        let advisor = CompressionAdvisor::new(1.1);
        let advice = advisor.advise_inline();
        assert_eq!(advice, CompressionAdvice::Skip);
    }

    #[test]
    fn advise_inline_use_lz4() {
        let mut advisor = CompressionAdvisor::new(1.1);
        advisor.record(CompressionAlgorithm::Lz4, 1000, 500, 0);
        let advice = advisor.advise_inline();
        assert_eq!(advice, CompressionAdvice::Use(CompressionAlgorithm::Lz4));
    }

    #[test]
    fn advise_inline_skip_bad_ratio() {
        let mut advisor = CompressionAdvisor::new(1.5);
        advisor.record(CompressionAlgorithm::Lz4, 1000, 900, 0);
        let advice = advisor.advise_inline();
        assert_eq!(advice, CompressionAdvice::Skip);
    }

    #[test]
    fn advise_background_skip_no_samples() {
        let advisor = CompressionAdvisor::new(1.1);
        let advice = advisor.advise_background();
        assert_eq!(advice, CompressionAdvice::Skip);
    }

    #[test]
    fn advise_background_use_zstd() {
        let mut advisor = CompressionAdvisor::new(1.1);
        advisor.record(CompressionAlgorithm::Zstd { level: 3 }, 1000, 400, 0);
        let advice = advisor.advise_background();
        assert_eq!(
            advice,
            CompressionAdvice::Use(CompressionAlgorithm::Zstd { level: 3 })
        );
    }

    #[test]
    fn advise_background_skip_bad_ratio() {
        let mut advisor = CompressionAdvisor::new(1.5);
        advisor.record(CompressionAlgorithm::Zstd { level: 3 }, 1000, 900, 0);
        let advice = advisor.advise_background();
        assert_eq!(advice, CompressionAdvice::Skip);
    }

    #[test]
    fn best_algorithm_none_when_no_samples() {
        let advisor = CompressionAdvisor::new(1.0);
        assert_eq!(advisor.best_algorithm(), None);
    }

    #[test]
    fn best_algorithm_prefers_zstd_when_better() {
        let mut advisor = CompressionAdvisor::new(1.0);
        advisor.record(CompressionAlgorithm::Lz4, 1000, 600, 0);
        advisor.record(CompressionAlgorithm::Zstd { level: 3 }, 1000, 400, 0);
        assert_eq!(
            advisor.best_algorithm(),
            Some(CompressionAlgorithm::Zstd { level: 3 })
        );
    }

    #[test]
    fn best_algorithm_prefers_lz4_when_better() {
        let mut advisor = CompressionAdvisor::new(1.0);
        advisor.record(CompressionAlgorithm::Lz4, 1000, 400, 0);
        advisor.record(CompressionAlgorithm::Zstd { level: 3 }, 1000, 600, 0);
        assert_eq!(advisor.best_algorithm(), Some(CompressionAlgorithm::Lz4));
    }

    #[test]
    fn best_algorithm_equal_ratio_returns_lz4() {
        let mut advisor = CompressionAdvisor::new(1.0);
        advisor.record(CompressionAlgorithm::Lz4, 1000, 500, 0);
        advisor.record(CompressionAlgorithm::Zstd { level: 3 }, 1000, 500, 0);
        assert_eq!(advisor.best_algorithm(), Some(CompressionAlgorithm::Lz4));
    }

    #[test]
    fn best_algorithm_only_lz4_sampled() {
        let mut advisor = CompressionAdvisor::new(1.0);
        advisor.record(CompressionAlgorithm::Lz4, 1000, 500, 0);
        assert_eq!(advisor.best_algorithm(), Some(CompressionAlgorithm::Lz4));
    }

    #[test]
    fn best_algorithm_only_zstd_sampled() {
        let mut advisor = CompressionAdvisor::new(1.0);
        advisor.record(CompressionAlgorithm::Zstd { level: 3 }, 1000, 500, 0);
        assert_eq!(
            advisor.best_algorithm(),
            Some(CompressionAlgorithm::Zstd { level: 3 })
        );
    }

    #[test]
    fn min_ratio_threshold_boundary() {
        let mut advisor = CompressionAdvisor::new(2.0);
        advisor.record(CompressionAlgorithm::Lz4, 1000, 500, 0);
        let advice = advisor.advise_inline();
        assert_eq!(advice, CompressionAdvice::Use(CompressionAlgorithm::Lz4));
    }

    #[test]
    fn multiple_records_accumulate() {
        let mut advisor = CompressionAdvisor::new(1.0);
        for _ in 0..5 {
            advisor.record(CompressionAlgorithm::Lz4, 1000, 500, 0);
        }
        assert_eq!(advisor.lz4_metrics().samples, 5);
    }

    #[test]
    fn compression_advice_use_eq() {
        assert_eq!(
            CompressionAdvice::Use(CompressionAlgorithm::Lz4),
            CompressionAdvice::Use(CompressionAlgorithm::Lz4)
        );
        assert_ne!(
            CompressionAdvice::Use(CompressionAlgorithm::Lz4),
            CompressionAdvice::Use(CompressionAlgorithm::Zstd { level: 3 })
        );
    }
}
