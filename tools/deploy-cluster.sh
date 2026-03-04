#!/usr/bin/env bash
# deploy-cluster.sh — Multi-node ClaudeFS deployment orchestrator
# Builds ClaudeFS on orchestrator, distributes to all cluster nodes, starts services
#
# Usage:
#   ./deploy-cluster.sh build                 # Build release binary
#   ./deploy-cluster.sh deploy [--skip-build] # Deploy to all nodes
#   ./deploy-cluster.sh deploy --node <name>  # Deploy to specific node
#   ./deploy-cluster.sh start-services        # Start services on all nodes
#   ./deploy-cluster.sh validate              # Validate deployment
#   ./deploy-cluster.sh rollback              # Rollback to previous version

set -euo pipefail

REGION="${AWS_REGION:-us-west-2}"
PROJECT_TAG="${PROJECT_TAG:-claudefs}"
REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEPLOY_DIR="${HOME}/.cfs/deployments"
BINARY_NAME="cfs"
RELEASE_BINARY="$REPO_DIR/target/release/$BINARY_NAME"

# Ensure we have proper SSH configuration
SSH_OPTIONS="-o StrictHostKeyChecking=no -o ConnectTimeout=10 -o BatchMode=yes"

# --- Logging ---

log_info() { echo "[INFO] $(date '+%Y-%m-%d %H:%M:%S') $*"; }
log_warn() { echo "[WARN] $(date '+%Y-%m-%d %H:%M:%S') $*" >&2; }
log_error() { echo "[ERROR] $(date '+%Y-%m-%d %H:%M:%S') $*" >&2; }
log_section() { echo ""; echo "==> $*"; }

# --- Initialization ---

init_deploy_dir() {
  mkdir -p "$DEPLOY_DIR/binaries"
  mkdir -p "$DEPLOY_DIR/configs"
  mkdir -p "$DEPLOY_DIR/logs"
  mkdir -p "$DEPLOY_DIR/rollback"
}

# --- Discovery ---

# Get all instances for deployment (excluding orchestrator)
get_all_nodes() {
  aws ec2 describe-instances \
    --filters \
      "Name=tag:project,Values=$PROJECT_TAG" \
      "Name=tag:role,Values!=orchestrator" \
      "Name=instance-state-name,Values=running" \
    --query 'Reservations[].Instances[*].[InstanceId,Tags[?Key==`Name`].Value|[0],Tags[?Key==`Role`].Value|[0],PublicIpAddress,Tags[?Key==`Site`].Value|[0]]' \
    --output json \
    --region "$REGION"
}

# Get instances by role
get_nodes_by_role() {
  local role="$1"
  aws ec2 describe-instances \
    --filters \
      "Name=tag:project,Values=$PROJECT_TAG" \
      "Name=tag:role,Values=$role" \
      "Name=instance-state-name,Values=running" \
    --query 'Reservations[].Instances[*].[InstanceId,Tags[?Key==`Name`].Value|[0],PublicIpAddress,Tags[?Key==`Site`].Value|[0]]' \
    --output json \
    --region "$REGION"
}

# --- Build ---

cmd_build() {
  log_section "Building ClaudeFS release binary"

  if [[ ! -d "$REPO_DIR" ]]; then
    log_error "Repository directory not found: $REPO_DIR"
    exit 1
  fi

  cd "$REPO_DIR"

  # Clean previous build
  log_info "Cleaning previous build artifacts"
  cargo clean 2>&1 | grep -v "^warning" || true

  # Build release
  log_info "Building release binary (this may take 10-30 minutes)"
  if ! cargo build --release 2>&1 | tail -50; then
    log_error "Build failed"
    exit 1
  fi

  if [[ ! -f "$RELEASE_BINARY" ]]; then
    log_error "Binary not found after build: $RELEASE_BINARY"
    exit 1
  fi

  log_info "✓ Build successful: $RELEASE_BINARY ($(du -h "$RELEASE_BINARY" | cut -f1))"

  # Copy to deployment directory
  local timestamp
  timestamp=$(date +%Y%m%d-%H%M%S)
  local binary_backup
  binary_backup="$DEPLOY_DIR/binaries/cfs-${timestamp}"
  cp "$RELEASE_BINARY" "$binary_backup"
  chmod +x "$binary_backup"

  log_info "Binary backed up to: $binary_backup"
}

