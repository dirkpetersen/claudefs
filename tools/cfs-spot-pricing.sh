#!/usr/bin/env bash
# cfs-spot-pricing.sh — AWS Spot price querying and launch decision logic
#
# Commands:
#   query            - Query current spot prices
#   should-launch    - Determine if spot instance should be launched
#   cost-breakdown   - Aggregate daily cost data from cluster
#
set -euo pipefail

LOG="${LOG:-/var/log/cfs-spot-pricing.log}"
REGION="${REGION:-us-west-2}"
TIMEOUT=30

log() {
    echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) $*" | tee -a "$LOG" >&2
}

usage() {
    cat <<EOF
Usage: cfs-spot-pricing <command> [options]

Commands:
  query --instance-types <types> --region <region>
    Query AWS EC2 DescribeSpotPriceHistory API
    --instance-types: comma-separated list (e.g., "i4i.2xlarge,c7a.xlarge")
    --region: AWS region (default: us-west-2)

  should-launch --instance-type <type> --region <region>
    Decision logic for spot launch
    Returns: true, false, or maybe

  cost-breakdown --cluster <name> --date <YYYY-MM-DD>
    Aggregate instance cost data from /tmp/claudefs-cost/<cluster>-<date>.json

Examples:
  cfs-spot-pricing query --instance-types i4i.2xlarge --region us-west-2
  cfs-spot-pricing should-launch --instance-type i4i.2xlarge --region us-west-2
  cfs-spot-pricing cost-breakdown --cluster cfs-dev --date 2026-04-18
EOF
}

query_aws_api() {
    local instance_types="$1"
    local region="$2"

    local response
    response=$(aws ec2 describe-spot-price-history \
        --instance-types "$instance_types" \
        --region "$region" \
        --product-descriptions "Linux/UNIX" \
        --start-time "$(date -u -d '1 hour ago' +%Y-%m-%dT%H:%M:%SZ)" \
        --end-time "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
        --max-results 100 \
        --output json 2>&1) || {
        log "ERROR: AWS API call failed: $response"
        return 1
    }

    echo "$response"
}

parse_spot_response() {
    local response="$1"
    local instance_type="$2"

    echo "$response" | jq -r --arg it "$instance_type" \
        '.SpotPriceHistory | map(select(.InstanceType == $it)) | .[0] // empty' 2>/dev/null
}

calculate_discount() {
    local spot_price="$1"
    local on_demand_price="$2"

    if (( $(echo "$on_demand_price <= 0" | bc -l) )); then
        echo "0"
        return
    fi

    local discount
    discount=$(echo "scale=1; (($on_demand_price - $spot_price) / $on_demand_price) * 100" | bc -l)
    echo "$discount"
}

calculate_monthly_savings() {
    local spot_price="$1"
    local on_demand_price="$2"

    local savings
    savings=$(echo "scale=2; ($on_demand_price - $spot_price) * 730" | bc -l)
    echo "$savings"
}

get_on_demand_price() {
    local instance_type="$1"
    local region="$2"

    local od_price
    case "$instance_type" in
        i4i.2xlarge) od_price="0.624" ;;
        i4i.4xlarge) od_price="1.248" ;;
        i4i.8xlarge) od_price="2.496" ;;
        c7a.2xlarge) od_price="0.369" ;;
        c7a.4xlarge) od_price="0.738" ;;
        c7a.xlarge)  od_price="0.1845" ;;
        t3.medium)   od_price="0.0424" ;;
        t3.small)    od_price="0.0232" ;;
        m6i.2xlarge) od_price="0.40" ;;
        r6i.2xlarge) od_price="0.50" ;;
        *)           od_price="0.50" ;;
    esac
    echo "$od_price"
}

