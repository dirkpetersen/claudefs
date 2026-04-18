# A11: Phase 5 Block 2 — Preemptible Instance Lifecycle Tests

**Date:** 2026-04-18
**Agent:** A11 Infrastructure & CI
**Status:** 🟡 PLANNING — Ready for OpenCode
**Test Target:** 15 tests covering spot pricing, instance lifecycle, disruption handling, cost tracking

---

## Test Module: preemptible_lifecycle_tests.rs

Location: `crates/claudefs-tests/src/preemptible_lifecycle_tests.rs`
Target LOC: 500-600 lines
Target Tests: 15 (all passing, zero clippy warnings)

---

## Test Groups

### Group 1: Spot Pricing (4 tests, ~100 LOC)

#### 1.1 test_spot_pricing_query_valid

**Purpose:** Verify AWS Spot Price History API parsing

**Setup:**
```rust
let mock_prices = vec![
    ("i4i.2xlarge", "0.190000"),
    ("c7a.xlarge",  "0.050000"),
    ("t3.medium",   "0.015000"),
];
```

**Test:**
- Mock AWS API response with instance types, prices, timestamps
- Call `cfs_spot_pricing::query_prices(&mock_aws_client)`
- Parse JSON response

**Assertions:**
```rust
assert_eq!(prices.len(), 3);
assert!(prices.iter().any(|p| p.instance_type == "i4i.2xlarge" && p.price == 0.190));
assert!(prices.iter().any(|p| p.instance_type == "c7a.xlarge" && p.price == 0.050));
```

**Expected:** ✅ PASS (parsing correct)

---

#### 1.2 test_spot_pricing_history_trend

**Purpose:** Calculate 7-day price trend

**Setup:**
```rust
let history = vec![
    // Day 1: high price
    ("i4i.2xlarge", "0.210000", "2026-04-11T00:00:00Z"),
    // Day 2: dropping
    ("i4i.2xlarge", "0.195000", "2026-04-12T00:00:00Z"),
    ("i4i.2xlarge", "0.190000", "2026-04-13T00:00:00Z"),
    ("i4i.2xlarge", "0.185000", "2026-04-14T00:00:00Z"),
    ("i4i.2xlarge", "0.180000", "2026-04-15T00:00:00Z"),
    ("i4i.2xlarge", "0.175000", "2026-04-16T00:00:00Z"),
    ("i4i.2xlarge", "0.170000", "2026-04-17T00:00:00Z"),
    ("i4i.2xlarge", "0.190000", "2026-04-18T00:00:00Z"),
];
```

**Test:**
- Load 7-day history
- Calculate trend: linear regression or simple slope
- Detect: upward (slope > 0.005), downward (slope < -0.005), stable

**Assertions:**
```rust
let trend = calculate_trend(&history[0..7]);
assert_eq!(trend, Trend::Downward);
let trend = calculate_trend(&history);
assert_eq!(trend, Trend::Volatile);
```

**Expected:** ✅ PASS (trend detection accurate)

---

#### 1.3 test_breakeven_calculation

**Purpose:** Spot vs on-demand cost comparison

**Setup:**
```rust
let spot_price = 0.190;      // AWS spot rate
let on_demand_price = 0.624; // AWS on-demand rate
```

**Test:**
- Calculate discount: `(on_demand - spot) / on_demand * 100`
- Calculate savings per hour, per month
- Compare to expected values

**Assertions:**
```rust
let discount_pct = calculate_discount(&spot_price, &on_demand_price);
assert!((discount_pct - 69.6).abs() < 1.0); // ±1%
let monthly_savings = calculate_monthly_savings(&spot_price, &on_demand_price);
assert!((monthly_savings - 317.0).abs() < 5.0); // ±$5
```

**Expected:** ✅ PASS (math correct)

---

#### 1.4 test_should_launch_decision_logic

**Purpose:** Determine when to launch spot instances

**Setup:**
```rust
struct LaunchDecision {
    spot_price: f64,
    on_demand_price: f64,
    interruption_rate: f64,  // percentage
}
```

**Test Cases:**
- Case A: spot=0.19, on-demand=0.624, interruption=2% → should_launch() = true
- Case B: spot=0.50, on-demand=0.624, interruption=2% → should_launch() = true (discount <50% but still <80%)
- Case C: spot=0.60, on-demand=0.624, interruption=15% → should_launch() = false
- Case D: spot=0.19, on-demand=0.624, interruption=12% → should_launch() = false (rate too high)

