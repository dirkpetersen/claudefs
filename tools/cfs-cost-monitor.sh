#!/usr/bin/env bash
# cfs-cost-monitor.sh — Budget enforcement cron job for ClaudeFS dev cluster
# Runs every 15 minutes via cron on the orchestrator.
# Terminates spot instances if daily spend exceeds $100.

set -euo pipefail

REGION="us-west-2"
DAILY_LIMIT=100
PROJECT_TAG="claudefs"
LOG="/var/log/cfs-cost-monitor.log"

log() { echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) $*" >> "$LOG"; }

# Get today's spend from Cost Explorer
TODAY=$(date -u +%Y-%m-%d)
SPEND=$(aws ce get-cost-and-usage \
  --time-period "Start=$TODAY,End=$(date -u -d '+1 day' +%Y-%m-%d)" \
  --granularity DAILY \
  --metrics UnblendedCost \
  --region "$REGION" \
  --query 'ResultsByTime[0].Total.UnblendedCost.Amount' \
  --output text 2>/dev/null || echo "0")

# Handle empty/null response
if [[ "$SPEND" == "None" || -z "$SPEND" ]]; then
  SPEND="0"
fi

log "Daily spend: \$$SPEND / \$$DAILY_LIMIT"

# Compare as integers (multiply by 100 for cent precision)
SPEND_CENTS=$(echo "$SPEND" | awk '{printf "%d", $1 * 100}')
LIMIT_CENTS=$((DAILY_LIMIT * 100))

if (( SPEND_CENTS >= LIMIT_CENTS )); then
  log "BUDGET EXCEEDED — terminating spot instances"

  # Find all cfs spot instances
  INSTANCE_IDS=$(aws ec2 describe-instances \
    --filters \
      "Name=tag:project,Values=$PROJECT_TAG" \
      "Name=instance-lifecycle,Values=spot" \
      "Name=instance-state-name,Values=running" \
    --query 'Reservations[].Instances[].InstanceId' \
    --output text \
    --region "$REGION" 2>/dev/null)

  if [[ -n "$INSTANCE_IDS" ]]; then
    log "Terminating: $INSTANCE_IDS"
    aws ec2 terminate-instances --instance-ids $INSTANCE_IDS --region "$REGION" >> "$LOG" 2>&1
    log "Spot instances terminated"
  else
    log "No spot instances to terminate"
  fi

  # Publish to SNS
  aws sns publish \
    --topic-arn "arn:aws:sns:$REGION:405644541454:cfs-budget-alerts" \
    --subject "ClaudeFS: Daily budget exceeded (\$$SPEND)" \
    --message "Daily spend \$$SPEND exceeds \$$DAILY_LIMIT limit. Spot instances terminated." \
    --region "$REGION" >> "$LOG" 2>&1 || true
else
  # Warn at 80%
  WARN_CENTS=$((LIMIT_CENTS * 80 / 100))
  if (( SPEND_CENTS >= WARN_CENTS )); then
    log "WARNING: spend at $(( SPEND_CENTS * 100 / LIMIT_CENTS ))% of daily limit"
  fi
fi
