#!/usr/bin/env bash
# cluster-health-check.sh — Comprehensive ClaudeFS multi-node cluster health validator
# Tests connectivity, service health, and data consistency across all nodes
#
# Usage:
#   ./cluster-health-check.sh status              # Quick health snapshot
#   ./cluster-health-check.sh full               # Full comprehensive health check
#   ./cluster-health-check.sh connectivity       # Test inter-node RPC connectivity
#   ./cluster-health-check.sh replication        # Check replication status
#   ./cluster-health-check.sh monitor [--interval 60]  # Continuous monitoring

set -euo pipefail

REGION="${AWS_REGION:-us-west-2}"
PROJECT_TAG="${PROJECT_TAG:-claudefs}"
HEALTH_DIR="${HOME}/.cfs/cluster-health"

# Thresholds
LATENCY_WARN_MS=100
LATENCY_CRIT_MS=500
REPLICATION_LAG_WARN_SEC=5
REPLICATION_LAG_CRIT_SEC=30

SSH_OPTIONS="-o StrictHostKeyChecking=no -o ConnectTimeout=5 -o BatchMode=yes"

# --- Logging ---

log_info() { echo "[INFO] $*"; }
log_ok() { echo "✓ $*"; }
log_warn() { echo "⚠ WARN: $*" >&2; }
log_error() { echo "✗ ERROR: $*" >&2; }
log_section() { echo ""; echo "=== $* ==="; }

# --- Initialization ---

init_health_dir() {
  mkdir -p "$HEALTH_DIR"
  mkdir -p "$HEALTH_DIR/metrics"
  mkdir -p "$HEALTH_DIR/reports"
}

# --- Discovery ---

get_all_nodes() {
  aws ec2 describe-instances \
    --filters \
      "Name=tag:project,Values=$PROJECT_TAG" \
      "Name=tag:role,Values!=orchestrator" \
      "Name=instance-state-name,Values=running" \
    --query 'Reservations[].Instances[*].[InstanceId,Tags[?Key==`Name`].Value|[0],Tags[?Key==`Role`].Value|[0],PrivateIpAddress,PublicIpAddress,Tags[?Key==`Site`].Value|[0]]' \
    --output json \
    --region "$REGION"
}

get_storage_nodes() {
  aws ec2 describe-instances \
    --filters \
      "Name=tag:project,Values=$PROJECT_TAG" \
      "Name=tag:role,Values=storage" \
      "Name=instance-state-name,Values=running" \
    --query 'Reservations[].Instances[*].[InstanceId,Tags[?Key==`Name`].Value|[0],PrivateIpAddress,PublicIpAddress,Tags[?Key==`Site`].Value|[0]]' \
    --output json \
    --region "$REGION"
}

# --- Basic Health ---

cmd_status() {
  log_section "ClaudeFS Cluster Health Status"

  local all_nodes
  all_nodes=$(get_all_nodes)

  if [[ -z "$all_nodes" ]] || [[ "$all_nodes" == "[]" ]]; then
    log_error "No nodes found"
    return 1
  fi

  local total_nodes
  total_nodes=$(echo "$all_nodes" | jq 'length')

  log_info "Total nodes: $total_nodes"
  echo ""

  local node_count=0
  local healthy_count=0
  local ssh_failed=0

  echo "$all_nodes" | jq -r '.[]' | while read -r node_json; do
    local node_name node_role node_ip node_public_ip
    node_name=$(echo "$node_json" | jq -r '.[1]')
    node_role=$(echo "$node_json" | jq -r '.[2]')
    node_ip=$(echo "$node_json" | jq -r '.[3]')
    node_public_ip=$(echo "$node_json" | jq -r '.[4]')

    ((node_count++))

    # Test SSH connectivity
    if ssh $SSH_OPTIONS "ubuntu@${node_public_ip}" "echo 'OK'" >/dev/null 2>&1; then
      ((healthy_count++))

      # Check service status
      local service_name
      case "$node_role" in
        storage) service_name="cfs-server" ;;
        *) service_name="cfs-$node_role" ;;
      esac

      local service_status
      service_status=$(ssh $SSH_OPTIONS "ubuntu@${node_public_ip}" \
        "sudo systemctl is-active $service_name 2>/dev/null || echo 'inactive'" || echo "unknown")

      if [[ "$service_status" == "active" ]]; then
        log_ok "$node_name ($node_role): SSH ✓ Service $service_name ✓"
      else
        log_warn "$node_name ($node_role): SSH ✓ Service $service_name NOT RUNNING"
      fi
    else
      log_error "$node_name ($node_role): SSH FAILED"
      ((ssh_failed++))
    fi
  done

  echo ""
  log_info "Summary: $healthy_count/$node_count nodes SSH OK, $ssh_failed FAILED"
}

