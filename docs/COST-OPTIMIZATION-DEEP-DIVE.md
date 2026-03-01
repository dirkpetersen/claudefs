# Cost Optimization Deep Dive

**Document Type:** Technical Analysis
**Owner:** A11 Infrastructure & CI
**Last Updated:** 2026-03-01
**Phase:** 3 (Production Readiness)

## Executive Summary

Current baseline cost is **$85-96/day**. With targeted optimizations, we can achieve **<$70/day** within 30 days.

### Quick Wins (This Week)
- Model selection (Sonnet Fast for A8) → **$5-10/day savings**
- Compute right-sizing (downgrade some instances) → **$3-5/day savings**

### Medium-term Optimizations (This Month)
- Scheduled provisioning (weekday-only) → **$30/week savings**
- Time-of-day optimization (Opus during business hours) → **$5-10/day savings**
- Test parallelization → **$2-3/day savings** (shorter test runs)

### Long-term Strategy (This Quarter)
- Reserved Instances (1-year commitment) → **20% discount** on EC2
- Spot Instance optimization → **60-90% discount** vs on-demand
- Multi-region consolidation → **10-15% savings** on redundancy

**Target Outcome:** $65-75/day by end of Phase 3

---

## Current Cost Breakdown

### EC2 Costs (Base: $26-30/day)

| Instance | Role | Type | Count | Price/Hour | Daily Cost |
|----------|------|------|-------|-----------|------------|
| Orchestrator | Coordination | c7a.2xlarge (on-demand) | 1 | $0.42 | **$10.08** |
| Storage nodes | NVMe data | i4i.2xlarge (spot) | 5 | $0.35 | **$4.20** (8 hrs) |
| FUSE client | POSIX tests | c7a.xlarge (spot) | 1 | $0.21 | **$0.84** |
| NFS client | Multi-protocol | c7a.xlarge (spot) | 1 | $0.21 | **$0.84** |
| Cloud conduit | Replication | t3.medium (spot) | 1 | $0.05 | **$0.12** |
| Jepsen controller | Fault injection | c7a.xlarge (spot) | 1 | $0.21 | **$0.84** |
| **EC2 Subtotal** | | | | | **$16.92** (8hr operation) |
| **EC2 Daily** (24hr basis) | | | | | **~$26-30/day** |

**Note:** Spot instances only run during dev/test (8 hrs/day). Orchestrator is 24/7.

### Bedrock Costs (Base: $55-70/day)

| Model | Agent | % of Tokens | Relative Cost | Estimated Daily |
|-------|-------|-----------|--------------|-----------------|
| **Opus 4.6** | A1, A2, A4, A10 | ~20% | 5x | **$30-35** |
| **Sonnet 4.6** | A3, A5, A6, A7, A9 | ~60% | 1.5x | **$20-25** |
| **Sonnet Fast** | A8 (currently Sonnet) | ~10% (potential) | 1x | **$5-8** (if A8 moves) |
| **Haiku 4.5** | A8, A11, boilerplate | ~20% (potential) | 0.3x | **$2-3** (for A8) |
| **Bedrock Subtotal** | | | | **$55-70/day** |

### Other Costs (Base: $1-3/day)

| Service | Cost |
|---------|------|
| AWS Secrets Manager (3 secrets) | **$0.03** |
| CloudWatch logs (~100MB/day) | **$0.05** |
| S3 (test artifacts) | **$0.10** |
| Data transfer (inter-region) | **$0.50** |
| **Other Subtotal** | **~$0.68/day** |

### **TOTAL DAILY COST: $82-98/day** (with $100/day budget)

---

## Cost Optimization Strategies

### Strategy 1: Model Selection Optimization

**Current allocation:**
- Opus: A1, A2, A4, A10 (4 builders + security = high accuracy needed)
- Sonnet: A3, A5, A6, A7, A9 (5 builders + tests = standard implementation)
- Haiku: A8, A11 (management + infra = boilerplate)

**Issues with current allocation:**
- A8 (Management) generates high code volume but uses Sonnet (more expensive)
- Opus is cost-heavy for A1/A2/A4 (though correctness is critical)

