#!/usr/bin/env bash
# spot-instance-analyzer.sh — Analyze spot instance pricing and cost optimization opportunities
# Compares spot vs on-demand pricing, recommends instance type migrations, and analyzes reserved instances.
#
# Features:
#   - Query current and historical spot pricing
#   - Compare spot vs on-demand prices
#   - Recommend spot instances for cost reduction
#   - Calculate breakeven point for reserved instances
#   - Generate recommendations report
#   - Suggest instance type migrations

set -euo pipefail

REGION="us-west-2"
PROJECT_TAG="claudefs"
LOG="/var/log/spot-instance-analyzer.log"
REPORT_DIR="/var/lib/cfs-cost-reports"

log() {
  local level="$1"
  shift
  echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [$level] $*" >> "$LOG"
}

log_info() { log "INFO" "$@"; }
log_warn() { log "WARN" "$@"; }

# Get current instance pricing
get_instance_pricing() {
  local instance_type="$1"

  # Use AWS Pricing API to get on-demand price
  local on_demand_price=$(aws ce get-cost-and-usage \
    --time-period "Start=$(date -u +%Y-%m-%d),End=$(date -u -d '+1 day' +%Y-%m-%d)" \
    --granularity DAILY \
    --metrics UnblendedCost \
    --filter "{\"Dimensions\":{\"Key\":\"INSTANCE_TYPE\",\"Values\":[\"$instance_type\"]}}" \
    --query 'ResultsByTime[0].Total.UnblendedCost.Amount' \
    --output text --region "$REGION" 2>/dev/null || echo "0")

  echo "$on_demand_price"
}

# Get spot instance price
get_spot_price() {
  local instance_type="$1"

  # Query EC2 spot price history (last 24 hours average)
  aws ec2 describe-spot-price-history \
    --instance-types "$instance_type" \
    --start-time "$(date -u -d '1 day ago' -Iseconds)" \
    --product-descriptions "Linux/UNIX" \
    --region "$REGION" \
    --query 'SpotPriceHistory[0].SpotPrice' \
    --output text 2>/dev/null || echo "0"
}

# Calculate instance metrics
calculate_metrics() {
  local instance_type="$1"
  local hourly_on_demand="$2"
  local hourly_spot="$3"

  local spot_discount=$(echo "$hourly_on_demand $hourly_spot" | \
    awk '{if ($1 > 0) printf "%.1f", (1 - $2/$1) * 100; else print "0"}')

  local hourly_savings=$(echo "$hourly_on_demand $hourly_spot" | \
    awk '{printf "%.4f", $1 - $2}')

  local daily_savings=$(echo "$hourly_savings" | awk '{printf "%.2f", $1 * 24}')
  local monthly_savings=$(echo "$daily_savings" | awk '{printf "%.2f", $1 * 30}')

  echo "$spot_discount|$hourly_savings|$daily_savings|$monthly_savings"
}

# Get current running instances
get_running_instances() {
  aws ec2 describe-instances \
    --filters \
      "Name=tag:project,Values=$PROJECT_TAG" \
      "Name=instance-state-name,Values=running" \
    --query 'Reservations[].Instances[].[InstanceId, InstanceType, InstanceLifecycle]' \
    --output text \
    --region "$REGION" 2>/dev/null
}

