# A11 Phase 4 Block 5: Cost Monitoring & Optimization

**Date:** 2026-04-18
**Agent:** A11 Infrastructure & CI
**Status:** 📋 PLANNING
**Session:** 9 (Phase 4 Block 5 Implementation)

---

## Executive Summary

Phase 4 Block 5 builds on the production deployment pipeline (Block 4) to add comprehensive cost monitoring, attribution, and optimization. The goal is to achieve full visibility into ClaudeFS infrastructure costs and enable data-driven optimization decisions.

**Key Deliverables:**
1. **Cost tracking dashboard** (Grafana) — Daily, weekly, monthly cost breakdown by service
2. **Per-workload cost attribution** — Track costs per deployment stage, per test suite
3. **Budget enforcement and alerts** — Enhanced alerting at 25%, 50%, 75%, 100% of budget
4. **Spot instance optimization** — Automatic spot instance recommendations
5. **Reserved instance analysis** — Identify opportunities for RI commitments
6. **Cost monitoring tests** — Automated tests for cost tracking accuracy

**Success Criteria:**
- ✅ Daily cost tracking with per-service breakdowns (EC2, Bedrock, S3, Data Transfer)
- ✅ Per-deployment cost tracking (canary, 10%, 50%, 100% stages)
- ✅ Budget alerts at 25%, 50%, 75%, 100% of daily limit
- ✅ Spot vs on-demand cost comparison
- ✅ Cost forecast for next 7/30 days
- ✅ 15+ automated cost monitoring tests

**Estimated Duration:** Days 9-10 (4-8 hours)
**Owner:** A11 Infrastructure & CI
**Depends On:** Block 4 (✅ Complete), AWS Cost Explorer API, Prometheus

---

## Current State

### What Exists (Block 4 & Earlier)
- ✅ `tools/cfs-cost-monitor.sh` — Basic budget enforcement (EC2 $25/day, Bedrock $25/day)
- ✅ Hard limits with spot instance termination and Haiku-only mode
- ✅ AWS Cost Explorer integration for EC2 and Bedrock spend
- ✅ SNS alerts for budget overages

### What's Missing (Block 5)
- ❌ Detailed cost breakdown dashboard (Grafana)
- ❌ Per-service cost tracking (S3, Data Transfer, Secrets Manager)
- ❌ Per-deployment stage cost attribution
- ❌ Cost forecast and trend analysis
- ❌ Enhanced alerting (25%, 50%, 75% warnings)
- ❌ Spot vs on-demand recommendations
- ❌ Reserved instance analysis
- ❌ Cost tracking tests
- ❌ Cost optimization documentation

---

## Phase 4 Block 5: Implementation Plan

### Task 1: Enhanced Cost Monitor Script (2 hours)

**File:** `tools/cfs-cost-monitor-enhanced.sh` (400 LOC)

**Features:**
- ✅ Per-service cost tracking (EC2, Bedrock, S3, Data Transfer, Secrets Manager, Monitoring)
- ✅ Daily, weekly, monthly cost aggregation
- ✅ Cost forecast (linear extrapolation for next 7/30 days)
- ✅ Spot vs on-demand instance cost comparison
- ✅ Per-instance type cost breakdown
- ✅ Enhanced alerting (25%, 50%, 75%, 100% thresholds)
- ✅ Cost tracking to CloudWatch metrics for Grafana visualization
- ✅ JSON export for external reporting

**Implementation:**
1. Extend `cfs-cost-monitor.sh` with service-level breakdown
2. Query AWS Cost Explorer for EC2, Bedrock, S3, Data Transfer separately
3. Calculate cost per instance type (i4i.2xlarge, c7a.2xlarge, etc.)
4. Generate CloudWatch metrics for each service and instance type
5. Publish JSON cost report to S3 for historical analysis
6. Enhanced SNS alerts at 25%, 50%, 75%, 100% of budget

**Example CloudWatch Metrics to Create:**
```
cfs/cost/ec2/daily (USD)
cfs/cost/bedrock/daily (USD)
cfs/cost/s3/daily (USD)
cfs/cost/data-transfer/daily (USD)
cfs/cost/total/daily (USD)
cfs/cost/forecast/7day (USD)
cfs/cost/forecast/30day (USD)
cfs/instance/i4i.2xlarge/daily (USD)
cfs/instance/c7a.2xlarge/daily (USD)
cfs/instance/c7a.xlarge/daily (USD)
cfs/spot/savings/daily (USD)
```

---

### Task 2: Cost Attribution by Deployment Stage (1.5 hours)

