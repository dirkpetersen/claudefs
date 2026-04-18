# A11 Phase 4 Block 4: Deployment & Release Pipeline — Implementation Plan

**Agent:** A11 Infrastructure & CI
**Status:** 🟡 PLANNING → IN PROGRESS
**Date:** 2026-04-18
**Duration:** Days 7-8 (Phase 4 timeline)
**Session:** 8 (Session 1 of Block 4)

---

## Overview

Phase 4 Block 4 focuses on **production deployment automation and staged rollout**. After implementing recovery infrastructure (Block 3), we now build the pipeline to deliver ClaudeFS binaries to production with minimal risk through automated staging, health verification, and automatic rollback.

### Key Goals
1. Build production binaries (x86_64, aarch64) with optimizations
2. Cryptographically sign releases for security verification
3. Automate staged rollout with health monitoring
4. Generate release notes and track deployments
5. Enable rapid iteration while maintaining stability

---

## Deliverables

### 1. Production Build Automation (`tools/build-release.sh`)

**Purpose:** Create optimized, signed release binaries

**Features:**
- Cargo release build with LTO optimization
- Debug symbol stripping for size reduction
- Multi-arch builds (x86_64, aarch64)
- Binary checksumming (SHA256)
- Version tagging and artifact naming

**Implementation Steps:**

```bash
#!/bin/bash
set -e

VERSION="${1:-1.0.0}"
BUILD_DIR="./target/release"
ARTIFACT_DIR="./releases"

# Create artifacts directory
mkdir -p "$ARTIFACT_DIR"

# Clean and rebuild
cargo clean
cargo build --release --locked

# Binary files to package
BINARIES=(
  "$BUILD_DIR/cfs-server"
  "$BUILD_DIR/cfs-client"
  "$BUILD_DIR/cfs-mgmt"
)

# For each binary, create tarball and checksum
for BINARY in "${BINARIES[@]}"; do
  NAME=$(basename "$BINARY")

  # Strip debug symbols (x86_64)
  strip "$BINARY" -o "$BUILD_DIR/${NAME}.stripped"

  # Create tarball
  tar -czf "$ARTIFACT_DIR/${NAME}-v${VERSION}-x86_64.tar.gz" \
    -C "$BUILD_DIR" "${NAME}.stripped"

  # Generate checksum
  sha256sum "$ARTIFACT_DIR/${NAME}-v${VERSION}-x86_64.tar.gz" > \
    "$ARTIFACT_DIR/${NAME}-v${VERSION}-x86_64.sha256"
done

# aarch64 cross-compilation (if available)
if command -v cargo-build-cross &> /dev/null; then
  for BINARY in "${BINARIES[@]}"; do
    NAME=$(basename "$BINARY")
    cargo build --release --target aarch64-unknown-linux-gnu
    # Similar packaging for aarch64
  done
fi

echo "Release artifacts ready in $ARTIFACT_DIR"
```

**Output Structure:**
```
releases/
├── cfs-server-v1.0.0-x86_64.tar.gz
├── cfs-server-v1.0.0-x86_64.sha256
├── cfs-server-v1.0.0-x86_64.tar.gz.asc (signed, added in step 2)
├── cfs-client-v1.0.0-x86_64.tar.gz
├── cfs-client-v1.0.0-x86_64.sha256
├── cfs-mgmt-v1.0.0-x86_64.tar.gz
├── cfs-mgmt-v1.0.0-x86_64.sha256
└── MANIFEST-v1.0.0.json (metadata, added in step 2)
```

---

### 2. Binary Signing & Verification

**Purpose:** Cryptographically sign releases for tamper detection

**Implementation:**

**2a. GPG Key Setup**
- Store GPG private key in AWS Secrets Manager (`cfs/gpg-key`)
- Key ID exported during CI/CD
- Private key protected with strong passphrase

**2b. Signing Script** (`tools/sign-release.sh`)