# --- Distribution ---

deploy_to_node() {
  local node_name="$1"
  local node_ip="$2"
  local node_role="$3"
  local node_site="$4"

  log_info "Deploying to $node_name ($node_role@$node_site) at $node_ip"

  # Copy binary
  if ! scp $SSH_OPTIONS "$RELEASE_BINARY" "ubuntu@${node_ip}:/tmp/cfs-new" 2>&1 | tail -5; then
    log_warn "Failed to copy binary to $node_name"
    return 1
  fi

  # Validate binary
  if ! ssh $SSH_OPTIONS "ubuntu@${node_ip}" \
    "file /tmp/cfs-new | grep -q 'ELF' && echo 'Binary OK'" >/dev/null 2>&1; then
    log_warn "Binary validation failed on $node_name"
    return 1
  fi

  # Backup existing binary
  ssh $SSH_OPTIONS "ubuntu@${node_ip}" \
    "sudo mv /usr/local/bin/cfs /usr/local/bin/cfs-backup 2>/dev/null || true" \
    || true

  # Install new binary
  if ! ssh $SSH_OPTIONS "ubuntu@${node_ip}" \
    "sudo mv /tmp/cfs-new /usr/local/bin/cfs && sudo chmod +x /usr/local/bin/cfs"; then
    log_warn "Failed to install binary on $node_name"
    # Attempt rollback
    ssh $SSH_OPTIONS "ubuntu@${node_ip}" \
      "sudo mv /usr/local/bin/cfs-backup /usr/local/bin/cfs 2>/dev/null || true" \
      || true
    return 1
  fi

  log_info "✓ Deployed to $node_name"
  return 0
}

cmd_deploy() {
  local skip_build=false
  local target_node=""

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --skip-build)
        skip_build=true
        shift
        ;;
      --node)
        target_node="$2"
        shift 2
        ;;
      *)
        log_error "Unknown option: $1"
        exit 1
        ;;
    esac
  done

  # Build if needed
  if ! $skip_build; then
    cmd_build
  else
    if [[ ! -f "$RELEASE_BINARY" ]]; then
      log_error "Binary not found and --skip-build specified: $RELEASE_BINARY"
      exit 1
    fi
    log_info "Skipping build, using existing binary"
  fi

  init_deploy_dir

  log_section "Deploying to cluster nodes"

  if [[ -n "$target_node" ]]; then
    # Deploy to specific node
    log_info "Deploying to specific node: $target_node"
    local node_info
    node_info=$(get_all_nodes | jq ".[] | select(.[1] == \"$target_node\")" | head -1)

    if [[ -z "$node_info" ]]; then
      log_error "Node not found: $target_node"
      exit 1
    fi

    local node_ip node_role node_site
    node_ip=$(echo "$node_info" | jq -r '.[3]')
    node_role=$(echo "$node_info" | jq -r '.[2]')
    node_site=$(echo "$node_info" | jq -r '.[4] // "unknown"')

    deploy_to_node "$target_node" "$node_ip" "$node_role" "$node_site"
  else
    # Deploy to all nodes (storage first, then clients)
    local all_nodes
    all_nodes=$(get_all_nodes)

    local failed_nodes=()
    local success_count=0
    local total_count=0

    # Deploy in order: storage nodes first, then clients
    for role in storage conduit fuse nfs jepsen; do
      local nodes_for_role
      nodes_for_role=$(get_nodes_by_role "$role")

      if [[ -z "$nodes_for_role" ]] || [[ "$nodes_for_role" == "[]" ]]; then
        continue
      fi

      log_info "Deploying $role nodes..."

      echo "$nodes_for_role" | jq -r '.[]' | while read -r node_json; do
        local node_id node_name node_ip node_site
        node_id=$(echo "$node_json" | jq -r '.[0]')
        node_name=$(echo "$node_json" | jq -r '.[1]')
        node_ip=$(echo "$node_json" | jq -r '.[2]')
        node_site=$(echo "$node_json" | jq -r '.[3] // "unknown"')

        ((total_count++))

        if deploy_to_node "$node_name" "$node_ip" "$role" "$node_site"; then
          ((success_count++))
        else
          failed_nodes+=("$node_name")
        fi
      done
    done

    log_section "Deployment Summary"
    echo "Total nodes: $total_count"
    echo "Successful: $success_count"
    echo "Failed: $((total_count - success_count))"

    if (( total_count - success_count > 0 )); then
      log_warn "Deployment completed with failures on: ${failed_nodes[*]}"
      return 1
    fi
  fi

  log_info "Deployment complete!"
}

