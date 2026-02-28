#!/usr/bin/env bash
# client-node-user-data.sh — Cloud-init for ClaudeFS client spot nodes (c7a.xlarge)
# Used for FUSE clients, NFS/SMB clients, and Jepsen controller

set -euo pipefail
exec > >(tee /var/log/cfs-node-bootstrap.log) 2>&1
echo "=== ClaudeFS client node bootstrap started at $(date -u) ==="

REGION="us-west-2"

# --- System packages ---
apt-get update -y
apt-get install -y \
  build-essential pkg-config libssl-dev \
  fuse3 libfuse3-dev \
  fio \
  awscli jq curl wget \
  nfs-common cifs-utils samba-client \
  linux-tools-common linux-tools-$(uname -r) || true

# --- POSIX test tools ---
apt-get install -y \
  autoconf automake libtool \
  xfsprogs xfsdump \
  python3 python3-pip \
  clojure leiningen || true

# --- Create cfs user ---
useradd -m -s /bin/bash cfs || true

# --- Mount points for testing ---
mkdir -p /mnt/cfs-fuse /mnt/cfs-nfs /mnt/cfs-smb
chown cfs:cfs /mnt/cfs-fuse /mnt/cfs-nfs /mnt/cfs-smb

# --- Tag self ---
INSTANCE_ID=$(curl -s http://169.254.169.254/latest/meta-data/instance-id)
# Determine role from instance tag (set by orchestrator at launch)
ROLE=$(aws ec2 describe-tags \
  --filters "Name=resource-id,Values=$INSTANCE_ID" "Name=key,Values=role" \
  --query 'Tags[0].Value' --output text --region "$REGION" 2>/dev/null || echo "client")
aws ec2 create-tags --resources "$INSTANCE_ID" --tags \
  Key=Name,Value="cfs-${ROLE}" \
  Key=project,Value=claudefs \
  --region "$REGION"

# --- Kernel tuning for client ---
cat >> /etc/sysctl.d/99-cfs-client.conf << 'EOF'
kernel.io_uring_disabled = 0
fs.file-max = 1048576
net.core.rmem_max = 16777216
net.core.wmem_max = 16777216
vm.swappiness = 10
EOF
sysctl --system

echo "=== ClaudeFS client node bootstrap completed at $(date -u) ==="
echo "READY" > /tmp/cfs-bootstrap-complete
