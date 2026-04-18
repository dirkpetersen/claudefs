# A11: Phase 5 Block 2 — Preemptible Instance Lifecycle Management

**Date:** 2026-04-18
**Agent:** A11 Infrastructure & CI
**Status:** 🟡 PLANNING — Ready for Implementation
**Previous Block:** Block 1 ✅ COMPLETE (36 Terraform tests)
**Target:** 3-4 days, ~1,200 LOC

---

## Executive Summary

Block 2 focuses on **cost-efficient cluster provisioning and management** by automating the lifecycle of preemptible (spot) instances. Key objectives:

1. **Spot Pricing Intelligence** — Query AWS Spot prices, calculate breakeven vs on-demand
2. **Automatic Replacement** — Launch replacement instances when spot interruptions occur
3. **Graceful Drain** — Migrate workloads off nodes before termination (2-minute window)
4. **Health Monitoring** — Track instance health via CloudWatch metrics
5. **Cost Attribution** — Measure per-instance, per-replacement costs
6. **Integration** — Wire into existing tools/cfs-dev and cost monitor

**Success Metric:** Achieve **60-70% cost savings** via spot instances with **zero manual ops** and **zero data loss** during replacements.

---

## Architecture: Three Components

### Component 1: Spot Pricing Engine (`tools/cfs-spot-pricing.sh`)

**Responsibility:** Query current spot prices and make replacement decisions

**Features:**
- Query AWS EC2 Spot Price History API
- Compare spot vs on-demand rates
- Calculate savings % for each instance type
- Track price trends over time
- Trigger auto-launch when prices dip below threshold (e.g., <50% of on-demand)

**Implementation:**
```bash
# tools/cfs-spot-pricing.sh
# Usage: cfs-spot-pricing query --instance-type i4i.2xlarge --region us-west-2
# Usage: cfs-spot-pricing should-launch --instance-type c7a.xlarge
# Usage: cfs-spot-pricing cost-breakdown --cluster

# Functions:
# - query_current_price()          # AWS API query
# - query_price_history()          # Last 7 days trend
# - calculate_breakeven()          # spot vs on-demand
# - should_launch_now()            # Buy/wait decision
# - estimate_interruption_rate()   # Historical rate from AWS
# - format_cost_report()           # Human-readable output
```

**LOC:** ~200 lines
**Tests:** 4-5 unit tests (mock AWS API responses)

---

### Component 2: Instance Lifecycle Manager (`tools/cfs-instance-manager.sh`)

**Responsibility:** Handle instance provisioning, termination, replacement

**Features:**
- Provision new instances via Terraform
- Bootstrap with cloud-init
- Wait for instance readiness (health checks)
- Tag instances with cost tracking metadata
- Drain workload before termination
- Launch replacement automatically
- Track instance age and disruption history

**Implementation:**
```bash
# tools/cfs-instance-manager.sh
# Usage: cfs-instance-manager provision --role storage --site a --count 1
# Usage: cfs-instance-manager drain --node-id i-12345 --timeout 120
# Usage: cfs-instance-manager replace --node-id i-12345 --reason spot-interrupted
# Usage: cfs-instance-manager status --cluster

# Functions:
# - provision_instance()           # Terraform + cloud-init
# - drain_instance()               # Graceful workload migration
# - terminate_instance()           # Clean shutdown
# - replace_instance()             # Launch replacement, update tags
# - wait_for_readiness()           # Health check loop
# - update_cost_tags()             # Add replacement cost data
# - get_instance_status()          # Cluster view
```

**LOC:** ~300 lines
**Tests:** 6-7 integration tests (mock Terraform, AWS CLI)

---

### Component 3: Disruption Handler (`tools/cfs-disruption-handler.sh`)

**Responsibility:** Detect and handle spot interruption events on running instances

**Features:**
- Poll EC2 Instance Metadata Service (IMDSv2) every 5 seconds
- Detect 2-minute termination notice
- Initiate graceful drain
- Log termination event for cost tracking
- Coordinate with replacement launch

**Implementation:**
```bash
# tools/cfs-disruption-handler.sh
# Installation: runs as systemd service cfs-spot-monitor.service
# Poll interval: 5 seconds

# Functions:
# - fetch_termination_notice()     # IMDS API call
# - has_termination_notice()       # Parse response
# - initiate_drain()               # Call cfs-instance-manager drain
# - log_disruption()               # Metrics + CloudWatch
# - trigger_replacement()          # Start new instance
# - retry_on_temporary_failure()   # Exponential backoff
```

**LOC:** ~250 lines
**Systemd service:** ~50 lines
**Tests:** 4-5 tests (mock IMDS responses)

---

## Implementation Details