# --- Service Management ---

start_services_on_node() {
  local node_name="$1"
  local node_ip="$2"
  local node_role="$3"
  local node_site="$4"

  log_info "Starting services on $node_name ($node_role@$node_site)"

  # Determine service name based on role
  local service_name
  case "$node_role" in
    storage)
      service_name="cfs-server"
      ;;
    conduit)
      service_name="cfs-conduit"
      ;;
    fuse)
      service_name="cfs-fuse"
      ;;
    nfs)
      service_name="cfs-nfs"
      ;;
    jepsen)
      service_name="cfs-jepsen"
      ;;
    *)
      log_warn "Unknown role: $node_role"
      return 1
      ;;
  esac

  # Start service
  if ssh $SSH_OPTIONS "ubuntu@${node_ip}" \
    "sudo systemctl restart $service_name 2>/dev/null"; then
    log_info "✓ $service_name started"
    return 0
  else
    log_warn "Failed to start $service_name"
    return 1
  fi
}

cmd_start_services() {
  log_section "Starting services on all nodes"

  local all_nodes
  all_nodes=$(get_all_nodes)

  # Start storage nodes first
  echo "$all_nodes" | jq -r '.[] | select(.[2]=="storage")' | while read -r node_json; do
    local node_name node_ip node_role node_site
    node_name=$(echo "$node_json" | jq -r '.[1]')
    node_ip=$(echo "$node_json" | jq -r '.[3]')
    node_role=$(echo "$node_json" | jq -r '.[2]')
    node_site=$(echo "$node_json" | jq -r '.[4]')

    start_services_on_node "$node_name" "$node_ip" "$node_role" "$node_site"
  done

  # Wait for storage nodes to stabilize
  log_info "Waiting 10 seconds for storage nodes to stabilize"
  sleep 10

  # Start client and service nodes
  echo "$all_nodes" | jq -r '.[] | select(.[2]!="storage")' | while read -r node_json; do
    local node_name node_ip node_role node_site
    node_name=$(echo "$node_json" | jq -r '.[1]')
    node_ip=$(echo "$node_json" | jq -r '.[3]')
    node_role=$(echo "$node_json" | jq -r '.[2]')
    node_site=$(echo "$node_json" | jq -r '.[4]')

    start_services_on_node "$node_name" "$node_ip" "$node_role" "$node_site"
  done

  log_info "Services started"
}

# --- Validation ---

cmd_validate() {
  log_section "Validating deployment"

  local all_nodes
  all_nodes=$(get_all_nodes)

  local failures=0

  echo "$all_nodes" | jq -r '.[]' | while read -r node_json; do
    local node_name node_ip node_role
    node_name=$(echo "$node_json" | jq -r '.[1]')
    node_ip=$(echo "$node_json" | jq -r '.[3]')
    node_role=$(echo "$node_json" | jq -r '.[2]')

    # Check binary exists and is executable
    if ! ssh $SSH_OPTIONS "ubuntu@${node_ip}" \
      "test -x /usr/local/bin/cfs && /usr/local/bin/cfs --version >/dev/null 2>&1"; then
      log_warn "✗ $node_name: Binary not valid"
      ((failures++))
      return
    fi

    # Check service is running
    local service_name
    case "$node_role" in
      storage) service_name="cfs-server" ;;
      *) service_name="cfs-$node_role" ;;
    esac

    if ssh $SSH_OPTIONS "ubuntu@${node_ip}" \
      "sudo systemctl is-active $service_name >/dev/null 2>&1"; then
      echo "✓ $node_name: OK (binary + service running)"
    else
      log_warn "✗ $node_name: Service not running"
      ((failures++))
    fi
  done

  if (( failures == 0 )); then
    log_info "✓ All nodes validated successfully"
    return 0
  else
    log_error "Validation failed: $failures node(s) have issues"
    return 1
  fi
}

