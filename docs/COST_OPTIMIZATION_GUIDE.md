# ClaudeFS Cost Optimization Guide

**Last Updated:** 2026-03-04
**Daily Budget:** $100/day
**Current Spend:** ~$80-96/day
**Margin:** $4-20/day buffer

## Executive Summary

The ClaudeFS development cluster operates on a **strict $100/day AWS + Bedrock budget**. This guide details cost drivers, optimization strategies, and monitoring techniques to stay within budget while maintaining full development velocity across 11 parallel agents.

## Cost Breakdown (2026-03-04)

### AWS Services

| Service | Instance Type | Count | Hours/Day | Unit Cost | Daily Cost |
|---------|---------------|-------|-----------|-----------|------------|
| **EC2 Orchestrator** | `c7a.2xlarge` on-demand | 1 | 24 | $0.41/hr | **$10.00** |
| **EC2 Storage Nodes** | `i4i.2xlarge` spot | 5 | 8 | $0.29/hr | **$11.60** |
| **EC2 FUSE Client** | `c7a.xlarge` spot | 1 | 8 | $0.102/hr | **$0.82** |
| **EC2 NFS Client** | `c7a.xlarge` spot | 1 | 8 | $0.102/hr | **$0.82** |
| **EC2 Cloud Conduit** | `t3.medium` spot | 1 | 8 | $0.019/hr | **$0.15** |
| **EC2 Jepsen Controller** | `c7a.xlarge` spot | 1 | 8 | $0.102/hr | **$0.82** |
| **Secrets Manager** | N/A | 2 secrets | N/A | $0.015/day | **$0.03** |
| **CloudWatch Logs** | (storage + ingestion) | N/A | N/A | Minimal | **~$0.50** |
| **EC2 Subtotal** | | | | | **~$24/day** |

### Bedrock Services

| Model | Agents | Avg Tokens/Day | Cost/Token | Daily Cost |
|-------|--------|----------------|-----------|------------|
| **Opus** | A10 (Security Audit) | 2.5M | $0.025/1k | **$62.50** |
| **Sonnet** | A1-A9 (builders) | 2M | $0.005/1k | **$10.00** |
| **Haiku** | A8, A11 (boilerplate) | 500k | $0.0004/1k | **$0.20** |
| **Bedrock Subtotal** | | | | **~$72.70** |

### Grand Total

| Category | Daily | Notes |
|----------|-------|-------|
| AWS (EC2, Secrets, CloudWatch) | $25 | Relatively stable |
| Bedrock (Claude models) | $73 | **Largest cost driver** |
| **Total** | **$98** | ~$0.2-1 variance day-to-day |

---

## Cost Drivers & Opportunities

### #1: Bedrock/Claude Model Selection (LARGEST OPPORTUNITY)

**Current:** Opus, Sonnet, Haiku mix → **~$73/day**

| Model | Token Cost | Speed | Accuracy | Agent |
|-------|-----------|-------|----------|-------|
| **Opus** | High ($0.025/1k) | Slow | Best (reasoning) | A10 only |
| **Sonnet** | Medium ($0.005/1k) | Medium | Good (implementations) | A1-A9 |
| **Haiku** | Low ($0.0004/1k) | Fast | Basic | A8, A11 |

**Optimization Strategy:**
- **Reserve Opus for A10 only** (Security Audit, deep reasoning)
- **Keep Sonnet for A1-A7** (core implementation agents)
- **Switch A8 (Mgmt) to Haiku** for boilerplate UI code (saves ~$8/day)
- **Switch A9 (Test) to Sonnet Fast** or Haiku for test harnesses (saves ~$3/day)
- **Keep A11 (Infrastructure) on Haiku** (already optimized)

**Estimated Savings:** $11/day → **New Bedrock subtotal: ~$62/day**

### #2: Spot Instance Runtime Optimization

**Current:** 8 hours/day (9am-5pm UTC) → **$13/day for 5 test nodes**

**Options:**

1. **Reduce to 4 hours/day** (shorter test windows)
   - Run full integration tests only once per day
   - Reduces EC2 cost to ~$6.50/day
   - **Savings: $6.50/day** (but less testing)

2. **Scale test cluster dynamically**
   - 3 nodes for basic CI (always runs)
   - 5 nodes only for nightly/performance tests
   - Current setup: already pretty good

3. **Use cheaper instance types**
   - `i4i.2xlarge` → `i3.2xlarge` (cheaper SSD, older generation)
   - Not recommended: NVMe speed is critical for storage layer tests

**Recommendation:** Keep current 8-hour runtime for thorough testing. Spot instances are already the cheapest option.

### #3: Orchestrator Optimization

**Current:** `c7a.2xlarge` on-demand → **$10/day**

**Options:**
- Keep on-demand (required for always-available supervision)
- Could downgrade to `c7a.xlarge` → saves $5/day (but less headroom)
- **Recommendation:** Keep current sizing. Orchestrator needs headroom for watchdog/supervisor/cost-monitor runs + cargo builds.