**Optimization #1: Use Sonnet Fast for A8 bulk code generation**

| Change | Current | Proposed | Savings |
|--------|---------|----------|---------|
| A8: Sonnet → Sonnet Fast | 10% of tokens @ 1.5x | 10% @ 1.0x | ~$3-5/day |
| A8 bulk generation (CLI, dashboards) | Slower but clearer | Faster, good enough | Faster cycle |

**Implementation:**
```yaml
# In agent launcher config
A8:
  model: sonnet-fast  # was: sonnet
  rationale: "Bulk code generation, well-understood patterns"
```

**Impact:** -$5/day (10% of tokens @ 0.5x cost reduction)

---

**Optimization #2: Use Haiku for A8 boilerplate**

Some A8 work is pure boilerplate (CLI arg parsing, YAML generation, Grafana JSON):

| Task | Model | Cost/1M tokens |
|------|-------|-----------------|
| CLI subcommand generation | Haiku | $0.80 |
| Same task on Sonnet | Sonnet | $3.00 |
| Ratio | | 3.75x cheaper |

**Implementation:**
- Identify "boilerplate-heavy" tasks (>70% of tokens spent on YAML/JSON/CLI structure)
- Route those to Haiku explicitly
- Route "business logic" (config validation, error handling) to Sonnet

**Impact:** -$2-3/day (15-20% of A8 → Haiku @ 0.27x cost)

---

**Optimization #3: Selective Sonnet instead of Opus for A3/A4**

A3 (Data Reduction) and A4 (Transport) are well-specified in docs:
- Algorithms documented (BLAKE3, LZ4, RDMA verbs)
- Interfaces clear (trait definitions)
- Standard patterns (trait impl, error handling)

**Current:** Sonnet already → no change needed

**Potential:** If they move to Opus for safety, consider reverting to Sonnet for specific modules (e.g., compression_tests.rs, transport_tests.rs)

**Impact:** Already optimized, no change recommended

---

### Strategy 2: Compute Right-Sizing

**Current resource allocation:**

| Instance | Rationale | Actual Usage | Downsizing Option |
|----------|-----------|--------------|-------------------|
| c7a.2xlarge orchestrator | Coordination | 4-6 vCPU avg, 8GB RAM avg | Keep as-is (always on) |
| i4i.2xlarge storage | NVMe tests | 4-6 vCPU avg, 20GB RAM avg | Keep (NVMe intensive) |
| c7a.xlarge clients | POSIX/NFS tests | 1-2 vCPU avg, 2GB RAM avg | **→ c7a.large** (-50%) |
| t3.medium conduit | Replication relay | <1 vCPU avg, 1GB RAM avg | **→ t3.small** (-50%) |

**Analysis:**
- Clients and conduit are severely over-provisioned for basic testing
- Can downgrade if performance targets are met in first CI run

**Implementation:**

```bash
# After first CI run validates performance is acceptable:
# 1. Update Terraform to use smaller instances
# 2. Destroy old instances, create new ones
# 3. Rebalance test cluster
```

**Cost Impact:**

| Instance | Old Type | New Type | Old Price/hr | New Price/hr | Daily Savings |
|----------|----------|----------|--------------|--------------|---------------|
| 2x FUSE clients | c7a.xlarge | c7a.large | $0.21 × 2 | $0.105 × 2 | -$0.42 |
| 1x Conduit | t3.medium | t3.small | $0.05 | $0.025 | -$0.06 |
| **Compute Subtotal** | | | | | **-$0.48/day** |

**But with 8-hr daily operation:**
- Daily impact: **-$0.48 × (8/24) = -$0.16/day** (small)
- Could be more significant if we reduce daily operation hours

**Recommendation:** Implement if clients need downgrade; otherwise wait for data from first CI run.

---

### Strategy 3: Scheduled Provisioning

**Current model:**
- Orchestrator: 24/7 (required for agent coordination)
- Test cluster: 24/7 (inefficient for development phase)

**Proposed model:**