```bash
#!/bin/bash
set -e

ARTIFACT_DIR="${1:-.releases}"
KEY_ID="${CFS_GPG_KEY_ID:-DEADBEEF}"  # Key ID from Secrets Manager
MANIFEST="${ARTIFACT_DIR}/MANIFEST-$(git describe --tags).json"

# Extract key from Secrets Manager
aws secretsmanager get-secret-value --secret-id cfs/gpg-key \
  --region us-west-2 \
  --query SecretString --output text | gpg --import

# Sign each artifact
for FILE in "$ARTIFACT_DIR"/*.tar.gz; do
  gpg --local-user "$KEY_ID" \
      --armor \
      --detach-sign \
      "$FILE"
done

# Generate manifest
cat > "$MANIFEST" << EOF
{
  "version": "$(git describe --tags)",
  "timestamp": "$(date -u -Iseconds)",
  "binaries": {
    "cfs-server": {
      "x86_64": {
        "url": "https://github.com/dirkpetersen/claudefs/releases/download/...",
        "sha256": "$(sha256sum ${ARTIFACT_DIR}/cfs-server-*.tar.gz | awk '{print $1}')",
        "signature_url": "..."
      }
    }
  },
  "signing_key_id": "$KEY_ID"
}
EOF

echo "Signed $ARTIFACT_DIR"
```

**2c. Verification Script** (`tools/verify-release.sh`)

```bash
#!/bin/bash

ARTIFACT_DIR="${1:-.releases}"
MANIFEST="${2:-MANIFEST.json}"

# Import public key (if needed)
# gpg --import cfs-public-key.asc

# Verify each signature
for SIG_FILE in "$ARTIFACT_DIR"/*.asc; do
  BINARY_FILE="${SIG_FILE%.asc}"

  if gpg --verify "$SIG_FILE" "$BINARY_FILE"; then
    echo "✓ $(basename $BINARY_FILE) verified"
  else
    echo "✗ $(basename $BINARY_FILE) FAILED VERIFICATION"
    exit 1
  fi
done

echo "All signatures verified"
```

**Output:**
- `.asc` files for each binary (GPG armored signatures)
- `MANIFEST-v1.0.0.json` with metadata and checksums

---

### 3. GitHub Release & Artifact Upload

**Purpose:** Publish binaries to GitHub Releases for downloading

**Workflow:** `.github/workflows/release.yml`

```yaml
name: Release & Rollout
on:
  push:
    tags:
      - 'v[0-9]+.[0-9]+.[0-9]+'  # v1.0.0, v1.0.1, etc.

jobs:
  build-and-sign:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Build release binaries
        run: ./tools/build-release.sh ${{ github.ref_name }}

      - name: Sign artifacts
        env:
          CFS_GPG_KEY_ID: ${{ secrets.CFS_GPG_KEY_ID }}
        run: |
          # Import GPG key from Secrets Manager
          aws secretsmanager get-secret-value \
            --secret-id cfs/gpg-key \
            --region us-west-2 \
            --query SecretString --output text | gpg --import
          ./tools/sign-release.sh ./releases

      - name: Create GitHub Release
        uses: softprops/action-gh-release@v1
        with:
          files: releases/*
          body_path: RELEASE-NOTES-${{ github.ref_name }}.md
          draft: false
          prerelease: false
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}

  canary-deploy:
    needs: build-and-sign
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Deploy to canary node (1 test storage node)
        run: |
          ./tools/rollout.sh \
            --version ${{ github.ref_name }} \
            --stage canary \
            --nodes 1

      - name: Run 24h POSIX test suite
        run: cargo test --lib posix -- --test-threads=1

      - name: Health check
        run: |
          sleep 1800  # Wait 30 min for system to stabilize
          ./tools/health-check.sh --cluster-nodes 1 --timeout 86400

  10pct-rollout:
    needs: canary-deploy
    if: success()
    runs-on: ubuntu-latest
    steps:
      - name: Deploy to 10% (1 storage + 1 client)
        run: |
          ./tools/rollout.sh \
            --version ${{ github.ref_name }} \
            --stage 10pct \
            --nodes 2

  50pct-rollout:
    needs: 10pct-rollout
    if: success()
    runs-on: ubuntu-latest
    steps:
      - name: Deploy to 50% (3 storage + 1 client)
        run: |
          ./tools/rollout.sh \
            --version ${{ github.ref_name }} \
            --stage 50pct \
            --nodes 4

  100pct-rollout:
    needs: 50pct-rollout
    if: success()
    runs-on: ubuntu-latest
    steps:
      - name: Deploy to 100% (full cluster)
        run: |
          ./tools/rollout.sh \
            --version ${{ github.ref_name }} \
            --stage 100pct \
            --nodes 10

      - name: Post-deployment validation
        run: cargo test --lib integration -- --test-threads=1
```

