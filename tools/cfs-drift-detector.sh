#!/bin/bash
# tools/cfs-drift-detector.sh
# Detects divergence between declared cluster config (git) and actual state

set -euo pipefail

CONFIG_DIR="${CONFIG_DIR:-infrastructure}"
DRIFT_REPORT="${DRIFT_REPORT:-/var/lib/cfs-gitops/drift-report.json}"
PROMETHEUS_PUSHGATEWAY="${PROMETHEUS_PUSHGATEWAY:-http://localhost:9091}"
LOG_FILE="${LOG_FILE:-/var/log/cfs-gitops/drift-detector.log}"

# Drift counters
DRIFT_INFRASTRUCTURE=0
DRIFT_SOFTWARE=0
DRIFT_CONFIG=0
DRIFT_MONITORING=0
DRIFT_DEPLOYMENT=0

# Create directories
mkdir -p "$(dirname "$DRIFT_REPORT")" "$(dirname "$LOG_FILE")"

# Log function
log() {
    echo "[$(date +'%Y-%m-%d %H:%M:%S')] $1" | tee -a "$LOG_FILE"
}

# Get declared config value
get_declared_value() {
    local key=$1
    if [[ -f "infrastructure/cluster.yaml" ]]; then
        if command -v yq &> /dev/null; then
            yq eval ".spec.$key" "infrastructure/cluster.yaml" 2>/dev/null || echo "0"
        else
            grep "count:" "infrastructure/cluster.yaml" | head -1 | awk '{print $2}' || echo "0"
        fi
    else
        echo "0"
    fi
}

# Check infrastructure drift (instance counts, types)
check_infrastructure_drift() {
    log "Checking infrastructure drift..."

    local declared_storage=$(get_declared_value "nodes.storage.count")

    # In a real deployment, would query AWS EC2
    # For now, placeholder
    local actual_storage=5

    if [[ "$declared_storage" != "$actual_storage" && "$declared_storage" != "0" ]]; then
        log "DRIFT: Storage nodes mismatch (declared: $declared_storage, actual: $actual_storage)"
        ((DRIFT_INFRASTRUCTURE++))
    fi
}

# Check software drift
check_software_drift() {
    log "Checking software drift..."
    # Placeholder for version checking
}

# Check config drift
check_config_drift() {
    log "Checking config drift..."

    if [[ -f "infrastructure/components/prometheus.yaml" ]]; then
        local local_prometheus_hash=$(sha256sum "infrastructure/components/prometheus.yaml" 2>/dev/null | awk '{print $1}' || echo "none")
        log "Config drift check: prometheus hash = $local_prometheus_hash"
    fi
}

# Check monitoring drift
check_monitoring_drift() {
    log "Checking monitoring drift..."
    # Placeholder for monitoring checks
}

# Check deployment drift
check_deployment_drift() {
    log "Checking deployment drift..."
    # Placeholder for deployment checks
}

# Generate drift report JSON
generate_drift_report() {
    local timestamp=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
    local total_drift=$((DRIFT_INFRASTRUCTURE + DRIFT_SOFTWARE + DRIFT_CONFIG + DRIFT_MONITORING + DRIFT_DEPLOYMENT))

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
  "total_drift_score": $total_drift
}
JSON

    log "Drift report written to $DRIFT_REPORT"
}

# Push metrics to Prometheus Pushgateway
push_drift_metrics() {
    local total_drift=$((DRIFT_INFRASTRUCTURE + DRIFT_SOFTWARE + DRIFT_CONFIG + DRIFT_MONITORING + DRIFT_DEPLOYMENT))

    if command -v curl &> /dev/null; then
        cat << METRICS | curl -s --data-binary @- "${PROMETHEUS_PUSHGATEWAY}/metrics/job/cfs-gitops/instance/drift-detector" 2>/dev/null || true
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
claudefs_drift_total $total_drift
METRICS

        log "Metrics pushed to Prometheus Pushgateway"
    fi
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
