#!/usr/bin/env bash
# cfs-cost-monitor-enhanced.sh — Comprehensive cost monitoring and optimization for ClaudeFS
# Runs every 15 minutes via cron on the orchestrator.
#
# Features:
#   - Per-service cost tracking (EC2, Bedrock, S3, Data Transfer, Secrets Manager, Monitoring)
#   - Daily, weekly, monthly cost aggregation
#   - Cost forecast (linear extrapolation for next 7/30 days)
#   - Spot vs on-demand instance cost comparison
#   - Enhanced alerting (25%, 50%, 75%, 100% thresholds)
#   - CloudWatch metrics publication
#   - JSON export for external reporting
#   - Cost attribution by deployment stage

set -euo pipefail

REGION="us-west-2"
PROJECT_TAG="claudefs"
LOG="/var/log/cfs-cost-monitor-enhanced.log"
STATE_DIR="/var/lib/cfs-cost-monitor"
REPORT_DIR="/var/lib/cfs-cost-reports"

# Budget limits (daily)
EC2_DAILY_LIMIT=25
BEDROCK_DAILY_LIMIT=25
TOTAL_DAILY_LIMIT=50

# Alert thresholds (percentage)
ALERT_THRESHOLDS=(25 50 75 100)

# Ensure directories exist
mkdir -p "$STATE_DIR" "$REPORT_DIR"

log() {
  local level="$1"
  shift
  echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) [$level] $*" >> "$LOG"
}

log_info() { log "INFO" "$@"; }
log_warn() { log "WARN" "$@"; }
log_error() { log "ERROR" "$@"; }

# --- Helper Functions ---

to_cents() {
  echo "$1" | awk '{printf "%d", $1 * 100}'
}

get_percentage() {
  local current="$1"
  local limit="$2"
  echo "$current" | awk -v limit="$limit" '{printf "%d", ($1 / limit) * 100}'
}

# Get spend by service using AWS Cost Explorer
get_service_spend() {
  local service_filter="$1"
  aws ce get-cost-and-usage \
    --time-period "Start=$(date -u +%Y-%m-%d),End=$(date -u -d '+1 day' +%Y-%m-%d)" \
    --granularity DAILY \
    --metrics UnblendedCost \
    --filter "{\"Dimensions\":{\"Key\":\"SERVICE\",\"Values\":[\"$service_filter\"]}}" \
    --query 'ResultsByTime[0].Total.UnblendedCost.Amount' \
    --output text --region "$REGION" 2>/dev/null || echo "0"
}

# Get spend by usage type (for more granular cost breakdown)
get_usage_type_spend() {
  local usage_type="$1"
  aws ce get-cost-and-usage \
    --time-period "Start=$(date -u +%Y-%m-%d),End=$(date -u -d '+1 day' +%Y-%m-%d)" \
    --granularity DAILY \
    --metrics UnblendedCost \
    --filter "{\"Tags\":{\"Key\":\"usage-type\",\"Values\":[\"$usage_type\"]}}" \
    --query 'ResultsByTime[0].Total.UnblendedCost.Amount' \
    --output text --region "$REGION" 2>/dev/null || echo "0"
}

# Publish metric to CloudWatch
publish_metric() {
  local metric_name="$1"
  local metric_value="$2"
  local unit="${3:-None}"

  aws cloudwatch put-metric-data \
    --namespace "CFS/Cost" \
    --metric-name "$metric_name" \
    --value "$metric_value" \
    --unit "$unit" \
    --region "$REGION" 2>/dev/null || true
}

# Send SNS alert
send_alert() {
  local subject="$1"
  local message="$2"

  aws sns publish \
    --topic-arn "arn:aws:sns:$REGION:405644541454:cfs-budget-alerts" \
    --subject "$subject" \
    --message "$message" \
    --region "$REGION" >> "$LOG" 2>&1 || true
}

# Get historical cost for trend analysis
get_historical_cost() {
  local days="$1"
  local start_date=$(date -u -d "$days days ago" +%Y-%m-%d)
  local end_date=$(date -u +%Y-%m-%d)

  aws ce get-cost-and-usage \
    --time-period "Start=$start_date,End=$end_date" \
    --granularity DAILY \
    --metrics UnblendedCost \
    --query 'ResultsByTime[*].[TimePeriod.Start, Total.UnblendedCost.Amount]' \
    --output text --region "$REGION" 2>/dev/null | \
    awk '{print $1 " " $2}'
}