---

### 4. Staged Rollout Orchestration (`tools/rollout.sh`)

**Purpose:** Automate deployment across cluster stages with health verification

**Features:**
- Canary: 1 test node (24h POSIX validation)
- 10%: 1 storage + 1 client
- 50%: 3 storage + 1 client
- 100%: Full cluster (10+ nodes)
- Automatic rollback on failure
- Health verification between stages

**Implementation:**

```bash
#!/bin/bash
set -e

VERSION="${VERSION:-1.0.0}"
STAGE="${STAGE:-canary}"
NODES="${NODES:-1}"
ROLLOUT_TIMEOUT="${ROLLOUT_TIMEOUT:-600}"

# Orchestrator IP
ORCHESTRATOR="$(aws ec2 describe-instances \
  --filters "Name=tag:Name,Values=cfs-orchestrator" \
  --query 'Reservations[0].Instances[0].PrivateIpAddress' \
  --output text)"

echo "[$(date)] Starting $STAGE rollout of v$VERSION to $NODES nodes"

# 1. Download binaries from GitHub Release
echo "[$(date)] Downloading artifacts..."
RELEASE_URL="https://api.github.com/repos/dirkpetersen/claudefs/releases/tags/v$VERSION"
DOWNLOAD_DIR="/tmp/cfs-release-$VERSION"
mkdir -p "$DOWNLOAD_DIR"

curl -s "$RELEASE_URL" | jq -r '.assets[].browser_download_url' | \
  while read URL; do
    wget "$URL" -P "$DOWNLOAD_DIR"
  done

# 2. Verify signatures
echo "[$(date)] Verifying signatures..."
cd "$DOWNLOAD_DIR"
for SIG in *.asc; do
  BINARY="${SIG%.asc}"
  gpg --verify "$SIG" "$BINARY" || {
    echo "Signature verification failed!";
    exit 1;
  }
done

# 3. Select target nodes based on stage
case "$STAGE" in
  canary)
    TARGET_NODES=$(aws ec2 describe-instances \
      --filters "Name=tag:Role,Values=storage" \
        "Name=tag:Stage,Values=test" \
      --query 'Reservations[0:1].Instances[0].InstanceId' \
      --output text | head -n1)
    WAIT_TIME=86400  # 24 hours
    ;;
  10pct)
    TARGET_NODES=$(aws ec2 describe-instances \
      --filters "Name=tag:Role,Values=storage,client" \
      --query 'Reservations[0:2].Instances[0:1].InstanceId' \
      --output text | head -n2)
    WAIT_TIME=3600   # 1 hour
    ;;
  50pct)
    TARGET_NODES=$(aws ec2 describe-instances \
      --filters "Name=tag:Role,Values=storage,client" \
      --query 'Reservations[0:4].Instances[0:3].InstanceId' \
      --output text | head -n4)
    WAIT_TIME=3600
    ;;
  100pct)
    TARGET_NODES=$(aws ec2 describe-instances \
      --filters "Name=tag:State,Values=running" \
      --query 'Reservations[].Instances[].InstanceId' \
      --output text)
    WAIT_TIME=0  # No wait, full prod
    ;;
esac

# 4. Deploy to target nodes
echo "[$(date)] Deploying to $STAGE nodes: $TARGET_NODES"
for NODE_ID in $TARGET_NODES; do
  echo "[$(date)] Deploying to $NODE_ID..."

  # SSH to node and upgrade
  ssh "ec2-user@$NODE_ID" << SCRIPT
    set -e
    cd /tmp
    tar -xzf /tmp/cfs-server-v$VERSION-x86_64.tar.gz
    sudo systemctl stop cfs || true
    sudo install -m755 ./cfs-server.stripped /usr/local/bin/cfs-server
    sudo systemctl start cfs

    # Wait for service to be ready
    for i in {1..30}; do
      if curl -s http://localhost:9000/health > /dev/null; then
        echo "Service ready"
        exit 0
      fi
      sleep 1
    done
    echo "Service failed to start"
    exit 1
SCRIPT
done

# 5. Run health checks
echo "[$(date)] Running health checks for $WAIT_TIME seconds..."
START_TIME=$(date +%s)
while true; do
  ELAPSED=$(($(date +%s) - START_TIME))
  if [ $ELAPSED -ge $WAIT_TIME ]; then
    break
  fi

  # Check each node
  FAILED=0
  for NODE_ID in $TARGET_NODES; do
    NODE_IP=$(aws ec2 describe-instances \
      --instance-ids "$NODE_ID" \
      --query 'Reservations[0].Instances[0].PrivateIpAddress' \
      --output text)

    # Check health endpoint
    if ! curl -s "http://$NODE_IP:9000/health" | grep -q "ok"; then
      echo "❌ $NODE_ID health check failed"
      FAILED=$((FAILED + 1))
    fi
  done

  if [ $FAILED -gt 0 ]; then
    echo "[$(date)] Health check failed. Rolling back..."
    # Rollback procedure (restore previous version)
    exit 1
  fi

  sleep 60
done

echo "[$(date)] ✓ $STAGE rollout complete"
```

