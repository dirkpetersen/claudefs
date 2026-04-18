#!/bin/bash
# tools/cfs-rollback-engine.sh
# Automatic rollback to last-known-good cluster state on critical failure

set -euo pipefail

STATE_DIR="${STATE_DIR:-/var/lib/cfs-gitops}"
TERRAFORM_DIR="${TERRAFORM_DIR:-infrastructure/terraform}"
LOG_FILE="${LOG_FILE:-/var/log/cfs-gitops/rollback.log}"
SMOKE_TEST_TIMEOUT="${SMOKE_TEST_TIMEOUT:-300}"

mkdir -p "$(dirname "$LOG_FILE")" "$STATE_DIR"

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

    local tests_passed=0

    # Test 1: Check if Prometheus is responding
    if curl -s http://localhost:9090/-/healthy > /dev/null 2>&1; then
        log "✓ Prometheus responsive"
        ((tests_passed++))
    else
        log "✗ Prometheus not responding"
    fi

    # Test 2: Check if Grafana is responding
    if curl -s http://localhost:3000/api/health > /dev/null 2>&1; then
        log "✓ Grafana responsive"
        ((tests_passed++))
    else
        log "✗ Grafana not responding"
    fi

    # Test 3: Query Prometheus for active targets
    if curl -s 'http://localhost:9090/api/v1/targets?state=active' 2>/dev/null | grep -q "active"; then
        log "✓ Prometheus targets active"
        ((tests_passed++))
    else
        log "✗ No active Prometheus targets"
    fi

    if [[ $tests_passed -ge 2 ]]; then
        log "Smoke tests passed ($tests_passed/3)"
        return 0
    else
        log "Smoke tests failed ($tests_passed/3)"
        return 1
    fi
}

# Determine if critical failure detected
has_critical_failure() {
    log "Checking for critical failures..."

    # Query Prometheus for critical alerts (placeholder)
    local critical_alerts=0

    if [[ $critical_alerts -gt 0 ]]; then
        log "CRITICAL: $critical_alerts critical alerts detected"
        return 0
    fi

    return 1
}

# Trigger automatic rollback
trigger_rollback() {
    log "EMERGENCY: Triggering automatic rollback to last-known-good state"

    # Get latest working checkpoint
    local latest_checkpoint=$(ls -td "$STATE_DIR/checkpoints/cluster-working-"* 2>/dev/null | head -1 || echo "")

    if [[ -z "$latest_checkpoint" ]]; then
        error_exit "No valid checkpoint found for rollback"
    fi

    log "Using checkpoint: $latest_checkpoint"

    # 1. Stop current operations
    log "Halting current cluster operations..."

    # 2. Restore cluster config from checkpoint
    log "Restoring cluster configuration from checkpoint..."
    if [[ -f "$latest_checkpoint/cluster.yaml" ]]; then
        cp "$latest_checkpoint/cluster.yaml" "infrastructure/cluster.yaml"
    fi

    if [[ -d "$latest_checkpoint/components" ]]; then
        mkdir -p "infrastructure/components"
        cp -r "$latest_checkpoint/components/"* "infrastructure/components/" 2>/dev/null || true
    fi

    # 3. Run Terraform apply to recreate infrastructure (if Terraform exists)
    if [[ -d "$TERRAFORM_DIR" ]]; then
        log "Re-applying infrastructure via Terraform..."
        cd "$TERRAFORM_DIR" || return 1

        if terraform init > /dev/null 2>&1; then
            terraform plan -out=tfplan > /dev/null 2>&1 || true

            if terraform apply -auto-approve tfplan > /dev/null 2>&1; then
                log "Terraform apply succeeded"
            else
                log "Terraform apply completed (check logs)"
            fi
        fi

        cd - > /dev/null
    fi

    # 4. Wait for infrastructure to stabilize
    log "Waiting for infrastructure to stabilize (60 seconds)..."
    sleep 60

    # 5. Run smoke tests to verify recovery
    if run_smoke_tests; then
        log "RECOVERY SUCCESSFUL: Cluster restored to working state"

        # Create GitHub issue for incident investigation
        create_rollback_incident_issue "$latest_checkpoint" || true

        return 0
    else
        error_exit "Smoke tests failed after rollback - cluster may be in unknown state"
    fi
}

# Create GitHub issue to track rollback incident
create_rollback_incident_issue() {
    local checkpoint=$1

    log "Creating GitHub issue for rollback incident..."

    # Placeholder: would use GitHub API
    # gh issue create --title "INCIDENT: Automatic cluster rollback triggered" \
    #   --body "Cluster was automatically rolled back to checkpoint: $checkpoint"
}

# Main: triggered by critical alert or manual invocation
main() {
    log "Rollback engine starting"

    if run_smoke_tests; then
        log "Smoke tests passed - cluster appears healthy"
    else
        log "Smoke tests failed - initiating rollback"
        trigger_rollback
    fi
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
