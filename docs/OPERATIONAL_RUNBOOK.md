# ClaudeFS Operational Runbook

**Last Updated:** 2026-03-04
**Version:** 1.0
**Owner:** A11 (Infrastructure & CI)

## Overview

This runbook documents standard operating procedures for managing the ClaudeFS development cluster, including the orchestrator node, spot instance test cluster, CI/CD pipelines, cost management, and troubleshooting.

## Table of Contents

1. [Quick Start](#quick-start)
2. [Cluster Architecture](#cluster-architecture)
3. [Daily Operations](#daily-operations)
4. [Monitoring & Observability](#monitoring--observability)
5. [Troubleshooting](#troubleshooting)
6. [Cost Management](#cost-management)
7. [Deployment Procedures](#deployment-procedures)
8. [Runbooks by Role](#runbooks-by-role)

---

## Quick Start

### Checking Cluster Status

```bash
# SSH to orchestrator
ssh -i ~/.ssh/cfs-key.pem ubuntu@<orchestrator-public-ip>

# Check all agent tmux sessions
tmux ls

# View watchdog status
tail -100 /var/log/cfs-agents/watchdog.log

# View supervisor status
tail -50 /var/log/cfs-agents/supervisor.log

# Check last 5 commits
git log --oneline -5
```

### Starting/Stopping the Cluster

```bash
# Provision orchestrator and start agents (Phase 1)
cd ~/claudefs && cfs-dev up --phase 1

# Check current status
cfs-dev status

# Tear down spot cluster (keep orchestrator)
cfs-dev down

# Tear down everything (requires confirmation)
cfs-dev destroy

# View current AWS costs
cfs-dev cost
```

---

## Cluster Architecture

### Persistent Components

| Component | Type | Instance | Cost |
|-----------|------|----------|------|
| **Orchestrator** | On-demand EC2 | `c7a.2xlarge` (8 vCPU, 16 GB) | ~$10/day |
| **Watchdog** | Tmux session on orchestrator | N/A | Included in orchestrator |
| **Supervisor** | Cron job (every 15 min) | N/A | Included in orchestrator |
| **Cost Monitor** | Cron job (every 15 min) | N/A | Included in orchestrator |

### Spot Instance Test Cluster (Preemptible, Provisioned On-Demand)

| Role | Count | Instance Type | Cost | Duration |
|------|-------|---------------|------|----------|
| **Storage servers** | 5 | `i4i.2xlarge` | ~$1.75/hr | 8 hours (test run) |
| **FUSE client** | 1 | `c7a.xlarge` | ~$0.20/hr | 8 hours |
| **NFS/SMB client** | 1 | `c7a.xlarge` | ~$0.20/hr | 8 hours |
| **Cloud conduit** | 1 | `t3.medium` | ~$0.03/hr | 8 hours |
| **Jepsen controller** | 1 | `c7a.xlarge` | ~$0.20/hr | 8 hours |

**Total cost (full cluster, 8 hrs):** ~$15-20

### Autonomous Supervision (Three Layers)

1. **Watchdog** (`/opt/cfs-watchdog.sh`)
   - Runs every 2 minutes
   - Monitors agent tmux sessions
   - Restarts dead/idle agents
   - Pushes unpushed commits to GitHub
   - Persistent tmux session: `cfs-watchdog`

2. **Supervisor** (`/opt/cfs-supervisor.sh`)
   - Runs via cron every 15 minutes
   - Gathers full system diagnostics
   - Uses Claude Sonnet to diagnose and fix build errors
   - Commits forgotten files
   - Restarts dead agents
   - Pushes to GitHub

3. **Cost Monitor** (`/opt/cfs-cost-monitor.sh`)
   - Runs via cron every 15 minutes
   - Checks daily AWS spend via Cost Explorer API
   - Terminates all spot instances if $100/day exceeded
   - Publishes SNS alerts to `cfs-budget-alerts` topic

---

## Daily Operations

### Morning Check (Start of Day)

```bash
# SSH to orchestrator
ssh -i ~/.ssh/cfs-key.pem ubuntu@<orchestrator-public-ip>

# 1. Check agent health
tmux ls
# Expected: 11 agent sessions (cfs-a1 through cfs-a11) + 1 watchdog session (cfs-watchdog)

# 2. Check for build errors
cd ~/claudefs && cargo check 2>&1 | grep -E '(error|warning:)' | head -20

# 3. View last night's commits
git log --oneline --since="12 hours ago" | head -20

# 4. Check last supervisor run
tail -20 /var/log/cfs-agents/supervisor.log | grep 'System health\|error'

# 5. Check daily AWS spend
aws ce get-cost-and-usage --time-period Start=2026-03-04,End=2026-03-05 \
  --granularity DAILY --metrics UnblendedCost --group-by Type=DIMENSION,Key=SERVICE \
  --region us-west-2 | jq '.ResultsByTime[0].Groups[] | select(.Metrics.UnblendedCost.Amount != "0")'
```

### During-Day Monitoring

```bash
# Monitor for regressions (run hourly or as needed)
cd ~/claudefs

# Check if any agent is stuck (hasn't committed in 30 min)
git log --oneline --since="30 minutes ago" | wc -l
# Expected: >= 2-3 commits per 30 min from active agents

# Check cargo build status
cargo check 2>&1 | tail -5

# View supervisor logs for any issues
tail -5 /var/log/cfs-agents/supervisor.log

# If build is broken, run diagnostics
cargo check 2>&1 | grep error
# Then either:
# 1. File a GitHub issue if it's a code problem (for the responsible agent)
# 2. Run: cargo clean && cargo check (if it's a build cache issue)
```

### End-of-Day Summary

```bash
# Generate daily summary
git log --oneline --since="24 hours ago" > /tmp/daily-commits.txt
wc -l /tmp/daily-commits.txt

# Get test count
grep -r "#\[test\]" crates/*/src | wc -l

# Summarize by agent
git log --oneline --since="24 hours ago" | sed 's/\[//;s/\].*//' | sort | uniq -c

# Check final AWS spend
aws ce get-cost-and-usage --time-period Start=2026-03-04,End=2026-03-05 \
  --granularity DAILY --metrics UnblendedCost --region us-west-2
```

---

## Monitoring & Observability

### Key Metrics to Track

| Metric | Location | Healthy Range | Alert Threshold |
|--------|----------|----------------|-----------------|
| **Commits/hour** | Watchdog log | 5-15 | < 2 (agents stalled) |
| **Test count** | CHANGELOG.md | Increasing | Flat for >2 hours |
| **Build errors** | `cargo check` output | 0 | Any |
| **Unpushed commits** | `git log origin/main..HEAD` | 0 | > 5 |
| **Daily AWS cost** | AWS Cost Explorer | $80-96 | > $100 |
| **Watchdog restarts** | Watchdog log | < 3/hour | > 5/hour |
| **Supervisor fixes** | Supervisor log | 0-2/day | > 3/day (indicates instability) |

### Real-Time Monitoring Dashboard (Recommended)

```bash
# Terminal 1: Watch commits in real-time
cd ~/claudefs && watch -n 10 'git log --oneline -5'

# Terminal 2: Watch cargo build
watch -n 30 'cd ~/claudefs && cargo check 2>&1 | tail -10'

# Terminal 3: Watch supervisor logs
watch -n 30 'tail -10 /var/log/cfs-agents/supervisor.log'

# Terminal 4: Watch AWS cost
watch -n 300 'aws ce get-cost-and-usage --time-period Start=$(date -d "today" +%Y-%m-%d),End=$(date -d "tomorrow" +%Y-%m-%d) --granularity DAILY --metrics UnblendedCost --region us-west-2 | jq ".ResultsByTime[0].Total.UnblendedCost"'
```

### CloudWatch Metrics (Future)

When budget allows, export metrics to CloudWatch:
- Agent session lifetimes
- Commits per hour
- Build time per crate
- Test count over time
- AWS spend trend

---

## Troubleshooting

### Scenario 1: Agent Session Dead

**Symptoms:**
- `tmux ls` shows no `cfs-a<X>` session
- No commits in the last 30 minutes from that agent
- Supervisor log shows "relaunched"

**Investigation:**
```bash
tmux ls | grep cfs-a
tail -100 /var/log/cfs-agents/watchdog.log | grep -A 5 "A<agent_number>"
tail -100 /var/log/cfs-agents/supervisor.log | grep -A 3 "A<agent_number>"
```

**Fix:**
```bash
# Option 1: Supervisor will auto-restart (happens every 15 min)
# Option 2: Manual restart
/opt/cfs-agent-launcher.sh --agent A<X>
# Option 3: Kill and restart
tmux kill-session -t cfs-a<x>
/opt/cfs-agent-launcher.sh --agent A<X>
```

### Scenario 2: Build Errors

**Symptoms:**
- `cargo check` shows errors
- Supervisor log mentions build failures
- GitHub Actions CI shows red

**Investigation:**
```bash
cd ~/claudefs
cargo check 2>&1 | grep error | head -20
git log -1 --format=%H  # Get latest commit SHA
git show <SHA> # View what changed
```

**Common Causes & Fixes:**

| Error | Cause | Fix |
|-------|-------|-----|
| `error: expected identifier, found keyword` | Formatting issue from agents | `cargo fmt --all` |
| `error: unresolved import` | Missing module in lib.rs | Run supervisor to fix |
| `error: type mismatch` | Cross-crate API change | File GitHub issue for affected agents |
| Compilation hangs | DuckDB or libfabric build | `cargo clean && cargo build` |

### Scenario 3: Unpushed Commits Accumulating

**Symptoms:**
- `git log origin/main..HEAD` shows > 5 commits
- Supervisor log shows "push failed"
- Agent hasn't pushed in > 30 minutes

**Investigation:**
```bash
git log origin/main..HEAD --oneline
git status
git diff --stat origin/main
```

**Fixes:**
```bash
# Option 1: Manual push
cd ~/claudefs && git push origin main

# Option 2: If push is blocked due to divergence
git pull --rebase origin main && git push origin main

# Option 3: If local changes conflict
git stash && git pull origin main && git stash pop
```

### Scenario 4: Cost Spike (Approaching $100/day)

**Symptoms:**
- AWS Cost Explorer shows > $80
- Supervisor log mentions high spend
- SNS alert received

**Investigation:**
```bash
aws ce get-cost-and-usage --time-period Start=2026-03-04,End=2026-03-05 \
  --granularity HOURLY --metrics UnblendedCost --group-by Type=DIMENSION,Key=SERVICE \
  --region us-west-2 | jq '.ResultsByTime[] | "\(.TimePeriod.Start): \(.Groups[] | select(.Metrics.UnblendedCost.Amount != "0") | .Keys[0] + " = $" + .Metrics.UnblendedCost.Amount)"'

# Check for orphaned instances
aws ec2 describe-instances --region us-west-2 --filters "Name=instance-state-name,Values=running" \
  --query 'Reservations[].Instances[].[InstanceId,InstanceType,LaunchTime,State.Name]' --output table
```

**Immediate Actions:**
```bash
# Option 1: Trigger cost monitor manually
/opt/cfs-cost-monitor.sh

# Option 2: Manual termination of spot instances
aws ec2 terminate-instances --instance-ids <id1> <id2> --region us-west-2

# Option 3: If orchestrator is the issue, reduce phase or number of agents
# (Edit cfs-watchdog.sh to reduce AGENTS list)
```

---

## Cost Management

### Daily Budget: $100/day

Breakdown (2026-03-04):
- **EC2 orchestrator:** $10/day (always running)
- **EC2 spot instances (8 hrs/day):** $12/day
- **Bedrock (Claude Opus/Sonnet/Haiku):** $55-70/day (5-7 agents, ~5M tokens/day)
- **Secrets Manager:** $0.03/day
- **CloudWatch/logs:** Minimal
- **S3 (if used for Terraform state):** ~$0.10/day

### Cost Optimization Strategies

1. **Reduce Spot Cluster Runtime**
   - Default: 8 hours/day (during business hours)
   - After-hours: 0 (cost monitor auto-tears down)
   - Adjust: `cfs-dev down` to manually stop

2. **Bedrock Model Selection**
   - Opus: Use only for A10 (Security Audit) — deep reasoning
   - Sonnet: Use for A1-A9 (standard implementation)
   - Haiku: Use for A8, A11 (boilerplate, infrastructure) — **saves 80-90% on these agents**

3. **GitHub Actions Caching**
   - Leverage `actions/cache@v4` aggressively
   - Cache `~/.cargo` registry, index, build targets
   - Saves 10-15 minutes per workflow run

4. **Preemptible Instance Optimization**
   - Use `i4i.2xlarge` for storage nodes (not `i4i.4xlarge`)
   - Use `c7a.xlarge` for clients (not `c7a.2xlarge`)
   - Use `t3.medium` for conduit (cheap, light compute)

### Cost Monitoring Commands

```bash
# Today's spend (real-time)
aws ce get-cost-and-usage --time-period Start=$(date +%Y-%m-%d),End=$(date -d "tomorrow" +%Y-%m-%d) \
  --granularity DAILY --metrics UnblendedCost --region us-west-2

# Last 7 days average
for i in {0..6}; do
  date=$(date -d "$i days ago" +%Y-%m-%d)
  aws ce get-cost-and-usage --time-period Start=$date,End=$(date -d "$date + 1 day" +%Y-%m-%d) \
    --granularity DAILY --metrics UnblendedCost --region us-west-2 | jq ".ResultsByTime[0].Total.UnblendedCost.Amount"
done

# By service (EC2, Bedrock, etc.)
aws ce get-cost-and-usage --time-period Start=$(date +%Y-%m-%d),End=$(date -d "tomorrow" +%Y-%m-%d) \
  --granularity DAILY --metrics UnblendedCost --group-by Type=DIMENSION,Key=SERVICE --region us-west-2 | \
  jq '.ResultsByTime[0].Groups[] | select(.Metrics.UnblendedCost.Amount != "0") | {service: .Keys[0], cost: .Metrics.UnblendedCost}'
```

---

## Deployment Procedures

### Release Build Pipeline

```bash
# 1. Create release tag
cd ~/claudefs
git tag -a v0.1.0 -m "Release v0.1.0: Foundation phase complete"
git push origin v0.1.0

# 2. GitHub Actions automatically:
#    - Builds release binaries (debug + release)
#    - Runs full test suite
#    - Creates GitHub Release with artifacts
#    - Uploads to S3 (if configured)

# 3. Monitor workflow
gh run list --workflow release.yml | head -5
gh run view <run-id>  # Monitor specific run
```

### Manual Multi-Architecture Build (If Needed)

```bash
cd ~/claudefs

# Build for x86_64
cargo build --release --target x86_64-unknown-linux-gnu

# Build for aarch64 (requires cross)
cross build --release --target aarch64-unknown-linux-gnu

# Package binaries
mkdir -p dist/v0.1.0
cp target/x86_64-unknown-linux-gnu/release/cfs dist/v0.1.0/cfs-x86_64
cp target/aarch64-unknown-linux-gnu/release/cfs dist/v0.1.0/cfs-aarch64

# Create checksums
cd dist/v0.1.0
sha256sum cfs-* > SHA256SUMS
```

### Production Deployment (Terraform)

```bash
cd ~/claudefs/tools/terraform

# Validate configuration
terraform fmt
terraform validate

# Plan changes
terraform plan -out=tfplan

# Apply (after review)
terraform apply tfplan

# Output important values
terraform output orchestrator_public_ip
terraform output storage_nodes_private_ips
```

---

## Runbooks by Role

### For Developers (Using cfs-dev CLI)

```bash
# Provision cluster
cfs-dev up --phase 2

# Check status
cfs-dev status

# View logs
cfs-dev logs --agent A3  # View specific agent
cfs-dev logs --agent watchdog  # View watchdog

# SSH to nodes
cfs-dev ssh                          # SSH to orchestrator
cfs-dev ssh storage-node-1          # SSH to specific node

# Tear down
cfs-dev down      # Keep orchestrator, remove spot cluster
cfs-dev destroy   # Remove everything

# Check costs
cfs-dev cost
```

### For Infrastructure Engineers

```bash
# SSH to orchestrator (direct)
ssh -i ~/.ssh/cfs-key.pem ubuntu@<orchestrator-ip>

# Monitor clusters
tmux ls
tail -f /var/log/cfs-agents/watchdog.log

# Debug agents
tmux capture-pane -t cfs-a3 -p  # View agent's terminal
tmux send-keys -t cfs-a3 "C-c"  # Send Ctrl+C to stop agent

# Manage AWS resources
aws ec2 describe-instances --region us-west-2 --filters "Name=tag:Project,Values=claudefs"
aws ec2 terminate-instances --instance-ids i-xxx i-yyy --region us-west-2
```

### For Security Auditors

```bash
# Review supervisor logs for anomalies
grep -E '(error|failed|denied|panic|Security)' /var/log/cfs-agents/supervisor.log | tail -50

# Check for uncommitted secrets
cd ~/claudefs && git diff HEAD | grep -E '(password|secret|key|token)' || echo "No secrets in diff"

# Verify encryption in Terraform
grep -E '(encrypted|kms)' tools/terraform/*.tf

# Check IAM policies (should exist)
ls tools/terraform/iam-policies/
# Verify they don't grant overly broad permissions
```

---

## Appendix: Key Scripts & Files

| Path | Purpose | Owner | Frequency |
|------|---------|-------|-----------|
| `/opt/cfs-watchdog.sh` | Fast agent supervision | A11 | Every 2 min (tmux) |
| `/opt/cfs-supervisor.sh` | Intelligent recovery | A11 | Every 15 min (cron) |
| `/opt/cfs-cost-monitor.sh` | Budget enforcement | A11 | Every 15 min (cron) |
| `/opt/cfs-agent-launcher.sh` | Agent session mgmt | A11 | On-demand |
| `~/.github/workflows/ci-build.yml` | Basic CI (build + fmt + clippy) | A11 | On push/PR |
| `~/.github/workflows/tests-all.yml` | Full test suite | A9 | Nightly + PR |
| `~/.github/workflows/a9-tests.yml` | A9 test harness | A9 | On push to test crates |
| `~/.github/workflows/release.yml` | Release artifacts | A11 | On git tag |
| `~/.github/workflows/deploy-prod.yml` | Production deploy | A11 | Manual trigger |
| `./tools/terraform/*` | Infrastructure-as-code | A11 | Versioned |

---

## Questions & Support

For questions or issues:
1. Check this runbook (Ctrl+F for keywords)
2. Review supervisor logs: `/var/log/cfs-agents/supervisor.log`
3. Create GitHub Issue tagged with `infrastructure` or `ci`
4. Reach out to A11 (Infrastructure & CI agent)

---

**Last Updated:** 2026-03-04
**Next Review:** 2026-03-11