**Assertions:**
```rust
assert_eq!(should_launch(0.19, 0.624, 0.02), true);
assert_eq!(should_launch(0.50, 0.624, 0.02), true);
assert_eq!(should_launch(0.60, 0.624, 0.15), false);
assert_eq!(should_launch(0.19, 0.624, 0.12), false);
```

**Expected:** ✅ PASS (logic correct)

---

### Group 2: Instance Lifecycle (4 tests, ~150 LOC)

#### 2.1 test_provision_instance_success

**Purpose:** Provision instance via Terraform with correct tags

**Setup:**
```rust
struct MockTerraform {
    calls: Vec<String>,
    should_fail: bool,
}

impl MockTerraform {
    fn apply(&mut self, vars: &InstanceVars) -> Result<InstanceId> { ... }
}

let mut tf = MockTerraform::new();
let vars = InstanceVars {
    instance_type: "i4i.2xlarge",
    role: "storage",
    site: "a",
    name: "storage-site-a-node-1",
};
```

**Test:**
- Call `provision_instance(&mut tf, &vars)`
- Verify terraform apply invoked
- Check tags applied: Name, Role, Site, CostCenter, Agent, StartTime

**Assertions:**
```rust
let result = provision_instance(&mut tf, &vars);
assert!(result.is_ok());
let instance_id = result.unwrap();
assert!(instance_id.starts_with("i-"));
assert!(tf.calls.iter().any(|c| c.contains("terraform apply")));
```

**Expected:** ✅ PASS (provisioning succeeds)

---

#### 2.2 test_provision_instance_with_retries

**Purpose:** Retry on transient Terraform failures

**Setup:**
```rust
struct RetryableTerraform {
    call_count: usize,
    fail_until: usize,  // fail first N calls, then succeed
}

impl RetryableTerraform {
    fn apply(&mut self, _vars: &InstanceVars) -> Result<InstanceId> {
        self.call_count += 1;
        if self.call_count <= self.fail_until {
            Err("temporary failure")
        } else {
            Ok("i-provisioned".to_string())
        }
    }
}

let mut tf = RetryableTerraform {
    call_count: 0,
    fail_until: 2,  // fail 1st 2 calls, succeed on 3rd
};
```

**Test:**
- Configure terraform to fail 2 times
- Call `provision_instance_with_retry(&mut tf, &vars, 3, Duration::from_millis(10))`
- Verify exponential backoff: 10ms, 20ms, 40ms between retries

**Assertions:**
```rust
let result = provision_instance_with_retry(&mut tf, &vars, 3, Duration::from_millis(10));
assert!(result.is_ok());
assert_eq!(tf.call_count, 3); // took 3 attempts
```

**Expected:** ✅ PASS (retries succeed)

---

#### 2.3 test_drain_instance_graceful

**Purpose:** Graceful shutdown with pending operations

**Setup:**
```rust
struct MockInstance {
    pending_operations: usize,
    operation_completion_time_ms: u64,
}

impl MockInstance {
    fn drain(&mut self) -> Result<()> {
        // Simulate operations completing
        while self.pending_operations > 0 {
            // Operations complete within operation_completion_time_ms
            self.pending_operations -= 1;
        }
        Ok(())
    }
}

let mut instance = MockInstance {
    pending_operations: 10,
    operation_completion_time_ms: 50,
};
```

**Test:**
- Create instance with 10 pending operations
- Operations complete in 50ms each (total 500ms, well under 2-min window)
- Call `drain_instance(&mut instance, Duration::from_secs(120))`
- Verify all operations complete and status broadcasted

**Assertions:**
```rust
let result = drain_instance(&mut instance, Duration::from_secs(120));
assert!(result.is_ok());
assert_eq!(instance.pending_operations, 0);
```

**Expected:** ✅ PASS (graceful drain succeeds)

---

#### 2.4 test_drain_instance_timeout

**Purpose:** Drain timeout when operations take too long

**Setup:**
```rust
struct SlowInstance {
    pending_operations: usize,
    operation_completion_time_ms: u64,  // 150ms per op
}

let mut instance = SlowInstance {
    pending_operations: 10,
    operation_completion_time_ms: 150,  // Total: 1,500ms = 1.5 seconds
};
```