# --- Rollback ---

cmd_rollback() {
  log_section "Rolling back to previous version"

  local all_nodes
  all_nodes=$(get_all_nodes)

  log_info "Restoring binaries from backup"

  echo "$all_nodes" | jq -r '.[]' | while read -r node_json; do
    local node_name node_ip
    node_name=$(echo "$node_json" | jq -r '.[1]')
    node_ip=$(echo "$node_json" | jq -r '.[3]')

    log_info "Rolling back $node_name"

    if ssh $SSH_OPTIONS "ubuntu@${node_ip}" \
      "sudo mv /usr/local/bin/cfs-backup /usr/local/bin/cfs 2>/dev/null"; then
      log_info "✓ $node_name rolled back"
      # Restart service
      ssh $SSH_OPTIONS "ubuntu@${node_ip}" \
        "sudo systemctl restart cfs-server 2>/dev/null || sudo systemctl restart cfs-fuse 2>/dev/null || true"
    else
      log_warn "✗ Failed to rollback $node_name"
    fi
  done

  log_info "Rollback complete"
}

# --- Main ---

main() {
  init_deploy_dir

  local cmd="${1:-help}"
  case "$cmd" in
    build)
      cmd_build
      ;;
    deploy)
      shift || true
      cmd_deploy "$@"
      ;;
    start-services)
      cmd_start_services
      ;;
    validate)
      cmd_validate
      ;;
    rollback)
      cmd_rollback
      ;;
    help)
      cat << 'EOF'
deploy-cluster.sh — Multi-node ClaudeFS Deployment Orchestrator

Usage:
  ./deploy-cluster.sh build                 # Build release binary
  ./deploy-cluster.sh deploy [--skip-build] # Deploy to all nodes
  ./deploy-cluster.sh deploy --node <name>  # Deploy to specific node
  ./deploy-cluster.sh start-services        # Start services on all nodes
  ./deploy-cluster.sh validate              # Validate deployment
  ./deploy-cluster.sh rollback              # Rollback to previous version

Commands:
  build            Compile ClaudeFS release binary (10-30 min)
  deploy           Build and deploy to all cluster nodes (with validation)
  start-services   Start cfs services on all nodes (storage first)
  validate         Check that binaries are installed and services running
  rollback         Restore previous binary version from backup

Options:
  --skip-build     Skip building; deploy existing release binary
  --node <name>    Deploy to specific node only

Deployment Order:
  1. Storage nodes (Site A, then Site B)
  2. Conduit nodes
  3. Client nodes (FUSE, NFS)
  4. Jepsen controller

Rollback:
  - Previous binary saved as /usr/local/bin/cfs-backup on each node
  - 'rollback' command restores backup and restarts services

Environment Variables:
  AWS_REGION  AWS region (default: us-west-2)
  PROJECT_TAG Project tag for resource filtering (default: claudefs)

Examples:
  # Full cycle: build and deploy to all nodes
  ./deploy-cluster.sh deploy

  # Deploy without rebuild (faster iteration)
  ./deploy-cluster.sh deploy --skip-build

  # Deploy to specific node
  ./deploy-cluster.sh deploy --node storage-a-1

  # Just start services after manual config changes
  ./deploy-cluster.sh start-services

  # Validate current deployment
  ./deploy-cluster.sh validate

  # Emergency rollback
  ./deploy-cluster.sh rollback
EOF
      ;;
    *)
      log_error "Unknown command: $cmd"
      echo "Run './deploy-cluster.sh help' for usage"
      exit 1
      ;;
  esac
}

main "$@"