cmd_query() {
    local instance_types=""
    local region="$REGION"

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --instance-types)
                instance_types="$2"
                shift 2
                ;;
            --region)
                region="$2"
                shift 2
                ;;
            *)
                echo "Unknown option: $1" >&2
                usage
                exit 1
                ;;
        esac
    done

    if [[ -z "$instance_types" ]]; then
        echo "Error: --instance-types required" >&2
        exit 1
    fi

    local response
    response=$(query_aws_api "$instance_types" "$region") || exit 1

    local results="[]"
    IFS=',' read -ra ITYPES <<< "$instance_types"
    for it in "${ITypes[@]}"; do
        local spot_data
        spot_data=$(parse_spot_response "$response" "$it")

        if [[ -n "$spot_data" ]] && [[ "$spot_data" != "null" ]]; then
            local spot_price timestamp on_demand discount
            spot_price=$(echo "$spot_data" | jq -r '.SpotPrice // "0"')
            timestamp=$(echo "$spot_data" | jq -r '.Timestamp // empty')
            on_demand=$(get_on_demand_price "$it" "$region")
            discount=$(calculate_discount "$spot_price" "$on_demand")

            local entry
            entry=$(jq -n \
                --arg it "$it" \
                --arg sp "$spot_price" \
                --arg ts "$timestamp" \
                --arg od "$on_demand" \
                --arg dc "$discount" \
                '{
                    instance_type: $it,
                    current_spot_price: ($sp | tonumber),
                    timestamp: $ts,
                    on_demand_price: ($od | tonumber),
                    discount_pct: ($dc | tonumber)
                }')
            results=$(echo "$results" | jq ". + [$entry]")
        fi
    done

    echo "$results" | jq .
}

cmd_should_launch() {
    local instance_type=""
    local region="$REGION"

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --instance-type)
                instance_type="$2"
                shift 2
                ;;
            --region)
                region="$2"
                shift 2
                ;;
            *)
                echo "Unknown option: $1" >&2
                usage
                exit 1
                ;;
        esac
    done

    if [[ -z "$instance_type" ]]; then
        echo "Error: --instance-type required" >&2
        exit 1
    fi

    local response
    response=$(query_aws_api "$instance_type" "$region") || exit 1

    local spot_data
    spot_data=$(parse_spot_response "$response" "$instance_type")

    if [[ -z "$spot_data" ]] || [[ "$spot_data" == "null" ]]; then
        log "No spot price data available, defaulting to maybe"
        echo "maybe"
        return
    fi

    local spot_price on_demand discount interruption_rate
    spot_price=$(echo "$spot_data" | jq -r '.SpotPrice // "0"' | tr -d '\n')
    on_demand=$(get_on_demand_price "$instance_type" "$region")
    discount=$(calculate_discount "$spot_price" "$on_demand")

    local spot_ratio
    spot_ratio=$(echo "scale=2; $spot_price / $on_demand" | bc -l)

    interruption_rate=2

    log "Decision: spot=\$$spot_price, on_demand=\$$on_demand, ratio=$spot_ratio, discount=$discount%, interruption=$interruption_rate%"

    local decision
    if (( $(echo "$spot_ratio < 0.50" | bc -l) )) && (( interruption_rate < 5 )); then
        decision="true"
        log "Decision: true (good price, low interruption risk)"
    elif (( $(echo "$spot_ratio > 0.70" | bc -l) )) || (( interruption_rate > 10 )); then
        decision="false"
        log "Decision: false (price too high or high interruption risk)"
    else
        decision="maybe"
        log "Decision: maybe (price in acceptable range)"
    fi

    echo "$decision"
}

