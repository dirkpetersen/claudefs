# ClaudeFS Operational Readiness Guide

**Last Updated:** 2026-03-01
**Status:** Phase 7 Infrastructure Complete, Operational Activation Pending
**Audience:** Operations, DevOps, Infrastructure teams

## Executive Summary

ClaudeFS Phase 7 CI/CD infrastructure is **implementation complete** but awaiting operational activation. All workflows are committed locally and validated, but cannot be pushed to GitHub due to authentication token scope limitation. This document describes the pre-activation checklist, current blockers, and activation procedures.

### Current Status
- ✅ All 6 GitHub Actions workflows implemented and validated
- ✅ Terraform modules for infrastructure provisioning ready
- ✅ Autonomous supervision infrastructure (watchdog, supervisor, cost monitor) deployed
- ✅ Comprehensive documentation complete
- ⏳ GitHub push blocked: token needs `workflow` scope
- ⏳ Build validation blocked: compilation errors in uncommitted code

## Pre-Activation Checklist

### 1. GitHub Token Scope

**Issue:** Pushing `.github/workflows/` files requires GitHub token with `workflow` scope.

**Current State:**
```bash
$ git push origin main
# ERROR: refusing to allow an OAuth App to create or update workflow
#        `.github/workflows/a9-tests.yml` without `workflow` scope
```

**Resolution Steps:**

1. **Generate new token with proper scopes:**
   ```bash
   # Go to: https://github.com/settings/tokens/new
   # Select scopes: repo, workflow, read:org
   # Copy the token
   ```

2. **Update git credentials:**
   ```bash
   # Option A: Update ~/.git-credentials
   echo "https://USERNAME:NEW_TOKEN@github.com" > ~/.git-credentials
   chmod 600 ~/.git-credentials

   # Option B: Use GitHub CLI
   gh auth login --with-token < token.txt
   ```

3. **Push workflows:**
   ```bash
   git push origin main
   ```

4. **Verify workflows appear in GitHub Actions tab:**
   - Go to: https://github.com/dirkpetersen/claudefs/actions
   - Should show: ci-build, tests-all, integration-tests, release, deploy-prod, a9-tests

### 2. Build Validation

**Current Issue:** Workspace has uncommitted compilation errors preventing `cargo build`.

**Error Details:**
```
error[E0515]: cannot return value referencing local data `dm`
  --> crates/claudefs-security/src/pentest_full_tests.rs:183:13
```

**Resolution:**

The supervisor or appropriate agent (A10) should fix this via OpenCode:
```bash
cat > pentest-fix.md << 'EOF'
Fix the lifetime issue in pentest_full_tests.rs line 183.
The start_drain function returns a Future that captures references.
Ensure the Future is properly awaited and the borrow scope is correct.
EOF

~/.opencode/bin/opencode run "$(cat pentest-fix.md)" \
  --model fireworks-ai/accounts/fireworks/models/minimax-m2p5 > output.md
```

**Temporary Workaround:**
```bash
# Revert uncommitted security test changes
git checkout crates/claudefs-security/src/pentest_full_tests.rs
git checkout crates/claudefs-security/src/final_unsafe_tests.rs
```

### 3. AWS Credentials & Secrets

**Required Secrets (AWS Secrets Manager, us-west-2):**

```
Secret Name                  | Purpose
---------------------------- | ---------------------------------------------------
cfs/github-token            | GitHub API access for workflow runs
cfs/fireworks-api-key       | OpenCode model API access (already deployed)
cfs/ssh-private-key         | SSH access to test cluster nodes
```

**Verification:**
```bash
# From orchestrator, verify secrets are accessible
aws secretsmanager get-secret-value --secret-id cfs/github-token \
  --region us-west-2 --query 'SecretString' | jq '.'
```

### 4. S3 Backend for Terraform State

**Terraform requires S3 backend with DynamoDB locking:**

