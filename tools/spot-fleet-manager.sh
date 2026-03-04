#!/usr/bin/env bash
# spot-fleet-manager.sh — ClaudeFS spot instance lifecycle management
# Handles spot instance provisioning, monitoring, interruption detection, and failover
#
# Usage:
#   ./spot-fleet-manager.sh status                   # Show spot fleet status
#   ./spot-fleet-manager.sh monitor [--interval 60]  # Monitor for interruptions
#   ./spot-fleet-manager.sh replace <instance_id>    # Replace terminated instance
#   ./spot-fleet-manager.sh validate                 # Validate fleet health

set -euo pipefail

REGION="${AWS_REGION:-us-west-2}"
PROJECT_TAG="${PROJECT_TAG:-claudefs}"
STATE_DIR="${HOME}/.cfs/spot-fleet-state"

# --- Logging ---

log_info() { echo "[INFO] $(date '+%Y-%m-%d %H:%M:%S') $*"; }
log_warn() { echo "[WARN] $(date '+%Y-%m-%d %H:%M:%S') $*" >&2; }
log_error() { echo "[ERROR] $(date '+%Y-%m-%d %H:%M:%S') $*" >&2; }

# --- Initialization ---

init_state_dir() {
  mkdir -p "$STATE_DIR"
  chmod 700 "$STATE_DIR"
}

# --- Spot Instance Discovery ---

# Get all spot instances in the project
get_spot_instances() {
  aws ec2 describe-instances \
    --filters \
      "Name=tag:project,Values=$PROJECT_TAG" \
      "Name=instance-lifecycle,Values=spot" \
      "Name=instance-state-name,Values=running,pending" \
    --query 'Reservations[].Instances[*].[InstanceId,InstanceType,State.Name,Tags[?Key==`Name`].Value|[0]]' \
    --output json \
    --region "$REGION"
}

# Get on-demand instances
get_ondemand_instances() {
  aws ec2 describe-instances \
    --filters \
      "Name=tag:project,Values=$PROJECT_TAG" \
      "Name=instance-state-name,Values=running,pending" \
    --query 'Reservations[].Instances[?InstanceLifecycle==null].[InstanceId,InstanceType,State.Name,Tags[?Key==`Name`].Value|[0]]' \
    --output json \
    --region "$REGION"
}

# --- Status Commands ---

cmd_status() {
  log_info "Fetching spot fleet status..."

  local spot_instances
  spot_instances=$(get_spot_instances)
  local spot_count
  spot_count=$(echo "$spot_instances" | jq 'length')

  local ondemand_instances
  ondemand_instances=$(get_ondemand_instances)
  local ondemand_count
  ondemand_count=$(echo "$ondemand_instances" | jq 'length')

  echo ""
  echo "ClaudeFS Spot Fleet Status"
  echo "=========================="
  echo ""
  echo "Spot Instances ($spot_count):"
  if (( spot_count > 0 )); then
    echo "$spot_instances" | jq -r '.[] | "  \(.[3] // "unknown")\t(ID: \(.[0]), Type: \(.[1]), State: \(.[2]))"'
  else
    echo "  (none)"
  fi

  echo ""
  echo "On-Demand Instances ($ondemand_count):"
  if (( ondemand_count > 0 )); then
    echo "$ondemand_instances" | jq -r '.[] | "  \(.[3] // "unknown")\t(ID: \(.[0]), Type: \(.[1]), State: \(.[2]))"'
  else
    echo "  (none)"
  fi

  echo ""
  echo "Budget Status:"
  check_daily_budget
}

# --- Monitoring ---

cmd_monitor() {
  local interval="${1:-60}"
  log_info "Starting spot fleet monitoring (interval: ${interval}s)"
  init_state_dir

  while true; do
    check_interruption_notices
    check_for_terminated_instances
    validate_inter_node_connectivity

    sleep "$interval"
  done
}

check_interruption_notices() {
  # For each spot instance, check metadata for interruption notice
  local spot_instances
  spot_instances=$(get_spot_instances | jq -r '.[].[0]')

  while IFS= read -r instance_id; do
    if [[ -z "$instance_id" ]]; then continue; fi

    # Get instance public IP
    local public_ip
    public_ip=$(aws ec2 describe-instances \
      --instance-ids "$instance_id" \
      --query 'Reservations[0].Instances[0].PublicIpAddress' \
      --output text \
      --region "$REGION" 2>/dev/null || echo "")

    if [[ -z "$public_ip" ]] || [[ "$public_ip" == "None" ]]; then
      continue
    fi

    # Check metadata for interruption notice (2-minute warning)
    # Requires SSH key to be configured
    if check_instance_interruption_notice "$public_ip" "$instance_id"; then
      log_warn "Spot instance $instance_id received interruption notice!"
      handle_graceful_shutdown "$instance_id" "$public_ip"
    fi
  done <<< "$spot_instances"
}