### 1. Spot Pricing Intelligence

**AWS API Call:** EC2 DescribeSpotPriceHistory

```bash
aws ec2 describe-spot-price-history \
  --instance-types i4i.2xlarge c7a.xlarge t3.medium \
  --product-descriptions "Linux/UNIX" \
  --region us-west-2 \
  --start-time $(date -u -d '7 days ago' +%Y-%m-%dT%H:%M:%S) \
  --query 'SpotPriceHistory[*].[InstanceType,SpotPrice,Timestamp]' \
  --output json
```

**Calculation:**
- On-demand i4i.2xlarge: ~$0.624/hr (us-west-2)
- Current spot: ~$0.19/hr (30% of on-demand)
- Monthly savings: $(0.624 - 0.19) × 730 hrs = ~$317/month per node

**Decision Logic:**
- If spot < 50% on-demand AND interruption_rate < 5%: LAUNCH
- If spot > 70% on-demand: WAIT
- If interruption_rate > 10%: use on-demand

### 2. Graceful Drain (2-minute window)

**Timeline:**
- **T+0s:** Termination notice detected via IMDS
- **T+5s:** Initiate drain (stop accepting new ops)
- **T+5-115s:** Migrate workload to persistent orchestrator or other nodes
- **T+120s:** Graceful shutdown (final checkpoint)
- **T+120+ε:** Instance termination by AWS

**Drain Actions:**
1. Mark node as "draining" in cluster status
2. Signal all FUSE clients: "redirect to node X"
3. Move Raft leader elsewhere if needed (D4 multi-Raft)
4. Close replication connections gracefully
5. Flush pending writes to persistent storage
6. Wait for acknowledgment from clients (max 100s)
7. Exit cleanly, allowing termination

### 3. Replacement Launch

**Workflow:**
```
Disruption detected (T+0s)
    ↓
Initiate drain (T+5s)
    ↓
Wait for drain ACK (T+5-120s, timeout 115s)
    ↓
Instance terminates (T+120+ε, AWS kills it)
    ↓
Launch replacement via Terraform (T+120+5s)
    ↓
Cloud-init bootstrap (T+120+30s)
    ↓
Health check loop (T+120+60s, wait for ready)
    ↓
Rejoin cluster via SWIM (D2)
    ↓
Replica restoration (background)
    ↓
Ready for traffic (T+120+180s, ~3min total)
```

**Cost Impact:**
- 1 spot interruption/day = 1 replacement per day
- Replacement launch: ~3 min (no cost, on-demand orchestrator runs `terraform apply`)
- Monthly cost: ~$24 (spot nodes) + $10 (orchestrator) = $34 vs $80 (all on-demand)

### 4. Cost Tracking

**Tag Schema:**
```
Instance Tags:
  - Name: storage-site-a-node-1
  - Role: storage | client | jepsen
  - Site: A | B (for storage)
  - CostCenter: Testing
  - Agent: A11
  - StartTime: 2026-04-18T10:30:00Z
  - ReplacementOf: i-old-abcd1234 (if replacement)
  - DisruptionCount: 2
  - TotalUptime: 28800 (seconds)
  - EstimatedCost: $0.35 (for this instance)
```

**Cost Report:**
```json
{
  "period": "2026-04-18",
  "cluster": "claudefs-dev",
  "instances": [
    {
      "instance_id": "i-storage-a-1",
      "uptime_hours": 8,
      "instance_type": "i4i.2xlarge",
      "pricing_model": "spot",
      "hourly_rate": 0.19,
      "cost": 1.52,
      "disruption_count": 1,
      "replacement_cost": 0,
      "total": 1.52
    }
  ],
  "summary": {
    "cluster_cost": 24.50,
    "on_demand_equiv": 80.00,
    "savings": 55.50,
    "savings_percent": 69.4,
    "disruption_events": 3,
    "mttr_avg": "3m 42s"
  }
}
```

---

## Testing Strategy

### Test Module: `crates/claudefs-tests/src/preemptible_lifecycle_tests.rs`

**15 comprehensive tests** covering:

#### Group 1: Spot Pricing (4 tests)

1. **test_spot_pricing_query_valid**
   - Mock AWS API response
   - Verify parsing of instance types, prices, timestamps
   - Assert current_spot < on_demand

2. **test_spot_pricing_history_trend**
   - Mock 7-day historical data
   - Calculate trend (upward, stable, downward)
   - Assert trend accuracy within 5%

3. **test_breakeven_calculation**
   - Given spot=$0.19, on-demand=$0.624
   - Calculate discount% = (0.624-0.19)/0.624 = 69.6%
   - Assert result within 1%

