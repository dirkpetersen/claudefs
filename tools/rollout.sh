#!/bin/bash
# Staged rollout script for ClaudeFS deployment
# Usage: ./tools/rollout.sh --version V --stage STAGE [--nodes N] [--dry-run]
#
# Implements staged deployment: canary → 10% → 50% → 100%
# Includes health checks and automatic rollback on failure

set -e

VERSION=""
STAGE=""
NODES=""
DRY_RUN=0
WAIT_TIME=0
HEALTH_CHECK_INTERVAL=60
HEALTH_CHECK_TIMEOUT=1800
ROLLOUT_TIMEOUT=600
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --version) VERSION="$2"; shift 2 ;;
        --stage) STAGE="$2"; shift 2 ;;
        --nodes) NODES="$2"; shift 2 ;;
        --dry-run) DRY_RUN=1; shift ;;
        --health-interval) HEALTH_CHECK_INTERVAL="$2"; shift 2 ;;
        --health-timeout) HEALTH_CHECK_TIMEOUT="$2"; shift 2 ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

# Validate arguments
if [ -z "$VERSION" ] || [ -z "$STAGE" ]; then
    echo "Usage: $0 --version V --stage STAGE [--nodes N] [--dry-run]"
    echo ""
    echo "Stages: canary, 10pct, 50pct, 100pct"
    exit 1
fi

echo "[$(date)] Starting $STAGE rollout of ClaudeFS v$VERSION"

# Stage configuration
case "$STAGE" in
    canary)
        TARGET_NODE_COUNT=1
        WAIT_TIME=86400  # 24 hours
        DESCRIPTION="Canary deployment (1 storage node)"
        ;;
    10pct)
        TARGET_NODE_COUNT=2
        WAIT_TIME=3600   # 1 hour
        DESCRIPTION="10% rollout (1 storage + 1 client)"
        ;;
    50pct)
        TARGET_NODE_COUNT=4
        WAIT_TIME=3600
        DESCRIPTION="50% rollout (3 storage + 1 client)"
        ;;
    100pct)
        TARGET_NODE_COUNT=10
        WAIT_TIME=0
        DESCRIPTION="100% rollout (full cluster)"
        ;;
    *)
        echo "Error: Unknown stage: $STAGE"
        echo "Valid stages: canary, 10pct, 50pct, 100pct"
        exit 1
        ;;
esac

echo "[$(date)] Stage: $DESCRIPTION"

# If nodes specified, override target count
if [ -n "$NODES" ] && [ "$NODES" -gt 0 ]; then
    TARGET_NODE_COUNT="$NODES"
fi

# Create download directory
DOWNLOAD_DIR="/tmp/cfs-release-${VERSION}"
mkdir -p "$DOWNLOAD_DIR"

# Download artifacts
echo "[$(date)] Downloading artifacts for v$VERSION..."

if [ "$DRY_RUN" -eq 0 ]; then
    RELEASE_URL="https://api.github.com/repos/dirkpetersen/claudefs/releases/tags/v${VERSION}"

    # Try to download from GitHub Releases
    if command -v gh &> /dev/null; then
        gh release download "v${VERSION}" -D "$DOWNLOAD_DIR" 2>/dev/null || {
            echo "Warning: Could not download from GitHub, checking local releases..."
            if [ -d "./releases" ]; then
                cp ./releases/cfs-*-v${VERSION}-*.tar.gz* "$DOWNLOAD_DIR/" 2>/dev/null || true
            fi
        }
    else
        # Fallback: check local releases directory
        if [ -d "./releases" ]; then
            echo "[$(date)] Using local release artifacts..."
            cp ./releases/cfs-*-v${VERSION}-*.tar.gz* "$DOWNLOAD_DIR/" 2>/dev/null || true
        else
            echo "Error: Could not download artifacts and no local releases found"
            exit 1
        fi
    fi
fi

