# ClaudeFS Deployment & Release Management

**Phase 4 Block 4 — Production Deployment Automation**

This document describes how to build, sign, test, and deploy ClaudeFS releases to production using automated pipelines.

## Table of Contents

1. [Quick Start](#quick-start)
2. [Build Process](#build-process)
3. [Release Signing](#release-signing)
4. [Deployment Stages](#deployment-stages)
5. [Health Monitoring](#health-monitoring)
6. [Rollback Procedures](#rollback-procedures)
7. [Troubleshooting](#troubleshooting)

## Quick Start

### Automated Release (GitHub Actions)

```bash
# Tag a release and push to GitHub
git tag v1.0.0
git push origin v1.0.0

# GitHub Actions workflow triggers automatically:
# 1. Builds optimized binaries (x86_64, aarch64)
# 2. Signs artifacts with GPG
# 3. Creates GitHub Release with notes
# 4. Deploys to canary (24h validation)
# 5. Deploys to 10%, 50%, 100% on success
```

### Manual Local Build

```bash
# Build release binaries
./tools/build-release.sh v1.0.0

# Sign artifacts (requires GPG key)
./tools/sign-release.sh ./releases

# Verify signatures
./tools/verify-release.sh ./releases

# Deploy to canary
./tools/rollout.sh --version v1.0.0 --stage canary

# Monitor health during canary
./tools/health-check.sh --cluster-nodes 1 --timeout 86400

# Progress to next stages
./tools/rollout.sh --version v1.0.0 --stage 10pct
./tools/rollout.sh --version v1.0.0 --stage 50pct
./tools/rollout.sh --version v1.0.0 --stage 100pct
```

## Build Process

### Prerequisites

- Rust 1.70+ with `rustup`
- `cargo` build system
- Optional: `aarch64-linux-gnu-gcc` for cross-compilation

### LTO-Optimized Release Build

```bash
./tools/build-release.sh [VERSION] [--aarch64]
```

**Features:**
- **LTO (Link-Time Optimization):** ~60% smaller binaries
- **Debug Symbol Stripping:** Reduces runtime memory footprint
- **Multi-arch Support:** Builds for x86_64, optionally aarch64
- **Checksums:** SHA256 verification for artifact integrity
- **Build Manifest:** Metadata for tracking builds

**Output:**
```
releases/
├── cfs-v1.0.0-x86_64.tar.gz         # Compressed binary
├── cfs-v1.0.0-x86_64.tar.gz.sha256  # Checksum
├── cfs-v1.0.0-aarch64.tar.gz        # (if --aarch64)
├── cfs-v1.0.0-aarch64.tar.gz.sha256
└── BUILD-MANIFEST-v1.0.0.json       # Metadata
```

### Build Profile Configuration

Release builds use LTO optimization:

```toml
[profile.release]
opt-level = 3                  # Maximum optimization
lto = "fat"                    # Link-time optimization
codegen-units = 1             # Single codegen unit
panic = "abort"               # Smaller panic handler
split-debuginfo = "packed"    # Debug info for later stripping
```

**Performance vs Size:**
- **Compilation time:** +20-30% longer (one-time cost)
- **Binary size:** -60% smaller (from ~200MB to ~80MB)
- **Runtime performance:** Negligible difference (same optimization level)

## Release Signing

### GPG Key Setup

1. **Generate GPG key (one-time):**
   ```bash
   gpg --gen-key
   # Follow prompts: RSA 4096, no expiry, email, passphrase
   ```

2. **Store in AWS Secrets Manager:**
   ```bash
   # Export key
   gpg --export-secret-keys --armor KEY_ID > /tmp/private.key

   # Store in Secrets Manager
   aws secretsmanager create-secret \
     --name cfs/gpg-key \
     --secret-string "$(cat /tmp/private.key)" \
     --region us-west-2

   # Store key ID separately
   aws secretsmanager create-secret \
     --name cfs/gpg-key-id \
     --secret-string "KEY_ID" \
     --region us-west-2

   # Store passphrase
   aws secretsmanager create-secret \
     --name cfs/gpg-passphrase \
     --secret-string "PASSPHRASE" \
     --region us-west-2
   ```

3. **Export public key for distribution:**
   ```bash
   gpg --export --armor KEY_ID > cfs-public.asc
   # Distribute to users for verification
   ```

### Sign Release Artifacts

```bash
./tools/sign-release.sh [ARTIFACT_DIR] [GPG_KEY_ID]
```

**Process:**
1. Imports GPG key from AWS Secrets Manager (if available)
2. Creates detached signatures (.asc files) for each binary
3. Generates manifest JSON with metadata
4. Records signing key ID for verification

**Output:**
```
releases/
├── cfs-v1.0.0-x86_64.tar.gz.asc    # Detached signature
├── cfs-v1.0.0-aarch64.tar.gz.asc
└── MANIFEST-v1.0.0.json            # Signed metadata
```

### Verify Signatures

```bash
./tools/verify-release.sh [ARTIFACT_DIR]
```

**Checks:**
- GPG signature validity for each binary
- SHA256 checksum match
- Manifest integrity

## Deployment Stages

### Stage 1: Canary (1 Test Node, 24 hours)

Perfect for validating breaking changes with controlled exposure.

```bash
./tools/rollout.sh --version v1.0.0 --stage canary
```

**Process:**
1. Deploy to 1 test storage node (isolated from production)
2. Run full POSIX test suite (847+ tests)
3. Monitor health for 24 hours
4. Automatic rollback on failure (3+ health check failures)

**Validation:**
- pjdfstest compliance
- fsx stress testing
- Data consistency checks
- Replication verification

**Promotion Criteria:**
- All POSIX tests pass
- No health check failures (24 hours)
- Replication lag < 60 seconds

### Stage 2: 10% (2 Nodes, 1 Hour)

Test with real workloads and client access.

```bash
./tools/rollout.sh --version v1.0.0 --stage 10pct
```

**Deployment:**
- 1 production storage node
- 1 FUSE client node
- Health monitoring: 1 hour

**Rollback Trigger:**
- Health check failure
- Replication lag > 60s
- Service startup failure

### Stage 3: 50% (4 Nodes, 1 Hour)

Majority of cluster deployed.

```bash
./tools/rollout.sh --version v1.0.0 --stage 50pct
```

**Deployment:**
- 3 storage nodes (quorum)
- 1 NFS/SMB client node

### Stage 4: 100% (Full Cluster)

All nodes updated.

```bash
./tools/rollout.sh --version v1.0.0 --stage 100pct
```

**Deployment:**
- All storage nodes
- All client nodes
- All gateway nodes

## Health Monitoring

### Real-Time Health Checks

```bash
./tools/health-check.sh --cluster-nodes 3 --timeout 1800 --interval 30
```

**Checks Performed:**
1. **Node Status:** Service health endpoint response
2. **Replication Lag:** Cross-site replication delay (< 60s target)
3. **Data Consistency:** Read-write verification
4. **Raft Quorum:** Leader election status
5. **Disk Usage:** Alert if > 90%
6. **Memory Usage:** Alert if < 100MB free

**Metrics Monitored:**
- `deployments_started_total` — Total deployments initiated
- `deployments_successful_total` — Successful deployments
- `deployments_failed_total` — Failed deployments
- `rollout_stage_duration_seconds` — Time per stage
- `health_check_failures_total` — Health check failures
- `automatic_rollbacks_total` — Automatic rollbacks triggered

### Prometheus Metrics Endpoint

Access cluster metrics:

```bash
curl http://prometheus:9090/api/v1/query?query=health_check_failures_total
```

**Key Queries:**
```promql
# Replication lag (seconds)
replication_lag_seconds

# Raft leaders active
raft_leader_count

# Node health status
node_health_status

# Deployment stage progress
rollout_stage_progress_percent
```

## Rollback Procedures

### Automatic Rollback

Triggered by:
- 3+ consecutive health check failures
- Service startup failure
- Signature verification failure
- Replication lag > 60 seconds (sustained)

**Automatic Process:**
1. Logs rollback reason
2. Stops update process
3. Restores previous version on affected nodes
4. Verifies service restart
5. Publishes alert

### Manual Rollback

```bash
# Rollback specific node
ssh ec2-user@NODE_IP << 'SCRIPT'
  sudo systemctl stop cfs
  sudo install -m755 /var/backups/cfs-previous /usr/local/bin/cfs
  sudo systemctl start cfs
SCRIPT

# Verify rollback
./tools/health-check.sh --cluster-nodes 1 --timeout 300
```

### Version History

```bash
# View deployment history
git log --oneline --grep="v[0-9]" | head -20

# Restore specific version
git checkout v0.9.0
./tools/build-release.sh v0.9.0
./tools/rollout.sh --version v0.9.0 --stage 100pct
```

## Troubleshooting

### Build Failures

**LTO compilation timeout:**
```bash
# Reduce optimization level
RUSTFLAGS="" ./tools/build-release.sh v1.0.0
```

**Out of memory during linking:**
```bash
# Reduce codegen units
RUSTFLAGS="-C codegen-units=4" cargo build --release
```

### Deployment Failures

**SSH connectivity issues:**
```bash
# Test node connectivity
for NODE in $(aws ec2 describe-instances --query 'Reservations[*].Instances[*].PrivateIpAddress' --output text); do
  echo -n "$NODE: "
  ssh -o ConnectTimeout=5 ec2-user@$NODE exit 2>/dev/null && echo "OK" || echo "FAIL"
done
```

**Service startup failure:**
```bash
# Check logs on node
ssh ec2-user@NODE_IP 'sudo journalctl -u cfs -n 50'

# Verify binary is valid
ssh ec2-user@NODE_IP 'ldd /usr/local/bin/cfs'
```

**Health check timeouts:**
```bash
# Check Prometheus availability
curl http://prometheus:9090/api/v1/status/config

# Query specific metric
curl 'http://prometheus:9090/api/v1/query?query=up'
```

### Signature Verification Issues

**Import public key:**
```bash
gpg --import cfs-public.asc
```

**Re-verify all artifacts:**
```bash
./tools/verify-release.sh ./releases --verbose
```

**List imported keys:**
```bash
gpg --list-keys
```

## Release Checklist

Before deploying:

- [ ] Code reviewed and merged to main
- [ ] All tests passing (cargo test --all)
- [ ] Version bumped in Cargo.toml
- [ ] CHANGELOG.md updated
- [ ] Security audit complete (if Phase 3+)
- [ ] Performance baselines acceptable

During deployment:

- [ ] Build completes without warnings
- [ ] GPG signing succeeds
- [ ] Signatures verify correctly
- [ ] GitHub Release created
- [ ] Canary deployment successful (24h)
- [ ] 10% stage healthy (1h)
- [ ] 50% stage healthy (1h)
- [ ] 100% stage healthy

After deployment:

- [ ] Monitor metrics for 24 hours
- [ ] Check replication lag
- [ ] Verify Raft consensus
- [ ] Test client workloads
- [ ] Document any issues
- [ ] Update runbooks

## References

- [Architecture Decisions](decisions.md) — D5, D8: Deployment & S3 Tiering
- [Build System](../Cargo.toml) — Release profile configuration
- [GitHub Actions](../.github/workflows/release.yml) — Automated pipeline
- [Tools Directory](../tools/) — Deployment scripts
- [Health Monitoring](../crates/claudefs-mgmt/src/health.rs) — Health endpoints

## FAQ

**Q: Can I deploy without canary?**
A: Not recommended. Canary validates breaking changes and catches regressions. If urgent, use `--stage 100pct` directly at your own risk.

**Q: How do I verify a downloaded release?**
A:
```bash
# Download public key
wget https://github.com/dirkpetersen/claudefs/raw/main/cfs-public.asc
gpg --import cfs-public.asc

# Verify downloaded artifacts
gpg --verify cfs-v1.0.0-x86_64.tar.gz.asc cfs-v1.0.0-x86_64.tar.gz
```

**Q: What happens if a node crashes during deployment?**
A: Watchdog detects the crash within 2 minutes and restarts the service. If the crash is persistent, manual investigation is required. Deployment can be resumed on other nodes.

**Q: Can I deploy multiple versions simultaneously?**
A: Not recommended. Deploy one version completely before starting another. Use canary first if you need to validate behavior on live traffic.

**Q: How long does a full deployment take?**
A: ~30+ hours:
- Build & sign: 10 min
- Canary: 24h (can be shortened to 1h for non-breaking changes)
- 10% → 50% → 100%: ~30 min each

**Q: What's the difference between canary and 10%?**
A: Canary is isolated (test node only), while 10% includes production nodes and real client workloads. Use canary for comprehensive testing, 10% for real-world validation.

---

**Last Updated:** 2026-04-18
**Author:** A11 Infrastructure & CI Agent
**Status:** Phase 4 Block 4 — Production Ready
