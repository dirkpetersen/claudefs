use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ResourceLimiterError {
    #[error("Tenant not found: {0}")]
    TenantNotFound(String),
    #[error("Invalid limit configuration: {0}")]
    InvalidConfig(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SoftLimitThreshold {
    Percent(u32),
    Bytes(u64),
}

impl SoftLimitThreshold {
    pub fn to_bytes(&self, hard_limit: u64) -> u64 {
        match self {
            SoftLimitThreshold::Percent(pct) => hard_limit * (*pct as u64) / 100,
            SoftLimitThreshold::Bytes(bytes) => *bytes,
        }
    }

    pub fn as_percent(&self, hard_limit: u64) -> Option<f64> {
        match self {
            SoftLimitThreshold::Percent(pct) => Some(*pct as f64),
            SoftLimitThreshold::Bytes(bytes) if hard_limit > 0 => {
                Some((*bytes as f64 / hard_limit as f64) * 100.0)
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaEnforcer {
    pub tenant_id: String,
    pub hard_limit: u64,
    pub soft_limit: SoftLimitThreshold,
    pub current_usage: u64,
}

impl QuotaEnforcer {
    pub fn new(tenant_id: String, hard_limit: u64, soft_limit: SoftLimitThreshold) -> Self {
        Self {
            tenant_id,
            hard_limit,
            soft_limit,
            current_usage: 0,
        }
    }

    pub fn soft_threshold_bytes(&self) -> u64 {
        self.soft_limit.to_bytes(self.hard_limit)
    }

    pub fn usage_percent(&self) -> f64 {
        if self.hard_limit == 0 {
            return 0.0;
        }
        (self.current_usage as f64 / self.hard_limit as f64) * 100.0
    }

    pub fn at_soft_limit(&self) -> bool {
        self.current_usage >= self.soft_threshold_bytes()
    }

    pub fn exceeded(&self) -> bool {
        self.current_usage > self.hard_limit
    }

    pub fn bytes_available(&self) -> u64 {
        self.hard_limit.saturating_sub(self.current_usage)
    }

    pub fn add_usage(&mut self, bytes: i64) -> u64 {
        if bytes >= 0 {
            self.current_usage = self.current_usage.saturating_add(bytes as u64);
        } else {
            let abs_bytes = (-bytes) as u64;
            self.current_usage = self.current_usage.saturating_sub(abs_bytes);
        }
        self.current_usage
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum LimitCheckResult {
    Ok,
    SoftLimitWarning { usage_pct: f64 },
    HardLimitExceeded { overage_bytes: u64 },
}

impl LimitCheckResult {
    pub fn is_ok(&self) -> bool {
        matches!(self, LimitCheckResult::Ok)
    }

    pub fn is_soft_warning(&self) -> bool {
        matches!(self, LimitCheckResult::SoftLimitWarning { .. })
    }

    pub fn is_hard_exceeded(&self) -> bool {
        matches!(self, LimitCheckResult::HardLimitExceeded { .. })
    }
}

pub struct ResourceLimiterRegistry {
    enforcers: HashMap<String, QuotaEnforcer>,
}

impl ResourceLimiterRegistry {
    pub fn new() -> Self {
        Self {
            enforcers: HashMap::new(),
        }
    }

    pub fn register(
        &mut self,
        tenant_id: &str,
        hard_limit: u64,
        soft_limit: SoftLimitThreshold,
    ) -> Result<(), ResourceLimiterError> {
        if tenant_id.is_empty() {
            return Err(ResourceLimiterError::InvalidConfig(
                "tenant ID cannot be empty".to_string(),
            ));
        }

        if let SoftLimitThreshold::Percent(pct) = soft_limit {
            if pct > 100 {
                return Err(ResourceLimiterError::InvalidConfig(
                    "soft limit percent cannot exceed 100".to_string(),
                ));
            }
        }

        let enforcer = QuotaEnforcer::new(tenant_id.to_string(), hard_limit, soft_limit);
        self.enforcers.insert(tenant_id.to_string(), enforcer);
        Ok(())
    }

    pub fn check_and_update(
        &mut self,
        tenant_id: &str,
        bytes_delta: i64,
    ) -> Result<LimitCheckResult, ResourceLimiterError> {
        let enforcer = self
            .enforcers
            .get_mut(tenant_id)
            .ok_or_else(|| ResourceLimiterError::TenantNotFound(tenant_id.to_string()))?;

        let new_usage = enforcer.add_usage(bytes_delta);
        let usage_pct = if enforcer.hard_limit > 0 {
            (new_usage as f64 / enforcer.hard_limit as f64) * 100.0
        } else {
            0.0
        };

        let result = if new_usage > enforcer.hard_limit {
            let overage = new_usage - enforcer.hard_limit;
            LimitCheckResult::HardLimitExceeded {
                overage_bytes: overage,
            }
        } else if enforcer.at_soft_limit() {
            LimitCheckResult::SoftLimitWarning { usage_pct }
        } else {
            LimitCheckResult::Ok
        };

        Ok(result)
    }

    pub fn get_enforcer(&self, tenant_id: &str) -> Option<&QuotaEnforcer> {
        self.enforcers.get(tenant_id)
    }

    pub fn get_enforcer_mut(&mut self, tenant_id: &str) -> Option<&mut QuotaEnforcer> {
        self.enforcers.get_mut(tenant_id)
    }

    pub fn soft_limit_warnings(&self) -> Vec<(String, f64)> {
        self.enforcers
            .iter()
            .filter(|(_, e)| e.at_soft_limit() && !e.exceeded())
            .map(|(id, e)| (id.clone(), e.usage_percent()))
            .collect()
    }

    pub fn over_hard_limit(&self) -> Vec<&QuotaEnforcer> {
        self.enforcers.values().filter(|e| e.exceeded()).collect()
    }

    pub fn tenant_ids(&self) -> Vec<&str> {
        self.enforcers.keys().map(|k| k.as_str()).collect()
    }

    pub fn tenant_count(&self) -> usize {
        self.enforcers.len()
    }

    pub fn unregister(&mut self, tenant_id: &str) -> Option<QuotaEnforcer> {
        self.enforcers.remove(tenant_id)
    }

    pub fn clear(&mut self) {
        self.enforcers.clear();
    }
}

impl Default for ResourceLimiterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_soft_limit_threshold_percent_to_bytes() {
        let threshold = SoftLimitThreshold::Percent(80);
        assert_eq!(threshold.to_bytes(1000), 800);
    }

    #[test]
    fn test_soft_limit_threshold_bytes_to_bytes() {
        let threshold = SoftLimitThreshold::Bytes(500);
        assert_eq!(threshold.to_bytes(1000), 500);
    }

    #[test]
    fn test_soft_limit_threshold_percent_as_percent() {
        let threshold = SoftLimitThreshold::Percent(80);
        assert_eq!(threshold.as_percent(1000), Some(80.0));
    }

    #[test]
    fn test_soft_limit_threshold_bytes_as_percent() {
        let threshold = SoftLimitThreshold::Bytes(500);
        assert_eq!(threshold.as_percent(1000), Some(50.0));
    }

    #[test]
    fn test_quota_enforcer_new() {
        let enforcer =
            QuotaEnforcer::new("tenant1".to_string(), 1000, SoftLimitThreshold::Percent(80));

        assert_eq!(enforcer.tenant_id, "tenant1");
        assert_eq!(enforcer.hard_limit, 1000);
        assert_eq!(enforcer.current_usage, 0);
    }

    #[test]
    fn test_quota_enforcer_soft_threshold_bytes() {
        let enforcer =
            QuotaEnforcer::new("tenant1".to_string(), 1000, SoftLimitThreshold::Percent(80));

        assert_eq!(enforcer.soft_threshold_bytes(), 800);
    }

    #[test]
    fn test_quota_enforcer_usage_percent() {
        let mut enforcer =
            QuotaEnforcer::new("tenant1".to_string(), 1000, SoftLimitThreshold::Percent(80));

        enforcer.add_usage(500);
        assert_eq!(enforcer.usage_percent(), 50.0);
    }

    #[test]
    fn test_quota_enforcer_at_soft_limit() {
        let mut enforcer =
            QuotaEnforcer::new("tenant1".to_string(), 1000, SoftLimitThreshold::Percent(80));

        enforcer.add_usage(800);
        assert!(enforcer.at_soft_limit());
    }

    #[test]
    fn test_quota_enforcer_at_soft_limit_not_exceeded() {
        let mut enforcer =
            QuotaEnforcer::new("tenant1".to_string(), 1000, SoftLimitThreshold::Percent(80));

        enforcer.add_usage(750);
        assert!(enforcer.at_soft_limit());
        assert!(!enforcer.exceeded());
    }

    #[test]
    fn test_quota_enforcer_exceeded() {
        let mut enforcer =
            QuotaEnforcer::new("tenant1".to_string(), 1000, SoftLimitThreshold::Percent(80));

        enforcer.add_usage(1001);
        assert!(enforcer.exceeded());
    }

    #[test]
    fn test_quota_enforcer_bytes_available() {
        let mut enforcer =
            QuotaEnforcer::new("tenant1".to_string(), 1000, SoftLimitThreshold::Percent(80));

        enforcer.add_usage(300);
        assert_eq!(enforcer.bytes_available(), 700);
    }

    #[test]
    fn test_quota_enforcer_add_usage_positive() {
        let mut enforcer =
            QuotaEnforcer::new("tenant1".to_string(), 1000, SoftLimitThreshold::Percent(80));

        enforcer.add_usage(100);
        assert_eq!(enforcer.current_usage, 100);
    }

    #[test]
    fn test_quota_enforcer_add_usage_negative() {
        let mut enforcer =
            QuotaEnforcer::new("tenant1".to_string(), 1000, SoftLimitThreshold::Percent(80));

        enforcer.add_usage(500);
        enforcer.add_usage(-200);
        assert_eq!(enforcer.current_usage, 300);
    }

    #[test]
    fn test_quota_enforcer_add_usage_saturates() {
        let mut enforcer =
            QuotaEnforcer::new("tenant1".to_string(), 1000, SoftLimitThreshold::Percent(80));

        enforcer.add_usage(1500);
        assert_eq!(enforcer.current_usage, 1500);
    }

    #[test]
    fn test_limit_check_result_is_ok() {
        assert!(LimitCheckResult::Ok.is_ok());
        assert!(!LimitCheckResult::Ok.is_soft_warning());
        assert!(!LimitCheckResult::Ok.is_hard_exceeded());
    }

    #[test]
    fn test_limit_check_result_soft_warning() {
        let result = LimitCheckResult::SoftLimitWarning { usage_pct: 85.0 };
        assert!(!result.is_ok());
        assert!(result.is_soft_warning());
        assert!(!result.is_hard_exceeded());
    }

    #[test]
    fn test_limit_check_result_hard_exceeded() {
        let result = LimitCheckResult::HardLimitExceeded { overage_bytes: 100 };
        assert!(!result.is_ok());
        assert!(!result.is_soft_warning());
        assert!(result.is_hard_exceeded());
    }

    #[test]
    fn test_resource_limiter_registry_new() {
        let registry = ResourceLimiterRegistry::new();
        assert_eq!(registry.tenant_count(), 0);
    }

    #[test]
    fn test_resource_limiter_registry_register() {
        let mut registry = ResourceLimiterRegistry::new();

        let result = registry.register("tenant1", 1000, SoftLimitThreshold::Percent(80));
        assert!(result.is_ok());
        assert_eq!(registry.tenant_count(), 1);
    }

    #[test]
    fn test_resource_limiter_registry_register_empty_tenant() {
        let mut registry = ResourceLimiterRegistry::new();

        let result = registry.register("", 1000, SoftLimitThreshold::Percent(80));
        assert!(matches!(
            result,
            Err(ResourceLimiterError::InvalidConfig(_))
        ));
    }

    #[test]
    fn test_resource_limiter_registry_register_percent_over_100() {
        let mut registry = ResourceLimiterRegistry::new();

        let result = registry.register("tenant1", 1000, SoftLimitThreshold::Percent(150));
        assert!(matches!(
            result,
            Err(ResourceLimiterError::InvalidConfig(_))
        ));
    }

    #[test]
    fn test_resource_limiter_registry_check_and_update_ok() {
        let mut registry = ResourceLimiterRegistry::new();
        registry
            .register("tenant1", 1000, SoftLimitThreshold::Percent(80))
            .unwrap();

        let result = registry.check_and_update("tenant1", 500);
        assert!(matches!(result, Ok(LimitCheckResult::Ok)));
    }

    #[test]
    fn test_resource_limiter_registry_check_and_update_soft_warning() {
        let mut registry = ResourceLimiterRegistry::new();
        registry
            .register("tenant1", 1000, SoftLimitThreshold::Percent(80))
            .unwrap();

        let result = registry.check_and_update("tenant1", 850);
        assert!(matches!(
            result,
            Ok(LimitCheckResult::SoftLimitWarning { .. })
        ));
    }

    #[test]
    fn test_resource_limiter_registry_check_and_update_hard_exceeded() {
        let mut registry = ResourceLimiterRegistry::new();
        registry
            .register("tenant1", 1000, SoftLimitThreshold::Percent(80))
            .unwrap();

        let result = registry.check_and_update("tenant1", 1500);
        assert!(matches!(
            result,
            Ok(LimitCheckResult::HardLimitExceeded { .. })
        ));
    }

    #[test]
    fn test_resource_limiter_registry_check_and_update_tenant_not_found() {
        let mut registry = ResourceLimiterRegistry::new();

        let result = registry.check_and_update("nonexistent", 100);
        assert!(matches!(
            result,
            Err(ResourceLimiterError::TenantNotFound(_))
        ));
    }

    #[test]
    fn test_resource_limiter_registry_get_enforcer() {
        let mut registry = ResourceLimiterRegistry::new();
        registry
            .register("tenant1", 1000, SoftLimitThreshold::Percent(80))
            .unwrap();

        let enforcer = registry.get_enforcer("tenant1");
        assert!(enforcer.is_some());
        assert_eq!(enforcer.unwrap().hard_limit, 1000);
    }

    #[test]
    fn test_resource_limiter_registry_get_enforcer_not_found() {
        let registry = ResourceLimiterRegistry::new();
        let enforcer = registry.get_enforcer("nonexistent");
        assert!(enforcer.is_none());
    }

    #[test]
    fn test_resource_limiter_registry_soft_limit_warnings() {
        let mut registry = ResourceLimiterRegistry::new();
        registry
            .register("tenant1", 1000, SoftLimitThreshold::Percent(80))
            .unwrap();
        registry
            .register("tenant2", 1000, SoftLimitThreshold::Percent(80))
            .unwrap();

        registry.check_and_update("tenant1", 850).unwrap();

        let warnings = registry.soft_limit_warnings();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].0, "tenant1");
    }

    #[test]
    fn test_resource_limiter_registry_over_hard_limit() {
        let mut registry = ResourceLimiterRegistry::new();
        registry
            .register("tenant1", 1000, SoftLimitThreshold::Percent(80))
            .unwrap();
        registry
            .register("tenant2", 1000, SoftLimitThreshold::Percent(80))
            .unwrap();

        registry.check_and_update("tenant1", 1500).unwrap();

        let over = registry.over_hard_limit();
        assert_eq!(over.len(), 1);
        assert_eq!(over[0].tenant_id, "tenant1");
    }

    #[test]
    fn test_resource_limiter_registry_tenant_ids() {
        let mut registry = ResourceLimiterRegistry::new();
        registry
            .register("tenant1", 1000, SoftLimitThreshold::Percent(80))
            .unwrap();
        registry
            .register("tenant2", 1000, SoftLimitThreshold::Percent(80))
            .unwrap();

        let ids = registry.tenant_ids();
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn test_resource_limiter_registry_unregister() {
        let mut registry = ResourceLimiterRegistry::new();
        registry
            .register("tenant1", 1000, SoftLimitThreshold::Percent(80))
            .unwrap();

        let removed = registry.unregister("tenant1");
        assert!(removed.is_some());
        assert_eq!(registry.tenant_count(), 0);
    }

    #[test]
    fn test_resource_limiter_registry_clear() {
        let mut registry = ResourceLimiterRegistry::new();
        registry
            .register("tenant1", 1000, SoftLimitThreshold::Percent(80))
            .unwrap();
        registry
            .register("tenant2", 1000, SoftLimitThreshold::Percent(80))
            .unwrap();

        registry.clear();

        assert_eq!(registry.tenant_count(), 0);
    }

    #[test]
    fn test_resource_limiter_registry_multiple_tenants_different_limits() {
        let mut registry = ResourceLimiterRegistry::new();

        registry
            .register("tenant1", 1000, SoftLimitThreshold::Percent(80))
            .unwrap();
        registry
            .register("tenant2", 5000, SoftLimitThreshold::Percent(90))
            .unwrap();

        let result1 = registry.check_and_update("tenant1", 900);
        assert!(matches!(
            result1,
            Ok(LimitCheckResult::SoftLimitWarning { .. })
        ));

        let result2 = registry.check_and_update("tenant2", 900);
        assert!(matches!(result2, Ok(LimitCheckResult::Ok)));
    }

    #[test]
    fn test_quota_enforcer_zero_limit() {
        let enforcer = QuotaEnforcer::new("tenant1".to_string(), 0, SoftLimitThreshold::Percent(0));

        assert_eq!(enforcer.usage_percent(), 0.0);
        assert!(!enforcer.exceeded());
    }

    #[test]
    fn test_resource_limiter_negative_delta() {
        let mut registry = ResourceLimiterRegistry::new();
        registry
            .register("tenant1", 1000, SoftLimitThreshold::Percent(80))
            .unwrap();

        registry.check_and_update("tenant1", 500).unwrap();
        let result = registry.check_and_update("tenant1", -200);

        assert!(matches!(result, Ok(LimitCheckResult::Ok)));
    }

    #[test]
    fn test_get_enforcer_mut() {
        let mut registry = ResourceLimiterRegistry::new();
        registry
            .register("tenant1", 1000, SoftLimitThreshold::Percent(80))
            .unwrap();

        let enforcer = registry.get_enforcer_mut("tenant1");
        assert!(enforcer.is_some());

        enforcer.unwrap().add_usage(100);
        drop(enforcer);

        let enforcer = registry.get_enforcer("tenant1");
        assert_eq!(enforcer.unwrap().current_usage, 100);
    }

    #[test]
    fn test_soft_limit_bytes_type() {
        let mut registry = ResourceLimiterRegistry::new();
        registry
            .register("tenant1", 1000, SoftLimitThreshold::Bytes(800))
            .unwrap();

        let result = registry.check_and_update("tenant1", 850);
        assert!(matches!(
            result,
            Ok(LimitCheckResult::SoftLimitWarning { .. })
        ));
    }

    #[test]
    fn test_usage_percent_with_zero_hard_limit() {
        let mut enforcer =
            QuotaEnforcer::new("tenant1".to_string(), 0, SoftLimitThreshold::Percent(0));

        enforcer.add_usage(100);
        assert_eq!(enforcer.usage_percent(), 0.0);
    }
}