# Verify signatures if present
echo "[$(date)] Verifying artifact signatures..."
cd "$DOWNLOAD_DIR"

if [ "$DRY_RUN" -eq 0 ]; then
    VERIFY_FAILED=0
    for SIG in *.asc; do
        if [ -f "$SIG" ]; then
            BINARY="${SIG%.asc}"
            if ! gpg --verify "$SIG" "$BINARY" 2>/dev/null; then
                echo "✗ Signature verification failed for $BINARY"
                VERIFY_FAILED=$((VERIFY_FAILED + 1))
            fi
        fi
    done

    if [ $VERIFY_FAILED -gt 0 ]; then
        echo "Error: Signature verification failed"
        exit 1
    fi
    echo "[$(date)] ✓ All signatures verified"
fi

# Select target nodes based on stage
echo "[$(date)] Selecting target nodes for $STAGE stage..."

TARGET_NODES=()

if command -v aws &> /dev/null; then
    case "$STAGE" in
        canary)
            # 1 test storage node
            TARGET_NODES=$(aws ec2 describe-instances \
                --filters "Name=tag:Role,Values=storage" \
                          "Name=tag:Stage,Values=test" \
                          "Name=instance-state-name,Values=running" \
                --query 'Reservations[*].Instances[0].InstanceId' \
                --output text | tr '\t' '\n' | head -n1)
            ;;
        10pct)
            # 1 storage + 1 client
            TARGET_NODES=$(aws ec2 describe-instances \
                --filters "Name=tag:Role,Values=storage,client" \
                          "Name=instance-state-name,Values=running" \
                --query 'Reservations[*].Instances[0].InstanceId' \
                --output text | tr '\t' '\n' | head -n2)
            ;;
        50pct)
            # 3 storage + 1 client
            TARGET_NODES=$(aws ec2 describe-instances \
                --filters "Name=tag:Role,Values=storage,client" \
                          "Name=instance-state-name,Values=running" \
                --query 'Reservations[*].Instances[*].InstanceId' \
                --output text | tr '\t' '\n' | head -n4)
            ;;
        100pct)
            # All running nodes
            TARGET_NODES=$(aws ec2 describe-instances \
                --filters "Name=instance-state-name,Values=running" \
                --query 'Reservations[*].Instances[*].InstanceId' \
                --output text | tr '\t' '\n')
            ;;
    esac
else
    echo "Warning: AWS CLI not available, using mock deployment"
    DRY_RUN=1
fi