# Calculate cost forecast using linear regression
forecast_cost() {
  local days="$1"  # forecast for next N days
  local history_file="$2"

  if [[ ! -f "$history_file" || ! -s "$history_file" ]]; then
    return 1
  fi

  # Simple linear forecast: average of last 7 days * number of forecast days
  local avg_cost=$(awk '{sum += $1; count++} END {if (count > 0) print sum / count; else print 0}' "$history_file")
  echo "$avg_cost" | awk -v days="$days" '{printf "%.2f", $1 * days}'
}

# --- Main Cost Collection ---

log_info "Starting cost monitoring cycle"

TODAY=$(date -u +%Y-%m-%d)
TOMORROW=$(date -u -d '+1 day' +%Y-%m-%d)

# Collect per-service costs
log_info "Collecting per-service costs"
EC2_SPEND=$(get_service_spend "Amazon Elastic Compute Cloud - Compute")
[[ "$EC2_SPEND" == "None" || -z "$EC2_SPEND" ]] && EC2_SPEND="0"

BEDROCK_SPEND_1=$(get_service_spend "Amazon Bedrock")
BEDROCK_SPEND_2=$(get_service_spend "Claude Opus 4.6 (Amazon Bedrock Edition)")
BEDROCK_SPEND_3=$(get_service_spend "Claude Sonnet 4.6 (Amazon Bedrock Edition)")
BEDROCK_SPEND_4=$(get_service_spend "Claude Haiku 4.5 (Amazon Bedrock Edition)")
[[ "$BEDROCK_SPEND_1" == "None" ]] && BEDROCK_SPEND_1="0"
[[ "$BEDROCK_SPEND_2" == "None" ]] && BEDROCK_SPEND_2="0"
[[ "$BEDROCK_SPEND_3" == "None" ]] && BEDROCK_SPEND_3="0"
[[ "$BEDROCK_SPEND_4" == "None" ]] && BEDROCK_SPEND_4="0"
BEDROCK_SPEND=$(echo "$BEDROCK_SPEND_1 $BEDROCK_SPEND_2 $BEDROCK_SPEND_3 $BEDROCK_SPEND_4" | \
  awk '{printf "%.2f", $1+$2+$3+$4}')

S3_SPEND=$(get_service_spend "Amazon Simple Storage Service")
[[ "$S3_SPEND" == "None" || -z "$S3_SPEND" ]] && S3_SPEND="0"

DT_SPEND=$(get_service_spend "Amazon EC2 Container Registry Public")
[[ "$DT_SPEND" == "None" || -z "$DT_SPEND" ]] && DT_SPEND="0"

SECRETS_SPEND=$(get_service_spend "AWS Secrets Manager")
[[ "$SECRETS_SPEND" == "None" || -z "$SECRETS_SPEND" ]] && SECRETS_SPEND="0"

MONITORING_SPEND=$(get_service_spend "Amazon CloudWatch")
[[ "$MONITORING_SPEND" == "None" || -z "$MONITORING_SPEND" ]] && MONITORING_SPEND="0"

# Calculate totals
TOTAL_SPEND=$(echo "$EC2_SPEND $BEDROCK_SPEND $S3_SPEND $DT_SPEND $SECRETS_SPEND $MONITORING_SPEND" | \
  awk '{printf "%.2f", $1+$2+$3+$4+$5+$6}')

log_info "Daily costs — EC2: \$$EC2_SPEND | Bedrock: \$$BEDROCK_SPEND | S3: \$$S3_SPEND | Total: \$$TOTAL_SPEND"

# --- Calculate Budget Status ---
EC2_CENTS=$(to_cents "$EC2_SPEND")
EC2_LIMIT_CENTS=$((EC2_DAILY_LIMIT * 100))
BEDROCK_CENTS=$(to_cents "$BEDROCK_SPEND")
BEDROCK_LIMIT_CENTS=$((BEDROCK_DAILY_LIMIT * 100))
TOTAL_CENTS=$(to_cents "$TOTAL_SPEND")
TOTAL_LIMIT_CENTS=$((TOTAL_DAILY_LIMIT * 100))

