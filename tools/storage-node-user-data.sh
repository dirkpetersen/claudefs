#!/usr/bin/env bash
# storage-node-user-data.sh — Cloud-init for ClaudeFS storage spot nodes (i4i.2xlarge)
# Sets up NVMe drives, installs minimal tooling for running compiled ClaudeFS binaries

set -euo pipefail
exec > >(tee /var/log/cfs-node-bootstrap.log) 2>&1
echo "=== ClaudeFS storage node bootstrap started at $(date -u) ==="

REGION="us-west-2"

# --- System packages ---
apt-get update -y
apt-get install -y \
  build-essential pkg-config libssl-dev \
  fuse3 libfuse3-dev \
  nvme-cli fio \
  awscli jq curl wget \
  nfs-common nfs-kernel-server \
  linux-tools-common linux-tools-$(uname -r) || true

# --- Format and mount NVMe instance storage ---
# i4i.2xlarge has 1x 1875 GB NVMe SSD
NVME_DEVICES=$(nvme list -o json 2>/dev/null | jq -r '.Devices[] | select(.ModelNumber | contains("Instance Storage") or contains("Amazon EC2 NVMe")) | .DevicePath' || true)

if [[ -z "$NVME_DEVICES" ]]; then
  # Fallback: find non-root NVMe devices
  NVME_DEVICES=$(lsblk -d -n -o NAME,TYPE | awk '$2=="disk" && $1~/nvme/ {print "/dev/"$1}' | grep -v "$(findmnt -n -o SOURCE / | sed 's/p[0-9]*$//' | xargs basename)" || true)
fi

MOUNT_IDX=0
for DEV in $NVME_DEVICES; do
  MOUNT_POINT="/data/nvme${MOUNT_IDX}"
  mkdir -p "$MOUNT_POINT"
  # Format with ext4, disable journaling for performance (data is replicated)
  mkfs.ext4 -F -E nodiscard -O ^has_journal "$DEV" || continue
  mount -o noatime,nodiratime,discard "$DEV" "$MOUNT_POINT"
  echo "$DEV $MOUNT_POINT ext4 noatime,nodiratime,discard 0 0" >> /etc/fstab
  MOUNT_IDX=$((MOUNT_IDX + 1))
done

echo "Mounted $MOUNT_IDX NVMe device(s)"

# --- Create cfs user ---
useradd -m -s /bin/bash cfs || true
chown -R cfs:cfs /data/

# --- Tag self ---
INSTANCE_ID=$(curl -s http://169.254.169.254/latest/meta-data/instance-id)
NODE_INDEX=$(aws ec2 describe-instances \
  --filters "Name=tag:project,Values=claudefs" "Name=tag:role,Values=storage" "Name=instance-state-name,Values=running" \
  --query 'length(Reservations[].Instances[])' --output text --region "$REGION" 2>/dev/null || echo "0")
aws ec2 create-tags --resources "$INSTANCE_ID" --tags \
  Key=Name,Value="cfs-storage-${NODE_INDEX}" \
  Key=project,Value=claudefs \
  Key=role,Value=storage \
  --region "$REGION"

# --- Kernel tuning for storage ---
cat >> /etc/sysctl.d/99-cfs-storage.conf << 'EOF'
# Increase io_uring limits
kernel.io_uring_disabled = 0
# Increase max open files
fs.file-max = 1048576
# Network buffers for RDMA/TCP transport
net.core.rmem_max = 16777216
net.core.wmem_max = 16777216
net.core.rmem_default = 1048576
net.core.wmem_default = 1048576
# VM tuning
vm.dirty_ratio = 40
vm.dirty_background_ratio = 10
vm.swappiness = 10
EOF
sysctl --system

echo "=== ClaudeFS storage node bootstrap completed at $(date -u) ==="
echo "READY" > /tmp/cfs-bootstrap-complete
