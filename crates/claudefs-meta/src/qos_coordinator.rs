//! QoS coordination between A2 Metadata and A4 Transport services.
//!
//! This module coordinates QoS enforcement between metadata operations
//! and network transport, implementing SLA tracking, priority estimation,
//! and backpressure management.

use std::collections::VecDeque;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use crate::tenant::TenantId;
use crate::types::Timestamp;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum Priority {
    Critical,
    Interactive,
    Bulk,
}

impl Priority {
    pub fn sla_target_ms(&self) -> u64 {
        match self {
            Priority::Critical => 10,
            Priority::Interactive => 50,
            Priority::Bulk => 500,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Priority::Critical => "critical",
            Priority::Interactive => "interactive",
            Priority::Bulk => "bulk",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpType {
    Read,
    Write,
    Metadata,
    Delete,
}

impl OpType {
    pub fn is_data_intensive(&self) -> bool {
        matches!(self, OpType::Write)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RequestId(String);

impl RequestId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for RequestId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QosRequest {
    pub request_id: RequestId,
    pub operation_type: OpType,
    pub tenant_id: TenantId,
    pub priority: Priority,
    pub estimated_duration_ms: u64,
    pub estimated_bytes: u64,
    pub deadline_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QosContext {
    pub request_id: RequestId,
    pub priority: Priority,
    pub tenant_id: TenantId,
    pub started_at: Timestamp,
    pub deadline: Option<Timestamp>,
    pub sla_target_p99_ms: u64,
}

impl QosContext {
    pub fn from_request(request: QosRequest) -> Self {
        let deadline = if request.deadline_ms > 0 {
            let now = Timestamp::now();
            Some(Timestamp {
                secs: now.secs + request.deadline_ms / 1000,
                nanos: now.nanos,
            })
        } else {
            None
        };

        Self {
            request_id: request.request_id,
            priority: request.priority,
            tenant_id: request.tenant_id,
            started_at: Timestamp::now(),
            deadline,
            sla_target_p99_ms: request.priority.sla_target_ms(),
        }
    }

    pub fn is_deadline_missed(&self) -> bool {
        if let Some(deadline) = self.deadline {
            Timestamp::now() > deadline
        } else {
            false
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QosMetrics {
    pub request_id: RequestId,
    pub operation_type: OpType,
    pub priority: Priority,
    pub latency_ms: u64,
    pub sla_target_ms: u64,
    pub sla_met: bool,
    pub tenant_id: TenantId,
    pub bytes_processed: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QosViolation {
    pub request_id: RequestId,
    pub priority: Priority,
    pub tenant_id: TenantId,
    pub sla_target_ms: u64,
    pub actual_latency_ms: u64,
    pub violation_severity: f64,
    pub timestamp: Timestamp,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QosHint {
    pub priority: Priority,
    pub deadline_us: u64,
    pub max_latency_us: u64,
}

impl QosHint {
    pub fn from_context(context: &QosContext) -> Self {
        let deadline_us = context
            .deadline
            .map(|d| {
                let now = Timestamp::now();
                if d > now {
                    (d.secs - now.secs) * 1_000_000 + ((d.nanos - now.nanos) / 1000) as u64
                } else {
                    0
                }
            })
            .unwrap_or(0);

        Self {
            priority: context.priority,
            deadline_us,
            max_latency_us: context.sla_target_p99_ms * 1000,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QosMetricsSummary {
    pub total_requests: u64,
    pub sla_attainment_pct: f64,
    pub critical_p50_latency_ms: u64,
    pub critical_p99_latency_ms: u64,
    pub interactive_p50_latency_ms: u64,
    pub interactive_p99_latency_ms: u64,
    pub bulk_p50_latency_ms: u64,
    pub bulk_p99_latency_ms: u64,
}

pub struct QosCoordinatorConfig {
    pub max_queue_depth: usize,
    pub sla_attainment_target_pct: f64,
    pub critical_iops_weight: f64,
    pub interactive_iops_weight: f64,
    pub bulk_iops_weight: f64,
}

impl Default for QosCoordinatorConfig {
    fn default() -> Self {
        Self {
            max_queue_depth: 1000,
            sla_attainment_target_pct: 95.0,
            critical_iops_weight: 1.0,
            interactive_iops_weight: 0.7,
            bulk_iops_weight: 0.3,
        }
    }
}

pub struct QosCoordinator {
    requests: DashMap<RequestId, QosContext>,
    queues: DashMap<Priority, VecDeque<RequestId>>,
    metrics_history: std::sync::RwLock<Vec<QosMetrics>>,
    violations_history: std::sync::RwLock<Vec<QosViolation>>,
    tenant_tier: DashMap<TenantId, u8>,
    config: QosCoordinatorConfig,
}

impl QosCoordinator {
    pub fn new(config: QosCoordinatorConfig) -> Self {
        let queues = DashMap::new();
        queues.insert(Priority::Critical, VecDeque::new());
        queues.insert(Priority::Interactive, VecDeque::new());
        queues.insert(Priority::Bulk, VecDeque::new());

        Self {
            requests: DashMap::new(),
            queues,
            metrics_history: std::sync::RwLock::new(Vec::with_capacity(100000)),
            violations_history: std::sync::RwLock::new(Vec::with_capacity(10000)),
            tenant_tier: DashMap::new(),
            config,
        }
    }

    pub fn set_tenant_tier(&self, tenant_id: TenantId, tier: u8) {
        self.tenant_tier.insert(tenant_id, tier);
    }

    pub fn create_context(&self, request: QosRequest) -> QosContext {
        let context = QosContext::from_request(request.clone());

        if let Some(mut queue) = self.queues.get_mut(&context.priority) {
            if queue.len() < self.config.max_queue_depth {
                queue.push_back(request.request_id.clone());
            }
        }

        self.requests
            .insert(context.request_id.clone(), context.clone());

        context
    }

    pub fn estimate_priority(&self, tenant_id: &TenantId, op_type: &OpType) -> Priority {
        let tier = self.tenant_tier.get(tenant_id).map(|t| *t).unwrap_or(1);

        if tier >= 3 {
            if !op_type.is_data_intensive() {
                return Priority::Critical;
            }
            return Priority::Interactive;
        }

        if tier >= 2 {
            return Priority::Interactive;
        }

        if matches!(op_type, OpType::Metadata) {
            return Priority::Interactive;
        }

        if op_type.is_data_intensive() {
            return Priority::Bulk;
        }

        Priority::Bulk
    }

    pub fn should_reject_operation(&self, context: &QosContext) -> bool {
        if context.is_deadline_missed() {
            return true;
        }

        if let Some(queue) = self.queues.get(&context.priority) {
            if queue.len() >= self.config.max_queue_depth {
                if matches!(context.priority, Priority::Bulk) {
                    return true;
                }
            }
        }

        if let Some(deadline) = context.deadline {
            let now = Timestamp::now();
            let remaining_ms = if deadline.secs > now.secs {
                (deadline.secs - now.secs) * 1000
            } else {
                0
            };

            if remaining_ms < context.sla_target_p99_ms {
                return true;
            }
        }

        false
    }

    pub fn emit_qos_hint(&self, context: &QosContext) -> QosHint {
        QosHint::from_context(context)
    }

    pub fn record_completion(
        &self,
        request_id: RequestId,
        actual_latency_ms: u64,
        bytes_processed: u64,
    ) -> Option<QosMetrics> {
        let context = self.requests.remove(&request_id)?.1;

        let sla_met = actual_latency_ms <= context.sla_target_p99_ms;

        let metrics = QosMetrics {
            request_id: context.request_id.clone(),
            operation_type: OpType::Metadata,
            priority: context.priority,
            latency_ms: actual_latency_ms,
            sla_target_ms: context.sla_target_p99_ms,
            sla_met,
            tenant_id: context.tenant_id.clone(),
            bytes_processed,
        };

        if !sla_met {
            let violation = QosViolation {
                request_id: context.request_id.clone(),
                priority: context.priority,
                tenant_id: context.tenant_id.clone(),
                sla_target_ms: context.sla_target_p99_ms,
                actual_latency_ms,
                violation_severity: ((actual_latency_ms as f64 - context.sla_target_p99_ms as f64)
                    / context.sla_target_p99_ms as f64)
                    * 100.0,
                timestamp: Timestamp::now(),
            };

            let mut violations = self.violations_history.write().unwrap();
            if violations.len() >= 10000 {
                violations.remove(0);
            }
            violations.push(violation);
        }

        let mut metrics_history = self.metrics_history.write().unwrap();
        if metrics_history.len() >= 100000 {
            metrics_history.remove(0);
        }
        metrics_history.push(metrics.clone());

        if let Some(mut queue) = self.queues.get_mut(&context.priority) {
            queue.retain(|id| *id != request_id);
        }

        Some(metrics)
    }

    pub fn get_violations(&self, tenant_id: &TenantId) -> Vec<QosViolation> {
        let violations = self.violations_history.read().unwrap();
        violations
            .iter()
            .filter(|v| v.tenant_id == *tenant_id)
            .cloned()
            .collect()
    }

    pub fn get_metrics_summary(&self) -> QosMetricsSummary {
        let metrics_history = self.metrics_history.read().unwrap();

        let total_requests = metrics_history.len() as u64;
        let sla_met_count = metrics_history.iter().filter(|m| m.sla_met).count() as f64;
        let sla_attainment_pct = if total_requests > 0 {
            (sla_met_count / total_requests as f64) * 100.0
        } else {
            100.0
        };

        let critical_latencies: Vec<u64> = metrics_history
            .iter()
            .filter(|m| m.priority == Priority::Critical && m.sla_met)
            .map(|m| m.latency_ms)
            .collect();

        let interactive_latencies: Vec<u64> = metrics_history
            .iter()
            .filter(|m| m.priority == Priority::Interactive && m.sla_met)
            .map(|m| m.latency_ms)
            .collect();

        let bulk_latencies: Vec<u64> = metrics_history
            .iter()
            .filter(|m| m.priority == Priority::Bulk && m.sla_met)
            .map(|m| m.latency_ms)
            .collect();

        fn calc_p50(latencies: &[u64]) -> u64 {
            if latencies.is_empty() {
                return 0;
            }
            let mut sorted = latencies.to_vec();
            sorted.sort();
            sorted[sorted.len() / 2]
        }

        fn calc_p99(latencies: &[u64]) -> u64 {
            if latencies.is_empty() {
                return 0;
            }
            let mut sorted = latencies.to_vec();
            sorted.sort();
            let idx = (sorted.len() as f64 * 0.99) as usize;
            sorted[idx.min(sorted.len() - 1)]
        }

        QosMetricsSummary {
            total_requests,
            sla_attainment_pct,
            critical_p50_latency_ms: calc_p50(&critical_latencies),
            critical_p99_latency_ms: calc_p99(&critical_latencies),
            interactive_p50_latency_ms: calc_p50(&interactive_latencies),
            interactive_p99_latency_ms: calc_p99(&interactive_latencies),
            bulk_p50_latency_ms: calc_p50(&bulk_latencies),
            bulk_p99_latency_ms: calc_p99(&bulk_latencies),
        }
    }

    pub fn queue_depth(&self, priority: Priority) -> usize {
        self.queues.get(&priority).map(|q| q.len()).unwrap_or(0)
    }

    pub fn total_queue_depth(&self) -> usize {
        self.queues.iter().map(|q| q.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_coordinator() -> QosCoordinator {
        QosCoordinator::new(QosCoordinatorConfig::default())
    }

    #[test]
    fn test_create_qos_context() {
        let coordinator = make_coordinator();
        let tenant = TenantId::new("tenant1");

        let request = QosRequest {
            request_id: RequestId::new(),
            operation_type: OpType::Read,
            tenant_id: tenant.clone(),
            priority: Priority::Interactive,
            estimated_duration_ms: 10,
            estimated_bytes: 4096,
            deadline_ms: 100,
        };

        let context = coordinator.create_context(request);
        assert_eq!(context.priority, Priority::Interactive);
        assert!(context.deadline.is_some());
    }

    #[test]
    fn test_create_qos_context_without_deadline() {
        let coordinator = make_coordinator();
        let tenant = TenantId::new("tenant1");

        let request = QosRequest {
            request_id: RequestId::new(),
            operation_type: OpType::Read,
            tenant_id: tenant.clone(),
            priority: Priority::Bulk,
            estimated_duration_ms: 10,
            estimated_bytes: 4096,
            deadline_ms: 0,
        };

        let context = coordinator.create_context(request);
        assert_eq!(context.priority, Priority::Bulk);
        assert!(context.deadline.is_none());
    }

    #[test]
    fn test_estimate_priority_high_tier() {
        let coordinator = make_coordinator();
        let tenant = TenantId::new("tenant1");

        coordinator.set_tenant_tier(tenant.clone(), 5);

        let priority = coordinator.estimate_priority(&tenant, &OpType::Read);
        assert_eq!(priority, Priority::Critical);

        let priority2 = coordinator.estimate_priority(&tenant, &OpType::Write);
        assert_eq!(priority2, Priority::Interactive);
    }

    #[test]
    fn test_estimate_priority_medium_tier() {
        let coordinator = make_coordinator();
        let tenant = TenantId::new("tenant1");

        coordinator.set_tenant_tier(tenant.clone(), 2);

        let priority = coordinator.estimate_priority(&tenant, &OpType::Read);
        assert_eq!(priority, Priority::Interactive);
    }

    #[test]
    fn test_estimate_priority_low_tier_metadata() {
        let coordinator = make_coordinator();
        let tenant = TenantId::new("tenant1");

        coordinator.set_tenant_tier(tenant.clone(), 1);

        let priority = coordinator.estimate_priority(&tenant, &OpType::Metadata);
        assert_eq!(priority, Priority::Interactive);
    }

    #[test]
    fn test_estimate_priority_low_tier_bulk() {
        let coordinator = make_coordinator();
        let tenant = TenantId::new("tenant1");

        coordinator.set_tenant_tier(tenant.clone(), 1);

        let priority = coordinator.estimate_priority(&tenant, &OpType::Write);
        assert_eq!(priority, Priority::Bulk);
    }

    #[test]
    fn test_reject_operation_deadline_missed() {
        let coordinator = make_coordinator();

        let context = QosContext {
            request_id: RequestId::new(),
            priority: Priority::Interactive,
            tenant_id: TenantId::new("tenant1"),
            started_at: Timestamp::now(),
            deadline: Some(Timestamp { secs: 0, nanos: 0 }),
            sla_target_p99_ms: 50,
        };

        assert!(coordinator.should_reject_operation(&context));
    }

    #[test]
    fn test_reject_operation_queue_full_bulk() {
        let coordinator = make_coordinator();

        let context = QosContext {
            request_id: RequestId::new(),
            priority: Priority::Bulk,
            tenant_id: TenantId::new("tenant1"),
            started_at: Timestamp::now(),
            deadline: None,
            sla_target_p99_ms: 500,
        };

        let result = coordinator.should_reject_operation(&context);
        assert!(!result);
    }

    #[test]
    fn test_emit_qos_hint() {
        let coordinator = make_coordinator();
        let tenant = TenantId::new("tenant1");

        let request = QosRequest {
            request_id: RequestId::new(),
            operation_type: OpType::Write,
            tenant_id: tenant.clone(),
            priority: Priority::Critical,
            estimated_duration_ms: 10,
            estimated_bytes: 4096,
            deadline_ms: 100,
        };

        let context = coordinator.create_context(request);
        let hint = coordinator.emit_qos_hint(&context);

        assert_eq!(hint.priority, Priority::Critical);
        assert!(hint.max_latency_us > 0);
    }

    #[test]
    fn test_record_completion_sla_met() {
        let coordinator = make_coordinator();
        let tenant = TenantId::new("tenant1");

        let request = QosRequest {
            request_id: RequestId::new(),
            operation_type: OpType::Read,
            tenant_id: tenant.clone(),
            priority: Priority::Interactive,
            estimated_duration_ms: 10,
            estimated_bytes: 4096,
            deadline_ms: 100,
        };

        let _ = coordinator.create_context(request.clone());

        let metrics = coordinator.record_completion(request.request_id.clone(), 30, 4096);

        assert!(metrics.is_some());
        assert!(metrics.unwrap().sla_met);
    }

    #[test]
    fn test_record_completion_sla_missed() {
        let coordinator = make_coordinator();
        let tenant = TenantId::new("tenant1");

        let request = QosRequest {
            request_id: RequestId::new(),
            operation_type: OpType::Read,
            tenant_id: tenant.clone(),
            priority: Priority::Critical,
            estimated_duration_ms: 10,
            estimated_bytes: 4096,
            deadline_ms: 100,
        };

        let _ = coordinator.create_context(request.clone());

        let metrics = coordinator.record_completion(request.request_id.clone(), 20, 4096);

        assert!(metrics.is_some());
        assert!(!metrics.unwrap().sla_met);
    }

    #[test]
    fn test_get_violations() {
        let coordinator = make_coordinator();
        let tenant = TenantId::new("tenant1");

        let request = QosRequest {
            request_id: RequestId::new(),
            operation_type: OpType::Read,
            tenant_id: tenant.clone(),
            priority: Priority::Critical,
            estimated_duration_ms: 10,
            estimated_bytes: 4096,
            deadline_ms: 100,
        };

        let _ = coordinator.create_context(request.clone());

        let _ = coordinator.record_completion(request.request_id.clone(), 20, 4096);

        let violations = coordinator.get_violations(&tenant);
        assert!(!violations.is_empty());
    }

    #[test]
    fn test_metrics_summary() {
        let coordinator = make_coordinator();
        let tenant = TenantId::new("tenant1");

        for i in 0..5 {
            let request = QosRequest {
                request_id: RequestId::new(),
                operation_type: OpType::Read,
                tenant_id: tenant.clone(),
                priority: Priority::Critical,
                estimated_duration_ms: 10,
                estimated_bytes: 4096,
                deadline_ms: 100,
            };

            let _ = coordinator.create_context(request.clone());
            let _ = coordinator.record_completion(request.request_id, 5 + i, 4096);
        }

        let summary = coordinator.get_metrics_summary();
        assert_eq!(summary.total_requests, 5);
        assert!(summary.sla_attainment_pct > 0.0);
    }

    #[test]
    fn test_multiple_priorities_concurrent() {
        let coordinator = make_coordinator();

        let req1 = QosRequest {
            request_id: RequestId::new(),
            operation_type: OpType::Read,
            tenant_id: TenantId::new("tenant1"),
            priority: Priority::Critical,
            estimated_duration_ms: 10,
            estimated_bytes: 4096,
            deadline_ms: 100,
        };

        let req2 = QosRequest {
            request_id: RequestId::new(),
            operation_type: OpType::Write,
            tenant_id: TenantId::new("tenant2"),
            priority: Priority::Bulk,
            estimated_duration_ms: 10,
            estimated_bytes: 4096,
            deadline_ms: 1000,
        };

        let _ = coordinator.create_context(req1);
        let _ = coordinator.create_context(req2);

        assert!(coordinator.queue_depth(Priority::Critical) > 0);
        assert!(coordinator.queue_depth(Priority::Bulk) > 0);
    }

    #[test]
    fn test_priority_sla_targets() {
        assert_eq!(Priority::Critical.sla_target_ms(), 10);
        assert_eq!(Priority::Interactive.sla_target_ms(), 50);
        assert_eq!(Priority::Bulk.sla_target_ms(), 500);
    }

    #[test]
    fn test_qos_hint_from_context() {
        let context = QosContext {
            request_id: RequestId::new(),
            priority: Priority::Interactive,
            tenant_id: TenantId::new("tenant1"),
            started_at: Timestamp::now(),
            deadline: None,
            sla_target_p99_ms: 50,
        };

        let hint = QosHint::from_context(&context);
        assert_eq!(hint.priority, Priority::Interactive);
        assert_eq!(hint.max_latency_us, 50000);
    }

    #[test]
    fn test_op_type_is_data_intensive() {
        assert!(OpType::Write.is_data_intensive());
        assert!(!OpType::Read.is_data_intensive());
        assert!(!OpType::Metadata.is_data_intensive());
        assert!(!OpType::Delete.is_data_intensive());
    }

    #[test]
    fn test_total_queue_depth() {
        let coordinator = make_coordinator();

        assert_eq!(coordinator.total_queue_depth(), 0);

        let req1 = QosRequest {
            request_id: RequestId::new(),
            operation_type: OpType::Read,
            tenant_id: TenantId::new("tenant1"),
            priority: Priority::Critical,
            estimated_duration_ms: 10,
            estimated_bytes: 4096,
            deadline_ms: 100,
        };

        let _ = coordinator.create_context(req1);

        assert_eq!(coordinator.total_queue_depth(), 1);
    }
}