---

### 5. Release Notes Generation

**Purpose:** Automatically generate human-readable release notes

**Script:** `tools/generate-release-notes.sh`

```bash
#!/bin/bash

VERSION="${1:-1.0.0}"
PREVIOUS_TAG=$(git describe --tags --abbrev=0 $(git rev-list --tags --max-count=1 ^$(git describe --tags --abbrev=0)^) 2>/dev/null || echo "HEAD~50")

cat > "RELEASE-NOTES-v${VERSION}.md" << EOF
# ClaudeFS v${VERSION} Release Notes

**Release Date:** $(date -u +"%Y-%m-%d %H:%M:%S UTC")

## What's New

### Features
- Production deployment automation with staged rollout
- Binary signing and verification for security
- Automated health monitoring during rollout
- Recovery action framework for operational automation

### Bug Fixes
$(git log --oneline ${PREVIOUS_TAG}..HEAD | grep -i "fix\|bug\|issue" | sed 's/^/- /' || echo "- Minor bug fixes and improvements")

### Security
- GPG-signed release binaries
- Signature verification before deployment
- Automatic rollback on health check failure

## Installation

\`\`\`bash
# Verify signature
gpg --verify cfs-server-v${VERSION}-x86_64.tar.gz.asc cfs-server-v${VERSION}-x86_64.tar.gz

# Extract and install
tar -xzf cfs-server-v${VERSION}-x86_64.tar.gz
sudo install -m755 ./cfs-server /usr/local/bin/cfs-server
\`\`\`

## Contributors

$(git shortlog -s -n ${PREVIOUS_TAG}..HEAD | sed 's/^/- /')

## Full Changelog

[View full commit history](https://github.com/dirkpetersen/claudefs/compare/${PREVIOUS_TAG}..v${VERSION})

---

For detailed information, see the [full release page](https://github.com/dirkpetersen/claudefs/releases/tag/v${VERSION}).
EOF
```

---

## Testing Strategy

### Unit Tests (12 tests)
- test_build_script_creates_binaries
- test_binary_stripping_reduces_size
- test_checksums_generated_correctly
- test_gpg_signing_works
- test_signature_verification_passes
- test_manifest_created_with_metadata
- test_rollout_script_selects_correct_nodes
- test_health_check_detects_failure
- test_automatic_rollback_triggers
- test_release_notes_generation
- test_version_tagging_correct
- test_multi_arch_build_support

### Integration Tests (8 tests)
- test_build_release_pipeline_end_to_end
- test_canary_deployment_workflow
- test_staged_rollout_10pct
- test_staged_rollout_50pct
- test_staged_rollout_100pct
- test_rollback_on_health_failure
- test_signature_verification_in_deployment
- test_github_release_created_with_artifacts

---

## Configuration

**File:** `crates/claudefs-mgmt/src/deployment_config.rs` (or similar)

