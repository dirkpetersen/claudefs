# A11: Phase 5 Block 5 — GitOps Orchestration — OpenCode Input

**Agent:** A11 Infrastructure & CI
**Date:** 2026-04-18 Session 16+
**Target:** 5 shell scripts + 5 YAML config files + 10-15 integration tests
**Total LOC:** 1,400+ (scripts + configs) + 400+ (tests) = 1,800+ total

---

## Mission

Generate complete GitOps orchestration system for ClaudeFS test cluster. This includes:
1. **GitOps controller** — watches git repo, applies changes to cluster
2. **Declarative cluster config** — cluster state defined in git (YAML)
3. **Drift detection** — monitors for divergence from git state
4. **Self-healing** — auto-remediate common failures
5. **Rollback automation** — recover from critical failures

---

## Deliverables

### 1. GitOps Controller: `tools/cfs-gitops-controller.sh`

**Purpose:** Main orchestration engine. Polls git for changes, applies them to cluster infrastructure.

**Specification:**

```bash
#!/bin/bash
# tools/cfs-gitops-controller.sh
# GitOps controller for ClaudeFS test cluster
# Polls git for cluster config changes, applies via Terraform

set -euo pipefail

GIT_REPO="${GIT_REPO:-.}"  # ClaudeFS repo
CONFIG_DIR="${CONFIG_DIR:-infrastructure}"
TERRAFORM_DIR="${TERRAFORM_DIR:-infrastructure/terraform}"
STATE_DIR="${STATE_DIR:-/var/lib/cfs-gitops}"
LOG_FILE="${LOG_FILE:-/var/log/cfs-gitops/controller.log}"
POLL_INTERVAL="${POLL_INTERVAL:-300}"  # 5 minutes default

# Create directories if not exist
mkdir -p "$STATE_DIR" "$(dirname "$LOG_FILE")"

# Logging function
log() {
    echo "[$(date +'%Y-%m-%d %H:%M:%S')] $1" | tee -a "$LOG_FILE"
}

# Error handler
error_exit() {
    log "ERROR: $1"
    exit 1
}

# Get current git HEAD
get_git_head() {
    cd "$GIT_REPO"
    git rev-parse HEAD
}

# Get saved git HEAD (checkpoint)
get_saved_head() {
    if [[ -f "$STATE_DIR/last_applied_head" ]]; then
        cat "$STATE_DIR/last_applied_head"
    else
        echo ""
    fi
}

# Check if config has changed since last apply
has_config_changed() {
    local current_head=$(get_git_head)
    local saved_head=$(get_saved_head)

    if [[ "$current_head" != "$saved_head" ]]; then
        log "Config changed: $saved_head -> $current_head"
        return 0  # True - has changed
    fi
    return 1  # False - no change
}

# Validate cluster config YAML
validate_cluster_config() {
    local config_file="$GIT_REPO/$CONFIG_DIR/cluster.yaml"

    if [[ ! -f "$config_file" ]]; then
        error_exit "Cluster config not found: $config_file"
    fi

    # Check YAML syntax (requires yq or similar)
    if ! yq eval '.' "$config_file" > /dev/null 2>&1; then
        error_exit "Invalid YAML in $config_file"
    fi

    log "Cluster config validated: $config_file"
}

# Generate Terraform variables from cluster config
generate_terraform_vars() {
    local config_file="$GIT_REPO/$CONFIG_DIR/cluster.yaml"
    local tf_vars_file="$TERRAFORM_DIR/terraform.tfvars.json"

    # Parse cluster.yaml and generate terraform.tfvars.json
    # Extract: cluster_name, region, environment, node_specs, etc.

    cat > "$tf_vars_file" << 'TFVARS'
{
  "cluster_name": "cfs-dev-cluster",
  "region": "us-west-2",
  "environment": "development",
  "storage_node_count": 5,
  "storage_instance_type": "i4i.2xlarge",
  "client_node_count": 2,
  "client_instance_type": "c7a.xlarge",
  "conduit_count": 1,
  "conduit_instance_type": "t3.medium",
  "jepsen_count": 1,
  "jepsen_instance_type": "c7a.xlarge",
  "monitoring_enabled": true,
  "prometheus_scrape_interval": 15,
  "prometheus_retention_days": 30
}
TFVARS

    log "Generated Terraform variables: $tf_vars_file"
}

# Run Terraform plan
terraform_plan() {
    log "Running terraform plan..."

    cd "$TERRAFORM_DIR"

    if terraform plan -out=tfplan > "$STATE_DIR/terraform.plan.log" 2>&1; then
        log "Terraform plan successful"
        return 0
    else
        log "Terraform plan failed (non-fatal, reviewing for safety)"
        cat "$STATE_DIR/terraform.plan.log" >> "$LOG_FILE"
        return 1
    fi
}

# Run Terraform apply (with safety checks)
terraform_apply() {
    log "Running terraform apply..."

    cd "$TERRAFORM_DIR"

    # Parse plan to check for destructive changes
    if grep -q "will be destroyed" tfplan; then
        log "WARNING: Terraform plan includes resource destruction"
        log "Requiring manual approval for destructive changes"
        return 1  # Don't auto-apply destructive changes
    fi

    if terraform apply -auto-approve tfplan > "$STATE_DIR/terraform.apply.log" 2>&1; then
        log "Terraform apply successful"
        return 0
    else
        log "Terraform apply failed"
        cat "$STATE_DIR/terraform.apply.log" >> "$LOG_FILE"
        return 1
    fi
}

# Update checkpoint (git HEAD applied successfully)
update_checkpoint() {
    local current_head=$(get_git_head)
    echo "$current_head" > "$STATE_DIR/last_applied_head"
    git tag -f "cluster-working-$(date +%s)" "$current_head"
    log "Checkpoint updated: $current_head"
}

# Main loop
main() {
    log "GitOps controller starting (poll interval: ${POLL_INTERVAL}s)"

    while true; do
        if has_config_changed; then
            log "Applying config changes from git..."

            validate_cluster_config
            generate_terraform_vars

            if terraform_plan && terraform_apply; then
                update_checkpoint
                log "Config successfully applied"
            else
                log "Config apply failed, will retry on next poll"
            fi
        else
            log "No config changes detected"
        fi

        sleep "$POLL_INTERVAL"
    done
}

# Run if executed directly
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
```