**Test:**
- Create instance with 10 slow operations (150ms each = 1.5s total)
- Set drain timeout to 0.5 seconds
- Call `drain_instance(&mut instance, Duration::from_millis(500))`
- Verify timeout error after 500ms, forced shutdown

**Assertions:**
```rust
let result = drain_instance(&mut instance, Duration::from_millis(500));
assert!(result.is_err());
match result {
    Err(DrainError::Timeout { .. }) => (),
    _ => panic!("expected timeout error"),
}
```

**Expected:** ✅ PASS (timeout triggers, forced shutdown occurs)

---

### Group 3: Disruption Handling (4 tests, ~150 LOC)

#### 3.1 test_spot_termination_notice_detected

**Purpose:** IMDS polling detects termination notice

**Setup:**
```rust
struct MockIMDS {
    call_count: usize,
    termination_notice_after: usize,  // notice on call N
}

impl MockIMDS {
    fn fetch_termination_notice(&mut self) -> Option<TerminationNotice> {
        self.call_count += 1;
        if self.call_count >= self.termination_notice_after {
            Some(TerminationNotice {
                time: Instant::now() + Duration::from_secs(120),
            })
        } else {
            None
        }
    }
}

let mut imds = MockIMDS {
    call_count: 0,
    termination_notice_after: 2,  // notice on 2nd call
};
```

**Test:**
- Poll IMDS every 10ms (simulating 5s real interval)
- Termination notice appears on 2nd call
- Verify detection latency < 20ms

**Assertions:**
```rust
let start = Instant::now();
let notice = poll_imds(&mut imds, Duration::from_millis(10));
let elapsed = start.elapsed();
assert!(notice.is_some());
assert!(elapsed < Duration::from_millis(20));
```

**Expected:** ✅ PASS (notice detected quickly)

---

#### 3.2 test_disruption_triggers_drain

**Purpose:** Termination notice → drain initiated

**Setup:**
```rust
struct DisruptionScenario {
    imds: MockIMDS,
    instance: MockInstance,
    drain_called: bool,
}
```

**Test:**
- Simulate spot termination notice
- Verify drain is initiated within 1 second
- Check status changes to "draining"

**Assertions:**
```rust
let mut scenario = DisruptionScenario::new();
let drain_initiated = scenario.handle_disruption();
assert!(drain_initiated);
assert_eq!(scenario.instance.status, InstanceStatus::Draining);
```

**Expected:** ✅ PASS (drain initiated on disruption)

---

#### 3.3 test_replacement_launch_after_disruption

**Purpose:** Replacement instance launches after disruption

**Setup:**
```rust
struct ReplacementScenario {
    old_instance_id: String,
    new_instance_id: Option<String>,
    replacement_tags: HashMap<String, String>,
}
```

**Test:**
- Simulate disruption of instance i-old-123
- Verify replacement launch initiated at T+120s
- Check ReplacementOf tag = i-old-123
- Verify new instance reaches "ready" status within 3min

**Assertions:**
```rust
let mut scenario = ReplacementScenario::new("i-old-123");
scenario.simulate_disruption();
assert_eq!(scenario.new_instance_id, Some("i-new-456".to_string()));
assert_eq!(
    scenario.replacement_tags.get("ReplacementOf").unwrap(),
    "i-old-123"
);
```

**Expected:** ✅ PASS (replacement launched correctly)

---

#### 3.4 test_concurrent_disruptions

**Purpose:** Handle 3+ simultaneous spot interruptions

**Setup:**
```rust
struct ConcurrentDisruptionScenario {
    instances: Vec<MockInstance>,
    disruptions: Vec<(usize, Instant)>,  // (instance index, disruption time)
}

let mut scenario = ConcurrentDisruptionScenario::new();
scenario.instances.push(MockInstance::new("i-1"));
scenario.instances.push(MockInstance::new("i-2"));
scenario.instances.push(MockInstance::new("i-3"));
scenario.disruptions = vec![
    (0, Instant::now()),
    (1, Instant::now() + Duration::from_millis(10)),
    (2, Instant::now() + Duration::from_millis(20)),
];
```