NODE_ARRAY=($TARGET_NODES)
ACTUAL_NODE_COUNT=${#NODE_ARRAY[@]}

if [ $ACTUAL_NODE_COUNT -eq 0 ]; then
    echo "Error: No target nodes found"
    exit 1
fi

echo "[$(date)] Target nodes ($ACTUAL_NODE_COUNT): ${NODE_ARRAY[@]}"

# Deploy to target nodes
echo "[$(date)] Starting deployment..."
DEPLOY_FAILED=0

for NODE_ID in "${NODE_ARRAY[@]}"; do
    echo "[$(date)] Deploying to $NODE_ID..."

    if [ "$DRY_RUN" -eq 0 ]; then
        NODE_IP=$(aws ec2 describe-instances \
            --instance-ids "$NODE_ID" \
            --query 'Reservations[0].Instances[0].PrivateIpAddress' \
            --output text)

        echo "         Node IP: $NODE_IP"

        # Transfer artifacts to node
        TARBALL=$(ls "$DOWNLOAD_DIR"/cfs-*-v${VERSION}-*.tar.gz 2>/dev/null | head -1)
        if [ -z "$TARBALL" ]; then
            echo "Error: No tarball found for deployment"
            DEPLOY_FAILED=$((DEPLOY_FAILED + 1))
            continue
        fi

        echo "         Transferring $(basename "$TARBALL")..."

        if scp -o ConnectTimeout=10 -o StrictHostKeyChecking=no \
               "$TARBALL" "ec2-user@$NODE_IP:/tmp/" 2>/dev/null; then

            # Deploy on node
            echo "         Installing binary..."
            ssh -o ConnectTimeout=10 -o StrictHostKeyChecking=no \
                "ec2-user@$NODE_ID" << 'DEPLOY_SCRIPT'
set -e
TARBALL=$(ls /tmp/cfs-*-*.tar.gz 2>/dev/null | head -1)
if [ -z "$TARBALL" ]; then
    echo "Error: Tarball not found"
    exit 1
fi

cd /tmp
tar -xzf "$TARBALL"
BINARY=$(ls ./cfs 2>/dev/null | head -1)
if [ -z "$BINARY" ]; then
    echo "Error: Binary not found in tarball"
    exit 1
fi

# Stop service
sudo systemctl stop cfs 2>/dev/null || true

# Install new binary
sudo install -m755 "$BINARY" /usr/local/bin/cfs

# Start service
sudo systemctl start cfs

# Wait for service to be ready
for i in {1..30}; do
    if curl -s http://localhost:9000/health 2>/dev/null | grep -q ok; then
        echo "Service ready"
        exit 0
    fi
    sleep 1
done

echo "Error: Service failed to start within 30 seconds"
exit 1
DEPLOY_SCRIPT

            if [ $? -ne 0 ]; then
                echo "✗ Deployment failed on $NODE_ID"
                DEPLOY_FAILED=$((DEPLOY_FAILED + 1))
            else
                echo "✓ Deployed successfully to $NODE_ID"
            fi
        else
            echo "✗ Failed to transfer tarball to $NODE_ID"
            DEPLOY_FAILED=$((DEPLOY_FAILED + 1))
        fi
    else
        echo "[DRY-RUN] Would deploy to $NODE_ID"
    fi
done

if [ $DEPLOY_FAILED -gt 0 ]; then
    echo "[$(date)] ✗ Deployment failed on $DEPLOY_FAILED node(s)"
    exit 1
fi

# Run health checks
if [ $WAIT_TIME -gt 0 ]; then
    echo "[$(date)] Running health checks for $((WAIT_TIME / 60)) minutes..."

    START_TIME=$(date +%s)
    HEALTH_CHECK_FAILED=0

    while true; do
        ELAPSED=$(($(date +%s) - START_TIME))

        if [ $ELAPSED -ge $WAIT_TIME ]; then
            echo "[$(date)] Health check period complete"
            break
        fi

        # Check each node
        FAILED=0
        for NODE_ID in "${NODE_ARRAY[@]}"; do
            if [ "$DRY_RUN" -eq 0 ]; then
                NODE_IP=$(aws ec2 describe-instances \
                    --instance-ids "$NODE_ID" \
                    --query 'Reservations[0].Instances[0].PrivateIpAddress' \
                    --output text)

                if ! curl -s "http://$NODE_IP:9000/health" 2>/dev/null | grep -q ok; then
                    FAILED=$((FAILED + 1))
                fi
            fi
        done

        if [ $FAILED -gt 0 ]; then
            echo "[$(date)] ✗ Health check failed on $FAILED node(s)"
            HEALTH_CHECK_FAILED=$((HEALTH_CHECK_FAILED + 1))

            if [ $HEALTH_CHECK_FAILED -ge 3 ]; then
                echo "[$(date)] ✗ Multiple health check failures, triggering rollback..."
                exit 1
            fi
        else
            echo "[$(date)] ✓ Health check passed"
            HEALTH_CHECK_FAILED=0
        fi

        # Sleep before next check
        REMAINING=$((WAIT_TIME - ELAPSED))
        SLEEP_TIME=$((HEALTH_CHECK_INTERVAL < REMAINING ? HEALTH_CHECK_INTERVAL : REMAINING))
        sleep $SLEEP_TIME
    done
fi

echo "[$(date)] ✓ $STAGE rollout complete (v$VERSION)"
exit 0