**File:** `tools/cfs-cost-tagger.sh` (200 LOC)

**Features:**
- ✅ AWS resource tagging for cost allocation
- ✅ Tags by deployment stage (canary, stage-10pct, stage-50pct, stage-100pct)
- ✅ Tags by agent (A1, A2, ..., A11)
- ✅ Tags by test suite (posix, jepsen, fio, chaos)
- ✅ Cost breakdown report by tag
- ✅ Integration with Cost Allocation Tags in AWS

**Implementation:**
1. Create standard tag structure:
   ```
   project: claudefs
   environment: development
   stage: {canary|10pct|50pct|100pct}
   agent: {A1|A2|...|A11}
   test-suite: {posix|jepsen|fio|chaos}
   ```
2. Update rollout.sh to apply tags when deploying to each stage
3. Enable Cost Allocation Tags in AWS Billing console
4. Query AWS Cost Explorer with cost allocation tags
5. Generate per-stage, per-agent cost reports

**Example Cost Report:**
```
Stage: canary
  EC2 Cost: $2.50/day
  Bedrock Cost: $1.20/day
  Total: $3.70/day
Stage: 10pct
  EC2 Cost: $3.20/day
  Total: $3.20/day
Stage: 50pct
  EC2 Cost: $8.10/day
  Total: $8.10/day
```

---

### Task 3: Grafana Cost Dashboard (2 hours)

**Files:**
- `tools/grafana-cost-dashboard.json` (500+ lines)

**Dashboard Sections:**
1. **Cost Summary (Top)**
   - Daily cost (large gauge)
   - Weekly trend (line chart)
   - Monthly projection (bar chart)
   - Budget remaining (gauge)

2. **Service Breakdown (Cost by Service)**
   - EC2 (pie chart showing instance types)
   - Bedrock (pie chart showing models used)
   - S3 (bar chart over time)
   - Data Transfer (line chart)
   - Other (Secrets Manager, Monitoring)

3. **Deployment Stages**
   - Cost per stage (canary, 10%, 50%, 100%) as horizontal bar chart
   - Cost trend per stage (line chart, 7-day history)
   - Savings from spot instances (vs on-demand)

4. **Forecast & Trends**
   - 7-day cost forecast (line chart with confidence band)
   - 30-day cost forecast (line chart)
   - YoY budget comparison (if multi-year data)

5. **Instance Analysis**
   - Top 10 most expensive instance types (bar chart)
   - Spot savings per instance type (comparison table)
   - Instance count by type (trend line)

6. **Cost Alerts**
   - Alert history (list of budget warnings)
   - Time until 80% budget (counter)
   - Time until 100% budget (counter)