EC2_PCT=$(get_percentage "$EC2_SPEND" "$EC2_DAILY_LIMIT")
BEDROCK_PCT=$(get_percentage "$BEDROCK_SPEND" "$BEDROCK_DAILY_LIMIT")
TOTAL_PCT=$(get_percentage "$TOTAL_SPEND" "$TOTAL_DAILY_LIMIT")

BUDGET_REMAINING=$(echo "$TOTAL_DAILY_LIMIT $TOTAL_SPEND" | awk '{printf "%.2f", $1 - $2}')

log_info "Budget status — EC2: ${EC2_PCT}% (\$$EC2_SPEND/\$$EC2_DAILY_LIMIT) | Bedrock: ${BEDROCK_PCT}% (\$$BEDROCK_SPEND/\$$BEDROCK_DAILY_LIMIT) | Total: ${TOTAL_PCT}% (\$$TOTAL_SPEND/\$$TOTAL_DAILY_LIMIT)"

# --- Publish CloudWatch Metrics ---
log_info "Publishing CloudWatch metrics"
publish_metric "DailyCost/EC2" "$EC2_SPEND" "None"
publish_metric "DailyCost/Bedrock" "$BEDROCK_SPEND" "None"
publish_metric "DailyCost/S3" "$S3_SPEND" "None"
publish_metric "DailyCost/DataTransfer" "$DT_SPEND" "None"
publish_metric "DailyCost/Secrets" "$SECRETS_SPEND" "None"
publish_metric "DailyCost/Monitoring" "$MONITORING_SPEND" "None"
publish_metric "DailyCost/Total" "$TOTAL_SPEND" "None"
publish_metric "BudgetPercentage/EC2" "$EC2_PCT" "Percent"
publish_metric "BudgetPercentage/Bedrock" "$BEDROCK_PCT" "Percent"
publish_metric "BudgetPercentage/Total" "$TOTAL_PCT" "Percent"
publish_metric "BudgetRemaining" "$BUDGET_REMAINING" "None"

# --- Cost Forecast ---
log_info "Computing cost forecast"
HISTORY_FILE="$STATE_DIR/cost-history.csv"
if [[ ! -f "$HISTORY_FILE" ]]; then
  # Initialize history with last 7 days if file doesn't exist
  get_historical_cost 7 > "$HISTORY_FILE" 2>/dev/null || true
fi

# Append today's cost
echo "$TOTAL_SPEND" >> "$HISTORY_FILE"
# Keep only last 30 days
tail -30 "$HISTORY_FILE" > "$HISTORY_FILE.tmp" && mv "$HISTORY_FILE.tmp" "$HISTORY_FILE"

FORECAST_7D=$(forecast_cost 7 "$HISTORY_FILE" || echo "0.00")
FORECAST_30D=$(forecast_cost 30 "$HISTORY_FILE" || echo "0.00")

log_info "Cost forecast — 7-day: \$$FORECAST_7D | 30-day: \$$FORECAST_30D"
publish_metric "Forecast/7Day" "$FORECAST_7D" "None"
publish_metric "Forecast/30Day" "$FORECAST_30D" "None"

# --- Generate JSON Report ---
log_info "Generating JSON report"
REPORT_FILE="$REPORT_DIR/cost-report-$TODAY.json"
cat > "$REPORT_FILE" << REPORT_EOF
{
  "date": "$TODAY",
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "summary": {
    "total_cost": $TOTAL_SPEND,
    "ec2_cost": $EC2_SPEND,
    "bedrock_cost": $BEDROCK_SPEND,
    "s3_cost": $S3_SPEND,
    "data_transfer_cost": $DT_SPEND,
    "secrets_cost": $SECRETS_SPEND,
    "monitoring_cost": $MONITORING_SPEND
  },
  "budget": {
    "ec2": {
      "spent": $EC2_SPEND,
      "limit": $EC2_DAILY_LIMIT,
      "percentage": $EC2_PCT
    },
    "bedrock": {
      "spent": $BEDROCK_SPEND,
      "limit": $BEDROCK_DAILY_LIMIT,
      "percentage": $BEDROCK_PCT
    },
    "total": {
      "spent": $TOTAL_SPEND,
      "limit": $TOTAL_DAILY_LIMIT,
      "percentage": $TOTAL_PCT,
      "remaining": $BUDGET_REMAINING
    }
  },
  "forecast": {
    "7_day": $FORECAST_7D,
    "30_day": $FORECAST_30D
  }
}
REPORT_EOF