**Requirements:**
- Monitor `infrastructure/cluster.yaml` for changes
- On change: validate YAML → generate Terraform vars → plan → apply
- Non-destructive auto-apply, destructive changes require manual approval
- Create git tags for successful deployments (`cluster-working-<timestamp>`)
- Log all operations to `/var/log/cfs-gitops/controller.log`
- 500+ LOC with comprehensive error handling

---

### 2. Drift Detector: `tools/cfs-drift-detector.sh`

**Purpose:** Continuous monitoring for divergence between git config and running cluster.

**Specification:**

```bash
#!/bin/bash
# tools/cfs-drift-detector.sh
# Detects divergence between declared cluster config (git) and actual state

set -euo pipefail

CONFIG_DIR="${CONFIG_DIR:-infrastructure}"
DRIFT_REPORT="${DRIFT_REPORT:-/var/lib/cfs-gitops/drift-report.json}"
PROMETHEUS_PUSHGATEWAY="${PROMETHEUS_PUSHGATEWAY:-http://localhost:9091}"
LOG_FILE="${LOG_FILE:-/var/log/cfs-gitops/drift-detector.log}"

# Drift categories
DRIFT_INFRASTRUCTURE=0
DRIFT_SOFTWARE=0
DRIFT_CONFIG=0
DRIFT_MONITORING=0
DRIFT_DEPLOYMENT=0

# Log function
log() {
    echo "[$(date +'%Y-%m-%d %H:%M:%S')] $1" | tee -a "$LOG_FILE"
}

# Get declared config value (from infrastructure/cluster.yaml)
get_declared_value() {
    local key=$1
    yq eval ".spec.$key" "infrastructure/cluster.yaml"
}

# Check infrastructure drift (instance counts, types)
check_infrastructure_drift() {
    log "Checking infrastructure drift..."

    local declared_storage=$(get_declared_value "nodes.storage.count")
    local declared_client=$(get_declared_value "nodes.clients.count")

    # Query AWS for actual instance counts
    local actual_storage=$(aws ec2 describe-instances \
        --filters "Name=tag:role,Values=storage" "Name=instance-state-name,Values=running" \
        --query "length(Reservations[0].Instances[])" --output text 2>/dev/null || echo "0")

    local actual_client=$(aws ec2 describe-instances \
        --filters "Name=tag:role,Values=client" "Name=instance-state-name,Values=running" \
        --query "length(Reservations[0].Instances[])" --output text 2>/dev/null || echo "0")

    if [[ "$declared_storage" != "$actual_storage" ]]; then
        log "DRIFT: Storage nodes mismatch (declared: $declared_storage, actual: $actual_storage)"
        ((DRIFT_INFRASTRUCTURE++))
    fi

    if [[ "$declared_client" != "$actual_client" ]]; then
        log "DRIFT: Client nodes mismatch (declared: $declared_client, actual: $actual_client)"
        ((DRIFT_INFRASTRUCTURE++))
    fi
}

# Check software drift (service versions)
check_software_drift() {
    log "Checking software drift..."

    local declared_version=$(get_declared_value "version")

    # Query nodes for actual deployed version
    # This would require SSH to nodes and checking /opt/cfs/VERSION or similar
    # For now, placeholder for actual implementation

    log "Software drift check: would query node versions vs declared $declared_version"
}

# Check config drift (prometheus, grafana configs)
check_config_drift() {
    log "Checking config drift..."

    # Hash of local config files
    local local_prometheus_hash=$(sha256sum infrastructure/components/prometheus.yaml | awk '{print $1}')
    local local_alertmanager_hash=$(sha256sum infrastructure/components/alertmanager.yaml | awk '{print $1}')

    # Query running Prometheus for current config hash
    # Placeholder for actual implementation (would query Prometheus API)

    log "Config drift check: local prometheus hash = $local_prometheus_hash"
}

# Check monitoring drift (alert rules)
check_monitoring_drift() {
    log "Checking monitoring drift..."

    # Query Prometheus for number of loaded alert rules
    local local_alert_count=$(yq eval '.groups[] | select(.name=="ClaudeFS Alerts") | length(.rules)' infrastructure/components/prometheus-alerts.yaml)

    log "Monitoring drift: local alert rules = $local_alert_count"
}

# Check deployment drift (manual changes to infrastructure)
check_deployment_drift() {
    log "Checking deployment drift..."

    # Check git status - ensure no uncommitted changes on cluster nodes
    # This would involve checking if terraform state matches git state

    log "Deployment drift: checking for uncommitted infrastructure changes"
}

# Generate drift report JSON
generate_drift_report() {
    local timestamp=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

    cat > "$DRIFT_REPORT" << JSON
{
  "timestamp": "$timestamp",
  "drifts": {
    "infrastructure": $DRIFT_INFRASTRUCTURE,
    "software": $DRIFT_SOFTWARE,
    "config": $DRIFT_CONFIG,
    "monitoring": $DRIFT_MONITORING,
    "deployment": $DRIFT_DEPLOYMENT
  },
  "total_drift_score": $((DRIFT_INFRASTRUCTURE + DRIFT_SOFTWARE + DRIFT_CONFIG + DRIFT_MONITORING + DRIFT_DEPLOYMENT))
}
JSON

    log "Drift report written to $DRIFT_REPORT"
}

# Push metrics to Prometheus Pushgateway
push_drift_metrics() {
    cat << METRICS | curl --data-binary @- "${PROMETHEUS_PUSHGATEWAY}/metrics/job/cfs-gitops/instance/drift-detector"
# HELP claudefs_drift_infrastructure Infrastructure drift count
# TYPE claudefs_drift_infrastructure gauge
claudefs_drift_infrastructure $DRIFT_INFRASTRUCTURE

# HELP claudefs_drift_software Software drift count
# TYPE claudefs_drift_software gauge
claudefs_drift_software $DRIFT_SOFTWARE

# HELP claudefs_drift_config Config drift count
# TYPE claudefs_drift_config gauge
claudefs_drift_config $DRIFT_CONFIG

# HELP claudefs_drift_total Total drift score
# TYPE claudefs_drift_total gauge
claudefs_drift_total $((DRIFT_INFRASTRUCTURE + DRIFT_SOFTWARE + DRIFT_CONFIG + DRIFT_MONITORING + DRIFT_DEPLOYMENT))
METRICS

    log "Metrics pushed to Prometheus Pushgateway"
}

# Main function
main() {
    log "Drift detector starting..."

    check_infrastructure_drift
    check_software_drift
    check_config_drift
    check_monitoring_drift
    check_deployment_drift

    generate_drift_report
    push_drift_metrics

    log "Drift detection complete"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
```

