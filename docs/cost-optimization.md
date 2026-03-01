# ClaudeFS Cost Optimization Guide

Strategies for optimizing AWS infrastructure costs while maintaining Phase 2 performance targets.

## Cost Baseline (Phase 2)

### Budget Breakdown

| Component | Type | Daily Cost | Monthly Cost | Annual Cost |
|-----------|------|-----------|------------|------------|
| Orchestrator | c7a.2xlarge (on-demand) | $8.40 | $252 | $3,066 |
| Storage (5x spot) | i4i.2xlarge (spot) | $5.76 | $173 | $2,105 |
| Clients/Jepsen (4x spot) | c7a.xlarge (spot) | $1.63 | $49 | $595 |
| Conduit (1x spot) | t3.medium (spot) | $0.15 | $4.50 | $55 |
| **EC2 Subtotal** | | **$15.94/day** | **$479** | **$5,821** |
| Bedrock (API calls) | Opus/Sonnet/Haiku | $55-70 | $1,650-2,100 | $20,000-25,000 |
| Secrets Manager | 2 secrets | $0.03 | $1 | $12 |
| **Grand Total** | | **~$71-86/day** | **~$2,130-2,580** | **~$25,833-31,833** |

**AWS Budget Limit:** $100/day (cost monitor auto-terminates at this point)

## Cost Optimization Strategies

### 1. Spot Instance Strategy (70% savings)

**Current:** All test nodes use spot instances.

**Optimization tactics:**

```bash
# Monitor spot price trends
aws ec2 describe-spot-price-history \
  --product-descriptions "Linux/UNIX" \
  --instance-type i4i.2xlarge \
  --region us-west-2 \
  --start-time $(date -u -d '30 days ago' +%Y-%m-%dT%H:%M:%S) \
  --query 'SpotPriceHistory[*].[Timestamp,SpotPrice]' \
  --output table
```

**Tactics:**

1. **Set max prices intelligently**
   ```terraform
   # In terraform.tfvars
   spot_max_price = "0.50"  # Conservative price point
   # Typical i4i.2xlarge on-demand: $0.70/hr
   # Spot average: $0.20/hr (70% discount)
   # Set max at 0.50 for good availability + cost control
   ```

2. **Use multiple instance types** (if acceptable)
   ```bash
   # Instead of only i4i.2xlarge, allow i4i.xlarge or i3.2xlarge
   # More options = higher spot availability
   ```

3. **Distribute across availability zones**
   ```terraform
   # Spread nodes across 2-3 AZs to reduce termination risk
   # Spot termination risk per AZ: ~5% per day → ~<1% if spread
   ```

4. **Use Reserved Instances for orchestrator** (on-demand)
   ```bash
   # Orchestrator runs 24/7, so reserve it
   aws ec2 purchase-reserved-instances \
     --reserved-instances-offering-id <offering-id> \
     --instance-count 1
   # 1-year commitment: ~35% discount
   # 3-year commitment: ~60% discount
   ```

### 2. Compute Optimization

**Rightsize instances for workload:**

#### Orchestrator (always running)
- **Current:** c7a.2xlarge (8 vCPU, 16 GB, $0.35/hr)
- **Optimization:** c7a.xlarge (4 vCPU, 8 GB, $0.175/hr) — **50% savings**
  - Trade-off: Slower agent execution, longer CI/CD, slower OpenCode runs
  - Recommendation: Keep 2xlarge for faster iteration (developer experience > $5/day savings)

#### Storage Nodes
- **Current:** i4i.2xlarge (8 vCPU, 64 GB, NVMe local)
- **Alternatives:**
  - i4i.xlarge (4 vCPU, 32 GB) — **35% cheaper**, but 50% less NVMe
  - i3.2xlarge (8 vCPU, 60 GB, NVMe) — ~same price as i4i.2xlarge
  - **Recommendation:** Accept current sizing for IOPS testing (Phase 2)

#### Test Clients
- **Current:** c7a.xlarge (4 vCPU, 8 GB, $0.17/hr spot)
- **Optimization:** t3.xlarge (4 vCPU, 16 GB, $0.14/hr spot) — **20% cheaper**
  - T3 is burstable (good for intermittent tests)
  - Trade-off: Lower baseline performance (acceptable for test runners)

### 3. Storage Optimization

**EBS volumes (currently all gp3):**