```bash
# Create S3 bucket for state
aws s3 mb s3://claudefs-terraform-state-$(date +%s) \
  --region us-west-2

# Enable versioning
aws s3api put-bucket-versioning \
  --bucket claudefs-terraform-state-XXXXX \
  --versioning-configuration Status=Enabled \
  --region us-west-2

# Create DynamoDB table for locking
aws dynamodb create-table \
  --table-name claudefs-terraform-lock \
  --attribute-definitions AttributeName=LockID,AttributeType=S \
  --key-schema AttributeName=LockID,KeyType=HASH \
  --billing-mode PAY_PER_REQUEST \
  --region us-west-2
```

**Update Terraform backend configuration:**
```bash
# In tools/terraform/main.tf, ensure backend is configured:
# terraform {
#   backend "s3" {
#     bucket         = "claudefs-terraform-state-XXXXX"
#     key            = "prod/terraform.tfstate"
#     region         = "us-west-2"
#     dynamodb_table = "claudefs-terraform-lock"
#     encrypt        = true
#   }
# }
```

### 5. IAM Roles & Policies

**Required IAM Roles:**

1. **cfs-orchestrator-role:**
   - EC2 full access (for spot instances)
   - Bedrock full access (for Claude API calls)
   - Secrets Manager read access
   - CloudWatch logs write
   - S3 access (for Terraform state + artifacts)

2. **cfs-spot-node-role:**
   - Secrets Manager read (for cluster secrets)
   - CloudWatch logs write
   - EC2 describe (for discovery)

**Verification:**
```bash
aws iam get-role --role-name cfs-orchestrator-role
aws iam get-role --role-name cfs-spot-node-role
```

### 6. CI/CD Environment Variables

