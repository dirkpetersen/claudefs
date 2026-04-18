#!/usr/bin/env bash
# cfs-cost-tagger.sh — AWS resource tagging for cost allocation and attribution
# Applies cost allocation tags to EC2 instances and other resources for tracking costs
# by deployment stage, agent, and test suite.
#
# Features:
#   - Standard tag structure for cost allocation
#   - Per-deployment stage tagging (canary, 10pct, 50pct, 100pct)
#   - Per-agent attribution (A1-A11)
#   - Per-test suite tracking (posix, jepsen, fio, chaos)
#   - Cost breakdown reports by tag
#   - Integration with AWS Cost Allocation Tags

set -euo pipefail

REGION="us-west-2"
PROJECT_TAG="claudefs"
LOG="/var/log/cfs-cost-tagger.log"

log() {
  local level="$1"
  shift
  echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [$level] $*" >> "$LOG"
}

log_info() { log "INFO" "$@"; }
log_warn() { log "WARN" "$@"; }
log_error() { log "ERROR" "$@"; }

# Apply tags to EC2 instances
tag_instances() {
  local instance_ids="$1"
  local stage="$2"
  local agent="${3:-unknown}"
  local test_suite="${4:-general}"

  if [[ -z "$instance_ids" ]]; then
    log_warn "No instances to tag for stage: $stage"
    return 1
  fi

  log_info "Tagging instances for stage: $stage | agent: $agent | test-suite: $test_suite"

  aws ec2 create-tags \
    --resources $instance_ids \
    --tags \
      "Key=project,Value=$PROJECT_TAG" \
      "Key=environment,Value=development" \
      "Key=deployment-stage,Value=$stage" \
      "Key=agent,Value=$agent" \
      "Key=test-suite,Value=$test_suite" \
      "Key=cost-center,Value=engineering" \
    --region "$REGION" 2>/dev/null || {
    log_error "Failed to tag instances: $instance_ids"
    return 1
  }

  log_info "Successfully tagged instances: $instance_ids"
  return 0
}

# Enable Cost Allocation Tags in AWS Billing
enable_cost_allocation_tags() {
  log_info "Ensuring cost allocation tags are enabled"

  # List of tags to enable for cost allocation
  local tags=("deployment-stage" "agent" "test-suite")

  for tag in "${tags[@]}"; do
    # Note: Enabling cost allocation tags requires IAM permissions
    # This is typically done once in the AWS console, but we document it here
    log_info "Cost allocation tag '$tag' should be enabled in AWS Billing console"
  done
}

# Get instances by deployment stage
get_instances_by_stage() {
  local stage="$1"

  aws ec2 describe-instances \
    --filters \
      "Name=tag:project,Values=$PROJECT_TAG" \
      "Name=tag:deployment-stage,Values=$stage" \
      "Name=instance-state-name,Values=running,stopped" \
    --query 'Reservations[].Instances[].InstanceId' \
    --output text \
    --region "$REGION" 2>/dev/null || echo ""
}

# Generate cost breakdown by tag
generate_cost_report_by_tag() {
  local tag_key="$1"
  local report_file="$2"

  log_info "Generating cost report by tag: $tag_key"

  # Query AWS Cost Explorer for costs grouped by tag
  local report=$(aws ce get-cost-and-usage \
    --time-period "Start=$(date -u -d '30 days ago' +%Y-%m-%d),End=$(date -u +%Y-%m-%d)" \
    --granularity DAILY \
    --metrics UnblendedCost \
    --group-by "Type=TAG,Key=$tag_key" \
    --query 'ResultsByTime[*].[TimePeriod.Start, Groups[*].[Keys[0], Metrics.UnblendedCost.Amount]]' \
    --output text \
    --region "$REGION" 2>/dev/null || echo "")

  if [[ -z "$report" ]]; then
    log_warn "No cost data available for tag: $tag_key"
    return 1
  fi

  # Format and save report
  {
    echo "Cost Breakdown by $tag_key"
    echo "Generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "Period: Last 30 days"
    echo ""
    echo "$report" | awk '
      {
        date = $1
        tag_value = $2
        cost = $3

        if (date != prev_date) {
          if (prev_date != "") print ""
          printf "Date: %s\n", date
          prev_date = date
        }
        printf "  %s: $%.2f\n", tag_value, cost
      }
    '
  } > "$report_file"

  log_info "Cost report saved: $report_file"
}