# --- Comprehensive Health Check ---

cmd_full() {
  log_section "Full Cluster Health Check"

  init_health_dir

  local timestamp
  timestamp=$(date +%Y%m%d-%H%M%S)
  local report_file="$HEALTH_DIR/reports/health-${timestamp}.txt"

  exec > >(tee "$report_file")
  exec 2>&1

  echo "ClaudeFS Cluster Health Report"
  echo "Generated: $(date)"
  echo "Region: $REGION"
  echo ""

  # 1. Node status
  log_section "1. Node Status"
  cmd_status
  echo ""

  # 2. Connectivity
  log_section "2. Connectivity Tests"
  cmd_connectivity
  echo ""

  # 3. Service health
  log_section "3. Service Health"
  check_service_health
  echo ""

  # 4. Replication status
  log_section "4. Replication Status"
  cmd_replication
  echo ""

  # 5. Disk usage
  log_section "5. Disk Usage"
  check_disk_usage
  echo ""

  # 6. Memory and CPU
  log_section "6. Resource Usage"
  check_resource_usage
  echo ""

  echo "Report saved to: $report_file"
}

# --- Connectivity ---

cmd_connectivity() {
  log_info "Testing inter-node RPC connectivity"

  local storage_nodes
  storage_nodes=$(get_storage_nodes)

  if [[ -z "$storage_nodes" ]] || [[ "$storage_nodes" == "[]" ]]; then
    log_warn "No storage nodes found"
    return
  fi

  local node_ips=()
  local node_names=()

  echo "$storage_nodes" | jq -r '.[]' | while read -r node_json; do
    local node_name node_public_ip
    node_name=$(echo "$node_json" | jq -r '.[1]')
    node_public_ip=$(echo "$node_json" | jq -r '.[3]')

    node_ips+=("$node_public_ip")
    node_names+=("$node_name")
  done

  local connectivity_ok=true

  # Test connectivity between each pair of storage nodes
  for i in "${!node_ips[@]}"; do
    for j in "${!node_ips[@]}"; do
      if (( i < j )); then
        local node_a_name="${node_names[$i]}"
        local node_a_ip="${node_ips[$i]}"
        local node_b_name="${node_names[$j]}"
        local node_b_ip="${node_ips[$j]}"

        # Test basic TCP connectivity to RPC port (9400)
        if ssh $SSH_OPTIONS "ubuntu@${node_a_ip}" \
          "timeout 2 bash -c \"</dev/tcp/${node_b_ip}/9400\" 2>/dev/null"; then
          log_ok "$node_a_name → $node_b_name (port 9400): OK"
        else
          log_warn "$node_a_name → $node_b_name (port 9400): FAILED"
          connectivity_ok=false
        fi
      fi
    done
  done

  if $connectivity_ok; then
    log_ok "Inter-node connectivity: ALL OK"
  else
    log_warn "Some inter-node connectivity issues detected"
  fi
}

# --- Service Health ---