**In GitHub Secrets (https://github.com/dirkpetersen/claudefs/settings/secrets):**

```
AWS_REGION=us-west-2
AWS_ACCOUNT_ID=<your-account-id>
ECR_REGISTRY=<account-id>.dkr.ecr.us-west-2.amazonaws.com
ARTIFACT_BUCKET=claudefs-ci-artifacts
```

## Activation Sequence

### Phase 1: Pre-Activation (1 hour)

```bash
# 1. Resolve build issues
git checkout crates/claudefs-security/src/pentest_full_tests.rs
git checkout crates/claudefs-security/src/final_unsafe_tests.rs

# 2. Verify build succeeds
cargo build --workspace
cargo test --workspace --lib

# 3. Verify all workflows are valid YAML
for f in .github/workflows/*.yml; do
  python3 -c "import yaml; yaml.safe_load(open('$f'))"
  echo "✓ $f is valid"
done
```

### Phase 2: Push Workflows (5 minutes)

```bash
# 1. Update GitHub token with workflow scope
# 2. Push changes
git push origin main

# 3. Verify workflows appear in GitHub Actions tab
#    https://github.com/dirkpetersen/claudefs/actions
```

### Phase 3: First Test Run (30 minutes)

```bash
# 1. Make a test commit to trigger CI
git commit --allow-empty -m "[TEST] First CI run verification"
git push origin main

# 2. Monitor workflow execution
#    GitHub Actions > ci-build
#    Check for:
#    - ✓ Build succeeds (debug + release)
#    - ✓ Formatting passes
#    - ✓ Clippy lint passes
#    - ✓ Cargo audit passes
#    - ✓ Documentation builds

# 3. Check logs for warnings and adjust cache keys if needed
```

### Phase 4: Full Suite Run (90 minutes)

```bash
# 1. Trigger full test suite
git commit --allow-empty -m "[TEST] Full test suite validation"
git push origin main

# 2. Monitor:
#    - tests-all.yml (45 min) — all 3512+ tests
#    - integration-tests.yml (30 min) — cross-crate validation
#    - a9-tests.yml (20 min) — A9 test harness

# 3. Expected outputs:
#    - All tests pass
#    - Artifact cache builds
#    - Performance metrics collected
```

### Phase 5: Release & Deployment Verification (60 minutes)

```bash
# 1. Create a release tag
git tag -a v0.7.0 -m "Phase 7: Production Infrastructure Complete"
git push origin v0.7.0

# 2. Monitor release.yml:
#    - Binary builds (x86_64, ARM64)
#    - GitHub Release created
#    - Artifacts uploaded

# 3. Test production deployment workflow (on staging first)
#    - Go to: Actions > "Production Deployment"
#    - Run with: environment=staging, cluster_size=3
#    - Verify: Infrastructure spins up, health checks pass
```

## Monitoring & Cost Control

### Budget Monitoring

**Current Daily Budget: $100/day**

Monitor via:
```bash
# AWS Cost Explorer CLI
aws ce get-cost-and-usage \
  --time-period Start=2026-03-01,End=2026-03-02 \
  --granularity DAILY \
  --metrics UnblendedCost \
  --group-by Type=DIMENSION,Key=SERVICE

# Or use the CloudWatch dashboard
# https://console.aws.amazon.com/cloudwatch/
```

### Watchdog & Supervisor Status

```bash
# Check watchdog (tmux session on orchestrator)
tmux list-sessions | grep cfs-watchdog

# Check logs
tail -100f /var/log/cfs-agents/watchdog.log

# Check supervisor
tail -100f /var/log/cfs-agents/supervisor.log
```

### CI Metrics to Track

| Metric | Target | Alert Threshold |
|--------|--------|-----------------|
| Build time (ci-build.yml) | <30m | >40m |
| Test time (tests-all.yml) | <45m | >60m |
| Integration time | <30m | >45m |
| Cache hit rate | >70% | <50% |
| Artifact size | <500MB | >1GB |
| Failed tests | 0 | >0 |

## Troubleshooting

### Workflow Not Triggering

**Problem:** Pushed code but workflows don't run.

**Solution:**
1. Check workflows appear in GitHub Actions tab
2. Verify default branch is `main`
3. Check workflow trigger conditions (push vs pull_request)
4. Look for syntax errors in workflow YAML

### Test Failures in CI

**Problem:** Tests pass locally but fail in GitHub Actions.

**Causes:**
- Different system environment (kernel version, dependencies)
- Cache corruption
- Timing-sensitive tests
- Concurrent test interference

**Solution:**
```bash
# Re-run job without cache
# In GitHub Actions > [workflow run] > [job] > "Re-run job without cache"

# Or manually clear cache
gh actions-cache delete --repo dirkpetersen/claudefs --all
```

### Out of Budget

**Problem:** Spot instances terminated due to budget limit.

**Solution:**
```bash
# Check current spend
aws ce get-cost-and-usage \
  --time-period Start=2026-03-01,End=$(date +%Y-%m-%d) \
  --granularity DAILY \
  --metrics UnblendedCost

# If over budget, reduce:
# 1. Cluster size: 5 storage nodes → 3
# 2. Instance types: i4i.2xlarge → i4i.xlarge
# 3. Disable nightly test runs (adjust schedule)

# Update .github/workflows/tests-all.yml schedule
```

## Documentation References

- **Main Infrastructure Guide:** `docs/ci-cd-infrastructure.md`
- **Deployment Runbook:** `docs/deployment-runbook.md`
- **Phase 7 Summary:** `PHASE7_COMPLETION.md`
- **Cost Management:** `docs/cost-management.md`
- **Disaster Recovery:** `docs/disaster-recovery.md`

## Next Steps

### Immediate (Day 1)
1. Resolve GitHub token scope and push workflows
2. Fix compilation errors in uncommitted code
3. Run first CI validation

### Short-term (Week 1)
1. Monitor first full test cycle
2. Collect metrics on build times and cache behavior
3. Fine-tune thread counts and timeouts
4. Document any adjustments

### Medium-term (Month 1)
1. Collect performance baselines
2. Optimize Docker build layer caching
3. Implement cost optimization strategies
4. Plan Phase 8 enhancements

## Success Criteria

✅ **Ready for activation when:**
- All 6 workflows pushed to GitHub main
- First CI run completes successfully
- All 3512+ tests pass in CI
- Artifacts build and upload correctly
- Cost stays within $100/day budget
- Release workflow creates GitHub Release
- Deployment workflow provisions test infrastructure
- Health checks pass on deployed cluster

---

**Document Status:** ✅ Production Ready
**Last Review:** 2026-03-01
**Next Review:** 2026-03-15 (post-activation)

**Prepared by:** A11 (Infrastructure & CI)