# Generate detailed cost report for each deployment stage
generate_stage_cost_report() {
  local report_dir="$1"

  mkdir -p "$report_dir"

  log_info "Generating per-stage cost reports"

  local stages=("canary" "stage-10pct" "stage-50pct" "stage-100pct")
  for stage in "${stages[@]}"; do
    local report_file="$report_dir/cost-by-stage-${stage}.txt"
    generate_cost_report_by_tag "deployment-stage" "$report_file" || true
  done
}

# Generate cost summary for a specific time period
generate_cost_summary() {
  local period_days="${1:-7}"
  local output_file="${2:-/var/lib/cfs-cost-reports/cost-summary.json}"

  log_info "Generating cost summary for last $period_days days"

  local start_date=$(date -u -d "$period_days days ago" +%Y-%m-%d)
  local end_date=$(date -u +%Y-%m-%d)

  # Get total cost
  local total_cost=$(aws ce get-cost-and-usage \
    --time-period "Start=$start_date,End=$end_date" \
    --granularity DAILY \
    --metrics UnblendedCost \
    --query 'sum(ResultsByTime[*].Total.UnblendedCost.Amount)' \
    --output text \
    --region "$REGION" 2>/dev/null || echo "0")

  # Get costs by service
  local by_service=$(aws ce get-cost-and-usage \
    --time-period "Start=$start_date,End=$end_date" \
    --granularity DAILY \
    --metrics UnblendedCost \
    --group-by Type=DIMENSION,Key=SERVICE \
    --query 'ResultsByTime[*].Groups[*].[Keys[0], Metrics.UnblendedCost.Amount]' \
    --output text \
    --region "$REGION" 2>/dev/null || echo "")

  # Get costs by tag (deployment stage)
  local by_stage=$(aws ce get-cost-and-usage \
    --time-period "Start=$start_date,End=$end_date" \
    --granularity DAILY \
    --metrics UnblendedCost \
    --group-by Type=TAG,Key=deployment-stage \
    --query 'ResultsByTime[*].Groups[*].[Keys[0], Metrics.UnblendedCost.Amount]' \
    --output text \
    --region "$REGION" 2>/dev/null || echo "")

  # Generate JSON report
  mkdir -p "$(dirname "$output_file")"
  cat > "$output_file" << EOF
{
  "period": {
    "start_date": "$start_date",
    "end_date": "$end_date",
    "days": $period_days
  },
  "summary": {
    "total_cost": $total_cost,
    "average_daily_cost": $(echo "$total_cost / $period_days" | bc -l 2>/dev/null | awk '{printf "%.2f", $1}' || echo "0"),
    "currency": "USD"
  },
  "by_service": $(echo "$by_service" | jq -Rs 'split("\n") | map(select(length > 0) | split("\t") | {service: .[0], cost: .[1]}) | map_values(tonumber?)' 2>/dev/null || echo "{}"),
  "by_stage": $(echo "$by_stage" | jq -Rs 'split("\n") | map(select(length > 0) | split("\t") | {stage: .[0], cost: .[1]}) | map_values(tonumber?)' 2>/dev/null || echo "{}")
}
EOF

  log_info "Cost summary saved: $output_file"
}

# List all instances with their cost allocation tags
list_tagged_instances() {
  local output_file="${1:-/var/lib/cfs-cost-reports/tagged-instances.txt}"

  log_info "Listing all tagged instances"

  {
    echo "ClaudeFS Tagged Instances"
    echo "Generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo ""
    echo "Instance ID          | Type          | Stage        | Agent | Test Suite"
    echo "--------------------+---------------+--------------+-------+----------"

    aws ec2 describe-instances \
      --filters "Name=tag:project,Values=$PROJECT_TAG" \
      --query 'Reservations[].Instances[].[InstanceId, InstanceType, Tags[?Key==`deployment-stage`].Value|[0], Tags[?Key==`agent`].Value|[0], Tags[?Key==`test-suite`].Value|[0]]' \
      --output text \
      --region "$REGION" 2>/dev/null | while read -r instance_id instance_type stage agent test_suite; do
      printf "%-20s | %-13s | %-12s | %-5s | %s\n" \
        "$instance_id" \
        "$instance_type" \
        "${stage:-N/A}" \
        "${agent:-N/A}" \
        "${test_suite:-N/A}"
    done
  } | tee "$output_file"

  log_info "Instance list saved: $output_file"
}

