use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CapacityError {
    #[error("Insufficient data points for projection: need {need}, have {have}")]
    InsufficientData { need: usize, have: usize },
    #[error("Invalid time range")]
    InvalidTimeRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapacityDataPoint {
    pub timestamp: u64,
    pub used_bytes: u64,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub inode_count: u64,
}

impl CapacityDataPoint {
    pub fn new(timestamp: u64, used_bytes: u64, total_bytes: u64, inode_count: u64) -> Self {
        Self {
            timestamp,
            used_bytes,
            total_bytes,
            free_bytes: total_bytes.saturating_sub(used_bytes),
            inode_count,
        }
    }

    pub fn usage_percent(&self) -> f64 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        (self.used_bytes as f64 / self.total_bytes as f64) * 100.0
    }

    pub fn free_percent(&self) -> f64 {
        if self.total_bytes == 0 {
            return 100.0;
        }
        (self.free_bytes as f64 / self.total_bytes as f64) * 100.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinearTrend {
    pub slope_bytes_per_sec: f64,
    pub intercept_bytes: f64,
    pub r_squared: f64,
}

impl LinearTrend {
    pub fn predict_at(&self, timestamp: u64) -> f64 {
        self.slope_bytes_per_sec * (timestamp as f64) + self.intercept_bytes
    }

    pub fn daily_growth_bytes(&self) -> f64 {
        self.slope_bytes_per_sec * 86400.0
    }

    pub fn weekly_growth_bytes(&self) -> f64 {
        self.slope_bytes_per_sec * 604800.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapacityProjection {
    pub days_until_full: Option<f64>,
    pub days_until_80_percent: Option<f64>,
    pub projected_used_7d: u64,
    pub projected_used_30d: u64,
    pub projected_used_90d: u64,
    pub trend: LinearTrend,
    pub confidence: f64,
}

impl CapacityProjection {
    pub fn is_urgent(&self) -> bool {
        self.days_until_full.map_or(false, |d| d < 30.0)
    }

    pub fn is_warning(&self) -> bool {
        self.days_until_full.map_or(false, |d| d < 90.0)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CapacityRecommendation {
    Sufficient,
    PlanExpansion { days_until_full: u64 },
    OrderImmediately { days_until_full: u64 },
    Emergency,
}

pub struct CapacityPlanner {
    history: VecDeque<CapacityDataPoint>,
    max_history: usize,
}

impl CapacityPlanner {
    pub fn new(max_history: usize) -> Self {
        Self {
            history: VecDeque::with_capacity(max_history),
            max_history,
        }
    }

    pub fn add_data_point(&mut self, point: CapacityDataPoint) {
        if self.history.len() >= self.max_history {
            self.history.pop_front();
        }
        self.history.push_back(point);
    }

    pub fn data_point_count(&self) -> usize {
        self.history.len()
    }

    pub fn compute_trend(&self) -> Result<LinearTrend, CapacityError> {
        let n = self.history.len();
        if n < 2 {
            return Err(CapacityError::InsufficientData { need: 2, have: n });
        }

        let mut sum_x: f64 = 0.0;
        let mut sum_y: f64 = 0.0;
        let mut sum_xy: f64 = 0.0;
        let mut sum_xx: f64 = 0.0;

        for point in &self.history {
            let x = point.timestamp as f64;
            let y = point.used_bytes as f64;
            sum_x += x;
            sum_y += y;
            sum_xy += x * y;
            sum_xx += x * x;
        }

        let denominator = (n as f64 * sum_xx) - (sum_x * sum_x);
        if denominator == 0.0 {
            return Err(CapacityError::InvalidTimeRange);
        }

        let slope = ((n as f64 * sum_xy) - (sum_x * sum_y)) / denominator;
        let intercept = (sum_y - slope * sum_x) / (n as f64);

        let mean_y = sum_y / (n as f64);
        let mut ss_tot: f64 = 0.0;
        let mut ss_res: f64 = 0.0;

        for point in &self.history {
            let x = point.timestamp as f64;
            let y = point.used_bytes as f64;
            let predicted = slope * x + intercept;
            ss_tot += (y - mean_y).powi(2);
            ss_res += (y - predicted).powi(2);
        }

        let r_squared = if ss_tot > 0.0 {
            1.0 - (ss_res / ss_tot)
        } else {
            1.0
        };

        Ok(LinearTrend {
            slope_bytes_per_sec: slope,
            intercept_bytes: intercept,
            r_squared,
        })
    }

    pub fn project(&self) -> Result<CapacityProjection, CapacityError> {
        let trend = self.compute_trend()?;

        let latest = self
            .latest()
            .ok_or(CapacityError::InsufficientData { need: 1, have: 0 })?;
        let now = latest.timestamp;

        let days_until_full = if trend.slope_bytes_per_sec > 0.0 {
            let remaining_bytes = (latest.total_bytes as f64) - (latest.used_bytes as f64);
            if remaining_bytes > 0.0 {
                Some(remaining_bytes / (trend.slope_bytes_per_sec * 86400.0))
            } else {
                Some(0.0)
            }
        } else {
            None
        };

        let days_until_80_percent = if trend.slope_bytes_per_sec > 0.0 {
            let target_bytes = latest.total_bytes as f64 * 0.8;
            let remaining_for_80 = target_bytes - latest.used_bytes as f64;
            if remaining_for_80 > 0.0 {
                Some(remaining_for_80 / (trend.slope_bytes_per_sec * 86400.0))
            } else {
                Some(0.0)
            }
        } else {
            None
        };

        let projected_used_7d = trend.predict_at(now + 604800) as u64;
        let projected_used_30d = trend.predict_at(now + 2592000) as u64;
        let projected_used_90d = trend.predict_at(now + 7776000) as u64;

        Ok(CapacityProjection {
            days_until_full,
            days_until_80_percent,
            projected_used_7d,
            projected_used_30d,
            projected_used_90d,
            trend: trend.clone(),
            confidence: trend.r_squared,
        })
    }

    pub fn recommendation(&self) -> CapacityRecommendation {
        if self.history.is_empty() {
            return CapacityRecommendation::Sufficient;
        }

        let projection = match self.project() {
            Ok(p) => p,
            Err(_) => return CapacityRecommendation::Sufficient,
        };

        if let Some(days) = projection.days_until_full {
            let usage_percent = self.latest().map(|p| p.usage_percent()).unwrap_or(0.0);

            if days < 7.0 || usage_percent >= 95.0 {
                CapacityRecommendation::Emergency
            } else if days < 30.0 {
                CapacityRecommendation::OrderImmediately {
                    days_until_full: days as u64,
                }
            } else if days < 90.0 {
                CapacityRecommendation::PlanExpansion {
                    days_until_full: days as u64,
                }
            } else {
                CapacityRecommendation::Sufficient
            }
        } else if let Some(latest) = self.latest() {
            if latest.usage_percent() >= 90.0 {
                CapacityRecommendation::OrderImmediately { days_until_full: 0 }
            } else {
                CapacityRecommendation::Sufficient
            }
        } else {
            CapacityRecommendation::Sufficient
        }
    }

    pub fn data_in_range(&self, from: u64, to: u64) -> Vec<&CapacityDataPoint> {
        if from > to {
            return vec![];
        }
        self.history
            .iter()
            .filter(|p| p.timestamp >= from && p.timestamp <= to)
            .collect()
    }

    pub fn latest(&self) -> Option<&CapacityDataPoint> {
        self.history.back()
    }

    pub fn average_daily_growth(&self) -> f64 {
        match self.compute_trend() {
            Ok(trend) => trend.daily_growth_bytes(),
            Err(_) => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capacity_data_point_new() {
        let point = CapacityDataPoint::new(1000, 500_000_000_000, 1_000_000_000_000, 1_000_000);
        assert_eq!(point.used_bytes, 500_000_000_000);
        assert_eq!(point.total_bytes, 1_000_000_000_000);
        assert_eq!(point.free_bytes, 500_000_000_000);
    }

    #[test]
    fn test_capacity_data_point_usage_percent() {
        let point = CapacityDataPoint::new(1000, 250_000_000_000, 1_000_000_000_000, 1_000_000);
        assert_eq!(point.usage_percent(), 25.0);
    }

    #[test]
    fn test_capacity_data_point_free_percent() {
        let point = CapacityDataPoint::new(1000, 750_000_000_000, 1_000_000_000_000, 1_000_000);
        assert_eq!(point.free_percent(), 25.0);
    }

    #[test]
    fn test_linear_trend_predict_at() {
        let trend = LinearTrend {
            slope_bytes_per_sec: 1000.0,
            intercept_bytes: 10000.0,
            r_squared: 0.9,
        };
        let predicted = trend.predict_at(1000);
        assert!((predicted - 1_010_000.0).abs() < 1.0);
    }

    #[test]
    fn test_linear_trend_daily_growth_bytes() {
        let trend = LinearTrend {
            slope_bytes_per_sec: 1000.0,
            intercept_bytes: 0.0,
            r_squared: 0.9,
        };
        assert_eq!(trend.daily_growth_bytes(), 1000.0 * 86400.0);
    }

    #[test]
    fn test_linear_trend_weekly_growth_bytes() {
        let trend = LinearTrend {
            slope_bytes_per_sec: 1000.0,
            intercept_bytes: 0.0,
            r_squared: 0.9,
        };
        assert_eq!(trend.weekly_growth_bytes(), 1000.0 * 604800.0);
    }

    #[test]
    fn test_capacity_projection_is_urgent() {
        let proj = CapacityProjection {
            days_until_full: Some(25.0),
            days_until_80_percent: Some(50.0),
            projected_used_7d: 0,
            projected_used_30d: 0,
            projected_used_90d: 0,
            trend: LinearTrend {
                slope_bytes_per_sec: 1.0,
                intercept_bytes: 0.0,
                r_squared: 0.9,
            },
            confidence: 0.9,
        };
        assert!(proj.is_urgent());

        let proj2 = CapacityProjection {
            days_until_full: Some(35.0),
            ..proj
        };
        assert!(!proj2.is_urgent());
    }

    #[test]
    fn test_capacity_projection_is_warning() {
        let proj = CapacityProjection {
            days_until_full: Some(85.0),
            days_until_80_percent: Some(50.0),
            projected_used_7d: 0,
            projected_used_30d: 0,
            projected_used_90d: 0,
            trend: LinearTrend {
                slope_bytes_per_sec: 1.0,
                intercept_bytes: 0.0,
                r_squared: 0.9,
            },
            confidence: 0.9,
        };
        assert!(proj.is_warning());

        let proj2 = CapacityProjection {
            days_until_full: Some(95.0),
            ..proj
        };
        assert!(!proj2.is_warning());
    }

    #[test]
    fn test_capacity_planner_new() {
        let planner = CapacityPlanner::new(100);
        assert_eq!(planner.data_point_count(), 0);
    }

    #[test]
    fn test_capacity_planner_add_data_point() {
        let mut planner = CapacityPlanner::new(100);
        let point = CapacityDataPoint::new(1000, 500_000_000_000, 1_000_000_000_000, 1_000_000);
        planner.add_data_point(point);
        assert_eq!(planner.data_point_count(), 1);
    }

    #[test]
    fn test_capacity_planner_max_history_eviction() {
        let mut planner = CapacityPlanner::new(3);
        for i in 0..5 {
            let point = CapacityDataPoint::new(
                1000 + i as u64 * 100,
                500_000_000_000,
                1_000_000_000_000,
                1_000_000,
            );
            planner.add_data_point(point);
        }
        assert_eq!(planner.data_point_count(), 3);
        assert!(planner.latest().unwrap().timestamp > 1300);
    }

    #[test]
    fn test_capacity_planner_compute_trend_insufficient_data() {
        let planner = CapacityPlanner::new(100);
        let result = planner.compute_trend();
        assert!(matches!(
            result,
            Err(CapacityError::InsufficientData { need: 2, have: 0 })
        ));
    }

    #[test]
    fn test_capacity_planner_compute_trend_positive_slope() {
        let mut planner = CapacityPlanner::new(100);
        for i in 0..10 {
            let point = CapacityDataPoint::new(
                1000 + i as u64 * 100,
                500_000_000_000 + (i as u64 * 1_000_000_000),
                1_000_000_000_000,
                1_000_000,
            );
            planner.add_data_point(point);
        }
        let trend = planner.compute_trend().unwrap();
        assert!(trend.slope_bytes_per_sec > 0.0);
    }

    #[test]
    fn test_capacity_planner_compute_trend_r_squared_perfect() {
        let mut planner = CapacityPlanner::new(100);
        let base = 1_000_000_000_000u64;
        for i in 0..10 {
            let used = base + (i as u64 * 1_000_000_000);
            let point =
                CapacityDataPoint::new(1000 + i as u64 * 100, used, 2_000_000_000_000, 1_000_000);
            planner.add_data_point(point);
        }
        let trend = planner.compute_trend().unwrap();
        assert!((trend.r_squared - 1.0).abs() < 0.0001);
    }

    #[test]
    fn test_capacity_planner_project_days_until_full() {
        let mut planner = CapacityPlanner::new(100);
        for i in 0..10 {
            let point = CapacityDataPoint::new(
                1000 + i as u64 * 86400,
                500_000_000_000 + (i as u64 * 1_000_000_000),
                1_000_000_000_000,
                1_000_000,
            );
            planner.add_data_point(point);
        }
        let projection = planner.project().unwrap();
        assert!(projection.days_until_full.is_some());
    }

    #[test]
    fn test_capacity_planner_project_declining_usage() {
        let mut planner = CapacityPlanner::new(100);
        for i in 0..10 {
            let point = CapacityDataPoint::new(
                1000 + i as u64 * 86400,
                500_000_000_000 - (i as u64 * 1_000_000_000),
                1_000_000_000_000,
                1_000_000,
            );
            planner.add_data_point(point);
        }
        let projection = planner.project().unwrap();
        assert!(projection.days_until_full.is_none());
    }

    #[test]
    fn test_capacity_planner_recommendation_sufficient() {
        let mut planner = CapacityPlanner::new(100);
        for i in 0..10 {
            let point = CapacityDataPoint::new(
                1000 + i as u64 * 86400,
                100_000_000_000,
                1_000_000_000_000,
                1_000_000,
            );
            planner.add_data_point(point);
        }
        let rec = planner.recommendation();
        assert!(matches!(rec, CapacityRecommendation::Sufficient));
    }

    #[test]
    fn test_capacity_planner_recommendation_plan_expansion() {
        let mut planner = CapacityPlanner::new(100);
        for i in 0..10 {
            let point = CapacityDataPoint::new(
                1000 + i as u64 * 86400,
                500_000_000_000 + (i as u64 * 10_000_000_000),
                1_000_000_000_000,
                1_000_000,
            );
            planner.add_data_point(point);
        }
        let rec = planner.recommendation();
        assert!(matches!(rec, CapacityRecommendation::PlanExpansion { .. }));
    }

    #[test]
    fn test_capacity_planner_recommendation_emergency() {
        let mut planner = CapacityPlanner::new(100);
        for i in 0..10 {
            let point = CapacityDataPoint::new(
                1000 + i as u64 * 86400,
                950_000_000_000 + (i as u64 * 1_000_000_000),
                1_000_000_000_000,
                1_000_000,
            );
            planner.add_data_point(point);
        }
        let rec = planner.recommendation();
        assert!(matches!(rec, CapacityRecommendation::Emergency));
    }

    #[test]
    fn test_capacity_planner_data_in_range() {
        let mut planner = CapacityPlanner::new(100);
        for i in 0..10 {
            let point = CapacityDataPoint::new(
                1000 + i as u64 * 100,
                500_000_000_000,
                1_000_000_000_000,
                1_000_000,
            );
            planner.add_data_point(point);
        }
        let in_range = planner.data_in_range(1100, 1500);
        assert_eq!(in_range.len(), 5);
    }

    #[test]
    fn test_capacity_planner_latest() {
        let mut planner = CapacityPlanner::new(100);
        for i in 0..5 {
            let point = CapacityDataPoint::new(
                1000 + i as u64 * 100,
                500_000_000_000,
                1_000_000_000_000,
                1_000_000,
            );
            planner.add_data_point(point);
        }
        let latest = planner.latest().unwrap();
        assert_eq!(latest.timestamp, 1400);
    }

    #[test]
    fn test_capacity_planner_average_daily_growth() {
        let mut planner = CapacityPlanner::new(100);
        for i in 0..10 {
            let point = CapacityDataPoint::new(
                1000 + i as u64 * 86400,
                500_000_000_000 + (i as u64 * 1_000_000_000),
                1_000_000_000_000,
                1_000_000,
            );
            planner.add_data_point(point);
        }
        let avg_growth = planner.average_daily_growth();
        assert!(avg_growth > 0.0);
    }
}
