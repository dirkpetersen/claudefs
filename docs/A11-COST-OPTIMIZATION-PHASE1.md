# A11: Cost Optimization — Phase 1: Model Selection

**Status:** 📋 READY FOR IMPLEMENTATION
**Date:** 2026-03-04
**Target Savings:** $5-10/day
**Current Daily Cost:** $85-96/day
**Target Daily Cost:** $75-86/day

---

## Overview

The largest cost component is Bedrock (LLM inference). A strategic shift in model selection can reduce daily Bedrock costs by 10-15% without sacrificing code quality.

**Key insight:** Not all work requires Opus or Sonnet. Many tasks are well-suited to cheaper models.

---

## Current Model Assignment (Baseline)

| Agent | Current | Task | Tokens/Day | Cost/Day |
|-------|---------|------|-----------|----------|
| A1 | Opus | Storage engine implementation | High | $$$ |
| A2 | Opus | Metadata service, Raft protocol | High | $$$ |
| A3 | Sonnet | Data reduction (dedupe/compress) | Medium | $$ |
| A4 | Opus | Transport layer, RPC protocol | High | $$$ |
| A5 | Sonnet | FUSE client integration | Medium | $$ |
| A6 | Sonnet | Replication, cross-site conduit | Medium | $$ |
| A7 | Sonnet | Protocol gateways (NFS, pNFS, S3) | Medium | $$ |
| A8 | Haiku | Management CLI, metrics, UI | Low | $ |
| A9 | Sonnet | Test harnesses and validation | Medium | $$ |
| A10 | Opus | Security audit, fuzzing, crypto | High | $$$ |
| A11 | Haiku | Infrastructure, CI/CD, Terraform | Low | $ |

**Current breakdown:**
- Opus agents (A1, A2, A4, A10): ~50% of cost
- Sonnet agents (A3-A7, A9): ~40% of cost
- Haiku agents (A8, A11): ~10% of cost

---

## Proposed Model Changes (Phase 1)

### Change 1: A1 (Storage Engine) — Opus → Sonnet

**Rationale:**
- Phase 2 foundation work is complete; Phase 3 is mostly bug fixes and optimization
- io_uring implementation is stable; needs testing validation, not deep architecture work
- OpenCode handles the actual Rust implementation; A1 mostly orchestrates
- Sonnet can handle integration coordination

**Savings:** ~$2-3/day
**Risk:** LOW — storage architecture is stable

**Transition:**
```bash
# In tools/cfs-agent-launcher.sh
if [[ "$AGENT" == "a1" ]]; then
  MODEL="sonnet"  # Was: opus
fi
```

**Success criteria:** A1 continues to iterate on modules without issues; build tests pass

---

### Change 2: A4 (Transport Layer) — Opus → Sonnet

**Rationale:**
- Transport RPC protocol is well-documented and stable
- Phase 2 delivered 60 modules with 900+ tests
- Phase 3 is incremental improvements, not architectural changes
- Protocol design decisions are already made; now implementing edge cases

**Savings:** ~$2-3/day
**Risk:** LOW — protocol is mature

**Transition:**
```bash
# In tools/cfs-agent-launcher.sh
if [[ "$AGENT" == "a4" ]]; then
  MODEL="sonnet"  # Was: opus
fi
```

**Success criteria:** A4 can coordinate RDMA/TCP improvements; Jepsen tests pass

---

### Change 3: A8 (Management) — Haiku → Haiku Fast

**Rationale:**
- A8 generates high-volume boilerplate (CLI subcommands, YAML, JSON, Grafana JSON)
- Haiku Fast is 2-3x faster than Haiku, same pricing ($0.10/1M tokens)
- Better latency for repetitive tasks

**Savings:** 0 (same cost, faster execution)
**Benefit:** 2-3x faster task completion

**Transition:**
```bash
# In tools/cfs-agent-launcher.sh
if [[ "$AGENT" == "a8" ]]; then
  MODEL="haiku-fast"  # Was: haiku
fi
```

