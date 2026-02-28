#!/usr/bin/env bash
# orchestrator-user-data.sh — Cloud-init for ClaudeFS orchestrator node
# Base AMI: Ubuntu 25.10 Questing (kernel 6.17+)
# Installs: Rust 1.93, Node.js 22, Claude Code, OpenCode, GitHub CLI, cargo tools
# Retrieves secrets, clones repo, configures Claude Code for Bedrock

set -euo pipefail
exec > >(tee /var/log/cfs-bootstrap.log) 2>&1
echo "=== ClaudeFS orchestrator bootstrap started at $(date -u) ==="

REGION="us-west-2"
ACCOUNT_ID="405644541454"
REPO="dirkpetersen/claudefs"

# --- System packages ---
apt-get update -y
apt-get install -y \
  build-essential pkg-config libssl-dev libclang-dev \
  cmake protobuf-compiler \
  git curl wget unzip jq tmux htop \
  fuse3 libfuse3-dev \
  awscli \
  nfs-common \
  linux-tools-common linux-tools-$(uname -r) || true

# --- Create cfs user ---
useradd -m -s /bin/bash cfs || true
usermod -aG sudo cfs
echo "cfs ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/cfs

# --- Retrieve secrets ---
GITHUB_TOKEN=$(aws secretsmanager get-secret-value \
  --secret-id cfs/github-token --region "$REGION" \
  --query 'SecretString' --output text | jq -r .GITHUB_TOKEN)

SSH_KEY=$(aws secretsmanager get-secret-value \
  --secret-id cfs/ssh-private-key --region "$REGION" \
  --query 'SecretString' --output text)

FIREWORKS_API_KEY=$(aws secretsmanager get-secret-value \
  --secret-id cfs/fireworks-api-key --region "$REGION" \
  --query 'SecretString' --output text | jq -r .FIREWORKS_API_KEY)

# Set up SSH key for cfs user
sudo -u cfs mkdir -p /home/cfs/.ssh
echo "$SSH_KEY" > /home/cfs/.ssh/id_ed25519
chmod 600 /home/cfs/.ssh/id_ed25519
chown cfs:cfs /home/cfs/.ssh/id_ed25519
cat >> /home/cfs/.ssh/config << 'SSHEOF'
Host github.com
  IdentityFile ~/.ssh/id_ed25519
  StrictHostKeyChecking no
SSHEOF
chown -R cfs:cfs /home/cfs/.ssh
chmod 600 /home/cfs/.ssh/config

# --- Install Rust (as cfs user) ---
sudo -u cfs bash -c 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y'
sudo -u cfs bash -c 'source ~/.cargo/env && rustup default stable'
sudo -u cfs bash -c 'source ~/.cargo/env && rustup component add clippy rustfmt'

# --- Install cargo tools ---
sudo -u cfs bash -c 'source ~/.cargo/env && cargo install cargo-audit cargo-deny' || true
# nextest: install from pre-built binary (faster, avoids compilation issues)
sudo -u cfs bash -c 'curl -LsSf https://get.nexte.st/latest/linux | tar zxf - -C ~/.cargo/bin' || true

# --- Install Node.js 22 ---
curl -fsSL https://deb.nodesource.com/setup_22.x | bash -
apt-get install -y nodejs
corepack enable

# --- Install Claude Code ---
npm install -g @anthropic-ai/claude-code

# --- Install OpenCode (Rust code authoring via Fireworks AI) ---
sudo -u cfs bash -c 'curl -fsSL https://opencode.ai/install | bash'

# --- Install GitHub CLI ---
curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg \
  | dd of=/usr/share/keyrings/githubcli-archive-keyring.gpg
echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" \
  > /etc/apt/sources.list.d/github-cli.list
apt-get update -y
apt-get install -y gh

# --- Configure GitHub CLI ---
sudo -u cfs bash -c "echo '$GITHUB_TOKEN' | gh auth login --with-token"

# --- Clone repo ---
sudo -u cfs bash -c "git clone https://${GITHUB_TOKEN}@github.com/${REPO}.git /home/cfs/claudefs"
sudo -u cfs bash -c "cd /home/cfs/claudefs && git config user.name 'ClaudeFS Agent' && git config user.email 'claudefs-agent@noreply.github.com'"

# --- Configure Claude Code for Bedrock ---
sudo -u cfs mkdir -p /home/cfs/.config/claude-code
cat > /home/cfs/.config/claude-code/settings.json << 'CCEOF'
{
  "provider": "bedrock",
  "bedrockRegion": "us-west-2",
  "bedrockModelId": "global.anthropic.claude-sonnet-4-6-v1",
  "permissions": {
    "allow": [
      "Bash(*)",
      "Read(*)",
      "Write(*)",
      "Edit(*)",
      "Glob(*)",
      "Grep(*)"
    ]
  }
}
CCEOF
chown -R cfs:cfs /home/cfs/.config

# --- Environment variables for Claude Code / Bedrock ---
cat >> /home/cfs/.bashrc << ENVEOF
# ClaudeFS development environment
export AWS_REGION=us-west-2
export AWS_DEFAULT_REGION=us-west-2
export ANTHROPIC_MODEL=global.anthropic.claude-sonnet-4-6-v1
export CLAUDE_CODE_USE_BEDROCK=1
export DISABLE_PROMPT_CACHING=0
export FIREWORKS_API_KEY=${FIREWORKS_API_KEY}
export PATH="\$HOME/.opencode/bin:\$PATH"
source ~/.cargo/env
ENVEOF

# --- Install cost monitor cron job ---
cp /home/cfs/claudefs/tools/cfs-cost-monitor.sh /opt/cfs-cost-monitor.sh
chmod +x /opt/cfs-cost-monitor.sh
echo "*/15 * * * * root /opt/cfs-cost-monitor.sh" > /etc/cron.d/cfs-cost-monitor

# --- Create agent log directory ---
mkdir -p /var/log/cfs-agents
chown cfs:cfs /var/log/cfs-agents

# --- Install agent launcher ---
cp /home/cfs/claudefs/tools/cfs-agent-launcher.sh /opt/cfs-agent-launcher.sh
chmod +x /opt/cfs-agent-launcher.sh

# --- Tag self (IMDSv2) ---
IMDS_TOKEN=$(curl -s -X PUT "http://169.254.169.254/latest/api/token" -H "X-aws-ec2-metadata-token-ttl-seconds: 300")
INSTANCE_ID=$(curl -s -H "X-aws-ec2-metadata-token: $IMDS_TOKEN" http://169.254.169.254/latest/meta-data/instance-id)
aws ec2 create-tags --resources "$INSTANCE_ID" --tags \
  Key=Name,Value=cfs-orchestrator \
  Key=project,Value=claudefs \
  Key=role,Value=orchestrator \
  --region "$REGION" || echo "WARNING: Failed to tag instance (non-fatal)"

echo "=== ClaudeFS orchestrator bootstrap completed at $(date -u) ==="
echo "READY" > /tmp/cfs-bootstrap-complete