**Test:**
- Trigger 3 disruptions nearly simultaneously
- Verify all 3 drains initiated without deadlock
- Check 3 replacements launched
- Verify all complete within 5 minutes

**Assertions:**
```rust
let result = scenario.handle_all_disruptions();
assert!(result.is_ok());
assert_eq!(scenario.drain_count, 3);
assert_eq!(scenario.replacement_count, 3);
for instance in &scenario.instances {
    assert_eq!(instance.status, InstanceStatus::Ready);
}
```

**Expected:** ✅ PASS (concurrent disruptions handled correctly)

---

### Group 4: Cost Tracking (3 tests, ~100 LOC)

#### 4.1 test_instance_cost_calculation

**Purpose:** Accurate per-instance cost calculation

**Setup:**
```rust
struct CostCalculation {
    instance_type: String,
    pricing_model: String,  // "spot" or "on-demand"
    hourly_rate: f64,
    uptime_hours: f64,
}

let calc = CostCalculation {
    instance_type: "i4i.2xlarge".to_string(),
    pricing_model: "spot".to_string(),
    hourly_rate: 0.19,
    uptime_hours: 2.0,
};
```

**Test:**
- Instance: i4i.2xlarge, spot, 2 hours uptime
- Rate: $0.19/hr
- Expected cost: $0.38
- Calculate and verify

**Assertions:**
```rust
let cost = calc.total_cost();
assert!((cost - 0.38).abs() < 0.01); // ±$0.01
```

**Expected:** ✅ PASS (cost calculation accurate)

---

#### 4.2 test_replacement_cost_included

**Purpose:** Chain multiple instance replacements with cumulative cost

**Setup:**
```rust
struct ReplacementChain {
    instances: Vec<InstanceCost>,
}

let chain = ReplacementChain {
    instances: vec![
        InstanceCost {
            id: "i-old-1".to_string(),
            uptime_hours: 8.0,
            hourly_rate: 0.19,
            replacement_of: None,
        },
        InstanceCost {
            id: "i-old-2".to_string(),
            uptime_hours: 4.0,
            hourly_rate: 0.19,
            replacement_of: Some("i-old-1".to_string()),
        },
    ],
};
```

**Test:**
- Instance A: $1.52 (8 hrs × $0.19)
- Replacement: $0.76 (4 hrs × $0.19)
- Total: $2.28
- Verify chain tracked correctly

**Assertions:**
```rust
let total = chain.total_cost();
assert!((total - 2.28).abs() < 0.01);
assert_eq!(chain.instances[1].replacement_of, Some("i-old-1".to_string()));
```

**Expected:** ✅ PASS (replacement chain tracked)

---

#### 4.3 test_daily_cost_report_accuracy

**Purpose:** Aggregate daily costs across 9-instance cluster

**Setup:**
```rust
struct ClusterCostReport {
    date: String,
    instances: Vec<InstanceCost>,
    cluster_total: f64,
    on_demand_equivalent: f64,
    savings_percent: f64,
}

let report = ClusterCostReport {
    date: "2026-04-18".to_string(),
    instances: vec![
        // Orchestrator (on-demand, persistent)
        InstanceCost { hourly_rate: 0.34, uptime_hours: 24.0 },
        // 5 storage nodes (spot, 8hrs)
        InstanceCost { hourly_rate: 0.19, uptime_hours: 8.0 },
        InstanceCost { hourly_rate: 0.19, uptime_hours: 8.0 },
        InstanceCost { hourly_rate: 0.19, uptime_hours: 8.0 },
        InstanceCost { hourly_rate: 0.19, uptime_hours: 8.0 },
        InstanceCost { hourly_rate: 0.19, uptime_hours: 8.0 },
        // 2 client nodes (spot, 8hrs)
        InstanceCost { hourly_rate: 0.05, uptime_hours: 8.0 },
        InstanceCost { hourly_rate: 0.05, uptime_hours: 8.0 },
        // 1 conduit (spot, 8hrs)
        InstanceCost { hourly_rate: 0.015, uptime_hours: 8.0 },
        // 1 Jepsen (spot, 8hrs)
        InstanceCost { hourly_rate: 0.05, uptime_hours: 8.0 },
    ],
    cluster_total: 0.0,  // calculated
    on_demand_equivalent: 0.0,  // calculated
    savings_percent: 0.0,  // calculated
};
```