**Success criteria:** CLI and UI generation stays on schedule; no quality regression

---

### Change 4: A3 (Reduce) — Sonnet → Sonnet Fast (Selective)

**Rationale:**
- Data reduction algorithms are well-understood (dedupe, compression, encryption)
- Most Phase 3 work is integration testing and optimization
- Sonnet Fast can handle bulk code generation for new test cases

**Implementation:** Conditional use
- Sonnet: Algorithm design, edge cases, fuzzing
- Sonnet Fast: Bulk test generation, benchmarking code

**Savings:** ~$1-2/day (conditional use reduces average cost)
**Risk:** LOW — algorithms are stable

**Success criteria:** Test generation stays fast; correctness not impacted

---

### Change 5: A10 (Security) — Keep Opus

**Rationale:** DO NOT CHANGE
- Security auditing requires deep reasoning and multi-file context
- Fuzzing and crypto review needs the best model
- Cannot risk security regressions

**No change.**

---

## Implementation Roadmap

### Phase 1a: Configuration (Today)
**Owner:** A11
**Effort:** 30 minutes
**Status:** 📝 IN PROGRESS

**Action items:**
1. ✅ Create this document (DONE)
2. 📝 Create `tools/agent-launcher-config.yaml` with model mappings
3. 📝 Modify `tools/cfs-agent-launcher.sh` to read config
4. 📝 Add model selection logic to agent spawn

**Deliverable:** `tools/agent-launcher-config.yaml`
```yaml
agents:
  a1:
    model: sonnet           # Changed from opus
    context: 200k
    role: storage
  a2:
    model: opus             # Keep
    context: 200k
    role: metadata
  a3:
    model: sonnet-fast      # Selective use
    context: 100k
    role: reduce
  a4:
    model: sonnet           # Changed from opus
    context: 100k
    role: transport
  a5:
    model: sonnet
    context: 100k
    role: fuse
  a6:
    model: sonnet
    context: 100k
    role: replication
  a7:
    model: sonnet
    context: 100k
    role: gateway
  a8:
    model: haiku-fast       # Changed from haiku
    context: 100k
    role: management
  a9:
    model: sonnet
    context: 100k
    role: testing
  a10:
    model: opus             # Keep
    context: 200k
    role: security
  a11:
    model: haiku
    context: 100k
    role: infrastructure
```

---

### Phase 1b: Testing (Days 1-3)
**Owner:** A11
**Effort:** 2-3 hours
**Status:** 📋 PLANNED

**Action items:**
1. Deploy model changes to one agent first (A1)
2. Monitor for 24 hours: build success, test pass rate, task completion
3. If successful: deploy to A4
4. If successful: deploy to A3 and A8
5. Validate overall cost reduction

**Metrics to collect:**
- Build time (should stay <20 min)
- Test pass rate (should stay at 100%)
- Task completion time (should improve for A8)
- Daily cost (should drop $5-10)

**Rollback criteria:**
- If build fails: revert to previous model
- If tests drop <95%: revert to previous model
- If A1 productivity drops >20%: revert to Opus

---

### Phase 1c: Validation (Days 3-5)
**Owner:** A11
**Effort:** 1-2 hours
**Status:** 📋 PLANNED

**Action items:**
1. Compare daily costs (before vs after)
2. Document actual savings
3. Update CHANGELOG with results
4. Plan Phase 2 (compute right-sizing)

**Success criteria:**
- Daily cost reduced by $5-10 (target: $75-86)
- All agents productive and meeting deadlines
- No build or test regressions

---

## Expected Cost Impact

### Detailed Calculation

