# ClaudeFS Cost Optimization Guide

**Last Updated:** 2026-03-01
**Budget:** $100/day
**Typical Daily Cost:** $85-96/day
**Optimization Target:** <$80/day (20% savings)

## Cost Breakdown

### Current Daily Costs (US-West-2)

| Component | Type | Quantity | Rate | Daily | Notes |
|-----------|------|----------|------|-------|-------|
| **Orchestrator** | EC2 c7a.2xlarge on-demand | 1 | $0.42/hr | $10.08 | Always running |
| **Storage nodes** | EC2 i4i.2xlarge spot (8hr) | 5 | $0.28/hr | $11.20 | 60-90% discount |
| **FUSE client** | EC2 c7a.xlarge spot (8hr) | 1 | $0.07/hr | $0.56 | Test harness |
| **NFS/SMB client** | EC2 c7a.xlarge spot (8hr) | 1 | $0.07/hr | $0.56 | Multi-protocol |
| **Cloud conduit** | EC2 t3.medium spot (8hr) | 1 | $0.02/hr | $0.16 | Cross-site relay |
| **Jepsen controller** | EC2 c7a.xlarge spot (8hr) | 1 | $0.07/hr | $0.56 | Fault injection |
| **Secrets Manager** | Secrets | 2 | $0.04/mo | $0.03 | Negligible |
| **Subtotal EC2** | | | | **$22.68** | |
| **Bedrock API** | Claude API | 5-7 agents | Variable | **$55-70** | Largest cost |
| **S3** | Storage + transfer | ~100GB | $0.023/GB | **$2.30** | Low |
| **Bandwidth** | Data transfer | Cross-region | $0.02/GB | **<$1** | Internal |
| **Total** | | | | **$85-96** | |

## Key Opportunities

### 1. Bedrock API Optimization (Highest ROI)

**Current:** $55-70/day | **Target:** $30-40/day | **Savings:** $20-30/day

Strategy: Use appropriate model tier per task
- Haiku: 90% of work (bug fixes, tests, ops) - saves $0.003/token
- Sonnet: 9% of work (code review, impl) - saves $0.001/token
- Opus: 1% of work (arch reviews only) - no change

**Implementation:**
```bash
# tools/cfs-agent-launcher.sh — add model selection
if [[ "$AGENT" =~ A1|A3|ops ]]; then
  MODEL="haiku"  # Routine work
elif [[ "$AGENT" =~ A4|A5|A7 ]]; then
  MODEL="sonnet" # Complex work
else
  MODEL="haiku"  # Default
fi
```

**Expected:** $20-30/day savings (30% of API costs)

### 2. Compute Right-Sizing

**Current:** $22.68/day | **Target:** $15/day | **Savings:** $7/day

Right-size instances:
- i4i.2xlarge (5) → i4i.xlarge (3): $3.00/day
- c7a.xlarge (2) → c7a.large (2): $2.80/day
- Jepsen → t3.large: $0.40/day

**Testing required:** +5-10 min per test acceptable

**Expected:** $6.20/day savings (25% compute)

### 3. Scheduled Provisioning

**Current:** 24/7 cluster | **Target:** Weekday only

Provision cluster Mon-Fri 02:00-10:00 UTC only:
- Weekend shutdown: save $11.35/day
- Weekday nightly: save $5.68/day (cluster down during agent work hours)
- Total potential: $45/day (50% cluster savings)

**Trade-off:** Nightly-only testing, no continuous CI feedback

**Conservative approach:** Mon-Fri provisioning only
- Saves $25-30/day
- Still validates on working days

**Expected:** $10-15/day savings (if conservative)

### 4. Storage & Artifacts

**Current:** $2.30/day | **Target:** $1.30/day | **Savings:** $1/day

Reduce S3 artifact retention: 30 days → 7 days
- CI artifacts auto-expire after 7 days
- Release artifacts: permanent
- Reduce average storage by 50%

**Expected:** $1/day savings

## Implementation Plan

### Phase 1: Immediate (Week 1)
**Effort:** 4 hours | **Savings:** $20-25/day | **Risk:** Minimal

1. Update cfs-agent-launcher.sh for model selection
2. Update GitHub Actions artifact retention
3. Batch watchdog diagnostics into single API call
4. Monitor for 1 week

**Result:** $65-75/day

### Phase 2: Testing (Week 2-3)
**Effort:** 8 hours | **Savings:** $6/day | **Risk:** Moderate

1. Test with smaller instances on staging
2. Measure test time overhead (target: <10%)
3. If acceptable, roll out to production

**Result:** $60-70/day (if Phase 2 succeeds)

### Phase 3: Scheduled Provisioning (Month 2)
**Effort:** 6 hours | **Savings:** $10-15/day | **Risk:** Medium

1. Implement cron-based cluster provisioning
2. Pilot on staging: Mon-Fri only
3. Validate tests still complete reliably
4. Full rollout if successful

**Result:** $50-60/day (if Phase 3 succeeds)

## Monitoring

```bash
# Daily cost tracking
aws ce get-cost-and-usage \
  --time-period Start=$(date -d yesterday +%Y-%m-%d),End=$(date +%Y-%m-%d) \
  --metrics UnblendedCost \
  --query 'ResultsByTime[0].Total.UnblendedCost.Amount'

# Alert if over $105
```

## Annual Impact

- **Current:** $31,000-$35,000/year
- **Phase 1:** $23,700-$27,400 (-25%)
- **Phase 1+2:** $21,900-$25,600 (-35%)
- **All phases:** $18,250-$22,000 (-45%)

---

**Prepared by:** A11 (Infrastructure & CI)
**Status:** Ready for Implementation