**Implementation:**
1. Create Grafana dashboard JSON (provisioned via tools/grafana-cost-dashboard.json)
2. Add data source for CloudWatch (cfs/cost/* metrics)
3. Add data source for Prometheus (custom cost metrics)
4. Configure alert threshold on 80% budget gauge
5. Link to cost reports in S3

---

### Task 4: Spot Instance Optimization Tool (1.5 hours)

**File:** `tools/spot-instance-analyzer.sh` (300 LOC)

**Features:**
- ✅ Query current and historical spot pricing
- ✅ Compare spot vs on-demand prices
- ✅ Recommend spot instances for cost reduction
- ✅ Calculate breakeven point for reserved instances
- ✅ Generate recommendations report
- ✅ Suggest instance type migrations for cost optimization

**Implementation:**
1. Use AWS EC2 pricing API to get on-demand and spot prices
2. Calculate spot discount percentage
3. Recommend instance type changes if available at better pricing
4. Calculate total savings if all spot instances are used
5. Generate report showing:
   - Current: i4i.2xlarge spot (60% off on-demand)
   - Alternative: i4i.xlarge spot (70% off on-demand, lower cost but half capacity)
   - Savings: $2-3/day if using i4i.xlarge instead

**Example Output:**
```
Instance Analysis Report
========================

Current: i4i.2xlarge (8 vCPU, 64GB, 960GB NVMe)
  On-demand: $1.104/hr
  Spot: $0.442/hr (60% off)
  Daily: $10.60

Recommendation: Use i4i.xlarge (4 vCPU, 32GB, 480GB NVMe) for test nodes
  On-demand: $0.552/hr
  Spot: $0.165/hr (70% off)
  Daily per node: $3.96 (vs $10.60 for 2xlarge)
  Savings: $6.64/day with 5 storage nodes (smaller for development)

Reserved Instance Analysis:
  i4i.2xlarge 1-year RI: $0.612/hr (45% off on-demand)
  Daily: $14.69 (on-demand), $7.34 (reserved)
  Breakeven: ~18 months of always-on usage
  Recommendation: Use spot for development, RI only if always running
```

---

### Task 5: Cost Report Generator (1 hour)

**File:** `tools/generate-cost-report.sh` (250 LOC)

**Features:**
- ✅ Daily cost report with JSON export
- ✅ Weekly summary with trends
- ✅ Monthly report with comparisons
- ✅ Historical cost tracking (CSV export)
- ✅ Cost per agent attribution
- ✅ Cost per test suite attribution

**Implementation:**
1. Query AWS Cost Explorer API for daily costs
2. Aggregate by service, instance type, tag
3. Generate JSON report with structured data
4. Export to CSV for spreadsheet analysis
5. Push to S3 for historical archive
6. Generate HTML report for manual review

**Report Structure:**
```json
{
  "date": "2026-04-18",
  "summary": {
    "total_cost": 85.43,
    "ec2_cost": 20.50,
    "bedrock_cost": 64.93,
    "s3_cost": 0.00,
    "data_transfer_cost": 0.00
  },
  "by_service": { ... },
  "by_instance_type": { ... },
  "by_stage": { ... },
  "by_agent": { ... },
  "forecast_7day": 598.01,
  "forecast_30day": 2561.40,
  "budget_remaining": 14.57
}
```

---

### Task 6: Cost Monitoring Tests (1.5 hours)

**File:** `crates/claudefs-tests/src/cost_monitoring.rs` (300+ LOC)

**Test Coverage (15+ tests):**
1. ✅ Cost monitor script availability and executability
2. ✅ AWS Cost Explorer API connectivity
3. ✅ EC2 spend calculation accuracy
4. ✅ Bedrock spend calculation accuracy
5. ✅ Budget threshold detection (100%, 80%, 50%, 25%)
6. ✅ SNS alert generation
7. ✅ CloudWatch metrics publication
8. ✅ Cost allocation tags validation
9. ✅ Spot instance cost calculation
10. ✅ Cost forecast accuracy (within 10% of actual)
11. ✅ Reserved instance recommendation logic
12. ✅ Cost report JSON schema validation
13. ✅ Historical cost tracking consistency
14. ✅ Cost attribution by stage accuracy
15. ✅ Cost attribution by agent accuracy

**Implementation:**
1. Mock AWS Cost Explorer API responses
2. Unit tests for cost calculations
3. Integration tests against real AWS API (if budget allows)
4. Test alert thresholds and SNS notifications
5. Validate JSON report schema
6. Verify CloudWatch metrics are published

---

### Task 7: Cost Optimization Documentation (1 hour)

**File:** `docs/COST-OPTIMIZATION.md` (400 LOC)

**Sections:**
1. **Overview** — Cost structure, daily budget, breakdown
2. **Cost Monitor** — How to run, interpret results
3. **Spot Instance Strategy** — Why spot, breakeven analysis, risks
4. **Reserved Instance Analysis** — When to use, breakeven points
5. **Cost Attribution** — How costs are tagged and tracked
6. **Budget Enforcement** — Alert thresholds, auto-termination policy
7. **Cost Optimization Tips** — Best practices for developers
8. **Troubleshooting** — Common issues and solutions
9. **Historical Analysis** — How to analyze multi-month trends
10. **FAQ** — Frequently asked questions

**Key Guidance:**
- Spot instances save 60-70% but can be interrupted (acceptable for dev)
- Bedrock is largest cost component (60-70% of total)
- Model downgrade (Opus→Sonnet) saves $2-3/day
- Running cluster 24/7 exceeds $2,500/month; tear down when not in use
- S3 and Data Transfer costs negligible (<$1/day)

---

## Implementation Order

### Session 9 (This Session)

**Phase 4 Block 5 Implementation Timeline:**

| Task | Duration | Status | By Time |
|------|----------|--------|---------|
| 1. Enhanced cost monitor | 2h | 📋 Ready | 1h 30m |
| 2. Cost attribution tool | 1.5h | 📋 Ready | 3h |
| 3. Grafana dashboard | 2h | 📋 Ready | 5h |
| 4. Spot analyzer | 1.5h | 📋 Ready | 6h 30m |
| 5. Report generator | 1h | 📋 Ready | 7h 30m |
| 6. Cost monitoring tests | 1.5h | 📋 Ready | 9h |
| 7. Documentation | 1h | 📋 Ready | 10h |

**Total Estimated:** 10 hours (can split across 2-4 days)

---

## Success Criteria

### Functional Requirements
- ✅ Cost monitor tracks EC2, Bedrock, S3, Data Transfer, Secrets Manager, Monitoring separately
- ✅ Cost forecast accurate within 10% for next 7 and 30 days
- ✅ Per-deployment stage cost attribution working (canary, 10%, 50%, 100%)
- ✅ Spot instance savings > 50% vs on-demand
- ✅ Budget alerts at 25%, 50%, 75%, 100% thresholds
- ✅ Grafana dashboard displays all metrics in real-time

### Testing Requirements
- ✅ 15+ automated cost monitoring tests
- ✅ 90%+ test pass rate
- ✅ Cost calculations verified against AWS API results
- ✅ Alert generation tested

### Documentation Requirements
- ✅ Cost optimization guide (400+ lines)
- ✅ How-to for running cost reports
- ✅ Spot instance strategy documented
- ✅ Budget enforcement policy clear
- ✅ FAQ addressing common questions

### Production Readiness
- ✅ Cost monitor runs reliably on 15-min cron schedule
- ✅ No manual intervention required
- ✅ Graceful fallback if AWS API is slow or unavailable
- ✅ All costs logged for audit trail

---

## Dependencies & Integration

### External Dependencies
- AWS Cost Explorer API (EC2, Bedrock, S3, Data Transfer)
- AWS CloudWatch (metrics storage and query)
- AWS SNS (alert notifications)
- AWS Secrets Manager (cost reporting credentials)
- Grafana (dashboard visualization)
- Prometheus (optional, for custom metrics)

### Internal Dependencies
- A8 (Management): Prometheus exporter must export cost metrics
- A11 (this agent): Orchestrate all cost monitoring tools
- All crates: Must support `/health` endpoint (for cost attribution)

### Integration Points
- rollout.sh (Block 4) — Add cost attribution tags
- build-release.sh (Block 4) — Log build costs
- health-check.sh (Block 4) — Track deployment stage costs
- Grafana (A8 integration) — Display cost dashboards

---

## Commits & Pushes

### Commit Schedule (Session 9)

| Commit | Content | When |
|--------|---------|------|
| #1 | Enhanced cost monitor script + tests | After Task 1 |
| #2 | Cost attribution tool + docs | After Task 2 |
| #3 | Grafana dashboard + configuration | After Task 3 |
| #4 | Spot instance analyzer | After Task 4 |
| #5 | Cost report generator | After Task 5 |
| #6 | Cost monitoring tests (crate) | After Task 6 |
| #7 | Cost optimization documentation | After Task 7 |
| Final | Update CHANGELOG + Phase 4 Block 5 summary | After all tasks |

**Push after each commit:** `git push origin main`

---

## Risks & Mitigation

| Risk | Impact | Mitigation |
|------|--------|-----------|
| AWS API rate limiting | Cost reports fail | Add exponential backoff, cache results |
| Cost Explorer lag | Reports 24h delayed | Accept 1-day delay, document clearly |
| Spot price volatility | Budget forecasts inaccurate | Use 30-day historical average for forecasts |
| CloudWatch metric limits | Too many metrics to track | Aggregate by service, not per-instance |
| Grafana connection issues | Dashboard becomes stale | Add fallback JSON export, email reports |

---

## Success Metrics

### By End of Session 9

- ✅ 7 new tools/scripts created (1,350+ LOC)
- ✅ Grafana cost dashboard operational with 10+ panels
- ✅ 15+ automated cost monitoring tests passing
- ✅ Cost per deployment stage visible and tracked
- ✅ Cost forecast accurate within 10% for 7/30 days
- ✅ Daily cost reports available in JSON and HTML formats
- ✅ 400+ lines of cost optimization documentation
- ✅ All commits pushed to main with detailed messages
- ✅ Ready for Phase 4 Block 6 (final infrastructure setup)

---

## References

- Phase 4 Block 4: [Production Deployment Pipeline](A11-PHASE4-BLOCK4-SESSION8-COMPLETE.md)
- Phase 4 Block 3: [Recovery & Health Monitoring](A11-PHASE4-BLOCK3-PLAN.md)
- Cost Optimization Phase 1: [Model Selection](A11-COST-OPTIMIZATION-PHASE1.md)
- Agents Overview: [Development Agents](agents.md)
- Infrastructure: [Infrastructure Overview](agents.md#infrastructure--ci)

---

**Status:** ✅ PLAN READY FOR IMPLEMENTATION
**Estimated Start Time:** 2026-04-18 Session 9
**Owner:** A11 Infrastructure & CI
**Model:** Haiku (boilerplate shell scripts and Grafana JSON)