**Requirements:**
- Detect 5 types of drift: infrastructure, software, config, monitoring, deployment
- Query AWS EC2 API for current instance counts/types
- Generate JSON report at `/var/lib/cfs-gitops/drift-report.json`
- Push Prometheus metrics: `claudefs_drift_*` (infrastructure, software, config, total)
- Log findings to `/var/log/cfs-gitops/drift-detector.log`
- 350+ LOC

---

### 3. Remediation Engine: `tools/cfs-remediation-engine.sh`

**Purpose:** Automatically execute remediation actions based on alert triggers and remediation rules.

**Specification:**

```bash
#!/bin/bash
# tools/cfs-remediation-engine.sh
# Self-healing engine: executes remediation actions on alert triggers

set -euo pipefail

RULES_FILE="${RULES_FILE:-tools/cfs-remediation-rules.yaml}"
ACTION_LOG="${ACTION_LOG:-/var/log/cfs-gitops/remediation-actions.log}"
STATE_DIR="${STATE_DIR:-/var/lib/cfs-gitops}"

# Action execution tracking
ACTION_ID_COUNTER=0

log() {
    echo "[$(date +'%Y-%m-%d %H:%M:%S')] $1" | tee -a "$ACTION_LOG"
}

error_log() {
    echo "[$(date +'%Y-%m-%d %H:%M:%S')] ERROR: $1" | tee -a "$ACTION_LOG"
}

# Generate unique action ID
generate_action_id() {
    echo "action-$(date +%s)-$((RANDOM))"
}

# Execute remediation action
execute_action() {
    local action_type=$1
    local target=$2
    shift 2
    local args=("$@")

    local action_id=$(generate_action_id)
    log "Executing action: ID=$action_id, type=$action_type, target=$target"

    case "$action_type" in
        "scale")
            execute_scale_action "$action_id" "$target" "${args[@]}"
            ;;
        "restart")
            execute_restart_action "$action_id" "$target" "${args[@]}"
            ;;
        "evict")
            execute_evict_action "$action_id" "$target" "${args[@]}"
            ;;
        "drain")
            execute_drain_action "$action_id" "$target" "${args[@]}"
            ;;
        "rebalance")
            execute_rebalance_action "$action_id" "$target" "${args[@]}"
            ;;
        "rollback")
            execute_rollback_action "$action_id" "${args[@]}"
            ;;
        *)
            error_log "Unknown action type: $action_type"
            return 1
            ;;
    esac
}

# Scale action: increase instance size
execute_scale_action() {
    local action_id=$1
    local instance_id=$2
    local new_instance_type=${3:-c7a.2xlarge}

    log "[$action_id] Scaling instance $instance_id to type $new_instance_type"

    # In real implementation, would: stop instance, change type, restart
    # Placeholder for demo
    log "[$action_id] Scale action would execute: aws ec2 modify-instance-attribute --instance-id $instance_id"
}

# Restart action: graceful service restart
execute_restart_action() {
    local action_id=$1
    local service=$2
    local timeout=${3:-30}

    log "[$action_id] Restarting service $service (timeout: ${timeout}s)"

    # Send signal to restart service
    # In real impl: systemctl restart $service with timeout
}

# Evict action: move workload to healthier node
execute_evict_action() {
    local action_id=$1
    local workload=$2
    local destination=$3

    log "[$action_id] Evicting workload $workload to node $destination"

    # Drain workload from current node, move to destination
}

# Drain action: gracefully stop workload on node
execute_drain_action() {
    local action_id=$1
    local node=$2
    local timeout=${3:-300}

    log "[$action_id] Draining node $node (timeout: ${timeout}s)"

    # Wait for workload to shutdown gracefully
}

# Rebalance action: redistribute load across cluster
execute_rebalance_action() {
    local action_id=$1
    local strategy=${2:-consistent_hash}

    log "[$action_id] Rebalancing cluster with strategy: $strategy"

    # Recalculate consistent hash ring, move necessary data
}

# Rollback action: revert to last-known-good state
execute_rollback_action() {
    local action_id=$1

    log "[$action_id] Initiating cluster rollback to last known-good state"

    # Load last checkpoint (from $STATE_DIR/cluster.working-*.tag)
    # Reapply Terraform with previous config
}

# Create GitHub issue for action
create_github_issue() {
    local action_id=$1
    local action_type=$2
    local status=$3  # success or failure
    local details=$4

    # Would use GitHub API to create issue
    log "Creating GitHub issue for action $action_id (status: $status)"
}

# Handle alert trigger (from AlertManager webhook)
handle_alert() {
    local alert_name=$1
    local severity=$2
    local description=$3

    log "Alert received: $alert_name (severity: $severity)"

    # Look up rule in remediation rules file
    # Parse rule for trigger match
    # Execute actions

    # Example: if alert_name matches "high_cpu", execute scale action
    case "$alert_name" in
        "high_cpu_on_node")
            execute_action "scale" "affected_instance" "c7a.2xlarge"
            execute_action "evict" "highest_cpu_process"
            ;;
        "spot_interruption")
            execute_action "drain" "interrupted_instance" "300"
            execute_action "rebalance" "consistent_hash"
            ;;
        "prometheus_down")
            execute_action "restart" "prometheus" "30"
            ;;
    esac
}

# Main: webhook server mode (receive alerts from AlertManager)
# In production, would run as HTTP server listening on port
# For now, can be called directly with alert parameters

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    # Parse arguments: action_type target [args...]
    if [[ $# -lt 2 ]]; then
        echo "Usage: $0 <action_type> <target> [args...]"
        echo "  action_type: scale, restart, evict, drain, rebalance, rollback"
        echo "  target: instance/service/workload identifier"
        exit 1
    fi

    execute_action "$@"
fi
```

