#!/bin/bash
# =============================================================================
# ClaudeFS Cost Aggregator Script
# =============================================================================
# This script queries AWS Cost Explorer and outputs cost data in a format
# suitable for Prometheus textfile collection.
#
# Usage: Run daily via cron at 00:30 UTC (after billing finalization)
#   30 0 * * * /usr/local/bin/cfs-cost-aggregator.sh
#
# Output format:
#   - TSV file: /var/lib/cfs-metrics/cost.tsv
#   - Prometheus metrics: /var/lib/cfs-metrics/cost.prom
#
# Prerequisites:
#   - AWS CLI v2 installed
#   - IAM role with ce:GetCostAndUsage permission
#   - jq for JSON parsing
#
# For operators:
#   - Check logs: journalctl -u cfs-cost-aggregator -f
#   - Verify output: cat /var/lib/cfs-metrics/cost.prom
# =============================================================================

set -euo pipefail

# -----------------------------------------------------------------------------
# Configuration
# -----------------------------------------------------------------------------
# Output directory for metrics
METRICS_DIR="/var/lib/cfs-metrics"
TSV_OUTPUT="${METRICS_DIR}/cost.tsv"
PROM_OUTPUT="${METRICS_DIR}/cost.prom"
LOG_FILE="/var/log/cfs-cost-aggregator.log"

# AWS Cost Explorer configuration
# Time range: previous full day
START_DATE="$(date -d 'yesterday' +%Y-%m-%d)"
END_DATE="$(date +%Y-%m-%d)"
AWS_REGION="us-west-2"

# Cost thresholds for alerting
DAILY_LIMIT_100=100
DAILY_LIMIT_80=80
MONTHLY_LIMIT=3000

# -----------------------------------------------------------------------------
# Logging Functions
# -----------------------------------------------------------------------------
log_info() {
    local msg="$1"
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] INFO: ${msg}" | tee -a "${LOG_FILE}"
}

log_error() {
    local msg="$1"
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] ERROR: ${msg}" | tee -a "${LOG_FILE}" >&2
}

# -----------------------------------------------------------------------------
# Initialize Environment
# -----------------------------------------------------------------------------
init() {
    # Create metrics directory if it doesn't exist
    mkdir -p "${METRICS_DIR}"

    # Check AWS CLI availability
    if ! command -v aws &> /dev/null; then
        log_error "AWS CLI not found. Install aws cli v2."
        exit 1
    fi

    # Check jq availability
    if ! command -v jq &> /dev/null; then
        log_error "jq not found. Install jq for JSON parsing."
        exit 1
    fi
}

# -----------------------------------------------------------------------------
# Query AWS Cost Explorer
# -----------------------------------------------------------------------------
# Queries daily cost by service for the previous day
query_daily_cost() {
    log_info "Querying AWS Cost Explorer for ${START_DATE}"

    # Query Cost Explorer API for daily cost by service
    # Granularity: daily
    # Metrics: UnblendedCost (actual cost, not amortized)
    # Grouping: by Service
    local response
    response=$(aws ce get-cost-and-usage \
        --time-period Start="${START_DATE}",End="${END_DATE}" \
        --granularity DAILY \
        --metrics "UnblendedCost" \
        --group-by Type=DIMENSION,Key=SERVICE \
        --region "${AWS_REGION}" \
        2>&1) || {
        log_error "Failed to query Cost Explorer: ${response}"
        return 1
    }

    echo "${response}"
}

# -----------------------------------------------------------------------------
# Parse Cost Data
# -----------------------------------------------------------------------------
# Extract cost by service from Cost Explorer response
parse_cost_data() {
    local cost_json="$1"

    # Parse the JSON response and extract service costs
    # Returns tab-separated: timestamp service cost_usd
    echo "${cost_json}" | jq -r '
        .ResultsByTime[0] |
        .Groups[] |
        .Keys[0] as $service |
        .Metrics.UnblendedCost.Amount as $cost |
        "\(.) \($service) \($cost)"
    ' 2>/dev/null || {
        log_error "Failed to parse cost data"
        return 1
    }
}

# -----------------------------------------------------------------------------
# Calculate Cost Metrics
# -----------------------------------------------------------------------------
# Calculate total daily spend and monthly projection
calculate_metrics() {
    local cost_data="$1"

    # Calculate total daily cost
    local total_cost
    total_cost=$(echo "${cost_data}" | awk '{sum += $3} END {printf "%.2f", sum}')

    # Calculate monthly projection (current day of month * daily average)
    local day_of_month
    day_of_month=$(date +%-d)  # Day without leading zero
    local monthly_projection
    monthly_projection=$(echo "${total_cost} ${day_of_month}" | awk '{printf "%.2f", ($1 / $2) * 30}')

    # Calculate spot savings (if we can query reserved instance costs)
    # This is a placeholder - in production, compare on-demand vs spot
    local spot_savings=0.0

    echo "${total_cost}" > "${METRICS_DIR}/daily_total.txt"
    echo "${monthly_projection}" > "${METRICS_DIR}/monthly_projection.txt"

    log_info "Total daily cost: \$${total_cost}"
    log_info "Monthly projection: \$${monthly_projection}"
}