```rust
pub struct DeploymentConfig {
    pub build_profile: BuildProfile,  // debug, release, lto
    pub target_archs: Vec<Arch>,      // x86_64, aarch64
    pub signing_enabled: bool,
    pub gpg_key_id: String,
    pub rollout_stages: Vec<RolloutStage>,
    pub health_check_interval_secs: u64,
    pub health_check_timeout_secs: u64,
    pub rollback_on_failure: bool,
}

#[derive(Debug, Clone)]
pub enum BuildProfile {
    Debug,
    Release,
    ReleaseLTO,  // Link-time optimization for smaller size
}

#[derive(Debug, Clone)]
pub struct RolloutStage {
    pub name: String,        // "canary", "10%", "50%", "100%"
    pub node_count: usize,
    pub health_check_duration_secs: u64,
    pub auto_promote: bool,  // Automatically proceed to next stage
}
```

---

## Dependencies & Cross-Crate APIs

### External (GitHub Actions, AWS)
- GitHub API: Upload release artifacts
- AWS Secrets Manager: GPG key storage
- AWS EC2: Instance management for rollout
- Cargo: Build system

### Internal
- All crates (A1-A8): Must have `/health` endpoint for health checks
- health.rs (A8): Health status API
- metrics.rs (A8): Performance metrics during rollout

---

## Files to Create/Modify

**New Files:**
- `tools/build-release.sh` (80 lines)
- `tools/sign-release.sh` (60 lines)
- `tools/verify-release.sh` (40 lines)
- `tools/rollout.sh` (200 lines)
- `tools/health-check.sh` (60 lines)
- `tools/generate-release-notes.sh` (50 lines)
- `.github/workflows/release.yml` (150 lines)
- `crates/claudefs-mgmt/src/deployment.rs` (200 lines, optional)

**Modified Files:**
- `Cargo.toml` — Add `[profile.release]` with LTO optimization
- `docs/DEPLOYMENT.md` — Deployment procedures documentation

---

## Metrics & Observability

**New Metrics:**
- `deployments_started_total` (counter)
- `deployments_successful_total` (counter)
- `deployments_failed_total` (counter)
- `rollout_stage_duration_seconds` (histogram)
- `health_check_failures_total` (counter)
- `automatic_rollbacks_total` (counter)

**Logging:**
- All deployment stages logged with timestamps
- Rollback events logged with reason
- Health check failures logged with details

---

## Success Criteria

✅ **Block 4 Complete when:**
- [ ] `tools/build-release.sh` builds both x86_64 and aarch64 binaries
- [ ] Binary sizes reduced by 60%+ with stripping and LTO
- [ ] `tools/sign-release.sh` signs artifacts with GPG
- [ ] `tools/verify-release.sh` verifies signatures correctly
- [ ] GitHub Actions workflow creates releases automatically on tag
- [ ] `tools/rollout.sh` deploys to canary stage successfully
- [ ] Health checks detect failures and trigger rollback
- [ ] Staged rollout completes: canary → 10% → 50% → 100%
- [ ] Release notes auto-generated and published
- [ ] 20+ deployment tests passing
- [ ] Manual E2E test: v1.0.0 deployed to test cluster
- [ ] All code committed and pushed to main

**Next:** Phase 4 Block 5 (Cost Monitoring & Optimization)

---

## References

- Phase 4 Overview: `docs/A11-PHASE4-PLAN.md`
- Release Strategy: `docs/DEPLOYMENT.md`
- Build System: `Cargo.toml`
- Health Monitoring: `crates/claudefs-mgmt/src/health.rs`

---

## Timeline

**Estimated Implementation:** 2-3 days (Session 8-9)

**Sub-tasks:**
1. Build automation (3 hours) — cargo release, stripping, multi-arch
2. Binary signing (2 hours) — GPG integration, secrets management
3. GitHub release integration (2 hours) — Actions workflow
4. Staged rollout script (4 hours) — Node selection, health checks, rollback
5. Release notes generation (1 hour) — Automation, templating
6. Testing and validation (4 hours) — Local testing, canary deployment
7. Documentation (2 hours) — Deployment procedures, troubleshooting

**Total:** 18 hours over 2-3 sessions