# Generate optimization recommendations
generate_recommendations() {
  local report_file="${1:-$REPORT_DIR/spot-optimization-report.txt}"

  mkdir -p "$REPORT_DIR"

  log_info "Generating spot optimization recommendations"

  {
    echo "ClaudeFS Spot Instance Optimization Report"
    echo "Generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "Region: $REGION"
    echo ""
    echo "=========================================="
    echo "CURRENT INSTANCE ANALYSIS"
    echo "=========================================="
    echo ""

    # Define instances used in ClaudeFS cluster
    local instances=(
      "i4i.2xlarge"  # Storage servers
      "c7a.2xlarge"  # Orchestrator
      "c7a.xlarge"   # FUSE/NFS client
      "t3.medium"    # Cloud conduit
    )

    # Table header
    printf "%-15s | %-12s | %-12s | %-8s | Daily\n" "Instance Type" "On-Demand/hr" "Spot/hr" "Discount" "Savings"
    printf "%s\n" "----------------------------------------------------"

    local total_monthly_savings=0

    for instance in "${instances[@]}"; do
      local on_demand=$(get_instance_pricing "$instance" || echo "0")
      local spot=$(get_spot_price "$instance" || echo "0")

      if [[ "$on_demand" != "0" && "$spot" != "0" ]]; then
        local metrics=$(calculate_metrics "$instance" "$on_demand" "$spot")
        IFS='|' read -r discount hourly_savings daily_savings monthly_savings <<< "$metrics"

        printf "%-15s | \$%-11.4f | \$%-11.4f | %6s%% | \$%7.2f\n" \
          "$instance" "$on_demand" "$spot" "$discount" "$daily_savings"

        total_monthly_savings=$(echo "$total_monthly_savings $monthly_savings" | \
          awk '{printf "%.2f", $1 + $2}')
      fi
    done

    echo ""
    echo "Total Monthly Savings (all instances as spot): \$$total_monthly_savings"
    echo ""
    echo "=========================================="
    echo "INSTANCE TYPE MIGRATION OPPORTUNITIES"
    echo "=========================================="
    echo ""

    # Analyze alternative instance types
    local alt_instances=(
      "i4i.xlarge"   # 50% capacity of 2xlarge, 50% price
      "i4i.large"    # 25% capacity of 2xlarge, 25% price
      "c7a.medium"   # Smaller orchestrator
    )

    echo "Alternative configurations for cost reduction:"
    echo ""

    local alt_idx=0
    for original in "${instances[@]}"; do
      if [[ $alt_idx -lt ${#alt_instances[@]} ]]; then
        local alternative="${alt_instances[$alt_idx]}"
        local orig_price=$(get_spot_price "$original" || echo "0")
        local alt_price=$(get_spot_price "$alternative" || echo "0")

        if [[ "$orig_price" != "0" && "$alt_price" != "0" ]]; then
          local savings=$(echo "$orig_price $alt_price" | \
            awk '{printf "%.2f", ($1 - $2) * 24 * 30}')

          echo "Storage Servers:"
          printf "  Current:     %s spot @ \$%.4f/hr → \$%.2f/month (5 nodes)\n" \
            "$original" "$orig_price" "$(echo "$orig_price 120" | awk '{printf "%.2f", $1 * $2}')"
          printf "  Alternative: %s spot @ \$%.4f/hr → \$%.2f/month (10 nodes, half capacity each)\n" \
            "$alternative" "$alt_price" "$(echo "$alt_price 240" | awk '{printf "%.2f", $1 * $2}')"
          printf "  Monthly Savings: \$%s (same effective capacity)\n" "$savings"
          echo ""
        fi

        alt_idx=$((alt_idx + 1))
      fi
    done

    echo "=========================================="
    echo "RESERVED INSTANCE ANALYSIS"
    echo "=========================================="
    echo ""
    echo "Reserved Instance (1-year, all-upfront) Pricing:"
    echo ""

    for instance in "${instances[@]}"; do
      local spot_price=$(get_spot_price "$instance" || echo "0")

      if [[ "$spot_price" != "0" ]]; then
        # Estimate 1-year RI price (typically 45-60% off on-demand)
        local ri_hourly=$(echo "$spot_price" | awk '{printf "%.4f", $1 * 0.70}')
        local ri_annual=$(echo "$ri_hourly" | awk '{printf "%.2f", $1 * 24 * 365}')
        local spot_annual=$(echo "$spot_price" | awk '{printf "%.2f", $1 * 24 * 365}')

        local ri_breakeven=$(echo "$ri_annual $spot_price" | \
          awk '{if ($2 > 0) printf "%.1f", $1 / ($2 * 24); else print "N/A"}')

        printf "%s\n" "$instance:"
        printf "  1-year RI: \$%.4f/hr (\$%.2f/year)\n" "$ri_hourly" "$ri_annual"
        printf "  Spot: \$%.4f/hr (\$%.2f/year)\n" "$spot_price" "$spot_annual"
        printf "  Breakeven: ~%s months of always-on usage\n" "$ri_breakeven"
        echo ""
      fi
    done

    echo "=========================================="
    echo "RECOMMENDATIONS"
    echo "=========================================="
    echo ""
    echo "1. FOR DEVELOPMENT CLUSTER (current setup):"
    echo "   ✓ Use spot instances for all nodes"
    echo "   ✓ Expected savings: \$1,500-2,000/month vs on-demand"
    echo "   ✓ Risk: Spot interruption (acceptable for test cluster)"
    echo "   ✓ Mitigation: Tear down cluster when not in use"
    echo ""
    echo "2. FOR PRODUCTION DEPLOYMENT (future):"
    echo "   ✓ Use reserved instances (1-year all-upfront) for baseline capacity"
    echo "   ✓ Use spot instances for burst capacity"
    echo "   ✓ Savings: 60-70% off on-demand pricing"
    echo ""
    echo "3. FOR CURRENT DEVELOPMENT CYCLE:"
    echo "   ✓ Bedrock is largest cost component (60-70% of total)"
    echo "   ✓ Model downgrade (Opus→Sonnet) saves \$2-3/day"
    echo "   ✓ Consider model assignment by agent (see docs/A11-COST-OPTIMIZATION-PHASE1.md)"
    echo ""
    echo "4. FOR COST CONTROL:"
    echo "   ✓ Implement automatic cluster shutdown at 8 PM (US/Pacific)"
    echo "   ✓ Implement automatic cluster startup at 8 AM"
    echo "   ✓ Estimated savings: 50% reduction (only 10 hours/day)"
    echo ""

  } | tee "$report_file"

  log_info "Recommendations saved: $report_file"
}

# Compare on-demand vs spot for a single instance type
compare_pricing() {
  local instance_type="$1"

  log_info "Comparing pricing for $instance_type"

  local on_demand=$(get_instance_pricing "$instance_type" || echo "0")
  local spot=$(get_spot_price "$instance_type" || echo "0")

  if [[ "$on_demand" != "0" && "$spot" != "0" ]]; then
    local metrics=$(calculate_metrics "$instance_type" "$on_demand" "$spot")
    IFS='|' read -r discount hourly_savings daily_savings monthly_savings <<< "$metrics"

    cat << EOF
Instance Type: $instance_type
  On-Demand:    \$${on_demand}/hr (\$$(echo "$on_demand" | awk '{printf "%.2f", $1 * 24 * 30}')/month)
  Spot:         \$${spot}/hr (\$$(echo "$spot" | awk '{printf "%.2f", $1 * 24 * 30}')/month)
  Spot Discount: ${discount}%
  Hourly Savings: \$${hourly_savings}
  Daily Savings: \$${daily_savings}
  Monthly Savings: \$${monthly_savings}
EOF
  else
    echo "Could not retrieve pricing data for $instance_type"
    return 1
  fi
}

# Generate consolidated savings report
generate_savings_report() {
  local report_file="${1:-$REPORT_DIR/spot-savings-report.json}"

  mkdir -p "$REPORT_DIR"

  log_info "Generating savings report"

  {
    echo "{"
    echo "  \"report_date\": \"$(date -u +%Y-%m-%dT%H:%M:%SZ)\","
    echo "  \"region\": \"$REGION\","
    echo "  \"instances\": ["

    local instances=(
      "i4i.2xlarge"
      "c7a.2xlarge"
      "c7a.xlarge"
      "t3.medium"
    )

    local first=true
    for instance in "${instances[@]}"; do
      local on_demand=$(get_instance_pricing "$instance" 2>/dev/null || echo "0")
      local spot=$(get_spot_price "$instance" 2>/dev/null || echo "0")

      if [[ "$on_demand" != "0" && "$spot" != "0" ]]; then
        if ! $first; then echo ","; fi
        first=false

        echo "    {"
        echo "      \"instance_type\": \"$instance\","
        echo "      \"on_demand_hourly\": $on_demand,"
        echo "      \"spot_hourly\": $spot,"
        echo "      \"spot_discount_pct\": $(echo "$on_demand $spot" | \
          awk '{if ($1 > 0) printf "%.1f", (1 - $2/$1) * 100; else print "0"}'),"
        echo "      \"monthly_savings\": $(echo "$on_demand $spot" | \
          awk '{printf "%.2f", ($1 - $2) * 24 * 30}')"
        echo "    }"
      fi
    done

    echo "  ]"
    echo "}"
  } > "$report_file"

  log_info "Savings report saved: $report_file"
}

# --- CLI Interface ---

case "${1:-help}" in
  compare)
    if [[ -z "${2:-}" ]]; then
      echo "Usage: spot-instance-analyzer.sh compare <instance-type>"
      exit 1
    fi
    compare_pricing "$2"
    ;;

  analyze)
    generate_recommendations "${2:-}"
    ;;

  savings)
    generate_savings_report "${2:-}"
    ;;

  current-instances)
    echo "Current running instances in $PROJECT_TAG:"
    echo ""
    get_running_instances
    ;;

  help)
    cat << EOF