4. **test_should_launch_decision**
   - Given: spot<50%, interruption_rate<5%
   - Assert: should_launch() == true
   - Given: spot>70%, interruption_rate>10%
   - Assert: should_launch() == false

#### Group 2: Instance Lifecycle (4 tests)

5. **test_provision_instance_success**
   - Mock Terraform apply
   - Verify `terraform apply` called with correct vars
   - Check tags applied (Name, Role, Site, Cost)
   - Assert instance online after bootstrap

6. **test_provision_instance_with_retries**
   - Mock transient Terraform failure (50% fail rate)
   - Verify exponential backoff retry (3 attempts)
   - Assert eventual success or max-retries error

7. **test_drain_instance_graceful**
   - Mock in-flight operations (10 pending)
   - Simulate graceful shutdown (operations complete within 100s)
   - Assert zero data loss
   - Verify "draining" status broadcasted

8. **test_drain_instance_timeout**
   - Mock slow operations (take 150s to complete)
   - Drain timeout set to 120s
   - Assert timeout error after 120s
   - Assert forced shutdown occurs

#### Group 3: Disruption Handling (4 tests)

9. **test_spot_termination_notice_detected**
   - Mock IMDS response with termination notice
   - Poll every 5s
   - Assert detection within 10s (2 polls)

10. **test_disruption_triggers_drain**
    - Mock spot termination notice
    - Verify drain initiated within 1s
    - Assert graceful shutdown follows

11. **test_replacement_launch_after_disruption**
    - Mock spot interruption
    - Verify replacement launch initiated at T+120s
    - Check replacement tagged with ReplacementOf metadata
    - Assert new instance ready within 3 minutes

12. **test_concurrent_disruptions**
    - Mock 3 simultaneous spot interruptions
    - Verify all 3 drains initiated without deadlock
    - Assert 3 replacements launched
    - Check cost tags updated correctly

#### Group 4: Cost Tracking (3 tests)

13. **test_instance_cost_calculation**
    - Instance: i4i.2xlarge, spot, uptime 2 hours
    - Rate: $0.19/hr
    - Expected cost: $0.38
    - Assert within 1% accuracy

14. **test_replacement_cost_included**
    - Instance A: $1.52, replaced after 8 hours
    - Instance A-replacement: $0.70, uptime 4 hours
    - Total for A+A-replacement: $2.22
    - Assert ReplacementOf chain tracked

15. **test_daily_cost_report_accuracy**
    - 9 instances with varying uptimes and disruptions
    - Generate daily report
    - Assert total_cost = sum of individual costs ±1%
    - Assert savings_percent = (on-demand - spot) / on-demand ±2%

---

## Integration Points

### With Existing Tools

1. **tools/cfs-dev** (existing)
   - `cfs-dev up` → provisions cluster via Terraform (Block 1)
   - New: `cfs-instance-manager provision` called if scaling up
   - `cfs-dev down` → tears down cluster (graceful drain via Block 2)

2. **tools/cfs-cost-monitor.sh** (Phase 4 Block 5)
   - Reads spot pricing data from `cfs-spot-pricing`
   - Triggers replacement decisions from `cfs-instance-manager`
   - Aggregates costs into daily/monthly reports

3. **tools/cfs-watchdog.sh** (Phase 4 Block 4)
   - Detects dead instances (no process for 2 min)
   - Calls `cfs-instance-manager replace --reason health-check`
   - Keeps agents running through disruptions

4. **tools/cfs-supervisor.sh** (Phase 4 Block 4)
   - Monitors for uncaught disruptions
   - Validates cluster topology after replacements
   - Restarts failed reconciliations

### With ClaudeFS Core

1. **Cluster Membership (D2 SWIM)**
   - New nodes join via `cfs server join --seed ...`
   - Existing nodes gossip membership changes
   - No Terraform state needed after join

2. **Raft Multi-Shard (D4)**
   - Losing a node triggers automatic re-election
   - Shards re-replicate to surviving nodes
   - Multi-Raft provides parallelism

3. **Data Reduction (A3)**
   - Drain triggers background GC (no writes during migration)
   - Reduces memory footprint before shutdown
   - Helps replacement catch up on recovery

---

## Success Criteria

### Performance

- [ ] Spot interruption → graceful shutdown: **<2 min**
- [ ] Graceful shutdown → replacement online: **<3 min**
- [ ] Concurrent disruptions (3+ nodes): **no deadlocks**
- [ ] Cost savings via spot: **60-70%**
- [ ] Replacement launch latency: **<5 min**

### Reliability

- [ ] Zero data loss during replacement
- [ ] Zero missed disruptions (100% detection rate)
- [ ] Graceful drain completion: >95% (5-min window)
- [ ] Concurrent drains: all succeed simultaneously
- [ ] Cost tracking accuracy: ±1%