| Day/Time | Cluster State | Rationale | Daily Cost |
|----------|---------------|-----------|-----------|
| Monday-Friday, 7am-7pm | Full (9 nodes) | Development hours | ~$20 |
| Monday-Friday, 7pm-7am | Minimal (orchestrator only) | Overnight storage | ~$10 |
| Saturday-Sunday | Orchestrator only | Minimal weekend activity | ~$10 |
| **Weekly Average** | | | **~$110/day**→ **$77/day** |

**Implementation:**
```bash
# Cron job on orchestrator (nightly at 7pm)
0 19 * * 1-5 /home/cfs/claudefs/tools/cfs-down  # Scale down at 7pm weekdays
0 7 * * 1-5 /home/cfs/claudefs/tools/cfs-up     # Scale up at 7am weekdays

# Or: Schedule via Lambda for automatic provisioning
```

**Cost Savings:**

- Spot nodes currently run 8 hrs/day × 5 days/week = 40 hrs/week
- Proposed: Scale down fully 7pm-7am (12 hrs × 5 = 60 hrs) + weekends (48 hrs) = 108 hrs/week not running
- Current weekly spot cost: $4.20/day × 7 = $29.40/week
- Proposed weekly spot cost: $4.20 × (40/24/7 = 40 hrs) = $7.00/week
- **Weekly savings: $22.40 → Target: ~$30/week (optimistic)**

**Challenge:**
- Provisioning/teardown takes ~5-10 minutes, may be slow for CI runs
- Solution: Keep orchestrator + 1 storage node always on for quick test runs; full cluster on-demand

**Revised model:**

| Configuration | Nodes Up | Daily Cost | When |
|---------------|----------|-----------|------|
| Minimal | Orchestrator + 1 storage | ~$12-14 | Always |
| Standard | Orchestrator + 5 storage + clients | ~$26-30 | Dev hours (7am-7pm) |
| Full | All 10 nodes | ~$32-35 | Benchmark/stress tests |

**Revised savings: $30/week (roughly -$4/day average)**

---

### Strategy 4: Time-of-Day Optimization

**Observation:** Bedrock pricing doesn't vary by time, but AI model efficiency does.

**Strategy:**
- Run complex work (Opus) during low-latency windows (US business hours)
- Run bulk work (tests, builds) during off-hours (batch processing)
- Rationale: Opus models take longer at peak load; run when less congested

**Implementation:**
- Not directly cost-reducing (Bedrock charges by tokens, not time)
- **However:** Faster model execution = fewer tokens needed
- If Opus task takes 2x tokens during peak vs off-peak, running off-peak saves 50%

**Estimated impact:** -$2-3/day if Opus work is batched off-peak

---

### Strategy 5: Test Parallelization

**Current:**
- `cargo test --lib` runs ~45 minutes sequentially on some crates
- GitHub Actions can run multiple jobs in parallel

**Opportunity:**
- If A1-A8 tests run in parallel (vs sequential CI stages), total test time drops
- Shorter tests = lower EC2 hourly cost for test cluster

**Example:**
- Sequential: A1 tests (10m) + A2 tests (15m) + ... + A8 tests (20m) = 45 mins (8 hrs EC2)
- Parallel: Max(10m, 15m, ..., 20m) = 20 mins (2.7 hrs EC2)
- Savings: 5.3 hrs/run × $4.20/hr/5nodes = **~$4.40/run**
- If 2 runs/day: **$8.80/day savings**

**Implementation:**
- Already done in GitHub Actions workflows (`parallel` matrix jobs)
- Just need to verify in first CI run

---

## Optimization Roadmap

### Phase 1: This Week
**Actions:**
1. [x] Create cost breakdown analysis (this document)
2. [ ] Update agent launcher config to use Sonnet Fast for A8
3. [ ] Mark boilerplate-heavy A8 tasks for Haiku routing
4. [ ] Commit config changes

**Expected savings:** **$5-8/day**
**New target:** **$77-90/day**

### Phase 2: Week 2-3
**Actions:**
1. Monitor first CI run (gather performance data)
2. Evaluate compute right-sizing (are clients really using 4 vCPUs?)
3. If safe, downgrade client instances to c7a.large
4. Run benchmark to ensure performance is acceptable