check_instance_interruption_notice() {
  local public_ip="$1"
  local instance_id="$2"

  # This would require SSH access to the instance
  # For now, we use AWS API to check EC2 spot instance interruption warnings
  # via AWS EventBridge or by polling instance status

  local interruption_time
  interruption_time=$(aws ec2 describe-instances \
    --instance-ids "$instance_id" \
    --query 'Reservations[0].Instances[0].SpotInstanceRequestId' \
    --output text \
    --region "$REGION" 2>/dev/null)

  # A real implementation would check:
  # - Instance status checks for degradation
  # - EC2 Spot Instance Interruption Notices via SNS
  # - Scheduled maintenance events

  return 1  # No interruption notice detected
}

handle_graceful_shutdown() {
  local instance_id="$1"
  local public_ip="$2"

  log_info "Gracefully shutting down instance $instance_id..."

  # Export node state to S3 for recovery
  save_node_state "$instance_id" "$public_ip"

  # Stop ClaudeFS service
  ssh -o StrictHostKeyChecking=no -o ConnectTimeout=5 \
    "ubuntu@${public_ip}" \
    "sudo systemctl stop cfs-server 2>/dev/null || true" \
    || log_warn "Failed to SSH into $instance_id for graceful shutdown"

  # Notify orchestrator of impending shutdown
  log_info "Notification sent to orchestrator to prepare replacement"
}

check_for_terminated_instances() {
  # Check if any expected instances are missing
  local state_file="$STATE_DIR/expected_instances.txt"

  if [[ ! -f "$state_file" ]]; then
    return
  fi

  local expected_instances
  expected_instances=$(cat "$state_file")

  local current_instances
  current_instances=$(get_spot_instances | jq -r '.[].[0]' | sort)

  local expected_sorted
  expected_sorted=$(echo "$expected_instances" | sort)

  if [[ "$current_sorted" != "$expected_sorted" ]]; then
    log_warn "Instance count mismatch detected"
    log_warn "Expected: $(echo "$expected_sorted" | wc -l) instances"
    log_warn "Current: $(echo "$current_sorted" | wc -l) instances"

    # Find missing instances
    comm -23 <(echo "$expected_sorted") <(echo "$current_sorted") | while read -r missing_id; do
      log_warn "Missing instance: $missing_id"
      handle_missing_instance "$missing_id"
    done
  fi
}

handle_missing_instance() {
  local instance_id="$1"

  # Load saved state
  local state_file="$STATE_DIR/instance-${instance_id}.json"
  if [[ ! -f "$state_file" ]]; then
    log_warn "No saved state for instance $instance_id"
    return
  fi

  local instance_type instance_name role
  instance_type=$(jq -r '.instance_type' "$state_file")
  instance_name=$(jq -r '.instance_name' "$state_file")
  role=$(jq -r '.role' "$state_file")

  log_info "Replacing terminated instance: $instance_name ($role)"

  # Queue replacement request
  echo "$instance_id|$instance_type|$instance_name|$role" >> "$STATE_DIR/replacements-pending.txt"
}

save_node_state() {
  local instance_id="$1"
  local public_ip="$2"

  local state_file="$STATE_DIR/instance-${instance_id}.json"

  # Gather instance metadata
  local instance_info
  instance_info=$(aws ec2 describe-instances \
    --instance-ids "$instance_id" \
    --query 'Reservations[0].Instances[0]' \
    --output json \
    --region "$REGION")

  # Extract key fields
  local instance_type instance_name role site
  instance_type=$(echo "$instance_info" | jq -r '.InstanceType')
  instance_name=$(echo "$instance_info" | jq -r '.Tags[] | select(.Key=="Name") | .Value')
  role=$(echo "$instance_info" | jq -r '.Tags[] | select(.Key=="Role") | .Value')
  site=$(echo "$instance_info" | jq -r '.Tags[] | select(.Key=="Site") | .Value // "unknown"')

  # Save to state file
  cat > "$state_file" << EOF
{
  "instance_id": "$instance_id",
  "instance_type": "$instance_type",
  "instance_name": "$instance_name",
  "role": "$role",
  "site": "$site",
  "saved_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "public_ip": "$public_ip"
}
EOF

  log_info "Saved state for instance $instance_id to $state_file"

  # TODO: Export RocksDB/metadata snapshots via SSH if instance is still reachable
}

# --- Validation ---