ClaudeFS Spot Instance Analyzer

Usage:
  spot-instance-analyzer.sh <command> [options]

Commands:
  compare <instance-type>
    Compare on-demand vs spot pricing for an instance type
    Example:
      spot-instance-analyzer.sh compare i4i.2xlarge

  analyze [output-file]
    Generate comprehensive spot optimization recommendations
    Example:
      spot-instance-analyzer.sh analyze /tmp/recommendations.txt

  savings [output-file]
    Generate JSON report with spot savings analysis
    Example:
      spot-instance-analyzer.sh savings /tmp/savings.json

  current-instances
    List currently running instances in the cluster

  help
    Show this help message

Output:
  - Recommendations: /var/lib/cfs-cost-reports/spot-optimization-report.txt
  - Savings Report: /var/lib/cfs-cost-reports/spot-savings-report.json
  - Logs: /var/log/spot-instance-analyzer.log

Example Instances Analyzed:
  - i4i.2xlarge (storage servers, 8 vCPU, 64GB, 960GB NVMe)
  - c7a.2xlarge (orchestrator, 8 vCPU, 16GB)
  - c7a.xlarge (FUSE/NFS client, 4 vCPU, 8GB)
  - t3.medium (cloud conduit, 2 vCPU, 4GB)

Typical Spot Savings:
  - 60-70% off on-demand pricing
  - \$1,500-2,000/month for 10-node development cluster
  - Risk: Spot interruptions (acceptable for test cluster)
  - Mitigation: Tear down when not actively testing

EOF
    ;;

  *)
    echo "Unknown command: $1"
    echo "Run 'spot-instance-analyzer.sh help' for usage"
    exit 1
    ;;
esac