```bash
# Check actual volume usage
aws ec2 describe-volumes --region us-west-2 \
  --query 'Volumes[*].[VolumeId,Size,State]' --output table
```

**Optimization:**

1. **Downsize root volumes**
   ```terraform
   # Change from 50 GB to 30 GB (saves ~$1/month per node)
   root_block_device {
     volume_size = 30  # Down from 50
   }
   ```

2. **Use st1 (throughput optimized) for sequential I/O**
   - Not applicable (NVMe is for random I/O)

3. **Enable EBS-optimized** (improves performance per $)
   - Most recent instances are EBS-optimized by default

4. **Snapshot old volumes periodically**
   ```bash
   # Snapshots are cheaper than keeping volumes
   aws ec2 create-snapshot --volume-id <vol-id> \
     --description "Backup before teardown"
   ```

### 4. Network Optimization

**Data transfer costs:**

- **Within AZ:** Free (0 cost)
- **Cross-AZ:** $0.01/GB (significant for replication)
- **To Internet/S3:** $0.02/GB

**Optimization:**

1. **Keep all nodes in same AZ**
   ```terraform
   # Deploy all to us-west-2a (lowest latency, zero cross-AZ costs)
   # Trade-off: Less fault tolerance (AZ failure = total loss)
   # Acceptable for Phase 2 dev/test
   ```

2. **Use VPC endpoints for S3**
   ```bash
   # When S3 tiering goes live, use S3 VPC endpoint
   # Avoids data transfer costs through Internet Gateway
   ```

3. **Batch large transfers**
   - Combine 100 small uploads into 1 large upload (same cost, less latency)

### 5. Monitoring Cost

**Prometheus + Grafana running 24/7:**

| Component | Cost |
|-----------|------|
| Prometheus server (compute) | $0.10/day (included in orchestrator) |
| Prometheus storage (EBS) | $0.05/day (included in orchestrator) |
| Grafana (compute) | $0.05/day (included in orchestrator) |
| **Total** | **~$0.20/day** (negligible) |

**Optional cost reductions:**

1. **Reduce Prometheus retention**
   ```yaml
   storage:
     tsdb:
       retention:
         time: 3d  # Down from 15d (saves storage costs)
   ```

2. **Disable metrics during idle hours**
   ```bash
   # Stop Prometheus at 6 PM, restart at 8 AM
   0 18 * * 1-5 systemctl stop prometheus
   0 08 * * 1-5 systemctl start prometheus
   ```

### 6. Development Cluster Lifecycle

**Current:** Orchestrator always running.

**Recommendations:**

1. **Tear down test cluster when not in use**
   ```bash
   cfs-dev down  # Keeps orchestrator, terminates spot instances
   # Saves: ~$8/day (spots only), active 8 hrs/day = $4 savings
   ```

2. **Schedule cluster provisioning**
   ```bash
   # Start at 8 AM, tear down at 6 PM
   0 8 * * 1-5 cfs-dev up --phase 2    # Start cluster
   0 18 * * 1-5 cfs-dev down          # Stop cluster
   # Saves: ~$8 * 16/24 = $5.33/day
   ```

3. **Use nightly integration testing**
   ```bash
   # Run full test suite once per day (2 AM UTC)
   # Provision cluster → run tests → teardown
   # Saves: ~$8/day by not running 24/7
   ```

### 7. Bedrock API Cost Optimization

**Agent execution costs (Bedrock):**

| Model | Tokens/Day | Cost/Day | Monthly |
|-------|-----------|----------|---------|
| Opus (A1, A2, A4, A10) | ~50M | $20-25 | $600-750 |
| Sonnet (A3-A9) | ~80M | $15-20 | $450-600 |
| Haiku (A11 infrastructure) | ~20M | $5-10 | $150-300 |
| **Total** | **~150M** | **$40-55** | **$1,200-1,650** |

**Optimizations:**

1. **Use cheaper models for boilerplate work**
   - Haiku for documentation, simple edits, boilerplate
   - Sonnet for implementation, testing
   - Opus only for architecture, complex bugs

2. **Batch API requests**
   - Group multiple small tasks into larger prompts (reduce overhead)

3. **Use caching for repeated prompts**
   - Anthropic prompt caching can reduce token usage by 90%

4. **Reduce token consumption**
   - Provide only necessary context in prompts
   - Use summary format instead of full code snippets