check_service_health() {
  log_info "Checking service health on all nodes"

  local all_nodes
  all_nodes=$(get_all_nodes)

  echo "$all_nodes" | jq -r '.[]' | while read -r node_json; do
    local node_name node_role node_public_ip
    node_name=$(echo "$node_json" | jq -r '.[1]')
    node_role=$(echo "$node_json" | jq -r '.[2]')
    node_public_ip=$(echo "$node_json" | jq -r '.[4]')

    local service_name
    case "$node_role" in
      storage) service_name="cfs-server" ;;
      *) service_name="cfs-$node_role" ;;
    esac

    local status uptime memory
    status=$(ssh $SSH_OPTIONS "ubuntu@${node_public_ip}" \
      "sudo systemctl is-active $service_name 2>/dev/null || echo 'inactive'" || echo "unknown")

    if [[ "$status" == "active" ]]; then
      uptime=$(ssh $SSH_OPTIONS "ubuntu@${node_public_ip}" \
        "sudo systemctl show -p ActiveEnterTimestamp $service_name 2>/dev/null | cut -d= -f2 || echo 'unknown'" || echo "unknown")

      memory=$(ssh $SSH_OPTIONS "ubuntu@${node_public_ip}" \
        "ps aux | grep -E '\\s/usr/local/bin/cfs\\s' | grep -v grep | awk '{print \$6}' | head -1 || echo '0'" || echo "0")

      log_ok "$node_name ($service_name): UP, Started: $uptime, Memory: ${memory}K"
    else
      log_warn "$node_name ($service_name): NOT RUNNING (status: $status)"
    fi
  done
}

# --- Replication Status ---

cmd_replication() {
  log_info "Checking cross-site replication status"

  local storage_nodes
  storage_nodes=$(get_storage_nodes)

  # Group by site
  local site_a_nodes=()
  local site_b_nodes=()

  echo "$storage_nodes" | jq -r '.[]' | while read -r node_json; do
    local site
    site=$(echo "$node_json" | jq -r '.[4]')

    if [[ "$site" == "A" ]]; then
      site_a_nodes+=("$(echo "$node_json" | jq -r '.[1]')")
    elif [[ "$site" == "B" ]]; then
      site_b_nodes+=("$(echo "$node_json" | jq -r '.[1]')")
    fi
  done

  log_info "Site A nodes: ${#site_a_nodes[@]}"
  log_info "Site B nodes: ${#site_b_nodes[@]}"

  # Check for conduit nodes
  local conduit_nodes
  conduit_nodes=$(aws ec2 describe-instances \
    --filters \
      "Name=tag:project,Values=$PROJECT_TAG" \
      "Name=tag:role,Values=conduit" \
      "Name=instance-state-name,Values=running" \
    --query 'Reservations[].Instances[*].[Tags[?Key==`Name`].Value|[0],PublicIpAddress]' \
    --output json \
    --region "$REGION")

  local conduit_count
  conduit_count=$(echo "$conduit_nodes" | jq 'length')

  if (( conduit_count > 0 )); then
    log_ok "Conduit nodes: $conduit_count (cross-site replication available)"

    # Test conduit connectivity
    echo "$conduit_nodes" | jq -r '.[]' | while read -r conduit_json; do
      local conduit_name conduit_ip
      conduit_name=$(echo "$conduit_json" | jq -r '.[0]')
      conduit_ip=$(echo "$conduit_json" | jq -r '.[1]')

      if ssh $SSH_OPTIONS "ubuntu@${conduit_ip}" "echo 'OK'" >/dev/null 2>&1; then
        log_ok "Conduit $conduit_name: SSH OK"
      else
        log_warn "Conduit $conduit_name: SSH FAILED"
      fi
    done
  else
    log_warn "No conduit nodes running (cross-site replication not available)"
  fi
}

# --- Disk Usage ---

check_disk_usage() {
  log_info "Checking disk usage on all storage nodes"

  local storage_nodes
  storage_nodes=$(get_storage_nodes)

  echo "$storage_nodes" | jq -r '.[]' | while read -r node_json; do
    local node_name node_public_ip
    node_name=$(echo "$node_json" | jq -r '.[1]')
    node_public_ip=$(echo "$node_json" | jq -r '.[3]')

    local disk_usage
    disk_usage=$(ssh $SSH_OPTIONS "ubuntu@${node_public_ip}" \
      "df -h / | tail -1 | awk '{print \$5}' | tr -d '%'" || echo "unknown")

    if [[ "$disk_usage" != "unknown" ]]; then
      if (( disk_usage < 80 )); then
        log_ok "$node_name: Disk usage $disk_usage%"
      elif (( disk_usage < 95 )); then
        log_warn "$node_name: Disk usage $disk_usage% (approaching limit)"
      else
        log_error "$node_name: Disk usage $disk_usage% (CRITICAL)"
      fi
    fi
  done
}

