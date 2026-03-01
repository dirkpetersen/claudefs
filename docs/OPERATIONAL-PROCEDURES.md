# ClaudeFS Operational Procedures

**Document Type:** Operational Procedures
**Owner:** A11 Infrastructure & CI
**Last Updated:** 2026-03-01
**Audience:** Operations team, DevOps engineers, system administrators

## Table of Contents

1. [Daily Operations](#daily-operations)
2. [Monitoring & Alerting](#monitoring--alerting)
3. [Troubleshooting](#troubleshooting)
4. [Maintenance Procedures](#maintenance-procedures)
5. [Incident Response](#incident-response)
6. [Scaling & Capacity Planning](#scaling--capacity-planning)
7. [Backup & Disaster Recovery](#backup--disaster-recovery)

---

## Daily Operations

### Morning Check (Every Weekday)

**Time:** Start of business day
**Duration:** 5-10 minutes
**Checklist:**

```bash
# 1. Check cluster health
cfs-dev status

# 2. Review agent activity
cfs-dev logs --tail=50

# 3. Check cost burn rate
cfs-dev cost

# 4. Verify no overnight failures
grep ERROR /var/log/cfs-agents/watchdog.log | tail -20
grep ERROR /var/log/cfs-agents/supervisor.log | tail -20
```

**Expected outputs:**
- All 11 agents running (or 5 agents in Phase 1, 9 in Phase 2)
- No ERROR lines in logs (WARNINGs are OK)
- Daily spend <$100
- Last supervisor run <30 min ago

**Action if not OK:**
- See Troubleshooting section below

### Continuous Monitoring

The **watchdog** and **supervisor** run automatically:
- **Watchdog:** Every 2 minutes (checks agent health, relaunches dead sessions)
- **Supervisor:** Every 15 minutes (checks build errors, fixes via OpenCode, commits forgotten files)
- **Cost monitor:** Every 15 minutes (kills spot instances at $100/day budget)

No manual intervention needed unless logs show ERROR.

### Before Each Deployment/Merge

**Checklist:**

```bash
# 1. Verify build passes
cargo build --release

# 2. Run full test suite
cargo test --lib

# 3. Check for uncommitted work
git status

# 4. Review CHANGELOG for consistency
tail -50 CHANGELOG.md

# 5. Verify no security findings in progress
gh issue list --label HIGH --label CRITICAL
```

**Expected result:**
- Build succeeds
- All tests pass
- Git status is clean or only expected changes
- CHANGELOG updated
- No open HIGH/CRITICAL security issues

---

## Monitoring & Alerting

### Key Metrics to Watch

#### Cost Metrics
- **Daily spend:** Should be <$100
- **Cost per build:** Should be stable (investigate spikes)
- **Token usage by agent:** Opus should be ~20%, Sonnet ~60%, Haiku ~20%

**How to check:**
```bash
cfs-dev cost
# Shows: today's spend, weekly total, monthly projection, budget status
```

#### Performance Metrics
- **Build time:** Should be <30 minutes (target: <20 min)
- **Test time:** Should be <45 minutes
- **Cache hit rate:** Should be >75%

**Where to view:** GitHub Actions tab after workflow push

#### Infrastructure Metrics
- **Agent uptime:** All agents should have >99% uptime
- **Watchdog cycle time:** Should average <2 minutes
- **Supervisor fix success rate:** >90% of build errors fixed automatically
- **Spot instance interruption rate:** Should be <5% of hourly instances

**How to check:**
```bash
# Agent uptime (from supervisor logs)
grep "Agent status" /var/log/cfs-agents/supervisor.log | tail -20

# Watchdog cycle time (from watchdog logs)
grep "cycle" /var/log/cfs-agents/watchdog.log | tail -20

# Spot interruptions (from AWS)
aws ec2 describe-spot-instance-requests --region us-west-2 --query \
  'SpotInstanceRequests[?Status.Code==`marked-for-termination`].SpotInstanceRequestId' | wc -l
```

#### Cluster Health Metrics
- **Storage node availability:** All nodes should be up
- **Metadata Raft consensus:** Should have active leader
- **Replication lag:** Should be <1 second (target)
- **EC stripe health:** No degraded stripes

**How to check:**
```bash
# Connect to orchestrator first
cfs-dev ssh orchestrator

# Then on orchestrator, check cluster health via admin API
curl -s https://localhost:9443/api/cluster/health | jq .

# Or use CLI
cfs admin status

# Check replication lag
cfs admin replication status
```

### Dashboards

After workflow activation, dashboards are available at:
- **Grafana:** https://orchestrator-ip:3000 (admin/admin)
- **Prometheus:** https://orchestrator-ip:9090

**Key dashboards:**
1. **Cluster Overview** — Health, capacity, replication
2. **Cost Analysis** — Daily spend, cost per build, budget forecast
3. **Performance** — Build time, test time, cache hits
4. **Infrastructure** — Node uptime, watchdog health, supervisor stats

---

## Troubleshooting

### Problem: Agent Not Running

**Symptoms:**
- `cfs-dev status` shows agent as "idle" or "missing"
- Logs don't update for >10 minutes

**Diagnosis:**
```bash
# Check tmux session
tmux ls | grep cfs-

# Check if agent process exists
ps aux | grep claude | grep -v grep

# Check watchdog logs
tail -50 /var/log/cfs-agents/watchdog.log | grep -i "agent name"
```

**Solution:**
1. Watchdog should auto-restart within 2 minutes
2. If not restarted after 5 minutes, manually restart:
   ```bash
   tmux kill-session -t cfs-a1  # Example: kill A1 agent session
   # Watchdog will restart it
   ```

3. If watchdog is not running:
   ```bash
   tmux new-session -d -s cfs-watchdog "cd /home/cfs/claudefs && bash tools/cfs-watchdog.sh"
   ```

### Problem: Build Fails

**Symptoms:**
- CI workflow shows red ❌
- `cargo build` fails locally

**Diagnosis:**
```bash
# Get the error message
cargo build 2>&1 | grep -A5 "error"

# Check if it's a merge conflict
git status | grep "both modified"

# Check if external dependency is missing
cargo update --dry-run
```

**Solution:**
1. **Merge conflict:** Follow merge resolution procedure
2. **Rust compilation error:** Check if it's a known issue from A1-A8, file GitHub issue
3. **Test compilation error:** Run with verbose output:
   ```bash
   cargo test --lib --no-fail-fast 2>&1 | tee /tmp/test-build.log
   ```

4. Supervisor will auto-fix if error is fixable via OpenCode
5. If error persists, manually run OpenCode:
   ```bash
   cat > input.md << 'EOF'
   <your error and context>
   EOF
   ~/.opencode/bin/opencode run "$(cat input.md)" \
     --model fireworks-ai/accounts/fireworks/models/minimax-m2p5 > output.md
   ```

### Problem: Tests Failing

**Symptoms:**
- `cargo test --lib` shows failed tests
- CI workflow test stage shows red ❌

**Diagnosis:**
```bash
# Find which test failed
cargo test --lib 2>&1 | grep "FAILED"

# Run that specific test with output
cargo test -p <crate> <test_name> -- --nocapture --test-threads=1

# Check if it's flaky
for i in {1..5}; do cargo test -p <crate> <test_name> || break; done
```

**Solution:**
1. **New test failure:** File issue, investigate with agent owner
2. **Flaky test:** File issue with "flaky" label, mark as `#[ignore]` for now
3. **Dependency issue:** Check if any crate was updated:
   ```bash
   git log -1 --name-only | grep Cargo.toml
   ```

4. **Local environment issue:** Clean and rebuild:
   ```bash
   cargo clean
   cargo test --lib
   ```

### Problem: High Cost / Budget Alert

**Symptoms:**
- `cfs-dev cost` shows spend >$80/day
- Budget alert email received at 80% threshold

**Diagnosis:**
```bash
# Check spot instance count
aws ec2 describe-instances --region us-west-2 \
  --filters "Name=tag:Environment,Values=cfs-dev" \
           "Name=instance-state-name,Values=running" \
  --query 'Reservations[].Instances[].InstanceType' | sort | uniq -c

# Check if large instances are running
aws ec2 describe-instances --region us-west-2 \
  --query 'Reservations[].Instances[?InstanceType==`i4i.4xlarge`].[InstanceId,State.Name]'

# Check model usage
grep "model:" /var/log/cfs-agents/supervisor.log | tail -20
```

**Solution:**
1. **Too many spot instances:** Scale down or wait for cost monitor to auto-kill
2. **Large instances running:** Check if performance test is active; if so, wait for it to complete
3. **Too much Opus/Sonnet usage:** Wait for cycle to complete; next cycle should be cheaper
4. **Consistent high cost:** Plan optimization for next phase (model selection, compute sizing)

**Emergency:** If spend reaches $100, cost monitor automatically kills all spot instances.

---

## Maintenance Procedures

### Weekly Maintenance Window

**Time:** Friday evening (outside business hours)
**Duration:** 1-2 hours
**Procedure:**

```bash
# 1. Notify team
echo "Starting weekly maintenance window"

# 2. Pause agent provisioning (don't start new builds)
# (No CLI command; just don't queue new work)

# 3. Run system update check
cfs-dev status

# 4. Analyze logs for issues
grep WARNING /var/log/cfs-agents/*.log | head -20

# 5. Review cost forecast
cfs-dev cost

# 6. Clean up old artifacts
find /home/cfs/claudefs/target -name "*.d" -mtime +7 -delete

# 7. Compress old logs
gzip /var/log/cfs-agents/*.log.* 2>/dev/null

# 8. Run diagnostics
bash tools/ci-diagnostics.sh > /tmp/diagnostic-report.txt

# 9. Review diagnostic report
less /tmp/diagnostic-report.txt

# 10. Commit any pending work and push
git add -A && git commit -m "[A11] Weekly maintenance - log cleanup, diagnostics" || true
git push
```

### Monthly Capacity Planning

**Time:** First Monday of each month
**Duration:** 30 minutes
**Checklist:**

```bash
# 1. Analyze storage usage growth
du -sh /home/cfs/claudefs/target
du -sh /var/log/cfs-agents

# 2. Check artifact size trend
for f in /tmp/artifact-size-*.txt; do echo "$f:"; tail -1 "$f"; done

# 3. Project next month's capacity needs
# If target/: grows 5GB/week, need 20GB/month → upgrade to 150GB storage

# 4. Check instance uptime
aws ec2 describe-instances --region us-west-2 \
  --filters "Name=tag:Name,Values=cfs-orchestrator" \
  --query 'Reservations[].Instances[].[LaunchTime,State.Name]'

# 5. Review cost trend
# Compare this month's spend to previous month
# If increasing >10%, investigate cause

# 6. Create capacity report
cat > /tmp/capacity-report.txt << 'EOF'
## Capacity Report - 2026-03
- Storage usage: XXX GB
- Monthly spend: $XXX
- Agent uptime: XX.X%
- Test pass rate: XX%
- Recommendations: ...
EOF
```

### Quarterly Infrastructure Review

**Time:** End of each quarter (March 31, June 30, etc.)
**Duration:** 2-4 hours
**Agenda:**

1. **Cost analysis** — Trends, optimizations, projections
2. **Performance review** — Build time, test time, cache hit rate
3. **Scaling readiness** — Can we handle 2x workload? 10x?
4. **Architecture debt** — Technical debt items, refactoring needs
5. **Tool evaluation** — Any new tools to adopt? Deprecated tools?
6. **Security review** — Any new threats? Compliance changes?

---

## Incident Response

### Process

1. **Detect** — Watchdog/Supervisor logs alert, or developer reports
2. **Assess** — Determine severity (SEV1/2/3) and impact
3. **Respond** — Execute severity-appropriate actions
4. **Resolve** — Fix root cause
5. **Communicate** — Update team
6. **Learn** — Post-incident review

### Severity Levels

#### SEV1: Critical
**Example:** Cluster down, data loss risk, cost runaway >$200/day

**Response time:** Immediate
**Actions:**
1. Kill costly resources (cost monitor auto-does this at $100)
2. Scale down to minimal cluster (1 node)
3. Investigate root cause
4. Notify team immediately
5. Post-incident review within 24 hours

#### SEV2: Major
**Example:** Build failing for 1+ hour, 50% of tests failing, replication lag >5min

**Response time:** <15 minutes
**Actions:**
1. Investigate via logs
2. File GitHub issue with details
3. Notify agent owner
4. Monitor for escalation to SEV1
5. Post-incident review within 1 week

#### SEV3: Minor
**Example:** Single test flaky, 1 agent idle, cache hit rate slightly low

**Response time:** <1 hour
**Actions:**
1. Log the issue
2. Investigate if it resolves itself
3. File GitHub issue if persistent
4. Resolve in next optimization cycle

---

## Scaling & Capacity Planning

### Horizontal Scaling: Adding Storage Nodes

**When:** Cluster approaching capacity (>80% utilization)

**Steps:**

```bash
# 1. Provision new nodes
aws ec2 run-instances --region us-west-2 \
  --image-id ami-xxx \
  --instance-type i4i.2xlarge \
  --count 2 \
  --tag-specifications 'ResourceType=instance,Tags=[{Key=Environment,Value=cfs-dev},{Key=Role,Value=storage}]'

# 2. Wait for nodes to boot and join cluster
cfs admin node list  # Wait for new nodes to appear

# 3. Trigger rebalancing
cfs admin rebalance start

# 4. Monitor rebalancing progress
cfs admin rebalance status

# 5. Validate cluster health after rebalancing
cfs admin cluster health
```

### Vertical Scaling: Upgrading Instance Types

**When:** Single node becoming bottleneck (CPU >80%, NVMe throughput saturated)

**Steps:**

1. Drain the node:
   ```bash
   cfs admin node drain <node-id>
   ```

2. Stop the node:
   ```bash
   aws ec2 stop-instances --instance-ids <instance-id>
   ```

3. Change instance type:
   ```bash
   aws ec2 modify-instance-attribute --instance-id <instance-id> \
     --instance-type '{Value: i4i.4xlarge}'
   ```

4. Start the node:
   ```bash
   aws ec2 start-instances --instance-ids <instance-id>
   ```

5. Wait for node to rejoin:
   ```bash
   cfs admin node list  # Wait for status = "healthy"
   ```

6. Trigger rebalancing:
   ```bash
   cfs admin rebalance start
   ```

### Forecast: 6-Month Projection

**Based on current growth:**
- Storage: 50GB/month → need 300GB+ by end of year (upgrade to 500GB)
- Compute: 1 orchestrator + 5-9 storage nodes → may need 15+ by Q4
- Cost: $85/day baseline, optimizations target $70/day → watch for creep above $80/day

**Recommendations:**
- Plan for 2x cluster size by Q4 2026
- Implement multi-region deployment by Q2 2026
- Budget for 10-15 additional instances by year-end

---

## Backup & Disaster Recovery

### Backup Strategy

#### Metadata Backup
- **Frequency:** Every 6 hours
- **Retention:** 30 days
- **Storage:** AWS S3 (separate account if possible)
- **Procedure:**
  ```bash
  cfs admin backup create --name "metadata-$(date +%Y%m%d-%H%M%S)"
  ```

#### Data Backup
- **Strategy:** S3 tiering (D5 in decisions.md)
- **Frequency:** Continuous (asynchronous write-to-S3)
- **Retention:** Indefinite (S3 is source of truth in cache mode)
- **RTO:** Data recoverable from S3 without cluster
- **RPO:** Async lag (seconds to minutes)

#### Snapshot Backups
- **Frequency:** Daily
- **Retention:** 7 days on flash, then archive to S3
- **Procedure:**
  ```bash
  cfs admin snapshot create /data --name "daily-$(date +%Y%m%d)"
  # After 7 days, automatically archived to S3
  ```

### Disaster Recovery Procedures

#### Scenario 1: Single Node Failure

**RTO:** <5 minutes
**Procedure:**

```bash
# 1. Node automatically detected as down (watchdog)
# 2. Raft leader is elected among remaining nodes
# 3. Rebalancing automatically starts
cfs admin rebalance status  # Monitor progress

# 4. Once rebalancing complete, cluster is healthy
cfs admin cluster health
```

**No manual action required.** Watchdog handles everything.

#### Scenario 2: Two-Node Failure (in different failure domains)

**RTO:** <15 minutes
**Procedure:**

```bash
# 1. If failure is in same site (Raft quorum preserved):
#    Same as Scenario 1 — automatic recovery

# 2. If failure spans sites (Raft quorum lost):
#    Activate standby metadata:
cfs admin failover --force-activate-standby

# 3. Monitor replication lag during recovery
cfs admin replication status

# 4. Once stable, investigate root cause and plan repair
```

#### Scenario 3: Data Corruption / Checksum Failures

**RTO:** <1 hour
**Procedure:**

```bash
# 1. Detect (alerts on checksum mismatches)
# 2. Identify affected data ranges
cfs admin verify --scope=suspect

# 3. Recover from EC stripes (if data is erasure-coded)
cfs admin repair --method=ec-reconstruction

# 4. If EC reconstruction fails, recover from S3
cfs admin repair --from-s3 --range=<affected_range>

# 5. Validate integrity
cfs admin verify --scope=repaired
```

#### Scenario 4: Total Cluster Loss

**RTO:** 1-2 hours
**Procedure:**

```bash
# 1. Provision new cluster with same topology
cfs-dev up --phase 3 --cluster-name "prod-recovery"

# 2. Restore metadata from backup
cfs admin restore --backup-id <latest_backup_id>

# 3. Restore data from S3
cfs admin repair --from-s3 --all

# 4. Validate cluster health
cfs admin cluster health
cfs admin verify --scope=full

# 5. Bring clients online
# Point FUSE clients to new cluster DNS/IP
```

**Prevention:**
- Keep backups in separate AWS account
- Test recovery procedure quarterly
- Monitor backup integrity (SHA256 checksums)
- Document cluster configuration (DNS, IPs, credentials)

---

## Escalation Contacts

For specific issues:

| Issue Type | Contact | Time |
|-----------|---------|------|
| Build failure | A1-A8 agent owner | <1 hour |
| Test failure | A9 Test owner | <2 hours |
| Security finding | A10 Security owner | <4 hours |
| Infrastructure/cost issue | A11 (self) | Immediate |
| Cluster down | DevOps on-call | Immediate |

---

**Document Owner:** A11 Infrastructure & CI
**Last Updated:** 2026-03-01
**Review Frequency:** Monthly