**Requirements:**
- Parse remediation rules from `cfs-remediation-rules.yaml`
- Execute 6 action types: scale, restart, evict, drain, rebalance, rollback
- Log all actions with unique IDs to `/var/log/cfs-gitops/remediation-actions.log`
- Create GitHub issues for failed auto-remediation attempts
- 400+ LOC with comprehensive action handlers

---

### 4. Checkpoint Manager: `tools/cfs-checkpoint-manager.sh`

**Purpose:** Create and manage snapshots of working cluster state for rollback.

**Specification:**

```bash
#!/bin/bash
# tools/cfs-checkpoint-manager.sh
# Creates and manages cluster state snapshots for rollback

set -euo pipefail

STATE_DIR="${STATE_DIR:-/var/lib/cfs-gitops}"
S3_BACKUP_BUCKET="${S3_BACKUP_BUCKET:-cfs-terraform-backups}"
RETENTION_DAYS="${RETENTION_DAYS:-7}"
LOG_FILE="${LOG_FILE:-/var/log/cfs-gitops/checkpoint.log}"

log() {
    echo "[$(date +'%Y-%m-%d %H:%M:%S')] $1" | tee -a "$LOG_FILE"
}

# Create checkpoint after successful deployment
create_checkpoint() {
    local checkpoint_id="cluster-working-$(date +%s)"
    local checkpoint_dir="$STATE_DIR/checkpoints/$checkpoint_id"

    mkdir -p "$checkpoint_dir"

    log "Creating checkpoint: $checkpoint_id"

    # 1. Save current git HEAD
    git rev-parse HEAD > "$checkpoint_dir/git-head.txt"

    # 2. Save current Terraform state
    cp infrastructure/terraform/terraform.tfstate "$checkpoint_dir/terraform.tfstate"

    # 3. Save cluster config snapshot
    cp infrastructure/cluster.yaml "$checkpoint_dir/cluster.yaml"
    cp -r infrastructure/components/ "$checkpoint_dir/components/"

    # 4. Save cluster metrics (health snapshot)
    # Query Prometheus for health metrics at checkpoint time
    # Store in checkpoint_dir/health-snapshot.json

    # 5. Create git tag for this checkpoint
    git tag -a "$checkpoint_id" -m "Checkpoint: cluster working state" HEAD

    log "Checkpoint created: $checkpoint_dir"

    # 6. Upload to S3 for off-cluster backup
    tar czf "/tmp/$checkpoint_id.tar.gz" "$checkpoint_dir"
    aws s3 cp "/tmp/$checkpoint_id.tar.gz" "s3://$S3_BACKUP_BUCKET/checkpoints/"
    rm "/tmp/$checkpoint_id.tar.gz"

    log "Checkpoint backed up to S3: s3://$S3_BACKUP_BUCKET/checkpoints/$checkpoint_id.tar.gz"
}

# List available checkpoints
list_checkpoints() {
    log "Available checkpoints:"

    ls -lh "$STATE_DIR/checkpoints/" 2>/dev/null || log "No checkpoints found"

    git tag -l "cluster-working-*" | sort -r | head -5
}

# Get most recent working checkpoint
get_latest_checkpoint() {
    ls -td "$STATE_DIR/checkpoints/cluster-working-"* 2>/dev/null | head -1
}

# Cleanup old checkpoints (keep last 5)
cleanup_old_checkpoints() {
    log "Cleaning up old checkpoints (keeping last 5)..."

    local checkpoint_dirs=($(ls -dt "$STATE_DIR/checkpoints/cluster-working-"* 2>/dev/null || true))

    if [[ ${#checkpoint_dirs[@]} -gt 5 ]]; then
        for ((i=5; i<${#checkpoint_dirs[@]}; i++)); do
            local old_checkpoint="${checkpoint_dirs[$i]}"
            log "Deleting old checkpoint: $old_checkpoint"
            rm -rf "$old_checkpoint"
        done
    fi
}

# Validate checkpoint integrity
validate_checkpoint() {
    local checkpoint_dir=$1

    if [[ ! -d "$checkpoint_dir" ]]; then
        log "ERROR: Checkpoint directory not found: $checkpoint_dir"
        return 1
    fi

    log "Validating checkpoint: $checkpoint_dir"

    # Check all required files present
    local required_files=("git-head.txt" "terraform.tfstate" "cluster.yaml")
    for file in "${required_files[@]}"; do
        if [[ ! -f "$checkpoint_dir/$file" ]]; then
            log "ERROR: Missing required file in checkpoint: $file"
            return 1
        fi
    done

    log "Checkpoint validation passed"
    return 0
}

# Main
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-list}" in
        "create")
            create_checkpoint
            cleanup_old_checkpoints
            ;;
        "list")
            list_checkpoints
            ;;
        "validate")
            validate_checkpoint "${2:-.}"
            ;;
        *)
            echo "Usage: $0 {create|list|validate <checkpoint_dir>}"
            exit 1
            ;;
    esac
fi
```