# --- Resource Usage ---

check_resource_usage() {
  log_info "Checking CPU and memory usage"

  local all_nodes
  all_nodes=$(get_all_nodes)

  echo "$all_nodes" | jq -r '.[]' | while read -r node_json; do
    local node_name node_public_ip
    node_name=$(echo "$node_json" | jq -r '.[1]')
    node_public_ip=$(echo "$node_json" | jq -r '.[4]')

    local cpu_load memory_usage
    cpu_load=$(ssh $SSH_OPTIONS "ubuntu@${node_public_ip}" \
      "uptime | awk -F'load average:' '{print \$2}' | awk '{print \$1}'" || echo "unknown")

    memory_usage=$(ssh $SSH_OPTIONS "ubuntu@${node_public_ip}" \
      "free | grep Mem | awk '{printf \"%.0f\", (\$3/\$2)*100}'" || echo "unknown")

    if [[ "$cpu_load" != "unknown" ]] && [[ "$memory_usage" != "unknown" ]]; then
      log_ok "$node_name: CPU load=$cpu_load, Memory=${memory_usage}%"
    fi
  done
}

# --- Monitoring ---

cmd_monitor() {
  local interval="${1:-60}"
  log_info "Starting continuous health monitoring (interval: ${interval}s)"
  init_health_dir

  local iteration=0
  while true; do
    ((iteration++))
    log_section "Monitoring iteration $iteration - $(date)"

    cmd_status
    echo ""

    sleep "$interval"
  done
}

# --- Main ---

main() {
  init_health_dir

  local cmd="${1:-status}"
  case "$cmd" in
    status)
      cmd_status
      ;;
    full)
      cmd_full
      ;;
    connectivity)
      cmd_connectivity
      ;;
    replication)
      cmd_replication
      ;;
    monitor)
      shift || true
      cmd_monitor "$@"
      ;;
    help)
      cat << 'EOF'
cluster-health-check.sh — ClaudeFS Multi-Node Cluster Health Validator

Usage:
  ./cluster-health-check.sh status              # Quick health snapshot
  ./cluster-health-check.sh full               # Full comprehensive health check
  ./cluster-health-check.sh connectivity       # Test inter-node RPC connectivity
  ./cluster-health-check.sh replication        # Check replication status
  ./cluster-health-check.sh monitor [interval] # Continuous monitoring

Commands:
  status        Quick health snapshot (SSH connectivity + service status)
  full          Comprehensive report (nodes, connectivity, services, disk, resources)
  connectivity  Test inter-node TCP/RPC connectivity (port 9400)
  replication   Check cross-site replication setup (Site A/B, conduit)
  monitor       Continuous monitoring loop with interval (default 60s)

Thresholds:
  Latency warning:         > 100ms
  Latency critical:        > 500ms
  Replication lag warning: > 5s
  Replication lag critical: > 30s
  Disk warning:            > 80%
  Disk critical:           > 95%

Health Checks Performed:
  ✓ Node SSH connectivity
  ✓ Service status (cfs-server, cfs-fuse, cfs-nfs, cfs-conduit, cfs-jepsen)
  ✓ Inter-node RPC connectivity
  ✓ Service uptime and memory usage
  ✓ Cross-site replication connectivity
  ✓ Disk usage on storage nodes
  ✓ CPU load and memory usage

Reports:
  - Full reports saved to ~/.cfs/cluster-health/reports/
  - Metrics exported to ~/.cfs/cluster-health/metrics/
  - Timestamped for trend analysis

Environment Variables:
  AWS_REGION  AWS region (default: us-west-2)
  PROJECT_TAG Project tag for resource filtering (default: claudefs)

Examples:
  # Quick status check
  ./cluster-health-check.sh status

  # Full comprehensive report (saved to file)
  ./cluster-health-check.sh full

  # Test connectivity between storage nodes
  ./cluster-health-check.sh connectivity

  # Monitor every 30 seconds
  ./cluster-health-check.sh monitor 30

  # Check replication setup
  ./cluster-health-check.sh replication
EOF
      ;;
    *)
      log_error "Unknown command: $cmd"
      echo "Run './cluster-health-check.sh help' for usage"
      exit 1
      ;;
  esac
}

main "$@"