cmd_cost_breakdown() {
    local cluster=""
    local date=""

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --cluster)
                cluster="$2"
                shift 2
                ;;
            --date)
                date="$2"
                shift 2
                ;;
            *)
                echo "Unknown option: $1" >&2
                usage
                exit 1
                ;;
        esac
    done

    if [[ -z "$cluster" ]] || [[ -z "$date" ]]; then
        echo "Error: --cluster and --date required" >&2
        exit 1
    fi

    local cost_file="/tmp/claudefs-cost/${cluster}-${date}.json"

    if [[ ! -f "$cost_file" ]]; then
        echo "[]" | jq '{error: "Cost file not found", path: $path}' --arg path "$cost_file"
        return
    fi

    local instances
    instances=$(jq -c '.[]' "$cost_file" 2>/dev/null || echo "[]")

    local total_cost=0
    local on_demand_equivalent=0
    local disruption_count=0
    local detailed_instances="[]"

    while IFS= read -r instance; do
        [[ -z "$instance" ]] && continue

        local instance_id instance_type pricing_model hourly_rate uptime_hours
        instance_id=$(echo "$instance" | jq -r '.instance_id // "unknown"')
        instance_type=$(echo "$instance" | jq -r '.instance_type // "unknown"')
        pricing_model=$(echo "$instance" | jq -r '.pricing_model // "spot"')
        hourly_rate=$(echo "$instance" | jq -r '.hourly_rate // "0"')
        uptime_hours=$(echo "$instance" | jq -r '.uptime_hours // "0"')

        local cost disruption_events
        cost=$(echo "scale=2; $hourly_rate * $uptime_hours" | bc -l)
        disruption_events=$(echo "$instance" | jq -r '.disruption_events // "0"')

        total_cost=$(echo "scale=2; $total_cost + $cost" | bc -l)
        disruption_count=$((disruption_count + disruption_events))

        if [[ "$pricing_model" == "on-demand" ]]; then
            on_demand_equivalent=$(echo "scale=2; $on_demand_equivalent + $cost" | bc -l)
        else
            local od_rate
            od_rate=$(get_on_demand_price "$instance_type" "$REGION")
            local od_cost
            od_cost=$(echo "scale=2; $od_rate * $uptime_hours" | bc -l)
            on_demand_equivalent=$(echo "scale=2; $on_demand_equivalent + $od_cost" | bc -l)
        fi

        local detailed
        detailed=$(jq -n \
            --arg id "$instance_id" \
            --arg it "$instance_type" \
            --arg pm "$pricing_model" \
            --arg hr "$hourly_rate" \
            --arg uh "$uptime_hours" \
            --arg c "$cost" \
            '{
                instance_id: $id,
                instance_type: $it,
                pricing_model: $pm,
                hourly_rate: ($hr | tonumber),
                uptime_hours: ($uh | tonumber),
                cost: ($c | tonumber)
            }')
        detailed_instances=$(echo "$detailed_instances" | jq ". + [$detailed]")

    done <<< "$instances"

    local savings_pct=0
    if (( $(echo "$on_demand_equivalent > 0" | bc -l) )); then
        savings_pct=$(echo "scale=1; (($on_demand_equivalent - $total_cost) / $on_demand_equivalent) * 100" | bc -l)
    fi

    jq -n \
        --arg cluster "$cluster" \
        --arg date "$date" \
        --arg tc "$total_cost" \
        --arg ode "$on_demand_equivalent" \
        --arg sp "$savings_pct" \
        --arg dc "$disruption_count" \
        --argjson di "$detailed_instances" \
        '{
            cluster: $cluster,
            date: $date,
            total_cost: ($tc | tonumber),
            on_demand_equivalent: ($ode | tonumber),
            savings_pct: ($sp | tonumber),
            disruption_events: ($dc | tonumber),
            instances: $di
        }'
}

main() {
    local command="${1:-}"
    shift 2>/dev/null || true

    case "$command" in
        query)
            cmd_query "$@"
            ;;
        should-launch)
            cmd_should_launch "$@"
            ;;
        cost-breakdown)
            cmd_cost_breakdown "$@"
            ;;
        -h|--help|help)
            usage
            ;;
        *)
            echo "Error: Unknown command '$command'" >&2
            usage
            exit 1
            ;;
    esac
}

main "$@"