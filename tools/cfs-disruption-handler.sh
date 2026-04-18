#!/usr/bin/env bash
# cfs-disruption-handler.sh — Spot instance interruption detection and graceful shutdown
#
# Runs as a daemon to detect EC2 spot interruption notices via IMDS
# and coordinate graceful shutdown of ClaudeFS services.
#
set -euo pipefail

LOG_FILE="${LOG_FILE:-/var/log/cfs-disruption-handler.log}"
IMDS_ENDPOINT="http://169.254.169.254/latest/meta-data/spot/instance-action"
IMDS_TOKEN_ENDPOINT="http://169.254.169.254/latest/api/token"
POLL_INTERVAL=5
DRAIN_TIMEOUT=115
BACKOFF_TIMES=(5 10 20)
MAX_RETRIES=3

log() {
    echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) $*" | tee -a "$LOG_FILE"
}

log_error() {
    echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) ERROR: $*" | tee -a "$LOG_FILE" >&2
}

init_logging() {
    mkdir -p "$(dirname "$LOG_FILE")"
    touch "$LOG_FILE"
    log "Disruption handler started, PID=$$"
}

get_imds_token() {
    local token
    token=$(curl -s -X PUT -H "X-aws-ec2-metadata-token-ttl-seconds: 300" \
        "$IMDS_TOKEN_ENDPOINT" 2>/dev/null) || return 1

    if [[ -n "$token" ]] && [[ "$token" != "<html>"* ]]; then
        echo "$token"
        return 0
    fi
    return 1
}

get_node_id() {
    local token
    token=$(get_imds_token) || {
        log_error "Failed to get IMDS token"
        return 1
    }

    local instance_id
    instance_id=$(curl -s -H "X-aws-ec2-metadata-token: $token" \
        "http://169.254.169.254/latest/meta-data/instance-id" 2>/dev/null) || {
        log_error "Failed to get instance ID"
        return 1
    }

    echo "$instance_id"
}

check_termination_notice() {
    local token
    token=$(get_imds_token) || return 1

    local response
    response=$(curl -s -w "%{http_code}" -o /dev/null \
        -H "X-aws-ec2-metadata-token: $token" \
        "$IMDS_ENDPOINT" 2>/dev/null) || {
        return 1
    }

    if [[ "$response" == "200" ]]; then
        return 0
    fi

    return 1
}

get_termination_action() {
    local token
    token=$(get_imds_token) || return 1

    local action
    action=$(curl -s -H "X-aws-ec2-metadata-token: $token" \
        "$IMDS_ENDPOINT" 2>/dev/null) || return 1

    echo "$action"
}

handle_termination() {
    local start_time
    start_time=$(date +%s%3N)

    log "Spot interruption notice detected"

    local action
    action=$(get_termination_action)
    log "Termination action: $action"

    local node_id
    node_id=$(get_node_id) || node_id="unknown"
    log "Node ID: $node_id"

    log "Initiating graceful drain (timeout: ${DRAIN_TIMEOUT}s)"

    local drain_status drain_output
    if command -v cfs-instance-manager &>/dev/null; then
        drain_output=$(cfs-instance-manager drain --node-id "$node_id" --timeout "$DRAIN_TIMEOUT" 2>&1) || {
            log_error "Drain command failed: $drain_output"
        }
        drain_status="$?"
    else
        log_error "cfs-instance-manager not found, skipping drain"
        drain_status=1
    fi

    local elapsed_ms
    elapsed_ms=$(($(date +%s%3N) - start_time))

    if [[ "$drain_status" -eq 0 ]]; then
        log "Graceful drain completed in ${elapsed_ms}ms"
    else
        log_error "Drain failed or timed out after ${elapsed_count}ms"
    fi

    local operations_completed=0
    local drain_result
    if [[ -n "$drain_output" ]]; then
        operations_completed=$(echo "$drain_output" | jq -r '.operations_completed // 0')
    fi

    log "Shutdown sequence: $operations_completed operations completed"

    log "Sending final checkpoint to orchestrator"
    if command -v cfs &>/dev/null; then
        cfs cluster checkpoint --node "$node_id" 2>&1 | tee -a "$LOG_FILE" || true
    fi

    log "Stopping ClaudeFS services"
    systemctl stop cfs-fuse 2>/dev/null || true
    systemctl stop cfs-storage 2>/dev/null || true
    systemctl stop cfs-meta 2>/dev/null || true

    log "Graceful shutdown complete, exiting (AWS will terminate at scheduled time)"
    log "Final stats: elapsed_ms=$elapsed_ms, operations=$operations_completed"

    exit 0
}

run_with_backoff() {
    local attempt=0
    local success=0

    while (( attempt < MAX_RETRIES )); do
        if "$@"; then
            success=1
            break
        fi

        local backoff
        if (( attempt < ${#BACKOFF_TIMES[@]} )); then
            backoff="${BACKOFF_TIMES[$attempt]}"
        else
            backoff="${BACKOFF_TIMES[-1]}"
        fi

        log "Retry $((attempt + 1))/$MAX_RETRIES after ${backoff}s backoff"
        sleep "$backoff"
        ((attempt++))
    done

    return $((1 - success))
}

main() {
    init_logging

    local node_id
    node_id=$(get_node_id) || {
        log_error "Failed to get node ID, exiting"
        exit 1
    }
    log "Running on node: $node_id"

    log "Starting interruption monitoring (poll interval: ${POLL_INTERVAL}s)"

    local check_count=0

    while true; do
        ((check_count++))

        if check_termination_notice; then
            log "Termination notice detected on check #$check_count"
            handle_termination
            exit 0
        fi

        sleep "$POLL_INTERVAL"
    done
}

main "$@"