**Calculation:**
- Orchestrator: 24 hrs × $0.34 = $8.16
- Storage (5): 5 × 8 hrs × $0.19 = $7.60
- Clients (2): 2 × 8 hrs × $0.05 = $0.80
- Conduit: 8 hrs × $0.015 = $0.12
- Jepsen: 8 hrs × $0.05 = $0.40
- **Spot Total: $17.08**

On-demand equivalent (no spot discount):
- All 9 nodes on-demand, same uptime: ~$60-80 (depending on AZs)

**Test:**
- Generate report for cluster
- Verify total cost = sum of instances ±1%
- Verify savings_percent = (on-demand - spot) / on-demand ±2%

**Assertions:**
```rust
let report = generate_daily_report(&cluster_instances);
let expected_total = 8.16 + 7.60 + 0.80 + 0.12 + 0.40;
assert!((report.cluster_total - expected_total).abs() < 0.20); // ±$0.20
let expected_savings = (60.0 - report.cluster_total) / 60.0 * 100.0;
assert!((report.savings_percent - expected_savings).abs() < 5.0); // ±5%
```

**Expected:** ✅ PASS (report accurate within tolerance)

---

## Test Implementation Structure

```rust
#[cfg(test)]
mod preemptible_lifecycle_tests {
    use std::time::{Duration, Instant};
    use std::collections::HashMap;

    // ============= Group 1: Spot Pricing =============
    mod spot_pricing {
        #[test]
        fn test_spot_pricing_query_valid() { ... }

        #[test]
        fn test_spot_pricing_history_trend() { ... }

        #[test]
        fn test_breakeven_calculation() { ... }

        #[test]
        fn test_should_launch_decision_logic() { ... }
    }

    // ============= Group 2: Instance Lifecycle =============
    mod instance_lifecycle {
        #[test]
        fn test_provision_instance_success() { ... }

        #[test]
        fn test_provision_instance_with_retries() { ... }

        #[test]
        fn test_drain_instance_graceful() { ... }

        #[test]
        fn test_drain_instance_timeout() { ... }
    }

    // ============= Group 3: Disruption Handling =============
    mod disruption_handling {
        #[test]
        fn test_spot_termination_notice_detected() { ... }

        #[test]
        fn test_disruption_triggers_drain() { ... }

        #[test]
        fn test_replacement_launch_after_disruption() { ... }

        #[test]
        fn test_concurrent_disruptions() { ... }
    }

    // ============= Group 4: Cost Tracking =============
    mod cost_tracking {
        #[test]
        fn test_instance_cost_calculation() { ... }

        #[test]
        fn test_replacement_cost_included() { ... }

        #[test]
        fn test_daily_cost_report_accuracy() { ... }
    }

    // ============= Helper Mocks & Fixtures =============
    mod mocks {
        struct MockTerraform { ... }
        struct MockIMDS { ... }
        struct MockInstance { ... }
        // ... more mocks
    }
}
```

---

## Build & Test Commands

```bash
# Run all preemptible lifecycle tests
cargo test --test preemptible_lifecycle_tests -- --nocapture

# Run specific group
cargo test preemptible_lifecycle_tests::spot_pricing

# Run with logging
RUST_LOG=debug cargo test preemptible_lifecycle_tests

# Check for clippy warnings
cargo clippy -p claudefs-tests

# Check documentation
cargo doc -p claudefs-tests --no-deps --open
```

---

## Success Criteria

- ✅ All 15 tests compile without errors
- ✅ All 15 tests pass (100% pass rate)
- ✅ Zero clippy warnings in new test code
- ✅ Test execution time <5 seconds total
- ✅ Code coverage: all main functions tested
- ✅ No panics in tests (use Result assertions)
- ✅ Documentation/comments on all test functions

---

## OpenCode Delegation

**Ready for OpenCode implementation with minimax-m2p5**

**Input:** This file + `docs/A11-PHASE5-BLOCK2-PLAN.md`

**Expected Output:**
- `crates/claudefs-tests/src/preemptible_lifecycle_tests.rs` (500-600 LOC)
- 15 passing tests
- Zero clippy warnings
- Integration with existing test suite

---

**Document:** A11-PHASE5-BLOCK2-TESTS.md
**Created:** 2026-04-18 Session 12
**Status:** 🟡 PLANNING — Ready for OpenCode