**Requirements:**
- Create snapshots of: git HEAD, Terraform state, cluster config, health metrics
- Upload to S3 for off-cluster backup
- Maintain 5 most recent checkpoints, delete older ones
- Create git tags for each checkpoint
- Validate checkpoint integrity before use
- 250+ LOC

---

### 5. Rollback Engine: `tools/cfs-rollback-engine.sh`

**Purpose:** Automatic cluster recovery from critical failures by reverting to last checkpoint.

**Specification:**

```bash
#!/bin/bash
# tools/cfs-rollback-engine.sh
# Automatic rollback to last-known-good cluster state on critical failure

set -euo pipefail

STATE_DIR="${STATE_DIR:-/var/lib/cfs-gitops}"
TERRAFORM_DIR="${TERRAFORM_DIR:-infrastructure/terraform}"
LOG_FILE="${LOG_FILE:-/var/log/cfs-gitops/rollback.log}"
SMOKE_TEST_TIMEOUT="${SMOKE_TEST_TIMEOUT:-300}"

log() {
    echo "[$(date +'%Y-%m-%d %H:%M:%S')] $1" | tee -a "$LOG_FILE"
}

error_exit() {
    log "ERROR: $1"
    exit 1
}

# Run smoke tests to verify cluster health
run_smoke_tests() {
    log "Running smoke tests to verify cluster health..."

    local timeout=$SMOKE_TEST_TIMEOUT
    local start_time=$(date +%s)

    # Test 1: Check if Prometheus is responding
    if ! curl -s http://localhost:9090/-/healthy > /dev/null 2>&1; then
        log "FAIL: Prometheus not responding"
        return 1
    fi

    # Test 2: Check if Grafana is responding
    if ! curl -s http://localhost:3000/api/health > /dev/null 2>&1; then
        log "FAIL: Grafana not responding"
        return 1
    fi

    # Test 3: Query Prometheus for active targets
    local active_targets=$(curl -s 'http://localhost:9090/api/v1/targets?state=active' | grep -c active || true)
    if [[ $active_targets -lt 3 ]]; then
        log "FAIL: Too few active scrape targets ($active_targets)"
        return 1
    fi

    log "Smoke tests passed"
    return 0
}

# Determine if critical failure detected
has_critical_failure() {
    local alert_query=$1

    log "Checking for critical failures..."

    # Query Prometheus for critical alerts
    # Example: get count of critical alerts
    local critical_alerts=$(curl -s 'http://localhost:9090/api/v1/query?query=ALERTS%7Bseverity=%22critical%22%7D' 2>/dev/null | grep -c "critical" || true)

    if [[ $critical_alerts -gt 0 ]]; then
        log "CRITICAL: $critical_alerts critical alerts detected"
        return 0  # True - failure detected
    fi

    return 1  # False - no critical failure
}

# Trigger automatic rollback
trigger_rollback() {
    log "EMERGENCY: Triggering automatic rollback to last-known-good state"

    # Get latest working checkpoint
    local latest_checkpoint=$(ls -td "$STATE_DIR/checkpoints/cluster-working-"* 2>/dev/null | head -1)

    if [[ -z "$latest_checkpoint" ]]; then
        error_exit "No valid checkpoint found for rollback"
    fi

    log "Using checkpoint: $latest_checkpoint"

    # 1. Stop current operations
    log "Halting current cluster operations..."

    # 2. Restore Terraform state from checkpoint
    log "Restoring Terraform state from checkpoint..."
    cp "$latest_checkpoint/terraform.tfstate" "$TERRAFORM_DIR/terraform.tfstate"

    # 3. Restore cluster config from checkpoint
    log "Restoring cluster configuration from checkpoint..."
    cp "$latest_checkpoint/cluster.yaml" "infrastructure/cluster.yaml"
    cp -r "$latest_checkpoint/components/" "infrastructure/components/"

    # 4. Run Terraform apply to recreate infrastructure
    log "Re-applying infrastructure via Terraform..."
    cd "$TERRAFORM_DIR"
    terraform init
    terraform plan -out=tfplan

    if ! terraform apply tfplan; then
        error_exit "Terraform apply failed during rollback"
    fi

    cd - > /dev/null

    # 5. Wait for infrastructure to stabilize
    log "Waiting for infrastructure to stabilize (60 seconds)..."
    sleep 60

    # 6. Run smoke tests to verify recovery
    if run_smoke_tests; then
        log "RECOVERY SUCCESSFUL: Cluster restored to working state"

        # Create GitHub issue for incident investigation
        create_rollback_incident_issue "$latest_checkpoint"

        return 0
    else
        error_exit "Smoke tests failed after rollback - cluster may be in unknown state"
    fi
}

# Create GitHub issue to track rollback incident
create_rollback_incident_issue() {
    local checkpoint=$1

    log "Creating GitHub issue for rollback incident..."

    # Would use GitHub API
    # gh issue create --title "INCIDENT: Automatic cluster rollback triggered" \
    #   --body "Cluster was automatically rolled back to checkpoint: $checkpoint
    # ... details ..."
}

# Main: triggered by critical alert or manual invocation
main() {
    log "Rollback engine starting"

    if has_critical_failure; then
        if run_smoke_tests; then
            log "Smoke tests passed - no rollback needed yet"
        else
            log "Smoke tests failed - initiating rollback"
            trigger_rollback
        fi
    else
        log "No critical failures detected"
    fi
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
```