## Monthly Cost Scenarios

### Scenario 1: Full-Time Phase 2 Development

**Setup:** Orchestrator + full test cluster running 24/7

```
EC2:           $479/month (on-demand orchestrator + spot tests)
Bedrock:       $1,650/month (5 agents active)
Storage:       $20/month
Monitoring:    $10/month
Total:         ~$2,159/month
```

### Scenario 2: Scheduled Development (8 AM–6 PM Weekdays)

**Setup:** Orchestrator always on, test cluster 8 hrs/day 5 days/week

```
EC2:           $252/month (orchestrator) + $137/month (tests) = $389
Bedrock:       $1,650/month (5 agents during working hours)
Storage:       $20/month
Monitoring:    $10/month
Total:         ~$2,069/month (15% savings)
```

### Scenario 3: Nightly Testing Only

**Setup:** Cluster spins up 2 AM, runs tests, tears down by 6 AM (4 hrs/day)

```
EC2:           $252/month (orchestrator) + $23/month (tests) = $275
Bedrock:       $200/month (nightly test runner only)
Storage:       $20/month
Monitoring:    $10/month
Total:         ~$505/month (77% savings!)
```

## Cost Monitoring

### AWS Budgets

```bash
# View current spend
aws budgets describe-budget --account-id <account> \
  --budget-name cfs-daily-100

# Update budget
aws budgets update-budget --account-id <account> \
  --new-budget Limit=50  # Reduce from $100 if needed
```

### Cost Anomaly Detection

```bash
# Enable AWS Cost Anomaly Detection
aws ce enable-cost-anomaly-detection \
  --cost-anomaly-detection-expression-configuration
```

### Cost Allocation Tags

ClaudeFS tags all resources automatically:
```hcl
tags = {
  Project     = "claudefs"
  Environment = "dev"
  Agent       = "A11"  # Track costs per agent
}
```

Query costs by tag:
```bash
aws ce get-cost-and-usage \
  --time-period Start=2026-03-01,End=2026-03-31 \
  --granularity DAILY \
  --metrics "UnblendedCost" \
  --group-by Type=DIMENSION,Key=TAG \
  --filter '{"Tags": {"Key": "Agent"}}'
```

## Recommendations for Phase 2

| Action | Savings | Priority | Effort |
|--------|---------|----------|--------|
| Switch t3.xlarge for test clients | $1/day | Medium | Low |
| Tear down cluster outside working hours | $8/day | High | Low |
| Set spot price limit to 0.50 | $2-3/day | Medium | Low |
| Reduce Prometheus retention to 3d | $0.50/day | Low | Low |
| Use Reserved Instance for orchestrator | $3-4/day | Medium | Medium |
| **Total potential savings** | **$14-17/day** | | |

## Production Cost Estimate

When moving to production (not dev/test):

| Component | Size | Cost |
|-----------|------|------|
| Metadata servers | 3x (always on) | $12/day |
| Storage servers | 30x (mixed hot/warm) | $50-100/day |
| Cloud conduit | 2x (cross-site) | $1/day |
| Monitoring | Prometheus + Grafana | $0.50/day |
| S3 tiering | 100 TB baseline | $2,300/month |
| **Production (estimated)** | | **$100-150/day + $2,300/month storage** |

## Budget Enforcement

ClaudeFS includes automatic cost controls:

1. **AWS Budgets** — alerts at 80%, hard stop at $100/day
2. **Cost monitor script** — terminates spot instances when budget exceeded
3. **Terraform cost estimation** — `terraform plan` shows resource costs

```bash
# Before provisioning, check estimated cost
terraform plan -json | jq '.resource_changes[] | select(.type == "aws_instance")'

# After provisioning, see actual costs
aws ce get-cost-and-usage --time-period Start=$(date -d '1 day ago' +%Y-%m-%d),End=$(date +%Y-%m-%d) \
  --granularity DAILY --metrics "UnblendedCost"
```

## Next Steps

- **Establish cost baseline** during Phase 2 (track actual vs estimated)
- **Implement scheduled cluster provisioning** (save $8/day)
- **Set up AWS Cost Anomaly Detection** (catch budget surprises)
- **Review monthly costs** and adjust per actual usage patterns
- **Plan Reserved Instance strategy** for Phase 3 production

---

**Last Updated:** 2026-03-01
**Author:** A11 Infrastructure & CI