# -----------------------------------------------------------------------------
# Generate Prometheus Metrics
# -----------------------------------------------------------------------------
# Write textfile format metrics for Prometheus node_exporter
generate_prom_metrics() {
    local cost_data="$1"
    local timestamp
    timestamp=$(date +%s)

    # Calculate totals
    local total_cost
    total_cost=$(echo "${cost_data}" | awk '{sum += $3} END {printf "%.2f", sum}')
    local monthly_projection
    monthly_projection=$(cat "${METRICS_DIR}/monthly_projection.txt" 2>/dev/null || echo "0")

    # Write Prometheus textfile format
    cat > "${PROM_OUTPUT}" << EOF
# HELP aws_daily_spend_usd Daily AWS spending in USD
# TYPE aws_daily_spend_usd gauge
aws_daily_spend_usd ${total_cost}

# HELP aws_monthly_projection_usd Projected monthly AWS spending in USD
# TYPE aws_monthly_projection_usd gauge
aws_monthly_projection_usd ${monthly_projection}

# HELP aws_daily_limit_percent Current daily spend as percentage of $100 limit
# TYPE aws_daily_limit_percent gauge
aws_daily_limit_percent $(echo "${total_cost}" | awk '{printf "%.1f", ($1 / 100) * 100}')
EOF

    # Add per-service metrics
    echo "${cost_data}" | while read -r date service cost; do
        # Sanitize service name for metric label (replace spaces/special chars)
        local service_label
        service_label=$(echo "${service}" | tr ' ' '_' | tr '-' '_' | tr '[:upper:]' '[:lower:]')
        echo "# HELP aws_cost_by_service_usd AWS cost by service in USD" >> "${PROM_OUTPUT}"
        echo "# TYPE aws_cost_by_service_usd gauge" >> "${PROM_OUTPUT}"
        echo "aws_cost_by_service_usd{service=\"${service_label}\"} ${cost}" >> "${PROM_OUTPUT}"
    done

    log_info "Prometheus metrics written to ${PROM_OUTPUT}"
}

# -----------------------------------------------------------------------------
# Write TSV Output
# -----------------------------------------------------------------------------
# Write cost data in TSV format for external analysis
write_tsv() {
    local cost_data="$1"

    # Write TSV header and data
    {
        echo "timestamp	service	cost_usd"
        echo "${cost_data}" | while read -r date service cost; do
            echo "${date}	${service}	${cost}"
        done
    } > "${TSV_OUTPUT}"

    log_info "TSV output written to ${TSV_OUTPUT}"
}

# -----------------------------------------------------------------------------
# Check Thresholds and Alert
# -----------------------------------------------------------------------------
# Check if costs exceed thresholds and log warnings
check_thresholds() {
    local daily_total
    daily_total=$(cat "${METRICS_DIR}/daily_total.txt")

    # Check daily limit - 80% threshold
    local percent_80
    percent_80=$(echo "${daily_total} ${DAILY_LIMIT_80}" | awk '{printf "%.0f", ($1 / $2) * 100}')
    if [ "${percent_80}" -ge 80 ]; then
        log_error "Daily spend ${daily_total} exceeds 80% of \$${DAILY_LIMIT_80} limit (${percent_80}%)"
    fi

    # Check daily limit - 100% threshold
    local percent_100
    percent_100=$(echo "${daily_total} ${DAILY_LIMIT_100}" | awk '{printf "%.0f", ($1 / $2) * 100}')
    if [ "${percent_100}" -ge 100 ]; then
        log_error "CRITICAL: Daily spend ${daily_total} exceeded \$${DAILY_LIMIT_100} limit (${percent_100}%)"
    fi

    # Check monthly projection
    local monthly_projection
    monthly_projection=$(cat "${METRICS_DIR}/monthly_projection.txt")
    if (( $(echo "${monthly_projection} > ${MONTHLY_LIMIT}" | bc -l) )); then
        log_error "Monthly projection \$${monthly_projection} exceeds \$${MONTHLY_LIMIT} limit"
    fi
}

# -----------------------------------------------------------------------------
# Main Execution
# -----------------------------------------------------------------------------
main() {
    log_info "Starting ClaudeFS cost aggregation"

    init

    # Query and process cost data
    local cost_json
    cost_json=$(query_daily_cost) || exit 1

    local cost_data
    cost_data=$(parse_cost_data "${cost_json}") || exit 1

    # Generate outputs
    calculate_metrics "${cost_data}"
    generate_prom_metrics "${cost_data}"
    write_tsv "${cost_data}"
    check_thresholds

    # Ensure proper permissions for node_exporter textfile collector
    chmod 644 "${PROM_OUTPUT}"
    chmod 644 "${TSV_OUTPUT}"

    log_info "Cost aggregation completed successfully"
}

# Run main function
main "$@"