**Requirements:**
- Detect critical failures via Prometheus alerts
- Run smoke tests (Prometheus health, Grafana health, active targets)
- Restore Terraform state and cluster config from checkpoint
- Re-apply infrastructure via terraform apply
- Verify recovery via smoke tests
- Create GitHub issue for incident investigation
- 300+ LOC

---

## YAML Configuration Files

### 6. Cluster Configuration: `infrastructure/cluster.yaml`

```yaml
apiVersion: claudefs.io/v1alpha1
kind: Cluster
metadata:
  name: cfs-dev-cluster
  environment: development
  region: us-west-2

spec:
  version: 2026.04

  nodes:
    storage:
      count: 5
      instance_type: i4i.2xlarge
      volume_size_gb: 500
      tags:
        role: storage
        Name: cfs-storage-node

    clients:
      count: 2
      instance_type: c7a.xlarge
      tags:
        role: client
        Name: cfs-client-node

    conduit:
      count: 1
      instance_type: t3.medium
      tags:
        role: conduit
        Name: cfs-conduit

    jepsen:
      count: 1
      instance_type: c7a.xlarge
      tags:
        role: jepsen
        Name: cfs-jepsen

  monitoring:
    prometheus:
      enabled: true
      scrape_interval_seconds: 15
      retention_days: 30
      port: 9090

    alertmanager:
      enabled: true
      port: 9093

    grafana:
      enabled: true
      port: 3000

  networking:
    region: us-west-2
    availability_zones:
      - us-west-2a
      - us-west-2b
    security_group_name: cfs-cluster-sg
```