### Observability

- [ ] All events logged to `/var/log/cfs-instance-*.log`
- [ ] CloudWatch metrics: disruptions, replacements, costs
- [ ] Grafana dashboard: instance lifecycle timeline
- [ ] Alerts: node down, drain timeout, replacement failure

### Documentation

- [ ] README: spot pricing decision logic (200 lines)
- [ ] Runbook: troubleshoot a failed drain (100 lines)
- [ ] Cost breakdown: instance types, pricing, savings (150 lines)
- [ ] Architectural diagram: disruption timeline (ASCII + explain)

---

## Implementation Plan: 3-4 Days

### Day 1: Spot Pricing Engine + Tests

- [ ] Write `tools/cfs-spot-pricing.sh` (200 LOC)
- [ ] Write 4-5 tests for pricing logic
- [ ] Mock AWS API responses
- [ ] Verify calculations (breakeven, discount %)
- [ ] Commit: "[A11] Phase 5 Block 2: Spot Pricing Engine"

### Day 2: Instance Lifecycle Manager + Tests

- [ ] Write `tools/cfs-instance-manager.sh` (300 LOC)
- [ ] Implement provision, drain, replace, status subcommands
- [ ] Write 6-7 integration tests
- [ ] Mock Terraform and AWS CLI
- [ ] Test graceful drain with timeout
- [ ] Commit: "[A11] Phase 5 Block 2: Instance Lifecycle Manager"

### Day 3: Disruption Handler + Integration Tests

- [ ] Write `tools/cfs-disruption-handler.sh` (250 LOC)
- [ ] Write `systemd/cfs-spot-monitor.service` (50 LOC)
- [ ] Write 4-5 integration tests
- [ ] Mock IMDS responses
- [ ] Test drain → replacement workflow
- [ ] Test concurrent disruptions
- [ ] Commit: "[A11] Phase 5 Block 2: Disruption Handler"

### Day 4: Cost Tracking + Documentation

- [ ] Enhance cost tracking tags (update instance manager)
- [ ] Write 3 cost calculation tests
- [ ] Integrate with existing cost monitor
- [ ] Write documentation (500 lines)
- [ ] Final testing, clippy checks
- [ ] Commit: "[A11] Phase 5 Block 2: Complete (15 tests, ~1,200 LOC)"

---

## Dependencies & Blockers

### Hard Dependencies
- ✅ Block 1 (Terraform) — required, already complete
- ✅ Phase 4 Cost Monitor — integration target, already complete
- ✅ AWS account with EC2/Spot permissions — assumed available

### Soft Dependencies
- 🟡 A1 Phase 11 (online scaling) — helps with recovery, not blocking
- 🟡 A8 monitoring (Phase 5 Block 4) — for Grafana dashboards (later)

### Potential Blockers
- **IMDS timeout:** If EC2 Metadata Service is slow or unavailable
  - Mitigation: exponential backoff + fallback to health check API
- **Terraform state lock:** If multiple agents provision simultaneously
  - Mitigation: DynamoDB lock table (created in Block 1)
- **Spot price spikes:** If all nodes interrupted simultaneously
  - Mitigation: retain last 1-2 on-demand instances for failover

---

## Risk Mitigation

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|-----------|
| **Drain timeout (workload stuck)** | Medium | High | 2-min hard deadline + forced shutdown |
| **Cost tags lost** | Low | Medium | Terraform-stored tags + audit trail |
| **IMDS unavailable** | Low | High | Fallback to CloudWatch metrics |
| **Spot all interrupted** | Very low | Critical | Keep 1 on-demand fallback |
| **Data loss during drain** | Very low | Critical | Verify before -> acknowledge drain OK |

---

## Next Steps After Block 2

Once Block 2 is complete:
- Run integration tests with real spot instances (cost: ~$1-2)
- Document operational runbooks for each failure scenario
- Move to **Block 3: GitHub Actions CI/CD** (Blocks 2 & 3 can overlap)

---

## References

- [AWS Spot Instances](https://aws.amazon.com/ec2/spot/)
- [EC2 Instance Metadata Service](https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/ec2-instance-metadata.html)
- [Phase 4 Cost Monitor](A11-PHASE4-BLOCK5-PLAN.md)
- [Phase 5 Block 1 Terraform](A11-PHASE5-BLOCK1-TERRAFORM-TESTS.md)
- [Architecture Decisions](decisions.md) — D2 (SWIM), D4 (Raft shards)

---

**Document:** A11-PHASE5-BLOCK2-PLAN.md
**Created:** 2026-04-18 Session 12
**Status:** 🟡 PLANNING — Ready for implementation approval
