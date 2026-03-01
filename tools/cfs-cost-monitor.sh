#!/usr/bin/env bash
# cfs-cost-monitor.sh — Budget enforcement cron job for ClaudeFS dev cluster
# Runs every 15 minutes via cron on the orchestrator.
#
# Two separate budgets:
#   - EC2: $25/day — terminates spot instances if exceeded
#   - Bedrock: $25/day — switches all agents to Haiku if exceeded

set -euo pipefail

REGION="us-west-2"
EC2_DAILY_LIMIT=25
BEDROCK_DAILY_LIMIT=25
PROJECT_TAG="claudefs"
LOG="/var/log/cfs-cost-monitor.log"
HAIKU_FLAG="/tmp/cfs-bedrock-budget-exceeded"

log() { echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) $*" >> "$LOG"; }

TODAY=$(date -u +%Y-%m-%d)
TOMORROW=$(date -u -d '+1 day' +%Y-%m-%d)

# --- Get spend by service ---
get_service_spend() {
  local service_filter="$1"
  aws ce get-cost-and-usage \
    --time-period "Start=$TODAY,End=$TOMORROW" \
    --granularity DAILY \
    --metrics UnblendedCost \
    --filter "{\"Dimensions\":{\"Key\":\"SERVICE\",\"Values\":[\"$service_filter\"]}}" \
    --query 'ResultsByTime[0].Total.UnblendedCost.Amount' \
    --output text --region "$REGION" 2>/dev/null || echo "0"
}

# Get EC2 spend (compute only)
EC2_SPEND=$(get_service_spend "Amazon Elastic Compute Cloud - Compute")
[[ "$EC2_SPEND" == "None" || -z "$EC2_SPEND" ]] && EC2_SPEND="0"

# Get Bedrock spend (all Bedrock services)
BEDROCK_SPEND_1=$(get_service_spend "Amazon Bedrock")
BEDROCK_SPEND_2=$(get_service_spend "Claude Opus 4.6 (Amazon Bedrock Edition)")
BEDROCK_SPEND_3=$(get_service_spend "Claude Sonnet 4.6 (Amazon Bedrock Edition)")
BEDROCK_SPEND_4=$(get_service_spend "Claude Haiku 4.5 (Amazon Bedrock Edition)")
[[ "$BEDROCK_SPEND_1" == "None" ]] && BEDROCK_SPEND_1="0"
[[ "$BEDROCK_SPEND_2" == "None" ]] && BEDROCK_SPEND_2="0"
[[ "$BEDROCK_SPEND_3" == "None" ]] && BEDROCK_SPEND_3="0"
[[ "$BEDROCK_SPEND_4" == "None" ]] && BEDROCK_SPEND_4="0"
BEDROCK_SPEND=$(echo "$BEDROCK_SPEND_1 $BEDROCK_SPEND_2 $BEDROCK_SPEND_3 $BEDROCK_SPEND_4" | awk '{printf "%.2f", $1+$2+$3+$4}')

log "EC2: \$$EC2_SPEND / \$$EC2_DAILY_LIMIT | Bedrock: \$$BEDROCK_SPEND / \$$BEDROCK_DAILY_LIMIT"

to_cents() { echo "$1" | awk '{printf "%d", $1 * 100}'; }

EC2_CENTS=$(to_cents "$EC2_SPEND")
EC2_LIMIT_CENTS=$((EC2_DAILY_LIMIT * 100))
BEDROCK_CENTS=$(to_cents "$BEDROCK_SPEND")
BEDROCK_LIMIT_CENTS=$((BEDROCK_DAILY_LIMIT * 100))

# --- EC2 budget: terminate spot instances ---
if (( EC2_CENTS >= EC2_LIMIT_CENTS )); then
  log "EC2 BUDGET EXCEEDED (\$$EC2_SPEND) — terminating spot instances"

  INSTANCE_IDS=$(aws ec2 describe-instances \
    --filters \
      "Name=tag:project,Values=$PROJECT_TAG" \
      "Name=instance-lifecycle,Values=spot" \
      "Name=instance-state-name,Values=running" \
    --query 'Reservations[].Instances[].InstanceId' \
    --output text --region "$REGION" 2>/dev/null)

  if [[ -n "$INSTANCE_IDS" ]]; then
    log "Terminating: $INSTANCE_IDS"
    aws ec2 terminate-instances --instance-ids $INSTANCE_IDS --region "$REGION" >> "$LOG" 2>&1
  fi

  aws sns publish \
    --topic-arn "arn:aws:sns:$REGION:405644541454:cfs-budget-alerts" \
    --subject "ClaudeFS: EC2 budget exceeded (\$$EC2_SPEND/$EC2_DAILY_LIMIT)" \
    --message "EC2 spend \$$EC2_SPEND exceeds \$$EC2_DAILY_LIMIT limit. Spot instances terminated." \
    --region "$REGION" >> "$LOG" 2>&1 || true
fi

# --- Bedrock budget: switch to Haiku only ---
if (( BEDROCK_CENTS >= BEDROCK_LIMIT_CENTS )); then
  if [[ ! -f "$HAIKU_FLAG" ]]; then
    log "BEDROCK BUDGET EXCEEDED (\$$BEDROCK_SPEND) — switching all agents to Haiku"
    touch "$HAIKU_FLAG"

    # Kill all agent sessions so they restart with Haiku
    sudo -u cfs tmux kill-session -t cfs-a1 2>/dev/null || true
    sudo -u cfs tmux kill-session -t cfs-a2 2>/dev/null || true
    sudo -u cfs tmux kill-session -t cfs-a3 2>/dev/null || true
    sudo -u cfs tmux kill-session -t cfs-a4 2>/dev/null || true
    sudo -u cfs tmux kill-session -t cfs-a11 2>/dev/null || true
    # Watchdog will relaunch them — it reads the haiku flag

    aws sns publish \
      --topic-arn "arn:aws:sns:$REGION:405644541454:cfs-budget-alerts" \
      --subject "ClaudeFS: Bedrock budget exceeded — switching to Haiku" \
      --message "Bedrock spend \$$BEDROCK_SPEND exceeds \$$BEDROCK_DAILY_LIMIT limit. All agents switched to Haiku until tomorrow." \
      --region "$REGION" >> "$LOG" 2>&1 || true
  else
    log "Bedrock budget still exceeded, Haiku mode active"
  fi
else
  # Reset flag at start of new day if under budget
  if [[ -f "$HAIKU_FLAG" ]]; then
    local flag_date
    flag_date=$(date -r "$HAIKU_FLAG" +%Y-%m-%d 2>/dev/null || echo "")
    if [[ "$flag_date" != "$TODAY" ]]; then
      log "New day, resetting Haiku-only mode"
      rm -f "$HAIKU_FLAG"
    fi
  fi
fi

# --- Warnings at 80% ---
EC2_WARN=$((EC2_LIMIT_CENTS * 80 / 100))
BEDROCK_WARN=$((BEDROCK_LIMIT_CENTS * 80 / 100))
(( EC2_CENTS >= EC2_WARN && EC2_CENTS < EC2_LIMIT_CENTS )) && log "WARNING: EC2 at $(( EC2_CENTS * 100 / EC2_LIMIT_CENTS ))%"
(( BEDROCK_CENTS >= BEDROCK_WARN && BEDROCK_CENTS < BEDROCK_LIMIT_CENTS )) && log "WARNING: Bedrock at $(( BEDROCK_CENTS * 100 / BEDROCK_LIMIT_CENTS ))%"