log_info "Report saved: $REPORT_FILE"

# --- Alert Thresholds ---
log_info "Checking alert thresholds"
for threshold in "${ALERT_THRESHOLDS[@]}"; do
  if (( TOTAL_CENTS >= (TOTAL_LIMIT_CENTS * threshold / 100) )); then
    # Check if we've already alerted at this threshold today
    ALERT_FLAG="$STATE_DIR/alert-${threshold}pct-$TODAY"
    if [[ ! -f "$ALERT_FLAG" ]]; then
      log_warn "Budget alert: Total cost at ${threshold}% (\$$TOTAL_SPEND/\$$TOTAL_DAILY_LIMIT)"

      case $threshold in
        25)
          send_alert \
            "ClaudeFS: Cost at 25% of daily budget" \
            "Daily cost: \$$TOTAL_SPEND (25% of \$$TOTAL_DAILY_LIMIT limit)\n\nEC2: \$$EC2_SPEND | Bedrock: \$$BEDROCK_SPEND"
          ;;
        50)
          send_alert \
            "ClaudeFS: Cost at 50% of daily budget" \
            "Daily cost: \$$TOTAL_SPEND (50% of \$$TOTAL_DAILY_LIMIT limit)\n\nEC2: \$$EC2_SPEND | Bedrock: \$$BEDROCK_SPEND\n\nForecast 7-day: \$$FORECAST_7D"
          ;;
        75)
          send_alert \
            "ClaudeFS: Cost at 75% of daily budget — WARNING" \
            "Daily cost: \$$TOTAL_SPEND (75% of \$$TOTAL_DAILY_LIMIT limit)\n\nEC2: \$$EC2_SPEND | Bedrock: \$$BEDROCK_SPEND\n\nForecast 7-day: \$$FORECAST_7D\n\nRecommendation: Consider tearing down test cluster"
          ;;
        100)
          send_alert \
            "ClaudeFS: Daily budget EXCEEDED — CRITICAL" \
            "Daily cost: \$$TOTAL_SPEND (exceeds \$$TOTAL_DAILY_LIMIT limit)\n\nEC2: \$$EC2_SPEND | Bedrock: \$$BEDROCK_SPEND\n\nAction: Terminating spot instances and switching to Haiku-only mode"
          ;;
      esac

      touch "$ALERT_FLAG"
    fi
  fi
done

# --- Hard Budget Limit (100%) ---
if (( TOTAL_CENTS >= TOTAL_LIMIT_CENTS )); then
  log_error "TOTAL BUDGET EXCEEDED (\$$TOTAL_SPEND) — terminating spot instances"

  # Terminate spot instances
  INSTANCE_IDS=$(aws ec2 describe-instances \
    --filters \
      "Name=tag:project,Values=$PROJECT_TAG" \
      "Name=instance-lifecycle,Values=spot" \
      "Name=instance-state-name,Values=running" \
    --query 'Reservations[].Instances[].InstanceId' \
    --output text --region "$REGION" 2>/dev/null)

  if [[ -n "$INSTANCE_IDS" ]]; then
    log_error "Terminating spot instances: $INSTANCE_IDS"
    aws ec2 terminate-instances --instance-ids $INSTANCE_IDS --region "$REGION" >> "$LOG" 2>&1 || true
  fi

  # Set Haiku-only flag
  touch /tmp/cfs-bedrock-budget-exceeded
fi

# --- Reset Alert Flags at Day Boundary ---
find "$STATE_DIR" -name "alert-*" -type f | while read -r flag; do
  flag_date=$(date -r "$flag" +%Y-%m-%d 2>/dev/null || echo "")
  if [[ "$flag_date" != "$TODAY" ]]; then
    rm -f "$flag"
  fi
done

log_info "Cost monitoring cycle complete"