### #4: GitHub Actions Caching

**Current:** Using `actions/cache@v4` aggressively

**Optimization:**
- ✅ Cache `~/.cargo/registry` (already done)
- ✅ Cache `~/.cargo/git` (already done)
- ✅ Cache `target/` (already done)
- ✅ Per-job cache keys based on Cargo.lock (already done)

**Status:** Already well-optimized. Reduces workflow time from ~60min to ~20min.

### #5: CloudWatch Logs (Minimal)

**Current:** ~$0.50/day (supervisor/watchdog logs)

**Optimization:**
- Logs stored locally on orchestrator
- No ingestion to CloudWatch (manual S3 archival if needed)
- **Status:** Already optimized. Consider only if archival/compliance needed.

### #6: Bedrock Budget Enforcement Flag

**Current:** When Bedrock spend exceeds $25/day threshold, cost monitor sets a flag, all agents forced to Haiku

**How it works:**
1. `cfs-cost-monitor.sh` checks Bedrock spend
2. If > $25, creates `/tmp/cfs-bedrock-budget-exceeded`
3. Agent launcher detects flag and forces all agents to Haiku
4. Flag resets at midnight when spend counter resets

**Recommendation:** Increase threshold to $35-40/day to give agents more headroom before budget cut.

---

## Recommended Cost Optimization (Phased)

### Phase 1: Immediate (Low Risk, High Impact)

**Action:** Switch A8 (Mgmt) to Haiku for boilerplate work

```bash
# Edit cfs-agent-launcher.sh
# Change A8 model from Sonnet to Haiku for UI/CLI boilerplate

# Files affected:
# - crates/claudefs-mgmt/src/ (React UI, CLI subcommands, Grafana JSON)
# Keep Sonnet for API design and integration with A2/A4
```

**Expected Savings:** $8/day → **New total: $90/day**

**Risk:** Low. Haiku is sufficient for UI boilerplate. Complex decisions (API design) still use Sonnet.

### Phase 2: Conditional (Medium Risk, Medium Impact)

**Action:** Make test cluster scale dynamic

```bash
# Modify cfs-dev or Terraform:
# - Basic suite (ci-build.yml): 3 nodes
# - Full suite (tests-all.yml + integration-tests.yml): 5-7 nodes
# - Nightly/performance: 10-20 nodes (for scaling benchmarks)
```

**Expected Savings:** $6/day (if basic suite runs 4 hrs, full suite 4 hrs)

**New total: $84/day**

**Risk:** Medium. Requires changing test infrastructure. Could reduce testing coverage if not done carefully.

### Phase 3: Advanced (High Risk, Lower Impact)

**Action:** Use reserved instances for orchestrator

```bash
# Current: $10/day on-demand
# Reserved: ~$5/day (for 1-year commitment)
# Savings: $5/day
# New total: $79/day
```

**Risk:** High. Requires AWS account admin to purchase reservations. Not flexible if requirements change.

**Timeline:** Do this only after 6 months of stable orchestrator usage.

---

## Cost Monitoring & Alerts

### Daily Cost Check (Developer)

```bash
# Quick check
aws ce get-cost-and-usage \
  --time-period Start=$(date +%Y-%m-%d),End=$(date -d "+1 day" +%Y-%m-%d) \
  --granularity DAILY --metrics UnblendedCost --region us-west-2 | \
  jq '.ResultsByTime[0].Total.UnblendedCost.Amount'

# Expected: $80-100 (warn if > $95)
```

### Hourly Trend (Infrastructure Engineer)

```bash
# Check if on track for daily budget
current_hour=$(date +%H)
current_cost=$(aws ce get-cost-and-usage \
  --time-period Start=$(date +%Y-%m-%d),End=$(date -d "+1 day" +%Y-%m-%d) \
  --granularity HOURLY --metrics UnblendedCost --region us-west-2 | \
  jq '.ResultsByTime[].Total.UnblendedCost.Amount | tonumber' | paste -sd+ | bc)

# Scale to 24 hours
projected=$((current_cost * 24 / current_hour))
echo "Current: $current_cost, Projected daily: $projected"

# Alert if projected > $110
```

### AWS Budget Alerts (Automated)

**Already configured:**
- AWS Budgets: `cfs-daily-100` with $100/day limit
- Alerts: 80% ($80) and 100% ($100)
- SNS topic: `cfs-budget-alerts`
- Cost monitor script: `/opt/cfs-cost-monitor.sh` (hard kill at $100)

### Bedrock Budget Tracking

**Manually track via:**
```bash
# Past 7 days by service
for i in {0..6}; do
  date=$(date -d "$i days ago" +%Y-%m-%d)
  echo "=== $date ==="
  aws ce get-cost-and-usage \
    --time-period Start=$date,End=$(date -d "$date +1 day" +%Y-%m-%d) \
    --granularity DAILY --metrics UnblendedCost \
    --group-by Type=DIMENSION,Key=SERVICE --region us-west-2 | \
    jq '.ResultsByTime[0].Groups[] | select(.Keys[0] == "Bedrock") | .Metrics.UnblendedCost'
done
```