### 7. Remediation Rules: `tools/cfs-remediation-rules.yaml`

```yaml
apiVersion: claudefs.io/v1alpha1
kind: RemediationRules
metadata:
  name: cfs-remediation-rules

spec:
  rules:
    - name: "High CPU on Node"
      trigger: 'node_cpu_usage > 85'
      duration_seconds: 300
      actions:
        - type: scale
          target: affected_instance
          new_instance_type: c7a.2xlarge
        - type: evict
          workload: highest_cpu_consumer
      fallback: notify_admin
      max_retries: 2

    - name: "High Memory Pressure"
      trigger: 'node_memory_usage > 85'
      duration_seconds: 300
      actions:
        - type: restart
          service: prometheus
          graceful_timeout: 60
      fallback: escalate_to_human

    - name: "Spot Interruption Detected"
      trigger: 'spot_interruption_notice'
      duration_seconds: 60
      actions:
        - type: drain
          node: affected_instance
          timeout_seconds: 300
        - type: rebalance
          strategy: consistent_hash
      fallback: manual_intervention

    - name: "Prometheus Service Down"
      trigger: 'alertmanager_prometheus_down'
      duration_seconds: 300
      actions:
        - type: restart
          service: prometheus
          graceful_timeout: 30
          max_retries: 3
      fallback: escalate_to_human

    - name: "Critical Cluster Failure"
      trigger: 'critical_alerts > 3'
      duration_seconds: 300
      actions:
        - type: rollback
      fallback: manual_incident_response
```

### 8. GitOps Deployment Config: `infrastructure/kustomization.yaml`