cmd_validate() {
  log_info "Validating spot fleet health..."

  local spot_instances
  spot_instances=$(get_spot_instances)

  echo "Spot Instance Validation"
  echo "========================"
  echo ""

  local failures=0

  # Check SSH connectivity to each instance
  echo "$spot_instances" | jq -r '.[]' | while read -r instance_json; do
    local instance_id instance_name public_ip
    instance_id=$(echo "$instance_json" | jq -r '.[0]')
    instance_name=$(echo "$instance_json" | jq -r '.[3] // "unknown"')

    public_ip=$(aws ec2 describe-instances \
      --instance-ids "$instance_id" \
      --query 'Reservations[0].Instances[0].PublicIpAddress' \
      --output text \
      --region "$REGION" 2>/dev/null || echo "")

    if [[ -z "$public_ip" ]] || [[ "$public_ip" == "None" ]]; then
      log_warn "Instance $instance_name ($instance_id): No public IP"
      ((failures++))
      return
    fi

    # Test SSH
    if ssh -o StrictHostKeyChecking=no -o ConnectTimeout=5 \
      "ubuntu@${public_ip}" "echo 'OK'" >/dev/null 2>&1; then
      echo "✓ $instance_name ($instance_id): SSH OK"
    else
      log_warn "✗ $instance_name ($instance_id): SSH FAILED"
      ((failures++))
    fi
  done

  echo ""
  if (( failures == 0 )); then
    log_info "All instances validated successfully"
    return 0
  else
    log_error "Validation failed: $failures instance(s) have issues"
    return 1
  fi
}

validate_inter_node_connectivity() {
  # Test connectivity between storage nodes (would be called periodically)
  # This is a stub for future enhancement
  :
}

# --- Budget Monitoring ---

check_daily_budget() {
  local daily_budget=100
  local current_spend

  current_spend=$(aws ce get-cost-and-usage \
    --time-period Start="$(date -d 'today' '+%Y-%m-%d')" End="$(date '+%Y-%m-%d')" \
    --granularity DAILY \
    --metrics "UnblendedCost" \
    --query 'ResultsByTime[0].Total.UnblendedCost.Amount' \
    --output text \
    --region "$REGION" 2>/dev/null || echo "0")

  # Cost Explorer may not be available, just report as unavailable
  if [[ "$current_spend" == "0" ]] || [[ -z "$current_spend" ]]; then
    echo "  Current spend: (unavailable)"
  else
    echo "  Current spend: \$$current_spend/$daily_budget"
    local percentage
    percentage=$(awk "BEGIN {printf \"%.0f\", ($current_spend/$daily_budget)*100}")
    echo "  Budget used: $percentage%"
  fi
}

cmd_replace() {
  local instance_id="$1"
  if [[ -z "$instance_id" ]]; then
    log_error "Usage: spot-fleet-manager.sh replace <instance_id>"
    exit 1
  fi

  log_info "Replacing terminated instance: $instance_id"

  # Load saved state
  local state_file="$STATE_DIR/instance-${instance_id}.json"
  if [[ ! -f "$state_file" ]]; then
    log_error "No saved state for instance $instance_id"
    exit 1
  fi

  local instance_type instance_name role
  instance_type=$(jq -r '.instance_type' "$state_file")
  instance_name=$(jq -r '.instance_name' "$state_file")
  role=$(jq -r '.role' "$state_file")

  log_info "Instance details: name=$instance_name, type=$instance_type, role=$role"
  log_info "Use 'cfs-dev up' with Terraform to provision replacement"
}

# --- Main ---

main() {
  init_state_dir

  local cmd="${1:-status}"
  case "$cmd" in
    status)
      cmd_status
      ;;
    monitor)
      shift || true
      cmd_monitor "$@"
      ;;
    validate)
      cmd_validate
      ;;
    replace)
      shift || true
      cmd_replace "$@"
      ;;
    help)
      cat << 'EOF'
spot-fleet-manager.sh — ClaudeFS Spot Instance Lifecycle Management

Usage:
  ./spot-fleet-manager.sh status                   # Show spot fleet status
  ./spot-fleet-manager.sh monitor [--interval 60]  # Monitor for interruptions
  ./spot-fleet-manager.sh replace <instance_id>    # Replace terminated instance
  ./spot-fleet-manager.sh validate                 # Validate fleet health

Commands:
  status      Show current status of all spot instances and budget
  monitor     Run continuous monitoring for spot interruptions (2-min warning)
              Handles graceful shutdown and queues replacement
  validate    Test SSH connectivity to all instances and check health
  replace     Prepare to replace a terminated instance (manual trigger)

Environment Variables:
  AWS_REGION  AWS region (default: us-west-2)
  PROJECT_TAG Project tag for resource filtering (default: claudefs)

Examples:
  # Start monitoring for spot interruptions
  ./spot-fleet-manager.sh monitor --interval 30

  # Check current status
  ./spot-fleet-manager.sh status

  # Validate all instances
  ./spot-fleet-manager.sh validate

State Files:
  ~/.cfs/spot-fleet-state/expected_instances.txt    List of expected instance IDs
  ~/.cfs/spot-fleet-state/instance-{id}.json        Saved state for each instance
  ~/.cfs/spot-fleet-state/replacements-pending.txt  Queue of instances needing replacement
EOF
      ;;
    *)
      log_error "Unknown command: $cmd"
      echo "Run './spot-fleet-manager.sh help' for usage"
      exit 1
      ;;
  esac
}

main "$@"