---

## What NOT to Cut (Risks of Over-Optimization)

### ❌ Don't Reduce Spot Cluster Too Much
- Storage layer needs real NVMe testing
- Downgrading `i4i` to `i3` loses 3x throughput
- Fewer test nodes → less coverage

### ❌ Don't Switch Core Implementation Agents to Haiku
- A1-A7 do critical work (storage, metadata, transport, FUSE)
- Haiku lacks sufficient reasoning for complex design decisions
- Leads to bugs, rework, long-term cost increase

### ❌ Don't Disable Watchdog/Supervisor/Cost-Monitor
- These systems save thousands in failed deployments
- Remove them only if you have enterprise support

### ❌ Don't Move Orchestrator to Spot Instances
- Spot terminations would break the entire cluster
- Requires always-available supervision layer

---

## Budget Timeline & Milestones

| Date | Phase | Expected Daily Cost | Notes |
|------|-------|-------------------|-------|
| 2026-03-04 | Baseline | $98 | 11 agents, full test suite |
| 2026-03-11 | Phase 1 (A8→Haiku) | $90 | -$8/day |
| 2026-03-18 | Phase 2 (dynamic cluster) | $84 | -$6/day |
| 2026-04-01 | Review | $84 | Assess additional savings |
| 2026-06-01 | Reserved instances | $79 | -$5/day (if committed) |

**Target for Q2 2026:** $70-75/day (with 20-25% buffer for spikes)

---

## Quarterly Cost Forecast

### Q1 2026 (Jan-Mar): Development Phase

| Expense | Monthly | Notes |
|---------|---------|-------|
| AWS | $750 | 25 × 30 days |
| Bedrock | $2,190 | 73 × 30 days |
| **Subtotal** | **$2,940** | ~$98/day |
| Headroom (20%) | ~$588 | For spike days |
| **Total Q1** | **$3,528** | |

### Q2 2026 (Apr-Jun): Optimization Phase (with Phase 1-2 improvements)

| Expense | Monthly | Notes |
|---------|---------|-------|
| AWS | $600 | 20 × 30 days (Phase 2 savings) |
| Bedrock | $1,860 | 62 × 30 days (Phase 1 savings) |
| **Subtotal** | **$2,460** | ~$82/day |
| Headroom (20%) | ~$492 | For spike days |
| **Total Q2** | **$2,952** | |

### Annual Forecast

- **Year 1 total:** ~$33,000 (18 months estimated)
- **Monthly average:** ~$1,800 (after optimization)

---

## Cost Comparison vs Competitors

| Solution | Setup Cost | Monthly Ops | Agents |
|----------|-----------|-----------|--------|
| **ClaudeFS (us)** | $0 | $1,800 | 11 parallel agents |
| **Manual Dev (1 person)** | $0 | $10,000+ (salary) | 1 person |
| **AWS CodeBuild (500 min/mo)** | $0 | $200 | Limited to CI only |
| **Cirrus CI (free tier)** | $0 | $0 | Limited compute, 1-2 jobs |

**ClaudeFS advantage:** $1,800/month for 11 parallel AI agents = $164/agent/month. Compare to $5,000+/month for a human engineer.

---

## Emergency Cost Control

### If Daily Spend Exceeds $100

1. **Automatic:** Cost monitor kills all spot instances (5 min)
2. **Manual:**
   ```bash
   # Kill all spot instances
   aws ec2 terminate-instances \
     --instance-ids $(aws ec2 describe-instances \
       --filters "Name=instance-lifecycle,Values=spot" "Name=instance-state-name,Values=running" \
       --query 'Reservations[].Instances[].InstanceId' --output text) \
     --region us-west-2
   ```

3. **Bedrock budget:** If Bedrock > $25, all agents forced to Haiku (automatic)

4. **Reduce agents:** Disable A9/A10 temporarily (reduces load significantly)

---

## Recommendations for Next Review

- [ ] Implement Phase 1 (A8→Haiku) by 2026-03-11
- [ ] Collect 2 weeks of data on new cost structure
- [ ] Review Phase 2 (dynamic cluster) feasibility
- [ ] Set quarterly budget review meetings
- [ ] Consider cost center allocation if multiple projects use cluster

---

## References

- AWS EC2 pricing: https://aws.amazon.com/ec2/pricing/on-demand/
- AWS Spot pricing: https://aws.amazon.com/ec2/spot/pricing/
- Anthropic Claude pricing: https://www.anthropic.com/pricing
- AWS Budgets: https://docs.aws.amazon.com/awsbudgets/latest/userguide/
- Cost Explorer: https://docs.aws.amazon.com/awscostmanagement/latest/userguide/ce-what-is.html

---

**Document Version:** 1.0
**Last Updated:** 2026-03-04
**Next Review:** 2026-03-11
**Owner:** A11 (Infrastructure & CI)