```yaml
apiVersion: kustomize.config.k8s.io/v1beta1
kind: Kustomization

resources:
  - cluster.yaml
  - components/prometheus.yaml
  - components/alertmanager.yaml
  - components/grafana.yaml

commonLabels:
  app: claudefs
  component: infrastructure

patchesStrategicMerge:
  - patches/development.yaml
  - patches/monitoring.yaml

vars:
  - name: CLUSTER_NAME
    objref:
      apiVersion: claudefs.io/v1alpha1
      kind: Cluster
      name: cfs-dev-cluster
    fieldref:
      fieldpath: metadata.name
```

---

## Rust Integration Tests

### 9. GitOps Orchestration Tests: `crates/claudefs-tests/src/gitops_orchestration_tests.rs`

**Specification:**

Generate 10-15 comprehensive tests organized in modules:

```rust
// Test Module 1: Configuration Parsing (2 tests)
#[test]
fn test_cluster_config_valid_yaml() -> Result<(), String>
fn test_components_config_valid_yaml() -> Result<(), String>

// Test Module 2: Drift Detection (3 tests)
#[test]
fn test_drift_detector_detects_instance_count_change() -> Result<(), String>
#[test]
fn test_drift_detector_detects_config_hash_mismatch() -> Result<(), String>
#[test]
fn test_drift_detector_categorizes_drift_by_severity() -> Result<(), String>

// Test Module 3: Remediation Rules (2 tests)
#[test]
fn test_remediation_rules_yaml_parse() -> Result<(), String>
#[test]
fn test_remediation_rules_action_validation() -> Result<(), String>

// Test Module 4: Checkpoints & Rollback (2 tests)
#[test]
fn test_checkpoint_creation_and_storage() -> Result<(), String>
#[test]
fn test_rollback_scenario_recovery() -> Result<(), String>

// Test Module 5: End-to-End Scenarios (3-5 tests)
#[test]
fn test_gitops_config_change_deployment() -> Result<(), String>
#[test]
fn test_gitops_drift_auto_reconciliation() -> Result<(), String>
#[test]
fn test_gitops_critical_failure_auto_rollback() -> Result<(), String>

// Helpers
fn load_cluster_config() -> Result<serde_yaml::Value, String>
fn validate_yaml_schema(config: &serde_yaml::Value) -> Result<(), String>
fn simulate_drift_scenario(drift_type: &str) -> Result<(), String>
```

**Key Requirements:**
- Use `serde_yaml` to parse YAML configs
- Use `std::fs` to load files from infrastructure/ directory
- No external dependencies (no mocking, no AWS calls)
- Result<(), String> error handling
- Descriptive test names and assertion messages
- 400+ LOC, 10-15 tests total
- All tests compile without warnings
- All tests can be marked #[ignore] for cluster-only execution

---

## Final Deliverables Checklist

**Shell Scripts (5 files, 1,500+ LOC total):**
- [ ] `tools/cfs-gitops-controller.sh` (500+ LOC)
- [ ] `tools/cfs-drift-detector.sh` (350+ LOC)
- [ ] `tools/cfs-remediation-engine.sh` (400+ LOC)
- [ ] `tools/cfs-checkpoint-manager.sh` (250+ LOC)
- [ ] `tools/cfs-rollback-engine.sh` (300+ LOC)

**YAML Configuration (3 files, 300+ LOC total):**
- [ ] `infrastructure/cluster.yaml` (150+ LOC)
- [ ] `tools/cfs-remediation-rules.yaml` (200+ LOC)
- [ ] `infrastructure/kustomization.yaml` (50+ LOC)

**Rust Tests (1 file, 400+ LOC, 10-15 tests):**
- [ ] `crates/claudefs-tests/src/gitops_orchestration_tests.rs` (400+ LOC)

**Quality Metrics:**
- [ ] All shell scripts have valid bash syntax
- [ ] All YAML files have valid syntax
- [ ] All Rust compiles cleanly: `cargo build -p claudefs-tests`
- [ ] Zero new warnings from clippy
- [ ] All tests properly organized in modules
- [ ] Descriptive test names and helpful error messages

---

## Success Criteria

✅ All deliverables generated and syntactically valid
✅ 10-15 integration tests written and compiling
✅ Zero build errors, zero new warnings
✅ Tests can run on cluster with infrastructure in place
✅ Documentation comments explain complex logic
✅ All scripts executable (+x permission)
✅ Ready for integration with Terraform provisioning

---

**Status: READY FOR OPENCODE GENERATION**

OpenCode should:
1. Generate all 5 shell scripts with comprehensive error handling
2. Generate all 3 YAML config files with proper structure
3. Generate 10-15 Rust integration tests
4. Ensure all files are syntactically valid
5. Include helpful comments explaining functionality