# --- CLI Interface ---

case "${1:-help}" in
  tag-stage)
    # Usage: cfs-cost-tagger.sh tag-stage <stage> <agent> [test-suite]
    if [[ -z "${2:-}" ]]; then
      log_error "Usage: cfs-cost-tagger.sh tag-stage <stage> <agent> [test-suite]"
      exit 1
    fi

    stage="$2"
    agent="${3:-unknown}"
    test_suite="${4:-general}"

    # Get instances for this stage
    instance_ids=$(get_instances_by_stage "$stage" || echo "")

    if [[ -z "$instance_ids" ]]; then
      log_warn "No instances found for stage: $stage"
      exit 0
    fi

    tag_instances "$instance_ids" "$stage" "$agent" "$test_suite"
    ;;

  enable-tags)
    enable_cost_allocation_tags
    ;;

  report-by-tag)
    # Usage: cfs-cost-tagger.sh report-by-tag <tag-key> [output-file]
    if [[ -z "${2:-}" ]]; then
      log_error "Usage: cfs-cost-tagger.sh report-by-tag <tag-key> [output-file]"
      exit 1
    fi

    tag_key="$2"
    output_file="${3:-/var/lib/cfs-cost-reports/cost-by-${tag_key}.txt}"
    generate_cost_report_by_tag "$tag_key" "$output_file"
    ;;

  report-stages)
    # Usage: cfs-cost-tagger.sh report-stages [report-dir]
    report_dir="${2:-/var/lib/cfs-cost-reports}"
    generate_stage_cost_report "$report_dir"
    ;;

  report-summary)
    # Usage: cfs-cost-tagger.sh report-summary [days] [output-file]
    days="${2:-7}"
    output_file="${3:-/var/lib/cfs-cost-reports/cost-summary.json}"
    generate_cost_summary "$days" "$output_file"
    ;;

  list-instances)
    # Usage: cfs-cost-tagger.sh list-instances [output-file]
    output_file="${2:-/var/lib/cfs-cost-reports/tagged-instances.txt}"
    list_tagged_instances "$output_file"
    ;;

  help)
    cat << EOF
ClaudeFS Cost Tagger — AWS resource tagging for cost allocation

Usage:
  cfs-cost-tagger.sh <command> [options]

Commands:
  tag-stage <stage> <agent> [test-suite]
    Tag instances for a specific deployment stage
    Examples:
      cfs-cost-tagger.sh tag-stage canary A11 posix
      cfs-cost-tagger.sh tag-stage stage-100pct A9 jepsen

  enable-tags
    Enable cost allocation tags in AWS Billing

  report-by-tag <tag-key> [output-file]
    Generate cost report by tag
    Examples:
      cfs-cost-tagger.sh report-by-tag deployment-stage
      cfs-cost-tagger.sh report-by-tag agent /tmp/cost-by-agent.txt

  report-stages [report-dir]
    Generate cost reports for each deployment stage

  report-summary [days] [output-file]
    Generate cost summary for a time period (default: 7 days)

  list-instances [output-file]
    List all tagged instances with their cost allocation tags

  help
    Show this help message

Environment:
  REGION              AWS region (default: us-west-2)
  PROJECT_TAG         Project tag for filtering (default: claudefs)

Cost Allocation Tags:
  project             Project name (always: claudefs)
  environment         Environment (always: development)
  deployment-stage    Stage: canary, stage-10pct, stage-50pct, stage-100pct
  agent               Agent: A1-A11
  test-suite          Test suite: posix, jepsen, fio, chaos, general
  cost-center         Cost center (always: engineering)

EOF
    ;;

  *)
    log_error "Unknown command: $1"
    echo "Run 'cfs-cost-tagger.sh help' for usage"
    exit 1
    ;;
esac
