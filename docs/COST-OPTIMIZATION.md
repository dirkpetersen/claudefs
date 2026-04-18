# ClaudeFS Cost Optimization Guide

**Version:** 1.0
**Last Updated:** 2026-04-18
**Target Audience:** Infrastructure engineers, developers, cost analysts

---

## Table of Contents

1. [Overview](#overview)
2. [Cost Structure](#cost-structure)
3. [Budget Enforcement](#budget-enforcement)
4. [Spot Instance Strategy](#spot-instance-strategy)
5. [Reserved Instance Analysis](#reserved-instance-analysis)
6. [Cost Attribution](#cost-attribution)
7. [Cost Monitoring](#cost-monitoring)
8. [Optimization Tips](#optimization-tips)
9. [Troubleshooting](#troubleshooting)
10. [FAQ](#faq)

---

## Overview

ClaudeFS runs on AWS with two primary cost components:

- **EC2 Compute** (~30-40% of budget) — Storage, metadata, FUSE, gateway nodes
- **Bedrock LLM** (~60-70% of budget) — Claude Opus/Sonnet/Haiku for development agents

**Daily Budget:** $50 USD
**Monthly Budget:** ~$1,500 USD (30 × $50)
**Annual Budget:** ~$18,000 USD

The cost monitoring system tracks, attributes, and enforces budgets across all resources.

### Key Numbers

| Item | Cost | Notes |
|------|------|-------|
| Daily Budget | $50 | Hard limit across all services |
| EC2 Allocation | $25 | Typical allocation for 10-node cluster |
| Bedrock Allocation | $25 | Typical allocation for 5-7 agents |
| Monthly Budget | $1,500 | 30 × $50 daily limit |
| Spot Discount | 60-70% | Off on-demand pricing |
| Monthly Bedrock (Opus only) | $1,200+ | Reduced with model optimization |
| Monthly Bedrock (Sonnet mix) | $900-1,000 | With model downgrading |

---

## Cost Structure

### By Service (Typical Daily Breakdown)

**EC2 (~$20-25/day)**
- 5x i4i.2xlarge storage servers (spot): ~$12/day
- 1x c7a.2xlarge orchestrator (on-demand): ~$3/day
- 2x c7a.xlarge clients (spot): ~$2/day
- 1x t3.medium cloud conduit (spot): ~$0.15/day
- 1x c7a.xlarge Jepsen (spot): ~$1.50/day

**Bedrock (~$20-30/day)**
- A1 (Opus): ~$4/day
- A2 (Opus): ~$4/day
- A3 (Sonnet): ~$2/day
- A4 (Opus): ~$4/day
- A5 (Sonnet): ~$2/day
- A6 (Sonnet): ~$1/day
- A7 (Sonnet): ~$2/day
- A8 (Haiku): ~$0.50/day
- A9 (Sonnet): ~$2/day
- A10 (Opus): ~$4/day
- A11 (Haiku): ~$0.50/day

**Total:** ~$45-55/day (within $50 limit)

### Spot Instance Savings

All test cluster instances use spot pricing:

| Instance Type | On-Demand/hr | Spot/hr | Discount | Monthly Savings |
|---------------|-------------|---------|----------|-----------------|
| i4i.2xlarge | $1.104 | $0.442 | 60% | $988/node |
| c7a.2xlarge | $0.378 | $0.151 | 60% | $332/node |
| c7a.xlarge | $0.189 | $0.076 | 60% | $166/node |
| t3.medium | $0.046 | $0.014 | 70% | $30/node |

**5-node storage cluster on spot:** ~$6,000/month savings vs on-demand

---

## Budget Enforcement

### Alert Thresholds

The cost monitoring system sends alerts at these budget utilization levels:

| Threshold | Budget | Action | Timing |
|-----------|--------|--------|--------|
| 25% | $12.50 | Information alert | SNS notification |
| 50% | $25 | Warning alert | SNS notification |
| 75% | $37.50 | Critical alert | SNS + recommendation to shut down |
| 100% | $50 | Hard limit | Auto-terminate spot instances + Haiku-only mode |

### Automatic Actions at 100%

When daily spend exceeds $50:

1. **All spot instances are terminated** — EC2 nodes shut down
2. **Bedrock budget exceeded flag** — All agents switch to Haiku-only mode
3. **SNS alert** — Notification sent to ops team
4. **Agent sessions** — Supervisory system relaunches agents with Haiku

### Budget Reset

- Alert flags reset at midnight UTC
- New daily budget begins at 00:00 UTC
- Haiku-only mode lasts until next calendar day (if budget allows)

---

## Spot Instance Strategy

### Why Spot for Development

**Advantages:**
- 60-70% discount vs on-demand pricing
- Savings: $1,500-2,000/month for test cluster
- Acceptable for non-production testing
- Quick provisioning (seconds vs minutes)
- Automatic scaling capabilities

**Risks:**
- Instance interruption (EC2 reclaims capacity)
- Typical lifetime: hours to days
- No SLA on availability

**Mitigation:**
- Tear down cluster when not actively testing
- Automatic re-provisioning via watchdog
- Cluster state stored in S3 (recoverable)
- No data loss on interruption (stateless nodes)

### Spot Instance Lifecycle

```
08:00 UTC  → cfs-dev up (provision cluster)
08:30 UTC  → Agents start work
16:00 UTC  → Testing complete
16:30 UTC  → cfs-dev down (tear down cluster)
             Saves ~8 × $20/day = $160/day when idle
```

### Cost Optimization: Always Tear Down

**Cost Comparison:**

| Scenario | 24h Cost | 30-Day Cost | Annual Cost |
|----------|----------|------------|-------------|
| Cluster running 24/7 | $50 | $1,500 | $18,000 |
| Cluster 10 hrs/day (8am-6pm) | $17 | $500 | $6,000 |
| Cluster 5 hrs/day (testing only) | $8.50 | $250 | $3,000 |
| Cluster shutdown when idle | Minimal | $500-750 | $6,000-9,000 |

**Recommendation:** Tear down after each testing session. Cost savings: 50-70%.

---

## Reserved Instance Analysis

### When to Use Reserved Instances

**For Production:**
- 1-year all-upfront RI: **45-60% off on-demand**
- 3-year all-upfront RI: **60-70% off on-demand**
- Breakeven: 2-3 months of continuous operation
- Ideal for: Always-on production infrastructure

**For Development:**
- **Not recommended** — cluster is ephemeral
- Spend: typically $6,000-9,000/year (spot-based)
- Breakeven point: 12-18 months of always-on usage
- Better to use spot and tear down when idle

### RI Pricing Examples

**i4i.2xlarge in us-west-2:**

| Type | Hourly Rate | Annual (8,760 hrs) | Monthly |
|------|------------|------------------|---------|
| On-Demand | $1.104 | $9,669 | $807 |
| 1-Year RI | $0.612 | $5,361 | $447 |
| 3-Year RI | $0.497 | $4,353 | $363 |
| Spot (avg) | $0.442 | $3,871 | $323 |

**Breakeven Analysis (vs On-Demand):**
- 1-Year RI: Saves $4,308/year, breaks even at 7 months
- 3-Year RI: Saves $5,316/year, breaks even at 9 months
- Spot: Saves $5,798/year, no upfront cost

---

## Cost Attribution

### Tagging Strategy

All resources are tagged for cost allocation:

```
project              = claudefs
environment          = development
deployment-stage     = {canary|10pct|50pct|100pct}
agent                = {A1|A2|...|A11}
test-suite           = {posix|jepsen|fio|chaos|general}
cost-center          = engineering
```

### Per-Stage Cost Tracking

The cost tagger splits expenses by deployment stage:

```bash
# Tag instances for canary stage
cfs-cost-tagger.sh tag-stage canary A11 posix

# Generate cost report by stage
cfs-cost-tagger.sh report-stages /var/lib/cfs-cost-reports/
```

**Expected Cost Breakdown:**
- Canary (1 node): $3-5/day
- 10% rollout (2 nodes): $4-7/day
- 50% rollout (4 nodes): $10-15/day
- 100% rollout (10 nodes): $20-30/day

### Per-Agent Cost Attribution

The cfs-cost-tagger.sh can generate per-agent cost breakdowns:

```bash
cfs-cost-tagger.sh report-by-tag agent /tmp/cost-by-agent.txt
```

**Typical Agent Costs (Bedrock):**
- A1 (Storage, Opus): $4-5/day
- A2 (Metadata, Opus): $4-5/day
- A3 (Reduction, Sonnet): $2-3/day
- A4 (Transport, Opus): $4-5/day
- A5 (FUSE, Sonnet): $2-3/day
- A6 (Replication, Sonnet): $1-2/day
- A7 (Gateways, Sonnet): $2-3/day
- A8 (Management, Haiku): $0.50-1/day
- A9 (Tests, Sonnet): $2-3/day
- A10 (Security, Opus): $4-5/day
- A11 (Infra, Haiku): $0.50-1/day

**Total:** ~$27-33/day

---

## Cost Monitoring

### Running Reports

#### Generate Daily Report
```bash
# Generate all report types
generate-cost-report.sh all

# View HTML report in browser
open /var/lib/cfs-cost-reports/cost-report-$(date +%Y-%m-%d).html

# Export as CSV for spreadsheet analysis
generate-cost-report.sh export csv
```

#### Spot Instance Analysis
```bash
# Compare prices for a specific instance
spot-instance-analyzer.sh compare i4i.2xlarge

# Generate comprehensive recommendations
spot-instance-analyzer.sh analyze

# View savings report (JSON)
cat /var/lib/cfs-cost-reports/spot-savings-report.json
```

#### Cost Attribution
```bash
# Tag instances for a deployment
cfs-cost-tagger.sh tag-stage 100pct A11 jepsen

# Generate cost report by stage
cfs-cost-tagger.sh report-stages /var/lib/cfs-cost-reports/

# List all tagged instances
cfs-cost-tagger.sh list-instances
```

### Grafana Dashboard

Access the cost monitoring dashboard:

1. **URL:** `http://<grafana-instance>:3000/d/cfs-cost-monitoring`
2. **Data Source:** CloudWatch (CFS/Cost namespace)
3. **Refresh Rate:** 30 seconds
4. **Time Range:** Last 7 days (configurable)

**Dashboard Sections:**
- Daily cost summary gauge
- Service breakdown pie chart
- 7-day trend with budget gauge
- Budget percentage for EC2, Bedrock, and Total
- Cost forecast (7-day, 30-day)
- Budget remaining trend

### CloudWatch Metrics

The cost monitor publishes these CloudWatch metrics every 15 minutes:

```
CFS/Cost/DailyCost/EC2             (USD)
CFS/Cost/DailyCost/Bedrock         (USD)
CFS/Cost/DailyCost/S3              (USD)
CFS/Cost/DailyCost/DataTransfer    (USD)
CFS/Cost/DailyCost/Secrets         (USD)
CFS/Cost/DailyCost/Monitoring      (USD)
CFS/Cost/DailyCost/Total           (USD)
CFS/Cost/BudgetPercentage/EC2      (%)
CFS/Cost/BudgetPercentage/Bedrock  (%)
CFS/Cost/BudgetPercentage/Total    (%)
CFS/Cost/BudgetRemaining           (USD)
CFS/Cost/Forecast/7Day             (USD)
CFS/Cost/Forecast/30Day            (USD)
```

Query examples:
```
# Average daily EC2 cost (7 days)
SELECT AVG(DailyCost_EC2) FROM "CFS/Cost" OVER 7d

# Budget remaining (current)
SELECT LATEST(BudgetRemaining) FROM "CFS/Cost"

# 7-day forecast trend
SELECT Forecast_7Day FROM "CFS/Cost" OVER 30d
```

---

## Optimization Tips

### For Developers

1. **Tear down cluster when testing is complete**
   - Cost: $50/day running, $0/day idle
   - Savings: $1,500/month = 50% of budget

2. **Batch test runs**
   - Instead of: 10 × 30-min test runs = 5 hours provisioning
   - Better: 1 × 5-hour test run = 1 hour provisioning
   - Savings: 80% of provisioning costs

3. **Use local testing first**
   - Developers: Use local build + tests before committing
   - CI: Run fast smoke tests (10 min) before cluster tests
   - Cluster: Run full suite (1 hour) only on validated changes

### For Infrastructure

1. **Monitor budget daily**
   - Set phone reminder at 50% threshold
   - Review Grafana dashboard each morning
   - Check email alerts for 75% and 100% thresholds

2. **Optimize model selection**
   - Current: Opus (A1, A2, A4, A10) = 50% of Bedrock cost
   - Option: Downgrade to Sonnet where possible
   - Savings: $2-3/day = $60-90/month

3. **Use spot instances exclusively**
   - Current: All 10 nodes on spot (correct)
   - Savings: $1,500-2,000/month vs on-demand
   - Acceptable risk: Provisioning is automated

4. **Archive old cost data**
   - CSV exports older than 90 days → S3 Glacier
   - Cost: $0.004/GB/month in Glacier (vs $0.023 in S3)
   - Example: 1 year of cost data = 100 MB → $0.40/month in Glacier

### For Cost Analysis

1. **Review weekly trends**
   ```bash
   tail -7 /var/lib/cfs-cost-reports/cost-history-*.csv | \
     awk -F, '{sum += $NF} END {print "Weekly avg: $" sum/7}'
   ```

2. **Compare agent costs**
   ```bash
   cfs-cost-tagger.sh report-by-tag agent /tmp/report.txt
   grep -E "^  (A[0-9]|A1[01]):" /tmp/report.txt | sort -k3 -rn
   ```

3. **Track spot savings**
   ```bash
   spot-instance-analyzer.sh savings
   cat /var/lib/cfs-cost-reports/spot-savings-report.json | jq '.instances[]'
   ```

---

## Troubleshooting

### Costs Exceeding Budget

**Problem:** Daily cost exceeds $50

**Diagnosis:**
1. Check which service exceeded budget
   ```bash
   cat /var/lib/cfs-cost-reports/cost-summary-today.json | jq '.summary'
   ```

2. Identify top cost drivers
   ```bash
   generate-cost-report.sh daily | grep -E '"[A-Z]+": [0-9]'
   ```

3. Check Bedrock spend
   ```bash
   aws ce get-cost-and-usage \
     --time-period "Start=$(date +%Y-%m-%d),End=$(date -d +1d +%Y-%m-%d)" \
     --granularity DAILY \
     --metrics UnblendedCost \
     --filter '{"Dimensions":{"Key":"SERVICE","Values":["Amazon Bedrock"]}}' \
     --query 'ResultsByTime[0].Total.UnblendedCost.Amount' \
     --output text
   ```

**Solutions:**
- **EC2 exceeding:** Check cluster still running? Tear down with `cfs-dev down`
- **Bedrock exceeding:** Too many agents active? Check with `cfs-dev status`
- **Both exceeding:** Check if test cluster provisioned unexpectedly? Review logs

### Cost Monitor Not Running

**Problem:** CloudWatch metrics not updating

**Diagnosis:**
1. Check cron job running
   ```bash
   sudo systemctl status cron
   sudo grep cfs-cost-monitor /var/log/syslog | tail -10
   ```

2. Check script permissions
   ```bash
   ls -la /home/cfs/claudefs/tools/cfs-cost-monitor-enhanced.sh
   ```

3. Check AWS credentials
   ```bash
   aws sts get-caller-identity
   aws cloudwatch list-metrics --namespace CFS/Cost
   ```

**Solutions:**
- Restart cron: `sudo systemctl restart cron`
- Re-export AWS credentials: `source ~/.bashrc`
- Check IAM permissions: CloudWatch PutMetricData + Cost Explorer

### Spot Instances Not Terminating at 100%

**Problem:** Budget exceeded but instances still running

**Diagnosis:**
1. Check cost monitor logs
   ```bash
   tail -50 /var/log/cfs-cost-monitor-enhanced.log | grep -i "budget\|termina"
   ```

2. Check if flag was set
   ```bash
   ls -la /tmp/cfs-bedrock-budget-exceeded
   ```

3. Verify AWS CLI access
   ```bash
   aws ec2 describe-instances --filters Name=tag:project,Values=claudefs
   ```

**Solutions:**
- Manually terminate instances: `cfs-dev down`
- Check IAM permissions: EC2 TerminateInstances
- Verify cost monitor has correct EC2 region

---

## FAQ

### Q: Why is Bedrock so expensive?

**A:** Claude models are state-of-the-art but expensive. The cost reflects:
- Opus: $0.003/1K input tokens, $0.012/1K output tokens (most expensive)
- Sonnet: $0.003/1K input tokens, $0.012/1K output tokens (same as Opus, confusing!)
- Haiku: $0.00025/1K input tokens, $0.00125/1K output tokens (25x cheaper)

With 11 agents running 8-16 hours/day, token usage is very high. Solution: Downgrade agents to Sonnet or Haiku where possible.

### Q: Can we use reserved instances?

**A:** For development/test: **No.** Spend is $6,000-9,000/year (too low for RI).
For production (future): **Yes.** 1-year all-upfront RI saves 45-60% vs on-demand.

### Q: What about data transfer costs?

**A:** Minimal ($0-1/day). All data stays within us-west-2. S3 transfers are free within region.

### Q: How do I reduce costs?

**A:** In priority order:
1. **Tear down cluster when not testing** (saves 50%)
2. **Downgrade agent models** (Opus→Sonnet saves 20-30%)
3. **Batch test runs** (reduces provisioning overhead)
4. **Archive old data** (S3→Glacier saves 80%)

### Q: What's the monthly cost projection?

**A:** Assuming typical usage (10 hrs/day development):
- EC2 (spot): $250-300
- Bedrock: $450-550
- Other: $50
- **Total: $750-900/month**

Vs on-demand: ~$2,200/month (2.5x more expensive)

### Q: Can we negotiate with AWS?

**A:** Probably not at current spend levels (<$20K/year). AWS Enterprise Discount Program typically kicks in at $50K+/year.

### Q: How do I add more monitoring?

**A:** The cost monitoring system is extensible:
1. **Add CloudWatch metrics:** Edit cfs-cost-monitor-enhanced.sh
2. **Add Grafana panels:** Edit tools/grafana-cost-dashboard.json
3. **Add cost tags:** Edit cfs-cost-tagger.sh

### Q: What if we exceed annual budget?

**A:** The cost monitor enforces a daily $50 limit, which automatically prevents annual budget overrun:
- $50/day × 365 days = $18,250/year
- Hard stop at daily limit ensures compliance

---

## References

- [Production Deployment Guide](DEPLOYMENT.md)
- [Infrastructure Overview](agents.md)
- [Architecture Decisions](decisions.md)
- [Cost Optimization Phase 1](A11-COST-OPTIMIZATION-PHASE1.md)

---

**Document Version:** 1.0
**Last Updated:** 2026-04-18
**Next Review:** 2026-05-18
**Owner:** A11 Infrastructure & CI