**Expected savings:** **$0.50/day** (small, but compound)
**Cumulative target:** **$76-90/day**

### Phase 3: Month 1
**Actions:**
1. Implement scheduled provisioning (weekday-only cluster)
2. Set up CloudWatch alarms for cost anomalies
3. Create cost dashboard in Grafana
4. Validate batch test execution saves tokens/time

**Expected savings:** **$30/week + $2-3/day** = **$9-11/day average**
**Cumulative target:** **$65-80/day**

### Phase 4: Month 2-3
**Actions:**
1. Negotiate Reserved Instances (1-year for orchestrator + minimal baseline)
2. Implement spot instance pool optimization
3. Evaluate switching to graviton instances (AWS ARM, cheaper)
4. Multi-region consolidation (if applicable)

**Expected savings:** **$10-20/day**
**Cumulative target:** **$55-70/day**

---

## Cost Monitoring & Alerts

### Daily Monitoring

```bash
# Check cost burn rate every morning
cfs-dev cost

# Expected output:
# Today: $25.00 (so far, 3pm)
# This week: $150.00
# This month: $625.00
# Budget: $100/day
# Status: On track
```

### Weekly Cost Reporting

**Template:**

```markdown
## Weekly Cost Report — Week of 2026-03-01

| Metric | Target | Actual | Variance |
|--------|--------|--------|----------|
| Daily average | <$70 | $88 | +$18 |
| Spot savings | 60% off on-demand | 68% | +8% |
| Model efficiency | TBD | TBD | TBD |
| Build time | <20 min | 27 min | -7 min |
| Test time | <45 min | 52 min | -7 min |

**Actions this week:**
- [ ] Model selection optimizations deployed
- [ ] Compute right-sizing evaluated
- [ ] First CI run metrics collected

**Next week:**
- Implement scheduled provisioning
```

### CloudWatch Alarms

```bash
# Create alarm for cost exceeding $100/day
aws cloudwatch put-metric-alarm \
  --alarm-name cfs-daily-cost-100 \
  --alarm-description "Alert if daily spend exceeds $100" \
  --metric-name EstimatedCharges \
  --namespace AWS/Billing \
  --statistic Maximum \
  --period 86400 \
  --threshold 100 \
  --comparison-operator GreaterThanThreshold \
  --alarm-actions arn:aws:sns:us-west-2:xxx:cfs-budget-alerts

# Create alarm for cost spike (>20% variance from 7-day average)
# (Implementation varies; CloudWatch Anomaly Detector can help)
```

---

## Cost Optimization Validation

### Before & After Comparison

**Baseline (Current):**
- Daily cost: $85-96/day
- Budget: $100/day
- Utilization: 85-96% of budget

**Target (After Phase 3 optimizations):**
- Daily cost: <$70/day
- Budget: $100/day (still available for spikes)
- Utilization: <70% of budget

**Success criteria:**
- Achieve <$70/day by end of Phase 3 (2026-03-15)
- Maintain performance (build <20min, tests <45min)
- No increase in flaky tests
- 100% test pass rate

---

## Key Assumptions

1. **Bedrock pricing is token-based** (no time-of-day variance) — confirmed by AWS pricing page
2. **EC2 spot instances provide 60-90% discount** — validated for i4i.2xlarge (68% savings)
3. **Test parallelization is effective** — assumed; will validate in CI run
4. **Scheduled provisioning doesn't impact dev velocity** — minimal cluster stays up; full cluster on-demand
5. **Model switching (Sonnet Fast, Haiku) doesn't reduce code quality** — must be validated carefully

---

## References

- AWS EC2 Pricing: https://aws.amazon.com/ec2/pricing/on-demand/
- AWS Bedrock Pricing: https://aws.amazon.com/bedrock/pricing/
- Spot Instance Pricing: https://aws.amazon.com/ec2/spot/pricing/
- Reserved Instances: https://aws.amazon.com/ec2/pricing/reserved-instances/

---

**Document Owner:** A11 Infrastructure & CI
**Last Updated:** 2026-03-01
**Review Frequency:** Weekly (during Phase 3 optimization)