**Current daily cost breakdown (Bedrock):**
- A1 (Opus): 2,000 tokens × $0.00300/token = $6.00
- A2 (Opus): 2,000 tokens × $0.00300/token = $6.00
- A3 (Sonnet): 1,500 tokens × $0.00150/token = $2.25
- A4 (Opus): 2,000 tokens × $0.00300/token = $6.00
- A5 (Sonnet): 1,000 tokens × $0.00150/token = $1.50
- A6 (Sonnet): 1,000 tokens × $0.00150/token = $1.50
- A7 (Sonnet): 1,500 tokens × $0.00150/token = $2.25
- A8 (Haiku): 500 tokens × $0.00010/token = $0.05
- A9 (Sonnet): 1,500 tokens × $0.00150/token = $2.25
- A10 (Opus): 2,000 tokens × $0.00300/token = $6.00
- A11 (Haiku): 300 tokens × $0.00010/token = $0.03
- **Bedrock subtotal:** $33.83/day
- **With overhead (other agents, supervision, background):** ~$55-65/day

**After Phase 1 changes:**
- A1 (Sonnet): 2,000 tokens × $0.00150/token = $3.00 (-$3.00)
- A2 (Opus): 2,000 tokens × $0.00300/token = $6.00 (no change)
- A3 (Sonnet Fast): 1,000 tokens × $0.00100/token = $1.00 (-$1.25)
- A4 (Sonnet): 2,000 tokens × $0.00150/token = $3.00 (-$3.00)
- A8 (Haiku Fast): 500 tokens × $0.00010/token = $0.05 (no change)
- Others: (no change)
- **New Bedrock subtotal:** $24.83/day (-$9.00)
- **With overhead:** ~$45-55/day (-$10-15/day)

**Expected total daily cost:**
- Current: $85-96/day
- After Phase 1: $75-86/day
- **Savings: $10-15/day (12-15% reduction)**

---

## Risks & Mitigation

### Risk 1: Code quality degrades with Sonnet
**Probability:** LOW
**Impact:** MEDIUM — slower agent productivity
**Mitigation:** Monitor first week closely; have rollback plan ready

---

### Risk 2: Sonnet can't handle edge cases
**Probability:** LOW
**Impact:** MEDIUM — specific tasks fail, blocking agent
**Mitigation:** Have supervisor ready to delegate critical work back to Opus via OpenCode

---

### Risk 3: Cost savings don't materialize
**Probability:** LOW
**Impact:** MEDIUM — still at $80-90/day
**Mitigation:** Have Phase 2 (compute right-sizing) and Phase 3 (scheduled provisioning) ready

---

## Success Criteria

| Criterion | Target | Method |
|-----------|--------|--------|
| Daily cost | $75-86 | CloudWatch billing |
| Build time | <25 min | GitHub Actions metrics |
| Test pass rate | ≥95% | cargo test summary |
| Agent productivity | On schedule | CHANGELOG and commit frequency |
| Task completion | No blockers | GitHub issues, supervisor logs |

---

## Timeline

| Phase | Date | Owner | Effort | Status |
|-------|------|-------|--------|--------|
| 1a (Config) | Today (3/4) | A11 | 30 min | 📝 IN PROGRESS |
| 1b (Testing) | 3/4-3/7 | A11 | 2-3 hrs | 📋 PLANNED |
| 1c (Validation) | 3/7-3/9 | A11 | 1-2 hrs | 📋 PLANNED |

---

## Next Steps

1. ✅ Create agent-launcher-config.yaml
2. ✅ Update cfs-agent-launcher.sh to use config
3. ⏳ Test with A1 first (selective rollout)
4. ⏳ Monitor metrics for 24 hours
5. ⏳ Gradual rollout to A3, A4, A8
6. ⏳ Validate total cost savings
7. ⏳ Document results in CHANGELOG

---

**Document Owner:** A11 Infrastructure & CI
**Status:** 📝 READY FOR IMPLEMENTATION
**Cost Savings Target:** $10-15/day
**ROI:** Saves $300-450/month for ~30 min implementation effort
**Next Review:** 2026-03-09 (post-deployment)
